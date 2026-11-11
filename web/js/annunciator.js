// Annunciator sequence (ISA-18.1 style with ringback):
//   new alarm      -> fast flash + horn
//   acknowledged   -> steady on
//   cleared        -> slow flash (ringback) until RESET
// Info-severity windows are plain status lights (steady, no horn).

import * as sound from "./sound.js";

const state = new Map(); // tag -> { active, acked, ringback, severity, panel }
let hornEnabled = true;
let silenced = false;

const RANK = { warning: 1, alarm: 2, trip: 3 };
function updateSignal() {
  let worst = null;
  for (const st of state.values()) {
    if (st.acked || !st.active || !RANK[st.severity]) continue;
    if (!worst || RANK[st.severity] > RANK[worst]) worst = st.severity;
  }
  sound.alarmSignal(hornEnabled && !silenced ? worst : null);
}

export function setHornEnabled(v) { hornEnabled = v; updateSignal(); }
export function hornIsEnabled() { return hornEnabled; }

/** Feed the current active alarm list from telemetry. */
export function ingest(alarms) {
  const now = new Set();
  for (const a of alarms) {
    now.add(a.tag);
    const st = state.get(a.tag);
    if (!st) {
      const info = a.severity === "info";
      state.set(a.tag, { active: true, acked: info, ringback: false, severity: a.severity, panel: a.panel, text: a.text });
      if (!info) silenced = false;
    } else {
      if (!st.active) {
        // Returned before reset: alarm again.
        st.acked = a.severity === "info";
        st.ringback = false;
        if (!st.acked) silenced = false;
      }
      st.active = true;
      st.severity = a.severity;
      st.text = a.text;
    }
  }
  for (const [tag, st] of state) {
    if (!now.has(tag) && st.active) {
      st.active = false;
      if (st.severity === "info") state.delete(tag);
      else st.ringback = true;
    }
  }
  updateSignal();
}

export function acknowledge(panel) {
  sound.play("ack");
  for (const st of state.values()) if (!panel || st.panel === panel) st.acked = true;
  ingest([...state.entries()].filter(([, s]) => s.active).map(([tag, s]) => ({ tag, severity: s.severity, panel: s.panel, text: s.text })));
}

export function silence() { sound.play("button"); silenced = true; updateSignal(); }

export function reset(panel) {
  sound.play("button");
  for (const [tag, st] of state) {
    if ((!panel || st.panel === panel) && !st.active && st.acked) state.delete(tag);
  }
}

/** CSS class for one window. */
export function windowClass(tag) {
  const st = state.get(tag);
  if (!st) return "";
  const sev = st.severity;
  if (!st.active) return st.acked ? `${sev} ringback` : `${sev} fast`;
  return st.acked ? `${sev} on` : `${sev} fast`;
}

/** Highest-priority unacknowledged state for a panel/tab. */
export function panelState(panel) {
  let out = "";
  for (const st of state.values()) {
    if (st.panel !== panel || st.severity === "info") continue;
    if (!st.acked) return "unacked";
    if (st.active) out = "active";
  }
  return out;
}

export function textOf(tag) { return state.get(tag)?.text; }
