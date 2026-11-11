//! Main condenser, circulating water, air removal (vacuum pumps and steam
//! jet air ejectors), hotwell and deaerator.
//!
//! Condenser absolute pressure = saturation pressure of the condensing
//! temperature + partial pressure of non-condensable air. The condensing
//! temperature follows the heat load and circulating water flow. Air leaks in
//! continuously and is removed by:
//! * mechanical vacuum ("hogging") pumps: pull down from atmosphere, but
//!   lose effectiveness below ~15 kPa;
//! * steam jet air ejectors (SJAEs): need main steam as motive steam, weak at
//!   high pressure, but hold deep vacuum against normal in-leakage.

use nr_core::{clamp, steam};
use serde::{Deserialize, Serialize};

pub const ATMOSPHERE_KPA: f64 = 101.3;
/// Design condenser pressure at full load [kPa].
pub const DESIGN_KPA: f64 = 7.4;
/// Partial pressure per kg of air in the condenser shell [kPa/kg].
const KPA_PER_KG_AIR: f64 = 0.5;
const AIR_LEAK: f64 = 0.0025; // kg/s
const VACUUM_PUMP_TAU: f64 = 900.0; // s per pump at full effectiveness
const SJAE_TAU: f64 = 600.0; // s
/// Minimum main steam pressure for SJAE motive steam [Pa].
pub const SJAE_MIN_STEAM: f64 = 1.5e6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condenser {
    pub pressure_kpa: f64,
    pub air_kg: f64,
    pub cw_pumps: [bool; 3],
    cw_speed: [f64; 3],
    pub vacuum_pumps: [bool; 2],
    pub sjae_on: bool,
    pub sjae_effective: bool,
    pub cw_inlet_c: f64,
    pub condensing_c: f64,
    /// Hotwell level [%].
    pub hotwell_pct: f64,
    /// Cooling capacity multiplier (equipment quality).
    pub capacity: f64,
    /// Air removal multiplier (equipment quality).
    pub air_removal: f64,
}

impl Condenser {
    /// Design vacuum, all CW pumps and the SJAEs running (plant at power).
    pub fn at_power() -> Self {
        let mut c = Self::at_atmosphere();
        c.air_kg = AIR_LEAK * SJAE_TAU;
        c.sjae_on = true;
        c.sjae_effective = true;
        c.pressure_kpa = DESIGN_KPA;
        c.condensing_c = 38.5;
        c
    }

    /// Shell at atmospheric pressure, CW pumps running, air removal off.
    pub fn at_atmosphere() -> Self {
        Self {
            pressure_kpa: ATMOSPHERE_KPA,
            air_kg: (ATMOSPHERE_KPA - 4.0) / KPA_PER_KG_AIR,
            cw_pumps: [true; 3],
            cw_speed: [1.0; 3],
            vacuum_pumps: [false; 2],
            sjae_on: false,
            sjae_effective: false,
            cw_inlet_c: 18.0,
            condensing_c: 30.0,
            hotwell_pct: 50.0,
            capacity: 1.0,
            air_removal: 1.0,
        }
    }

    pub fn cw_flow_frac(&self) -> f64 {
        self.cw_speed.iter().sum::<f64>() / 3.0
    }

    /// Condenser can accept steam (vacuum below the C-9 interlock).
    pub fn available(&self) -> bool {
        self.pressure_kpa < 25.0 && self.cw_flow_frac() > 0.3
    }

    /// Cycle efficiency multiplier relative to design vacuum.
    pub fn efficiency(&self) -> f64 {
        clamp(1.0 - 0.012 * (self.pressure_kpa - DESIGN_KPA), 0.4, 1.03)
    }

    /// Advance. `q_in` = heat rejected to the condenser [W], `steam_pressure`
    /// = main steam pressure available for SJAE motive steam [Pa].
    pub fn step(&mut self, q_in: f64, steam_pressure: f64, msiv_open: bool, dt: f64) {
        for i in 0..3 {
            let target = if self.cw_pumps[i] { 1.0 } else { 0.0 };
            let tau = if self.cw_pumps[i] { 8.0 } else { 20.0 };
            self.cw_speed[i] += (target - self.cw_speed[i]) * (dt / tau).min(1.0);
        }
        // Condensing temperature: CW temperature rise + terminal difference.
        let cw_flow = 45_000.0 * self.cw_flow_frac() * self.capacity; // kg/s
        let ua = 3.8e8 * self.cw_flow_frac().max(0.02).sqrt() * self.capacity;
        let rise = if cw_flow > 1.0 {
            q_in / (cw_flow * 4186.0)
        } else {
            q_in / 4.0e6
        };
        let target_t = self.cw_inlet_c + rise + q_in / ua + 3.0;
        self.condensing_c +=
            (clamp(target_t, 5.0, 110.0) - self.condensing_c) * (dt / 30.0).min(1.0);

        // Air in-leakage and removal.
        let p = self.pressure_kpa;
        let vp_eff = clamp((p - 12.0) / 10.0, 0.0, 1.0);
        let pumps = self.vacuum_pumps.iter().filter(|v| **v).count() as f64;
        self.sjae_effective = self.sjae_on && msiv_open && steam_pressure > SJAE_MIN_STEAM;
        let sjae_eff = if self.sjae_effective {
            if p > 40.0 {
                0.3
            } else {
                1.0
            }
        } else {
            0.0
        };
        let k = self.air_removal * (pumps * vp_eff / VACUUM_PUMP_TAU + sjae_eff / SJAE_TAU);
        self.air_kg += (AIR_LEAK - k * self.air_kg) * dt;
        self.air_kg = self.air_kg.max(0.0);

        let p_vapor = steam::psat(self.condensing_c.max(33.0)) / 1000.0;
        self.pressure_kpa = (p_vapor + self.air_kg * KPA_PER_KG_AIR).min(ATMOSPHERE_KPA);
    }
}

/// Deaerating feedwater heater. Oxygen is only stripped when the deaerator is
/// above ~0.15 MPa (saturation ~111 °C), heated by turbine extraction steam
/// at load or by pegging steam from main steam at low load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deaerator {
    pub pressure_mpa: f64,
    pub pegging_steam: bool,
    pub level_pct: f64,
}

impl Deaerator {
    pub fn at_power() -> Self {
        Self {
            pressure_mpa: 0.9,
            pegging_steam: false,
            level_pct: 55.0,
        }
    }

    pub fn cold() -> Self {
        Self {
            pressure_mpa: 0.11,
            pegging_steam: false,
            level_pct: 55.0,
        }
    }

    pub fn effective(&self) -> bool {
        self.pressure_mpa > 0.15
    }

    pub fn temperature(&self) -> f64 {
        steam::tsat(self.pressure_mpa * 1e6)
    }

    /// `load` = turbine extraction availability 0..1.
    pub fn step(&mut self, load: f64, main_steam_ok: bool, dt: f64) {
        let extraction = 0.12 + 0.85 * clamp(load, 0.0, 1.0);
        let pegging = if self.pegging_steam && main_steam_ok {
            0.25
        } else {
            0.0
        };
        let target = if load > 0.02 {
            extraction.max(pegging)
        } else {
            pegging.max(0.105)
        };
        self.pressure_mpa += (target - self.pressure_mpa) * (dt / 120.0).min(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(c: &mut Condenser, s: f64, q: f64) {
        for _ in 0..(s / 1.0) as usize {
            c.step(q, 7.0e6, true, 1.0);
        }
    }

    #[test]
    fn design_vacuum_holds_at_power() {
        let mut c = Condenser::at_power();
        run(&mut c, 3600.0, 2.26e9);
        assert!(
            (5.0..10.0).contains(&c.pressure_kpa),
            "p = {}",
            c.pressure_kpa
        );
    }

    #[test]
    fn hogging_then_sjae_pulldown() {
        let mut c = Condenser::at_atmosphere();
        c.vacuum_pumps = [true, true];
        run(&mut c, 1500.0, 20e6);
        assert!(c.pressure_kpa < 25.0, "after hogging {}", c.pressure_kpa);
        c.sjae_on = true;
        run(&mut c, 2400.0, 20e6);
        c.vacuum_pumps = [false, false];
        run(&mut c, 1800.0, 20e6);
        assert!(c.pressure_kpa < 8.0, "after SJAE {}", c.pressure_kpa);
    }

    #[test]
    fn sjae_alone_is_slow_and_needs_steam() {
        let mut c = Condenser::at_atmosphere();
        c.sjae_on = true;
        for _ in 0..1800 {
            c.step(0.0, 0.5e6, true, 1.0);
        }
        assert!(c.pressure_kpa > 95.0, "no motive steam: {}", c.pressure_kpa);
        run(&mut c, 1800.0, 0.0);
        assert!(
            c.pressure_kpa > 40.0,
            "SJAE alone 30 min: {}",
            c.pressure_kpa
        );
    }

    #[test]
    fn losing_cw_destroys_vacuum() {
        let mut c = Condenser::at_power();
        c.cw_pumps = [false; 3];
        run(&mut c, 600.0, 2.26e9);
        assert!(c.pressure_kpa > 30.0, "p = {}", c.pressure_kpa);
    }

    #[test]
    fn deaerator_needs_steam() {
        let mut d = Deaerator::cold();
        for _ in 0..1200 {
            d.step(0.0, true, 1.0);
        }
        assert!(!d.effective());
        d.pegging_steam = true;
        for _ in 0..1200 {
            d.step(0.0, true, 1.0);
        }
        assert!(d.effective());
    }
}
