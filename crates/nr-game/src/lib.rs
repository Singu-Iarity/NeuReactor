//! NeuReactor career mode.
//!
//! A career starts in a chosen year with a construction budget. In the
//! **build phase** the player equips the plant: every physical component in
//! an era/quality tier, every automation module as an analog cabinet or
//! digital software, plus computers and panel packages. Equipment that is two
//! eras apart from what it connects to needs a conversion kit, which is added
//! to the purchase automatically. After commissioning, the **operating
//! phase** runs a calendar: power sold earns money, O&M costs money, new
//! technology appears as the years pass, and research can unlock it early.

pub mod grid_model;
pub mod station;
pub use station::{Call, Station};

use nr_api::equipment::{
    self, Component, Controller, ConversionKit, Equipment, Era, Slot, Tier, UnitSize, Unlock,
};
use nr_api::{modules, upgrades, GameMode, ModuleId, ModuleMode, UpgradeId};
use nr_reactors::Pwr;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const DAY: f64 = 86_400.0;
const DAYS_PER_YEAR: f64 = 365.0;
const MONTHS: [(&str, u32); 12] = [
    ("Jan", 31),
    ("Feb", 28),
    ("Mar", 31),
    ("Apr", 30),
    ("May", 31),
    ("Jun", 30),
    ("Jul", 31),
    ("Aug", 31),
    ("Sep", 30),
    ("Oct", 31),
    ("Nov", 30),
    ("Dec", 31),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    /// Construction budget for a career starting in `year`: a multiple of
    /// what a full set of quality components of that era costs.
    pub fn budget(self, year: i32) -> f64 {
        let era = Era::of_year(year);
        let standard: f64 = equipment::SLOTS
            .iter()
            .map(|s| Component::new(era, Tier::Quality).price(s.slot))
            .sum();
        let factor = match self {
            Difficulty::Easy => 1.4,
            Difficulty::Normal => 1.12,
            Difficulty::Hard => 0.95,
        };
        (standard * factor / 1e6).round() * 1e6
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Build,
    Operating,
}

/// Something the player can buy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Purchase {
    Component {
        slot: Slot,
        era: Era,
        tier: Tier,
        /// Size class (defaults to the unit's current size).
        #[serde(default)]
        size: Option<UnitSize>,
    },
    Controller {
        module: ModuleId,
        controller: Controller,
    },
    Upgrade {
        upgrade: UpgradeId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchProject {
    pub unlock: Unlock,
    pub name: String,
    pub cost: f64,
    pub days: f64,
    pub progress_days: f64,
    /// Funding level: 0 normal, 1 accelerated (2× speed), 2 crash (3.5×).
    #[serde(default)]
    pub funding: u8,
}

/// Research funding levels: (speed multiplier, extra spend per day as a
/// multiple of the project's normal daily cost).
pub const FUNDING: [(f64, f64, &str); 3] = [
    (1.0, 0.0, "Normal"),
    (2.0, 1.5, "Accelerated"),
    (3.5, 4.0, "Crash programme"),
];

impl ResearchProject {
    /// Normal daily cost (the up-front price spread over the duration).
    pub fn daily_cost(&self) -> f64 {
        self.cost / self.days.max(1.0)
    }
    /// Extra spend per day at the current funding level.
    pub fn extra_per_day(&self) -> f64 {
        self.daily_cost() * FUNDING[self.funding.min(2) as usize].1
    }
    pub fn speed(&self) -> f64 {
        FUNDING[self.funding.min(2) as usize].0
    }
    /// Calendar days left at the current funding.
    pub fn days_left(&self) -> f64 {
        (self.days - self.progress_days).max(0.0) / self.speed()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub date: String,
    pub text: String,
}

/// Running costs accrued over a calendar month and billed at month end.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bills {
    /// Nuclear fuel burn-up (thermal energy produced).
    pub fuel: f64,
    /// Cooling water abstraction and make-up.
    pub water: f64,
    /// Electricity imported from the grid for station service.
    pub grid_power: f64,
    /// Equipment upkeep.
    pub maintenance: f64,
    /// Operating staff.
    pub staff: f64,
    /// Diesel fuel deliveries.
    pub diesel_fuel: f64,
    /// Fines and grid penalties (charged when incurred; listed for reference).
    pub penalties: f64,
}

impl Bills {
    /// Amount still to be charged on the statement.
    pub fn payable(&self) -> f64 {
        self.fuel + self.water + self.grid_power + self.maintenance + self.staff
    }
}

/// Monthly statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statement {
    pub month: String,
    pub bills: Bills,
    pub revenue: f64,
}

/// Fuel cost per MWh thermal.
const FUEL_PER_MWH_T: f64 = 1.9;
/// Cooling water per CW pump-hour, and base make-up per hour.
const WATER_PER_PUMP_H: f64 = 260.0;
const WATER_BASE_H: f64 = 45.0;
/// Retail markup on imported station power.
const IMPORT_MARKUP: f64 = 1.6;
/// Diesel fuel price per litre.
pub const DIESEL_PER_L: f64 = 1.1;

/// Running cost rates right now [$ per hour].
pub fn hourly_costs(plant: &Pwr, price_mwh: f64, om_per_day: f64) -> Bills {
    let cw = plant.condenser.cw_pumps.iter().filter(|r| **r).count() as f64;
    Bills {
        fuel: plant.q_core.max(0.0) / 1e6 * FUEL_PER_MWH_T
            / plant.automation.equipment.perf(Slot::ReactorCore),
        water: WATER_BASE_H + WATER_PER_PUMP_H * cw,
        grid_power: plant.elec.import_mw * price_mwh * IMPORT_MARKUP,
        maintenance: 0.6 * om_per_day / 24.0,
        staff: 0.4 * om_per_day / 24.0,
        ..Bills::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Career {
    pub start_year: i32,
    pub difficulty: Difficulty,
    pub phase: Phase,
    pub money: f64,
    /// Days of operation since 1 January of the start year.
    pub elapsed_days: f64,
    pub price_mwh: f64,
    pub om_per_day: f64,
    pub revenue_total: f64,
    pub costs_total: f64,
    pub equipment: Equipment,
    pub modules: BTreeSet<ModuleId>,
    pub upgrades: BTreeSet<UpgradeId>,
    pub researched: BTreeSet<Unlock>,
    pub research: Vec<ResearchProject>,
    announced: BTreeSet<Unlock>,
    pub notifications: Vec<Notification>,
    /// Costs accrued so far this month.
    #[serde(default)]
    pub bills: Bills,
    #[serde(default)]
    pub month_revenue: f64,
    #[serde(default)]
    pub statements: Vec<Statement>,
    #[serde(default)]
    month_key: u32,
    #[serde(default)]
    was_online: bool,
    #[serde(default)]
    last_net_mw: f64,
}

/// Purchase quote shown in the shop.
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub purchase: Purchase,
    pub label: String,
    pub price: f64,
    pub kits: Vec<ConversionKit>,
    pub refund: f64,
    pub total: f64,
    pub available: bool,
    pub year: i32,
    pub owned: bool,
    pub perf: Option<f64>,
}

impl Career {
    pub fn new(start_year: i32, difficulty: Difficulty) -> Self {
        let mut c = Self {
            start_year,
            difficulty,
            phase: Phase::Build,
            money: difficulty.budget(start_year),
            elapsed_days: 0.0,
            price_mwh: 45.0,
            om_per_day: 120_000.0,
            revenue_total: 0.0,
            costs_total: 0.0,
            equipment: Equipment {
                components: BTreeMap::new(),
                controllers: BTreeMap::new(),
                kits: BTreeSet::new(),
                ic_size: Default::default(),
            },
            modules: BTreeSet::new(),
            upgrades: BTreeSet::new(),
            researched: BTreeSet::new(),
            research: Vec::new(),
            announced: BTreeSet::new(),
            notifications: Vec::new(),
            bills: Bills::default(),
            month_revenue: 0.0,
            statements: Vec::new(),
            month_key: 0,
            was_online: false,
            last_net_mw: 0.0,
        };
        // Everything already available at the start is not news.
        for u in Unlock::all() {
            if c.available(u) {
                c.announced.insert(u);
            }
        }
        c.notify(format!(
            "Construction budget ${:.0}M. Equip every plant component, choose your automation, then commission the unit.",
            c.money / 1e6
        ));
        c
    }

    // ------------------------------------------------------------ calendar

    pub fn year_fraction(&self) -> f64 {
        self.start_year as f64 + self.elapsed_days / DAYS_PER_YEAR
    }

    pub fn year(&self) -> i32 {
        self.year_fraction().floor() as i32
    }

    pub fn date(&self) -> String {
        let mut day = (self.elapsed_days % DAYS_PER_YEAR).floor() as u32;
        for (name, len) in MONTHS {
            if day < len {
                return format!("{} {name} {}", day + 1, self.year());
            }
            day -= len;
        }
        format!("31 Dec {}", self.year())
    }

    /// Months since January of the start year.
    fn month_index(&self) -> u32 {
        let years = (self.elapsed_days / DAYS_PER_YEAR).floor() as u32;
        let mut day = (self.elapsed_days % DAYS_PER_YEAR).floor() as u32;
        let mut m = 0;
        for (_, len) in MONTHS {
            if day < len {
                break;
            }
            day -= len;
            m += 1;
        }
        years * 12 + m.min(11)
    }

    fn month_name(index: u32, start_year: i32) -> String {
        format!(
            "{} {}",
            MONTHS[(index % 12) as usize].0,
            start_year + (index / 12) as i32
        )
    }

    /// Bill the month's accrued running costs.
    fn close_month(&mut self) {
        let bills = std::mem::take(&mut self.bills);
        let due = bills.payable();
        self.money -= due;
        self.costs_total += due;
        let month = Self::month_name(self.month_key, self.start_year);
        let m = |v: f64| v / 1e6;
        self.notify(format!(
            "Monthly statement {month}: fuel ${:.2}M, water ${:.2}M, grid power ${:.2}M, maintenance ${:.2}M, staff ${:.2}M — ${:.2}M charged. Revenue ${:.2}M.",
            m(bills.fuel),
            m(bills.water),
            m(bills.grid_power),
            m(bills.maintenance),
            m(bills.staff),
            m(due),
            m(self.month_revenue)
        ));
        self.statements.push(Statement {
            month,
            bills,
            revenue: self.month_revenue,
        });
        if self.statements.len() > 36 {
            self.statements.remove(0);
        }
        self.month_revenue = 0.0;
    }

    /// Price to refill both emergency diesel fuel tanks.
    pub fn diesel_fuel_quote(&self, plant: &Pwr) -> f64 {
        plant
            .elec
            .diesels
            .iter()
            .map(|d| (d.fuel_capacity_l - d.fuel_l).max(0.0))
            .sum::<f64>()
            * DIESEL_PER_L
    }

    /// Order a diesel fuel delivery (fills both tanks).
    pub fn buy_diesel_fuel(&mut self, plant: &mut Pwr) -> Result<(), String> {
        let cost = self.diesel_fuel_quote(plant);
        if cost < 1.0 {
            return Err("diesel fuel tanks are already full".into());
        }
        if cost > self.money {
            return Err("not enough money".into());
        }
        self.money -= cost;
        self.costs_total += cost;
        self.bills.diesel_fuel += cost;
        for d in &mut plant.elec.diesels {
            d.fuel_l = d.fuel_capacity_l;
        }
        self.notify(format!("Diesel fuel delivered: ${:.0}k", cost / 1e3));
        Ok(())
    }

    /// The unit at hot standby, sized by its core, with this career's
    /// equipment and every module in MANUAL.
    fn fresh_plant(&self) -> Pwr {
        let mut plant = Pwr::hot_standby_sized(self.equipment.unit_size());
        plant.automation.game_mode = GameMode::Career;
        for m in modules::CATALOG {
            plant.automation.modes.insert(m.id, ModuleMode::Manual);
        }
        self.push_to_plant(&mut plant);
        plant
    }

    /// Size of a component purchase: as asked, or matched to the unit.
    fn size_for(&self, slot: Slot, size: Option<UnitSize>) -> UnitSize {
        if !equipment::slot_is_sized(slot) {
            return UnitSize::default();
        }
        size.unwrap_or_else(|| self.equipment.unit_size())
    }

    /// Pay a credit (e.g. a grid capacity premium).
    pub fn credit(&mut self, amount: f64, why: &str) {
        self.money += amount;
        self.revenue_total += amount;
        self.month_revenue += amount;
        self.notify(format!("Credit ${:.2}M: {why}", amount / 1e6));
    }

    /// Charge a fine (e.g. synchronising without grid permission).
    pub fn fine(&mut self, amount: f64, why: &str) {
        self.money -= amount;
        self.costs_total += amount;
        self.bills.penalties += amount;
        self.notify(format!("Fine ${:.2}M: {why}", amount / 1e6));
    }

    fn notify(&mut self, text: String) {
        let date = self.date();
        self.notifications.push(Notification { date, text });
        if self.notifications.len() > 200 {
            self.notifications.remove(0);
        }
    }

    pub fn available(&self, u: Unlock) -> bool {
        self.year() >= u.year() || self.researched.contains(&u)
    }

    fn unlock_for(p: Purchase) -> Unlock {
        match p {
            Purchase::Component { slot, era, .. } => Unlock::Component { slot, era },
            Purchase::Controller {
                controller: Controller::Analog { era, .. },
                ..
            } => Unlock::AnalogControllers { era },
            Purchase::Controller {
                controller: Controller::Digital,
                ..
            } => Unlock::DigitalControllers,
            Purchase::Upgrade { upgrade } => Unlock::Upgrade { upgrade },
        }
    }

    // ---------------------------------------------------------------- shop

    /// Price, conversion kits and refund for a purchase.
    pub fn quote(&self, p: Purchase) -> Quote {
        let mut after = self.equipment.clone();
        let (price, refund, owned, label, perf) = match p {
            Purchase::Component {
                slot,
                era,
                tier,
                size,
            } => {
                let c = Component::sized(era, tier, self.size_for(slot, size));
                let old = after.components.insert(slot, c);
                // A core bought during construction is the design size:
                // the I&C is engineered for it from the start.
                if slot == Slot::ReactorCore && self.phase == Phase::Build {
                    after.ic_size = c.size;
                }
                let refund = old
                    .map(|o| o.price(slot) * self.refund_rate())
                    .unwrap_or(0.0);
                (
                    c.price(slot),
                    refund,
                    old == Some(c),
                    c.label(),
                    Some(c.perf()),
                )
            }
            Purchase::Controller { module, controller } => {
                let old = if self.modules.contains(&module) {
                    after.controllers.get(&module).copied()
                } else {
                    None
                };
                after.controllers.insert(module, controller);
                let refund = old
                    .map(|o| o.price(module) * self.refund_rate())
                    .unwrap_or(0.0);
                (
                    controller.price(module),
                    refund,
                    old == Some(controller),
                    controller.label(),
                    None,
                )
            }
            Purchase::Upgrade { upgrade } => {
                let owned = self.upgrades.contains(&upgrade);
                (
                    upgrades::info(upgrade).price as f64,
                    0.0,
                    owned,
                    upgrades::info(upgrade).name.to_string(),
                    None,
                )
            }
        };
        let kits: Vec<ConversionKit> = after
            .required_kits()
            .into_iter()
            .filter(|k| !self.equipment.kits.contains(&k.key))
            .collect();
        let kit_cost: f64 = kits.iter().map(|k| k.price).sum();
        let u = Self::unlock_for(p);
        // Unit sizes arrive over the years (bigger units later).
        let size_year = match p {
            Purchase::Component { slot, size, .. } if equipment::slot_is_sized(slot) => {
                self.size_for(slot, size).year()
            }
            _ => i32::MIN,
        };
        Quote {
            purchase: p,
            label,
            price,
            total: if owned {
                0.0
            } else {
                price + kit_cost - refund
            },
            kits,
            refund,
            available: self.available(u) && self.year() >= size_year,
            year: u.year().max(size_year),
            owned,
            perf,
        }
    }

    fn refund_rate(&self) -> f64 {
        match self.phase {
            Phase::Build => 1.0,
            Phase::Operating => 0.2,
        }
    }

    pub fn buy(&mut self, p: Purchase, plant: &mut Pwr) -> Result<(), String> {
        let q = self.quote(p);
        if q.owned {
            return Err("already installed".into());
        }
        if !q.available {
            return Err(format!("not available until {} (or research it)", q.year));
        }
        if let Purchase::Component { .. } = p {
            if self.phase == Phase::Operating && !plant.in_outage() {
                return Err("major components can only be replaced during an outage (reactor shut down, generator off-line)".into());
            }
        }
        if q.total > self.money {
            return Err(format!("insufficient funds: need ${:.1}M", q.total / 1e6));
        }
        self.money -= q.total;
        self.costs_total += q.total.max(0.0);
        for k in &q.kits {
            self.equipment.kits.insert(k.key.clone());
        }
        match p {
            Purchase::Component {
                slot,
                era,
                tier,
                size,
            } => {
                let size = self.size_for(slot, size);
                let old_unit = self.equipment.unit_size();
                self.equipment
                    .components
                    .insert(slot, Component::sized(era, tier, size));
                if slot == Slot::ReactorCore {
                    if self.phase == Phase::Build {
                        self.equipment.ic_size = size;
                    } else if size != old_unit {
                        // New core: the unit is rebuilt (refuelled) at the
                        // new size, at hot standby.
                        *plant = self.fresh_plant();
                        self.notify(format!(
                            "New {} core installed: {} fuel assemblies. Re-scale any undersized components before running at full power.",
                            size.label(),
                            size.assemblies()
                        ));
                    }
                }
            }
            Purchase::Controller { module, controller } => {
                self.equipment.controllers.insert(module, controller);
                self.modules.insert(module);
            }
            Purchase::Upgrade { upgrade } => {
                self.upgrades.insert(upgrade);
            }
        }
        let kit_note = if q.kits.is_empty() {
            String::new()
        } else {
            format!(
                " (+ {})",
                q.kits
                    .iter()
                    .map(|k| k.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        self.notify(format!(
            "Purchased {}: ${:.1}M{kit_note}",
            purchase_name(p),
            q.total / 1e6
        ));
        if self.phase == Phase::Operating {
            self.push_to_plant(plant);
        }
        Ok(())
    }

    /// Everything the player can buy right now, with quotes.
    pub fn shop(&self) -> Shop {
        let components = equipment::SLOTS
            .iter()
            .map(|s| ShopSlot {
                slot: s.slot,
                name: s.name.into(),
                effect: s.effect.into(),
                installed: self.equipment.components.get(&s.slot).copied(),
                sized: equipment::slot_is_sized(s.slot),
                options: {
                    let sizes: Vec<UnitSize> = if equipment::slot_is_sized(s.slot) {
                        UnitSize::ALL.to_vec()
                    } else {
                        vec![UnitSize::default()]
                    };
                    sizes
                        .into_iter()
                        .flat_map(|size| {
                            Era::ALL.iter().flat_map(move |era| {
                                Tier::ALL.iter().map(move |tier| (size, *era, *tier))
                            })
                        })
                        .map(|(size, era, tier)| {
                            self.quote(Purchase::Component {
                                slot: s.slot,
                                era,
                                tier,
                                size: Some(size),
                            })
                        })
                        .collect()
                },
            })
            .collect();
        let controllers = modules::CATALOG
            .iter()
            .filter(|m| m.implemented)
            .map(|m| {
                let mut options: Vec<Quote> = Vec::new();
                // Supervisory software (plant master, gateway) has no analog version.
                if !m.actuators.is_empty() || equipment::module_component(m.id).is_some() {
                    for era in equipment::analog_eras() {
                        for tier in Tier::ALL {
                            options.push(self.quote(Purchase::Controller {
                                module: m.id,
                                controller: Controller::Analog { era, tier },
                            }));
                        }
                    }
                }
                options.push(self.quote(Purchase::Controller {
                    module: m.id,
                    controller: Controller::Digital,
                }));
                ShopModule {
                    module: m.id,
                    name: m.name.into(),
                    panel: m.panel.into(),
                    installed: if self.modules.contains(&m.id) {
                        self.equipment.controllers.get(&m.id).copied()
                    } else {
                        None
                    },
                    requires: m.requires_upgrades.to_vec(),
                    options,
                }
            })
            .collect();
        let hardware = upgrades::CATALOG
            .iter()
            .map(|u| self.quote(Purchase::Upgrade { upgrade: u.id }))
            .collect();
        Shop {
            components,
            controllers,
            hardware,
        }
    }

    pub fn missing_components(&self) -> Vec<Slot> {
        equipment::SLOTS
            .iter()
            .map(|s| s.slot)
            .filter(|s| !self.equipment.components.contains_key(s))
            .collect()
    }

    // ------------------------------------------------------------ research

    pub fn research_options(&self) -> Vec<ResearchOption> {
        Unlock::all()
            .into_iter()
            .filter(|u| !self.available(*u) && !self.research.iter().any(|r| r.unlock == *u))
            .map(|u| {
                let (cost, days) = self.research_terms(u);
                let prereq_ok = u.prerequisite().map(|p| self.available(p)).unwrap_or(true);
                ResearchOption {
                    unlock: u,
                    name: u.name(),
                    year: u.year(),
                    cost,
                    days,
                    prerequisite: u.prerequisite().filter(|_| !prereq_ok).map(|p| p.name()),
                }
            })
            .collect()
    }

    fn research_terms(&self, u: Unlock) -> (f64, f64) {
        let early = (u.year() as f64 - self.year_fraction()).max(0.0);
        let cost = u.reference_price() * 0.35 * (1.0 + 0.2 * early);
        let days = (20.0 + 12.0 * early).min(365.0);
        (cost, days)
    }

    /// Change a running project's funding level (0 normal, 1 accelerated,
    /// 2 crash programme). Extra funding is charged daily.
    pub fn fund_research(&mut self, u: Unlock, level: u8) -> Result<(), String> {
        let level = level.min(2);
        let Some(r) = self.research.iter_mut().find(|r| r.unlock == u) else {
            return Err("that project is not running".into());
        };
        r.funding = level;
        let (name, extra, label) = (r.name.clone(), r.extra_per_day(), FUNDING[level as usize].2);
        self.notify(format!(
            "{name}: {label} funding{}",
            if extra > 0.0 {
                format!(" (+${:.0}k per day)", extra / 1e3)
            } else {
                String::new()
            }
        ));
        Ok(())
    }

    pub fn start_research(&mut self, u: Unlock) -> Result<(), String> {
        if self.available(u) {
            return Err("already available".into());
        }
        if self.research.iter().any(|r| r.unlock == u) {
            return Err("already being researched".into());
        }
        if let Some(p) = u.prerequisite() {
            if !self.available(p) {
                return Err(format!("requires {} first", p.name()));
            }
        }
        let (cost, days) = self.research_terms(u);
        if cost > self.money {
            return Err(format!("insufficient funds: need ${:.1}M", cost / 1e6));
        }
        self.money -= cost;
        self.costs_total += cost;
        self.research.push(ResearchProject {
            unlock: u,
            name: u.name(),
            cost,
            days,
            progress_days: 0.0,
            funding: 0,
        });
        self.notify(format!(
            "Research started: {} ({days:.0} days, ${:.1}M)",
            u.name(),
            cost / 1e6
        ));
        Ok(())
    }

    // ----------------------------------------------------------- lifecycle

    /// Finish the build phase. Returns the plant to operate.
    pub fn commission(&mut self) -> Result<Pwr, String> {
        if self.phase != Phase::Build {
            return Err("already commissioned".into());
        }
        let missing = self.missing_components();
        if !missing.is_empty() {
            let names: Vec<&str> = missing
                .iter()
                .map(|s| equipment::slot_info(*s).name)
                .collect();
            return Err(format!("still to equip: {}", names.join(", ")));
        }
        self.phase = Phase::Operating;
        let plant = self.fresh_plant();
        self.notify(format!(
            "Unit commissioned with ${:.0}M remaining. The reactor is at hot standby: take it critical and sell power at ${:.0}/MWh.",
            self.money / 1e6,
            self.price_mwh
        ));
        Ok(plant)
    }

    fn push_to_plant(&self, plant: &mut Pwr) {
        let mut e = self.equipment.clone();
        // Controllers of modules not owned are irrelevant to the plant.
        e.controllers.retain(|m, _| self.modules.contains(m));
        for m in modules::CATALOG {
            e.controllers.entry(m.id).or_insert(Controller::Digital);
        }
        plant.install(self.modules.clone(), self.upgrades.clone(), e);
    }

    /// Advance the career by `dt` seconds of plant time.
    pub fn tick(&mut self, dt: f64, plant: &Pwr) {
        self.tick_with(dt, plant, 1.0)
    }

    /// Advance with a grid performance revenue multiplier.
    pub fn tick_with(&mut self, dt: f64, plant: &Pwr, revenue_mult: f64) {
        if self.phase != Phase::Operating || dt <= 0.0 {
            return;
        }
        let days = dt / DAY;
        let year_before = self.year();
        self.elapsed_days += days;
        let hours = dt / 3600.0;
        let net_mw = plant.gross_mwe - plant.house_load();
        if plant.tg.breaker && net_mw > 0.0 {
            let revenue = net_mw * hours * self.price_mwh * revenue_mult;
            self.money += revenue;
            self.revenue_total += revenue;
            self.month_revenue += revenue;
        }
        // Accrue running costs; they are billed at the end of the month.
        let r = hourly_costs(plant, self.price_mwh, self.om_per_day);
        let b = &mut self.bills;
        b.fuel += r.fuel * hours;
        b.water += r.water * hours;
        b.grid_power += r.grid_power * hours;
        b.maintenance += r.maintenance * hours;
        b.staff += r.staff * hours;
        // Unplanned loss of generation: the grid operator penalises the
        // sudden loss of the unit's output.
        if self.was_online && !plant.tg.breaker && !plant.breaker_opened_by_operator {
            let lost = self.last_net_mw.max(0.0);
            let penalty = 100_000.0 + 1_500.0 * lost;
            self.fine(
                penalty,
                &format!("unplanned loss of {lost:.0} MWe from the grid"),
            );
        }
        self.was_online = plant.tg.breaker;
        self.last_net_mw = net_mw;
        let key = self.month_index();
        if key != self.month_key {
            self.close_month();
            self.month_key = key;
        }

        // Extra research funding is spent daily; out of money, projects
        // fall back to normal funding.
        let extra: f64 = self.research.iter().map(|r| r.extra_per_day() * days).sum();
        if extra > 0.0 {
            if self.money >= extra {
                self.money -= extra;
                self.costs_total += extra;
            } else {
                for r in &mut self.research {
                    r.funding = 0;
                }
                self.notify("Research funding cut back to normal: not enough money".into());
            }
        }
        let mut finished = Vec::new();
        for r in &mut self.research {
            r.progress_days += days * r.speed();
            if r.progress_days >= r.days {
                finished.push(r.unlock);
            }
        }
        self.research.retain(|r| r.progress_days < r.days);
        for u in finished {
            self.researched.insert(u);
            self.announced.insert(u);
            self.notify(format!(
                "Research complete: you can now install {}.",
                u.name()
            ));
        }
        if self.year() != year_before {
            for u in Unlock::all() {
                if !self.announced.contains(&u) && self.available(u) {
                    self.announced.insert(u);
                    self.notify(format!(
                        "Due to new research, you can now install {}.",
                        u.name()
                    ));
                }
            }
        }
    }

    pub fn view(&self) -> CareerView {
        CareerView {
            phase: self.phase,
            start_year: self.start_year,
            difficulty: self.difficulty,
            date: self.date(),
            year: self.year(),
            money: self.money,
            price_mwh: self.price_mwh,
            om_per_day: self.om_per_day,
            revenue_total: self.revenue_total,
            costs_total: self.costs_total,
            missing_components: self.missing_components(),
            notifications: self.notifications.iter().rev().take(60).cloned().collect(),
            unit_size: self.equipment.unit_size(),
            bills: self.bills.clone(),
            month_revenue: self.month_revenue,
            statements: self.statements.iter().rev().take(12).cloned().collect(),
            research: self.research.clone(),
            research_options: self.research_options(),
            shop: self.shop(),
        }
    }
}

fn purchase_name(p: Purchase) -> String {
    match p {
        Purchase::Component {
            slot,
            era,
            tier,
            size,
        } => {
            let size = size.map(|s| format!(" {}", s.label())).unwrap_or_default();
            format!("{era:?} {tier:?}{size} {}", equipment::slot_info(slot).name)
        }
        Purchase::Controller { module, controller } => {
            format!("{} ({})", modules::info(module).name, controller.label())
        }
        Purchase::Upgrade { upgrade } => upgrades::info(upgrade).name.into(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ShopSlot {
    pub slot: Slot,
    pub name: String,
    pub effect: String,
    /// Comes in size classes (everything but the board instruments).
    pub sized: bool,
    pub installed: Option<Component>,
    pub options: Vec<Quote>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShopModule {
    pub module: ModuleId,
    pub name: String,
    pub panel: String,
    pub installed: Option<Controller>,
    pub requires: Vec<UpgradeId>,
    pub options: Vec<Quote>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Shop {
    pub components: Vec<ShopSlot>,
    pub controllers: Vec<ShopModule>,
    pub hardware: Vec<Quote>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResearchOption {
    pub unlock: Unlock,
    pub name: String,
    pub year: i32,
    pub cost: f64,
    pub days: f64,
    pub prerequisite: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CareerView {
    pub phase: Phase,
    pub start_year: i32,
    pub difficulty: Difficulty,
    pub date: String,
    pub year: i32,
    pub money: f64,
    pub price_mwh: f64,
    pub om_per_day: f64,
    pub revenue_total: f64,
    pub costs_total: f64,
    pub missing_components: Vec<Slot>,
    pub notifications: Vec<Notification>,
    pub unit_size: UnitSize,
    pub bills: Bills,
    pub month_revenue: f64,
    pub statements: Vec<Statement>,
    pub research: Vec<ResearchProject>,
    pub research_options: Vec<ResearchOption>,
    pub shop: Shop,
}

#[cfg(test)]
mod tests;
