//! NeuReactor plant models.
//!
//! Currently: a four-loop PWR. BWR, RBMK-1000/1500 and sodium fast breeder
//! plants will be added here, assembled from the same `nr-physics`
//! components and `nr-control` modules.

pub mod core_map;
pub mod plant;
pub mod pwr;

pub use plant::{AutomationState, PlantEvent};
pub use pwr::Pwr;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_startup;
