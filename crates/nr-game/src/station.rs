//! Grid dispatch and plant communications (phones), used in sandbox and
//! career alike.
//!
//! * The grid dispatcher issues load requests while the unit is on-line,
//!   grants permission to synchronise, and scores how well net output follows
//!   the committed demand.
//! * The phone panel lets the operator call the turbine hall (roll checks),
//!   grid dispatch, maintenance and chemistry. Replies arrive after a
//!   realistic delay and change plant state (e.g. turbine hall ready).

use nr_api::{Alarm, Severity};
use nr_reactors::{PlantEvent, Pwr};
use serde::{Deserialize, Serialize};

/// Rated net output used for dispatch [MWe].
const RATED_NET: f64 = 1105.0;
const RAMP_MW_PER_MIN: f64 = 20.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadRequest {
    pub id: u32,
    pub target_mwe: f64,
    pub issued_at: f64,
    /// Time by which net output should be at the new target.
    pub deadline: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub seq: u64,
    pub sim_time: f64,
    pub from: String,
    pub text: String,
    /// Rings the phone in the UI.
    pub ring: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Call {
    TurbineRollChecks,
    SyncPermission,
    ReportOnline,
    ReportTrip,
    Maintenance,
    BoronSample,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingReply {
    at: f64,
    call: Call,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    /// Committed net demand [MWe] (0 = no commitment).
    pub demand_mwe: f64,
    pub pending: Option<LoadRequest>,
    /// Deadline of the last accepted request.
    pub deadline: Option<f64>,
    /// Automatic generation control: accept and follow requests automatically.
    pub agc: bool,
    pub on_target_s: f64,
    pub off_target_s: f64,
    pub deadline_missed: bool,
    pub boron_sample: Option<f64>,
    pub messages: Vec<Message>,
    replies: Vec<PendingReply>,
    next_request_at: f64,
    next_id: u32,
    seq: u64,
    seed: u64,
    was_online: bool,
    was_tripped: bool,
}

impl Default for Station {
    fn default() -> Self {
        Self::new()
    }
}

impl Station {
    pub fn new() -> Self {
        Self {
            demand_mwe: 0.0,
            pending: None,
            deadline: None,
            agc: false,
            on_target_s: 0.0,
            off_target_s: 0.0,
            deadline_missed: false,
            boron_sample: None,
            messages: Vec::new(),
            replies: Vec::new(),
            next_request_at: f64::INFINITY,
            next_id: 1,
            seq: 0,
            seed: 0x9E37_79B9_7F4A_7C15,
            was_online: false,
            was_tripped: false,
        }
    }

    fn rand(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    fn say(&mut self, t: f64, from: &str, text: String, ring: bool) {
        self.seq += 1;
        self.messages.push(Message {
            seq: self.seq,
            sim_time: t,
            from: from.into(),
            text,
            ring,
        });
        if self.messages.len() > 200 {
            self.messages.remove(0);
        }
    }

    fn issue(&mut self, t: f64, target: f64, why: &str) {
        let target = (target / 25.0).round() * 25.0;
        let from = if self.demand_mwe > 0.0 {
            self.demand_mwe
        } else {
            0.0
        };
        let minutes = (target - from).abs() / RAMP_MW_PER_MIN;
        let req = LoadRequest {
            id: self.next_id,
            target_mwe: target,
            issued_at: t,
            deadline: t + minutes * 60.0 + 1800.0,
        };
        self.next_id += 1;
        self.say(
            t,
            "Grid Dispatch",
            format!(
                "{why} Please bring the unit to {target:.0} MWe net within {:.0} minutes.",
                (req.deadline - t) / 60.0
            ),
            true,
        );
        self.pending = Some(req);
    }

    /// Operator (or AGC) accepts the pending load request.
    pub fn accept(&mut self, t: f64) -> Option<f64> {
        let req = self.pending.take()?;
        self.demand_mwe = req.target_mwe;
        self.deadline = Some(req.deadline);
        self.deadline_missed = false;
        self.say(
            t,
            "Control Room",
            format!("Acknowledged: {:.0} MWe.", req.target_mwe),
            false,
        );
        Some(req.target_mwe)
    }

    /// Place a phone call; the reply comes after a delay.
    pub fn call(&mut self, call: Call, t: f64) {
        let (to, text, delay) = match call {
            Call::TurbineRollChecks => (
                "Turbine Hall",
                "Requesting turbine roll checks: turning gear, drains, gland seals.",
                120.0 + 180.0 * self.rand(),
            ),
            Call::SyncPermission => (
                "Grid Dispatch",
                "Requesting permission to synchronise the unit.",
                45.0 + 60.0 * self.rand(),
            ),
            Call::ReportOnline => ("Grid Dispatch", "Reporting the unit on-line.", 20.0),
            Call::ReportTrip => ("Grid Dispatch", "Reporting a unit trip.", 20.0),
            Call::Maintenance => ("Maintenance", "Requesting a maintenance walkdown.", 300.0),
            Call::BoronSample => ("Chemistry", "Requesting an RCS boron sample.", 600.0),
        };
        self.say(t, "Control Room", format!("To {to}: {text}"), false);
        self.replies.push(PendingReply {
            at: t + delay,
            call,
        });
    }

    fn reply(&mut self, call: Call, plant: &mut Pwr) {
        let t = plant.time;
        match call {
            Call::TurbineRollChecks => {
                if plant.turbine_tripped || plant.tg.rpm() < 100.0 {
                    plant.turbine_hall_ready = true;
                    self.say(t, "Turbine Hall", "Roll checks complete: off turning gear, drains open, seals in. Ready to latch and roll.".into(), true);
                } else {
                    plant.turbine_hall_ready = true;
                    self.say(
                        t,
                        "Turbine Hall",
                        "Machine is already rolling; local checks done, all normal.".into(),
                        true,
                    );
                }
            }
            Call::SyncPermission => {
                if plant.tg.rpm() > 1700.0 && !plant.turbine_tripped {
                    plant.grid_permission = true;
                    if self.demand_mwe <= 0.0 {
                        self.next_request_at = t + 60.0;
                    }
                    self.say(
                        t,
                        "Grid Dispatch",
                        "Permission granted: you may synchronise. Call when on-line.".into(),
                        true,
                    );
                } else {
                    self.say(
                        t,
                        "Grid Dispatch",
                        "Call back when the machine is at synchronous speed.".into(),
                        true,
                    );
                }
            }
            Call::ReportOnline => self.say(
                t,
                "Grid Dispatch",
                "Copy, unit on-line. Stand by for load instructions.".into(),
                true,
            ),
            Call::ReportTrip => self.say(
                t,
                "Grid Dispatch",
                "Copy the trip. Report when you are ready to return.".into(),
                true,
            ),
            Call::Maintenance => self.say(
                t,
                "Maintenance",
                "Walkdown complete: no deficiencies found.".into(),
                true,
            ),
            Call::BoronSample => {
                let sample = plant.boron_ppm + (self.rand() - 0.5) * 6.0;
                self.boron_sample = Some(sample);
                self.say(
                    t,
                    "Chemistry",
                    format!("RCS boron sample: {sample:.0} ppm."),
                    true,
                );
            }
        }
    }

    /// Advance with the plant. Returns events for the plant log (e.g. an
    /// unpermitted synchronisation).
    pub fn tick(&mut self, plant: &mut Pwr, dt: f64) -> Vec<PlantEvent> {
        let mut events = Vec::new();
        let t = plant.time;
        // Phone replies.
        let due: Vec<Call> = self
            .replies
            .iter()
            .filter(|r| r.at <= t)
            .map(|r| r.call)
            .collect();
        self.replies.retain(|r| r.at > t);
        for c in due {
            self.reply(c, plant);
        }

        let online = plant.tg.breaker;
        if online && !self.was_online {
            if !plant.grid_permission {
                events.push(PlantEvent::warn(
                    "Unit synchronised WITHOUT grid permission",
                ));
                self.say(
                    t,
                    "Grid Dispatch",
                    "You synchronised without permission! This will be reported.".into(),
                    true,
                );
            }
            if self.demand_mwe <= 0.0 && self.pending.is_none() {
                self.next_request_at = t + 30.0;
            }
        }
        if !online && self.was_online {
            plant.grid_permission = false;
            self.say(
                t,
                "Grid Dispatch",
                "Unit off-line. Dispatch suspended.".into(),
                true,
            );
            self.demand_mwe = 0.0;
            self.pending = None;
            self.deadline = None;
            self.next_request_at = f64::INFINITY;
        }
        if plant.reactor_tripped && !self.was_tripped {
            plant.turbine_hall_ready = false;
        }
        self.was_online = online;
        self.was_tripped = plant.reactor_tripped;

        if online && self.pending.is_none() && t >= self.next_request_at {
            let target = if self.demand_mwe <= 0.0 {
                300.0
            } else {
                let mut tgt;
                loop {
                    tgt = (0.4 + 0.6 * self.rand()) * RATED_NET;
                    if (tgt - self.demand_mwe).abs() > 75.0 {
                        break;
                    }
                }
                tgt
            };
            let why = if self.demand_mwe <= 0.0 {
                "Welcome on-line."
            } else {
                "New dispatch instruction."
            };
            self.issue(t, target, why);
            self.next_request_at = t + 3600.0 * (2.0 + 4.0 * self.rand());
        }
        if self.agc && self.pending.is_some() {
            self.accept(t);
        }

        // Performance against demand.
        if online && self.demand_mwe > 0.0 {
            let net = plant.gross_mwe - plant.house_load();
            let past_deadline = self.deadline.map(|d| t > d).unwrap_or(true);
            if past_deadline {
                if (net - self.demand_mwe).abs() <= 0.03 * RATED_NET {
                    self.on_target_s += dt;
                } else {
                    self.off_target_s += dt;
                    if !self.deadline_missed && self.deadline.is_some() {
                        self.deadline_missed = true;
                        self.say(
                            t,
                            "Grid Dispatch",
                            format!(
                                "Deadline missed: we need {:.0} MWe from you.",
                                self.demand_mwe
                            ),
                            true,
                        );
                    }
                }
            }
        }
        events
    }

    /// Revenue multiplier for the current moment.
    pub fn revenue_multiplier(&self, plant: &Pwr) -> f64 {
        if self.demand_mwe <= 0.0 || !plant.tg.breaker {
            return 1.0;
        }
        let net = plant.gross_mwe - plant.house_load();
        if (net - self.demand_mwe).abs() <= 0.03 * RATED_NET {
            1.05
        } else if self.deadline.map(|d| plant.time > d).unwrap_or(true) {
            0.85
        } else {
            1.0
        }
    }

    pub fn alarms(&self, plant: &Pwr) -> Vec<Alarm> {
        let mut a = Vec::new();
        let mut add = |tag: &str, text: &str, sev: Severity| {
            a.push(Alarm {
                tag: tag.into(),
                panel: "grid".into(),
                text: text.into(),
                severity: sev,
            })
        };
        if self.pending.is_some() {
            add("grid_request", "GRID REQUEST PENDING", Severity::Warning);
        }
        if self.agc {
            add("agc", "AGC IN CONTROL", Severity::Info);
        }
        if plant.grid_permission {
            add("grid_perm_ok", "SYNC PERMISSION GRANTED", Severity::Info);
        }
        if plant.turbine_hall_ready {
            add("hall_ready", "TURBINE HALL READY", Severity::Info);
        }
        if !self.replies.is_empty() {
            add("call_waiting", "CALL IN PROGRESS", Severity::Info);
        }
        if plant.tg.breaker && self.demand_mwe > 0.0 {
            let net = plant.gross_mwe - plant.house_load();
            let past = self.deadline.map(|d| plant.time > d).unwrap_or(true);
            if past && (net - self.demand_mwe).abs() > 0.03 * RATED_NET {
                add("off_demand", "OUTPUT OFF DEMAND", Severity::Warning);
            }
        }
        if self.deadline_missed {
            add(
                "deadline_missed",
                "DISPATCH DEADLINE MISSED",
                Severity::Alarm,
            );
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(st: &mut Station, p: &mut Pwr, secs: f64) {
        let n = (secs / 1.0) as usize;
        for _ in 0..n {
            p.advance(1.0);
            st.tick(p, 1.0);
        }
    }

    #[test]
    fn turbine_hall_call_sets_ready_after_delay() {
        let mut p = Pwr::hot_standby();
        let mut st = Station::new();
        st.call(Call::TurbineRollChecks, p.time);
        run(&mut st, &mut p, 60.0);
        assert!(!p.turbine_hall_ready);
        run(&mut st, &mut p, 300.0);
        assert!(p.turbine_hall_ready);
        assert!(st
            .messages
            .iter()
            .any(|m| m.from == "Turbine Hall" && m.ring));
    }

    #[test]
    fn dispatch_issues_requests_when_online() {
        let mut p = Pwr::full_power();
        let mut st = Station::new();
        run(&mut st, &mut p, 5.0); // sees the unit already on-line
        st.was_online = false;
        run(&mut st, &mut p, 60.0);
        let req = st.pending.clone().expect("request issued");
        assert_eq!(req.target_mwe, 300.0);
        assert_eq!(st.accept(p.time), Some(300.0));
        assert!(st.pending.is_none());
        // AGC accepts automatically.
        st.agc = true;
        st.next_request_at = p.time;
        run(&mut st, &mut p, 2.0);
        assert!(st.pending.is_none());
        assert!(st.demand_mwe != 300.0);
    }

    #[test]
    fn sync_permission_only_at_speed() {
        let mut p = Pwr::hot_standby();
        let mut st = Station::new();
        st.call(Call::SyncPermission, p.time);
        run(&mut st, &mut p, 200.0);
        assert!(!p.grid_permission);
        assert!(st
            .messages
            .iter()
            .any(|m| m.text.contains("synchronous speed")));
    }
}
