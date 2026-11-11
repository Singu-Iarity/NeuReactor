//! Fission product decay heat.
//!
//! Nine exponential groups fitted (non-negative least squares) to the
//! Way–Wigner curve P/P0 = 0.0622 t^-0.2 for an infinitely long irradiation,
//! valid from ~1 s to several weeks after shutdown. Will be replaced by full
//! ANS-5.1 group data (with finite irradiation history) on the roadmap.

use serde::{Deserialize, Serialize};

/// (fraction of fission power, decay constant [1/s])
const GROUPS: [(f64, f64); 9] = [
    (1.6702e-2, 1.0),
    (1.2828e-2, 3.0e-1),
    (9.4430e-3, 1.0e-1),
    (7.5934e-3, 3.0e-2),
    (6.8898e-3, 1.0e-2),
    (3.5671e-3, 3.0e-3),
    (5.9655e-3, 1.0e-3),
    (6.1473e-3, 1.0e-4),
    (7.6982e-3, 1.0e-6),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayHeat {
    /// Decay power per group, relative to nominal power.
    pub groups: [f64; 9],
}

impl DecayHeat {
    /// Total fraction of fission energy released as delayed decay heat.
    pub fn delayed_fraction() -> f64 {
        GROUPS.iter().map(|g| g.0).sum()
    }

    /// Equilibrium after long operation at relative fission power `p`.
    pub fn equilibrium(p: f64) -> Self {
        let mut groups = [0.0; 9];
        for (g, (f, _)) in groups.iter_mut().zip(GROUPS) {
            *g = f * p;
        }
        Self { groups }
    }

    /// Advance with relative fission power `p`.
    pub fn step(&mut self, p: f64, dt: f64) {
        for (g, (f, l)) in self.groups.iter_mut().zip(GROUPS) {
            let target = f * p;
            *g = target + (*g - target) * (-l * dt).exp();
        }
    }

    /// Current decay heat relative to nominal power.
    pub fn power(&self) -> f64 {
        self.groups.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_way_wigner_after_shutdown() {
        let mut d = DecayHeat::equilibrium(1.0);
        let checkpoints = [1.0, 10.0, 100.0, 1000.0, 3600.0, 86_400.0];
        let mut t = 0.0;
        let dt = 0.1;
        for &cp in &checkpoints {
            while t < cp - 1e-9 {
                let h = if t < 100.0 { dt } else { 10.0 };
                d.step(0.0, h);
                t += h;
            }
            let ww = 0.0622 * t.powf(-0.2);
            let err = d.power() / ww - 1.0;
            assert!(err.abs() < 0.15, "t={t}: {} vs {}", d.power(), ww);
        }
    }
}
