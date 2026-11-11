//! Reactor protection system (trip logic).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpsSetpoints {
    pub high_flux_pct: f64,
    pub high_pressure_mpa: f64,
    pub low_pressure_mpa: f64,
    pub high_pzr_level_pct: f64,
    pub low_flow_pct: f64,
    pub low_low_sg_level_pct: f64,
    pub over_temp_dt_pct: f64,
    /// P-7: low-power permissive (blocks at-power trips below it).
    pub p7_pct: f64,
    /// P-9: turbine trip -> reactor trip above this power.
    pub p9_pct: f64,
    pub high_startup_rate_dpm: f64,
}

impl Default for RpsSetpoints {
    fn default() -> Self {
        Self {
            high_flux_pct: 109.0,
            high_pressure_mpa: 16.55,
            low_pressure_mpa: 13.7,
            high_pzr_level_pct: 92.0,
            low_flow_pct: 90.0,
            low_low_sg_level_pct: 12.0,
            over_temp_dt_pct: 115.0,
            p7_pct: 10.0,
            p9_pct: 50.0,
            high_startup_rate_dpm: 5.0,
        }
    }
}

/// Measured (instrument) values the RPS acts on.
#[derive(Debug, Clone, Default)]
pub struct RpsInputs {
    pub flux_pct: f64,
    pub pressure_mpa: f64,
    pub pzr_level_pct: f64,
    pub flow_pct: f64,
    pub min_sg_level_pct: f64,
    pub delta_t_pct: f64,
    pub startup_rate_dpm: f64,
    pub turbine_tripped: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Rps {
    pub setpoints: RpsSetpoints,
}

impl Rps {
    /// Returns the list of trip signals currently present.
    pub fn evaluate(&self, i: &RpsInputs) -> Vec<&'static str> {
        let s = &self.setpoints;
        let mut trips = Vec::new();
        let at_power = i.flux_pct > s.p7_pct;
        if i.flux_pct > s.high_flux_pct {
            trips.push("Power range high flux");
        }
        if i.startup_rate_dpm > s.high_startup_rate_dpm && i.flux_pct < s.p7_pct {
            trips.push("High startup rate");
        }
        if i.pressure_mpa > s.high_pressure_mpa {
            trips.push("Pressurizer pressure high");
        }
        if at_power && i.pressure_mpa < s.low_pressure_mpa {
            trips.push("Pressurizer pressure low");
        }
        if at_power && i.pzr_level_pct > s.high_pzr_level_pct {
            trips.push("Pressurizer level high");
        }
        if at_power && i.flow_pct < s.low_flow_pct {
            trips.push("RCS low flow");
        }
        if i.min_sg_level_pct < s.low_low_sg_level_pct {
            trips.push("SG level low-low");
        }
        if i.delta_t_pct > s.over_temp_dt_pct {
            trips.push("Overtemperature ΔT");
        }
        if i.turbine_tripped && i.flux_pct > s.p9_pct {
            trips.push("Turbine trip above P-9");
        }
        trips
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal() -> RpsInputs {
        RpsInputs {
            flux_pct: 100.0,
            pressure_mpa: 15.5,
            pzr_level_pct: 55.0,
            flow_pct: 100.0,
            min_sg_level_pct: 50.0,
            delta_t_pct: 100.0,
            startup_rate_dpm: 0.0,
            turbine_tripped: false,
        }
    }

    #[test]
    fn no_trips_at_normal_full_power() {
        assert!(Rps::default().evaluate(&normal()).is_empty());
    }

    #[test]
    fn low_flow_trip_only_above_p7() {
        let rps = Rps::default();
        let mut i = normal();
        i.flow_pct = 50.0;
        assert_eq!(rps.evaluate(&i), vec!["RCS low flow"]);
        i.flux_pct = 5.0;
        assert!(rps.evaluate(&i).is_empty());
    }
}
