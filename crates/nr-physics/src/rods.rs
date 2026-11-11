//! Control rod banks: position, drive motion, scram and integral worth.

use nr_core::clamp;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RodBank {
    pub name: String,
    /// Withdrawn position in steps (0 = fully inserted).
    pub position: f64,
    /// Fully withdrawn step count.
    pub max_steps: f64,
    /// Total bank worth (positive Δk/k removed when fully inserted).
    pub worth: f64,
    /// Demanded position (steps) for manual moves.
    pub target: f64,
    /// Is this a shutdown bank (only fully in / fully out in normal operation)?
    pub shutdown_bank: bool,
}

impl RodBank {
    pub fn new(name: &str, max_steps: f64, worth: f64, position: f64, shutdown_bank: bool) -> Self {
        Self {
            name: name.to_string(),
            position,
            max_steps,
            worth,
            target: position,
            shutdown_bank,
        }
    }

    /// Fraction inserted, 0 = out, 1 = in.
    pub fn insertion(&self) -> f64 {
        1.0 - self.position / self.max_steps
    }

    /// Negative reactivity currently inserted by this bank. Uses the classic
    /// S-curve integral worth for a bank in a cosine-shaped axial flux.
    pub fn reactivity(&self) -> f64 {
        let x = clamp(self.insertion(), 0.0, 1.0);
        -self.worth * (x - (2.0 * PI * x).sin() / (2.0 * PI))
    }

    /// Drive the bank toward `target` at `speed` steps/s.
    pub fn drive(&mut self, speed: f64, dt: f64) {
        let max = speed * dt;
        self.position += clamp(self.target - self.position, -max, max);
        self.position = clamp(self.position, 0.0, self.max_steps);
    }

    /// Gravity drop during scram: full insertion in ~`drop_time` seconds.
    pub fn drop(&mut self, drop_time: f64, dt: f64) {
        self.position = (self.position - self.max_steps * dt / drop_time).max(0.0);
        self.target = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_curve_worth() {
        let mut b = RodBank::new("D", 228.0, 0.01, 228.0, false);
        assert!(b.reactivity().abs() < 1e-12);
        b.position = 114.0;
        assert!((b.reactivity() + 0.005).abs() < 1e-9);
        b.position = 0.0;
        assert!((b.reactivity() + 0.01).abs() < 1e-12);
    }

    #[test]
    fn drive_and_drop() {
        let mut b = RodBank::new("D", 228.0, 0.01, 200.0, false);
        b.target = 100.0;
        b.drive(72.0 / 60.0, 10.0);
        assert!((b.position - 188.0).abs() < 1e-9);
        for _ in 0..30 {
            b.drop(2.0, 0.1);
        }
        assert_eq!(b.position, 0.0);
    }
}
