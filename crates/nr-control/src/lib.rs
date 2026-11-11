//! NeuReactor instrumentation & control.
//!
//! * [`controllers`]: one controller per automation module. They are plain
//!   signal-in/signal-out blocks so any reactor type can wire them up.
//! * [`rps`]: the reactor protection system (trip logic), always active.

pub mod controllers;
pub mod rps;

pub use rps::{Rps, RpsInputs, RpsSetpoints};
