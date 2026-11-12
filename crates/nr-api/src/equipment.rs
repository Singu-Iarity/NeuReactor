//! Plant equipment: physical components with era and quality tiers,
//! analog/digital automation controller variants, and the compatibility
//! rules that require conversion kits between equipment of different eras.

use crate::modules::ModuleId;
use crate::upgrades::{self, UpgradeId};
use serde::{Deserialize, Serialize};

/// Technology generation. Equipment two eras apart needs a conversion kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Era {
    Vintage,
    Dated,
    Modern,
}

impl Era {
    pub const ALL: [Era; 3] = [Era::Vintage, Era::Dated, Era::Modern];
    /// First year equipment of this era exists (before slot offsets).
    pub fn year(self) -> i32 {
        match self {
            Era::Vintage => 1965,
            Era::Dated => 1980,
            Era::Modern => 1995,
        }
    }
    pub fn of_year(year: i32) -> Era {
        if year >= 1995 {
            Era::Modern
        } else if year >= 1980 {
            Era::Dated
        } else {
            Era::Vintage
        }
    }
    pub fn index(self) -> i32 {
        self as i32
    }
    pub fn gap(a: Era, b: Era) -> i32 {
        (a.index() - b.index()).abs()
    }
    fn price_mult(self) -> f64 {
        [1.0, 1.35, 1.8][self as usize]
    }
    fn perf(self) -> f64 {
        [0.96, 1.0, 1.03][self as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Basic,
    Quality,
    Premium,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Basic, Tier::Quality, Tier::Premium];
    fn price_mult(self) -> f64 {
        [0.7, 1.0, 1.45][self as usize]
    }
    fn perf(self) -> f64 {
        [0.98, 1.0, 1.02][self as usize]
    }
}

/// Physical component slots of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Turbine,
    Generator,
    Condenser,
    FeedPumps,
    CoolantPumps,
    SteamGenerators,
    RodDrives,
    Pressurizer,
    BoardInstruments,
    CirculatingWater,
    AirRemoval,
    Deaerator,
    SteamIsolation,
    StationTransformers,
    EmergencyDiesels,
}

pub struct SlotInfo {
    pub slot: Slot,
    pub name: &'static str,
    /// Price of a dated / quality unit (USD).
    pub base_price: f64,
    /// Years after the era start this slot's new generation appears.
    pub year_offset: i32,
    pub effect: &'static str,
}

pub const SLOTS: &[SlotInfo] = &[
    SlotInfo {
        slot: Slot::StationTransformers,
        name: "Station Service Transformers (SUT/UAT)",
        base_price: 25e6,
        year_offset: 2,
        effect: "Transformer capacity before overload trips",
    },
    SlotInfo {
        slot: Slot::EmergencyDiesels,
        name: "Emergency Diesel Generators",
        base_price: 35e6,
        year_offset: 3,
        effect: "Diesel start time and fuel economy",
    },
    SlotInfo {
        slot: Slot::CirculatingWater,
        name: "Circulating Water Pumps",
        base_price: 30e6,
        year_offset: 1,
        effect: "Condenser cooling capacity (vacuum at load)",
    },
    SlotInfo {
        slot: Slot::AirRemoval,
        name: "Condenser Air Removal (Vacuum Pumps & SJAEs)",
        base_price: 12e6,
        year_offset: 2,
        effect: "Vacuum pulldown speed and attainable vacuum",
    },
    SlotInfo {
        slot: Slot::Deaerator,
        name: "Condensate Pumps & Deaerator",
        base_price: 22e6,
        year_offset: 3,
        effect: "Feedwater heating / deaeration",
    },
    SlotInfo {
        slot: Slot::SteamIsolation,
        name: "Main Steam Isolation Valves",
        base_price: 10e6,
        year_offset: 4,
        effect: "Steam line isolation (tightness)",
    },
    SlotInfo {
        slot: Slot::Turbine,
        name: "Main Turbine",
        base_price: 120e6,
        year_offset: 0,
        effect: "Steam-to-shaft efficiency (electrical output)",
    },
    SlotInfo {
        slot: Slot::Generator,
        name: "Main Generator",
        base_price: 80e6,
        year_offset: 1,
        effect: "Generator efficiency (electrical output)",
    },
    SlotInfo {
        slot: Slot::Condenser,
        name: "Condenser",
        base_price: 40e6,
        year_offset: 2,
        effect: "Condenser vacuum (cycle efficiency)",
    },
    SlotInfo {
        slot: Slot::FeedPumps,
        name: "Main Feedwater Pumps",
        base_price: 25e6,
        year_offset: 1,
        effect: "Maximum feedwater flow",
    },
    SlotInfo {
        slot: Slot::CoolantPumps,
        name: "Reactor Coolant Pumps",
        base_price: 60e6,
        year_offset: 3,
        effect: "RCS flow",
    },
    SlotInfo {
        slot: Slot::SteamGenerators,
        name: "Steam Generators",
        base_price: 150e6,
        year_offset: 4,
        effect: "Heat transfer (steam pressure at power)",
    },
    SlotInfo {
        slot: Slot::RodDrives,
        name: "Control Rod Drive Mechanisms",
        base_price: 30e6,
        year_offset: 2,
        effect: "Rod stepping speed",
    },
    SlotInfo {
        slot: Slot::Pressurizer,
        name: "Pressurizer Heaters & Sprays",
        base_price: 20e6,
        year_offset: 3,
        effect: "Heater capacity",
    },
    SlotInfo {
        slot: Slot::BoardInstruments,
        name: "Control Board Instruments",
        base_price: 15e6,
        year_offset: 0,
        effect: "Indication accuracy (instrument noise)",
    },
];

pub fn slot_info(s: Slot) -> &'static SlotInfo {
    SLOTS.iter().find(|i| i.slot == s).expect("slot in table")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub era: Era,
    pub tier: Tier,
}

impl Component {
    pub const STANDARD: Component = Component {
        era: Era::Dated,
        tier: Tier::Quality,
    };

    /// Performance factor relative to a dated/quality component.
    pub fn perf(self) -> f64 {
        self.era.perf() * self.tier.perf()
    }

    pub fn price(self, slot: Slot) -> f64 {
        slot_info(slot).base_price * self.era.price_mult() * self.tier.price_mult()
    }

    pub fn year(self, slot: Slot) -> i32 {
        if self.era == Era::Vintage {
            self.era.year()
        } else {
            self.era.year() + slot_info(slot).year_offset
        }
    }

    pub fn label(self) -> String {
        format!("{:?} {:?}", self.era, self.tier)
    }
}

/// How an automation module is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Controller {
    /// Stand-alone analog controller cabinet: no computer needed, less precise.
    Analog { era: Era, tier: Tier },
    /// Software on a plant computer: precise, needs computer + panel package.
    Digital,
}

impl Controller {
    /// Controller setpoint error, in units of the module's error scale.
    pub fn imprecision(self) -> f64 {
        match self {
            Controller::Analog { era, tier } => {
                let e = if era == Era::Vintage { 1.0 } else { 0.7 };
                e * [1.0, 0.7, 0.5][tier as usize]
            }
            Controller::Digital => 0.1,
        }
    }

    pub fn price(self, module: ModuleId) -> f64 {
        let base = crate::modules::info(module).price as f64;
        match self {
            Controller::Analog { era, tier } => base * 0.5 * era.price_mult() * tier.price_mult(),
            Controller::Digital => base,
        }
    }

    pub fn year(self) -> i32 {
        match self {
            Controller::Analog {
                era: Era::Vintage, ..
            } => 1965,
            Controller::Analog { .. } => 1978,
            Controller::Digital => 1980,
        }
    }

    /// Era of the equipment that physically drives the plant component.
    pub fn era(self, module: ModuleId) -> Era {
        match self {
            Controller::Analog { era, .. } => era,
            Controller::Digital => crate::modules::info(module)
                .requires_upgrades
                .iter()
                .map(|u| Era::of_year(upgrade_year(*u)))
                .max()
                .unwrap_or(Era::Dated),
        }
    }

    pub fn label(self) -> String {
        match self {
            Controller::Analog { era, tier } => format!("Analog {era:?} {tier:?}"),
            Controller::Digital => "Digital".into(),
        }
    }
}

/// Analog controllers only exist up to the dated era.
pub fn analog_eras() -> [Era; 2] {
    [Era::Vintage, Era::Dated]
}

/// First year an I&C hardware upgrade can be installed.
pub fn upgrade_year(u: UpgradeId) -> i32 {
    use UpgradeId as U;
    match u {
        U::PlantProcessComputer => 1980,
        U::RodGroupSelect => 1966,
        U::DigitalCondenserControls => 1993,
        U::GridDispatchLink => 1998,
        U::DigitalElectricalControls => 1991,
        U::MessageTerminal => 1982,
        U::MessagingHmi => 1997,
        U::CoreExitThermocoupleMap => 1981,
        U::TurbineControlComputer => 1982,
        U::SolidStateRodControl => 1984,
        U::GeneratorSyncInterface => 1985,
        U::TurbinePanelInterface => 1986,
        U::DigitalExciter => 1988,
        U::DigitalAnnunciators => 1990,
        U::DigitalRcsControls => 1994,
        U::DigitalCvcs => 1994,
        U::DigitalFeedwater => 1996,
        U::DigitalNis => 1997,
    }
}

/// The physical component an automation module acts on.
pub fn module_component(m: ModuleId) -> Option<Slot> {
    use ModuleId as M;
    match m {
        M::RodControl => Some(Slot::RodDrives),
        M::PressurizerPressure | M::PressurizerLevel => Some(Slot::Pressurizer),
        M::RcpSequencer => Some(Slot::CoolantPumps),
        M::FeedwaterLevel | M::AuxFeedAutoStart => Some(Slot::FeedPumps),
        M::CondenserVacuum => Some(Slot::AirRemoval),
        M::Deaerator => Some(Slot::Deaerator),
        M::SteamDump => Some(Slot::Condenser),
        M::ReactorPower => Some(Slot::RodDrives),
        M::DieselSequencer => Some(Slot::EmergencyDiesels),
        M::TurbineGovernor => Some(Slot::Turbine),
        M::GeneratorSync | M::Excitation => Some(Slot::Generator),
        _ => None,
    }
}

/// Mechanically / electrically coupled components.
pub const COUPLINGS: &[(Slot, Slot)] = &[
    (Slot::Turbine, Slot::Generator),
    (Slot::Turbine, Slot::Condenser),
    (Slot::Condenser, Slot::FeedPumps),
    (Slot::FeedPumps, Slot::SteamGenerators),
    (Slot::CoolantPumps, Slot::SteamGenerators),
    (Slot::Condenser, Slot::CirculatingWater),
    (Slot::Condenser, Slot::AirRemoval),
    (Slot::FeedPumps, Slot::Deaerator),
];

/// A conversion / interface kit needed because two connected pieces of
/// equipment are two or more eras apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversionKit {
    pub key: String,
    pub name: String,
    pub price: f64,
}

/// Complete equipment configuration of a unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Equipment {
    pub components: std::collections::BTreeMap<Slot, Component>,
    pub controllers: std::collections::BTreeMap<ModuleId, Controller>,
    pub kits: std::collections::BTreeSet<String>,
}

impl Equipment {
    /// Standard dated/quality components, every module as digital software.
    pub fn standard() -> Self {
        Self {
            components: SLOTS
                .iter()
                .map(|s| (s.slot, Component::STANDARD))
                .collect(),
            controllers: crate::modules::CATALOG
                .iter()
                .map(|m| (m.id, Controller::Digital))
                .collect(),
            kits: Default::default(),
        }
    }

    pub fn perf(&self, slot: Slot) -> f64 {
        self.components.get(&slot).map(|c| c.perf()).unwrap_or(1.0)
    }

    /// Every conversion kit this configuration needs.
    pub fn required_kits(&self) -> Vec<ConversionKit> {
        let mut out = Vec::new();
        for (a, b) in COUPLINGS {
            if let (Some(ca), Some(cb)) = (self.components.get(a), self.components.get(b)) {
                if Era::gap(ca.era, cb.era) >= 2 {
                    out.push(ConversionKit {
                        key: format!("couple:{a:?}:{b:?}"),
                        name: format!(
                            "{} ↔ {} adapter ({:?}/{:?})",
                            slot_info(*a).name,
                            slot_info(*b).name,
                            ca.era,
                            cb.era
                        ),
                        price: 0.15 * ca.price(*a).max(cb.price(*b)),
                    });
                }
            }
        }
        for (m, c) in &self.controllers {
            let Some(slot) = module_component(*m) else {
                continue;
            };
            let Some(comp) = self.components.get(&slot) else {
                continue;
            };
            let era = c.era(*m);
            if Era::gap(era, comp.era) >= 2 {
                out.push(ConversionKit {
                    key: format!("ctl:{m:?}"),
                    name: format!(
                        "{} interface for {:?} {} ({:?} controller)",
                        crate::modules::info(*m).name,
                        comp.era,
                        slot_info(slot).name,
                        era
                    ),
                    price: 1.0e6 + 0.25 * c.price(*m),
                });
            }
        }
        out
    }

    pub fn missing_kits(&self) -> Vec<ConversionKit> {
        self.required_kits()
            .into_iter()
            .filter(|k| !self.kits.contains(&k.key))
            .collect()
    }
}

/// Things the calendar or research can make available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unlock {
    Component { slot: Slot, era: Era },
    AnalogControllers { era: Era },
    DigitalControllers,
    Upgrade { upgrade: UpgradeId },
}

impl Unlock {
    pub fn year(self) -> i32 {
        match self {
            Unlock::Component { slot, era } => Component {
                era,
                tier: Tier::Quality,
            }
            .year(slot),
            Unlock::AnalogControllers { era } => Controller::Analog {
                era,
                tier: Tier::Quality,
            }
            .year(),
            Unlock::DigitalControllers => Controller::Digital.year(),
            Unlock::Upgrade { upgrade } => upgrade_year(upgrade),
        }
    }

    pub fn name(self) -> String {
        match self {
            Unlock::Component { slot, era } => format!("{era:?} {}", slot_info(slot).name),
            Unlock::AnalogControllers { era } => format!("{era:?} analog control cabinets"),
            Unlock::DigitalControllers => "Digital automation software".into(),
            Unlock::Upgrade { upgrade } => upgrades::info(upgrade).name.into(),
        }
    }

    /// Reference price used to cost research.
    pub fn reference_price(self) -> f64 {
        match self {
            Unlock::Component { slot, era } => Component {
                era,
                tier: Tier::Quality,
            }
            .price(slot),
            Unlock::AnalogControllers { .. } => 6.0e6,
            Unlock::DigitalControllers => 10.0e6,
            Unlock::Upgrade { upgrade } => upgrades::info(upgrade).price as f64,
        }
    }

    /// What must already be available before this can be researched.
    pub fn prerequisite(self) -> Option<Unlock> {
        match self {
            Unlock::Component {
                slot,
                era: Era::Modern,
            } => Some(Unlock::Component {
                slot,
                era: Era::Dated,
            }),
            Unlock::Component { .. } => None,
            Unlock::AnalogControllers { era: Era::Vintage } => None,
            Unlock::AnalogControllers { .. } => {
                Some(Unlock::AnalogControllers { era: Era::Vintage })
            }
            Unlock::DigitalControllers => Some(Unlock::AnalogControllers { era: Era::Dated }),
            Unlock::Upgrade { upgrade } => match upgrades::info(upgrade).computer {
                Some(c) => Some(Unlock::Upgrade { upgrade: c }),
                None => Some(Unlock::DigitalControllers),
            },
        }
    }

    pub fn all() -> Vec<Unlock> {
        let mut v = Vec::new();
        for s in SLOTS {
            for era in Era::ALL {
                v.push(Unlock::Component { slot: s.slot, era });
            }
        }
        for era in analog_eras() {
            v.push(Unlock::AnalogControllers { era });
        }
        v.push(Unlock::DigitalControllers);
        for u in upgrades::CATALOG {
            v.push(Unlock::Upgrade { upgrade: u.id });
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_plant_needs_no_kits() {
        assert!(Equipment::standard().required_kits().is_empty());
    }

    #[test]
    fn two_era_gap_needs_kits() {
        let mut e = Equipment::standard();
        e.components.insert(
            Slot::Turbine,
            Component {
                era: Era::Modern,
                tier: Tier::Premium,
            },
        );
        e.components.insert(
            Slot::Generator,
            Component {
                era: Era::Vintage,
                tier: Tier::Basic,
            },
        );
        let kits = e.required_kits();
        // turbine<->generator coupling, plus digital (modern-era sync/exciter
        // packages are dated, so only the coupling here)
        assert!(
            kits.iter().any(|k| k.key == "couple:Turbine:Generator"),
            "{kits:?}"
        );
        // A modern digital feedwater package on vintage feed pumps.
        let mut e = Equipment::standard();
        e.components.insert(
            Slot::FeedPumps,
            Component {
                era: Era::Vintage,
                tier: Tier::Quality,
            },
        );
        assert!(e
            .required_kits()
            .iter()
            .any(|k| k.key == "ctl:FeedwaterLevel"));
        // ...but a vintage analog controller matches.
        e.controllers.insert(
            ModuleId::FeedwaterLevel,
            Controller::Analog {
                era: Era::Vintage,
                tier: Tier::Basic,
            },
        );
        assert!(!e
            .required_kits()
            .iter()
            .any(|k| k.key == "ctl:FeedwaterLevel"));
    }

    #[test]
    fn perf_and_price_ordering() {
        let vb = Component {
            era: Era::Vintage,
            tier: Tier::Basic,
        };
        let mp = Component {
            era: Era::Modern,
            tier: Tier::Premium,
        };
        assert!(vb.perf() < 1.0 && mp.perf() > 1.0);
        assert!(vb.price(Slot::Turbine) < mp.price(Slot::Turbine));
        assert!(
            Controller::Digital.imprecision()
                < Controller::Analog {
                    era: Era::Dated,
                    tier: Tier::Premium
                }
                .imprecision()
        );
    }
}
