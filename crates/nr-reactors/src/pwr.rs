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
use nr_api::equipment::{Controller, Equipment, Slot, UnitSize};
use nr_api::{
    authorize, modules, Actuator, Alarm, BorationMode, Command, CommandKind, ControlRoomGeneration,
    Denied, GameMode, ModuleId, ModuleMode, ModuleView, NisBlock, NisView, Origin, PolicyContext,
    RodDirection, RodView, Severity, Telemetry, UpgradeId, UpgradeView, API_VERSION,
};
use nr_control::controllers::{self as ctl, overlap_sequence};
use nr_control::{Rps, RpsInputs};
use nr_core::{clamp, lerp, steam, Rng, MPA};
use nr_physics::turbine_generator::RATED_RPM;
use nr_physics::{
    Breaker, CloseOutcome, Condenser, Deaerator, DecayHeat, DelayedNeutronData, DieselState,
    Electrical, LoadDemand, PointKinetics, Poisons, RodBank, Sensor, SensorFault, TurbineGenerator,
};
use std::collections::BTreeMap;

/// Design parameters.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
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

impl PwrParams {
    /// Design parameters for a unit size: extensive quantities (power, flow,
    /// volumes, heat capacities, pump and heater capacities, house load)
    /// scale with rated thermal power; temperatures, pressures and
    /// reactivity coefficients stay the same.
    pub fn for_size(size: UnitSize) -> Self {
        let mut p = Self::default();
        let s = size.scale();
        p.thermal_power = size.thermal_mw() * 1e6;
        p.gross_mwe = size.mwe();
        p.generator_rating = size.mwe() * 1.13e6;
        p.flow_nominal *= s;
        p.vol_core *= s;
        p.vol_hot *= s;
        p.vol_sg *= s;
        p.vol_cold *= s;
        p.vol_pzr *= s;
        p.fuel_heat_capacity *= s;
        p.heater_power *= s;
        p.pzr_loss *= s;
        p.pzr_capacitance *= s;
        p.spray_max *= s;
        p.letdown_nominal *= s;
        p.charging_max *= s;
        p.sg_mass_nominal *= s;
        p.sg_capacitance *= s;
        p.house_load_mwe *= s;
        p
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

/// Instrument tags: (tag, lag [s], noise sd, range).
const SENSOR_SPEC: &[(&str, f64, f64, (f64, f64))] = &[
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
    ("cet_avg_c", 3.0, 0.1, (0.0, 1200.0)),
    ("cet_max_c", 3.0, 0.1, (0.0, 1200.0)),
    ("cet_q1_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_q2_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_q3_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_q4_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_bank_a_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_bank_b_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_bank_c_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_bank_d_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_bank_sa_c", 3.0, 0.15, (0.0, 1200.0)),
    ("cet_bank_sb_c", 3.0, 0.15, (0.0, 1200.0)),
    ("fuel_avg_c", 5.0, 0.3, (0.0, 2800.0)),
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
    ("gen_mw", 0.5, 2.0, (-200.0, 1600.0)),
    ("gen_mvar", 0.5, 2.0, (-1000.0, 1600.0)),
    ("gen_mva", 0.5, 2.0, (0.0, 1800.0)),
    ("gen_pf", 0.5, 0.002, (0.0, 1.0)),
    ("gen_stator_ka", 0.5, 0.05, (0.0, 60.0)),
    ("field_amps", 0.5, 5.0, (0.0, 12000.0)),
    ("switchyard_kv", 0.5, 0.3, (0.0, 400.0)),
    ("bus_a_kv", 0.2, 0.01, (0.0, 8.0)),
    ("bus_b_kv", 0.2, 0.01, (0.0, 8.0)),
    ("safety1_kv", 0.2, 0.01, (0.0, 5.0)),
    ("safety2_kv", 0.2, 0.01, (0.0, 5.0)),
    ("generator_health_pct", 0.0, 0.0, (0.0, 100.0)),
    ("turbine_vibration_um", 1.0, 1.5, (0.0, 300.0)),
    ("power_ref_pct", 0.0, 0.0, (0.0, 120.0)),
    ("house_load_mw", 0.5, 0.2, (0.0, 200.0)),
    ("max_gross_mwe", 0.0, 0.0, (0.0, 2000.0)),
    ("load_limit_mwe", 0.0, 0.0, (0.0, 2000.0)),
    ("sut_load_mw", 0.3, 0.2, (0.0, 200.0)),
    ("uat_load_mw", 0.3, 0.2, (0.0, 200.0)),
    ("grid_import_mw", 0.5, 0.1, (0.0, 200.0)),
    ("edg_load_mw", 0.5, 0.05, (0.0, 20.0)),
    ("battery_pct", 0.0, 0.0, (0.0, 100.0)),
    ("dc_volts", 0.5, 0.2, (0.0, 140.0)),
    ("edg1_fuel_pct", 0.0, 0.0, (0.0, 100.0)),
    ("edg2_fuel_pct", 0.0, 0.0, (0.0, 100.0)),
    ("condenser_kpa", 2.0, 0.1, (0.0, 110.0)),
    ("condensing_temp_c", 5.0, 0.2, (0.0, 120.0)),
    ("cw_flow_pct", 1.0, 0.5, (0.0, 120.0)),
    ("hotwell_level_pct", 2.0, 0.3, (0.0, 100.0)),
    ("da_pressure_mpa", 2.0, 0.003, (0.0, 1.5)),
    ("da_temp_c", 5.0, 0.3, (0.0, 200.0)),
];

/// Engineering units of one unit of controller imprecision per module.
fn bias_scale(m: ModuleId) -> f64 {
    match m {
        ModuleId::ReactorPower => 0.5,
        ModuleId::RodControl => 1.0,
        ModuleId::PressurizerPressure => 0.08,
        ModuleId::PressurizerLevel => 3.0,
        ModuleId::FeedwaterLevel => 4.0,
        ModuleId::SteamDump => 1.0,
        ModuleId::TurbineGovernor => 10.0,
        ModuleId::Excitation => 0.3,
        ModuleId::BoronControl => 8.0,
        _ => 0.0,
    }
}

/// "D2" -> ("D", 2): a single rod group selection.
fn parse_group(sel: &str) -> Option<(&str, u8)> {
    let (bank, g) = sel.split_at(sel.len().checked_sub(1)?);
    match g {
        "1" if !bank.is_empty() => Some((bank, 1)),
        "2" if !bank.is_empty() => Some((bank, 2)),
        _ => None,
    }
}

/// Nuclear instrumentation trip blocks.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct NisState {
    pub sr_blocked: bool,
    pub ir_blocked: bool,
    pub pr_low_blocked: bool,
}

/// Complete PWR plant state.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Pwr {
    pub p: PwrParams,
    pub name: String,
    pub scenario: String,
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
    pub rod_selection: String,
    pub rod_direction: RodDirection,
    /// Manual rod drive speed [steps/min] (rod speed selector).
    pub rod_spm: f64,
    /// Rod balancer currently realigning each bank.
    balancing: [bool; 6],
    /// Ramped reactor power reference of the reactor power controller [%].
    pub power_ref: f64,
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
    /// Letdown isolated (and heaters cut off) on low pressurizer level.
    pub letdown_isolated: bool,
    pub porv_open: bool,

    // Secondary
    pub t_steam: f64,
    pub sg_mass: f64,
    pub feed: f64,
    pub t_feed: f64,
    pub turbine_valve: f64,
    pub steam_dump: f64,
    pub turbine_tripped: bool,
    pub turbine_trip_reason: Option<String>,
    pub condenser: Condenser,
    pub deaerator: Deaerator,
    pub msiv_open: bool,
    /// Station electrical distribution.
    pub elec: Electrical,
    /// The generator breaker was opened deliberately (not by a trip).
    pub breaker_opened_by_operator: bool,
    reverse_power_s: f64,
    /// Turbine hall local checks done (comms); cleared by a turbine trip.
    pub turbine_hall_ready: bool,
    /// Grid dispatch permission to synchronise (comms).
    pub grid_permission: bool,
    vac_required_until: f64,
    rod_block_until: f64,
    /// When the safety AC buses were lost (vital AC transfer timing).
    #[serde(default)]
    vital_ac_lost_at: Option<f64>,
    /// Overpower turbine runback in progress.
    #[serde(default)]
    pub runback: bool,
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

    /// Slowly wandering setpoint error of each automation controller
    /// (analog controllers wander more than digital ones).
    pub(crate) ctl_bias: BTreeMap<ModuleId, f64>,

    // Instruments
    #[serde(skip)]
    pub sensors: BTreeMap<&'static str, Sensor>,
    #[serde(skip)]
    pub(crate) readings: BTreeMap<&'static str, f64>,
    #[serde(skip)]
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
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
        Self::full_power_sized(UnitSize::default())
    }

    /// Full power for a unit size, with every component sized to match.
    pub fn full_power_sized(size: UnitSize) -> Self {
        let p = PwrParams::for_size(size);
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
            rng: Rng::new(0x5EED),
            kinetics: PointKinetics::equilibrium(DelayedNeutronData::u235_thermal(), 1.0),
            poisons: Poisons::equilibrium(3.0e13, -0.028, -0.007, 1.0),
            decay: DecayHeat::equilibrium(1.0),
            banks,
            boron_ppm: 0.0,
            boration: BorationMode::Off,
            rod_selection: "manual".into(),
            rod_direction: RodDirection::Hold,
            rod_spm: 48.0,
            balancing: [false; 6],
            power_ref: 100.0,
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
            letdown_isolated: false,
            porv_open: false,
            t_steam: p.steam_temp_full,
            sg_mass: p.sg_mass_nominal,
            feed,
            t_feed,
            turbine_valve: p.turbine_valve_full,
            steam_dump: 0.0,
            turbine_tripped: false,
            turbine_trip_reason: None,
            condenser: Condenser::at_power(),
            deaerator: Deaerator::at_power(),
            msiv_open: true,
            elec: Electrical::normal(),
            breaker_opened_by_operator: false,
            reverse_power_s: 0.0,
            turbine_hall_ready: true,
            grid_permission: true,
            vac_required_until: -1.0,
            rod_block_until: -1.0,
            runback: false,
            vital_ac_lost_at: None,
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
            ctl_bias: BTreeMap::new(),
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
            ModuleId::CondenserVacuum,
            ModuleId::Deaerator,
        ] {
            plant.automation.modes.insert(m, ModuleMode::Auto);
        }
        plant.automation.equipment = nr_api::Equipment::standard_sized(size);
        plant.apply_size();
        let d = plant.load_demand();
        plant.elec.settle(&d);
        plant.elec.step(&d, plant.tg.breaker, 0.0);
        plant.init_sensors();
        plant
    }

    /// Hot standby a week after shutdown: no-load temperature and pressure,
    /// all rods inserted, xenon decayed, turbine on turning gear. The
    /// operator must take the reactor critical on the source range and
    /// work up through the intermediate and power ranges.
    pub fn hot_standby() -> Self {
        Self::hot_standby_sized(UnitSize::default())
    }

    /// Hot standby for a unit size.
    pub fn hot_standby_sized(size: UnitSize) -> Self {
        let mut plant = Self::full_power_sized(size);
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
        plant.turbine_trip_reason = None;
        plant.condenser = Condenser::at_atmosphere();
        plant.deaerator = Deaerator::cold();
        plant.turbine_hall_ready = false;
        plant.grid_permission = false;
        plant.rod_selection = "SA".into();
        plant.elec = Electrical::shutdown();
        plant.apply_size();
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
        let d = plant.load_demand();
        plant.elec.settle(&d);
        plant.elec.step(&d, plant.tg.breaker, 0.0);
        plant.init_sensors();
        plant
    }

    pub fn from_scenario(name: &str) -> Option<Self> {
        Self::from_scenario_sized(name, UnitSize::default())
    }

    pub fn from_scenario_sized(name: &str, size: UnitSize) -> Option<Self> {
        match name {
            "full_power" => Some(Self::full_power_sized(size)),
            "hot_standby" => Some(Self::hot_standby_sized(size)),
            _ => None,
        }
    }

    /// Unit size (the installed reactor core).
    pub fn size(&self) -> UnitSize {
        self.automation.equipment.unit_size()
    }

    /// Size-dependent settings of sub-systems (electrical loads, ratings).
    fn apply_size(&mut self) {
        let s = self.size().scale();
        self.elec.scale = s;
        self.elec.sut_rating = 80.0 * s;
        self.elec.uat_rating = 80.0 * s;
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
        let e = &self.automation.equipment;
        let quality = e.perf(Slot::Turbine)
            * e.perf(Slot::Generator).powf(0.3)
            * e.perf(Slot::Condenser).powf(0.5);
        (self.p.gross_mwe * 1e6 + tg.losses()) / self.p.thermal_power * quality
    }

    fn feed_max(&self) -> f64 {
        let pumps = (self.elec.buses.bus_a as u8 + self.elec.buses.bus_b as u8) as f64 / 2.0;
        self.p.feed_max_frac * self.automation.equipment.capacity(Slot::FeedPumps) * pumps
    }

    /// Controller setpoint error for a module, in engineering units.
    fn bias(&self, m: ModuleId) -> f64 {
        self.ctl_bias.get(&m).copied().unwrap_or(0.0) * bias_scale(m)
    }

    fn step_biases(&mut self, dt: f64) {
        let tau: f64 = 300.0;
        for (m, c) in self.automation.equipment.controllers.clone() {
            let sd = c.imprecision();
            let b = self.ctl_bias.entry(m).or_insert(0.0);
            *b += -*b / tau * dt + sd * (2.0 * dt / tau).sqrt() * self.rng.normal();
        }
    }

    /// Re-derive equipment-dependent parameters (instrument noise).
    pub fn apply_equipment(&mut self) {
        let mult = (1.0 / self.automation.equipment.perf(Slot::BoardInstruments)).powi(8);
        for (tag, _, noise, _) in SENSOR_SPEC {
            if let Some(s) = self.sensors.get_mut(tag) {
                s.noise = noise * mult;
            }
        }
    }

    pub(crate) fn loop_mass(&self) -> f64 {
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
        let hw = &self.automation.upgrades;
        (hw.contains(&UpgradeId::GeneratorSyncInterface)
            && hw.contains(&UpgradeId::TurbineControlComputer))
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
        self.overpower_runback(dt);
        self.apply_output_limit();
        self.run_protection();
        self.pzr_low_level_interlock();
        self.step_electrical(dt);
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

    fn load_demand(&self) -> LoadDemand {
        LoadDemand {
            rcps: self.rcp_running,
            cw_pumps: self.condenser.cw_pumps,
            vacuum_pumps: self.condenser.vacuum_pumps,
            feed: self.feed,
            pzr_heaters: self.heaters,
            charging: self.charging > 0.5,
        }
    }

    /// Automatic bus transfer & load restoration (module in AUTO).
    fn auto_bus_transfer(&mut self) {
        let e = &mut self.elec;
        let offsite = e.grid_available && !e.sut_tripped;
        let uat_ok = self.tg.breaker && !e.uat_tripped;
        // Offsite power back: re-energise the 6.9 kV buses from the SUT.
        if offsite {
            if !e.closed(Breaker::Sut) {
                e.set_breaker(Breaker::Sut, true);
                self.events.push(PlantEvent::info(
                    "Auto transfer: startup transformer energised",
                ));
            }
            let a_fed = e.closed(Breaker::BusASut) || (uat_ok && e.closed(Breaker::BusAUat));
            if !a_fed {
                e.set_breaker(Breaker::BusAUat, false);
                e.set_breaker(Breaker::BusASut, true);
                self.events.push(PlantEvent::info(
                    "Auto transfer: bus A restored from startup transformer",
                ));
            }
            let b_fed = e.closed(Breaker::TieAB) || (uat_ok && e.closed(Breaker::BusBUat));
            if !b_fed {
                e.set_breaker(Breaker::BusBUat, false);
                e.set_breaker(Breaker::TieAB, true);
                self.events.push(PlantEvent::info(
                    "Auto transfer: bus B restored through the A-B tie",
                ));
            }
        }
        // Safety buses back to their normal feeds; diesels unloaded and stopped.
        let (bus_a, bus_b) = (e.buses.bus_a, e.buses.bus_b);
        for (feed, edg, live, i) in [
            (Breaker::Safety1Feed, Breaker::Edg1, bus_a, 0),
            (Breaker::Safety2Feed, Breaker::Edg2, bus_b, 1),
        ] {
            if live && (!e.closed(feed) || e.closed(edg)) {
                if e.closed(edg) {
                    e.set_breaker(edg, false);
                    e.stop_diesel(i);
                }
                e.set_breaker(feed, true);
                self.events.push(PlantEvent::info(if i == 0 {
                    "Auto transfer: safety bus 1 back on normal feed"
                } else {
                    "Auto transfer: safety bus 2 back on normal feed"
                }));
            }
        }
        // Unit loaded: house load from the startup to the unit auxiliary transformer.
        if uat_ok && self.gross_mwe > 0.15 * self.p.gross_mwe && e.closed(Breaker::BusASut) {
            e.set_breaker(Breaker::BusAUat, true);
            e.set_breaker(Breaker::BusASut, false);
            e.set_breaker(Breaker::BusBUat, true);
            e.set_breaker(Breaker::TieAB, false);
            self.events.push(PlantEvent::info(
                "Auto transfer: house load transferred to the unit auxiliary transformer",
            ));
        }
    }

    /// Battery-backed vital AC transfer gap [s] (None: no battery-backed
    /// supply).
    fn vital_ac_gap(&self) -> Option<f64> {
        self.automation
            .equipment
            .components
            .get(&Slot::VitalAc)
            .map(nr_api::equipment::vital_ac_gap)
            .unwrap_or(Some(0.3))
    }

    /// Annunciators and indicating lamps: the vital instrument bus, fed
    /// from the safety buses, or from the battery through the vital AC
    /// supply once it has picked up (MG set / inverter transfer time).
    pub fn indication_power(&self) -> bool {
        let b = &self.elec.buses;
        if !b.dc {
            return false;
        }
        if b.safety1 || b.safety2 {
            return true;
        }
        match (self.vital_ac_gap(), self.vital_ac_lost_at) {
            (Some(gap), Some(lost)) => self.time - lost >= gap,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// Is 6.9 kV bus A (false) or B (true) energised?
    fn bus_live(&self, bus_b: bool) -> bool {
        if bus_b {
            self.elec.buses.bus_b
        } else {
            self.elec.buses.bus_a
        }
    }

    /// Station service load drawn by the plant's own equipment [MW].
    pub fn house_load(&self) -> f64 {
        self.elec.house_load_mw
    }

    /// Low pressurizer level (17 %): letdown isolation valves close and the
    /// heaters are cut off so they are not uncovered. Letdown stays isolated
    /// until it is re-established (by the operator, or by the level control
    /// module in AUTO once level has recovered).
    fn pzr_low_level_interlock(&mut self) {
        let lvl = self.r("pzr_level_pct");
        if lvl < 17.0 {
            if !self.letdown_isolated {
                self.letdown_isolated = true;
                self.events.push(PlantEvent::warn(
                    "PZR level low (17 %): letdown isolated, pressurizer heaters cut off",
                ));
            }
            self.heaters = 0.0;
        }
        if self.letdown_isolated {
            let auto_restore = self.automation.mode(ModuleId::PressurizerLevel) == ModuleMode::Auto
                && self.module_usable(ModuleId::PressurizerLevel)
                && lvl > 25.0;
            if auto_restore {
                self.letdown_isolated = false;
                self.letdown = self.p.letdown_nominal;
                self.events.push(PlantEvent::info(
                    "Letdown re-established (level control in AUTO)",
                ));
            } else {
                self.letdown = 0.0;
            }
        }
    }

    fn step_electrical(&mut self, dt: f64) {
        let eq = &self.automation.equipment;
        self.elec.transformer_quality = eq.perf(Slot::StationTransformers)
            * eq.size_ratio(Slot::StationTransformers).min(1.0).cbrt();
        self.elec.diesel_quality = eq.perf(Slot::EmergencyDiesels);
        // Diesel load sequencer: start on safety bus undervoltage, close the
        // output breaker once the diesel is up to speed.
        if self.automation.mode(ModuleId::DieselSequencer) == ModuleMode::Auto
            && self.module_usable(ModuleId::DieselSequencer)
        {
            for i in 0..2 {
                let bus_live = if i == 0 {
                    self.elec.buses.safety1
                } else {
                    self.elec.buses.safety2
                };
                let br = if i == 0 { Breaker::Edg1 } else { Breaker::Edg2 };
                if !bus_live && self.elec.diesels[i].state == DieselState::Stopped {
                    self.elec.start_diesel(i);
                }
                if !bus_live && self.elec.diesels[i].running() && !self.elec.closed(br) {
                    // Shed the bus from its dead normal feed before loading
                    // the diesel onto it.
                    let feed = if i == 0 {
                        Breaker::Safety1Feed
                    } else {
                        Breaker::Safety2Feed
                    };
                    self.elec.set_breaker(feed, false);
                    self.elec.set_breaker(br, true);
                }
            }
        }
        let safety_live = self.elec.buses.safety1 || self.elec.buses.safety2;
        if safety_live {
            self.vital_ac_lost_at = None;
        } else if self.vital_ac_lost_at.is_none() {
            self.vital_ac_lost_at = Some(self.time);
        }
        if self.tg.breaker {
            self.breaker_opened_by_operator = false;
            // Anti-motoring (reverse power) relay: a motoring generator is
            // tripped off the grid after a short time delay.
            if self.tg.net_power < 0.0 {
                self.reverse_power_s += dt;
                if self.reverse_power_s > 5.0 {
                    self.tg.open();
                    self.events
                        .push(PlantEvent::trip("Generator reverse power trip (32G)"));
                }
            } else {
                self.reverse_power_s = 0.0;
            }
        } else {
            self.reverse_power_s = 0.0;
        }
        if let Some(c) = self
            .automation
            .equipment
            .components
            .get(&Slot::StationBatteries)
        {
            self.elec.battery_hours = nr_api::equipment::battery_hours(c);
        }
        if self.automation.mode(ModuleId::BusTransfer) == ModuleMode::Auto
            && self.module_usable(ModuleId::BusTransfer)
        {
            self.auto_bus_transfer();
        }
        let demand = self.load_demand();
        let events = self.elec.step(&demand, self.tg.breaker, dt);
        for e in events {
            if e.contains("trip") || e.contains("fuel") {
                self.events.push(PlantEvent::trip(e));
            } else {
                self.events.push(PlantEvent::info(e));
            }
        }
        let b = self.elec.buses.clone();
        // Loads on dead buses stop.
        for i in 0..4 {
            let live = if i < 2 { b.bus_a } else { b.bus_b };
            if self.rcp_running[i] && !live {
                self.rcp_running[i] = false;
                self.events.push(PlantEvent::trip(&format!(
                    "RCP {} tripped: 6.9 kV bus de-energised",
                    i + 1
                )));
            }
        }
        for i in 0..3 {
            let live = if i < 2 { b.bus_a } else { b.bus_b };
            if self.condenser.cw_pumps[i] && !live {
                self.condenser.cw_pumps[i] = false;
            }
        }
        if !b.bus_a {
            self.condenser.vacuum_pumps[0] = false;
        }
        if !b.bus_b {
            self.condenser.vacuum_pumps[1] = false;
        }
        if !b.safety1 {
            self.heaters = 0.0;
        }
        if !b.safety1 && !b.safety2 {
            self.charging = 0.0;
        }
        // Grid lost: the generator is islanded and its breaker opens.
        if !self.elec.grid_available && self.tg.breaker {
            self.tg.open();
            self.events.push(PlantEvent::trip(
                "Loss of offsite power: generator breaker opened",
            ));
        }
        // RPS and breaker trip coils are fail-safe on loss of DC.
        if !b.dc && !self.reactor_tripped && !self.training_mode {
            self.trips.push("Loss of DC control power".into());
            self.events
                .push(PlantEvent::trip("Loss of DC control power"));
            self.trip_reactor();
        }
    }

    fn step_rods(&mut self, dt: f64) {
        if self.reactor_tripped {
            for b in &mut self.banks {
                b.drop(self.p.scram_time, dt);
            }
            return;
        }
        // Rod drive power cabinets sized for a smaller core cannot step the
        // extra rods: no rod motion (the rods still scram by gravity).
        if self.rod_drives_undersized() {
            self.rod_direction = RodDirection::Hold;
            for b in &mut self.banks {
                b.target = b.position;
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
            let speed =
                sign * self.rod_spm / 60.0 * self.automation.equipment.perf(Slot::RodDrives);
            let sel = self.rod_selection.clone();
            if sel.eq_ignore_ascii_case("manual") {
                let pos: Vec<f64> = self.banks[2..].iter().map(|b| b.position).collect();
                let d = overlap_sequence(&pos, self.p.rod_max_steps, self.p.rod_overlap, speed, dt);
                for (b, d) in self.banks[2..].iter_mut().zip(d) {
                    b.target = clamp(b.position + d, 0.0, b.max_steps);
                }
            } else {
                // One or more banks / groups selected ("D", "D2", "A1,B2,D").
                for item in sel.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    let (bank, group) = match parse_group(item) {
                        Some((b, g)) => (b, Some(g)),
                        None => (item, None),
                    };
                    if let Some(b) = self
                        .banks
                        .iter_mut()
                        .find(|b| b.name.eq_ignore_ascii_case(bank))
                    {
                        match group {
                            Some(g) => b.move_group(g, speed * dt),
                            None => b.target = clamp(b.position + speed * dt, 0.0, b.max_steps),
                        }
                    }
                }
            }
        }
        // C-2: power range high flux rod stop (103 %) blocks withdrawal,
        // manual or automatic.
        if self.c2_rod_stop() {
            for b in &mut self.banks {
                if b.target > b.position {
                    b.target = b.position;
                }
            }
        }
        let speed = self.p.rod_speed_max.max(self.rod_spm / 60.0)
            * self.automation.equipment.perf(Slot::RodDrives);
        for i in 0..self.banks.len() {
            let moved = self.banks[i].drive(speed, dt);
            // Occasional missed step on one group: groups drift apart.
            if moved.abs() > 0.0 && self.rng.uniform() < moved.abs() * 0.004 {
                let slip = if self.rng.uniform() < 0.5 { 1.0 } else { -1.0 };
                self.banks[i].move_group(2, slip);
            }
        }
        self.run_balancer(dt);
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
        let forced: f64 = self.rcp_speed.iter().sum::<f64>() / 4.0
            * self.automation.equipment.capacity(Slot::CoolantPumps);
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
        let ua = ua_full
            * (0.25 + 0.75 * self.flow_frac.powf(0.8))
            * dryout
            * self.automation.equipment.perf(Slot::SteamGenerators);
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
        let q_net =
            self.heaters * p.heater_power * self.automation.equipment.perf(Slot::Pressurizer)
                - p.pzr_loss
                - q_spray;
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
        let msiv = self.msiv_open;

        let w_turb = if self.turbine_tripped || !msiv {
            0.0
        } else {
            self.turbine_valve * cv * ps
        };
        // Steam dumps discharge to the condenser: interlocked off without
        // vacuum and circulating water (C-9).
        let dump_ok = msiv && self.condenser.available();
        let w_dump = if dump_ok {
            self.steam_dump * p.dump_capacity * w_nom * ps / steam::psat(p.steam_temp_full)
        } else {
            0.0
        };
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

        // Deaerator (extraction steam at load, pegging steam at low load) and
        // feedwater heating.
        let load = clamp(w_turb / w_nom, 0.0, 1.0);
        self.deaerator.step(load, msiv && ps > 1.0e6, dt);
        let t_feed_target =
            lerp(40.0, p.feed_temp_full, load).max(self.deaerator.temperature() - 3.0);
        self.t_feed += (t_feed_target - self.t_feed) * (dt / 60.0);
        let h_fw = steam::h_liquid(self.t_feed);
        self.feed_flow = self.feed * self.feed_max() * w_nom;

        let hf = steam::hf(ps);
        let hfg = steam::hg(ps) - hf;
        let q_out = self.feed_flow * (hf - h_fw) + self.steam_flow * hfg;
        self.t_steam += (self.q_sg - q_out) / p.sg_capacitance * dt;
        self.sg_mass = (self.sg_mass + (self.feed_flow - self.steam_flow) * dt).max(0.0);

        // Turbine-generator: output depends on condenser vacuum.
        let p_avail = w_turb * (steam::hg(ps) - h_fw);
        let p_mech = p_avail * self.shaft_efficiency() * self.condenser.efficiency();
        let h_cond = steam::h_liquid(self.condenser.condensing_c.max(33.0));
        // Heat rejected: turbine exhaust (energy not converted to shaft work;
        // feedwater heating by extraction is internal to the cycle) plus
        // steam dumped straight to the condenser.
        let q_cond = (p_avail - p_mech).max(0.0) + w_dump * (steam::hg(ps) - h_cond);
        let eq = &self.automation.equipment;
        self.condenser.capacity = eq.capacity(Slot::Condenser)
            * eq.capacity(Slot::CirculatingWater)
            * self.size().scale();
        self.condenser.air_removal = eq.perf(Slot::AirRemoval).powi(4);
        self.deaerator.quality = eq.perf(Slot::Deaerator);
        self.condenser.step(q_cond, ps, msiv, dt);
        // Hotwell: condensed steam in, condensate/feed out, auto makeup/reject.
        let condensed = w_turb + w_dump;
        self.condenser.hotwell_pct += ((condensed - self.feed_flow) / 2000.0
            + (50.0 - self.condenser.hotwell_pct) * 0.002)
            * dt;
        self.condenser.hotwell_pct = clamp(self.condenser.hotwell_pct, 0.0, 100.0);

        if !self.turbine_tripped && self.condenser.pressure_kpa > 30.0 {
            self.trip_turbine_for("Turbine low condenser vacuum trip");
        }
        if let Some(event) = self.tg.step(p_mech, dt, &mut self.rng) {
            match event {
                "Turbine overspeed" => self.trip_turbine_for("Turbine overspeed trip"),
                "Turbine high vibration" => self.trip_turbine_for("Turbine high vibration trip"),
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
        let tilt = self.quadrant_tilt();
        for (i, tag) in ["pr_n41_pct", "pr_n42_pct", "pr_n43_pct", "pr_n44_pct"]
            .iter()
            .enumerate()
        {
            m.insert(tag, n * 100.0 * (1.0 + tilt[i]));
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
        // Core exit thermocouples: average, hottest, by quadrant and by the
        // assemblies under each rod bank.
        let cet = self.core_exit_temps();
        let core = crate::core_map::core(self.size());
        let avg = |f: &dyn Fn(&crate::core_map::Assembly) -> bool| {
            let v: Vec<f64> = core
                .iter()
                .zip(&cet)
                .filter(|(a, _)| f(a))
                .map(|(_, t)| *t)
                .collect();
            v.iter().sum::<f64>() / v.len().max(1) as f64
        };
        m.insert("cet_avg_c", avg(&|_| true));
        m.insert("cet_max_c", cet.iter().cloned().fold(f64::MIN, f64::max));
        for (q, tag) in ["cet_q1_c", "cet_q2_c", "cet_q3_c", "cet_q4_c"]
            .into_iter()
            .enumerate()
        {
            m.insert(tag, avg(&|a| a.quadrant == q));
        }
        for (bank, tag) in [
            ("A", "cet_bank_a_c"),
            ("B", "cet_bank_b_c"),
            ("C", "cet_bank_c_c"),
            ("D", "cet_bank_d_c"),
            ("SA", "cet_bank_sa_c"),
            ("SB", "cet_bank_sb_c"),
        ] {
            m.insert(tag, avg(&|a| a.rcca.map(|r| r.0) == Some(bank)));
        }
        m.insert("fuel_avg_c", self.t_fuel);
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
        m.insert("net_mwe", self.gross_mwe - self.house_load());
        m.insert("max_gross_mwe", self.max_gross_mwe());
        m.insert("load_limit_mwe", self.load_limit_mwe());
        m.insert("house_load_mw", self.elec.house_load_mw);
        m.insert("sut_load_mw", self.elec.sut_load_mw);
        m.insert("uat_load_mw", self.elec.uat_load_mw);
        m.insert("grid_import_mw", self.elec.import_mw);
        m.insert("edg_load_mw", self.elec.edg_load_mw);
        m.insert("battery_pct", self.elec.battery_pct);
        m.insert("dc_volts", self.elec.dc_volts);
        m.insert(
            "edg1_fuel_pct",
            100.0 * self.elec.diesels[0].fuel_l / self.elec.diesels[0].fuel_capacity_l,
        );
        m.insert(
            "edg2_fuel_pct",
            100.0 * self.elec.diesels[1].fuel_l / self.elec.diesels[1].fuel_capacity_l,
        );
        m.insert("reactivity_pcm", self.rho.total * 1e5);
        m.insert("turbine_speed_rpm", self.tg.rpm());
        m.insert("gen_freq_hz", self.tg.gen_hz());
        m.insert("grid_freq_hz", self.tg.grid_hz);
        m.insert("sync_phase_deg", self.tg.phase);
        m.insert("sync_slip_hz", self.tg.slip_hz());
        m.insert("gen_voltage_kv", self.tg.gen_kv());
        // Reactive power from excitation: internal EMF vs grid voltage across
        // the synchronous reactance (over-excited = MVAR out to the grid).
        let rated_mva = self.tg.rating / 1e6 / 0.9;
        let p_mw = if self.tg.breaker {
            self.tg.net_power / 1e6
        } else {
            0.0
        };
        let q_mvar = if self.tg.breaker && self.tg.grid_kv > 1.0 {
            let emf =
                nr_physics::turbine_generator::RATED_KV * self.tg.speed * self.tg.excitation / 0.5;
            clamp(
                rated_mva * (emf / self.tg.grid_kv - 1.0) / 0.6,
                -0.6 * rated_mva,
                rated_mva,
            )
        } else {
            0.0
        };
        let s_mva = (p_mw * p_mw + q_mvar * q_mvar).sqrt();
        let v_kv = self.tg.gen_kv().max(1.0);
        m.insert("gen_mw", p_mw);
        m.insert("gen_mvar", q_mvar);
        m.insert("gen_mva", s_mva);
        m.insert("gen_pf", if s_mva > 1.0 { p_mw / s_mva } else { 1.0 });
        m.insert("gen_stator_ka", s_mva / (3f64.sqrt() * v_kv));
        m.insert(
            "field_amps",
            self.tg.excitation * 9_000.0 * rated_mva / 1280.0,
        );
        m.insert(
            "switchyard_kv",
            if self.elec.grid_available {
                345.0 * self.tg.grid_kv / nr_physics::turbine_generator::RATED_KV
            } else {
                0.0
            },
        );
        let sag = |load: f64, rating: f64| 1.0 - 0.04 * (load / rating.max(1.0)).min(1.5);
        let src = if self.elec.closed(Breaker::BusAUat) && self.tg.breaker {
            sag(self.elec.uat_load_mw, self.elec.uat_rating)
        } else {
            sag(self.elec.sut_load_mw, self.elec.sut_rating)
        };
        m.insert(
            "bus_a_kv",
            if self.elec.buses.bus_a {
                6.9 * src
            } else {
                0.0
            },
        );
        m.insert(
            "bus_b_kv",
            if self.elec.buses.bus_b {
                6.9 * src
            } else {
                0.0
            },
        );
        m.insert(
            "safety1_kv",
            if self.elec.buses.safety1 {
                4.16 * src
            } else {
                0.0
            },
        );
        m.insert(
            "safety2_kv",
            if self.elec.buses.safety2 {
                4.16 * src
            } else {
                0.0
            },
        );
        m.insert("grid_voltage_kv", self.tg.grid_kv);
        m.insert("excitation_pct", self.tg.excitation * 100.0);
        m.insert("generator_health_pct", self.tg.health);
        m.insert("turbine_vibration_um", self.tg.vibration());
        m.insert("power_ref_pct", self.power_ref);
        m.insert("condenser_kpa", self.condenser.pressure_kpa);
        m.insert("condensing_temp_c", self.condenser.condensing_c);
        m.insert("cw_flow_pct", self.condenser.cw_flow_frac() * 100.0);
        m.insert("hotwell_level_pct", self.condenser.hotwell_pct);
        m.insert("da_pressure_mpa", self.deaerator.pressure_mpa);
        m.insert("da_temp_c", self.deaerator.temperature());
        m
    }

    /// Rebuild state that is not saved (instruments, readings) after
    /// loading a saved plant.
    pub fn restored(mut self) -> Self {
        // Drop upgrades that are no longer sold (superseded in old saves).
        self.automation
            .upgrades
            .retain(|u| nr_api::upgrades::in_catalog(*u));
        self.init_sensors();
        self.apply_size();
        self
    }

    /// Any trip or alarm (not status lights or warnings) standing now?
    /// Returns the first one's text.
    pub fn serious_alarm(&self) -> Option<String> {
        self.alarms()
            .into_iter()
            .find(|a| matches!(a.severity, Severity::Alarm | Severity::Trip))
            .map(|a| a.text)
    }

    fn init_sensors(&mut self) {
        self.sensors.clear();
        let truth = self.true_values();
        let spec = SENSOR_SPEC;
        for (tag, tau, noise, range) in spec.iter() {
            let v = truth.get(tag).copied().unwrap_or(0.0);
            self.sensors
                .insert(tag, Sensor::new(v, *tau, *noise, *range));
        }
        self.readings = truth;
        self.apply_equipment();
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

    /// True core exit temperature of every assembly (row-major) [°C].
    pub fn core_exit_temps(&self) -> Vec<f64> {
        let core = crate::core_map::core(self.size());
        let power = crate::core_map::assembly_power(core, &self.banks, self.quadrant_tilt());
        crate::core_map::exit_temps(core, &power, self.t_cold, self.t_hot)
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
        self.step_biases(dt);

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
            // Hold = leave the turbine target where it is (never feed the
            // governor's moving load reference back as a new target: that
            // ratchets load up while steam pressure sags).
            let prev = self
                .automation
                .setpoint(ModuleId::TurbineGovernor, "target_mwe")
                .unwrap_or(target);
            // Stop the ramp where the load is, bounded by the old and new
            // targets so it can never creep past either.
            let held = self.gov.load_ref.clamp(prev.min(target), prev.max(target));
            let t = ctl::PlantMaster.update(target, held, tavg - tref, pressure - p_set);
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
                self.turbine_valve = self.gov.update(
                    target + self.bias(ModuleId::TurbineGovernor),
                    ramp,
                    self.r("gross_mwe"),
                    dt,
                );
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
                self.tg.excitation = self.avr.update(
                    sp + self.bias(ModuleId::Excitation),
                    self.r("gen_voltage_kv"),
                    dt,
                );
            } else {
                self.avr.track(self.tg.excitation);
            }
        }

        let rpc = auto(self, ModuleId::ReactorPower) && self.module_usable(ModuleId::ReactorPower);
        // An analog cabinet only has ROD mode: keep the stored mode honest
        // (covers controller swaps, career purchases and loaded saves).
        if self.rpc_analog()
            && self
                .automation
                .setpoint(ModuleId::ReactorPower, "mode")
                .unwrap_or(0.0)
                != 0.0
        {
            self.automation
                .setpoint_set(ModuleId::ReactorPower, "mode", 0.0);
        }
        let rpc_mode = if self.rpc_analog() {
            0
        } else {
            self.automation
                .setpoint(ModuleId::ReactorPower, "mode")
                .unwrap_or(0.0)
                .round() as i32
        };
        let p_now = self.r("nuclear_power_pct");
        let sur = self.r("startup_rate_dpm");
        if rpc {
            let sp = self
                .automation
                .setpoint(ModuleId::ReactorPower, "power_pct")
                .unwrap_or(10.0);
            let ramp = self
                .automation
                .setpoint(ModuleId::ReactorPower, "ramp_pct_min")
                .unwrap_or(2.0);
            let max = ramp / 60.0 * dt;
            self.power_ref += clamp(sp - self.power_ref, -max, max);
        }
        if auto(self, ModuleId::RodControl) && !self.reactor_tripped {
            let speed = if rpc {
                // Power control: rods follow the power reference (not in BORON mode),
                // never faster than a 0.7 dpm startup rate.
                let err = self.power_ref - p_now + self.bias(ModuleId::ReactorPower);
                let mut v = if rpc_mode == 1 || err.abs() < 0.3 {
                    0.0
                } else {
                    clamp(err * 0.15, -1.2, 1.2)
                };
                if v > 0.0 && sur > 0.7 {
                    v = 0.0;
                }
                if v < 0.0 && sur < -0.5 {
                    v = 0.0;
                }
                v
            } else {
                self.rod_ctl.update(
                    tavg,
                    tref + self.bias(ModuleId::RodControl),
                    p_now / 100.0,
                    self.turbine_load(),
                    dt,
                )
            };
            let pos: Vec<f64> = self.banks[2..].iter().map(|b| b.position).collect();
            let speed = if speed > 0.0 && !self.shutdown_banks_out() {
                0.0
            } else {
                speed
            };
            let deltas =
                overlap_sequence(&pos, self.p.rod_max_steps, self.p.rod_overlap, speed, dt);
            for (b, d) in self.banks[2..].iter_mut().zip(deltas) {
                b.target = clamp(b.position + d, 0.0, b.max_steps);
            }
        }
        if rpc && !auto(self, ModuleId::BoronControl) {
            self.boration = match rpc_mode {
                1 => {
                    let err = self.power_ref - p_now;
                    if err > 1.0 && sur < 0.3 {
                        BorationMode::Dilute
                    } else if err < -1.0 {
                        BorationMode::Borate
                    } else {
                        BorationMode::Off
                    }
                }
                2 => {
                    // Coordinated: rods do the fast work; boron slowly brings the
                    // lead bank back into its band once power is on target.
                    let d = self.banks[5].position;
                    let err = self.power_ref - p_now;
                    match self.boration {
                        BorationMode::Dilute if d > 200.0 && err > -0.5 && sur < 0.5 => {
                            BorationMode::Dilute
                        }
                        BorationMode::Borate if d < 193.0 && err.abs() < 1.5 => {
                            BorationMode::Borate
                        }
                        _ if d > 215.0 && err > 0.3 && sur < 0.3 => BorationMode::Dilute,
                        _ if d < 170.0 && err.abs() < 1.0 && p_now > 0.5 => BorationMode::Borate,
                        _ => BorationMode::Off,
                    }
                }
                _ => self.boration,
            };
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
            let bb = self.bias(ModuleId::BoronControl);
            self.boration = match self
                .boron_ctl
                .update(self.banks[5].position, low + bb, high + bb)
            {
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
            let (h, s) =
                self.pzr_p_ctl
                    .update(pressure, sp + self.bias(ModuleId::PressurizerPressure), dt);
            self.heaters = h;
            self.spray = s;
        }

        if auto(self, ModuleId::PressurizerLevel) {
            let sp = self.pzr_level_program(tavg) + self.bias(ModuleId::PressurizerLevel);
            self.charging = self
                .pzr_l_ctl
                .update(self.r("pzr_level_pct"), sp, self.letdown, dt);
        }

        if auto(self, ModuleId::FeedwaterLevel) {
            let sp = self
                .automation
                .setpoint(ModuleId::FeedwaterLevel, "level_pct")
                .unwrap_or(50.0);
            let steam_frac = self.r("steam_flow_kg_s") / (self.feed_max() * w_nom);
            self.feed = self.fw_ctl.update(
                self.r("sg_level_pct"),
                sp + self.bias(ModuleId::FeedwaterLevel),
                steam_frac,
                dt,
            );
        }

        if auto(self, ModuleId::CondenserVacuum) {
            let steam_ok =
                self.msiv_open && self.steam_pressure() > nr_physics::condenser::SJAE_MIN_STEAM;
            let p_kpa = self.r("condenser_kpa");
            let c = &mut self.condenser;
            c.cw_pumps = [true; 3];
            c.sjae_on = steam_ok;
            if p_kpa > 20.0 || !c.sjae_effective {
                c.vacuum_pumps = [true, true];
            } else if p_kpa < 12.0 {
                c.vacuum_pumps = [false, false];
            }
        }

        if auto(self, ModuleId::Deaerator) {
            let da = self.r("da_pressure_mpa");
            if da < 0.2 {
                self.deaerator.pegging_steam = true;
            } else if self.r("steam_flow_kg_s") > 0.25 * w_nom && da > 0.35 {
                self.deaerator.pegging_steam = false;
            }
        }

        if auto(self, ModuleId::SteamDump) {
            let noload = self
                .automation
                .setpoint(ModuleId::SteamDump, "noload_tavg_c")
                .unwrap_or(self.p.tavg_noload);
            // Off-line (or tripped) the dumps hold no-load temperature;
            // on-line they only open on a load rejection.
            let hold_noload = self.reactor_tripped || !self.tg.breaker;
            let db = self.bias(ModuleId::SteamDump);
            let demand = self
                .dump_ctl
                .update(tavg, tref + db, noload + db, hold_noload, dt);
            // Dumps interlocked shut (no condenser vacuum / circulating
            // water, C-9, or MSIVs closed): hold the controller at zero so
            // it does not wind up and slam the dumps open when C-9 clears.
            let blocked = !self.msiv_open || !self.condenser.available();
            // Valve stroke: about 10 s to fully open in modulating service.
            let pos = if blocked {
                0.0
            } else {
                clamp(
                    demand,
                    self.steam_dump - 0.5 * dt,
                    self.steam_dump + 0.1 * dt,
                )
            };
            if blocked {
                self.dump_ctl.track(0.0);
            }
            self.steam_dump = pos;
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

    fn trip_turbine_for(&mut self, reason: &str) {
        if !self.turbine_tripped {
            self.events.push(PlantEvent::trip(reason));
            self.turbine_trip_reason = Some(reason.to_string());
        }
        self.trip_turbine();
    }

    /// C-2 rod stop: power range above 103 %.
    pub fn c2_rod_stop(&self) -> bool {
        !self.reactor_tripped && self.r("nuclear_power_pct") > 103.0
    }

    /// Overpower turbine runback: above 104 % reactor power the turbine
    /// load is backed off (≈10 %/min) until power is back below 102 %.
    fn overpower_runback(&mut self, dt: f64) {
        let power = self.r("nuclear_power_pct");
        let on_line = self.tg.breaker && !self.turbine_tripped && !self.reactor_tripped;
        if !on_line {
            self.runback = false;
            return;
        }
        if !self.runback && power > 104.0 {
            self.runback = true;
            self.events
                .push(PlantEvent::warn("Turbine runback: reactor overpower"));
        } else if self.runback && power < 102.0 {
            self.runback = false;
            self.events
                .push(PlantEvent::info("Turbine runback complete"));
        }
        if self.runback {
            // Back the load target off to what the unit makes at ~98 %
            // reactor power; while still above 104 %, also close the
            // governor valves directly (≈10 %/min).
            let cap = 0.98 * self.max_gross_mwe();
            for m in [ModuleId::TurbineGovernor, ModuleId::PlantMaster] {
                if let Some(t) = self.automation.setpoint(m, "target_mwe") {
                    if t > cap {
                        self.automation.setpoint_set(m, "target_mwe", cap);
                    }
                }
            }
            if power > 104.0 {
                self.turbine_valve = (self.turbine_valve * (1.0 - 0.10 / 60.0 * dt)).max(0.0);
            }
        }
    }

    /// Gross output this unit can make at 100 % reactor power with its
    /// installed equipment (turbine, generator, condenser and steam
    /// generator quality; undersized components) [MWe].
    pub fn max_gross_mwe(&self) -> f64 {
        let e = &self.automation.equipment;
        let quality = e.perf(Slot::Turbine)
            * e.perf(Slot::Generator).powf(0.3)
            * e.perf(Slot::Condenser).powf(0.5)
            * e.perf(Slot::SteamGenerators).powf(0.5);
        (self.p.gross_mwe * quality).min(self.p.gross_mwe * self.output_limit().0 * 1.03)
    }

    /// Reactor power controller implemented as an analog cabinet?
    fn rpc_analog(&self) -> bool {
        matches!(
            self.automation
                .equipment
                .controllers
                .get(&ModuleId::ReactorPower),
            Some(Controller::Analog { .. })
        )
    }

    /// Rod drive mechanisms / power cabinets too small for the core.
    pub fn rod_drives_undersized(&self) -> bool {
        self.automation.equipment.size_ratio(Slot::RodDrives) < 1.0
    }

    /// Output limit from undersized plant components: (fraction, slot).
    pub fn output_limit(&self) -> (f64, Option<Slot>) {
        self.automation.equipment.output_limit()
    }

    /// Governor load limiter: the turbine load that matches 102 % reactor
    /// power with the installed equipment [MWe].
    pub fn load_limit_mwe(&self) -> f64 {
        1.02 * self.max_gross_mwe()
    }

    /// Undersized turbine-side equipment caps steam flow to the turbine.
    fn apply_output_limit(&mut self) {
        // Load limiter: a load target above what the reactor can supply
        // only drains the primary (Tavg and steam pressure sag).
        let limit = self.load_limit_mwe();
        for m in [ModuleId::TurbineGovernor, ModuleId::PlantMaster] {
            if self.automation.setpoint(m, "target_mwe").unwrap_or(0.0) > limit {
                self.automation.setpoint_set(m, "target_mwe", limit);
            }
        }
        let (limit, _) = self.output_limit();
        if limit < 1.0 {
            // Cap steam flow (not just valve position: throttling raises
            // steam pressure) at the limit of the smallest component.
            let ps = self.steam_pressure().max(1.0e5);
            let vmax = limit * self.steam_nominal() / (self.turbine_cv() * ps);
            if self.turbine_valve > vmax {
                self.turbine_valve = vmax;
            }
            let cap = limit * self.p.gross_mwe;
            for m in [ModuleId::TurbineGovernor, ModuleId::PlantMaster] {
                if self.automation.setpoint(m, "target_mwe").unwrap_or(0.0) > cap {
                    self.automation.setpoint_set(m, "target_mwe", cap);
                }
            }
        }
    }

    fn module_usable(&self, m: ModuleId) -> bool {
        if !self.automation.owned.contains(&m) {
            return false;
        }
        // Computers and automation must be scaled for the installed core.
        if !self.automation.equipment.ic_matches() {
            return false;
        }
        match self.automation.equipment.controllers.get(&m) {
            Some(Controller::Analog { .. }) => true,
            _ => modules::info(m)
                .requires_upgrades
                .iter()
                .all(|u| self.automation.upgrades.contains(u)),
        }
    }

    /// Automatic rod balancer (part of the rod control module): realigns
    /// rod groups that have slipped apart. Speed selector OFF/S/M/F.
    fn run_balancer(&mut self, dt: f64) {
        let sel = self
            .automation
            .setpoint(ModuleId::RodControl, "balancer_speed")
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 3.0) as usize;
        if sel == 0 || self.reactor_tripped || !self.module_usable(ModuleId::RodControl) {
            self.balancing = [false; 6];
            return;
        }
        let digital = matches!(
            self.automation
                .equipment
                .controllers
                .get(&ModuleId::RodControl),
            Some(Controller::Digital) | None
        );
        let sel = if digital { sel } else { sel.min(2) };
        let rate =
            [0.0, 4.0, 8.0, 16.0][sel] / 60.0 * self.automation.equipment.perf(Slot::RodDrives);
        let deadband = if digital { 1.0 } else { 3.0 };
        for (i, b) in self.banks.iter_mut().enumerate() {
            let off = b.group2_offset;
            if off.abs() > deadband {
                self.balancing[i] = true;
            }
            if self.balancing[i] {
                if off.abs() < 0.25 {
                    self.balancing[i] = false;
                } else {
                    b.move_group(2, -off.signum() * (rate * dt).min(off.abs()));
                }
            }
        }
    }

    /// Radial power tilt per quadrant from rod group misalignment. Group 1
    /// rods sit in quadrants 1 and 3, group 2 in quadrants 2 and 4.
    pub fn quadrant_tilt(&self) -> [f64; 4] {
        let d: f64 = self
            .banks
            .iter()
            .map(|b| {
                let (g1, g2) = b.group_reactivity();
                g1 - g2
            })
            .sum();
        let k = (50.0 * d).clamp(-0.2, 0.2);
        [k, -k, k, -k]
    }

    fn shutdown_banks_out(&self) -> bool {
        self.banks
            .iter()
            .filter(|b| b.shutdown_bank)
            .all(|b| b.position >= b.max_steps)
    }

    fn trip_turbine(&mut self) {
        // Local roll checks must be redone after any trip.
        self.turbine_hall_ready = false;
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
            upgrades: self.automation.upgrades.clone(),
            analog: self
                .automation
                .equipment
                .controllers
                .iter()
                .filter(|(_, c)| matches!(c, Controller::Analog { .. }))
                .map(|(m, _)| *m)
                .collect(),
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
                let sd_out = self.shutdown_banks_out();
                let Some(b) = self
                    .banks
                    .iter_mut()
                    .find(|b| b.name.eq_ignore_ascii_case(bank))
                else {
                    return deny("unknown rod bank");
                };
                if !b.shutdown_bank && *target > b.position && !sd_out {
                    self.rod_block_until = self.time + 5.0;
                    return deny("ROD WITHDRAWAL BLOCKED: withdraw shutdown banks SA and SB fully (228 steps) first");
                }
                b.target = clamp(*target, 0.0, b.max_steps);
            }
            Command::RodMotion {
                selection,
                direction,
            } if self.rod_drives_undersized() && *direction != RodDirection::Hold => {
                let _ = selection;
                return deny("rod drive power cabinets are sized for a smaller core and cannot step this core's rods: install rod drives for the core size");
            }
            Command::RodMotion {
                selection,
                direction,
            } => {
                let items: Vec<&str> = selection
                    .split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .collect();
                let manual = items.len() == 1 && items[0].eq_ignore_ascii_case("manual");
                let mut any_group = false;
                let mut any_control = manual;
                for item in &items {
                    if manual {
                        break;
                    }
                    let (bank_name, group) = match parse_group(item) {
                        Some((b, g)) => (b, Some(g)),
                        None => (*item, None),
                    };
                    let Some(bank) = self
                        .banks
                        .iter()
                        .find(|b| b.name.eq_ignore_ascii_case(bank_name))
                    else {
                        return deny("rod selection must be MANUAL, banks (D) or groups (D2), e.g. \"A1,B2,D\"");
                    };
                    any_group |= group.is_some();
                    any_control |= !bank.shutdown_bank;
                }
                if items.is_empty() {
                    return deny("no rods selected");
                }
                if (any_group || items.len() > 1)
                    && !self
                        .automation
                        .upgrades
                        .contains(&UpgradeId::RodGroupSelect)
                {
                    return deny(
                        "group and multi-group selection need the Rod Group Select pushbutton grid",
                    );
                }
                if self.reactor_tripped && *direction == RodDirection::Out {
                    return deny("reactor trip is latched; reset before withdrawing rods");
                }
                if *direction == RodDirection::Out && self.c2_rod_stop() {
                    self.rod_block_until = self.time + 5.0;
                    return deny(
                        "ROD WITHDRAWAL BLOCKED: C-2 power range high flux rod stop (above 103 %)",
                    );
                }
                if any_control && *direction == RodDirection::Out && !self.shutdown_banks_out() {
                    self.rod_block_until = self.time + 5.0;
                    return deny("ROD WITHDRAWAL BLOCKED: withdraw shutdown banks SA and SB fully (228 steps) first");
                }
                self.rod_selection = selection.clone();
                self.rod_direction = *direction;
                if *direction == RodDirection::Hold {
                    for b in &mut self.banks {
                        b.target = b.position;
                    }
                }
            }
            Command::SetRodSpeed { steps_per_min } => {
                self.rod_spm = clamp(*steps_per_min, 4.0, 72.0)
            }
            Command::SetBoration { mode } => self.boration = *mode,
            Command::SetCwPump { index, running } => {
                if *running && *index < 3 && !self.bus_live(*index >= 2) {
                    return deny(&format!(
                        "CW pump {} has no power: its 6.9 kV bus is de-energised",
                        index + 1
                    ));
                }
                let Some(p) = self.condenser.cw_pumps.get_mut(*index) else {
                    return deny("CW pump index must be 0-2");
                };
                *p = *running;
            }
            Command::SetVacuumPump { index, running } => {
                if *running && *index < 2 && !self.bus_live(*index == 1) {
                    return deny(&format!(
                        "vacuum pump {} has no power: its 6.9 kV bus is de-energised",
                        index + 1
                    ));
                }
                let Some(p) = self.condenser.vacuum_pumps.get_mut(*index) else {
                    return deny("vacuum pump index must be 0-1");
                };
                *p = *running;
            }
            Command::SetSjae { on } => self.condenser.sjae_on = *on,
            Command::SetPeggingSteam { open } => self.deaerator.pegging_steam = *open,
            Command::SetElecBreaker { breaker, closed } => {
                let Some(b) = Electrical::breaker_from_name(breaker) else {
                    return deny("unknown breaker");
                };
                self.elec.set_breaker(b, *closed);
            }
            Command::SetDiesel { index, running } => {
                if *index > 1 {
                    return deny("diesel index must be 0 or 1");
                }
                if *running {
                    self.elec.start_diesel(*index);
                } else {
                    self.elec.stop_diesel(*index);
                }
            }
            Command::ResetTransformers => {
                self.elec.reset_transformers();
                self.events
                    .push(PlantEvent::info("Station transformer lockouts reset"));
            }
            Command::SetOffsitePower { available } => {
                self.elec.grid_available = *available;
                self.events.push(if *available {
                    PlantEvent::info("Offsite power restored")
                } else {
                    PlantEvent::trip("LOSS OF OFFSITE POWER")
                });
            }
            Command::RefuelDiesels => {
                for d in &mut self.elec.diesels {
                    d.fuel_l = d.fuel_capacity_l;
                }
            }
            Command::SetMsiv { open } => {
                self.msiv_open = *open;
                self.events.push(PlantEvent::warn(if *open {
                    "Main steam isolation valves opened"
                } else {
                    "Main steam isolation valves closed"
                }));
            }
            Command::SetPressurizerHeaters { fraction } => {
                self.heaters = clamp(*fraction, 0.0, 1.0)
            }
            Command::SetPressurizerSpray { fraction } => self.spray = clamp(*fraction, 0.0, 1.0),
            Command::SetCharging { kg_s } => self.charging = clamp(*kg_s, 0.0, self.p.charging_max),
            Command::SetLetdown { kg_s } => {
                if self.letdown_isolated && *kg_s > 0.0 {
                    if self.r("pzr_level_pct") < 17.0 {
                        return deny("letdown is isolated on low pressurizer level: raise charging and restore level above 17 % first");
                    }
                    self.letdown_isolated = false;
                    self.events.push(PlantEvent::info("Letdown re-established"));
                }
                self.letdown = clamp(*kg_s, 0.0, 15.0);
            }
            Command::SetCoolantPump { index, running } => {
                if *index >= 4 {
                    return deny("RCP index must be 0-3");
                }
                if *running && !self.bus_live(*index >= 2) {
                    return deny(&format!(
                        "RCP {} has no power: 6.9 kV bus {} is de-energised",
                        index + 1,
                        if *index >= 2 { "B" } else { "A" }
                    ));
                }
                self.rcp_running[*index] = *running;
            }
            Command::SetFeedwater { fraction } => self.feed = clamp(*fraction, 0.0, 1.0),
            Command::SetSteamDump { fraction } => {
                if *fraction > 0.0 && !self.condenser.available() {
                    self.vac_required_until = self.time + 8.0;
                }
                self.steam_dump = clamp(*fraction, 0.0, 1.0)
            }
            Command::SetTurbineValve { fraction } => {
                if self.turbine_tripped {
                    return deny("turbine is tripped; reset (latch) the turbine first");
                }
                self.turbine_valve = clamp(*fraction, 0.0, 1.0);
            }
            Command::SetGeneratorBreaker { closed: false } => {
                if self.tg.breaker {
                    self.breaker_opened_by_operator = true;
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
                if self.condenser.pressure_kpa > 25.0 {
                    self.vac_required_until = self.time + 8.0;
                    return deny("CONDENSER VACUUM REQUIRED: pull condenser vacuum below 25 kPa before latching the turbine");
                }
                if self.automation.game_mode == GameMode::Career && !self.turbine_hall_ready {
                    return deny("call the turbine hall for roll checks before latching the turbine (Comms tab)");
                }
                self.turbine_trip_reason = None;
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
                if *module == ModuleId::ReactorPower
                    && key == "mode"
                    && *value != 0.0
                    && self.rpc_analog()
                {
                    return deny("an analog reactor power controller only has ROD mode: BORON and COORD need the digital controller");
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
                // Preset: install everything (fully modernised) or strip the
                // unit back to its original analog, manual configuration.
                let modern = *generation == ControlRoomGeneration::Digital;
                for u in nr_api::upgrades::CATALOG {
                    self.set_upgrade(u.id, modern);
                }
                for m in modules::CATALOG {
                    self.set_module_installed(m.id, modern);
                }
            }
            Command::SetUpgradeInstalled { upgrade, installed } => {
                self.set_upgrade(*upgrade, *installed)
            }
            Command::SetComponent {
                slot,
                era,
                tier,
                size,
            } => {
                let current = self
                    .automation
                    .equipment
                    .components
                    .get(slot)
                    .map(|c| c.size)
                    .unwrap_or(self.size());
                let size = size.unwrap_or(current);
                if *slot == Slot::ReactorCore && size != self.size() {
                    return deny("the core size sets the whole plant: change unit size with LOAD SCENARIO and a size");
                }
                self.automation
                    .equipment
                    .components
                    .insert(*slot, nr_api::Component::sized(*era, *tier, size));
                self.events.push(PlantEvent::info(&format!(
                    "{} replaced: {:?} {:?} {}",
                    nr_api::equipment::slot_info(*slot).name,
                    era,
                    tier,
                    size.label()
                )));
                self.sandbox_fit_kits();
            }
            Command::SetModuleController { module, controller } => {
                if self.automation.mode(*module) != ModuleMode::Manual {
                    self.set_module_mode(*module, ModuleMode::Manual);
                }
                self.automation
                    .equipment
                    .controllers
                    .insert(*module, *controller);
                self.ctl_bias.remove(module);
                self.events.push(PlantEvent::info(&format!(
                    "{}: {} controller fitted",
                    modules::info(*module).name,
                    controller.label()
                )));
                self.sandbox_fit_kits();
            }
            Command::SetModuleInstalled { module, installed } => {
                self.set_module_installed(*module, *installed)
            }
            Command::LoadScenario { name, size } => {
                let size = size.unwrap_or(self.size());
                let Some(mut fresh) = Self::from_scenario_sized(name, size) else {
                    return deny("scenario must be hot_standby or full_power");
                };
                fresh.automation.game_mode = self.automation.game_mode;
                fresh.automation.owned = self.automation.owned.clone();
                fresh.automation.upgrades = self.automation.upgrades.clone();
                for (m, mode) in fresh.automation.modes.clone() {
                    if mode != ModuleMode::Manual && !fresh.automation.owned.contains(&m) {
                        fresh.automation.modes.insert(m, ModuleMode::Manual);
                    }
                }
                fresh.events.push(PlantEvent::info(&format!(
                    "Scenario loaded: {name} ({})",
                    size.label()
                )));
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

    fn set_upgrade(&mut self, upgrade: UpgradeId, installed: bool) {
        let changed = if installed {
            self.automation.upgrades.insert(upgrade)
        } else {
            self.automation.upgrades.remove(&upgrade)
        };
        if !changed {
            return;
        }
        if !installed {
            // Modules that need this hardware lose access and drop to manual.
            for m in modules::CATALOG {
                if m.requires_upgrades.contains(&upgrade)
                    && self.automation.mode(m.id) != ModuleMode::Manual
                {
                    self.set_module_mode(m.id, ModuleMode::Manual);
                    self.events.push(PlantEvent::warn(&format!(
                        "{} lost its computer link: MANUAL",
                        m.name
                    )));
                }
            }
        }
        let what = if installed { "installed" } else { "removed" };
        let name = nr_api::upgrades::info(upgrade).name;
        self.events
            .push(PlantEvent::info(&format!("{name} {what}")));
    }

    /// Sandbox: conversion kits are fitted automatically (free).
    fn sandbox_fit_kits(&mut self) {
        let e = &mut self.automation.equipment;
        e.kits = e.required_kits().into_iter().map(|k| k.key).collect();
        self.apply_equipment();
    }

    /// Install a complete equipment configuration (career purchases). Any
    /// module that can no longer run in its current mode drops to MANUAL.
    pub fn install(
        &mut self,
        owned: std::collections::BTreeSet<ModuleId>,
        upgrades: std::collections::BTreeSet<UpgradeId>,
        equipment: Equipment,
    ) {
        self.automation.owned = owned;
        self.automation.upgrades = upgrades;
        self.set_equipment(equipment);
        let ctx = self.policy_context();
        for m in modules::CATALOG {
            let mode = self.automation.mode(m.id);
            if mode == ModuleMode::Manual {
                continue;
            }
            let check = Command::SetModuleMode { module: m.id, mode };
            if authorize(&check, &Origin::Operator, &ctx).is_err() {
                self.set_module_mode(m.id, ModuleMode::Manual);
            }
        }
    }

    /// Automatic generation control needs the unit load controller in AUTO and
    /// the grid dispatch data link.
    pub fn agc_available(&self) -> bool {
        self.automation.mode(ModuleId::PlantMaster) == ModuleMode::Auto
            && self
                .automation
                .upgrades
                .contains(&UpgradeId::GridDispatchLink)
    }

    /// Reactor shut down and generator off-line: heavy components may be
    /// replaced.
    pub fn in_outage(&self) -> bool {
        self.kinetics.n < 1e-4 && !self.tg.breaker
    }

    /// Replace the whole equipment configuration (career commissioning).
    pub fn set_equipment(&mut self, e: Equipment) {
        self.automation.equipment = e;
        self.ctl_bias.clear();
        self.apply_equipment();
    }

    fn set_module_installed(&mut self, module: ModuleId, installed: bool) {
        let changed = if installed {
            self.automation.owned.insert(module)
        } else {
            if self.automation.mode(module) != ModuleMode::Manual {
                self.set_module_mode(module, ModuleMode::Manual);
            }
            self.automation.owned.remove(&module)
        };
        if changed {
            let what = if installed { "installed" } else { "removed" };
            self.events.push(PlantEvent::info(&format!(
                "{} {what}",
                modules::info(module).name
            )));
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
                    let sf = self.steam_flow / (self.feed_max() * self.steam_nominal());
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
                ModuleId::ReactorPower => {
                    self.power_ref = self.kinetics.n * 100.0;
                    self.automation.setpoint_set(
                        ModuleId::ReactorPower,
                        "power_pct",
                        (self.power_ref * 10.0).round() / 10.0,
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

    #[cfg(test)]
    pub(crate) fn alarms_for_test(&self) -> Vec<String> {
        self.alarms().into_iter().map(|a| a.tag).collect()
    }

    /// Every annunciator window / status light currently lit, by panel.
    fn alarms(&self) -> Vec<Alarm> {
        use Severity::{Alarm as A, Info as I, Trip as T, Warning as W};
        let mut out = Vec::new();
        let mut add = |panel: &str, tag: &str, text: &str, sev: Severity| {
            out.push(Alarm {
                tag: tag.into(),
                panel: panel.into(),
                text: text.into(),
                severity: sev,
            })
        };
        let mode = |m: ModuleId| self.automation.mode(m);
        let r = |t: &str| self.r(t);
        let tavg = r("tavg_c");
        let power = r("nuclear_power_pct");
        let tripped = self.reactor_tripped;

        // ------------------------------------------------------- reactor
        let rx = "reactor";
        if tripped {
            add(rx, "rx_trip", "REACTOR TRIP", T);
        }
        match mode(ModuleId::RodControl) {
            ModuleMode::Auto => add(rx, "rod_auto", "ROD CONTROL IN AUTO", I),
            ModuleMode::External => add(rx, "rod_auto", "ROD CONTROL EXTERNAL", I),
            ModuleMode::Manual => add(rx, "rod_manual", "ROD CONTROL IN MANUAL", I),
        }
        if !tripped
            && self
                .banks
                .iter()
                .any(|b| (b.target - b.position).abs() > 0.01)
        {
            add(rx, "rods_moving", "RODS MOVING", I);
        }
        let sd_partial = self
            .banks
            .iter()
            .any(|b| b.shutdown_bank && b.position > 0.0 && b.position < b.max_steps);
        let ctl_out = self
            .banks
            .iter()
            .any(|b| !b.shutdown_bank && b.position > 0.0);
        if !tripped && (sd_partial || (ctl_out && !self.shutdown_banks_out())) {
            add(rx, "sd_not_out", "SHUTDOWN BANKS NOT FULLY WITHDRAWN", W);
        }
        if self.time < self.rod_block_until {
            add(rx, "rod_block", "ROD WITHDRAWAL BLOCKED", A);
        }
        if self.c2_rod_stop() {
            add(rx, "c2", "C-2 PR HIGH FLUX ROD STOP", W);
        }
        if self.runback {
            add(
                "turbine",
                "runback",
                "TURBINE RUNBACK: REACTOR OVERPOWER",
                A,
            );
        }
        if !tripped && power > 5.0 && self.banks[5].position < 80.0 {
            if self.banks[5].position < 30.0 && self.banks[4].position < 128.0 {
                add(rx, "rod_ins_lolo", "ROD INSERTION LIMIT LO-LO", A);
            } else {
                add(rx, "rod_ins_lo", "ROD INSERTION LIMIT LO", W);
            }
        }
        if self.banks.iter().any(|b| b.group2_offset.abs() > 12.0) {
            add(rx, "rod_dev", "ROD GROUP DEVIATION", A);
        }
        if mode(ModuleId::ReactorPower) == ModuleMode::Auto {
            add(rx, "rpc_auto", "REACTOR POWER CONTROL IN AUTO", I);
        }
        if self.balancing.iter().any(|b| *b) {
            add(rx, "balancer", "ROD BALANCER ACTIVE", I);
        }
        if power > 10.0 && self.quadrant_tilt().iter().any(|t| t.abs() > 0.02) {
            add(rx, "qptr", "QUADRANT POWER TILT", W);
        }
        if !tripped && self.rho.total > -0.0005 && self.kinetics.n > 1e-8 {
            add(rx, "critical", "REACTOR CRITICAL", I);
        }
        if r("startup_rate_dpm") > 1.0 {
            add(rx, "sur", "HIGH STARTUP RATE", W);
        }
        if self.kinetics.period > 0.0 && self.kinetics.period < 15.0 {
            add(rx, "short_period", "SHORT PERIOD", A);
        }
        let prs = ["pr_n41_pct", "pr_n42_pct", "pr_n43_pct", "pr_n44_pct"].map(r);
        let pr_max = prs.iter().cloned().fold(f64::MIN, f64::max);
        let pr_min = prs.iter().cloned().fold(f64::MAX, f64::min);
        if pr_max > 10.0 && pr_max - pr_min > 2.0 {
            add(rx, "nis_dev", "NIS POWER RANGE CHANNEL DEVIATION", W);
        }
        if self.p6() {
            add(rx, "p6", "P-6 IR PERMISSIVE", I);
        }
        if self.p10() {
            add(rx, "p10", "P-10 PR PERMISSIVE", I);
        }
        if self.p6() && !self.nis.sr_blocked {
            add(rx, "sr_block_permitted", "SR BLOCK PERMITTED", W);
        }
        if self.nis.sr_blocked {
            add(rx, "sr_blocked", "SR TRIP BLOCKED / HV OFF", I);
        }
        if self.nis.ir_blocked {
            add(rx, "ir_blocked", "IR TRIP BLOCKED", I);
        }
        if self.nis.pr_low_blocked {
            add(rx, "pr_low_blocked", "PR LO SETPT TRIP BLOCKED", I);
        }

        // ----------------------------------------------------------- RCS
        let rc = "rcs";
        let p = r("rcs_pressure_mpa");
        if p > 15.9 {
            add(rc, "p_high", "PZR PRESS HIGH", A);
        }
        if p < 15.1 {
            add(rc, "p_low", "PZR PRESS LOW", A);
        }
        if self.porv_open {
            add(rc, "porv", "PZR PORV OPEN", A);
        }
        if self.pressure > PZR_SAFETY {
            add(rc, "pzr_safety", "PZR SAFETY VALVE OPEN", T);
        }
        let lvl = r("pzr_level_pct");
        if lvl > 70.0 {
            add(rc, "pzr_lvl_hi", "PZR LEVEL HIGH", A);
        } else if lvl < 17.0 {
            add(rc, "pzr_lvl_lo", "PZR LEVEL LOW", A);
        }
        if self.letdown_isolated {
            add(rc, "pzr_lo_iso", "PZR LO LEVEL: LETDOWN ISOL / HTRS OFF", A);
        }
        if (17.0..=70.0).contains(&lvl) && (lvl - self.pzr_level_program(tavg)).abs() > 5.0 {
            add(rc, "pzr_lvl_dev", "PZR LEVEL DEVIATION", W);
        }
        if self.heaters > 0.05 {
            add(rc, "heaters_on", "PZR HEATERS ON", I);
        }
        if self.spray > 0.02 {
            add(rc, "spray_open", "PZR SPRAY OPEN", I);
        }
        for i in 0..4 {
            if self.rcp_running[i] {
                add(
                    rc,
                    &format!("rcp{}_run", i + 1),
                    &format!("RCP {} RUNNING", i + 1),
                    I,
                );
            }
        }
        if self.rcp_running.iter().any(|r| !r) {
            add(rc, "rcp", "RCP STOPPED", W);
        }
        if r("rcs_flow_pct") < 92.0 {
            add(rc, "rcs_low_flow", "RCS LOW FLOW", A);
        }
        if (tavg - self.tref()).abs() > 3.0 && !tripped && self.tg.breaker {
            add(rc, "tavg_dev", "TAVG / TREF DEVIATION", W);
        }
        if tavg > 315.0 {
            add(rc, "tavg_high", "TAVG HIGH", A);
        }
        if steam::tsat(self.pressure) - r("thot_c") < 15.0 {
            add(rc, "subcooling", "SUBCOOLING MARGIN LOW", A);
        }

        // ---------------------------------------------------------- CVCS
        let cv = "cvcs";
        match self.boration {
            BorationMode::Borate => add(cv, "borating", "BORATION IN PROGRESS", I),
            BorationMode::Dilute => add(cv, "diluting", "DILUTION IN PROGRESS", I),
            BorationMode::Off => {}
        }
        if mode(ModuleId::BoronControl) == ModuleMode::Auto {
            add(cv, "auto_makeup", "AUTO MAKEUP ACTIVE", I);
        }
        if self.charging < 1.0 {
            add(cv, "charging_low", "CHARGING FLOW LOW", W);
        }
        if self.charging > 18.0 {
            add(cv, "charging_high", "CHARGING FLOW HIGH", W);
        }
        if self.letdown < 1.0 {
            add(cv, "letdown_iso", "LETDOWN ISOLATED", W);
        }

        // ------------------------------------------------ steam & feed
        let st = "steam";
        if self.msiv_open {
            add(st, "msiv_open", "MSIVs OPEN", I);
        } else {
            add(st, "msiv_closed", "MSIVs CLOSED", W);
        }
        if mode(ModuleId::SteamDump) == ModuleMode::Auto && self.condenser.available() {
            add(st, "dump_armed", "STEAM DUMP ARMED", I);
        }
        if self.steam_dump > 0.02 && self.condenser.available() {
            add(st, "dump_open", "STEAM DUMP OPEN", I);
        }
        if self.steam_dump > 0.02 && !self.condenser.available() {
            add(st, "dump_blocked", "STEAM DUMP BLOCKED (NO VACUUM)", A);
        }
        if self.feed > 0.01 {
            add(st, "feed_pumps", "FEED PUMPS RUNNING", I);
        }
        let sg = r("sg_level_pct");
        if sg > 75.0 {
            add(st, "sg_lvl_hi", "SG LEVEL HIGH", A);
        } else if sg < 15.0 {
            add(st, "sg_lvl_lolo", "SG LEVEL LO-LO", A);
        } else if sg < 30.0 {
            add(st, "sg_lvl_low", "SG LEVEL LOW", W);
        } else if !(38.0..=62.0).contains(&sg) {
            add(st, "sg_lvl_dev", "SG LEVEL DEVIATION", W);
        }
        let w_nom = self.steam_nominal();
        if (r("steam_flow_kg_s") - r("feed_flow_kg_s")).abs() > 0.1 * w_nom {
            add(st, "flow_mismatch", "STEAM / FEED FLOW MISMATCH", W);
        }
        if self.steam_pressure() > SG_ARV {
            add(st, "sg_arv", "ATMOSPHERIC RELIEF OPEN", A);
        }
        let vac = r("condenser_kpa");
        if vac > 15.0 && (!self.turbine_tripped || self.tg.speed > 0.1 || self.condenser.sjae_on) {
            add(st, "vac_low", "CONDENSER VACUUM LOW", A);
        }
        if self.time < self.vac_required_until {
            add(st, "vac_required", "CONDENSER VACUUM REQUIRED", A);
        }
        if self.condenser.sjae_on && !self.condenser.sjae_effective {
            add(st, "sjae_low", "SJAE MOTIVE STEAM LOW", W);
        }
        if self.condenser.sjae_effective {
            add(st, "sjae_on", "SJAEs IN SERVICE", I);
        }
        if self.condenser.vacuum_pumps.iter().any(|v| *v) {
            add(st, "vac_pumps", "VACUUM PUMPS RUNNING", I);
        }
        if self.condenser.cw_pumps.iter().any(|p| !p) {
            add(st, "cw_trip", "CW PUMP OFF", W);
        }
        let hw = self.condenser.hotwell_pct;
        if hw > 75.0 {
            add(st, "hotwell_hi", "HOTWELL LEVEL HIGH", W);
        } else if hw < 25.0 {
            add(st, "hotwell_lo", "HOTWELL LEVEL LOW", W);
        }
        if !self.deaerator.effective() {
            add(st, "deaeration", "DEAERATION NOT EFFECTIVE", W);
        }
        if self.deaerator.pegging_steam {
            add(st, "pegging", "DA PEGGING STEAM ON", I);
        }
        if self.feed > 0.05 && self.t_feed < 100.0 {
            add(st, "feed_temp_low", "FEEDWATER TEMP LOW", W);
        }

        // ------------------------------------------------ turbine-generator
        let tb = "turbine";
        if self.turbine_tripped {
            add(tb, "turb_trip", "TURBINE TRIP", A);
        } else {
            add(tb, "turb_latched", "TURBINE LATCHED", I);
        }
        let rpm = self.tg.rpm();
        if rpm < 10.0 {
            add(tb, "turning_gear", "TURBINE ON TURNING GEAR", I);
        }
        if mode(ModuleId::TurbineGovernor) == ModuleMode::Auto {
            add(tb, "gov_auto", "GOVERNOR IN AUTO", I);
        }
        if mode(ModuleId::GeneratorSync) == ModuleMode::Auto {
            add(tb, "autosync", "AUTO-SYNC ACTIVE", I);
        }
        if self.sync_check_active() {
            add(tb, "synccheck", "SYNC-CHECK ACTIVE", I);
        }
        if self.tg.breaker {
            add(tb, "gen_online", "GENERATOR ON-LINE", I);
        }
        if self.tg.speed > 1.03 {
            add(tb, "overspeed", "TURBINE OVERSPEED", A);
        }
        let vib = r("turbine_vibration_um");
        if vib > 125.0 {
            add(tb, "high_vib", "TURBINE HIGH VIBRATION", A);
        }
        if let Some(reason) = &self.turbine_trip_reason {
            if reason.contains("vacuum") {
                add(tb, "low_vac_trip", "LOW VACUUM TRIP", T);
            } else if reason.contains("vibration") {
                add(tb, "vib_trip", "HIGH VIBRATION TRIP", T);
            }
        }
        if !self.turbine_hall_ready && !self.turbine_tripped && rpm > 100.0 {
            add(tb, "hall_checks", "TURBINE ROLL WITHOUT HALL CHECKS", W);
        }
        if self.tg.lockout {
            add(tb, "gen_lockout", "GEN LOCKOUT 86G", T);
        }
        if self.tg.health < 90.0 {
            add(tb, "gen_damage", "GENERATOR DAMAGE", A);
        }
        if !self.tg.field_breaker {
            add(tb, "field_open", "FIELD BREAKER OPEN", I);
        }
        if self.time < self.sync_block_until {
            add(tb, "sync_blocked", "PHASE Δ OUT OF RANGE", A);
        }
        if self.tg.breaker && self.tg.net_power < 0.0 {
            add(tb, "reverse_power", "GEN REVERSE POWER", A);
        }
        let ready = !self.tg.breaker && self.tg.speed > 0.97 && !self.turbine_tripped;
        if ready {
            add(tb, "ready_sync", "READY TO SYNC", I);
            if !self.grid_permission {
                add(tb, "grid_perm", "GRID PERMISSION REQUIRED", W);
            }
        }

        // ----------------------------------------------------- electrical
        let el = "electrical";
        let e = &self.elec;
        if !e.grid_available {
            add(el, "loop", "LOSS OF OFFSITE POWER", T);
        }
        if e.sut_tripped {
            add(el, "sut_trip", "STARTUP XFMR TRIP / LOCKOUT", A);
        } else if e.sut_load_mw > e.sut_rating * e.transformer_quality.powi(3) {
            add(el, "sut_overload", "STARTUP XFMR OVERLOAD", W);
        }
        if e.uat_tripped {
            add(el, "uat_trip", "UNIT AUX XFMR TRIP / LOCKOUT", A);
        } else if e.uat_load_mw > e.uat_rating * e.transformer_quality.powi(3) {
            add(el, "uat_overload", "UNIT AUX XFMR OVERLOAD", W);
        }
        if !e.buses.bus_a {
            add(el, "bus_a_dead", "6.9 kV BUS A DE-ENERGISED", A);
        }
        if !e.buses.bus_b {
            add(el, "bus_b_dead", "6.9 kV BUS B DE-ENERGISED", A);
        }
        if !e.buses.safety1 {
            add(el, "s1_uv", "SAFETY BUS 1 UNDERVOLTAGE", A);
        }
        if !e.buses.safety2 {
            add(el, "s2_uv", "SAFETY BUS 2 UNDERVOLTAGE", A);
        }
        for (i, d) in e.diesels.iter().enumerate() {
            match d.state {
                DieselState::Running => add(
                    el,
                    &format!("edg{}_run", i + 1),
                    &format!("EDG {} RUNNING", i + 1),
                    I,
                ),
                DieselState::Starting => add(
                    el,
                    &format!("edg{}_run", i + 1),
                    &format!("EDG {} STARTING", i + 1),
                    W,
                ),
                DieselState::Stopped => {}
            }
            if d.fuel_l < 0.2 * d.fuel_capacity_l {
                add(
                    el,
                    &format!("edg{}_fuel", i + 1),
                    &format!("EDG {} FUEL LOW", i + 1),
                    W,
                );
            }
        }
        if !e.buses.safety1 && !e.buses.safety2 {
            add(el, "batt_discharge", "BATTERY DISCHARGING", W);
        }
        if !e.buses.dc {
            add(el, "dc_low", "125 V DC BUS LOW VOLTAGE", T);
        }
        if e.closed(Breaker::TieAB) {
            add(el, "tie_closed", "BUS A-B TIE CLOSED", I);
        }
        if e.import_mw > 0.5 {
            add(el, "importing", "IMPORTING GRID POWER", I);
        }
        if mode(ModuleId::BusTransfer) == ModuleMode::Auto {
            add(el, "abt_auto", "AUTO BUS TRANSFER IN SERVICE", I);
        }
        if !e.buses.safety1 && !e.buses.safety2 && e.buses.dc && self.vital_ac_gap().is_some() {
            add(el, "ups_battery", "VITAL BUS ON UPS / BATTERY", W);
        }
        if e.battery_pct < 25.0 {
            add(el, "batt_low", "STATION BATTERY LOW", A);
        }

        // ----------------------------------------------- plant computer
        let pc = "computer";
        let (limit, by) = self.output_limit();
        if let Some(slot) = by {
            add(
                "turbine",
                "derated",
                &format!(
                    "UNIT DERATED {:.0} %: {} UNDERSIZED",
                    limit * 100.0,
                    nr_api::equipment::slot_info(slot).name.to_uppercase()
                ),
                W,
            );
        }
        if self.rod_drives_undersized() {
            add(
                "reactor",
                "crdm_size",
                "ROD DRIVES UNDERSIZED FOR CORE: ROD MOTION BLOCKED",
                A,
            );
        }
        if !self.automation.equipment.ic_matches() {
            add(
                pc,
                "ic_scale",
                "I&C NOT SCALED FOR CORE SIZE: AUTOMATION OFF",
                A,
            );
        }
        if self.training_mode {
            add(pc, "training", "TRAINING MODE: RPS BYPASSED", I);
        }
        for (tag, s) in &self.sensors {
            if s.fault != SensorFault::None {
                add(pc, tag, &format!("INSTRUMENT FAULT {tag}"), W);
            }
        }
        out
    }

    pub fn telemetry(&self, time_scale: f64, paused: bool) -> Telemetry {
        let ctx = self.policy_context();
        Telemetry {
            api_version: API_VERSION,
            unit: self.name.clone(),
            reactor_type: format!("PWR-{:.0}", self.size().mwe()),
            unit_size: self.size(),
            rated_gross_mwe: self.p.gross_mwe,
            rated_thermal_mw: self.p.thermal_power / 1e6,
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
                    group2: b.group2(),
                })
                .collect(),
            rod_motion: format!("{}:{:?}", self.rod_selection, self.rod_direction).to_lowercase(),
            rod_speed_spm: self.rod_spm,
            quadrant_tilt: self.quadrant_tilt().to_vec(),
            coolant_pumps: self.rcp_running.to_vec(),
            generator_breaker: self.tg.breaker,
            field_breaker: self.tg.field_breaker,
            cw_pumps: self.condenser.cw_pumps.to_vec(),
            vacuum_pumps: self.condenser.vacuum_pumps.to_vec(),
            sjae_on: self.condenser.sjae_on,
            sjae_effective: self.condenser.sjae_effective,
            pegging_steam: self.deaerator.pegging_steam,
            msiv_open: self.msiv_open,
            condenser_available: self.condenser.available(),
            turbine_hall_ready: self.turbine_hall_ready,
            indication_power: self.indication_power(),
            electrical: serde_json::to_value(&self.elec).unwrap_or_default(),
            grid_permission: self.grid_permission,
            generator_lockout: self.tg.lockout,
            sync_check_active: self.sync_check_active(),
            sync_blocked: self.time < self.sync_block_until,
            core_exit_temps: if self
                .automation
                .upgrades
                .contains(&UpgradeId::CoreExitThermocoupleMap)
            {
                self.core_exit_temps()
                    .into_iter()
                    .map(|t| (t * 10.0).round() / 10.0)
                    .collect()
            } else {
                Vec::new()
            },
            nis: NisView {
                sr_blocked: self.nis.sr_blocked,
                ir_blocked: self.nis.ir_blocked,
                pr_low_blocked: self.nis.pr_low_blocked,
                sr_hv_on: !self.nis.sr_blocked,
                p6: self.p6(),
                p10: self.p10(),
            },
            sections: nr_api::upgrades::CATALOG
                .iter()
                .flat_map(|u| {
                    let gen = if self.automation.upgrades.contains(&u.id) {
                        ControlRoomGeneration::Digital
                    } else {
                        ControlRoomGeneration::Analog
                    };
                    u.sections.iter().map(move |s| (s.to_string(), gen))
                })
                .collect(),
            equipment: self.automation.equipment.clone(),
            required_kits: self.automation.equipment.required_kits(),
            upgrades: nr_api::upgrades::CATALOG
                .iter()
                .map(|u| UpgradeView {
                    id: u.id,
                    kind: u.kind,
                    computer: u.computer,
                    name: u.name.into(),
                    description: u.description.into(),
                    price: u.price,
                    sections: u.sections.iter().map(|s| s.to_string()).collect(),
                    installed: self.automation.upgrades.contains(&u.id),
                })
                .collect(),
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
                    price: m.price,
                    requires_upgrades: m.requires_upgrades.to_vec(),
                    setpoints: self
                        .automation
                        .setpoints
                        .get(&m.id)
                        .cloned()
                        .unwrap_or_default(),
                    setpoint_limits: self
                        .automation
                        .setpoints
                        .get(&m.id)
                        .map(|sp| {
                            sp.keys()
                                .filter_map(|k| {
                                    AutomationState::setpoint_limits(m.id, k)
                                        .map(|r| (k.clone(), r))
                                })
                                .collect()
                        })
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
