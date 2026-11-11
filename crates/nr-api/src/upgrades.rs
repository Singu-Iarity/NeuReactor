//! Instrumentation & control hardware upgrades.
//!
//! Two kinds of hardware are installed per unit:
//!
//! * **Computers** (plant process computer, turbine control computer) run
//!   automation but cannot see or drive a panel on their own.
//! * **Panel packages** modernise one area of the control board (new
//!   controls and displays) *and* wire it to its computer.
//!
//! An automation module needs both its computer and the panel package of the
//! panel it acts on; until then the computer cannot access that panel and the
//! panel keeps its original analog equipment. A unit can therefore run a
//! modern turbine/generator section while the reactor panels stay analog.
//!
//! Section ids are `"<tab>.<section>"` and match `data-section` attributes in
//! the web UI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeId {
    DigitalNis,
    SolidStateRodControl,
    DigitalAnnunciators,
    DigitalRcsControls,
    DigitalCvcs,
    DigitalFeedwater,
    TurbineControlComputer,
    TurbinePanelInterface,
    GeneratorSyncInterface,
    DigitalExciter,
    PlantProcessComputer,
    RodGroupSelect,
    DigitalCondenserControls,
    GridDispatchLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeKind {
    Computer,
    PanelPackage,
    /// Extra control board hardware (no computer, no change of look).
    BoardHardware,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeInfo {
    pub id: UpgradeId,
    pub kind: UpgradeKind,
    /// For panel packages: the computer this panel is wired to.
    pub computer: Option<UpgradeId>,
    pub name: &'static str,
    pub description: &'static str,
    /// Career-mode purchase price per unit (USD).
    pub price: u64,
    /// Control board sections this upgrade modernises.
    pub sections: &'static [&'static str],
}

use UpgradeId as U;

pub const CATALOG: &[UpgradeInfo] = &[
    UpgradeInfo {
        id: U::DigitalNis,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::PlantProcessComputer),
        name: "Digital Nuclear Instrumentation",
        description: "Replaces analog source, intermediate and power range drawers with digital channels and displays.",
        price: 6_000_000,
        sections: &["reactor.nis"],
    },
    UpgradeInfo {
        id: U::SolidStateRodControl,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::PlantProcessComputer),
        name: "Solid-State Rod Control & Position Indication",
        description: "Digital rod position indication and modern rod control station.",
        price: 4_000_000,
        sections: &["reactor.rods"],
    },
    UpgradeInfo {
        id: U::DigitalAnnunciators,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::PlantProcessComputer),
        name: "Digital Alarm System",
        description: "Replaces lightbox annunciators and the first-out panel with a computerised alarm system.",
        price: 2_500_000,
        sections: &[
            "reactor.alarms",
            "reactor.firstout",
            "rcs.alarms",
            "cvcs.alarms",
            "steam.alarms",
            "turbine.alarms",
            "computer.alarms",
        ],
    },
    UpgradeInfo {
        id: U::DigitalRcsControls,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::PlantProcessComputer),
        name: "Digital RCS Control Stations",
        description: "Modern indication and controller faceplates for the primary system and pressurizer.",
        price: 3_500_000,
        sections: &["rcs.main", "rcs.pzr", "rcs.rcp"],
    },
    UpgradeInfo {
        id: U::DigitalCvcs,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::PlantProcessComputer),
        name: "Digital CVCS Controls",
        description: "Modern boron, charging and letdown control stations.",
        price: 1_500_000,
        sections: &["cvcs.boron", "cvcs.charging"],
    },
    UpgradeInfo {
        id: U::DigitalFeedwater,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::PlantProcessComputer),
        name: "Digital Feedwater & Steam Dump Controls",
        description: "Modern steam generator, feedwater and steam dump indication and controllers.",
        price: 3_000_000,
        sections: &["steam.sg", "steam.feed", "steam.dump"],
    },
    UpgradeInfo {
        id: U::TurbineControlComputer,
        kind: UpgradeKind::Computer,
        computer: None,
        name: "Turbine Control Computer (DEH)",
        description: "Digital electro-hydraulic control computer for the turbine-generator. Needs the turbine-generator panel packages to reach the board.",
        price: 7_000_000,
        sections: &[],
    },
    UpgradeInfo {
        id: U::TurbinePanelInterface,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::TurbineControlComputer),
        name: "Turbine Control Panel Package",
        description: "Modern turbine control and acceleration panels wired to the turbine control computer.",
        price: 2_500_000,
        sections: &["turbine.control", "turbine.accel"],
    },
    UpgradeInfo {
        id: U::GeneratorSyncInterface,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::TurbineControlComputer),
        name: "Generator Sync Interface",
        description: "Digital synchroscope and breaker control. With the turbine control computer it adds a permanent sync-check: out-of-phase closes are always blocked.",
        price: 1_200_000,
        sections: &["turbine.sync"],
    },
    UpgradeInfo {
        id: U::DigitalExciter,
        kind: UpgradeKind::PanelPackage,
        computer: Some(U::TurbineControlComputer),
        name: "Digital Excitation System",
        description: "Static exciter with digital voltage regulator panel.",
        price: 2_000_000,
        sections: &["turbine.excitation"],
    },
    UpgradeInfo {
        id: U::PlantProcessComputer,
        kind: UpgradeKind::Computer,
        computer: None,
        name: "Plant Process Computer",
        description: "Central plant computer: trends, event logging and the automation for reactor, RCS, CVCS and steam plant panels (each panel also needs its package).",
        price: 5_000_000,
        sections: &["computer.recorders", "computer.log", "computer.instructor"],
    },
    ROD_GROUP_SELECT,
    DIGITAL_CONDENSER,
    GRID_DISPATCH_LINK,
];

/// Board hardware entries appended to the catalog.
pub const ROD_GROUP_SELECT: UpgradeInfo = UpgradeInfo {
    id: U::RodGroupSelect,
    kind: UpgradeKind::BoardHardware,
    computer: None,
    name: "Rod Group Select Pushbutton Grid",
    description: "Lit pushbutton matrix on the rod control panel: select a single rod group (e.g. D2) for the IN/HOLD/OUT lever to realign groups by hand.",
    price: 600_000,
    sections: &[],
};

pub const DIGITAL_CONDENSER: UpgradeInfo = UpgradeInfo {
    id: U::DigitalCondenserControls,
    kind: UpgradeKind::PanelPackage,
    computer: Some(U::PlantProcessComputer),
    name: "Digital Condenser & Feed Train Controls",
    description:
        "Modern condenser, air removal and deaerator panels wired to the plant process computer.",
    price: 1_800_000,
    sections: &["steam.condenser", "steam.deaerator"],
};

pub const GRID_DISPATCH_LINK: UpgradeInfo = UpgradeInfo {
    id: U::GridDispatchLink,
    kind: UpgradeKind::PanelPackage,
    computer: Some(U::PlantProcessComputer),
    name: "Grid Dispatch Link (AGC)",
    description: "Data link from the grid operator to the plant computer: modern grid station and automatic generation control (the unit load controller follows dispatch).",
    price: 3_500_000,
    sections: &["grid.station"],
};

pub fn info(id: UpgradeId) -> &'static UpgradeInfo {
    CATALOG
        .iter()
        .find(|u| u.id == id)
        .expect("every upgrade id is in the catalog")
}

/// Which upgrade (if any) covers a board section.
pub fn covering(section: &str) -> Option<UpgradeId> {
    CATALOG
        .iter()
        .find(|u| u.sections.contains(&section))
        .map(|u| u.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_are_covered_once() {
        let mut seen = std::collections::BTreeSet::new();
        for u in CATALOG {
            for s in u.sections {
                assert!(seen.insert(*s), "section {s} covered twice");
            }
        }
        assert_eq!(
            covering("turbine.sync"),
            Some(UpgradeId::GeneratorSyncInterface)
        );
        assert_eq!(covering("nope"), None);
        for u in CATALOG {
            match u.kind {
                UpgradeKind::PanelPackage => {
                    assert_eq!(info(u.computer.unwrap()).kind, UpgradeKind::Computer)
                }
                UpgradeKind::Computer | UpgradeKind::BoardHardware => assert!(u.computer.is_none()),
            }
        }
    }
}
