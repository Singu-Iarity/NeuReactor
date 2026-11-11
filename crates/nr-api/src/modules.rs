//! Automation module catalog.
//!
//! Automation is not a global "level": every panel/subsystem has its own
//! controller module. In sandbox mode all modules are available; in career
//! mode each must be bought and installed per unit. Each module owns a set of
//! actuators; while it is in [`ModuleMode::Auto`] those actuators ignore
//! manual commands.

use crate::commands::Actuator;
use crate::upgrades::UpgradeId;
use serde::{Deserialize, Serialize};
use UpgradeId as U;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleId {
    RodControl,
    BoronControl,
    PressurizerPressure,
    PressurizerLevel,
    RcpSequencer,
    FeedwaterLevel,
    Deaerator,
    CondenserVacuum,
    SteamDump,
    TurbineGovernor,
    GeneratorSync,
    Excitation,
    DieselSequencer,
    AuxFeedAutoStart,
    PlantMaster,
    ReactorPower,
    ExternalGateway,
}

/// Operating mode of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModuleMode {
    /// Module bypassed: the operator drives the actuators by hand.
    #[default]
    Manual,
    /// Built-in controller drives the actuators.
    Auto,
    /// A connected plugin drives the actuators (requires the External Gateway).
    External,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleInfo {
    pub id: ModuleId,
    pub name: &'static str,
    pub panel: &'static str,
    pub description: &'static str,
    /// Career-mode purchase price per unit (USD).
    pub price: u64,
    pub prerequisites: &'static [ModuleId],
    /// Hardware that must be installed before this module can run: the
    /// computer it runs on and the panel package of the panel it controls.
    pub requires_upgrades: &'static [UpgradeId],
    pub actuators: &'static [Actuator],
    /// False while the underlying plant system is not simulated yet.
    pub implemented: bool,
}

use Actuator as A;
use ModuleId as M;

pub const CATALOG: &[ModuleInfo] = &[
    ModuleInfo {
        id: M::RodControl,
        name: "Automatic Rod Control",
        panel: "Reactor",
        description: "Drives control banks with overlap to hold average coolant temperature on the Tavg program.",
        price: 4_500_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::SolidStateRodControl],
        actuators: &[A::ControlRods],
        implemented: true,
    },
    ModuleInfo {
        id: M::BoronControl,
        name: "Automatic Boron/Makeup Control",
        panel: "CVCS",
        description: "Borates or dilutes slowly to keep the lead control bank inside its target band (xenon and burnup compensation).",
        price: 2_000_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalCvcs],
        actuators: &[A::Boration],
        implemented: true,
    },
    ModuleInfo {
        id: M::PressurizerPressure,
        name: "Pressurizer Pressure Control",
        panel: "RCS",
        description: "Proportional and backup heaters plus spray valves hold RCS pressure at setpoint.",
        price: 1_500_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalRcsControls],
        actuators: &[A::PressurizerHeaters, A::PressurizerSpray],
        implemented: true,
    },
    ModuleInfo {
        id: M::PressurizerLevel,
        name: "Pressurizer Level Control",
        panel: "CVCS",
        description: "Modulates charging flow to follow the power-programmed pressurizer level.",
        price: 1_200_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalCvcs],
        actuators: &[A::Charging],
        implemented: true,
    },
    ModuleInfo {
        id: M::RcpSequencer,
        name: "RCP Start Sequencer",
        panel: "RCS",
        description: "Starts and stops reactor coolant pumps in the correct order with interlock checks.",
        price: 800_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalRcsControls],
        actuators: &[A::CoolantPumps],
        implemented: false,
    },
    ModuleInfo {
        id: M::FeedwaterLevel,
        name: "SG Feedwater Level Control",
        panel: "Steam & Feed",
        description: "Three-element (level, steam flow, feed flow) control of steam generator level.",
        price: 2_500_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalFeedwater],
        actuators: &[A::Feedwater],
        implemented: true,
    },
    ModuleInfo {
        id: M::Deaerator,
        name: "Deaerator Control",
        panel: "Steam & Feed",
        description: "Opens pegging steam from main steam when extraction steam cannot keep the deaerator hot enough to strip oxygen (low load).",
        price: 900_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalCondenserControls],
        actuators: &[A::Deaerator],
        implemented: true,
    },
    ModuleInfo {
        id: M::CondenserVacuum,
        name: "Condenser Vacuum & Hotwell Control",
        panel: "Steam & Feed",
        description: "Runs circulating water pumps, hogging vacuum pumps for pulldown, then hands over to the steam jet air ejectors.",
        price: 1_000_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalCondenserControls],
        actuators: &[A::Condenser],
        implemented: true,
    },
    ModuleInfo {
        id: M::SteamDump,
        name: "Steam Dump Control",
        panel: "Steam & Feed",
        description: "Load-rejection and plant-trip steam dump to the condenser; holds no-load Tavg after a trip.",
        price: 1_300_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalFeedwater],
        actuators: &[A::SteamDump],
        implemented: true,
    },
    ModuleInfo {
        id: M::TurbineGovernor,
        name: "Turbine Load Governor",
        panel: "Turbine-Generator",
        description: "Ramps governor valves to a target electrical load at a set rate.",
        price: 3_000_000,
        prerequisites: &[],
        requires_upgrades: &[U::TurbineControlComputer, U::TurbinePanelInterface],
        actuators: &[A::TurbineValves],
        implemented: true,
    },
    ModuleInfo {
        id: M::GeneratorSync,
        name: "Sync-Check Relay & Auto-Synchronizer",
        panel: "Turbine-Generator",
        description: "In AUTO: blocks out-of-phase breaker closure (sync-check) and automatically matches speed and closes the generator breaker.",
        price: 700_000,
        prerequisites: &[],
        requires_upgrades: &[U::TurbineControlComputer, U::GeneratorSyncInterface],
        actuators: &[A::GeneratorBreaker],
        implemented: true,
    },
    ModuleInfo {
        id: M::Excitation,
        name: "Automatic Voltage Regulator",
        panel: "Turbine-Generator",
        description: "Holds generator terminal voltage by trimming excitation.",
        price: 600_000,
        prerequisites: &[],
        requires_upgrades: &[U::TurbineControlComputer, U::DigitalExciter],
        actuators: &[A::Exciter],
        implemented: true,
    },
    ModuleInfo {
        id: M::DieselSequencer,
        name: "Diesel Generator Load Sequencer",
        panel: "Electrical",
        description: "Auto-starts emergency diesels on loss of offsite power and sequences safety loads.",
        price: 1_800_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalElectricalControls],
        actuators: &[A::Diesels],
        implemented: true,
    },
    ModuleInfo {
        id: M::AuxFeedAutoStart,
        name: "Auxiliary Feedwater Auto-Start",
        panel: "Steam & Feed",
        description: "Starts auxiliary feed pumps on low-low SG level or loss of main feed.",
        price: 1_100_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer, U::DigitalFeedwater],
        actuators: &[A::AuxFeedwater],
        implemented: false,
    },
    ModuleInfo {
        id: M::PlantMaster,
        name: "Plant Master / Unit Load Controller",
        panel: "Supervisory",
        description: "Unit load controller: sets turbine load demand, coordinates the subordinate loops and holds load on large deviations. With the Grid Dispatch Link it follows grid demand automatically (AGC).",
        price: 9_000_000,
        prerequisites: &[
            M::RodControl,
            M::PressurizerPressure,
            M::PressurizerLevel,
            M::FeedwaterLevel,
            M::SteamDump,
            M::TurbineGovernor,
        ],
        requires_upgrades: &[U::PlantProcessComputer, U::TurbineControlComputer],
        actuators: &[],
        implemented: true,
    },
    ModuleInfo {
        id: M::ReactorPower,
        name: "Reactor Power Controller",
        panel: "Reactor",
        description: "Holds reactor power at a setpoint. ROD mode moves the control banks (startup-rate limited); BORON mode dilutes/borates with rods parked; COORDINATED uses rods for fast changes and boron to return rods to their band. Analog cabinets support ROD mode only.",
        price: 5_500_000,
        prerequisites: &[M::RodControl],
        requires_upgrades: &[U::PlantProcessComputer, U::SolidStateRodControl],
        actuators: &[],
        implemented: true,
    },
    ModuleInfo {
        id: M::ExternalGateway,
        name: "External Control Gateway (DCS link)",
        panel: "Supervisory",
        description: "Lets an approved plugin drive actuators of modules you own, in place of the built-in controller.",
        price: 2_500_000,
        prerequisites: &[],
        requires_upgrades: &[U::PlantProcessComputer],
        actuators: &[],
        implemented: true,
    },
];

pub fn info(id: ModuleId) -> &'static ModuleInfo {
    CATALOG
        .iter()
        .find(|m| m.id == id)
        .expect("every module id is in the catalog")
}

/// The module that owns `actuator`, if any.
pub fn owner_of(actuator: Actuator) -> Option<ModuleId> {
    CATALOG
        .iter()
        .find(|m| m.actuators.contains(&actuator))
        .map(|m| m.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_actuator_has_at_most_one_owner() {
        for a in Actuator::ALL {
            let owners = CATALOG.iter().filter(|m| m.actuators.contains(a)).count();
            assert!(owners <= 1, "{a:?} has {owners} owners");
        }
    }

    #[test]
    fn prerequisites_exist() {
        for m in CATALOG {
            for p in m.prerequisites {
                let _ = info(*p);
            }
        }
    }
}
