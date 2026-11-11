//! Telemetry snapshot: what the control room shows.

use crate::commands::{BorationMode, ControlRoomGeneration};
use crate::modules::{ModuleId, ModuleMode};
use crate::policy::GameMode;
use crate::upgrades::UpgradeId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Alarm,
    Trip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    pub tag: String,
    /// Control room panel / tab the annunciator window lives on.
    pub panel: String,
    pub text: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RodView {
    pub bank: String,
    pub position: f64,
    pub target: f64,
    pub max_steps: f64,
    /// Position of the bank's second rod group.
    pub group2: f64,
}

/// Nuclear instrumentation system status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NisView {
    /// Source range trip blocked (high voltage removed).
    pub sr_blocked: bool,
    pub ir_blocked: bool,
    pub pr_low_blocked: bool,
    /// Source range detector high voltage energised.
    pub sr_hv_on: bool,
    /// P-6: intermediate range above 1e-10 A (permits SR block).
    pub p6: bool,
    /// P-10: power range above 10 % (permits IR / PR-low block).
    pub p10: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeView {
    pub id: UpgradeId,
    pub kind: crate::upgrades::UpgradeKind,
    pub computer: Option<UpgradeId>,
    pub name: String,
    pub description: String,
    pub price: u64,
    pub sections: Vec<String>,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleView {
    pub id: ModuleId,
    pub name: String,
    pub panel: String,
    pub mode: ModuleMode,
    pub owned: bool,
    pub implemented: bool,
    pub price: u64,
    pub requires_upgrades: Vec<UpgradeId>,
    pub setpoints: BTreeMap<String, f64>,
    /// Adjustment limits [min, max] of each setpoint, in engineering units.
    #[serde(default)]
    pub setpoint_limits: BTreeMap<String, [f64; 2]>,
}

/// One snapshot of instrument readings and discrete plant status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Telemetry {
    pub api_version: u32,
    pub unit: String,
    pub reactor_type: String,
    pub game_mode: GameMode,
    pub sim_time: f64,
    pub time_scale: f64,
    pub paused: bool,
    pub training_mode: bool,
    /// Instrument readings by tag (see `docs/api/README.md` for the tag list).
    pub readings: BTreeMap<String, f64>,
    pub rods: Vec<RodView>,
    pub rod_motion: String,
    /// Manual rod drive speed [steps/min].
    pub rod_speed_spm: f64,
    /// Radial power tilt per quadrant (fraction, Q1..Q4).
    pub quadrant_tilt: Vec<f64>,
    pub coolant_pumps: Vec<bool>,
    pub generator_breaker: bool,
    pub field_breaker: bool,
    pub cw_pumps: Vec<bool>,
    pub vacuum_pumps: Vec<bool>,
    pub sjae_on: bool,
    pub sjae_effective: bool,
    pub pegging_steam: bool,
    pub msiv_open: bool,
    pub condenser_available: bool,
    /// Turbine hall local checks complete (comms).
    pub turbine_hall_ready: bool,
    /// Electrical distribution: breakers, buses, transformers, diesels, DC.
    pub electrical: serde_json::Value,
    /// Grid dispatch permission to synchronise (comms).
    pub grid_permission: bool,
    pub generator_lockout: bool,
    /// Sync-check relay is supervising breaker closure.
    pub sync_check_active: bool,
    /// "PHASE Δ OUT OF RANGE" lamp (a close was just blocked).
    pub sync_blocked: bool,
    pub nis: NisView,
    /// Generation of every control board section that an upgrade can
    /// modernise (`"turbine.sync" -> digital`). Sections not listed are analog.
    pub sections: BTreeMap<String, ControlRoomGeneration>,
    pub upgrades: Vec<UpgradeView>,
    pub equipment: crate::equipment::Equipment,
    pub required_kits: Vec<crate::equipment::ConversionKit>,
    pub scenario: String,
    pub boration: BorationMode,
    pub reactor_tripped: bool,
    pub turbine_tripped: bool,
    pub trips: Vec<String>,
    pub alarms: Vec<Alarm>,
    pub modules: Vec<ModuleView>,
}
