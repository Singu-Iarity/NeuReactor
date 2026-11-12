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

/// Rated net output of the unit, used for dispatch [MWe].
fn rated_net(plant: &Pwr) -> f64 {
    // What the unit can actually deliver at 100 % reactor power with its
    // installed equipment.
    plant.max_gross_mwe() - plant.p.house_load_mwe
}
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
    /// Sender's plant location (shown on the messaging terminal).
    #[serde(default)]
    pub location: String,
    /// Acknowledged on the messaging terminal.
    #[serde(default)]
    pub acked: bool,
}

/// Plant location of each message originator.
pub fn location_of(from: &str) -> &'static str {
    match from {
        "Turbine Hall" => "TURB BLDG EL 30",
        "Grid Dispatch" => "SYSTEM DISPATCH",
        "Maintenance" => "MAINT SHOP",
        "Chemistry" => "CHEM LAB",
        "Plant Operator" => "AUX BLDG",
        _ => "MAIN CONTROL RM",
    }
}

/// What a free-text message from the control room asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    Call(Call),
    /// Accept the pending grid load request.
    AcceptLoad,
    /// Ask grid dispatch to lower (`raise` = false) or raise the committed
    /// output, to `mw` (or by `mw` when `relative`).
    LoadChange {
        raise: bool,
        mw: Option<f64>,
        relative: bool,
    },
}

impl Intent {
    /// The station this request is for.
    pub fn party(self) -> &'static str {
        match self {
            Intent::Call(Call::TurbineRollChecks) => "Turbine Hall",
            Intent::Call(Call::Maintenance) => "Maintenance",
            Intent::Call(Call::BoronSample) => "Chemistry",
            _ => "Grid Dispatch",
        }
    }
}

/// Stations the control room can message.
pub const STATIONS: [&str; 5] = [
    "Grid Dispatch",
    "Turbine Hall",
    "Maintenance",
    "Chemistry",
    "Plant Operator",
];

/// First number in a message (e.g. "900" in "lower to 900 MW").
fn number_in(t: &str) -> Option<f64> {
    let mut num = String::new();
    for c in t.chars() {
        if c.is_ascii_digit() || (c == '.' && !num.is_empty()) {
            num.push(c);
        } else if !num.is_empty() {
            break;
        }
    }
    num.parse().ok()
}

/// Interpret an operator's typed message ("calling for turbine runup",
/// "request permission to sync", "need a boron sample" ...).
pub fn parse_request(text: &str) -> Option<Intent> {
    let t = text.to_lowercase();
    let any = |ws: &[&str]| ws.iter().any(|w| t.contains(w));
    // Derate / offer more: a change word plus a load word or a MW figure.
    let loadish = any(&[
        "load",
        "output",
        "demand",
        "generation",
        "mw",
        "megawatt",
        "power",
    ]);
    let lower = any(&[
        "lower",
        "reduce",
        "decrease",
        "derate",
        "cut",
        "back down",
        "drop",
        "less",
    ]);
    let raise = any(&[
        "raise",
        "increase",
        "uprate",
        "more",
        "extra",
        "additional",
        "higher",
        "pick up",
    ]);
    if loadish && (lower ^ raise) && !any(&["boron", "chemistry"]) {
        return Some(Intent::LoadChange {
            raise,
            mw: number_in(&t),
            relative: any(&[" by "]),
        });
    }
    if any(&["boron", "sample", "chemistry", "chem lab", "ppm"]) {
        return Some(Intent::Call(Call::BoronSample));
    }
    if any(&[
        "sync",
        "permission",
        "parallel",
        "tie in",
        "tie to",
        "connect to the grid",
        "connect to grid",
        "close the breaker",
        "close breaker",
    ]) {
        return Some(Intent::Call(Call::SyncPermission));
    }
    if any(&["trip", "scram", "tripped"]) {
        return Some(Intent::Call(Call::ReportTrip));
    }
    if any(&[
        "online",
        "on-line",
        "on line",
        "on the grid",
        "synchronized",
        "synchronised",
    ]) {
        return Some(Intent::Call(Call::ReportOnline));
    }
    if any(&[
        "roll",
        "run up",
        "runup",
        "run-up",
        "turning gear",
        "latch",
        "warm up",
        "warmup",
        "gland",
        "drains",
    ]) || (t.contains("turbine") && any(&["check", "ready", "start", "prep"]))
    {
        return Some(Intent::Call(Call::TurbineRollChecks));
    }
    if any(&[
        "maint",
        "walkdown",
        "walk down",
        "repair",
        "work order",
        "inspect",
        "electrician",
        "mechanic",
        "leak",
    ]) {
        return Some(Intent::Call(Call::Maintenance));
    }
    if any(&[
        "accept",
        "acknowledge",
        "copy the load",
        "roger",
        "will comply",
        "wilco",
        "load request",
        "going to",
    ]) {
        return Some(Intent::AcceptLoad);
    }
    None
}

/// Energy and money totals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    /// Generated energy [MWh].
    pub gross_mwh: f64,
    /// Energy sold to the grid [MWh].
    pub net_mwh: f64,
    /// Station service energy used [MWh].
    pub house_mwh: f64,
    /// Energy bought from the grid while off-line [MWh].
    pub import_mwh: f64,
    pub revenue: f64,
    /// Running costs (fuel, water, imported power, maintenance, staff).
    pub costs: f64,
    /// Grid fines and penalties.
    pub penalties: f64,
    /// Seconds of operation counted.
    pub seconds: f64,
}

impl Ledger {
    pub fn profit(&self) -> f64 {
        self.revenue - self.costs - self.penalties
    }
}

/// Results of one dispatch instruction (from acceptance until the next).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionLedger {
    pub id: u32,
    pub target_mwe: f64,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub on_target_s: f64,
    #[serde(flatten)]
    pub ledger: Ledger,
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
    /// Internal: message not understood (terminal free text).
    SayAgain,
    /// Internal: load acceptance with no request outstanding.
    NoRequest,
    /// Ask grid dispatch to change the committed output (derate / offer more).
    LoadChange,
    /// Internal: a message sent to the wrong station.
    Misrouted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingReply {
    at: f64,
    call: Call,
    /// Who answers (misrouted messages).
    #[serde(default)]
    who: Option<String>,
    /// Who the message was really for (misrouted messages).
    #[serde(default)]
    meant_for: Option<String>,
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
    #[serde(with = "nr_core::serde_f64")]
    next_request_at: f64,
    next_id: u32,
    seq: u64,
    seed: u64,
    was_online: bool,
    was_tripped: bool,
    /// Reminder sent for the pending (unacknowledged) instruction.
    #[serde(default)]
    reminded: bool,
    /// Net output has reached the committed demand since it was set.
    #[serde(default)]
    pub reached: bool,
    #[serde(default)]
    drift_s: f64,
    #[serde(default)]
    drift_warned: bool,
    /// Fines incurred (amount, reason); drained by the host each tick and
    /// charged in career mode.
    #[serde(skip)]
    pub fines: Vec<(f64, String)>,
    /// All-time energy and money (since the session or career started).
    #[serde(default)]
    pub totals: Ledger,
    /// The dispatch instruction being followed now.
    #[serde(default)]
    pub instruction: Option<InstructionLedger>,
    /// Finished instructions, newest last.
    #[serde(default)]
    pub history: Vec<InstructionLedger>,
    /// Requested new commitment awaiting dispatch's answer.
    #[serde(default)]
    pub change_target: Option<f64>,
    /// Credits (capacity premiums) to pay out: drained by the host.
    #[serde(skip)]
    pub credits: Vec<(f64, String)>,
    /// Power price last used for accounting [$/MWh].
    #[serde(default = "default_price")]
    pub last_price: f64,
}

fn default_price() -> f64 {
    45.0
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
            reminded: false,
            reached: false,
            drift_s: 0.0,
            drift_warned: false,
            fines: Vec::new(),
            totals: Ledger::default(),
            instruction: None,
            history: Vec::new(),
            change_target: None,
            credits: Vec::new(),
            last_price: 45.0,
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
            location: location_of(from).into(),
            acked: from == "Control Room",
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
        self.reminded = false;
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

    fn commit(&mut self, req: &LoadRequest, t: f64) {
        self.close_instruction(t);
        self.instruction = Some(InstructionLedger {
            id: req.id,
            target_mwe: req.target_mwe,
            started_at: t,
            ended_at: None,
            on_target_s: 0.0,
            ledger: Ledger::default(),
        });
        self.demand_mwe = req.target_mwe;
        self.deadline = Some(req.deadline);
        self.deadline_missed = false;
        self.reached = false;
        self.drift_s = 0.0;
        self.drift_warned = false;
    }

    fn close_instruction(&mut self, t: f64) {
        if let Some(mut done) = self.instruction.take() {
            done.ended_at = Some(t);
            self.history.push(done);
            if self.history.len() > 50 {
                self.history.remove(0);
            }
        }
    }

    fn fine_pair(&mut self, (amount, why): (f64, String)) {
        self.fine(amount, why);
    }

    fn fine(&mut self, amount: f64, why: String) {
        self.totals.penalties += amount;
        if let Some(i) = &mut self.instruction {
            i.ledger.penalties += amount;
        }
        self.fines.push((amount, why));
    }

    /// Book energy, revenue and running costs for `dt` seconds at the
    /// given power price.
    pub fn account(&mut self, plant: &Pwr, price_mwh: f64, om_per_day: f64, dt: f64) {
        self.last_price = price_mwh;
        let h = dt / 3600.0;
        let online = plant.tg.breaker;
        let net = plant.gross_mwe - plant.house_load();
        let revenue = if online && net > 0.0 {
            net * price_mwh * self.revenue_multiplier(plant) * h
        } else {
            0.0
        };
        let costs = crate::hourly_costs(plant, price_mwh, om_per_day).payable() * h;
        let on_target = online
            && self.demand_mwe > 0.0
            && (net - self.demand_mwe).abs() <= 0.03 * rated_net(plant);
        let book = |l: &mut Ledger| {
            l.gross_mwh += plant.gross_mwe.max(0.0) * h;
            l.net_mwh += if online { net.max(0.0) * h } else { 0.0 };
            l.house_mwh += plant.house_load() * h;
            l.import_mwh += plant.elec.import_mw * h;
            l.revenue += revenue;
            l.costs += costs;
            l.seconds += dt;
        };
        book(&mut self.totals);
        if let Some(i) = &mut self.instruction {
            book(&mut i.ledger);
            if on_target {
                i.on_target_s += dt;
            }
        }
    }

    /// Operator (or AGC) accepts the pending load request.
    pub fn accept(&mut self, t: f64) -> Option<f64> {
        let req = self.pending.take()?;
        self.commit(&req, t);
        self.say(
            t,
            "Control Room",
            format!("Acknowledged: {:.0} MWe.", req.target_mwe),
            false,
        );
        Some(req.target_mwe)
    }

    /// Acknowledge one message (or all with `None`) on the terminal.
    pub fn ack(&mut self, seq: Option<u64>) {
        for m in &mut self.messages {
            if seq.is_none_or(|s| s == m.seq) {
                m.acked = true;
            }
        }
    }

    /// Unacknowledged incoming messages.
    pub fn unacked(&self) -> usize {
        self.messages.iter().filter(|m| !m.acked).count()
    }

    /// A free-text message typed on the messaging terminal. Returns the
    /// understood intent (the caller performs `AcceptLoad`).
    pub fn typed(&mut self, text: &str, to: Option<&str>, t: f64) -> Option<Intent> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let to = to.filter(|d| STATIONS.contains(d));
        match to {
            Some(d) => self.say(t, "Control Room", format!("To {d}: {text}"), false),
            None => self.say(t, "Control Room", text.to_string(), false),
        }
        let intent = parse_request(text);
        // Sent to the wrong station: they answer, puzzled.
        if let (Some(d), Some(i)) = (to, intent) {
            if d != i.party() {
                let at = t + 15.0 + 15.0 * self.rand();
                self.replies.push(PendingReply {
                    at,
                    call: Call::Misrouted,
                    who: Some(d.to_string()),
                    meant_for: Some(i.party().to_string()),
                });
                return None;
            }
        }
        match intent {
            Some(Intent::LoadChange {
                raise,
                mw,
                relative,
            }) => {
                let step = mw.unwrap_or(100.0);
                let target = if relative || mw.is_none() {
                    if raise {
                        self.demand_mwe + step
                    } else {
                        self.demand_mwe - step
                    }
                } else {
                    step
                };
                self.request_load_change(target, t);
                return intent;
            }
            Some(Intent::Call(c)) => self.call(c, t),
            Some(Intent::AcceptLoad) if self.pending.is_none() => {
                self.replies.push(PendingReply {
                    at: t + 15.0,
                    call: Call::NoRequest,
                    who: None,
                    meant_for: None,
                });
                return None;
            }
            Some(Intent::AcceptLoad) => {}
            None => {
                let at = t + 10.0 + 10.0 * self.rand();
                self.replies.push(PendingReply {
                    at,
                    call: Call::SayAgain,
                    who: to.map(str::to_string),
                    meant_for: None,
                });
            }
        }
        intent
    }

    /// Ask grid dispatch to change the committed output to `target` MWe
    /// net (derate, or offer more). The answer comes after a short delay.
    pub fn request_load_change(&mut self, target: f64, t: f64) {
        self.change_target = Some(target.max(0.0));
        let verb = if target < self.demand_mwe {
            "reduce"
        } else {
            "raise"
        };
        self.say(
            t,
            "Control Room",
            format!("To Grid Dispatch: Requesting to {verb} the unit's committed output to {target:.0} MWe net."),
            false,
        );
        let at = t + 60.0 + 90.0 * self.rand();
        self.replies.push(PendingReply {
            at,
            call: Call::LoadChange,
            who: None,
            meant_for: None,
        });
    }

    fn credit(&mut self, amount: f64, why: String) {
        self.totals.revenue += amount;
        if let Some(i) = &mut self.instruction {
            i.ledger.revenue += amount;
        }
        self.credits.push((amount, why));
    }

    /// Grid dispatch answers a load change request.
    fn answer_load_change(&mut self, plant: &Pwr) {
        let t = plant.time;
        let Some(want) = self.change_target.take() else {
            return;
        };
        let online = plant.tg.breaker;
        if !online || self.demand_mwe <= 0.0 {
            self.say(t, "Grid Dispatch", "MCR, you have no committed output to change. Call when you are on-line and following an instruction.".into(), true);
            return;
        }
        let max = rated_net(plant).max(0.0);
        let cur = self.demand_mwe;
        let price = self.last_price;
        let mins = |d: f64| d.abs() / RAMP_MW_PER_MIN + 30.0;
        if want < cur - 1.0 {
            let target = want.max(0.2 * max);
            if self.rand() < 0.85 {
                let d = cur - target;
                let charge = d * price;
                self.set_commitment(target, t + mins(d) * 60.0, t);
                self.say(
                    t,
                    "Grid Dispatch",
                    format!("Approved: reduce to {target:.0} MWe net within {:.0} minutes. Replacement power for the {d:.0} MWe shortfall is charged at ${:.0}k.", mins(d), charge / 1e3),
                    true,
                );
                self.fine(charge, format!("replacement power for a {d:.0} MWe derate"));
            } else {
                self.say(
                    t,
                    "Grid Dispatch",
                    format!("Denied, MCR: the system is short right now. Hold {cur:.0} MWe."),
                    true,
                );
            }
        } else if want > cur + 1.0 {
            if cur >= max - 1.0 {
                self.say(
                    t,
                    "Grid Dispatch",
                    format!("MCR, your unit is rated {max:.0} MWe net: you are already there."),
                    true,
                );
                return;
            }
            let target = want.min(max);
            if self.rand() < 0.6 {
                let d = target - cur;
                let bonus = d * price * 0.5;
                self.set_commitment(target, t + mins(d) * 60.0, t);
                self.say(
                    t,
                    "Grid Dispatch",
                    format!("Thanks MCR, we'll take it: raise to {target:.0} MWe net within {:.0} minutes. Capacity premium of ${:.0}k on the extra output.", mins(d), bonus / 1e3),
                    true,
                );
                self.credit(bonus, format!("capacity premium for {d:.0} MWe extra"));
            } else {
                self.say(
                    t,
                    "Grid Dispatch",
                    format!("No thanks MCR, the system is long. Hold {cur:.0} MWe."),
                    true,
                );
            }
        } else {
            self.say(
                t,
                "Grid Dispatch",
                format!("Copy, no change: hold {cur:.0} MWe."),
                true,
            );
        }
    }

    /// Change the committed output (a negotiated change counts as a new
    /// instruction for the ledger).
    fn set_commitment(&mut self, target: f64, deadline: f64, t: f64) {
        let req = LoadRequest {
            id: self.next_id,
            target_mwe: target,
            issued_at: t,
            deadline,
        };
        self.next_id += 1;
        self.commit(&req, t);
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
            Call::SayAgain | Call::NoRequest | Call::LoadChange | Call::Misrouted => return,
        };
        self.say(t, "Control Room", format!("To {to}: {text}"), false);
        self.replies.push(PendingReply {
            at: t + delay,
            call,
            who: None,
            meant_for: None,
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
            Call::SayAgain => self.say(
                t,
                "Plant Operator",
                "MCR, please repeat your last message, not understood.".into(),
                true,
            ),
            Call::LoadChange => self.answer_load_change(plant),
            Call::Misrouted => {}
            Call::NoRequest => self.say(
                t,
                "Grid Dispatch",
                "MCR, we have no load instruction outstanding.".into(),
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
        let due: Vec<PendingReply> = self.replies.iter().filter(|r| r.at <= t).cloned().collect();
        self.replies.retain(|r| r.at > t);
        for r in due {
            match (r.call, &r.who) {
                (Call::Misrouted, Some(who)) => {
                    let meant = r.meant_for.as_deref().unwrap_or("someone else");
                    let line = match who.as_str() {
                        "Chemistry" => format!("Chemistry lab here. MCR, that's not one for us. Try {meant}."),
                        "Maintenance" => format!("Maintenance shop. Uh, MCR, we can't help with that. Sounds like {meant}."),
                        "Turbine Hall" => format!("Turbine hall. MCR, say again? That's a call for {meant}, not the turbine floor."),
                        _ => format!("{who}: MCR, I think you want {meant} for that."),
                    };
                    self.say(t, who, line, true);
                }
                (Call::SayAgain, Some(who)) => self.say(
                    t,
                    who,
                    "MCR, please repeat your last message, not understood.".into(),
                    true,
                ),
                _ => self.reply(r.call, plant),
            }
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
            self.reached = false;
            self.close_instruction(t);
            self.next_request_at = f64::INFINITY;
        }
        if plant.reactor_tripped && !self.was_tripped {
            plant.turbine_hall_ready = false;
        }
        self.was_online = online;
        self.was_tripped = plant.reactor_tripped;

        if online && self.pending.is_none() && t >= self.next_request_at {
            let target = if self.demand_mwe <= 0.0 {
                // First instruction: pick up load, or hold what the unit
                // already produces.
                (plant.gross_mwe - plant.house_load())
                    .max(300.0)
                    .min(rated_net(plant))
            } else {
                let mut tgt;
                loop {
                    tgt = (0.4 + 0.6 * self.rand()) * rated_net(plant);
                    if (tgt - self.demand_mwe).abs() > 0.07 * rated_net(plant) {
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
        // Unacknowledged instruction: remind, then it becomes binding anyway.
        if let Some(req) = self.pending.clone() {
            if !self.reminded && t > req.issued_at + 300.0 {
                self.reminded = true;
                self.say(
                    t,
                    "Grid Dispatch",
                    format!(
                        "MCR, Grid Dispatch: please acknowledge the instruction for {:.0} MWe.",
                        req.target_mwe
                    ),
                    true,
                );
            }
            if t > req.deadline {
                self.pending = None;
                self.commit(&req, t);
                self.deadline_missed = true;
                self.say(
                    t,
                    "Grid Dispatch",
                    format!(
                        "Instruction for {:.0} MWe was never acknowledged and the deadline has passed. Logged as non-compliance; we still need {:.0} MWe.",
                        req.target_mwe, req.target_mwe
                    ),
                    true,
                );
                self.fine_pair((
                    150_000.0,
                    format!(
                        "dispatch instruction ({:.0} MWe) not acknowledged",
                        req.target_mwe
                    ),
                ));
            }
        }

        // Performance against demand.
        if online && self.demand_mwe > 0.0 {
            let net = plant.gross_mwe - plant.house_load();
            let past_deadline = self.deadline.map(|d| t > d).unwrap_or(true);
            let on_target = (net - self.demand_mwe).abs() <= 0.03 * rated_net(plant);
            if on_target {
                if !self.reached {
                    self.reached = true;
                    let text = if past_deadline {
                        format!(
                            "Unit now at {:.0} MWe, noted late. Hold this output, thank you.",
                            self.demand_mwe
                        )
                    } else {
                        format!(
                            "Thank you MCR, unit at {:.0} MWe on schedule. Hold this output.",
                            self.demand_mwe
                        )
                    };
                    self.say(t, "Grid Dispatch", text, true);
                }
                self.drift_s = 0.0;
                self.drift_warned = false;
            }
            if past_deadline {
                if on_target {
                    self.on_target_s += dt;
                } else {
                    self.off_target_s += dt;
                    if !self.reached && !self.deadline_missed {
                        self.deadline_missed = true;
                        let short = (self.demand_mwe - net).abs();
                        self.say(
                            t,
                            "Grid Dispatch",
                            format!(
                                "Deadline missed: we need {:.0} MWe from you, you are at {:.0} MWe.",
                                self.demand_mwe,
                                net.max(0.0)
                            ),
                            true,
                        );
                        self.fine_pair((
                            50_000.0 + 500.0 * short,
                            format!("missed dispatch deadline ({:.0} MWe off target)", short),
                        ));
                    }
                }
            }
            if self.reached && !on_target {
                self.drift_s += dt;
                if self.drift_s > 600.0 && !self.drift_warned {
                    self.drift_warned = true;
                    self.say(
                        t,
                        "Grid Dispatch",
                        format!(
                            "MCR, you have drifted off your dispatch target of {:.0} MWe ({:.0} MWe now). Please correct.",
                            self.demand_mwe,
                            net.max(0.0)
                        ),
                        true,
                    );
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
        if (net - self.demand_mwe).abs() <= 0.03 * rated_net(plant) {
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
            if past && (net - self.demand_mwe).abs() > 0.03 * rated_net(plant) {
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
        // First instruction holds the unit's current output (at least 300 MWe).
        let net = p.gross_mwe - p.house_load();
        assert!(
            (req.target_mwe - net).abs() <= 12.5,
            "{} vs {net}",
            req.target_mwe
        );
        let first = req.target_mwe;
        assert_eq!(st.accept(p.time), Some(first));
        assert!(st.pending.is_none());
        // AGC accepts automatically.
        st.agc = true;
        st.next_request_at = p.time;
        run(&mut st, &mut p, 2.0);
        assert!(st.pending.is_none());
        assert!(st.demand_mwe != first);
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

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn understands_typical_requests() {
        let c = |s: &str| parse_request(s);
        assert_eq!(
            c("calling for turbine runup"),
            Some(Intent::Call(Call::TurbineRollChecks))
        );
        assert_eq!(
            c("Turbine hall, ready the machine for roll"),
            Some(Intent::Call(Call::TurbineRollChecks))
        );
        assert_eq!(
            c("request permission to sync to the grid"),
            Some(Intent::Call(Call::SyncPermission))
        );
        assert_eq!(
            c("unit 1 is online"),
            Some(Intent::Call(Call::ReportOnline))
        );
        assert_eq!(
            c("we just had a reactor trip"),
            Some(Intent::Call(Call::ReportTrip))
        );
        assert_eq!(
            c("need an RCS boron sample please"),
            Some(Intent::Call(Call::BoronSample))
        );
        assert_eq!(
            c("send maintenance for a walkdown"),
            Some(Intent::Call(Call::Maintenance))
        );
        assert_eq!(c("roger, accept the new load"), Some(Intent::AcceptLoad));
        assert_eq!(c("what is the weather like"), None);
    }

    #[test]
    fn unclear_message_gets_say_again() {
        let mut s = Station::new();
        assert!(s.typed("blah blah", None, 0.0).is_none());
        assert_eq!(s.replies.len(), 1);
        assert_eq!(s.replies[0].call, Call::SayAgain);
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn ignored_request_becomes_binding_and_is_fined() {
        let mut plant = Pwr::full_power();
        let mut st = Station::new();
        st.tick(&mut plant, 0.1); // sees the unit on-line
        st.next_request_at = plant.time;
        st.tick(&mut plant, 0.1);
        let req = st.pending.clone().expect("request issued");
        plant.time = req.issued_at + 301.0;
        st.tick(&mut plant, 0.1);
        assert!(st
            .messages
            .iter()
            .any(|m| m.text.contains("please acknowledge")));
        plant.time = req.deadline + 1.0;
        st.tick(&mut plant, 0.1);
        assert!(st.pending.is_none());
        assert_eq!(st.demand_mwe, req.target_mwe);
        assert!(!st.fines.is_empty());
    }

    #[test]
    fn reaching_target_gets_thanks() {
        let mut plant = Pwr::full_power();
        let mut st = Station::new();
        st.tick(&mut plant, 0.1);
        let net = plant.gross_mwe - plant.house_load();
        st.issue(plant.time, net, "Test.");
        st.accept(plant.time);
        st.tick(&mut plant, 0.1);
        assert!(st.reached);
        assert!(st
            .messages
            .iter()
            .any(|m| m.text.contains("Hold this output")));
        assert!(st.fines.is_empty());
    }

    #[test]
    fn ledger_books_energy_and_money_per_instruction() {
        let mut plant = Pwr::full_power();
        let mut st = Station::new();
        st.tick(&mut plant, 0.1);
        let net = plant.gross_mwe - plant.house_load();
        st.issue(plant.time, net, "Test.");
        st.accept(plant.time);
        st.tick(&mut plant, 0.1);
        st.account(&plant, 45.0, 120_000.0, 3600.0);
        assert!((st.totals.net_mwh - net).abs() < 1.0);
        assert!((st.totals.revenue - net * 45.0 * 1.05).abs() < 100.0);
        assert!(st.totals.costs > 5_000.0);
        let i = st.instruction.as_ref().unwrap();
        assert!((i.ledger.revenue - st.totals.revenue).abs() < 1e-6);
        assert!(i.on_target_s > 3000.0);
        // A new instruction closes the old one.
        st.issue(plant.time, 600.0, "Test.");
        st.accept(plant.time);
        assert_eq!(st.history.len(), 1);
        assert_eq!(st.instruction.as_ref().unwrap().ledger.revenue, 0.0);
    }

    #[test]
    fn misrouted_message_gets_a_confused_reply() {
        let mut plant = Pwr::full_power();
        let mut st = Station::new();
        assert!(st
            .typed("calling for turbine runup", Some("Chemistry"), plant.time)
            .is_none());
        plant.time += 60.0;
        st.tick(&mut plant, 60.0);
        let m = st
            .messages
            .iter()
            .rev()
            .find(|m| m.from == "Chemistry")
            .unwrap();
        assert!(m.text.contains("Turbine Hall"), "{}", m.text);
        assert!(
            !st.messages.iter().any(|m| m.from == "Turbine Hall"),
            "the turbine hall never got the request"
        );
        assert!(st.replies.is_empty());
    }

    #[test]
    fn parses_load_change_requests() {
        let p = |s: &str| parse_request(s);
        assert_eq!(
            p("can we lower load to 900 MW"),
            Some(Intent::LoadChange {
                raise: false,
                mw: Some(900.0),
                relative: false
            })
        );
        assert_eq!(
            p("request to raise output by 50 MW"),
            Some(Intent::LoadChange {
                raise: true,
                mw: Some(50.0),
                relative: true
            })
        );
        assert_eq!(
            p("calling for turbine runup"),
            Some(Intent::Call(Call::TurbineRollChecks))
        );
    }

    #[test]
    fn dispatch_answers_derate_and_offer() {
        let mut plant = Pwr::full_power();
        let mut st = Station::new();
        st.tick(&mut plant, 0.1);
        st.issue(plant.time, 900.0, "Test.");
        st.accept(plant.time);
        // Derate: approved most of the time, with a replacement power charge.
        let mut approved = 0;
        for _ in 0..10 {
            st.demand_mwe = 900.0;
            st.fines.clear();
            st.request_load_change(800.0, plant.time);
            plant.time += 200.0;
            st.tick(&mut plant, 200.0);
            if st.demand_mwe == 800.0 {
                approved += 1;
                assert!((st.fines.last().unwrap().0 - 100.0 * 45.0).abs() < 1.0);
            } else {
                assert!(st.fines.is_empty());
            }
        }
        assert!(approved >= 5, "derates approved {approved}/10");
        // Offer more: capped at the unit's rating, premium paid when taken.
        let mut taken = 0;
        for _ in 0..10 {
            st.demand_mwe = 800.0;
            st.credits.clear();
            st.request_load_change(5000.0, plant.time);
            plant.time += 200.0;
            st.tick(&mut plant, 200.0);
            if st.demand_mwe > 800.0 {
                taken += 1;
                assert!(st.demand_mwe <= rated_net(&plant) + 1e-6);
                assert!(!st.credits.is_empty());
            }
        }
        assert!(taken >= 2, "offers taken {taken}/10");
    }
}
