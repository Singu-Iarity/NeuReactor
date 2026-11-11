//! NeuReactor public API.
//!
//! Everything that talks to a running plant — the built-in control room UI,
//! the CLI, tests, and third-party plugins — goes through the types in this
//! crate:
//!
//! * [`Command`]: every operator action, tagged by the actuator it moves.
//! * [`Telemetry`]: what the control room instruments show.
//! * [`modules`]: the per-subsystem automation module catalog.
//! * [`policy`]: who may issue which command in which game mode.

pub mod commands;
pub mod modules;
pub mod policy;
pub mod telemetry;

pub use commands::{Actuator, BorationMode, Command, CommandKind};
pub use modules::{ModuleId, ModuleInfo, ModuleMode, CATALOG};
pub use policy::{authorize, can_read, Denied, GameMode, Origin, PolicyContext, Scope};
pub use telemetry::{Alarm, ModuleView, RodView, Severity, Telemetry};

/// API version exposed at `/api/v1`.
pub const API_VERSION: u32 = 1;
