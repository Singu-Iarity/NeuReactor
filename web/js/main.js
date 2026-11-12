// NeuReactor control room: tabs, annunciators, polling and shared context.

import { rating, setRating } from "./rating.js";
import { api, send, toast } from "./api.js";
import { h, annunciatorPanel, animate } from "./widgets.js";
import * as ann from "./annunciator.js";
import * as sound from "./sound.js";
import { settings } from "./settings.js";
import { clipboard } from "./procedures.js";
import * as reactor from "./panels/reactor.js";
import * as rcs from "./panels/rcs.js";
import * as cvcs from "./panels/cvcs.js";
import * as steam from "./panels/steam.js";
import * as turbine from "./panels/turbine.js";
import * as grid from "./panels/grid.js";
import * as electrical from "./panels/electrical.js";
import * as comms from "./panels/comms.js";
import * as equipment from "./panels/equipment.js";
import * as computer from "./panels/computer.js";
import * as career from "./panels/career.js";

const PANELS = [reactor, rcs, cvcs, steam, turbine, electrical, grid, comms, equipment, computer, career];
/** Panels that must see every telemetry update even when hidden. */
const ALWAYS = new Set(["computer", "grid"]);
const $ = s => document.querySelector(s);

let telemetry = null;
let station = null;
let active = localStorageGet("nr-tab") || "reactor";

function localStorageGet(k) { try { return localStorage.getItem(k); } catch (_) { return null; } }
function localStorageSet(k, v) { try { localStorage.setItem(k, v); } catch (_) { /* ignore */ } }

// ------------------------------------------------------- blink clock
// One shared clock for every flashing window, lamp and tab so they all flash
// in step and never restart mid-flash.
setInterval(() => document.body.classList.toggle("bf"), 200);
setInterval(() => document.body.classList.toggle("bs"), 800);

// ------------------------------------------------------- SR count audio
let countsOn = false, cps = 0;
setInterval(() => {
  if (!countsOn) return;
  // Audible count rate is divided down above ~100 cps like a real scaler.
  const rate = cps > 100 ? 100 + Math.log10(cps / 100) * 40 : cps;
  let k = rate * 0.05;
  while (k > 0) { if (Math.random() < Math.min(1, k)) sound.play("count", Math.random() * 0.05); k -= 1; }
}, 50);

// ------------------------------------------------------- screen shake
// The room shakes with turbine vibration (felt through the floor).
let shakeAmp = 0;
const roomEl = $("#room");
animate((dt, now) => {
  const vib = telemetry?.readings?.turbine_vibration_um ?? 0;
  const target = settings.get("shake") ? Math.max(0, Math.min(1, (vib - 55) / 145)) : 0;
  shakeAmp += (target - shakeAmp) * Math.min(1, dt * 4);
  if (shakeAmp < 0.01) { if (roomEl.style.transform) roomEl.style.transform = ""; return; }
  const a = shakeAmp * 6;
  const t = now / 1000;
  const x = a * (Math.sin(t * 61) * 0.6 + Math.sin(t * 97 + 1.3) * 0.4);
  const y = a * (Math.sin(t * 73 + 0.7) * 0.6 + Math.sin(t * 113) * 0.4);
  roomEl.style.transform = `translate(${x.toFixed(2)}px, ${y.toFixed(2)}px)`;
});

// ---------------------------------------------------------------- ctx
const ctx = {
  send,
  t: () => telemetry,
  station: () => station,
  pollStation: () => pollStation(),
  moduleMode: id => telemetry?.modules.find(m => m.id === id)?.mode ?? "manual",
  setpoint: (id, key) => telemetry?.modules.find(m => m.id === id)?.setpoints[key],
  setMode: (module, mode) => send({ type: "set_module_mode", module, mode }),
  audioCounts(on) { countsOn = on; sound.unlock(); },
  countRate(v) { cps = v || 0; },
  scramPair,
  select: id => select(id),
};

/** Covered "push both to scram" button pair. */
function scramPair() {
  let armed = [false, false], timer = null;
  const press = i => {
    armed[i] = true;
    btns[i].classList.add("lit");
    clearTimeout(timer);
    if (armed[0] && armed[1]) {
      send({ type: "scram" });
      armed = [false, false];
      setTimeout(() => btns.forEach(b => b.classList.remove("lit")), 400);
    } else {
      timer = setTimeout(() => { armed = [false, false]; btns.forEach(b => b.classList.remove("lit")); }, 2000);
    }
  };
  const btns = [0, 1].map(i => h("button", { class: "scram-btn", onclick: () => { sound.play("button"); press(i); } }, i ? "B" : "A"));
  const cover = h("button", { class: "scram-cover", onclick: () => { sound.play("cover"); el.classList.toggle("open"); }, title: "Lift cover" }, "MANUAL SCRAM");
  const el = h("div", { class: "scram-pair" }, cover, h("div", { class: "scram-btns" }, ...btns), h("div", { class: "scram-label" }, "PUSH BOTH TO SCRAM"));
  return el;
}

// ---------------------------------------------------------------- build
// Read the unit rating before building panels so meters fit the unit size.
try { setRating(await api("/telemetry")); } catch (_) { /* defaults */ }
const builtSize = rating.size;
const tabs = $("#tabs");
const renderers = new Map();
const annCells = new Map();

for (const P of PANELS) {
  const tab = h("button", { class: "tab", "data-tab": P.id, onclick: () => { sound.play("detent"); select(P.id); } }, P.title);
  tabs.append(tab);
  const body = h("div", { class: "panel-body" });
  const head = h("div", { class: "panel-head", "data-section": `${P.id}.alarms` });
  if (!P.windows.length) head.style.display = "none";
  if (P.windows.length) {
    const a = annunciatorPanel(`${P.title.toUpperCase()} INDICATOR PANEL`, P.windows, P.cols || 4);
    a.cells.forEach((cell, tag) => annCells.set(tag, cell));
    head.append(a.el, h("div", { class: "ann-ctl" },
      h("button", { class: "pb-round red", onclick: () => ann.acknowledge(P.id), title: "Acknowledge this panel" }, "ACK"),
      h("button", { class: "pb-round white", onclick: () => ann.reset(P.id), title: "Reset cleared alarms" }, "RST")));
  }
  const section = h("section", { class: "panel", id: `panel-${P.id}` }, head, body,
    h("div", { class: "clipboard-rail" }, clipboard(P.id, () => telemetry)));
  roomEl.append(section);
  renderers.set(P.id, P.build(body, ctx));
}

function select(id) {
  active = id;
  localStorageSet("nr-tab", id);
  document.querySelectorAll(".tab").forEach(t => t.classList.toggle("active", t.dataset.tab === id));
  document.querySelectorAll(".panel").forEach(p => p.classList.toggle("active", p.id === `panel-${id}`));
  if (telemetry) render();
}
select(PANELS.some(p => p.id === active) ? active : "reactor");

// Top bar.
$("#master-ack").onclick = () => ann.acknowledge();
$("#master-sil").onclick = () => ann.silence();
$("#master-rst").onclick = () => ann.reset();
$("#horn").onclick = () => { ann.setHornEnabled(!ann.hornIsEnabled()); $("#horn").classList.toggle("lit", ann.hornIsEnabled()); };
$("#horn").classList.add("lit");
const mute = $("#mute"), vol = $("#volume");
const showMute = () => { mute.textContent = sound.settings.muted ? "🔇" : "🔊"; mute.classList.toggle("off", sound.settings.muted); };
vol.value = Math.round(sound.settings.volume * 100);
vol.oninput = () => { sound.unlock(); sound.settings.volume = vol.value / 100; };
mute.onclick = () => { sound.unlock(); sound.settings.muted = !sound.settings.muted; showMute(); };
showMute();
$("#top-scram").append(scramPair());
$("#pb-demand-box").onclick = () => select("grid");
document.querySelectorAll("#timectl button").forEach(b => b.onclick = async () => {
  const s = b.dataset.scale;
  if (s === "pause") return send({ type: "set_paused", paused: !telemetry?.paused });
  // Choosing a speed by hand takes over from auto warp.
  if (telemetry?.auto_warp) {
    await api("/time/auto_warp", { on: false });
    toast("Auto warp off");
  }
  await send({ type: "set_paused", paused: false });
  send({ type: "set_time_scale", factor: +s });
});

// Auto time warp.
$("#warp").onclick = async () => {
  sound.play("button");
  await api("/time/auto_warp", { on: !telemetry?.auto_warp, factor: +$("#warp-factor").value });
};
$("#warp-factor").onchange = () => api("/time/auto_warp", { on: !!telemetry?.auto_warp, factor: +$("#warp-factor").value });
let lastWarping = false;

function fmtTime(s) {
  s = Math.floor(s);
  return `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

const lastClass = new Map();
function setClass(el, cls) {
  if (lastClass.get(el) !== cls) { lastClass.set(el, cls); el.className = cls; }
}

// Control room lighting: normal AC lighting from the 6.9 kV buses (through
// 480 V lighting panels), AC emergency lighting from the diesel-backed safety
// buses, battery emergency lights from 125 V DC. Lights flicker as they
// change over.
let lightLevel = "normal", lightTimer = null;
function updateLighting(t) {
  const b = t.electrical?.buses;
  if (!b) return;
  // Annunciators and indicating lamps run from the vital instrument bus
  // (safety buses, or the battery through UPS inverters).
  const dcLost = t.indication_power === false;
  if (document.body.classList.contains("dc-lost") !== dcLost) {
    document.body.classList.toggle("dc-lost", dcLost);
    document.body.classList.add("dc-flicker");
    setTimeout(() => document.body.classList.remove("dc-flicker"), 1200);
  }
  const level = b.bus_a || b.bus_b ? "normal" : b.safety1 || b.safety2 ? "emergency" : b.dc ? "battery" : "dark";
  if (level === lightLevel) return;
  const el = document.getElementById("mcr-lights");
  const worse = ["normal", "emergency", "battery", "dark"];
  const dimming = worse.indexOf(level) > worse.indexOf(lightLevel);
  lightLevel = level;
  clearTimeout(lightTimer);
  el.className = `lights-${level} flicker`;
  sound.play(dimming ? "jhandle" : "detent");
  lightTimer = setTimeout(() => { el.className = `lights-${level}`; }, dimming ? 900 : 1400);
}

function render() {
  const t = telemetry;
  // A different unit size (new core, sandbox size change): rebuild the room.
  if (t.unit_size && t.unit_size !== builtSize) { location.reload(); return; }
  updateLighting(t);
  document.querySelectorAll(".sect, .panel-head").forEach(el => {
    const g = t.sections[el.dataset.section] || "analog";
    if (el.dataset.gen !== g) el.dataset.gen = g;
  });
  $("#unit").textContent = `${t.unit} · ${t.reactor_type} · ${t.game_mode.toUpperCase()} · ${t.scenario.replace("_", " ").toUpperCase()}`;
  $("#simtime").textContent = fmtTime(t.sim_time);
  document.querySelectorAll("#timectl button").forEach(b => {
    const s = b.dataset.scale;
    b.classList.toggle("on", s === "pause" ? t.paused : !t.paused && +s === t.time_scale);
  });
  $("#warp").classList.toggle("on", !!t.auto_warp);
  $("#warp").classList.toggle("active", !!t.warping);
  if (document.activeElement !== $("#warp-factor") && t.warp_factor) $("#warp-factor").value = String(t.warp_factor);
  $("#warp-state").textContent = !t.auto_warp ? "" : t.warping ? `WARPING ${t.warp_factor}×` : `HOLD: ${t.warp_reason || "…"}`;
  $("#warp-state").className = `warp-state ${t.warping ? "go" : t.auto_warp ? "hold" : ""}`;
  if (lastWarping && !t.warping && t.auto_warp) toast(`Auto warp slowed down: ${t.warp_reason}`);
  lastWarping = !!t.warping;
  const R = t.readings;
  $("#pb-gross").textContent = `${R.gross_mwe.toFixed(0)} MWe`;
  $("#pb-net").textContent = `${R.net_mwe.toFixed(0)} MWe`;
  $("#pb-demand").textContent = station && station.demand_mwe > 0 ? `${station.demand_mwe.toFixed(0)} MWe` : "—";
  $("#pb-demand-box").classList.toggle("pending", !!station?.pending);

  ann.ingest(t.alarms);
  sound.update(t);
  annCells.forEach((cell, tag) => setClass(cell, `ann-win ${ann.windowClass(tag)}`));
  for (const P of PANELS) {
    setClass(document.querySelector(`.tab[data-tab="${P.id}"]`), `tab ${P.id === active ? "active" : ""} ${ann.panelState(P.id)}`);
  }
  for (const [id, r] of renderers) {
    if (id !== active && !ALWAYS.has(id)) continue;
    try { r(t, id === active); } catch (e) { console.error(`panel ${id}:`, e); }
  }
}

async function poll() {
  try {
    telemetry = await api("/telemetry");
    if (telemetry.ok === false) throw new Error(telemetry.error);
    render();
    $("#conn").classList.remove("lost");
  } catch (e) {
    $("#conn").classList.add("lost");
    toast("Lost connection to the NeuReactor server");
  }
}

// Grid dispatch & phones: ring and pop a toast for each new incoming call.
let lastSeq = null;
async function pollStation() {
  try {
    station = await api("/station");
    const msgs = station.messages || [];
    const newest = msgs.length ? msgs[0].seq : 0;
    if (lastSeq !== null) {
      const fresh = msgs.filter(m => m.seq > lastSeq && m.ring).reverse();
      if (fresh.length) {
        sound.play("phone");
        const m = fresh[fresh.length - 1];
        toast(`☎ ${m.from}: ${m.text}`);
      }
    }
    lastSeq = newest;
  } catch (_) { /* reported by telemetry poll */ }
}

setInterval(poll, 250);
setInterval(pollStation, 1000);
poll();
pollStation();
