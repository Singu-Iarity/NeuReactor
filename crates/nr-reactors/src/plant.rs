//! Reactor-type independent plant bookkeeping: automation module state and
//! the event log.

use nr_api::{modules, GameMode, ModuleId, ModuleMode, Severity, UpgradeId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct AutomationState {
    pub game_mode: GameMode,
    /// Installed automation modules.
    pub owned: BTreeSet<ModuleId>,
    /// Installed I&C hardware upgrades.
    pub upgrades: BTreeSet<UpgradeId>,
    /// Physical component variants, controller variants and conversion kits.
    pub equipment: nr_api::Equipment,
    pub modes: BTreeMap<ModuleId, ModuleMode>,
    pub setpoints: BTreeMap<ModuleId, BTreeMap<String, f64>>,
}

impl AutomationState {
    pub fn new() -> Self {
        let mut s = Self {
            game_mode: GameMode::Sandbox,
            // Sandbox default: fully equipped unit. The instructor station can
            // strip it back to the original analog/manual plant or toggle
            // individual items; career starts from the original plant.
            owned: modules::CATALOG.iter().map(|m| m.id).collect(),
            upgrades: nr_api::upgrades::CATALOG.iter().map(|u| u.id).collect(),
            equipment: nr_api::Equipment::standard(),
            modes: BTreeMap::new(),
            setpoints: BTreeMap::new(),
        };
        let defaults: &[(ModuleId, &str, f64)] = &[
            (ModuleId::RodControl, "tavg_full_c", 308.6),
            (ModuleId::RodControl, "tavg_noload_c", 291.7),
            (ModuleId::BoronControl, "band_low_steps", 170.0),
            (ModuleId::BoronControl, "band_high_steps", 215.0),
            (ModuleId::PressurizerPressure, "pressure_mpa", 15.51),
            (ModuleId::PressurizerLevel, "level_full_pct", 55.0),
            (ModuleId::PressurizerLevel, "level_noload_pct", 25.0),
            (ModuleId::FeedwaterLevel, "level_pct", 50.0),
            (ModuleId::SteamDump, "noload_tavg_c", 291.7),
            (ModuleId::TurbineGovernor, "target_mwe", 0.0),
            (ModuleId::TurbineGovernor, "ramp_mw_min", 30.0),
            (ModuleId::TurbineGovernor, "speed_rpm", 1800.0),
            (ModuleId::TurbineGovernor, "accel_rpm_min", 120.0),
            (ModuleId::Excitation, "voltage_kv", 22.0),
            (ModuleId::PlantMaster, "target_mwe", 0.0),
        ];
        for (m, k, v) in defaults {
            s.setpoint_set(*m, k, *v);
        }
        s
    }

    pub fn mode(&self, m: ModuleId) -> ModuleMode {
        self.modes.get(&m).copied().unwrap_or_default()
    }

    pub fn setpoint(&self, m: ModuleId, key: &str) -> Option<f64> {
        self.setpoints.get(&m)?.get(key).copied()
    }

    pub fn setpoint_set(&mut self, m: ModuleId, key: &str, v: f64) {
        self.setpoints
            .entry(m)
            .or_default()
            .insert(key.to_string(), v);
    }
}

impl Default for AutomationState {
    fn default() -> Self {
        Self::new()
    }
}

/// A logged plant event (for the UI event log and plugins).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlantEvent {
    pub severity: Severity,
    pub text: String,
}

impl PlantEvent {
    pub fn info(t: &str) -> Self {
        Self {
            severity: Severity::Info,
            text: t.into(),
        }
    }
    pub fn warn(t: &str) -> Self {
        Self {
            severity: Severity::Warning,
            text: t.into(),
        }
    }
    pub fn trip(t: &str) -> Self {
        Self {
            severity: Severity::Trip,
            text: t.into(),
        }
    }
}
