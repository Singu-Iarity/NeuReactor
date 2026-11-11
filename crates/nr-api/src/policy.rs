//! Access policy for operators and plugins.
//!
//! Goal: plugins can read almost everything and build great tools, but in
//! career mode they can never automate a subsystem the player has not bought.
//! A plugin may only drive actuators whose owning module is owned *and* placed
//! in [`ModuleMode::External`] by the operator, which itself requires the
//! purchased External Control Gateway.

use crate::commands::{Command, CommandKind};
use crate::modules::{self, ModuleId, ModuleMode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    Sandbox,
    Career,
}

/// Capability scopes a plugin can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Instrument readings, alarms, module states (what the operator sees).
    TelemetryRead,
    /// True physics state (sandbox only).
    TruthRead,
    /// Drive actuators / change automation.
    ControlWrite,
    /// Time scale and pause.
    SimControl,
    /// Fault injection, training mode (sandbox only).
    ScenarioWrite,
    /// Finances, market, inventory (career).
    EconomyRead,
    /// Contribute UI panels to the control room.
    UiPanel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The human at the control room UI / CLI.
    Operator,
    /// A third-party plugin with granted scopes.
    Plugin { id: String, scopes: BTreeSet<Scope> },
}

/// Plant state the policy needs to decide.
#[derive(Debug, Clone)]
pub struct PolicyContext {
    pub mode: GameMode,
    /// Modules installed on this unit (career). Ignored in sandbox.
    pub owned: BTreeSet<ModuleId>,
    pub module_modes: BTreeMap<ModuleId, ModuleMode>,
}

impl PolicyContext {
    pub fn sandbox() -> Self {
        Self {
            mode: GameMode::Sandbox,
            owned: BTreeSet::new(),
            module_modes: BTreeMap::new(),
        }
    }

    pub fn owns(&self, m: ModuleId) -> bool {
        self.mode == GameMode::Sandbox || self.owned.contains(&m)
    }

    fn mode_of(&self, m: ModuleId) -> ModuleMode {
        self.module_modes.get(&m).copied().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Denied(pub String);

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn deny<T>(msg: impl Into<String>) -> Result<T, Denied> {
    Err(Denied(msg.into()))
}

/// May `origin` read data covered by `scope` in `mode`?
pub fn can_read(origin: &Origin, scope: Scope, mode: GameMode) -> bool {
    let sandbox_only = matches!(scope, Scope::TruthRead | Scope::ScenarioWrite);
    if sandbox_only && mode == GameMode::Career {
        return false;
    }
    match origin {
        Origin::Operator => true,
        Origin::Plugin { scopes, .. } => scopes.contains(&scope),
    }
}

/// Decide whether `cmd` from `origin` is allowed. Plant-level checks (e.g.
/// "actuator is in AUTO") happen afterwards in the plant itself.
pub fn authorize(cmd: &Command, origin: &Origin, ctx: &PolicyContext) -> Result<(), Denied> {
    let kind = cmd.kind();
    let career = ctx.mode == GameMode::Career;

    // Rules shared by everyone.
    match kind {
        CommandKind::Scenario if career => return deny("scenario commands are sandbox-only"),
        CommandKind::ModuleMode(m, mode) => {
            if !ctx.owns(m) && mode != ModuleMode::Manual {
                return deny(format!("module {m:?} is not installed on this unit"));
            }
            if mode != ModuleMode::Manual && !modules::info(m).implemented {
                return deny(format!("module {m:?} is not available yet"));
            }
            if mode == ModuleMode::External && !ctx.owns(ModuleId::ExternalGateway) {
                return deny("External mode requires the External Control Gateway");
            }
            if mode == ModuleMode::Auto {
                for p in modules::info(m).prerequisites {
                    if ctx.mode_of(*p) != ModuleMode::Auto {
                        return deny(format!("{m:?} requires {p:?} in AUTO"));
                    }
                }
            }
        }
        CommandKind::Setpoint(m) if !ctx.owns(m) => {
            return deny(format!("module {m:?} is not installed on this unit"))
        }
        _ => {}
    }

    let scopes = match origin {
        Origin::Operator => return Ok(()),
        Origin::Plugin { scopes, .. } => scopes,
    };
    let need = |s: Scope| -> Result<(), Denied> {
        if scopes.contains(&s) {
            Ok(())
        } else {
            deny(format!("plugin lacks scope {s:?}"))
        }
    };

    match kind {
        CommandKind::Sim => need(Scope::SimControl),
        CommandKind::Scenario => need(Scope::ScenarioWrite),
        // Tripping the reactor is always a safe action.
        CommandKind::Scram => need(Scope::ControlWrite),
        CommandKind::ResetTrip => {
            need(Scope::ControlWrite)?;
            if career {
                return deny("only the operator can reset a reactor trip in career mode");
            }
            Ok(())
        }
        CommandKind::ModuleMode(..) => {
            need(Scope::ControlWrite)?;
            if career {
                return deny("only the operator can change automation modes in career mode");
            }
            Ok(())
        }
        CommandKind::Setpoint(m) => {
            need(Scope::ControlWrite)?;
            if career && ctx.mode_of(m) == ModuleMode::Manual {
                return deny(format!("module {m:?} is in MANUAL"));
            }
            Ok(())
        }
        CommandKind::Manual(actuator) => {
            need(Scope::ControlWrite)?;
            if !career {
                return Ok(());
            }
            let Some(owner) = modules::owner_of(actuator) else {
                return deny(format!(
                    "{actuator:?} has no automation module; operator only"
                ));
            };
            if !ctx.owns(owner) {
                return deny(format!("{actuator:?} requires owning {owner:?}"));
            }
            if ctx.mode_of(owner) != ModuleMode::External {
                return deny(format!("{owner:?} is not in EXTERNAL mode"));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;

    fn plugin(scopes: &[Scope]) -> Origin {
        Origin::Plugin {
            id: "test".into(),
            scopes: scopes.iter().copied().collect(),
        }
    }

    fn career(owned: &[ModuleId], modes: &[(ModuleId, ModuleMode)]) -> PolicyContext {
        PolicyContext {
            mode: GameMode::Career,
            owned: owned.iter().copied().collect(),
            module_modes: modes.iter().copied().collect(),
        }
    }

    fn rods() -> Command {
        Command::MoveRodBank {
            bank: "D".into(),
            target: 100.0,
        }
    }

    #[test]
    fn sandbox_plugins_can_do_everything_they_have_scopes_for() {
        let ctx = PolicyContext::sandbox();
        let p = plugin(&[Scope::ControlWrite, Scope::ScenarioWrite]);
        assert!(authorize(&rods(), &p, &ctx).is_ok());
        assert!(authorize(&Command::SetTrainingMode { enabled: true }, &p, &ctx).is_ok());
        assert!(authorize(&rods(), &plugin(&[Scope::TelemetryRead]), &ctx).is_err());
        assert!(!can_read(&p, Scope::TruthRead, GameMode::Sandbox));
        assert!(can_read(
            &plugin(&[Scope::TruthRead]),
            Scope::TruthRead,
            GameMode::Sandbox
        ));
    }

    #[test]
    fn career_plugin_cannot_automate_unowned_subsystems() {
        let p = plugin(&[Scope::ControlWrite]);
        // Nothing owned.
        let ctx = career(&[], &[]);
        assert!(authorize(&rods(), &p, &ctx).is_err());
        // Owned but in AUTO / MANUAL: still denied.
        let ctx = career(
            &[ModuleId::RodControl],
            &[(ModuleId::RodControl, ModuleMode::Auto)],
        );
        assert!(authorize(&rods(), &p, &ctx).is_err());
        // Owned and EXTERNAL: allowed.
        let ctx = career(
            &[ModuleId::RodControl, ModuleId::ExternalGateway],
            &[(ModuleId::RodControl, ModuleMode::External)],
        );
        assert!(authorize(&rods(), &p, &ctx).is_ok());
        // ...but not for a different, unowned subsystem.
        assert!(authorize(&Command::SetFeedwater { fraction: 0.5 }, &p, &ctx).is_err());
        // Operator is never blocked from manual control.
        assert!(authorize(
            &Command::SetFeedwater { fraction: 0.5 },
            &Origin::Operator,
            &ctx
        )
        .is_ok());
    }

    #[test]
    fn career_blocks_truth_and_scenarios() {
        let p = plugin(&[Scope::TruthRead, Scope::ScenarioWrite]);
        let ctx = career(&[], &[]);
        assert!(!can_read(&p, Scope::TruthRead, GameMode::Career));
        let cmd = Command::SetTrainingMode { enabled: true };
        assert!(authorize(&cmd, &p, &ctx).is_err());
        assert!(authorize(&cmd, &Origin::Operator, &ctx).is_err());
    }

    #[test]
    fn module_modes_require_ownership_and_gateway() {
        let ctx = career(&[ModuleId::RodControl], &[]);
        let auto = Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::Auto,
        };
        let ext = Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::External,
        };
        let fw = Command::SetModuleMode {
            module: ModuleId::FeedwaterLevel,
            mode: ModuleMode::Auto,
        };
        assert!(authorize(&auto, &Origin::Operator, &ctx).is_ok());
        assert!(authorize(&ext, &Origin::Operator, &ctx).is_err());
        assert!(authorize(&fw, &Origin::Operator, &ctx).is_err());
        assert!(authorize(&auto, &plugin(&[Scope::ControlWrite]), &ctx).is_err());
    }

    #[test]
    fn plant_master_needs_its_prerequisites_in_auto() {
        let ctx = PolicyContext::sandbox();
        let pm = Command::SetModuleMode {
            module: ModuleId::PlantMaster,
            mode: ModuleMode::Auto,
        };
        assert!(authorize(&pm, &Origin::Operator, &ctx).is_err());
    }
}
