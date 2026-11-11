//! Four-loop pressurized water reactor (~3411 MWt / ~1150 MWe class).
//!
//! Model summary (see `docs/PHYSICS.md`):
//! * point kinetics (6 delayed groups), Doppler / moderator / boron / rod /
//!   xenon / samarium reactivity, 9-group decay heat;
//! * Westinghouse-style nuclear instrumentation: source range (counts/s,
//!   high voltage removed when blocked above P-6), intermediate range
//!   (compensated ion chamber current, blockable above P-10) and four power
//!   range channels;
//! * lumped fuel heat capacity with fuel→coolant conductance;
//! * primary loop as four mixing volumes (core, hot leg, SG tubes, cold leg)
//!   with pump coastdown and natural circulation;
//! * pressurizer with insurge compression, heaters, spray, PORV and safeties;
//! * lumped saturated SG secondary, steam dumps, relief valves, feedwater;
//! * turbine-generator shaft dynamics and grid synchronisation.

use crate::plant::{AutomationState, PlantEvent};
use nr_api::{
    authorize, modules, Actuator, Alarm, BorationMode, Command, CommandKind, ControlRoomGeneration,
    Denied, GameMode, ModuleId, ModuleMode, ModuleView, NisBlock, NisView, Origin, PolicyContext,
    RodDirection, RodView, Severity, Telemetry, API_VERSION,
};
use nr_control::controllers::{self as ctl, overlap_sequence};
use nr_control::{Rps, RpsInputs};
use nr_core::{clamp, lerp, steam, Rng, MPA};
use nr_physics::turbine_generator::RATED_RPM;
use nr_physics::{
    CloseOutcome, DecayHeat, DelayedNeutronData, PointKinetics, Poisons, RodBank, Sensor,
    SensorFault, TurbineGenerator,
};
use std::collections::BTreeMap;

/// Design parameters.
#[derive(Debug, Clone)]
pub struct PwrParams {
    pub thermal_power: f64,
    pub gross_mwe: f64,
    pub generator_rating: f64,
    pub flow_nominal: f64,
    pub cp_primary: f64,
    pub tavg_full: f64,
    pub tavg_noload: f64,
    pub steam_temp_full: f64,
    pub pressure_nominal: f64,
    pub vol_core: f64,
    pub vol_hot: f64,
    pub vol_sg: f64,
    pub vol_cold: f64,
    pub vol_pzr: f64,
    pub fuel_heat_capacity: f64,
    pub fuel_dt_full: f64,
    pub doppler: f64,
    pub moderator: f64,
    pub boron_worth: f64,
    pub excess_reactivity: f64,
    pub heater_power: f64,
    pub pzr_loss: f64,
    pub pzr_capacitance: f64,
    pub pzr_compression: f64,
    pub spray_max: f64,
    pub letdown_nominal: f64,
    pub charging_max: f64,
    pub sg_mass_nominal: f64,
    pub sg_capacitance: f64,
    pub feed_temp_full: f64,
    pub feed_max_frac: f64,
    pub turbine_valve_full: f64,
    pub dump_capacity: f64,
    pub house_load_mwe: f64,
    pub rod_speed_max: f64,
    pub rod_speed_manual: f64,
    pub rod_max_steps: f64,
    pub rod_overlap: f64,
    pub scram_time: f64,
    /// Source range counts/s per unit relative flux.
    pub sr_cps_per_n: f64,
    /// Intermediate range amps per unit relative flux.
    pub ir_amps_per_n: f64,
    /// Intermediate range residual (gamma / leakage) current.
    pub ir_floor_amps: f64,
}

impl Default for PwrParams {
    fn default() -> Self {
        Self {
            thermal_power: 3411.0e6,
            gross_mwe: 1150.0,
            generator_rating: 1300.0e6,
            flow_nominal: 17_700.0,
            cp_primary: 5_500.0,
            tavg_full: 308.6,
            tavg_noload: 291.7,
            steam_temp_full: 285.8,
            pressure_nominal: 15.51 * MPA,
            vol_core: 25.0,
            vol_hot: 40.0,
            vol_sg: 110.0,
            vol_cold: 135.0,
            vol_pzr: 51.0,
            fuel_heat_capacity: 3.2e7,
            fuel_dt_full: 320.0,
            doppler: -3.0e-5,
            moderator: -2.0e-4,
            boron_worth: -1.0e-4,
            excess_reactivity: 0.13,
            heater_power: 1.8e6,
            pzr_loss: 0.15e6,
            pzr_capacitance: 320.0,
            pzr_compression: 0.15,
            spray_max: 40.0,
            letdown_nominal: 7.5,
            charging_max: 20.0,
            sg_mass_nominal: 180_000.0,
            sg_capacitance: 2.5e9,
            feed_temp_full: 226.0,
            feed_max_frac: 1.3,
            turbine_valve_full: 0.85,
            dump_capacity: 0.4,
            house_load_mwe: 45.0,
            rod_speed_max: 72.0 / 60.0,
            rod_speed_manual: 48.0 / 60.0,
            rod_max_steps: 228.0,
            rod_overlap: 100.0,
            scram_time: 2.2,
            sr_cps_per_n: 5.0e10,
            ir_amps_per_n: 8.0e-4,
            ir_floor_amps: 1.0e-11,
        }
    }
}

const PZR_PORV_OPEN: f64 = 16.2 * MPA;
const PZR_PORV_CLOSE: f64 = 15.9 * MPA;
const PZR_SAFETY: f64 = 17.2 * MPA;
const SG_ARV: f64 = 7.85 * MPA;
const SG_SAFETY: f64 = 8.3 * MPA;
/// P-6 intermediate range permissive [A].
const P6_AMPS: f64 = 1.0e-10;
/// P-10 power range permissive [%].
const P10_PCT: f64 = 10.0;
/// Physics step [s].
pub const DT: f64 = 0.05;

/// Nuclear instrumentation trip blocks.
#[derive(Debug, Clone, Default)]
pub struct NisState {
    pub sr_blocked: bool,
    pub ir_blocked: bool,
    pub pr_low_blocked: bool,
}

/// Complete PWR plant state.
#[derive(Debug, Clone)]
pub struct Pwr {
    pub p: PwrParams,
    pub name: String,
    pub scenario: String,
    pub time: f64,
    pub training_mode: bool,
    pub control_room: ControlRoomGeneration,
    rng: Rng,

    // Neutronics
    pub kinetics: PointKinetics,
    pub poisons: Poisons,
    pub decay: DecayHeat,
    pub banks: Vec<RodBank>,
    pub boron_ppm: f64,
    pub boration: BorationMode,
    pub rod_selection: String,
    pub rod_direction: RodDirection,
    pub nis: NisState,

    // Primary
    pub t_fuel: f64,
    pub t_core: f64,
    pub t_hot: f64,
    pub t_sg: f64,
    pub t_cold: f64,
    pub rcp_running: [bool; 4],
    pub rcp_speed: [f64; 4],
    pub rcs_mass: f64,
    pub pressure: f64,
    pzr_water_vol: f64,
    pub heaters: f64,
    pub spray: f64,
    pub charging: f64,
    pub letdown: f64,
    pub porv_open: bool,

    // Secondary
    pub t_steam: f64,
    pub sg_mass: f64,
    pub feed: f64,
    pub t_feed: f64,
    pub turbine_valve: f64,
    pub steam_dump: f64,
    pub turbine_tripped: bool,
    pub tg: TurbineGenerator,
    pub sync_block_until: f64,
    prev_breaker: bool,

    // Protection
    rps: Rps,
    pub reactor_tripped: bool,
    pub trips: Vec<String>,

    // Automation
    pub automation: AutomationState,
    rod_ctl: ctl::RodControl,
    pzr_p_ctl: ctl::PressureControl,
    pzr_l_ctl: ctl::LevelControl,
    fw_ctl: ctl::FeedwaterControl,
    dump_ctl: ctl::SteamDumpControl,
    gov: ctl::TurbineGovernor,
    boron_ctl: ctl::BoronControl,
    avr: ctl::Avr,

    // Instruments
    pub sensors: BTreeMap<&'static str, Sensor>,
    readings: BTreeMap<&'static str, f64>,
    pub events: Vec<PlantEvent>,

    // Derived quantities cached each step (truth).
    pub rho: Reactivity,
    pub flow_frac: f64,
    pub q_core: f64,
    pub q_sg: f64,
    pub steam_flow: f64,
    pub feed_flow: f64,
    pub gross_mwe: f64,
}

/// Breakdown of core reactivity (Δk/k).
#[derive(Debug, Clone, Default)]
pub struct Reactivity {
    pub rods: f64,
    pub boron: f64,
    pub doppler: f64,
    pub moderator: f64,
    pub xenon: f64,
    pub samarium: f64,
    pub total: f64,
}

impl Pwr {
    /// Plant at equilibrium 100 % power with the main control loops in AUTO.
    pub fn full_power() -> Self {
        let p = PwrParams::default();
        let q = p.thermal_power;
        let w = p.flow_nominal;
        let dt_core = q / (w * p.cp_primary);
        let t_cold = p.tavg_full - dt_core / 2.0;
        let t_hot = p.tavg_full + dt_core / 2.0;
        let t_fuel = p.tavg_full + p.fuel_dt_full;
        let ps = steam::psat(p.steam_temp_full);
        let t_feed = p.feed_temp_full;
        let w_steam = q / (steam::hg(ps) - steam::h_liquid(t_feed));
        let feed = 1.0 / p.feed_max_frac;

        let banks = vec![
            RodBank::new("SA", p.rod_max_steps, 0.030, p.rod_max_steps, true),
            RodBank::new("SB", p.rod_max_steps, 0.030, p.rod_max_steps, true),
            RodBank::new("A", p.rod_max_steps, 0.010, p.rod_max_steps, false),
            RodBank::new("B", p.rod_max_steps, 0.012, p.rod_max_steps, false),
            RodBank::new("C", p.rod_max_steps, 0.011, p.rod_max_steps, false),
            RodBank::new("D", p.rod_max_steps, 0.009, 200.0, false),
        ];

        let mut plant = Self {
            name: "Unit 1".into(),
            scenario: "full_power".into(),
            time: 0.0,
            training_mode: false,
            control_room: ControlRoomGeneration::Analog,
            rng: Rng::new(0x5EED),
            kinetics: PointKinetics::equilibrium(DelayedNeutronData::u235_thermal(), 1.0),
            poisons: Poisons::equilibrium(3.0e13, -0.028, -0.007, 1.0),
            decay: DecayHeat::equilibrium(1.0),
            banks,
            boron_ppm: 0.0,
            boration: BorationMode::Off,
            rod_selection: "manual".into(),
            rod_direction: RodDirection::Hold,
            nis: NisState {
                sr_blocked: true,
                ir_blocked: true,
                pr_low_blocked: true,
            },
            t_fuel,
            t_core: t_hot,
            t_hot,
            t_sg: t_cold,
            t_cold,
            rcp_running: [true; 4],
            rcp_speed: [1.0; 4],
            rcs_mass: 0.0,
            pressure: p.pressure_nominal,
            pzr_water_vol: 0.0,
            heaters: 0.0,
            spray: 0.0,
            charging: p.letdown_nominal,
            letdown: p.letdown_nominal,
            porv_open: false,
            t_steam: p.steam_temp_full,
            sg_mass: p.sg_mass_nominal,
            feed,
            t_feed,
            turbine_valve: p.turbine_valve_full,
            steam_dump: 0.0,
            turbine_tripped: false,
            tg: TurbineGenerator::synced(p.generator_rating),
            sync_block_until: -1.0,
            prev_breaker: true,
            rps: Rps::default(),
            reactor_tripped: false,
            trips: Vec::new(),
            automation: AutomationState::new(),
            rod_ctl: ctl::RodControl::default(),
            pzr_p_ctl: ctl::PressureControl::default(),
            pzr_l_ctl: ctl::LevelControl::new(p.charging_max),
            fw_ctl: ctl::FeedwaterControl::default(),
            dump_ctl: ctl::SteamDumpControl::default(),
            gov: ctl::TurbineGovernor::default(),
            boron_ctl: ctl::BoronControl::default(),
            avr: ctl::Avr::default(),
            sensors: BTreeMap::new(),
            readings: BTreeMap::new(),
            events: Vec::new(),
            rho: Reactivity::default(),
            flow_frac: 1.0,
            q_core: q,
            q_sg: q,
            steam_flow: w_steam,
            feed_flow: w_steam,
            gross_mwe: p.gross_mwe,
            p,
        };

        // Pressurizer inventory at programmed level.
        let level = plant.pzr_level_program(plant.p.tavg_full) / 100.0;
        plant.pzr_water_vol = level * plant.p.vol_pzr;
        plant.rcs_mass = plant.loop_mass() + plant.pzr_water_vol * steam::rho_f(plant.pressure);
        plant.heaters = plant.p.pzr_loss / plant.p.heater_power;

        // Critical boron concentration.
        plant.update_reactivity();
        plant.boron_ppm = plant.rho.total / -plant.p.boron_worth;
        plant.update_reactivity();

        plant.gross_mwe = plant.p.gross_mwe;
        plant.gov.track(plant.turbine_valve, plant.gross_mwe);
        plant.gov.speed_ref = RATED_RPM;
        plant
            .automation
            .setpoint_set(ModuleId::TurbineGovernor, "target_mwe", plant.p.gross_mwe);
        plant
            .automation
            .setpoint_set(ModuleId::PlantMaster, "target_mwe", plant.p.gross_mwe);
        plant.rod_ctl.reset(1.0);
        plant.pzr_p_ctl.track(plant.heaters);
        plant.pzr_l_ctl.track(plant.charging, plant.letdown);
        plant.fw_ctl.track(plant.feed, plant.feed);
        plant.avr.track(plant.tg.excitation);

        for m in [
            ModuleId::RodControl,
            ModuleId::PressurizerPressure,
            ModuleId::PressurizerLevel,
            ModuleId::FeedwaterLevel,
            ModuleId::SteamDump,
            ModuleId::TurbineGovernor,
            ModuleId::Excitation,
        ] {
            plant.automation.modes.insert(m, ModuleMode::Auto);
        }
        plant.init_sensors();
        plant
    }

    /// Hot standby a week after shutdown: no-load temperature and pressure,
    /// all rods inserted, xenon decayed, turbine on turning gear. The
    /// operator must take the reactor critical on the source range and
    /// work up through the intermediate and power ranges.
    pub fn hot_standby() -> Self {
        let mut plant = Self::full_power();
        plant.scenario = "hot_standby".into();
        let week = 7.0 * 86_400.0;
        plant.decay.step(0.0, week);
        for _ in 0..(week / 600.0) as usize {
            plant.poisons.step(0.0, 600.0);
        }
        let tnl = plant.p.tavg_noload;
        plant.t_fuel = tnl;
        plant.t_core = tnl;
        plant.t_hot = tnl;
        plant.t_sg = tnl;
        plant.t_cold = tnl;
        plant.t_steam = tnl;
        plant.t_feed = 40.0;

        // Boron for an estimated critical position of C=160 / D=60 steps.
        for (b, pos) in plant
            .banks
            .iter_mut()
            .zip([228.0, 228.0, 228.0, 228.0, 160.0, 60.0])
        {
            b.position = pos;
        }
        plant.boron_ppm = 0.0;
        plant.update_reactivity();
        plant.boron_ppm = plant.rho.total / -plant.p.boron_worth;
        for b in &mut plant.banks {
            b.position = 0.0;
            b.target = 0.0;
        }
        plant.update_reactivity();
        let data = DelayedNeutronData::u235_thermal();
        let source = plant.kinetics.source;
        let n = source * data.generation_time / -plant.rho.total;
        plant.kinetics = PointKinetics::equilibrium(data, n);
        plant.kinetics.source = source;

        let level = plant.pzr_level_program(tnl) / 100.0;
        plant.pzr_water_vol = level * plant.p.vol_pzr;
        plant.rcs_mass = plant.loop_mass() + plant.pzr_water_vol * steam::rho_f(plant.pressure);

        plant.feed = 0.0;
        plant.turbine_valve = 0.0;
        plant.turbine_tripped = true;
        plant.tg.breaker = false;
        plant.prev_breaker = false;
        plant.tg.field_breaker = false;
        plant.tg.excitation = 0.0;
        plant.tg.speed = 3.0 / RATED_RPM; // turning gear
        plant.gross_mwe = 0.0;
        plant.steam_flow = 0.0;
        plant.feed_flow = 0.0;
        plant.q_core = plant.thermal_fraction() * plant.p.thermal_power;
        plant.q_sg = plant.q_core;
        plant.nis = NisState::default();
        plant.gov.track_speed(0.0, plant.tg.rpm());
        plant.gov.track(0.0, 0.0);
        plant.avr.track(0.0);
        plant
            .automation
            .setpoint_set(ModuleId::TurbineGovernor, "target_mwe", 0.0);
        plant
            .automation
            .setpoint_set(ModuleId::TurbineGovernor, "speed_rpm", 0.0);
        plant
            .automation
            .setpoint_set(ModuleId::PlantMaster, "target_mwe", 0.0);
        plant.fw_ctl.track(0.0, 0.0);
        plant
            .automation
            .modes
            .insert(ModuleId::RodControl, ModuleMode::Manual);
        plant.init_sensors();
        plant
    }

    pub fn from_scenario(name: &str) -> Option<Self> {
        match name {
            "full_power" => Some(Self::full_power()),
            "hot_standby" => Some(Self::hot_standby()),
            _ => None,
        }
    }

    fn turbine_cv(&self) -> f64 {
        let ps = steam::psat(self.p.steam_temp_full);
        let w = self.p.thermal_power / (steam::hg(ps) - steam::h_liquid(self.p.feed_temp_full));
        w / (self.p.turbine_valve_full * ps)
    }

    fn steam_nominal(&self) -> f64 {
        self.turbine_cv() * self.p.turbine_valve_full * steam::psat(self.p.steam_temp_full)
    }

    /// Steam-to-shaft efficiency, calibrated so full power gives rated
    /// gross output after windage and friction losses.
    fn shaft_efficiency(&self) -> f64 {
        let mut tg = self.tg.clone();
        tg.speed = 1.0;
        (self.p.gross_mwe * 1e6 + tg.losses()) / self.p.thermal_power
    }

    fn loop_mass(&self) -> f64 {
        let p = &self.p;
        steam::rho_primary(self.t_core) * p.vol_core
            + steam::rho_primary(self.t_hot) * p.vol_hot
            + steam::rho_primary(self.t_sg) * p.vol_sg
            + steam::rho_primary(self.t_cold) * p.vol_cold
    }

    pub fn tavg(&self) -> f64 {
        0.5 * (self.t_hot + self.t_cold)
    }

    /// Turbine load fraction used for the Tavg program.
    fn turbine_load(&self) -> f64 {
        clamp(self.gross_mwe / self.p.gross_mwe, 0.0, 1.2)
    }

    pub fn tref(&self) -> f64 {
        let sp = |k: &str, d: f64| {
            self.automation
                .setpoint(ModuleId::RodControl, k)
                .unwrap_or(d)
        };
        let full = sp("tavg_full_c", self.p.tavg_full);
        let noload = sp("tavg_noload_c", self.p.tavg_noload);
        lerp(noload, full, self.turbine_load())
    }

    fn pzr_level_program(&self, tavg: f64) -> f64 {
        let sp = |k: &str, d: f64| {
            self.automation
                .setpoint(ModuleId::PressurizerLevel, k)
                .unwrap_or(d)
        };
        let x = clamp(
            (tavg - self.p.tavg_noload) / (self.p.tavg_full - self.p.tavg_noload),
            0.0,
            1.0,
        );
        lerp(sp("level_noload_pct", 25.0), sp("level_full_pct", 55.0), x)
    }

    pub fn pzr_level(&self) -> f64 {
        100.0 * self.pzr_water_vol / self.p.vol_pzr
    }

    pub fn sg_level(&self) -> f64 {
        50.0 + (self.sg_mass - self.p.sg_mass_nominal) / (0.25 * self.p.sg_mass_nominal) * 50.0
    }

    pub fn steam_pressure(&self) -> f64 {
        steam::psat(self.t_steam)
    }

    /// Thermal power fraction (fission + decay heat).
    pub fn thermal_fraction(&self) -> f64 {
        self.kinetics.n * (1.0 - DecayHeat::delayed_fraction()) + self.decay.power()
    }

    pub fn sync_check_active(&self) -> bool {
        self.control_room == ControlRoomGeneration::Digital
            || self.automation.mode(ModuleId::GeneratorSync) == ModuleMode::Auto
    }

    fn update_reactivity(&mut self) {
        let p = &self.p;
        let tc_core = 0.5 * (self.t_cold + self.t_core);
        let r = &mut self.rho;
        r.rods = self.banks.iter().map(|b| b.reactivity()).sum();
        r.boron = p.boron_worth * self.boron_ppm;
        r.doppler = p.doppler * (self.t_fuel - p.tavg_noload);
        r.moderator = p.moderator * (tc_core - p.tavg_noload);
        r.xenon = self.poisons.xenon_reactivity();
        r.samarium = self.poisons.samarium_reactivity();
        r.total =
            p.excess_reactivity + r.rods + r.boron + r.doppler + r.moderator + r.xenon + r.samarium;
    }

    // ---------------------------------------------------------------- step

    /// Advance the plant by one physics step.
    pub fn step(&mut self) {
        let dt = DT;
        self.time += dt;
        self.read_instruments(dt);
        self.update_nis_permissives();
        self.run_automation(dt);
        self.run_protection();
        self.step_rods(dt);
        self.step_neutronics(dt);
        self.step_primary(dt);
        self.step_secondary(dt);
    }

    /// Advance by `seconds` of simulated time.
    pub fn advance(&mut self, seconds: f64) {
        let n = (seconds / DT).round() as usize;
        for _ in 0..n {
            self.step();
        }
    }

    fn step_rods(&mut self, dt: f64) {
        if self.reactor_tripped {
            for b in &mut self.banks {
                b.drop(self.p.scram_time, dt);
            }
            return;
        }
        // Manual rod control lever.
        if self.automation.mode(ModuleId::RodControl) == ModuleMode::Manual
            && self.rod_direction != RodDirection::Hold
        {
            let sign = if self.rod_direction == RodDirection::Out {
                1.0
            } else {
                -1.0
            };
            let speed = sign * self.p.rod_speed_manual;
            if self.rod_selection.eq_ignore_ascii_case("manual") {
                let pos: Vec<f64> = self.banks[2..].iter().map(|b| b.position).collect();
                let d = overlap_sequence(&pos, self.p.rod_max_steps, self.p.rod_overlap, speed, dt);
                for (b, d) in self.banks[2..].iter_mut().zip(d) {
                    b.target = clamp(b.position + d, 0.0, b.max_steps);
                }
            } else if let Some(b) = self
                .banks
                .iter_mut()
                .find(|b| b.name.eq_ignore_ascii_case(&self.rod_selection))
            {
                b.target = clamp(b.position + speed * dt, 0.0, b.max_steps);
            }
        }
        let speed = self.p.rod_speed_max;
        for b in &mut self.banks {
            b.drive(speed, dt);
        }
    }

    fn step_neutronics(&mut self, dt: f64) {
        // Boration / dilution through CVCS makeup (5 kg/s blender flow).
        let makeup = 5.0;
        let rate = match self.boration {
            BorationMode::Off => 0.0,
            BorationMode::Borate => makeup / self.rcs_mass * (7000.0 - self.boron_ppm),
            BorationMode::Dilute => makeup / self.rcs_mass * (0.0 - self.boron_ppm),
        };
        self.boron_ppm = (self.boron_ppm + rate * dt).max(0.0);
        self.update_reactivity();
        self.kinetics.step(self.rho.total, dt);
        self.poisons.step(self.kinetics.n, dt);
        self.decay.step(self.kinetics.n, dt);
    }

    fn step_primary(&mut self, dt: f64) {
        let p = self.p.clone();
        // Pump speeds: spin-up 5 s, coastdown ~15 s.
        for i in 0..4 {
            let tau = if self.rcp_running[i] { 5.0 } else { 15.0 };
            let target = if self.rcp_running[i] { 1.0 } else { 0.0 };
            self.rcp_speed[i] += (target - self.rcp_speed[i]) * (dt / tau).min(1.0);
        }
        let forced: f64 = self.rcp_speed.iter().sum::<f64>() / 4.0;
        let q_frac = self.thermal_fraction().max(0.0);
        let natural = 0.05 * q_frac.cbrt() + 0.002;
        self.flow_frac = forced.max(natural);
        let w = p.flow_nominal * self.flow_frac;
        let cp = p.cp_primary;

        // Fuel.
        self.q_core = q_frac * p.thermal_power;
        let r_fc = p.fuel_dt_full / p.thermal_power;
        let tc_core = 0.5 * (self.t_cold + self.t_core);
        let q_fc = (self.t_fuel - tc_core) / r_fc;
        self.t_fuel += (self.q_core - q_fc) / p.fuel_heat_capacity * dt;

        // Steam generator heat transfer.
        let ts = self.t_steam;
        let ua_full = p.thermal_power / (p.tavg_full - p.steam_temp_full);
        let dryout = clamp(self.sg_mass / (0.3 * p.sg_mass_nominal), 0.0, 1.0);
        let ua = ua_full * (0.25 + 0.75 * self.flow_frac.powf(0.8)) * dryout;
        let t_sg_avg = 0.5 * (self.t_hot + self.t_sg);
        self.q_sg = ua * (t_sg_avg - ts);

        // Loop nodes (mixing volumes, upwind transport).
        let node = |t: f64, vol: f64| steam::rho_primary(t) * vol * cp;
        let c_core = node(self.t_core, p.vol_core);
        let c_hot = node(self.t_hot, p.vol_hot);
        let c_sg = node(self.t_sg, p.vol_sg);
        let c_cold = node(self.t_cold, p.vol_cold);
        let t_charge = 265.0;
        let d_core = (w * cp * (self.t_cold - self.t_core) + q_fc) / c_core;
        let d_hot = w * cp * (self.t_core - self.t_hot) / c_hot;
        let d_sg = (w * cp * (self.t_hot - self.t_sg) - self.q_sg) / c_sg;
        let d_cold = (w * cp * (self.t_sg - self.t_cold)
            + self.charging * cp * (t_charge - self.t_cold))
            / c_cold;
        self.t_core += d_core * dt;
        self.t_hot += d_hot * dt;
        self.t_sg += d_sg * dt;
        self.t_cold += d_cold * dt;

        // Inventory: charging in, letdown out, PORV/safety steam out.
        let mut relief = 0.0;
        if self.pressure > PZR_PORV_OPEN {
            self.porv_open = true;
        } else if self.pressure < PZR_PORV_CLOSE {
            self.porv_open = false;
        }
        if self.porv_open {
            relief += 25.0;
        }
        if self.pressure > PZR_SAFETY {
            relief += 120.0;
        }
        self.rcs_mass += (self.charging - self.letdown - relief) * dt;

        // Pressurizer water volume from inventory balance.
        let rho_f = steam::rho_f(self.pressure);
        let new_vol = clamp((self.rcs_mass - self.loop_mass()) / rho_f, 0.0, p.vol_pzr);
        let insurge = new_vol - self.pzr_water_vol;
        self.pzr_water_vol = new_vol;
        let v_steam = (p.vol_pzr - new_vol).max(1.0);

        // Pressure: insurge compression + net heat input.
        let spray_flow = p.spray_max * self.spray * forced;
        let q_spray = spray_flow * (steam::hf(self.pressure) - steam::h_liquid(self.t_cold));
        let q_net = self.heaters * p.heater_power - p.pzr_loss - q_spray;
        let mut dp =
            p.pzr_compression * self.pressure * insurge / v_steam + q_net / p.pzr_capacitance * dt;
        dp -= relief * 2_000.0 * dt; // steam relief depressurises
        self.pressure = clamp(self.pressure + dp, 0.2 * MPA, 19.0 * MPA);
    }

    fn step_secondary(&mut self, dt: f64) {
        let p = self.p.clone();
        let ps = self.steam_pressure();
        let cv = self.turbine_cv();
        let w_nom = self.steam_nominal();

        let w_turb = if self.turbine_tripped {
            0.0
        } else {
            self.turbine_valve * cv * ps
        };
        let w_dump =
            self.steam_dump * p.dump_capacity * w_nom * ps / steam::psat(p.steam_temp_full);
        let w_arv = if ps > SG_ARV {
            (ps - SG_ARV) / (0.2 * MPA) * 0.1 * w_nom
        } else {
            0.0
        };
        let w_safety = if ps > SG_SAFETY {
            (ps - SG_SAFETY) / (0.1 * MPA) * 0.5 * w_nom
        } else {
            0.0
        };
        self.steam_flow = w_turb + w_dump + w_arv + w_safety;

        // Feedwater heated by turbine extraction steam.
        let load = clamp(w_turb / w_nom, 0.0, 1.0);
        let t_feed_target = lerp(40.0, p.feed_temp_full, load);
        self.t_feed += (t_feed_target - self.t_feed) * (dt / 60.0);
        let h_fw = steam::h_liquid(self.t_feed);
        self.feed_flow = self.feed * p.feed_max_frac * w_nom;

        let hf = steam::hf(ps);
        let hfg = steam::hg(ps) - hf;
        let q_out = self.feed_flow * (hf - h_fw) + self.steam_flow * hfg;
        self.t_steam += (self.q_sg - q_out) / p.sg_capacitance * dt;
        self.sg_mass = (self.sg_mass + (self.feed_flow - self.steam_flow) * dt).max(0.0);

        // Turbine-generator.
        let p_mech = w_turb * (steam::hg(ps) - h_fw) * self.shaft_efficiency();
        if let Some(event) = self.tg.step(p_mech, dt, &mut self.rng) {
            match event {
                "Turbine overspeed" => {
                    self.events.push(PlantEvent::trip("Turbine overspeed trip"));
                    self.trip_turbine();
                }
                other => self.events.push(PlantEvent::warn(&format!(
                    "{other}: generator breaker opened"
                ))),
            }
        }
        self.gross_mwe = if self.tg.breaker {
            (self.tg.net_power / 1e6).max(0.0)
        } else {
            0.0
        };
    }

    // ----------------------------------------------------------- instruments

    fn true_values(&self) -> BTreeMap<&'static str, f64> {
        let mut m = BTreeMap::new();
        let n = self.kinetics.n;
        let dt_core = self.t_hot - self.t_cold;
        let dt_full = self.p.thermal_power / (self.p.flow_nominal * self.p.cp_primary);
        let sur = 26.06 / self.kinetics.period;
        let sr = if self.nis.sr_blocked {
            0.0
        } else {
            self.p.sr_cps_per_n * n
        };
        m.insert("sr_cps", sr);
        m.insert("sr_sur_dpm", if self.nis.sr_blocked { 0.0 } else { sur });
        m.insert("ir_amps", self.p.ir_amps_per_n * n + self.p.ir_floor_amps);
        m.insert("ir_sur_dpm", sur);
        for tag in ["pr_n41_pct", "pr_n42_pct", "pr_n43_pct", "pr_n44_pct"] {
            m.insert(tag, n * 100.0);
        }
        m.insert("nuclear_power_pct", n * 100.0);
        m.insert(
            "thermal_power_mw",
            self.thermal_fraction() * self.p.thermal_power / 1e6,
        );
        m.insert("startup_rate_dpm", sur);
        m.insert("tavg_c", self.tavg());
        m.insert("tref_c", self.tref());
        m.insert("thot_c", self.t_hot);
        m.insert("tcold_c", self.t_cold);
        m.insert("delta_t_pct", dt_core / dt_full * 100.0);
        m.insert("rcs_pressure_mpa", self.pressure / MPA);
        m.insert("pzr_level_pct", self.pzr_level());
        m.insert("pzr_level_program_pct", self.pzr_level_program(self.tavg()));
        m.insert("rcs_flow_pct", self.flow_frac * 100.0);
        m.insert("boron_ppm", self.boron_ppm);
        m.insert("pzr_heaters_pct", self.heaters * 100.0);
        m.insert("pzr_spray_pct", self.spray * 100.0);
        m.insert("charging_kg_s", self.charging);
        m.insert("letdown_kg_s", self.letdown);
        m.insert("sg_pressure_mpa", self.steam_pressure() / MPA);
        m.insert("sg_level_pct", self.sg_level());
        m.insert("steam_flow_kg_s", self.steam_flow);
        m.insert("feed_flow_kg_s", self.feed_flow);
        m.insert("feed_demand_pct", self.feed * 100.0);
        m.insert("feed_temp_c", self.t_feed);
        m.insert("turbine_valve_pct", self.turbine_valve * 100.0);
        m.insert("steam_dump_pct", self.steam_dump * 100.0);
        m.insert("gross_mwe", self.gross_mwe);
        m.insert("net_mwe", self.gross_mwe - self.p.house_load_mwe);
        m.insert("reactivity_pcm", self.rho.total * 1e5);
        m.insert("turbine_speed_rpm", self.tg.rpm());
        m.insert("gen_freq_hz", self.tg.gen_hz());
        m.insert("grid_freq_hz", self.tg.grid_hz);
        m.insert("sync_phase_deg", self.tg.phase);
        m.insert("sync_slip_hz", self.tg.slip_hz());
        m.insert("gen_voltage_kv", self.tg.gen_kv());
        m.insert("grid_voltage_kv", self.tg.grid_kv);
        m.insert("excitation_pct", self.tg.excitation * 100.0);
        m.insert("generator_health_pct", self.tg.health);
        m
    }

    fn init_sensors(&mut self) {
        self.sensors.clear();
        let truth = self.true_values();
        let spec: &[(&str, f64, f64, (f64, f64))] = &[
            ("sr_cps", 1.0, 0.0, (0.0, 1.0e6)),
            ("sr_sur_dpm", 1.0, 0.02, (-1.0, 5.0)),
            ("ir_amps", 0.3, 0.0, (1.0e-11, 1.0e-3)),
            ("ir_sur_dpm", 1.0, 0.02, (-1.0, 5.0)),
            ("pr_n41_pct", 0.2, 0.15, (0.0, 120.0)),
            ("pr_n42_pct", 0.2, 0.15, (0.0, 120.0)),
            ("pr_n43_pct", 0.2, 0.15, (0.0, 120.0)),
            ("pr_n44_pct", 0.2, 0.15, (0.0, 120.0)),
            ("nuclear_power_pct", 0.2, 0.1, (0.0, 120.0)),
            ("thermal_power_mw", 5.0, 3.0, (0.0, 4500.0)),
            ("startup_rate_dpm", 1.0, 0.02, (-1.0, 5.0)),
            ("tavg_c", 2.0, 0.05, (0.0, 400.0)),
            ("tref_c", 0.0, 0.0, (0.0, 400.0)),
            ("thot_c", 2.0, 0.05, (0.0, 400.0)),
            ("tcold_c", 2.0, 0.05, (0.0, 400.0)),
            ("delta_t_pct", 2.0, 0.2, (0.0, 150.0)),
            ("rcs_pressure_mpa", 0.3, 0.005, (0.0, 20.0)),
            ("pzr_level_pct", 1.0, 0.2, (0.0, 100.0)),
            ("pzr_level_program_pct", 0.0, 0.0, (0.0, 100.0)),
            ("rcs_flow_pct", 0.5, 0.3, (0.0, 120.0)),
            ("boron_ppm", 60.0, 1.0, (0.0, 3000.0)),
            ("pzr_heaters_pct", 0.0, 0.0, (0.0, 100.0)),
            ("pzr_spray_pct", 0.0, 0.0, (0.0, 100.0)),
            ("charging_kg_s", 1.0, 0.05, (0.0, 30.0)),
            ("letdown_kg_s", 1.0, 0.05, (0.0, 30.0)),
            ("sg_pressure_mpa", 0.3, 0.005, (0.0, 10.0)),
            ("sg_level_pct", 1.0, 0.3, (0.0, 100.0)),
            ("steam_flow_kg_s", 1.0, 2.0, (0.0, 2500.0)),
            ("feed_flow_kg_s", 1.0, 2.0, (0.0, 2500.0)),
            ("feed_demand_pct", 0.0, 0.0, (0.0, 100.0)),
            ("feed_temp_c", 5.0, 0.2, (0.0, 300.0)),
            ("turbine_valve_pct", 0.0, 0.0, (0.0, 100.0)),
            ("steam_dump_pct", 0.0, 0.0, (0.0, 100.0)),
            ("gross_mwe", 0.5, 1.0, (0.0, 1400.0)),
            ("net_mwe", 0.5, 1.0, (-100.0, 1400.0)),
            ("reactivity_pcm", 1.0, 0.5, (-20000.0, 1000.0)),
            ("turbine_speed_rpm", 0.2, 0.2, (0.0, 2200.0)),
            ("gen_freq_hz", 0.1, 0.002, (0.0, 70.0)),
            ("grid_freq_hz", 0.1, 0.002, (55.0, 65.0)),
            ("sync_phase_deg", 0.0, 0.0, (-180.0, 180.0)),
            ("sync_slip_hz", 0.1, 0.001, (-70.0, 70.0)),
            ("gen_voltage_kv", 0.2, 0.02, (0.0, 30.0)),
            ("grid_voltage_kv", 0.2, 0.02, (0.0, 30.0)),
            ("excitation_pct", 0.0, 0.0, (0.0, 100.0)),
            ("generator_health_pct", 0.0, 0.0, (0.0, 100.0)),
        ];
        for (tag, tau, noise, range) in spec {
            let v = truth.get(tag).copied().unwrap_or(0.0);
            self.sensors
                .insert(tag, Sensor::new(v, *tau, *noise, *range));
        }
        self.readings = truth;
    }

    fn read_instruments(&mut self, dt: f64) {
        let truth = self.true_values();
        let sr_off = self.nis.sr_blocked;
        for (tag, s) in self.sensors.iter_mut() {
            let t = truth.get(tag).copied().unwrap_or(0.0);
            let t = if t.is_finite() { t } else { 0.0 };
            if sr_off && (*tag == "sr_cps" || *tag == "sr_sur_dpm") {
                // High voltage removed: the channel reads zero at once.
                s.value = 0.0;
                self.readings.insert(tag, 0.0);
                continue;
            }
            let mut v = s.update(t, dt, &mut self.rng);
            if s.fault == SensorFault::None {
                match *tag {
                    // Counting statistics (~1 s counting interval).
                    "sr_cps" => v = (v + v.max(0.0).sqrt() * self.rng.normal()).max(0.0),
                    "ir_amps" => v *= 1.0 + 0.02 * self.rng.normal(),
                    _ => {}
                }
            }
            self.readings.insert(tag, v);
        }
    }

    fn r(&self, tag: &str) -> f64 {
        self.readings.get(tag).copied().unwrap_or(0.0)
    }

    fn p6(&self) -> bool {
        self.r("ir_amps") > P6_AMPS
    }

    fn p10(&self) -> bool {
        ["pr_n41_pct", "pr_n42_pct", "pr_n43_pct", "pr_n44_pct"]
            .iter()
            .filter(|t| self.r(t) > P10_PCT)
            .count()
            >= 2
    }

    /// Below the permissives (with reset hysteresis) the blocks are removed
    /// automatically.
    fn update_nis_permissives(&mut self) {
        let p10_reset = ["pr_n41_pct", "pr_n42_pct", "pr_n43_pct", "pr_n44_pct"]
            .iter()
            .filter(|t| self.r(t) > P10_PCT - 2.0)
            .count()
            < 2;
        let p6_reset = self.r("ir_amps") < 0.3 * P6_AMPS;
        if p10_reset && (self.nis.ir_blocked || self.nis.pr_low_blocked) {
            self.nis.ir_blocked = false;
            self.nis.pr_low_blocked = false;
            self.events.push(PlantEvent::info(
                "P-10 cleared: IR and PR low-setpoint trips reinstated",
            ));
        }
        if p6_reset && self.nis.sr_blocked {
            self.nis.sr_blocked = false;
            self.events.push(PlantEvent::info(
                "P-6 cleared: source range energised and trip reinstated",
            ));
        }
    }

    // ------------------------------------------------------------ automation

    fn run_automation(&mut self, dt: f64) {
        let auto = |s: &Self, m: ModuleId| s.automation.mode(m) == ModuleMode::Auto;
        let tavg = self.r("tavg_c");
        let tref = self.tref();
        let pressure = self.r("rcs_pressure_mpa");
        let w_nom = self.steam_nominal();
        let breaker = self.tg.breaker;
        let rpm = self.r("turbine_speed_rpm");

        // Plant master sets the governor load target.
        if auto(self, ModuleId::PlantMaster) {
            let target = self
                .automation
                .setpoint(ModuleId::PlantMaster, "target_mwe")
                .unwrap_or(0.0);
            let p_set = self
                .automation
                .setpoint(ModuleId::PressurizerPressure, "pressure_mpa")
                .unwrap_or(15.51);
            let t =
                ctl::PlantMaster.update(target, self.gov.load_ref, tavg - tref, pressure - p_set);
            self.automation
                .setpoint_set(ModuleId::TurbineGovernor, "target_mwe", t);
        }

        // Auto-synchronizer near synchronous speed.
        if auto(self, ModuleId::GeneratorSync) && !breaker && !self.turbine_tripped && rpm > 1750.0
        {
            let a = ctl::AutoSynchronizer.update(
                self.tg.grid_hz,
                self.tg.grid_kv,
                self.tg.gen_kv(),
                self.tg.phase,
                self.tg.slip_hz(),
            );
            if auto(self, ModuleId::TurbineGovernor) {
                self.automation
                    .setpoint_set(ModuleId::TurbineGovernor, "speed_rpm", a.speed_rpm);
            }
            if auto(self, ModuleId::Excitation) {
                self.automation
                    .setpoint_set(ModuleId::Excitation, "voltage_kv", a.voltage_kv);
            }
            if a.close && self.tg.close(true) == CloseOutcome::Closed {
                self.events.push(PlantEvent::info(
                    "Generator synchronised to grid (auto-synchronizer)",
                ));
            }
        }

        // Breaker state transitions switch the governor between speed and load control.
        if self.tg.breaker != self.prev_breaker {
            if self.tg.breaker {
                let pickup = (0.05 * self.p.gross_mwe).max(self.gross_mwe);
                self.gov.track(self.turbine_valve, self.gross_mwe);
                let tgt = self
                    .automation
                    .setpoint(ModuleId::TurbineGovernor, "target_mwe")
                    .unwrap_or(0.0);
                if tgt < pickup {
                    self.automation
                        .setpoint_set(ModuleId::TurbineGovernor, "target_mwe", pickup);
                }
            } else {
                self.gov.track_speed(self.turbine_valve, rpm);
                self.automation
                    .setpoint_set(ModuleId::TurbineGovernor, "speed_rpm", RATED_RPM);
            }
            self.prev_breaker = self.tg.breaker;
        }

        if auto(self, ModuleId::TurbineGovernor) {
            if self.turbine_tripped {
                self.turbine_valve = 0.0;
                self.gov.track_speed(0.0, rpm);
            } else if self.tg.breaker {
                let target = self
                    .automation
                    .setpoint(ModuleId::TurbineGovernor, "target_mwe")
                    .unwrap_or(0.0);
                let ramp = self
                    .automation
                    .setpoint(ModuleId::TurbineGovernor, "ramp_mw_min")
                    .unwrap_or(30.0);
                self.turbine_valve = self.gov.update(target, ramp, self.r("gross_mwe"), dt);
            } else {
                let target = self
                    .automation
                    .setpoint(ModuleId::TurbineGovernor, "speed_rpm")
                    .unwrap_or(0.0);
                let accel = self
                    .automation
                    .setpoint(ModuleId::TurbineGovernor, "accel_rpm_min")
                    .unwrap_or(300.0);
                self.turbine_valve = self.gov.update_speed(target, accel, rpm, dt);
            }
        }

        if auto(self, ModuleId::Excitation) {
            if self.tg.field_breaker && !self.tg.breaker {
                let sp = self
                    .automation
                    .setpoint(ModuleId::Excitation, "voltage_kv")
                    .unwrap_or(22.0);
                self.tg.excitation = self.avr.update(sp, self.r("gen_voltage_kv"), dt);
            } else {
                self.avr.track(self.tg.excitation);
            }
        }

        if auto(self, ModuleId::RodControl) && !self.reactor_tripped {
            let speed = self.rod_ctl.update(
                tavg,
                tref,
                self.r("nuclear_power_pct") / 100.0,
                self.turbine_load(),
                dt,
            );
            let pos: Vec<f64> = self.banks[2..].iter().map(|b| b.position).collect();
            let deltas =
                overlap_sequence(&pos, self.p.rod_max_steps, self.p.rod_overlap, speed, dt);
            for (b, d) in self.banks[2..].iter_mut().zip(deltas) {
                b.target = clamp(b.position + d, 0.0, b.max_steps);
            }
        }

        if auto(self, ModuleId::BoronControl) {
            let low = self
                .automation
                .setpoint(ModuleId::BoronControl, "band_low_steps")
                .unwrap_or(170.0);
            let high = self
                .automation
                .setpoint(ModuleId::BoronControl, "band_high_steps")
                .unwrap_or(215.0);
            self.boration = match self.boron_ctl.update(self.banks[5].position, low, high) {
                -1 => BorationMode::Dilute,
                1 => BorationMode::Borate,
                _ => BorationMode::Off,
            };
        }

        if auto(self, ModuleId::PressurizerPressure) {
            let sp = self
                .automation
                .setpoint(ModuleId::PressurizerPressure, "pressure_mpa")
                .unwrap_or(15.51);
            let (h, s) = self.pzr_p_ctl.update(pressure, sp, dt);
            self.heaters = h;
            self.spray = s;
        }

        if auto(self, ModuleId::PressurizerLevel) {
            let sp = self.pzr_level_program(tavg);
            self.charging = self
                .pzr_l_ctl
                .update(self.r("pzr_level_pct"), sp, self.letdown, dt);
        }

        if auto(self, ModuleId::FeedwaterLevel) {
            let sp = self
                .automation
                .setpoint(ModuleId::FeedwaterLevel, "level_pct")
                .unwrap_or(50.0);
            let steam_frac = self.r("steam_flow_kg_s") / (self.p.feed_max_frac * w_nom);
            self.feed = self
                .fw_ctl
                .update(self.r("sg_level_pct"), sp, steam_frac, dt);
        }

        if auto(self, ModuleId::SteamDump) {
            let noload = self
                .automation
                .setpoint(ModuleId::SteamDump, "noload_tavg_c")
                .unwrap_or(self.p.tavg_noload);
            // Off-line (or tripped) the dumps hold no-load temperature;
            // on-line they only open on a load rejection.
            let hold_noload = self.reactor_tripped || !self.tg.breaker;
            self.steam_dump = self.dump_ctl.update(tavg, tref, noload, hold_noload, dt);
        }
    }

    fn run_protection(&mut self) {
        let inputs = RpsInputs {
            sr_cps: self.r("sr_cps"),
            ir_amps: self.r("ir_amps"),
            flux_pct: self.r("nuclear_power_pct"),
            sr_blocked: self.nis.sr_blocked,
            ir_blocked: self.nis.ir_blocked,
            pr_low_blocked: self.nis.pr_low_blocked,
            pressure_mpa: self.r("rcs_pressure_mpa"),
            pzr_level_pct: self.r("pzr_level_pct"),
            flow_pct: self.r("rcs_flow_pct"),
            min_sg_level_pct: self.r("sg_level_pct"),
            delta_t_pct: self.r("delta_t_pct"),
            turbine_tripped: self.turbine_tripped,
        };
        let signals = self.rps.evaluate(&inputs);
        if self.training_mode {
            return;
        }
        for s in signals {
            // "Turbine trip above P-9" is a consequence while already tripped.
            if !self.trips.iter().any(|t| t == s) {
                self.trips.push(s.to_string());
                self.events.push(PlantEvent::trip(s));
            }
            if !self.reactor_tripped {
                self.trip_reactor();
            }
        }
    }

    fn trip_reactor(&mut self) {
        self.reactor_tripped = true;
        self.rod_direction = RodDirection::Hold;
        self.trip_turbine();
    }

    fn trip_turbine(&mut self) {
        if !self.turbine_tripped {
            self.turbine_tripped = true;
            self.turbine_valve = 0.0;
            self.events
                .push(PlantEvent::warn("Turbine tripped (stop valves closed)"));
        }
    }

    // -------------------------------------------------------------- commands

    pub fn policy_context(&self) -> PolicyContext {
        PolicyContext {
            mode: self.automation.game_mode,
            owned: self.automation.owned.clone(),
            module_modes: self.automation.modes.clone(),
        }
    }

    /// Apply a command after policy and interlock checks.
    pub fn command(&mut self, cmd: &Command, origin: &Origin) -> Result<(), Denied> {
        authorize(cmd, origin, &self.policy_context())?;
        if let CommandKind::Manual(actuator) = cmd.kind() {
            self.check_actuator(actuator, origin)?;
        }
        let deny = |m: &str| Err(Denied(m.to_string()));
        match cmd {
            Command::MoveRodBank { bank, target } => {
                if self.reactor_tripped {
                    return deny("reactor trip is latched; reset before moving rods");
                }
                let Some(b) = self
                    .banks
                    .iter_mut()
                    .find(|b| b.name.eq_ignore_ascii_case(bank))
                else {
                    return deny("unknown rod bank");
                };
                b.target = clamp(*target, 0.0, b.max_steps);
            }
            Command::RodMotion {
                selection,
                direction,
            } => {
                let valid = selection.eq_ignore_ascii_case("manual")
                    || self
                        .banks
                        .iter()
                        .any(|b| b.name.eq_ignore_ascii_case(selection));
                if !valid {
                    return deny("bank selector must be MANUAL or a bank name");
                }
                if self.reactor_tripped && *direction == RodDirection::Out {
                    return deny("reactor trip is latched; reset before withdrawing rods");
                }
                self.rod_selection = selection.clone();
                self.rod_direction = *direction;
                if *direction == RodDirection::Hold {
                    for b in &mut self.banks {
                        b.target = b.position;
                    }
                }
            }
            Command::SetBoration { mode } => self.boration = *mode,
            Command::SetPressurizerHeaters { fraction } => {
                self.heaters = clamp(*fraction, 0.0, 1.0)
            }
            Command::SetPressurizerSpray { fraction } => self.spray = clamp(*fraction, 0.0, 1.0),
            Command::SetCharging { kg_s } => self.charging = clamp(*kg_s, 0.0, self.p.charging_max),
            Command::SetLetdown { kg_s } => self.letdown = clamp(*kg_s, 0.0, 15.0),
            Command::SetCoolantPump { index, running } => {
                if *index >= 4 {
                    return deny("RCP index must be 0-3");
                }
                self.rcp_running[*index] = *running;
            }
            Command::SetFeedwater { fraction } => self.feed = clamp(*fraction, 0.0, 1.0),
            Command::SetSteamDump { fraction } => self.steam_dump = clamp(*fraction, 0.0, 1.0),
            Command::SetTurbineValve { fraction } => {
                if self.turbine_tripped {
                    return deny("turbine is tripped; reset (latch) the turbine first");
                }
                self.turbine_valve = clamp(*fraction, 0.0, 1.0);
            }
            Command::SetGeneratorBreaker { closed: false } => {
                if self.tg.breaker {
                    self.tg.open();
                    self.events
                        .push(PlantEvent::warn("Generator breaker opened by operator"));
                }
            }
            Command::SetGeneratorBreaker { closed: true } => return self.close_breaker(),
            Command::SetFieldBreaker { closed } => {
                self.tg.field_breaker = *closed;
                if *closed {
                    self.avr.track(self.tg.excitation);
                }
            }
            Command::SetExcitation { fraction } => self.tg.excitation = clamp(*fraction, 0.0, 1.0),
            Command::BlockNis { channel, blocked } => {
                if *blocked {
                    match channel {
                        NisBlock::SourceRange if !self.p6() => {
                            return deny("source range block requires P-6 (IR above 1E-10 A)")
                        }
                        NisBlock::IntermediateRange | NisBlock::PowerRangeLow if !self.p10() => {
                            return deny("block requires P-10 (2/4 power range above 10 %)")
                        }
                        _ => {}
                    }
                }
                let (flag, name) = match channel {
                    NisBlock::SourceRange => (&mut self.nis.sr_blocked, "Source range"),
                    NisBlock::IntermediateRange => (&mut self.nis.ir_blocked, "Intermediate range"),
                    NisBlock::PowerRangeLow => {
                        (&mut self.nis.pr_low_blocked, "Power range low setpoint")
                    }
                };
                *flag = *blocked;
                if *channel == NisBlock::SourceRange && *blocked {
                    self.readings.insert("sr_cps", 0.0);
                    self.readings.insert("sr_sur_dpm", 0.0);
                }
                let what = if *blocked {
                    "trip blocked"
                } else {
                    "trip reset (unblocked)"
                };
                self.events
                    .push(PlantEvent::info(&format!("{name} {what}")));
            }
            Command::Scram => {
                self.trips.push("Manual reactor trip".into());
                self.events.push(PlantEvent::trip("Manual reactor trip"));
                self.trip_reactor();
            }
            Command::TripTurbine => {
                self.events.push(PlantEvent::warn("Manual turbine trip"));
                self.trip_turbine();
            }
            Command::ResetTrip => {
                if self.reactor_tripped && self.banks.iter().any(|b| b.position > 0.0) {
                    return deny("rods still inserting");
                }
                self.reactor_tripped = false;
                self.trips.clear();
                for b in &mut self.banks {
                    b.target = b.position;
                }
                self.events.push(PlantEvent::info("Reactor trip reset"));
            }
            Command::ResetTurbine => {
                if self.reactor_tripped {
                    return deny("reactor trip present: turbine trip cannot be reset");
                }
                if self.tg.speed > 1.05 {
                    return deny("turbine overspeed: wait for speed to fall");
                }
                if self.turbine_tripped {
                    self.turbine_tripped = false;
                    self.turbine_valve = 0.0;
                    self.gov.track_speed(0.0, self.tg.rpm());
                    self.events
                        .push(PlantEvent::info("Turbine reset (latched)"));
                }
            }
            Command::SetModuleMode { module, mode } => self.set_module_mode(*module, *mode),
            Command::SetSetpoint { module, key, value } => {
                if self.automation.setpoint(*module, key).is_none() {
                    return deny("unknown setpoint for this module");
                }
                self.automation.setpoint_set(*module, key, *value);
            }
            Command::SetTimeScale { .. } | Command::SetPaused { .. } => {
                // Handled by the host (server / CLI) loop.
            }
            Command::InjectSensorFault { tag, fault } => {
                let f = match fault.as_str() {
                    "none" => SensorFault::None,
                    "stuck" => SensorFault::Stuck,
                    "fail_low" => SensorFault::FailLow,
                    "fail_high" => SensorFault::FailHigh,
                    _ => return deny("fault must be none|stuck|fail_low|fail_high"),
                };
                let Some(s) = self.sensors.get_mut(tag.as_str()) else {
                    return deny("unknown sensor tag");
                };
                s.fault = f;
            }
            Command::SetTrainingMode { enabled } => self.training_mode = *enabled,
            Command::RepairGenerator => {
                self.tg.repair();
                self.events
                    .push(PlantEvent::info("Generator repaired, 86G lockout reset"));
            }
            Command::SetControlRoom { generation } => {
                self.control_room = *generation;
                self.events.push(PlantEvent::info(&format!(
                    "Control room set to {generation:?}"
                )));
            }
            Command::LoadScenario { name } => {
                let Some(mut fresh) = Self::from_scenario(name) else {
                    return deny("scenario must be hot_standby or full_power");
                };
                fresh.automation.game_mode = self.automation.game_mode;
                fresh.automation.owned = self.automation.owned.clone();
                fresh.control_room = self.control_room;
                fresh
                    .events
                    .push(PlantEvent::info(&format!("Scenario loaded: {name}")));
                *self = fresh;
            }
        }
        Ok(())
    }

    fn close_breaker(&mut self) -> Result<(), Denied> {
        if self.turbine_tripped {
            return Err(Denied("turbine is tripped".into()));
        }
        let sync_check = self.sync_check_active();
        let phase = self.tg.phase;
        let slip = self.tg.slip_hz();
        match self.tg.close(sync_check) {
            CloseOutcome::Closed => {
                self.events.push(PlantEvent::info(&format!(
                    "Generator breaker closed (phase {phase:+.1}°, slip {slip:+.3} Hz)"
                )));
                Ok(())
            }
            CloseOutcome::Blocked { .. } => {
                self.sync_block_until = self.time + 5.0;
                let msg = format!(
                    "Sync-check blocked breaker close: PHASE Δ OUT OF RANGE (phase {phase:+.0}°, slip {slip:+.2} Hz)"
                );
                self.events.push(PlantEvent::warn(&msg));
                Err(Denied(msg))
            }
            CloseOutcome::Rough { damage, .. } => {
                self.events.push(PlantEvent::warn(&format!(
                    "Rough synchronization at {phase:+.0}°: generator stressed (−{damage:.1} % health)"
                )));
                if self.tg.lockout {
                    self.events
                        .push(PlantEvent::trip("Generator lockout relay 86G tripped"));
                }
                Ok(())
            }
            CloseOutcome::Tripped { damage, .. } => {
                self.events.push(PlantEvent::trip(&format!(
                    "Out-of-phase closure at {phase:+.0}°: generator differential trip (−{damage:.0} % health)"
                )));
                if self.tg.lockout {
                    self.events
                        .push(PlantEvent::trip("Generator lockout relay 86G tripped"));
                }
                self.trip_turbine();
                Ok(())
            }
            CloseOutcome::Refused(why) => Err(Denied(why.into())),
        }
    }

    fn check_actuator(&self, actuator: Actuator, origin: &Origin) -> Result<(), Denied> {
        let Some(owner) = modules::owner_of(actuator) else {
            return Ok(());
        };
        // The breaker control switch stays live under the auto-synchronizer;
        // closures are supervised by the sync-check relay instead.
        if actuator == Actuator::GeneratorBreaker && *origin == Origin::Operator {
            return Ok(());
        }
        match (self.automation.mode(owner), origin) {
            (ModuleMode::Auto, _) => Err(Denied(format!(
                "{owner:?} is in AUTO; switch it to MANUAL first"
            ))),
            (ModuleMode::External, Origin::Operator) => Err(Denied(format!(
                "{owner:?} is under EXTERNAL control; switch it to MANUAL first"
            ))),
            _ => Ok(()),
        }
    }

    fn set_module_mode(&mut self, module: ModuleId, mode: ModuleMode) {
        // Bumpless transfer: initialise controllers from current outputs.
        if mode == ModuleMode::Auto {
            match module {
                ModuleId::RodControl => self.rod_ctl.reset(self.turbine_load()),
                ModuleId::PressurizerPressure => self.pzr_p_ctl.track(self.heaters),
                ModuleId::PressurizerLevel => self.pzr_l_ctl.track(self.charging, self.letdown),
                ModuleId::FeedwaterLevel => {
                    let sf = self.steam_flow / (self.p.feed_max_frac * self.steam_nominal());
                    self.fw_ctl.track(self.feed, sf)
                }
                ModuleId::TurbineGovernor => {
                    if self.tg.breaker {
                        self.gov.track(self.turbine_valve, self.gross_mwe);
                        self.automation.setpoint_set(
                            ModuleId::TurbineGovernor,
                            "target_mwe",
                            self.gross_mwe,
                        );
                    } else {
                        let rpm = self.tg.rpm();
                        self.gov.track_speed(self.turbine_valve, rpm);
                        self.automation
                            .setpoint_set(ModuleId::TurbineGovernor, "speed_rpm", rpm);
                    }
                }
                ModuleId::Excitation => {
                    self.avr.track(self.tg.excitation);
                    self.automation.setpoint_set(
                        ModuleId::Excitation,
                        "voltage_kv",
                        self.tg.grid_kv,
                    );
                }
                ModuleId::PlantMaster => {
                    self.automation.setpoint_set(
                        ModuleId::PlantMaster,
                        "target_mwe",
                        self.gov.load_ref,
                    );
                }
                _ => {}
            }
        }
        if mode != ModuleMode::Auto {
            if module == ModuleId::RodControl {
                self.rod_direction = RodDirection::Hold;
                for b in &mut self.banks {
                    b.target = b.position;
                }
            }
            // Dropping a prerequisite drops the plant master too.
            if modules::info(ModuleId::PlantMaster)
                .prerequisites
                .contains(&module)
                && self.automation.mode(ModuleId::PlantMaster) == ModuleMode::Auto
            {
                self.automation
                    .modes
                    .insert(ModuleId::PlantMaster, ModuleMode::Manual);
                self.events
                    .push(PlantEvent::warn("Plant master dropped to MANUAL"));
            }
        }
        self.automation.modes.insert(module, mode);
    }

    // ------------------------------------------------------------- telemetry

    fn alarms(&self) -> Vec<Alarm> {
        let mut a = Vec::new();
        let mut add = |panel: &str, tag: &str, text: &str, sev: Severity| {
            a.push(Alarm {
                tag: tag.into(),
                panel: panel.into(),
                text: text.into(),
                severity: sev,
            })
        };
        let tavg = self.r("tavg_c");
        // Reactor / NIS
        if self.reactor_tripped {
            add("reactor", "rx_trip", "REACTOR TRIP", Severity::Trip);
        }
        if self.p6() {
            add("reactor", "p6", "P-6 IR PERMISSIVE", Severity::Info);
        }
        if self.p10() {
            add("reactor", "p10", "P-10 PR PERMISSIVE", Severity::Info);
        }
        if self.nis.sr_blocked {
            add(
                "reactor",
                "sr_blocked",
                "SR TRIP BLOCKED / HV OFF",
                Severity::Info,
            );
        }
        if self.nis.ir_blocked {
            add("reactor", "ir_blocked", "IR TRIP BLOCKED", Severity::Info);
        }
        if self.nis.pr_low_blocked {
            add(
                "reactor",
                "pr_low_blocked",
                "PR LO SETPT TRIP BLOCKED",
                Severity::Info,
            );
        }
        if self.p6() && !self.nis.sr_blocked {
            add(
                "reactor",
                "sr_block_permitted",
                "SR BLOCK PERMITTED",
                Severity::Warning,
            );
        }
        if self.r("startup_rate_dpm") > 1.0 {
            add("reactor", "sur", "HIGH STARTUP RATE", Severity::Warning);
        }
        if !self.reactor_tripped
            && self.banks[5].position < 30.0
            && self.banks[4].position < 128.0
            && self.r("nuclear_power_pct") > 5.0
        {
            add("reactor", "rod_ins", "ROD INSERTION LIMIT", Severity::Alarm);
        }
        // RCS
        if (tavg - self.tref()).abs() > 3.0 && !self.reactor_tripped && self.tg.breaker {
            add(
                "rcs",
                "tavg_dev",
                "TAVG / TREF DEVIATION",
                Severity::Warning,
            );
        }
        let p = self.r("rcs_pressure_mpa");
        if p > 15.9 {
            add("rcs", "p_high", "PZR PRESS HIGH", Severity::Alarm);
        }
        if p < 15.1 {
            add("rcs", "p_low", "PZR PRESS LOW", Severity::Alarm);
        }
        if self.porv_open {
            add("rcs", "porv", "PZR PORV OPEN", Severity::Alarm);
        }
        let lvl = self.r("pzr_level_pct");
        if (lvl - self.pzr_level_program(tavg)).abs() > 5.0 {
            add(
                "rcs",
                "pzr_lvl_dev",
                "PZR LEVEL DEVIATION",
                Severity::Warning,
            );
        }
        if self.rcp_running.iter().any(|r| !r) {
            add("rcs", "rcp", "RCP STOPPED", Severity::Warning);
        }
        if self.r("rcs_flow_pct") < 92.0 {
            add("rcs", "rcs_low_flow", "RCS LOW FLOW", Severity::Alarm);
        }
        // CVCS
        match self.boration {
            BorationMode::Borate => add("cvcs", "borating", "BORATION IN PROGRESS", Severity::Info),
            BorationMode::Dilute => add("cvcs", "diluting", "DILUTION IN PROGRESS", Severity::Info),
            BorationMode::Off => {}
        }
        if self.charging < 1.0 {
            add(
                "cvcs",
                "charging_low",
                "CHARGING FLOW LOW",
                Severity::Warning,
            );
        }
        // Steam & feed
        let sg = self.r("sg_level_pct");
        if !(38.0..=62.0).contains(&sg) {
            add(
                "steam",
                "sg_lvl_dev",
                "SG LEVEL DEVIATION",
                Severity::Warning,
            );
        }
        if sg < 20.0 {
            add("steam", "sg_lvl_low", "SG LEVEL LOW", Severity::Alarm);
        }
        if self.steam_pressure() > SG_ARV {
            add("steam", "sg_arv", "SG RELIEF VALVE OPEN", Severity::Alarm);
        }
        if self.steam_dump > 0.02 {
            add("steam", "dump_open", "STEAM DUMP OPEN", Severity::Info);
        }
        // Turbine-generator
        if self.turbine_tripped {
            add("turbine", "turb_trip", "TURBINE TRIP", Severity::Alarm);
        }
        if self.tg.speed > 1.03 {
            add("turbine", "overspeed", "TURBINE OVERSPEED", Severity::Alarm);
        }
        if self.tg.lockout {
            add("turbine", "gen_lockout", "GEN LOCKOUT 86G", Severity::Trip);
        }
        if self.tg.health < 90.0 {
            add("turbine", "gen_damage", "GENERATOR DAMAGE", Severity::Alarm);
        }
        if !self.tg.field_breaker {
            add(
                "turbine",
                "field_open",
                "FIELD BREAKER OPEN",
                Severity::Info,
            );
        }
        if self.time < self.sync_block_until {
            add(
                "turbine",
                "sync_blocked",
                "PHASE Δ OUT OF RANGE",
                Severity::Alarm,
            );
        }
        if self.tg.breaker && self.tg.net_power < 0.0 {
            add(
                "turbine",
                "reverse_power",
                "GEN REVERSE POWER",
                Severity::Alarm,
            );
        }
        if !self.tg.breaker && self.tg.speed > 0.97 && !self.turbine_tripped {
            add("turbine", "ready_sync", "READY TO SYNC", Severity::Info);
        }
        // Plant computer
        if self.training_mode {
            add(
                "computer",
                "training",
                "TRAINING MODE: RPS BYPASSED",
                Severity::Info,
            );
        }
        for (tag, s) in &self.sensors {
            if s.fault != SensorFault::None {
                add(
                    "computer",
                    tag,
                    &format!("INSTRUMENT FAULT {tag}"),
                    Severity::Warning,
                );
            }
        }
        a
    }

    pub fn telemetry(&self, time_scale: f64, paused: bool) -> Telemetry {
        let ctx = self.policy_context();
        Telemetry {
            api_version: API_VERSION,
            unit: self.name.clone(),
            reactor_type: "PWR-1150 (4-loop)".into(),
            game_mode: self.automation.game_mode,
            sim_time: self.time,
            time_scale,
            paused,
            training_mode: self.training_mode,
            readings: self
                .readings
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            rods: self
                .banks
                .iter()
                .map(|b| RodView {
                    bank: b.name.clone(),
                    position: b.position,
                    target: b.target,
                    max_steps: b.max_steps,
                })
                .collect(),
            rod_motion: format!("{}:{:?}", self.rod_selection, self.rod_direction).to_lowercase(),
            coolant_pumps: self.rcp_running.to_vec(),
            generator_breaker: self.tg.breaker,
            field_breaker: self.tg.field_breaker,
            generator_lockout: self.tg.lockout,
            sync_check_active: self.sync_check_active(),
            sync_blocked: self.time < self.sync_block_until,
            nis: NisView {
                sr_blocked: self.nis.sr_blocked,
                ir_blocked: self.nis.ir_blocked,
                pr_low_blocked: self.nis.pr_low_blocked,
                sr_hv_on: !self.nis.sr_blocked,
                p6: self.p6(),
                p10: self.p10(),
            },
            control_room: self.control_room,
            scenario: self.scenario.clone(),
            boration: self.boration,
            reactor_tripped: self.reactor_tripped,
            turbine_tripped: self.turbine_tripped,
            trips: self.trips.clone(),
            alarms: self.alarms(),
            modules: modules::CATALOG
                .iter()
                .map(|m| ModuleView {
                    id: m.id,
                    name: m.name.into(),
                    panel: m.panel.into(),
                    mode: self.automation.mode(m.id),
                    owned: ctx.owns(m.id),
                    implemented: m.implemented,
                    setpoints: self
                        .automation
                        .setpoints
                        .get(&m.id)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect(),
        }
    }

    /// True physics state (sandbox / debugging only).
    pub fn truth(&self) -> BTreeMap<String, f64> {
        let mut m: BTreeMap<String, f64> = self
            .true_values()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let mut put = |k: &str, v: f64| {
            m.insert(k.into(), v);
        };
        put("fuel_temp_c", self.t_fuel);
        put("core_outlet_c", self.t_core);
        put("sg_outlet_c", self.t_sg);
        put("steam_temp_c", self.t_steam);
        put("rcs_mass_kg", self.rcs_mass);
        put("sg_mass_kg", self.sg_mass);
        put("decay_heat_pct", self.decay.power() * 100.0);
        put("period_s", self.kinetics.period);
        put("relative_flux", self.kinetics.n);
        put("rho_rods_pcm", self.rho.rods * 1e5);
        put("rho_boron_pcm", self.rho.boron * 1e5);
        put("rho_doppler_pcm", self.rho.doppler * 1e5);
        put("rho_moderator_pcm", self.rho.moderator * 1e5);
        put("rho_xenon_pcm", self.rho.xenon * 1e5);
        put("rho_samarium_pcm", self.rho.samarium * 1e5);
        put("q_sg_mw", self.q_sg / 1e6);
        put("shaft_net_mw", self.tg.net_power / 1e6);
        m
    }

    pub fn game_mode(&self) -> GameMode {
        self.automation.game_mode
    }
}
