//! Point reactor kinetics with six delayed-neutron precursor groups.
//!
//! Integrated with a fully implicit (backward Euler) scheme which is
//! unconditionally stable for the stiff prompt-neutron term, so the plant can
//! run at large time steps and high time acceleration. Spatial (nodal) kinetics
//! is the next fidelity step on the roadmap and will reuse these constants.

use serde::{Deserialize, Serialize};

/// Delayed neutron data for one fissile mix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelayedNeutronData {
    /// Group fractions β_i.
    pub beta: [f64; 6],
    /// Group decay constants λ_i [1/s].
    pub lambda: [f64; 6],
    /// Prompt neutron generation time Λ [s].
    pub generation_time: f64,
}

impl DelayedNeutronData {
    /// Thermal U-235 (Keepin) data with a typical LWR generation time.
    pub fn u235_thermal() -> Self {
        Self {
            beta: [0.000215, 0.001424, 0.001274, 0.002568, 0.000748, 0.000273],
            lambda: [0.0124, 0.0305, 0.111, 0.301, 1.14, 3.01],
            generation_time: 2.0e-5,
        }
    }

    pub fn beta_total(&self) -> f64 {
        self.beta.iter().sum()
    }

    /// Reactivity that yields a stable positive period `period` [s] (inhour equation).
    pub fn inhour(&self, period: f64) -> f64 {
        let w = 1.0 / period;
        self.generation_time * w
            + self
                .beta
                .iter()
                .zip(&self.lambda)
                .map(|(b, l)| b * w / (w + l))
                .sum::<f64>()
    }
}

/// Kinetics state: relative neutron density `n` (1.0 = nominal full power)
/// and precursor concentrations in the same normalisation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointKinetics {
    pub data: DelayedNeutronData,
    pub n: f64,
    pub precursors: [f64; 6],
    /// External neutron source (startup source / spontaneous fission), in n/s units.
    pub source: f64,
    /// Smoothed reactor period [s]; `f64::INFINITY` when steady.
    #[serde(with = "nr_core::serde_f64")]
    pub period: f64,
}

impl PointKinetics {
    /// Critical equilibrium at relative power `n`.
    pub fn equilibrium(data: DelayedNeutronData, n: f64) -> Self {
        let mut c = [0.0; 6];
        for (i, ci) in c.iter_mut().enumerate() {
            *ci = data.beta[i] / (data.lambda[i] * data.generation_time) * n;
        }
        Self {
            data,
            n,
            precursors: c,
            source: 2.0e-6,
            period: f64::INFINITY,
        }
    }

    /// Advance by `dt` seconds at constant reactivity `rho` (Δk/k).
    pub fn step(&mut self, rho: f64, dt: f64) {
        let d = &self.data;
        let lam_gen = d.generation_time;
        let beta = d.beta_total();
        // Backward Euler: C_i' = (C_i + dt β_i/Λ n') / (1 + dt λ_i)
        // Substituting into the neutron balance gives a scalar linear equation for n'.
        let mut a = 1.0 - dt * (rho - beta) / lam_gen;
        let mut rhs = self.n + dt * self.source;
        for i in 0..6 {
            let denom = 1.0 + dt * d.lambda[i];
            a -= dt * d.lambda[i] * dt * d.beta[i] / lam_gen / denom;
            rhs += dt * d.lambda[i] * self.precursors[i] / denom;
        }
        let n_new = (rhs / a).max(1e-14);
        for i in 0..6 {
            self.precursors[i] =
                (self.precursors[i] + dt * d.beta[i] / lam_gen * n_new) / (1.0 + dt * d.lambda[i]);
        }
        // Instantaneous inverse period, exponentially smoothed (~1 s) like a period meter.
        let inv = (n_new / self.n).ln() / dt;
        let prev_inv = if self.period.is_finite() {
            1.0 / self.period
        } else {
            0.0
        };
        let alpha = (dt / 1.0).min(1.0);
        let smoothed = prev_inv + alpha * (inv - prev_inv);
        self.period = if smoothed.abs() < 1e-6 {
            f64::INFINITY
        } else {
            1.0 / smoothed
        };
        self.n = n_new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equilibrium_is_steady() {
        let mut k = PointKinetics::equilibrium(DelayedNeutronData::u235_thermal(), 1.0);
        k.source = 0.0;
        for _ in 0..10_000 {
            k.step(0.0, 0.05);
        }
        assert!((k.n - 1.0).abs() < 1e-9);
    }

    #[test]
    fn prompt_jump_matches_theory() {
        let data = DelayedNeutronData::u235_thermal();
        let beta = data.beta_total();
        let rho = 0.1 * beta;
        let mut k = PointKinetics::equilibrium(data, 1.0);
        k.source = 0.0;
        // Prompt jump completes in ~ a few Λ/(β-ρ) ≈ 5 ms; sample at 0.1 s.
        for _ in 0..1000 {
            k.step(rho, 1e-4);
        }
        let expected = beta / (beta - rho);
        assert!(
            (k.n / expected - 1.0).abs() < 0.02,
            "n = {}, expected {}",
            k.n,
            expected
        );
    }

    #[test]
    fn stable_period_matches_inhour() {
        let data = DelayedNeutronData::u235_thermal();
        let rho = 0.001; // 100 pcm
        let mut k = PointKinetics::equilibrium(data.clone(), 1.0);
        k.source = 0.0;
        let dt = 0.01;
        for _ in 0..(300.0 / dt) as usize {
            k.step(rho, dt);
        }
        // Find T such that inhour(T) == rho by bisection.
        let (mut lo, mut hi) = (1.0, 10_000.0);
        for _ in 0..100 {
            let mid = 0.5 * (lo + hi);
            if data.inhour(mid) > rho {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let expected = 0.5 * (lo + hi);
        assert!(
            (k.period / expected - 1.0).abs() < 0.03,
            "T = {}, expected {}",
            k.period,
            expected
        );
    }

    #[test]
    fn large_steps_stay_stable() {
        let mut k = PointKinetics::equilibrium(DelayedNeutronData::u235_thermal(), 1.0);
        for _ in 0..1000 {
            k.step(-0.08, 1.0);
        }
        assert!(k.n > 0.0 && k.n < 1e-3);
    }
}
