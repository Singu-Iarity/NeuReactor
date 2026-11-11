//! Operator / plugin commands.

use crate::modules::{ModuleId, ModuleMode};
use crate::upgrades::UpgradeId;
use serde::{Deserialize, Serialize};

/// A plant actuator group. Each is owned by at most one automation module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actuator {
    ControlRods,
    Boration,
    PressurizerHeaters,
    PressurizerSpray,
    Charging,
    Letdown,
    CoolantPumps,
    Feedwater,
    Deaerator,
    Condenser,
    SteamDump,
    TurbineValves,
    GeneratorBreaker,
    FieldBreaker,
    Exciter,
    NisBlocks,
    MainSteamIsolation,
    Diesels,
    AuxFeedwater,
}

impl Actuator {
    pub const ALL: &'static [Actuator] = &[
        Actuator::ControlRods,
        Actuator::Boration,
        Actuator::PressurizerHeaters,
        Actuator::PressurizerSpray,
        Actuator::Charging,
        Actuator::Letdown,
        Actuator::CoolantPumps,
        Actuator::Feedwater,
        Actuator::Deaerator,
        Actuator::Condenser,
        Actuator::SteamDump,
        Actuator::TurbineValves,
        Actuator::GeneratorBreaker,
        Actuator::FieldBreaker,
        Actuator::Exciter,
        Actuator::NisBlocks,
        Actuator::MainSteamIsolation,
        Actuator::Diesels,
        Actuator::AuxFeedwater,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BorationMode {
    #[default]
    Off,
    Borate,
    Dilute,
}

/// Rod control IN-HOLD-OUT lever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RodDirection {
    In,
    #[default]
    Hold,
    Out,
}

/// Nuclear instrumentation trip blocks the operator can apply above the
/// relevant permissive (P-6 for source range, P-10 for the others).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NisBlock {
    SourceRange,
    IntermediateRange,
    PowerRangeLow,
}

/// Control room technology generation of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ControlRoomGeneration {
    #[default]
    Analog,
    Digital,
}

/// Every action that can be taken on a plant. JSON form is
/// `{"type": "move_rod_bank", "bank": "D", "target": 180}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    // --- manual actuator commands ---
    /// Drive one bank to a target position (plugin-friendly).
    MoveRodBank {
        bank: String,
        target: f64,
    },
    /// Rod control lever. `selection` is `"manual"` (control banks in
    /// overlap) or a single bank name (`"SA"`, `"D"`...).
    RodMotion {
        selection: String,
        direction: RodDirection,
    },
    /// Manual rod drive speed (steps/min).
    SetRodSpeed {
        steps_per_min: f64,
    },
    SetBoration {
        mode: BorationMode,
    },
    SetPressurizerHeaters {
        fraction: f64,
    },
    SetPressurizerSpray {
        fraction: f64,
    },
    SetCharging {
        kg_s: f64,
    },
    SetLetdown {
        kg_s: f64,
    },
    SetCoolantPump {
        index: usize,
        running: bool,
    },
    SetFeedwater {
        fraction: f64,
    },
    SetSteamDump {
        fraction: f64,
    },
    SetTurbineValve {
        fraction: f64,
    },
    SetGeneratorBreaker {
        closed: bool,
    },
    SetFieldBreaker {
        closed: bool,
    },
    SetExcitation {
        fraction: f64,
    },
    SetCwPump {
        index: usize,
        running: bool,
    },
    SetVacuumPump {
        index: usize,
        running: bool,
    },
    SetSjae {
        on: bool,
    },
    SetPeggingSteam {
        open: bool,
    },
    SetMsiv {
        open: bool,
    },
    BlockNis {
        channel: NisBlock,
        blocked: bool,
    },
    // --- safety ---
    Scram,
    ResetTrip,
    TripTurbine,
    ResetTurbine,
    // --- automation ---
    SetModuleMode {
        module: ModuleId,
        mode: ModuleMode,
    },
    SetSetpoint {
        module: ModuleId,
        key: String,
        value: f64,
    },
    // --- simulation control ---
    SetTimeScale {
        factor: f64,
    },
    SetPaused {
        paused: bool,
    },
    // --- scenario / sandbox only ---
    InjectSensorFault {
        tag: String,
        fault: String,
    },
    SetTrainingMode {
        enabled: bool,
    },
    RepairGenerator,
    /// Install / remove an automation module (sandbox; career buys them).
    SetModuleInstalled {
        module: ModuleId,
        installed: bool,
    },
    /// Fit a physical component variant (sandbox; career buys them).
    SetComponent {
        slot: crate::equipment::Slot,
        era: crate::equipment::Era,
        tier: crate::equipment::Tier,
    },
    /// Choose how an automation module is implemented (sandbox).
    SetModuleController {
        module: ModuleId,
        controller: crate::equipment::Controller,
    },
    /// Install / remove an I&C hardware upgrade (sandbox; career buys them).
    SetUpgradeInstalled {
        upgrade: UpgradeId,
        installed: bool,
    },
    SetControlRoom {
        generation: ControlRoomGeneration,
    },
    /// `"hot_standby"` or `"full_power"`.
    LoadScenario {
        name: String,
    },
}

/// Coarse classification used by the access policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Manual(Actuator),
    /// Reactor or turbine trip: always a safe action.
    Trip,
    /// Reset of a reactor or turbine trip.
    Reset,
    ModuleMode(ModuleId, ModuleMode),
    Setpoint(ModuleId),
    Sim,
    Scenario,
}

impl Command {
    pub fn kind(&self) -> CommandKind {
        use Actuator as A;
        use Command as C;
        match self {
            C::MoveRodBank { .. } | C::RodMotion { .. } | C::SetRodSpeed { .. } => {
                CommandKind::Manual(A::ControlRods)
            }
            C::SetBoration { .. } => CommandKind::Manual(A::Boration),
            C::SetPressurizerHeaters { .. } => CommandKind::Manual(A::PressurizerHeaters),
            C::SetPressurizerSpray { .. } => CommandKind::Manual(A::PressurizerSpray),
            C::SetCharging { .. } => CommandKind::Manual(A::Charging),
            C::SetLetdown { .. } => CommandKind::Manual(A::Letdown),
            C::SetCoolantPump { .. } => CommandKind::Manual(A::CoolantPumps),
            C::SetFeedwater { .. } => CommandKind::Manual(A::Feedwater),
            C::SetSteamDump { .. } => CommandKind::Manual(A::SteamDump),
            C::SetTurbineValve { .. } => CommandKind::Manual(A::TurbineValves),
            C::SetGeneratorBreaker { .. } => CommandKind::Manual(A::GeneratorBreaker),
            C::SetFieldBreaker { .. } => CommandKind::Manual(A::FieldBreaker),
            C::SetExcitation { .. } => CommandKind::Manual(A::Exciter),
            C::BlockNis { .. } => CommandKind::Manual(A::NisBlocks),
            C::SetCwPump { .. } | C::SetVacuumPump { .. } | C::SetSjae { .. } => {
                CommandKind::Manual(A::Condenser)
            }
            C::SetPeggingSteam { .. } => CommandKind::Manual(A::Deaerator),
            C::SetMsiv { .. } => CommandKind::Manual(A::MainSteamIsolation),
            C::Scram | C::TripTurbine => CommandKind::Trip,
            C::ResetTrip | C::ResetTurbine => CommandKind::Reset,
            C::SetModuleMode { module, mode } => CommandKind::ModuleMode(*module, *mode),
            C::SetSetpoint { module, .. } => CommandKind::Setpoint(*module),
            C::SetTimeScale { .. } | C::SetPaused { .. } => CommandKind::Sim,
            C::InjectSensorFault { .. }
            | C::SetTrainingMode { .. }
            | C::RepairGenerator
            | C::SetModuleInstalled { .. }
            | C::SetComponent { .. }
            | C::SetModuleController { .. }
            | C::SetUpgradeInstalled { .. }
            | C::SetControlRoom { .. }
            | C::LoadScenario { .. } => CommandKind::Scenario,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let c = Command::MoveRodBank {
            bank: "D".into(),
            target: 180.0,
        };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#"{"type":"move_rod_bank","bank":"D","target":180.0}"#);
        let back: Command = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
        let m: Command = serde_json::from_str(
            r#"{"type":"set_module_mode","module":"rod_control","mode":"auto"}"#,
        )
        .unwrap();
        assert_eq!(
            m.kind(),
            CommandKind::ModuleMode(ModuleId::RodControl, ModuleMode::Auto)
        );
        let r: Command =
            serde_json::from_str(r#"{"type":"rod_motion","selection":"manual","direction":"out"}"#)
                .unwrap();
        assert_eq!(r.kind(), CommandKind::Manual(Actuator::ControlRods));
        let b: Command =
            serde_json::from_str(r#"{"type":"block_nis","channel":"source_range","blocked":true}"#)
                .unwrap();
        assert_eq!(b.kind(), CommandKind::Manual(Actuator::NisBlocks));
    }
}
