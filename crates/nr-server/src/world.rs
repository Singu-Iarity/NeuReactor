//! Server-side world: the plant plus host state (time scale, plugins, events).

use nr_api::{Origin, Scope};
use nr_reactors::{PlantEvent, Pwr};
use serde::Serialize;
use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{BuildHasher, Hasher};

#[derive(Debug, Clone, Serialize)]
pub struct PluginRecord {
    pub id: String,
    pub name: String,
    pub scopes: BTreeSet<Scope>,
    pub approved: bool,
    #[serde(skip)]
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoggedEvent {
    pub seq: u64,
    pub sim_time: f64,
    #[serde(flatten)]
    pub event: PlantEvent,
}

pub struct World {
    pub plant: Pwr,
    /// Active career, if any (sandbox otherwise).
    pub career: Option<nr_game::Career>,
    /// Grid dispatch and phones (sandbox and career).
    pub station: nr_game::Station,
    pub time_scale: f64,
    pub paused: bool,
    /// Auto time warp: run at `warp_factor` while the unit is steady on its
    /// dispatch target; drop back as soon as anything needs the operator.
    pub auto_warp: bool,
    pub warp_factor: f64,
    /// Currently warping (the time scale was raised by auto warp).
    pub warping: bool,
    /// Time scale to return to when the warp ends.
    pub warp_return: f64,
    /// Why auto warp is holding (empty while warping).
    pub warp_reason: String,
    /// Speed while ramping to a new commitment (load target set, output
    /// still moving).
    pub ramp_factor: f64,
    /// "hold", "ramp" or "warp".
    pub warp_stage: String,
    /// Grid request already brought to the operator's attention.
    pub warp_seen_request: Option<u32>,
    pub operator_token: String,
    pub plugins: BTreeMap<String, PluginRecord>,
    pub events: Vec<LoggedEvent>,
    next_seq: u64,
}

pub fn random_token() -> String {
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

impl World {
    pub fn new(scenario: &str) -> Self {
        Self {
            plant: Pwr::from_scenario(scenario).unwrap_or_else(Pwr::hot_standby),
            career: None,
            station: nr_game::Station::new(),
            time_scale: 1.0,
            paused: false,
            auto_warp: false,
            warp_factor: 1000.0,
            warping: false,
            warp_return: 1.0,
            warp_reason: String::new(),
            ramp_factor: 20.0,
            warp_stage: String::new(),
            warp_seen_request: None,
            operator_token: random_token(),
            plugins: BTreeMap::new(),
            events: Vec::new(),
            next_seq: 0,
        }
    }

    /// Resolve a bearer token to an origin.
    pub fn origin(&self, token: Option<&str>) -> Option<Origin> {
        let token = token?;
        if token == self.operator_token {
            return Some(Origin::Operator);
        }
        self.plugins
            .values()
            .find(|p| p.token == token && p.approved)
            .map(|p| Origin::Plugin {
                id: p.id.clone(),
                scopes: p.scopes.clone(),
            })
    }

    pub fn collect_events(&mut self) {
        let t = self.plant.time;
        for e in self.plant.events.drain(..) {
            self.next_seq += 1;
            self.events.push(LoggedEvent {
                seq: self.next_seq,
                sim_time: t,
                event: e,
            });
        }
        if self.events.len() > 500 {
            let cut = self.events.len() - 500;
            self.events.drain(..cut);
        }
    }

    pub fn log(&mut self, e: PlantEvent) {
        self.plant.events.push(e);
        self.collect_events();
    }
}
