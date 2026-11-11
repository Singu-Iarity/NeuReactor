//! Four-loop pressurized water reactor (~3411 MWt / ~1150 MWe class).
//!
//! Model summary (see `docs/PHYSICS.md`):
//! * point kinetics (6 delayed groups), Doppler / moderator / boron / rod /
//!   xenon / samarium reactivity, 9-group decay heat;
//! * lumped fuel heat capacity with fuel→coolant conductance;
//! * primary loop as four mixing volumes (core, hot leg, SG tubes, cold leg)
//!   with pump coastdown and natural circulation;
//! * pressurizer with insurge compression, heaters, spray, PORV and safeties;
//! * lumped saturated SG secondary, turbine, steam dumps, relief valves and
//!   feedwater with extraction heating.

use crate::plant::{AutomationState, PlantEvent};
use nr_api::{
    authorize, modules, Actuator, Alarm, BorationMode, Command, CommandKind, Denied, GameMode,
    ModuleId, ModuleMode, ModuleView, Origin, PolicyContext, RodView, Severity, Telemetry,
    API_VERSION,
};
use nr_control::controllers::{self as ctl, overlap_sequence};
use nr_control::{Rps, RpsInputs};
use nr_core::{clamp, lerp, steam, Rng, MPA};
use nr_physics::{
    DecayHeat, DelayedNeutronData, PointKinetics, Poisons, RodBank, Sensor, SensorFault,
};
use std::collections::BTreeMap;

/// Design parameters.
#[derive(Debug, Clone)]
pub struct PwrParams {
    pub thermal_power: f64,
    pub gross_mwe: f64,
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
    pub cycle_efficiency: f64,
    pub house_load_mwe: f64,
    pub rod_speed_max: f64,
    pub rod_max_steps: f64,
    pub rod_overlap: f64,
    pub scram_time: f64,
}

impl Default for PwrParams {
    fn default() -> Self {
        Self {
            thermal_power: 3411.0e6,
            gross_mwe: 1150.0,
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
            cycle_efficiency: 1150.0 / 3411.0,
            house_load_mwe: 45.0,
            rod_speed_max: 72.0 / 60.0,
            rod_max_steps: 228.0,
            rod_overlap: 100.0,
            scram_time: 2.2,
        }
    }
}

const PZR_PORV_OPEN: f64 = 16.2 * MPA;
const PZR_PORV_CLOSE: f64 = 15.9 * MPA;
const PZR_SAFETY: f64 = 17.2 * MPA;
const SG_ARV: f64 = 7.85 * MPA;
const SG_SAFETY: f64 = 8.3 * MPA;
/// Physics step [s].
pub const DT: f64 = 0.05;

/// Complete PWR plant state.
#[derive(Debug, Clone)]
pub struct Pwr {
    pub p: PwrParams,
    pub name: String,
    pub time: f64,
    pub training_mode: bool,
    rng: Rng,

    // Neutronics
    pub kinetics: PointKinetics,
    pub poisons: Poisons,
    pub decay: DecayHeat,
    pub banks: Vec<RodBank>,
    pub boron_ppm: f64,
    pub boration: BorationMode,

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
    pub breaker: bool,
    pub turbine_tripped: bool,

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
    sync: ctl::GeneratorSync,

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
    /// Plant at equilibrium 100 % power with all supported automation in AUTO.
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
        let feed = w_steam / (p.feed_max_frac * w_steam);

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
            time: 0.0,
            training_mode: false,
            rng: Rng::new(0x5EED),
            kinetics: PointKinetics::equilibrium(DelayedNeutronData::u235_thermal(), 1.0),
            poisons: Poisons::equilibrium(3.0e13, -0.028, -0.007, 1.0),
            decay: DecayHeat::equilibrium(1.0),
            banks,
            boron_ppm: 0.0,
            boration: BorationMode::Off,
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
            breaker: true,
            turbine_tripped: false,
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
            sync: ctl::GeneratorSync::default(),
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

        // Pressurizer heaters balance ambient losses.
        plant.heaters = plant.p.pzr_loss / plant.p.heater_power;

        // Critical boron concentration.
        plant.update_reactivity();
        plant.boron_ppm = (plant.rho.total) / -plant.p.boron_worth;
        plant.update_reactivity();

        // Turbine valve coefficient is implied by the full power state.
        plant.gross_mwe = plant.p.gross_mwe;
        plant.gov.track(plant.turbine_valve, plant.gross_mwe);
        plant
            .automation
            .setpoint_set(ModuleId::TurbineGovernor, "target_mwe", plant.p.gross_mwe);
        plant
            .automation
            .setpoint_set(ModuleId::PlantMaster, "target_mwe", plant.p.gross_mwe);
        plant.rod_ctl.reset(1.0);
        plant.pzr_p_ctl.track(plant.heaters);
        plant.pzr_l_ctl.track(plant.charging, plant.letdown);
        plant.fw_ctl.track(
            plant.feed,
            plant.feed / (1.0 / plant.p.feed_max_frac) * (1.0 / plant.p.feed_max_frac),
        );

        for m in [
            ModuleId::RodControl,
            ModuleId::PressurizerPressure,
            ModuleId::PressurizerLevel,
            ModuleId::FeedwaterLevel,
            ModuleId::SteamDump,
            ModuleId::TurbineGovernor,
            ModuleId::GeneratorSync,
        ] {
            plant.automation.modes.insert(m, ModuleMode::Auto);
        }
        plant.init_sensors();
        plant
    }

    fn turbine_cv(&self) -> f64 {
        let ps = steam::psat(self.p.steam_temp_full);
        let w = self.p.thermal_power / (steam::hg(ps) - steam::h_liquid(self.p.feed_temp_full));
        w / (self.p.turbine_valve_full * ps)
    }

    fn steam_nominal(&self) -> f64 {
        self.turbine_cv() * self.p.turbine_valve_full * steam::psat(self.p.steam_temp_full)
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

        let mut w_turb = if self.turbine_tripped {
            0.0
        } else {
            self.turbine_valve * cv * ps
        };
        if !self.breaker {
            // Unloaded turbine only passes rolling steam.
            w_turb = w_turb.min(0.05 * w_nom);
        }
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

        self.gross_mwe = if self.breaker && !self.turbine_tripped {
            w_turb * (steam::hg(ps) - h_fw) * p.cycle_efficiency / 1e6
        } else {
            0.0
        };
    }

    // ----------------------------------------------------------- instruments

    fn true_values(&self) -> BTreeMap<&'static str, f64> {
        let mut m = BTreeMap::new();
        let dt_core = self.t_hot - self.t_cold;
        let dt_full = self.p.thermal_power / (self.p.flow_nominal * self.p.cp_primary);
        m.insert("nuclear_power_pct", self.kinetics.n * 100.0);
        m.insert(
            "thermal_power_mw",
            self.thermal_fraction() * self.p.thermal_power / 1e6,
        );
        m.insert("startup_rate_dpm", 26.06 / self.kinetics.period);
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
        m
    }

    fn init_sensors(&mut self) {
        let truth = self.true_values();
        let spec: &[(&str, f64, f64, (f64, f64))] = &[
            ("nuclear_power_pct", 0.2, 0.15, (0.0, 125.0)),
            ("thermal_power_mw", 5.0, 3.0, (0.0, 4500.0)),
            ("startup_rate_dpm", 1.0, 0.02, (-1.0, 10.0)),
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
            ("steam_flow_kg_s", 1.0, 5.0, (0.0, 2500.0)),
            ("feed_flow_kg_s", 1.0, 5.0, (0.0, 2500.0)),
            ("feed_demand_pct", 0.0, 0.0, (0.0, 100.0)),
            ("feed_temp_c", 5.0, 0.2, (0.0, 300.0)),
            ("turbine_valve_pct", 0.0, 0.0, (0.0, 100.0)),
            ("steam_dump_pct", 0.0, 0.0, (0.0, 100.0)),
            ("gross_mwe", 0.5, 1.0, (0.0, 1400.0)),
            ("net_mwe", 0.5, 1.0, (-100.0, 1400.0)),
            ("reactivity_pcm", 1.0, 0.5, (-20000.0, 1000.0)),
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
        for (tag, s) in self.sensors.iter_mut() {
            let t = truth.get(tag).copied().unwrap_or(0.0);
            let t = if t.is_finite() { t } else { 0.0 };
            self.readings.insert(tag, s.update(t, dt, &mut self.rng));
        }
    }

    fn r(&self, tag: &str) -> f64 {
        self.readings.get(tag).copied().unwrap_or(0.0)
    }

    // ------------------------------------------------------------ automation

    fn run_automation(&mut self, dt: f64) {
        let auto = |s: &Self, m: ModuleId| s.automation.mode(m) == ModuleMode::Auto;
        let tavg = self.r("tavg_c");
        let tref = self.tref();
        let pressure = self.r("rcs_pressure_mpa");
        let w_nom = self.steam_nominal();

        // Plant master sets the governor target.
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

        if auto(self, ModuleId::TurbineGovernor) && !self.turbine_tripped {
            let target = self
                .automation
                .setpoint(ModuleId::TurbineGovernor, "target_mwe")
                .unwrap_or(0.0);
            let ramp = self
                .automation
                .setpoint(ModuleId::TurbineGovernor, "ramp_mw_min")
                .unwrap_or(30.0);
            if self.breaker {
                self.turbine_valve = self.gov.update(target, ramp, self.r("gross_mwe"), dt);
            } else {
                // Roll the turbine for synchronisation.
                self.turbine_valve = 0.03;
                self.gov.track(0.03, 0.0);
            }
        }

        if auto(self, ModuleId::GeneratorSync) && !self.breaker && !self.turbine_tripped {
            let rolling = self.turbine_valve > 0.01 && self.steam_pressure() > 5.0 * MPA;
            if self.sync.update(rolling, dt) {
                self.breaker = true;
                self.events
                    .push(PlantEvent::info("Generator synchronised to grid"));
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
            self.steam_dump = self
                .dump_ctl
                .update(tavg, tref, noload, self.reactor_tripped, dt);
        }
    }

    fn run_protection(&mut self) {
        let inputs = RpsInputs {
            flux_pct: self.r("nuclear_power_pct"),
            pressure_mpa: self.r("rcs_pressure_mpa"),
            pzr_level_pct: self.r("pzr_level_pct"),
            flow_pct: self.r("rcs_flow_pct"),
            min_sg_level_pct: self.r("sg_level_pct"),
            delta_t_pct: self.r("delta_t_pct"),
            startup_rate_dpm: self.r("startup_rate_dpm"),
            turbine_tripped: self.turbine_tripped,
        };
        let signals = self.rps.evaluate(&inputs);
        if self.training_mode {
            return;
        }
        for s in signals {
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
        self.trip_turbine();
    }

    fn trip_turbine(&mut self) {
        if !self.turbine_tripped {
            self.turbine_tripped = true;
            self.turbine_valve = 0.0;
            self.breaker = false;
            self.events.push(PlantEvent::warn("Turbine tripped"));
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
                    return deny("turbine is tripped; reset first");
                }
                self.turbine_valve = clamp(*fraction, 0.0, 1.0);
            }
            Command::SetGeneratorBreaker { closed } => {
                if *closed && self.turbine_tripped {
                    return deny("turbine is tripped");
                }
                self.breaker = *closed;
            }
            Command::Scram => {
                self.trips.push("Manual reactor trip".into());
                self.events.push(PlantEvent::trip("Manual reactor trip"));
                self.trip_reactor();
            }
            Command::ResetTrip => {
                let inputs_ok =
                    self.banks.iter().all(|b| b.position <= 0.0) || !self.reactor_tripped;
                if !inputs_ok {
                    return deny("rods still inserting");
                }
                self.reactor_tripped = false;
                self.turbine_tripped = false;
                self.trips.clear();
                for b in &mut self.banks {
                    b.target = b.position;
                }
                self.events.push(PlantEvent::info("Reactor trip reset"));
            }
            Command::SetModuleMode { module, mode } => self.set_module_mode(*module, *mode),
            Command::SetSetpoint { module, key, value } => {
                if self.automation.setpoint(*module, key).is_none() {
                    return deny("unknown setpoint for this module");
                }
                self.automation.setpoint_set(*module, key, *value);
                if *module == ModuleId::PlantMaster && key == "target_mwe" {
                    // nothing else to do: plant master pushes it to the governor
                }
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
        }
        Ok(())
    }

    fn check_actuator(&self, actuator: Actuator, origin: &Origin) -> Result<(), Denied> {
        let Some(owner) = modules::owner_of(actuator) else {
            return Ok(());
        };
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
                    self.gov.track(self.turbine_valve, self.gross_mwe);
                    self.automation.setpoint_set(
                        ModuleId::TurbineGovernor,
                        "target_mwe",
                        self.gross_mwe,
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
            // Hold rods where they are.
            if module == ModuleId::RodControl {
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
        let mut add = |tag: &str, text: &str, sev: Severity| {
            a.push(Alarm {
                tag: tag.into(),
                text: text.into(),
                severity: sev,
            })
        };
        if self.reactor_tripped {
            add("rx_trip", "REACTOR TRIP", Severity::Trip);
        }
        if self.turbine_tripped {
            add("turb_trip", "TURBINE TRIP", Severity::Alarm);
        }
        let tavg = self.r("tavg_c");
        if (tavg - self.tref()).abs() > 3.0 && !self.reactor_tripped {
            add("tavg_dev", "Tavg / Tref deviation", Severity::Warning);
        }
        let p = self.r("rcs_pressure_mpa");
        if p > 15.9 {
            add("p_high", "Pressurizer pressure high", Severity::Alarm);
        }
        if p < 15.1 {
            add("p_low", "Pressurizer pressure low", Severity::Alarm);
        }
        if self.porv_open {
            add("porv", "PORV open", Severity::Alarm);
        }
        let lvl = self.r("pzr_level_pct");
        if (lvl - self.pzr_level_program(tavg)).abs() > 5.0 {
            add(
                "pzr_lvl_dev",
                "Pressurizer level deviation",
                Severity::Warning,
            );
        }
        let sg = self.r("sg_level_pct");
        if !(38.0..=62.0).contains(&sg) {
            add("sg_lvl_dev", "SG level deviation", Severity::Warning);
        }
        if self.r("startup_rate_dpm") > 1.0 {
            add("sur", "High startup rate", Severity::Warning);
        }
        if !self.reactor_tripped && self.banks[5].position < 30.0 && self.banks[4].position < 128.0
        {
            add("rod_ins", "Rod insertion limit", Severity::Alarm);
        }
        if self.rcp_running.iter().any(|r| !r) {
            add("rcp", "Reactor coolant pump stopped", Severity::Warning);
        }
        if self.training_mode {
            add(
                "training",
                "TRAINING MODE: protection bypassed",
                Severity::Info,
            );
        }
        for (tag, s) in &self.sensors {
            if s.fault != SensorFault::None {
                add(tag, &format!("Instrument fault: {tag}"), Severity::Warning);
            }
        }
        a
    }

    pub fn telemetry(&self, time_scale: f64, paused: bool) -> Telemetry {
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
            coolant_pumps: self.rcp_running.to_vec(),
            generator_breaker: self.breaker,
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
                    owned: self.policy_context().owns(m.id),
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
        put("rho_rods_pcm", self.rho.rods * 1e5);
        put("rho_boron_pcm", self.rho.boron * 1e5);
        put("rho_doppler_pcm", self.rho.doppler * 1e5);
        put("rho_moderator_pcm", self.rho.moderator * 1e5);
        put("rho_xenon_pcm", self.rho.xenon * 1e5);
        put("rho_samarium_pcm", self.rho.samarium * 1e5);
        put("q_sg_mw", self.q_sg / 1e6);
        m
    }

    pub fn game_mode(&self) -> GameMode {
        self.automation.game_mode
    }
}
