//! NeuReactor core numerics.
//!
//! Small, dependency-free building blocks shared by every other crate:
//! interpolation tables, a PI controller, rate limiters, a deterministic RNG
//! and saturated water/steam properties.

pub mod math;
pub mod serde_f64;
pub mod steam;

pub use math::{clamp, lerp, Pi, RateLimiter, Rng, Table1D};

/// Seconds per minute, used for "per minute" plant rates (rod steps/min etc.).
pub const MINUTE: f64 = 60.0;
/// Pascals per megapascal.
pub const MPA: f64 = 1.0e6;
/// Celsius to Kelvin offset.
pub const KELVIN: f64 = 273.15;
