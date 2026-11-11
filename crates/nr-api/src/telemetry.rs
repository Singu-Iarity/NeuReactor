//! Telemetry snapshot: what the control room shows.

use crate::commands::BorationMode;
use crate::modules::{ModuleId, ModuleMode};
use crate::policy::GameMode;
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
    pub text: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RodView {
    pub bank: String,
    pub position: f64,
    pub target: f64,
    pub max_steps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleView {
    pub id: ModuleId,
    pub name: String,
    pub panel: String,
    pub mode: ModuleMode,
    pub owned: bool,
    pub implemented: bool,
    pub setpoints: BTreeMap<String, f64>,
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
    pub coolant_pumps: Vec<bool>,
    pub generator_breaker: bool,
    pub boration: BorationMode,
    pub reactor_tripped: bool,
    pub turbine_tripped: bool,
    pub trips: Vec<String>,
    pub alarms: Vec<Alarm>,
    pub modules: Vec<ModuleView>,
}
