// Annunciator sequence (ISA-18.1 style with ringback):
//   new alarm      -> fast flash + horn
//   acknowledged   -> steady on
//   cleared        -> slow flash (ringback) until RESET
// Info-severity windows are plain status lights (steady, no horn).

const state = new Map(); // tag -> { active, acked, ringback, severity, panel }
let hornEnabled = true;
let silenced = false;
let audio = null;
let hornNode = null;

function ensureAudio() {
  if (!audio) {
    try { audio = new (window.AudioContext || window.webkitAudioContext)(); } catch (_) { audio = null; }
  }
  return audio;
}
// Browsers only allow audio after a user gesture.
window.addEventListener("pointerdown", () => ensureAudio()?.resume?.(), { once: false });

function horn(on) {
  const ctx = audio;
  if (!ctx) return;
  if (on && !hornNode) {
    const osc = ctx.createOscillator();
    const lfo = ctx.createOscillator();
    const gain = ctx.createGain();
    const lfoGain = ctx.createGain();
    osc.type = "square"; osc.frequency.value = 520;
    lfo.type = "square"; lfo.frequency.value = 2.5;
    lfoGain.gain.value = 0.04; gain.gain.value = 0.04;
    lfo.connect(lfoGain).connect(gain.gain);
    osc.connect(gain).connect(ctx.destination);
    osc.start(); lfo.start();
    hornNode = { osc, lfo };
  } else if (!on && hornNode) {
    hornNode.osc.stop(); hornNode.lfo.stop();
    hornNode = null;
  }
}

export function setHornEnabled(v) { hornEnabled = v; if (!v) horn(false); }
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
  const anyUnacked = [...state.values()].some(s => !s.acked && s.active);
  horn(hornEnabled && !silenced && anyUnacked);
}

export function acknowledge(panel) {
  for (const st of state.values()) if (!panel || st.panel === panel) st.acked = true;
  ingest([...state.entries()].filter(([, s]) => s.active).map(([tag, s]) => ({ tag, severity: s.severity, panel: s.panel, text: s.text })));
}

export function silence() { silenced = true; horn(false); }

export function reset(panel) {
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
