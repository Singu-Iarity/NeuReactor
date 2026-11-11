//! Fission product poisons: I-135 → Xe-135 and Pm-149 → Sm-149.
//!
//! Concentrations are kept in normalised units (fission-rate weighted), so the
//! only reactor-specific input is the nominal thermal flux and the calibrated
//! reactivity worth of equilibrium full-power xenon / samarium.

use serde::{Deserialize, Serialize};

const LAMBDA_I: f64 = 2.93e-5; // 1/s  (T½ 6.57 h)
const LAMBDA_XE: f64 = 2.11e-5; // 1/s (T½ 9.14 h)
const LAMBDA_PM: f64 = 3.63e-6; // 1/s (T½ 53.1 h)
const SIGMA_XE: f64 = 2.65e-18; // cm²
const SIGMA_SM: f64 = 4.1e-20; // cm²
const GAMMA_I: f64 = 0.0639;
const GAMMA_XE: f64 = 0.00237;
const GAMMA_PM: f64 = 0.0113;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poisons {
    /// Nominal (100 % power) average thermal flux [n/cm²/s].
    pub flux_nominal: f64,
    /// Reactivity of equilibrium xenon at 100 % power (negative).
    pub xe_worth_full: f64,
    /// Reactivity of equilibrium samarium at 100 % power (negative).
    pub sm_worth_full: f64,
    pub iodine: f64,
    pub xenon: f64,
    pub promethium: f64,
    pub samarium: f64,
}

impl Poisons {
    /// Equilibrium inventories for relative power `p`.
    pub fn equilibrium(flux_nominal: f64, xe_worth_full: f64, sm_worth_full: f64, p: f64) -> Self {
        let mut s = Self {
            flux_nominal,
            xe_worth_full,
            sm_worth_full,
            iodine: 0.0,
            xenon: 0.0,
            promethium: 0.0,
            samarium: 0.0,
        };
        let phi = flux_nominal * p;
        s.iodine = GAMMA_I * p / LAMBDA_I;
        s.xenon = (GAMMA_I + GAMMA_XE) * p / (LAMBDA_XE + SIGMA_XE * phi);
        s.promethium = GAMMA_PM * p / LAMBDA_PM;
        s.samarium = if phi > 0.0 {
            GAMMA_PM * p / (SIGMA_SM * phi)
        } else {
            0.0
        };
        s
    }

    fn xenon_eq_full(&self) -> f64 {
        (GAMMA_I + GAMMA_XE) / (LAMBDA_XE + SIGMA_XE * self.flux_nominal)
    }

    fn samarium_eq_full(&self) -> f64 {
        GAMMA_PM / (SIGMA_SM * self.flux_nominal)
    }

    /// Advance by `dt` at relative power `p` (exact exponential update per step).
    pub fn step(&mut self, p: f64, dt: f64) {
        let p = p.max(0.0);
        let phi = self.flux_nominal * p;
        let i_src = GAMMA_I * p;
        let decay_i = LAMBDA_I * self.iodine;
        self.iodine = exp_relax(self.iodine, i_src / LAMBDA_I, LAMBDA_I, dt);

        let xe_rem = LAMBDA_XE + SIGMA_XE * phi;
        let xe_src = GAMMA_XE * p + decay_i;
        self.xenon = exp_relax(self.xenon, xe_src / xe_rem, xe_rem, dt);

        let decay_pm = LAMBDA_PM * self.promethium;
        self.promethium = exp_relax(self.promethium, GAMMA_PM * p / LAMBDA_PM, LAMBDA_PM, dt);
        self.samarium += dt * (decay_pm - SIGMA_SM * phi * self.samarium);
        self.samarium = self.samarium.max(0.0);
    }

    pub fn xenon_reactivity(&self) -> f64 {
        self.xe_worth_full * self.xenon / self.xenon_eq_full()
    }

    pub fn samarium_reactivity(&self) -> f64 {
        self.sm_worth_full * self.samarium / self.samarium_eq_full()
    }
}

/// Exact solution of y' = -k (y - target) over dt.
fn exp_relax(y: f64, target: f64, k: f64, dt: f64) -> f64 {
    target + (y - target) * (-k * dt).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pwr() -> Poisons {
        Poisons::equilibrium(3.0e13, -0.028, -0.007, 1.0)
    }

    #[test]
    fn equilibrium_is_steady() {
        let mut p = pwr();
        let x0 = p.xenon_reactivity();
        for _ in 0..100_000 {
            p.step(1.0, 10.0);
        }
        assert!((x0 + 0.028).abs() < 1e-12);
        assert!((p.xenon_reactivity() - x0).abs() < 1e-6);
        assert!((p.samarium_reactivity() + 0.007).abs() < 1e-4);
    }

    #[test]
    fn xenon_peaks_after_shutdown() {
        let mut p = pwr();
        let mut peak = (0.0, p.xenon_reactivity());
        let dt = 60.0;
        for i in 1..(48 * 60) {
            p.step(0.0, dt);
            let r = p.xenon_reactivity();
            if r < peak.1 {
                peak = (i as f64 * dt / 3600.0, r);
            }
        }
        assert!((7.0..12.0).contains(&peak.0), "peak at {} h", peak.0);
        // Peak xenon worth is substantially larger than equilibrium.
        assert!(peak.1 < -0.04, "peak worth {}", peak.1);
    }
}
