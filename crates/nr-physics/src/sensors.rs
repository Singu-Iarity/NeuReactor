//! Instrument model: what operators (and plugins) actually see.
//!
//! A sensor reads the true process value through a first-order lag, adds
//! noise, a slow drift and range limits, and can fail. Analog vs digital
//! instrumentation upgrades change these parameters.

use nr_core::{clamp, Rng};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorFault {
    None,
    /// Reading frozen at last value.
    Stuck,
    /// Reading pinned to the bottom of range.
    FailLow,
    /// Reading pinned to the top of range.
    FailHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensor {
    pub tau: f64,
    pub noise: f64,
    pub drift_rate: f64,
    pub range: (f64, f64),
    pub fault: SensorFault,
    pub value: f64,
    pub drift: f64,
}

impl Sensor {
    pub fn new(initial: f64, tau: f64, noise: f64, range: (f64, f64)) -> Self {
        Self {
            tau,
            noise,
            drift_rate: 0.0,
            range,
            fault: SensorFault::None,
            value: initial,
            drift: 0.0,
        }
    }

    pub fn update(&mut self, truth: f64, dt: f64, rng: &mut Rng) -> f64 {
        match self.fault {
            SensorFault::Stuck => return self.value,
            SensorFault::FailLow => {
                self.value = self.range.0;
                return self.value;
            }
            SensorFault::FailHigh => {
                self.value = self.range.1;
                return self.value;
            }
            SensorFault::None => {}
        }
        self.drift += self.drift_rate * dt;
        let a = if self.tau > 0.0 {
            (dt / self.tau).min(1.0)
        } else {
            1.0
        };
        let lagged = self.value + a * (truth + self.drift - self.value);
        self.value = lagged;
        clamp(
            lagged + self.noise * rng.normal(),
            self.range.0,
            self.range.1,
        )
    }
}
