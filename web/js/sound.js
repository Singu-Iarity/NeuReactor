// Control room sound engine. Every sound is synthesised with WebAudio, so
// nothing has to be downloaded.
//
// * One-shot effects: switch clunks, knob detents, pushbuttons, breaker
//   operations, rod drive steps, scram, valve slams, alarm chimes.
// * Alarm signals: annunciator horn (alarm), klaxon (trip), chime (warning).
// * Ambience driven by plant state: control room HVAC, reactor coolant pump
//   hum, turbine-generator whine (pitch follows rpm), steam dump / relief
//   roar, and the source range count-rate speaker.

let ctx = null;
let master = null, sfxBus = null, ambBus = null, alarmBus = null, callBus = null;
// Bus one-shot effects play on (call sounds use their own).
let activeBus = null;
let volume = 0.7, muted = false;
const prefs = { ambience: true, alarms: true, effects: true, calls: true };

function load() {
  try {
    const v = JSON.parse(localStorage.getItem("nr-sound") || "{}");
    if (typeof v.volume === "number") volume = v.volume;
    if (typeof v.muted === "boolean") muted = v.muted;
    Object.assign(prefs, v.prefs || {});
  } catch (_) { /* storage unavailable */ }
}
function save() {
  try { localStorage.setItem("nr-sound", JSON.stringify({ volume, muted, prefs })); } catch (_) { /* ignore */ }
}
load();

/** Create the audio graph (needs a user gesture first). */
export function unlock() {
  if (!ctx) {
    try { ctx = new (window.AudioContext || window.webkitAudioContext)(); } catch (_) { return null; }
    master = ctx.createGain();
    master.connect(ctx.destination);
    sfxBus = ctx.createGain(); sfxBus.connect(master);
    ambBus = ctx.createGain(); ambBus.connect(master);
    alarmBus = ctx.createGain(); alarmBus.connect(master);
    callBus = ctx.createGain(); callBus.connect(master);
    applyLevels();
  }
  if (ctx.state === "suspended") ctx.resume();
  return ctx;
}
window.addEventListener("pointerdown", unlock);
window.addEventListener("keydown", unlock);

function applyLevels() {
  if (!ctx) return;
  const t = ctx.currentTime;
  master.gain.setTargetAtTime(muted ? 0 : volume, t, 0.05);
  sfxBus.gain.setTargetAtTime(prefs.effects ? 1 : 0, t, 0.05);
  ambBus.gain.setTargetAtTime(prefs.ambience ? 1 : 0, t, 0.3);
  alarmBus.gain.setTargetAtTime(prefs.alarms ? 1 : 0, t, 0.05);
  callBus.gain.setTargetAtTime(prefs.calls ? 1 : 0, t, 0.05);
}

export const settings = {
  get volume() { return volume; },
  set volume(v) { volume = v; applyLevels(); save(); },
  get muted() { return muted; },
  set muted(v) { muted = v; applyLevels(); save(); },
  get prefs() { return prefs; },
  setPref(k, v) { prefs[k] = v; applyLevels(); save(); },
};

// ------------------------------------------------------------- helpers

let noiseBuf = null;
function noise() {
  if (!noiseBuf) {
    noiseBuf = ctx.createBuffer(1, ctx.sampleRate * 2, ctx.sampleRate);
    const d = noiseBuf.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }
  const n = ctx.createBufferSource();
  n.buffer = noiseBuf;
  n.loop = true;
  return n;
}

function env(gainNode, t, attack, peak, decay) {
  gainNode.gain.setValueAtTime(0.0001, t);
  gainNode.gain.exponentialRampToValueAtTime(peak, t + attack);
  gainNode.gain.exponentialRampToValueAtTime(0.0001, t + attack + decay);
}

/** Filtered noise burst. */
function burst(t, { freq = 2000, q = 1, type = "bandpass", peak = 0.5, attack = 0.002, decay = 0.05, bus = activeBus || sfxBus } = {}) {
  const n = noise(), f = ctx.createBiquadFilter(), g = ctx.createGain();
  f.type = type; f.frequency.value = freq; f.Q.value = q;
  n.connect(f).connect(g).connect(bus);
  env(g, t, attack, peak, decay);
  n.start(t, Math.random()); n.stop(t + attack + decay + 0.05);
}

/** Pitched thump / tone. */
function tone(t, { freq = 120, to = null, type = "sine", peak = 0.4, attack = 0.003, decay = 0.12, bus = activeBus || sfxBus } = {}) {
  const o = ctx.createOscillator(), g = ctx.createGain();
  o.type = type; o.frequency.setValueAtTime(freq, t);
  if (to) o.frequency.exponentialRampToValueAtTime(to, t + attack + decay);
  o.connect(g).connect(bus);
  env(g, t, attack, peak, decay);
  o.start(t); o.stop(t + attack + decay + 0.05);
}

// ------------------------------------------------------------ one-shots

const EFFECTS = {
  // Pistol-grip control switch: spring detent snap + heavy body thunk.
  jhandle(t) {
    burst(t, { freq: 3500, q: 2, peak: 0.35, decay: 0.025 });
    tone(t + 0.008, { freq: 140, to: 70, peak: 0.45, decay: 0.09 });
    burst(t + 0.03, { freq: 900, q: 1.5, peak: 0.12, decay: 0.06 });
  },
  // Rotary selector detent.
  detent(t) {
    burst(t, { freq: 4500, q: 4, peak: 0.25, decay: 0.015 });
    tone(t, { freq: 420, to: 300, type: "triangle", peak: 0.1, decay: 0.03 });
  },
  // Pushbutton / M-A station button.
  button(t) {
    burst(t, { freq: 2600, q: 3, peak: 0.25, decay: 0.02 });
    burst(t + 0.07, { freq: 2000, q: 3, peak: 0.12, decay: 0.02 });
  },
  // Spring-return switch thrown / released.
  spring(t) {
    burst(t, { freq: 3000, q: 2.5, peak: 0.25, decay: 0.03 });
    tone(t, { freq: 220, to: 120, peak: 0.15, decay: 0.05 });
  },
  springRelease(t) { burst(t, { freq: 3800, q: 3, peak: 0.18, decay: 0.02 }); },
  // Cover lifted on the scram buttons.
  cover(t) { burst(t, { freq: 1200, q: 1, peak: 0.2, decay: 0.08 }); },
  // Large circuit breaker closing: solenoid clack + mechanism thud.
  breakerClose(t) {
    burst(t, { freq: 1800, q: 0.8, peak: 0.6, attack: 0.001, decay: 0.08 });
    tone(t, { freq: 90, to: 45, peak: 0.8, decay: 0.25 });
    burst(t + 0.05, { freq: 400, q: 1, peak: 0.3, decay: 0.2, type: "lowpass" });
  },
  // Breaker tripping open: louder snap with an arc hiss.
  breakerOpen(t) {
    burst(t, { freq: 5000, q: 0.5, peak: 0.5, attack: 0.001, decay: 0.12, type: "highpass" });
    tone(t, { freq: 110, to: 50, peak: 0.7, decay: 0.2 });
    burst(t + 0.02, { freq: 900, q: 0.7, peak: 0.35, decay: 0.3 });
  },
  // Turbine stop/control valves slamming shut.
  valveSlam(t) {
    tone(t, { freq: 60, to: 30, peak: 0.9, decay: 0.5 });
    burst(t, { freq: 250, q: 0.7, type: "lowpass", peak: 0.6, decay: 0.6 });
    burst(t + 0.1, { freq: 1500, q: 0.6, peak: 0.25, decay: 0.8 });
  },
  // Reactor trip: trip breakers open, then rods drop and latch.
  scram(t) {
    EFFECTS.breakerOpen(t);
    for (let i = 0; i < 18; i++) burst(t + 0.15 + i * 0.09 + Math.random() * 0.04, { freq: 1500 + Math.random() * 2000, q: 2, peak: 0.25, decay: 0.03 });
    tone(t + 2.2, { freq: 80, to: 40, peak: 0.7, decay: 0.4 });
  },
  // One control rod drive step (magnetic jack latch sequence).
  rodStep(t) {
    burst(t, { freq: 2200, q: 5, peak: 0.12, decay: 0.012 });
    burst(t + 0.035, { freq: 1600, q: 5, peak: 0.09, decay: 0.012 });
  },
  // Pump motor start: inrush growl.
  pumpStart(t) {
    tone(t, { freq: 40, to: 120, type: "sawtooth", peak: 0.25, attack: 0.05, decay: 1.5 });
    EFFECTS.breakerClose(t);
  },
  pumpStop(t) { EFFECTS.breakerOpen(t); tone(t, { freq: 120, to: 30, type: "sawtooth", peak: 0.2, decay: 2.5 }); },
  // Alarm acknowledge / informational chime.
  ack(t) { tone(t, { freq: 880, type: "triangle", peak: 0.12, decay: 0.15, bus: alarmBus }); },
  chime(t) {
    tone(t, { freq: 1046, type: "sine", peak: 0.18, decay: 0.5, bus: alarmBus });
    tone(t + 0.18, { freq: 784, type: "sine", peak: 0.18, decay: 0.7, bus: alarmBus });
  },
  // Rejected command (interlock): dull double buzz.
  deny(t) {
    tone(t, { freq: 160, type: "square", peak: 0.08, decay: 0.08 });
    tone(t + 0.12, { freq: 160, type: "square", peak: 0.08, decay: 0.08 });
  },
  // Desk telephone ring: two bursts of a 440+480 Hz bell warble.
  phone(t) {
    for (const start of [0, 0.55]) {
      for (const f of [440, 480]) {
        const o = ctx.createOscillator(), g = ctx.createGain(), am = ctx.createOscillator(), ag = ctx.createGain();
        o.type = "triangle"; o.frequency.value = f;
        am.type = "square"; am.frequency.value = 22;
        ag.gain.value = 0.06;
        am.connect(ag).connect(g.gain);
        g.gain.value = 0.06;
        o.connect(g).connect(alarmBus);
        o.start(t + start); am.start(t + start);
        o.stop(t + start + 0.4); am.stop(t + start + 0.4);
      }
    }
  },
  // Handset pick-up click + short line tone.
  pickup(t) {
    burst(t, { freq: 1800, q: 2, peak: 0.2, decay: 0.03 });
    tone(t + 0.05, { freq: 425, type: "sine", peak: 0.06, attack: 0.01, decay: 0.25, bus: alarmBus });
  },
  // SR count-rate speaker click.
  count(t) { burst(t, { freq: 3000, q: 0.7, peak: 0.3, decay: 0.006 }); },
};

/** Play a one-shot effect by name. */
const CALL_SOUNDS = new Set(["phone", "pickup"]);
export function play(name, delay = 0) {
  if (!unlock() || !EFFECTS[name]) return;
  activeBus = CALL_SOUNDS.has(name) ? callBus : sfxBus;
  try { EFFECTS[name](ctx.currentTime + delay + 0.005); } finally { activeBus = null; }
}

// -------------------------------------------------------- alarm signals

let alarmNodes = null, alarmKind = null;
/** kind: null | "warning" | "alarm" | "trip" */
export function alarmSignal(kind) {
  if (!ctx) return;
  if (kind === alarmKind) return;
  if (alarmNodes) { alarmNodes.forEach(n => { try { n.stop(); } catch (_) { /* stopped */ } }); alarmNodes = null; }
  alarmKind = kind;
  if (!kind) return;
  const t = ctx.currentTime;
  const out = ctx.createGain();
  out.gain.value = 0;
  out.connect(alarmBus);
  if (kind === "warning") {
    // Soft repeating two-note chime.
    const o = ctx.createOscillator(), lfo = ctx.createOscillator(), lg = ctx.createGain();
    o.type = "sine"; o.frequency.value = 988;
    lfo.type = "square"; lfo.frequency.value = 0.8;
    lg.gain.value = 0.05;
    lfo.connect(lg).connect(out.gain);
    out.gain.value = 0.05;
    o.connect(out);
    o.start(t); lfo.start(t);
    alarmNodes = [o, lfo];
  } else if (kind === "alarm") {
    // Classic annunciator horn: buzzy square, pulsed.
    const o = ctx.createOscillator(), o2 = ctx.createOscillator(), lfo = ctx.createOscillator(), lg = ctx.createGain();
    o.type = "square"; o.frequency.value = 520;
    o2.type = "square"; o2.frequency.value = 523;
    lfo.type = "square"; lfo.frequency.value = 2.2;
    lg.gain.value = 0.06; out.gain.value = 0.06;
    lfo.connect(lg).connect(out.gain);
    o.connect(out); o2.connect(out);
    [o, o2, lfo].forEach(n => n.start(t));
    alarmNodes = [o, o2, lfo];
  } else {
    // Trip klaxon: alternating two tones.
    const o = ctx.createOscillator(), sweep = ctx.createOscillator(), sg = ctx.createGain();
    o.type = "sawtooth"; o.frequency.value = 650;
    sweep.type = "square"; sweep.frequency.value = 1.6;
    sg.gain.value = 170;
    sweep.connect(sg).connect(o.frequency);
    const f = ctx.createBiquadFilter(); f.type = "lowpass"; f.frequency.value = 2500;
    out.gain.value = 0.09;
    o.connect(f).connect(out);
    o.start(t); sweep.start(t);
    alarmNodes = [o, sweep];
  }
}

// ------------------------------------------------------------- ambience

let amb = null;
function buildAmbience() {
  const t = ctx.currentTime;
  const mk = (bus = ambBus) => { const g = ctx.createGain(); g.gain.value = 0; g.connect(bus); return g; };
  // Control room HVAC: low filtered noise.
  const hvac = mk(), hn = noise(), hf = ctx.createBiquadFilter();
  hf.type = "lowpass"; hf.frequency.value = 380;
  hn.connect(hf).connect(hvac); hn.start(t);
  hvac.gain.value = 0.05;
  // RCP motors: 60 Hz hum + harmonics, felt through the floor.
  const rcp = mk();
  const rcpOsc = [60, 120, 180].map((f, i) => { const o = ctx.createOscillator(); o.frequency.value = f; const g = ctx.createGain(); g.gain.value = [1, 0.5, 0.2][i]; o.connect(g).connect(rcp); o.start(t); return o; });
  // Turbine: whine following shaft speed plus broadband rumble.
  const turb = mk(), turbOsc = ctx.createOscillator(), turbOsc2 = ctx.createOscillator(), tf = ctx.createBiquadFilter();
  turbOsc.type = "sawtooth"; turbOsc2.type = "triangle";
  tf.type = "lowpass"; tf.frequency.value = 900;
  turbOsc.connect(tf); turbOsc2.connect(tf); tf.connect(turb);
  const rumble = mk(), rn = noise(), rf = ctx.createBiquadFilter();
  rf.type = "lowpass"; rf.frequency.value = 160;
  rn.connect(rf).connect(rumble); rn.start(t);
  turbOsc.start(t); turbOsc2.start(t);
  // Steam to condenser / atmosphere: roar.
  const steam = mk(), sn = noise(), sf = ctx.createBiquadFilter();
  sf.type = "bandpass"; sf.frequency.value = 900; sf.Q.value = 0.4;
  sn.connect(sf).connect(steam); sn.start(t);
  // Pressurizer PORV / relief: high hiss.
  const hiss = mk(), hsn = noise(), hsf = ctx.createBiquadFilter();
  hsf.type = "highpass"; hsf.frequency.value = 3000;
  hsn.connect(hsf).connect(hiss); hsn.start(t);
  amb = { rcp, rcpOsc, turb, turbOsc, turbOsc2, rumble, steam, hiss };
}

const set = (param, v, tc = 0.4) => param.setTargetAtTime(v, ctx.currentTime, tc);

// --------------------------------------------------------- state tracking

let prev = null;
let rodAccum = 0;

/** Called with every telemetry snapshot: drives ambience and event sounds. */
export function update(t) {
  if (!ctx || ctx.state !== "running") { prev = t; return; }
  if (!amb) buildAmbience();
  const R = t.readings;
  const scale = t.paused ? 0 : 1;

  // Ambience.
  const pumps = t.coolant_pumps.filter(Boolean).length / 4;
  set(amb.rcp.gain, 0.025 * pumps * scale);
  const rpm = R.turbine_speed_rpm || 0;
  const hz = rpm / 60;
  set(amb.turbOsc.frequency, Math.max(20, hz * 4), 0.3);
  set(amb.turbOsc2.frequency, Math.max(20, hz * 9), 0.3);
  set(amb.turb.gain, 0.03 * Math.min(1, rpm / 1800) * scale);
  set(amb.rumble.gain, (0.04 * Math.min(1, rpm / 1800) + 0.06 * Math.min(1, (R.steam_flow_kg_s || 0) / 1900)) * scale);
  const dump = (R.steam_dump_pct || 0) / 100 + (t.alarms.some(a => a.tag === "sg_arv") ? 0.6 : 0);
  set(amb.steam.gain, 0.12 * Math.min(1, dump) * scale, 0.2);
  set(amb.hiss.gain, (t.alarms.some(a => a.tag === "porv") ? 0.08 : 0) * scale, 0.1);

  // Rod drive stepping: one latch sequence per step moved.
  if (prev && !t.reactor_tripped) {
    let moved = 0;
    t.rods.forEach((r, i) => { moved += Math.abs(r.position - (prev.rods[i]?.position ?? r.position)); });
    rodAccum += moved;
  }
  if (rodAccum >= 1) {
    // Spread the steps over the next poll interval.
    const n = Math.min(12, Math.floor(rodAccum));
    rodAccum -= n;
    for (let i = 0; i < n; i++) play("rodStep", (i * 0.25) / n);
  }

  // Discrete plant events.
  if (prev && t.scenario === prev.scenario) {
    if (t.reactor_tripped && !prev.reactor_tripped) play("scram");
    if (t.turbine_tripped && !prev.turbine_tripped) play("valveSlam", 0.1);
    if (t.generator_breaker !== prev.generator_breaker) play(t.generator_breaker ? "breakerClose" : "breakerOpen");
    if (t.field_breaker !== prev.field_breaker) play(t.field_breaker ? "breakerClose" : "breakerOpen");
    t.coolant_pumps.forEach((on, i) => { if (on !== prev.coolant_pumps[i]) play(on ? "pumpStart" : "pumpStop"); });
  }
  prev = t;
}
