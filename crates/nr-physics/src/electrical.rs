//! Plant electrical distribution.
//!
//! ```text
//!  GRID ──[SUT]── startup transformer ──[A-SUT]──┐
//!                                                 BUS A (6.9 kV) ──[TIE]── BUS B (6.9 kV)
//!  GENERATOR ── unit aux transformer ──[A-UAT]────┘            └──[B-UAT]── UAT
//!       BUS A ──[S1]── SAFETY BUS 1 ──[STIE]── SAFETY BUS 2 ──[S2]── BUS B
//!       EDG 1 ──[EDG1]──┘                         └──[EDG2]── EDG 2
//!       SAFETY BUSES ── battery charger ── 125 V DC BUS ── BATTERY
//! ```
//! Bus B has no startup-transformer feeder of its own: it is normally fed from
//! the unit auxiliary transformer (generator on-line) or through the bus tie.
//! Motors draw a starting-current spike; sustained transformer overload trips
//! it on overcurrent.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Breaker {
    Sut,
    BusASut,
    BusAUat,
    BusBUat,
    TieAB,
    Safety1Feed,
    Safety2Feed,
    SafetyTie,
    Edg1,
    Edg2,
}

impl Breaker {
    pub const ALL: [Breaker; 10] = [
        Breaker::Sut,
        Breaker::BusASut,
        Breaker::BusAUat,
        Breaker::BusBUat,
        Breaker::TieAB,
        Breaker::Safety1Feed,
        Breaker::Safety2Feed,
        Breaker::SafetyTie,
        Breaker::Edg1,
        Breaker::Edg2,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DieselState {
    #[default]
    Stopped,
    Starting,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diesel {
    pub state: DieselState,
    pub start_timer: f64,
    pub fuel_l: f64,
    pub fuel_capacity_l: f64,
    pub load_mw: f64,
}

impl Diesel {
    fn new() -> Self {
        Self {
            state: DieselState::Stopped,
            start_timer: 0.0,
            fuel_l: 60_000.0,
            fuel_capacity_l: 60_000.0,
            load_mw: 0.0,
        }
    }

    pub fn running(&self) -> bool {
        self.state == DieselState::Running && self.fuel_l > 0.0
    }
}

/// Electrical loads requested by the plant this step (what is switched on).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadDemand {
    pub rcps: [bool; 4],
    pub cw_pumps: [bool; 3],
    pub vacuum_pumps: [bool; 2],
    /// Feedwater demand 0..1 (motor-driven feed pumps A on bus A, B on bus B).
    pub feed: f64,
    pub pzr_heaters: f64,
    pub charging: bool,
}

/// What actually has power this step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BusState {
    pub bus_a: bool,
    pub bus_b: bool,
    pub safety1: bool,
    pub safety2: bool,
    pub dc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Electrical {
    pub breakers: BTreeMap<Breaker, bool>,
    pub grid_available: bool,
    pub sut_tripped: bool,
    pub uat_tripped: bool,
    /// Startup transformer rating [MW].
    pub sut_rating: f64,
    pub uat_rating: f64,
    pub sut_load_mw: f64,
    pub uat_load_mw: f64,
    pub edg_load_mw: f64,
    /// Total station service load [MW].
    pub house_load_mw: f64,
    /// Power drawn from the grid through the startup transformer [MW].
    pub import_mw: f64,
    pub diesels: [Diesel; 2],
    pub battery_pct: f64,
    pub dc_volts: f64,
    pub buses: BusState,
    /// Remaining motor-start inrush per load (seconds, MW).
    inrush: Vec<(f64, f64, u8)>,
    sut_overcurrent: f64,
    uat_overcurrent: f64,
    prev: LoadDemand,
    /// Equipment quality multipliers.
    pub transformer_quality: f64,
    pub diesel_quality: f64,
    /// Plant size relative to the 1150 MWe reference (scales motor loads).
    #[serde(default = "one")]
    pub scale: f64,
}

fn one() -> f64 {
    1.0
}

const BASE_A: f64 = 4.0;
const BASE_B: f64 = 4.0;
const BASE_SAFETY: f64 = 1.5;
const RCP_MW: f64 = 6.0;
const CW_MW: f64 = 3.0;
const VAC_MW: f64 = 0.3;
const FEED_MW: f64 = 9.0;
const HEATER_MW: f64 = 1.8;
const CHARGING_MW: f64 = 0.6;

impl Electrical {
    /// Normal line-up with the generator on-line.
    pub fn normal() -> Self {
        let mut b = BTreeMap::new();
        for br in Breaker::ALL {
            b.insert(br, true);
        }
        // Bus A on the UAT at power; startup feeder open, tie open, EDGs off.
        b.insert(Breaker::BusASut, false);
        b.insert(Breaker::TieAB, false);
        b.insert(Breaker::SafetyTie, false);
        b.insert(Breaker::Edg1, false);
        b.insert(Breaker::Edg2, false);
        Self::with(b)
    }

    /// Shutdown line-up: bus A on the startup transformer, bus B through the tie.
    pub fn shutdown() -> Self {
        let mut b = BTreeMap::new();
        for br in Breaker::ALL {
            b.insert(br, true);
        }
        b.insert(Breaker::BusAUat, false);
        b.insert(Breaker::BusBUat, false);
        b.insert(Breaker::SafetyTie, false);
        b.insert(Breaker::Edg1, false);
        b.insert(Breaker::Edg2, false);
        Self::with(b)
    }

    fn with(breakers: BTreeMap<Breaker, bool>) -> Self {
        Self {
            breakers,
            grid_available: true,
            sut_tripped: false,
            uat_tripped: false,
            sut_rating: 80.0,
            uat_rating: 80.0,
            sut_load_mw: 0.0,
            uat_load_mw: 0.0,
            edg_load_mw: 0.0,
            house_load_mw: 0.0,
            import_mw: 0.0,
            diesels: [Diesel::new(), Diesel::new()],
            battery_pct: 100.0,
            dc_volts: 132.0,
            buses: BusState {
                bus_a: true,
                bus_b: true,
                safety1: true,
                safety2: true,
                dc: true,
            },
            inrush: Vec::new(),
            sut_overcurrent: 0.0,
            uat_overcurrent: 0.0,
            prev: LoadDemand::default(),
            transformer_quality: 1.0,
            diesel_quality: 1.0,
            scale: 1.0,
        }
    }

    pub fn closed(&self, b: Breaker) -> bool {
        self.breakers.get(&b).copied().unwrap_or(false)
    }

    pub fn breaker_from_name(name: &str) -> Option<Breaker> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }

    pub fn set_breaker(&mut self, b: Breaker, closed: bool) {
        self.breakers.insert(b, closed);
    }

    pub fn start_diesel(&mut self, i: usize) {
        let d = &mut self.diesels[i];
        if d.state == DieselState::Stopped && d.fuel_l > 0.0 {
            d.state = DieselState::Starting;
            d.start_timer = 10.0 / self.diesel_quality.powi(4);
        }
    }

    pub fn stop_diesel(&mut self, i: usize) {
        self.diesels[i].state = DieselState::Stopped;
        let br = if i == 0 { Breaker::Edg1 } else { Breaker::Edg2 };
        self.set_breaker(br, false);
    }

    /// Seed the running loads without motor-start inrush (initial conditions).
    pub fn settle(&mut self, demand: &LoadDemand) {
        self.prev = demand.clone();
        self.inrush.clear();
    }

    /// Reset a tripped transformer (operator, after clearing the overload).
    pub fn reset_transformers(&mut self) {
        self.sut_tripped = false;
        self.uat_tripped = false;
        self.sut_overcurrent = 0.0;
        self.uat_overcurrent = 0.0;
    }

    /// Advance. `gen_online` = main generator breaker closed (UAT source).
    /// Returns any protective events.
    pub fn step(&mut self, demand: &LoadDemand, gen_online: bool, dt: f64) -> Vec<&'static str> {
        let mut events = Vec::new();
        // Motor start inrush (≈5× running power for a few seconds).
        for i in 0..4 {
            if demand.rcps[i] && !self.prev.rcps[i] {
                self.inrush
                    .push((8.0, 5.0 * RCP_MW * self.scale, if i < 2 { 0 } else { 1 }));
            }
        }
        for i in 0..3 {
            if demand.cw_pumps[i] && !self.prev.cw_pumps[i] {
                self.inrush
                    .push((5.0, 4.0 * CW_MW * self.scale, if i < 2 { 0 } else { 1 }));
            }
        }
        self.prev = demand.clone();
        for r in &mut self.inrush {
            r.0 -= dt;
        }
        self.inrush.retain(|r| r.0 > 0.0);

        // Diesels.
        let dq = self.diesel_quality;
        for (i, d) in self.diesels.iter_mut().enumerate() {
            if d.state == DieselState::Starting {
                d.start_timer -= dt;
                if d.start_timer <= 0.0 {
                    d.state = DieselState::Running;
                    events.push(if i == 0 {
                        "EDG 1 up to speed"
                    } else {
                        "EDG 2 up to speed"
                    });
                }
            }
            if d.state == DieselState::Running {
                // ~0.07 L/s idle plus ~0.25 L/s per MW.
                d.fuel_l = (d.fuel_l - (0.07 + 0.25 * d.load_mw) / dq * dt).max(0.0);
                if d.fuel_l <= 0.0 {
                    d.state = DieselState::Stopped;
                    events.push("EDG out of fuel");
                }
            }
        }

        // Fast bus transfer: when the unit auxiliary transformer is lost
        // (generator trip), protective relaying transfers the 6.9 kV buses
        // to the startup transformer within cycles, so running motors (RCPs)
        // ride through.
        let sut_ok = self.grid_available && self.closed(Breaker::Sut) && !self.sut_tripped;
        if !(gen_online && !self.uat_tripped) && sut_ok {
            if self.closed(Breaker::BusAUat) {
                self.set_breaker(Breaker::BusAUat, false);
                self.set_breaker(Breaker::BusASut, true);
                events.push("Fast bus transfer: bus A to startup transformer");
            }
            if self.closed(Breaker::BusBUat) {
                self.set_breaker(Breaker::BusBUat, false);
                self.set_breaker(Breaker::TieAB, true);
                events.push("Fast bus transfer: bus B to startup transformer via A-B tie");
            }
        }

        // Sources.
        let sut = sut_ok;
        let uat = gen_online && !self.uat_tripped;
        let a_sut = sut && self.closed(Breaker::BusASut);
        let a_uat = uat && self.closed(Breaker::BusAUat);
        let b_uat = uat && self.closed(Breaker::BusBUat);
        let tie = self.closed(Breaker::TieAB);
        let mut bus_a = a_sut || a_uat;
        let mut bus_b = b_uat;
        if tie && (bus_a || bus_b) {
            bus_a = true;
            bus_b = true;
        }
        let e1 = self.diesels[0].running() && self.closed(Breaker::Edg1);
        let e2 = self.diesels[1].running() && self.closed(Breaker::Edg2);
        let mut s1 = (bus_a && self.closed(Breaker::Safety1Feed)) || e1;
        let mut s2 = (bus_b && self.closed(Breaker::Safety2Feed)) || e2;
        if self.closed(Breaker::SafetyTie) && (s1 || s2) {
            s1 = true;
            s2 = true;
        }

        // DC: chargers on the safety buses; battery carries the bus otherwise.
        let charger = s1 || s2;
        if charger {
            self.battery_pct = (self.battery_pct + 0.02 * dt).min(100.0);
        } else {
            // About 2 hours of battery.
            self.battery_pct = (self.battery_pct - 100.0 / 7200.0 * dt).max(0.0);
        }
        self.dc_volts = if charger {
            132.0
        } else {
            105.0 + 20.0 * (self.battery_pct / 100.0).sqrt()
        };
        let dc = self.dc_volts > 108.0;

        // Loads per bus.
        let feed_each = FEED_MW * demand.feed.clamp(0.0, 1.0);
        let mut load_a = BASE_A + feed_each + if demand.vacuum_pumps[0] { VAC_MW } else { 0.0 };
        let mut load_b = BASE_B + feed_each + if demand.vacuum_pumps[1] { VAC_MW } else { 0.0 };
        for i in 0..4 {
            if demand.rcps[i] {
                if i < 2 {
                    load_a += RCP_MW
                } else {
                    load_b += RCP_MW
                }
            }
        }
        for i in 0..3 {
            if demand.cw_pumps[i] {
                if i < 2 {
                    load_a += CW_MW
                } else {
                    load_b += CW_MW
                }
            }
        }
        let k = self.scale;
        load_a *= k;
        load_b *= k;
        let load_s1 = k
            * (BASE_SAFETY
                + HEATER_MW * demand.pzr_heaters.clamp(0.0, 1.0)
                + if demand.charging { CHARGING_MW } else { 0.0 });
        let load_s2 = k * BASE_SAFETY;
        for r in &self.inrush {
            if r.2 == 0 {
                load_a += r.1
            } else {
                load_b += r.1
            }
        }
        let s1_from_a = s1 && !e1 && self.closed(Breaker::Safety1Feed) && bus_a;
        let s2_from_b = s2 && !e2 && self.closed(Breaker::Safety2Feed) && bus_b;
        if s1_from_a {
            load_a += load_s1;
        }
        if s2_from_b {
            load_b += load_s2;
        }
        let edg_load = if e1 { load_s1 } else { 0.0 } + if e2 { load_s2 } else { 0.0 };
        self.diesels[0].load_mw = if e1 {
            load_s1
                + if !e2 && self.closed(Breaker::SafetyTie) {
                    load_s2
                } else {
                    0.0
                }
        } else {
            0.0
        };
        self.diesels[1].load_mw = if e2 { load_s2 } else { 0.0 };
        self.edg_load_mw = edg_load;

        // Split network load between the transformers.
        let (mut sut_load, mut uat_load) = (0.0, 0.0);
        let net_a = if bus_a { load_a } else { 0.0 };
        let net_b = if bus_b { load_b } else { 0.0 };
        match (
            a_sut || (tie && !a_uat && !b_uat && sut && self.closed(Breaker::BusASut)),
            a_uat,
            b_uat,
        ) {
            (_, true, true) => uat_load = net_a + net_b,
            (true, false, true) => {
                if tie {
                    uat_load = net_a + net_b
                } else {
                    sut_load = net_a;
                    uat_load = net_b
                }
            }
            (true, false, false) => sut_load = net_a + net_b,
            (false, true, false) => uat_load = net_a + net_b,
            _ => {}
        }
        if a_sut && a_uat {
            // Paralleled sources share.
            sut_load = 0.5 * (net_a + net_b);
            uat_load = sut_load;
        }
        self.sut_load_mw = sut_load;
        self.uat_load_mw = uat_load;
        self.import_mw = sut_load;
        self.house_load_mw = net_a + net_b + edg_load;

        // Transformer overcurrent (inverse-time-ish integrator).
        let sut_rating = self.sut_rating * self.transformer_quality.powi(3);
        if sut_load > 1.2 * sut_rating {
            self.sut_overcurrent += (sut_load / sut_rating - 1.2) * dt;
        } else {
            self.sut_overcurrent = (self.sut_overcurrent - 0.2 * dt).max(0.0);
        }
        if self.sut_overcurrent > 2.0 && !self.sut_tripped {
            self.sut_tripped = true;
            events.push("Startup transformer overcurrent trip");
        }
        let uat_rating = self.uat_rating * self.transformer_quality.powi(3);
        if uat_load > 1.2 * uat_rating {
            self.uat_overcurrent += (uat_load / uat_rating - 1.2) * dt;
        } else {
            self.uat_overcurrent = (self.uat_overcurrent - 0.2 * dt).max(0.0);
        }
        if self.uat_overcurrent > 2.0 && !self.uat_tripped {
            self.uat_tripped = true;
            events.push("Unit auxiliary transformer overcurrent trip");
        }

        self.buses = BusState {
            bus_a,
            bus_b,
            safety1: s1,
            safety2: s2,
            dc,
        };
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demand() -> LoadDemand {
        LoadDemand {
            rcps: [true; 4],
            cw_pumps: [true; 3],
            vacuum_pumps: [false; 2],
            feed: 0.1,
            pzr_heaters: 0.1,
            charging: true,
        }
    }

    #[test]
    fn shutdown_lineup_powers_both_buses_from_sut() {
        let mut e = Electrical::shutdown();
        e.settle(&demand());
        e.step(&demand(), false, 1.0);
        e.step(&demand(), false, 10.0);
        assert!(e.buses.bus_a && e.buses.bus_b && e.buses.safety1 && e.buses.dc);
        assert!(
            e.sut_load_mw > 30.0 && e.uat_load_mw == 0.0,
            "{}",
            e.sut_load_mw
        );
    }

    #[test]
    fn bus_b_needs_uat_or_tie() {
        let mut e = Electrical::shutdown();
        e.settle(&demand());
        e.set_breaker(Breaker::TieAB, false);
        e.step(&demand(), false, 1.0);
        assert!(e.buses.bus_a && !e.buses.bus_b);
        e.set_breaker(Breaker::BusBUat, true);
        e.step(&demand(), true, 1.0);
        assert!(e.buses.bus_b);
    }

    #[test]
    fn starting_everything_at_once_overloads_the_sut() {
        let mut e = Electrical::shutdown();
        let mut d = LoadDemand::default();
        e.step(&d, false, 1.0);
        d = demand();
        d.feed = 1.0;
        let mut tripped = false;
        for _ in 0..100 {
            if e.step(&d, false, 0.1)
                .contains(&"Startup transformer overcurrent trip")
            {
                tripped = true;
                break;
            }
        }
        assert!(tripped, "sut load {}", e.sut_load_mw);
        e.step(&d, false, 0.1);
        assert!(!e.buses.bus_a);
    }

    #[test]
    fn loss_of_offsite_power_and_diesels() {
        let mut e = Electrical::shutdown();
        e.settle(&demand());
        e.step(&demand(), false, 1.0);
        e.grid_available = false;
        e.step(&demand(), false, 1.0);
        assert!(!e.buses.safety1 && !e.buses.bus_a);
        assert!(e.buses.dc, "battery carries DC");
        e.start_diesel(0);
        for _ in 0..12 {
            e.step(&demand(), false, 1.0);
        }
        e.set_breaker(Breaker::Edg1, true);
        e.step(&demand(), false, 1.0);
        assert!(e.buses.safety1 && !e.buses.bus_a);
        let f = e.diesels[0].fuel_l;
        e.step(&demand(), false, 100.0);
        assert!(e.diesels[0].fuel_l < f);
    }

    #[test]
    fn battery_runs_down_without_chargers() {
        let mut e = Electrical::shutdown();
        e.grid_available = false;
        for _ in 0..80 {
            e.step(&LoadDemand::default(), false, 100.0);
        }
        assert!(!e.buses.dc);
    }
}
