// NeuReactor control room: tabs, annunciators, polling and shared context.

import { api, send, toast } from "./api.js";
import { h, annunciatorPanel } from "./widgets.js";
import * as ann from "./annunciator.js";
import * as reactor from "./panels/reactor.js";
import * as rcs from "./panels/rcs.js";
import * as cvcs from "./panels/cvcs.js";
import * as steam from "./panels/steam.js";
import * as turbine from "./panels/turbine.js";
import * as automation from "./panels/equipment.js";
import * as computer from "./panels/computer.js";
import * as career from "./panels/career.js";

const PANELS = [reactor, rcs, cvcs, steam, turbine, automation, computer, career];
const $ = s => document.querySelector(s);

let telemetry = null;
let active = localStorageGet("nr-tab") || "reactor";

function localStorageGet(k) { try { return localStorage.getItem(k); } catch (_) { return null; } }
function localStorageSet(k, v) { try { localStorage.setItem(k, v); } catch (_) { /* ignore */ } }

// ------------------------------------------------------- SR count audio
let audioCtx = null, countsOn = false, cps = 0;
function click() {
  const n = audioCtx.createBufferSource();
  const buf = audioCtx.createBuffer(1, 256, audioCtx.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < d.length; i++) d[i] = (Math.random() * 2 - 1) * Math.exp(-i / 30) * 0.4;
  n.buffer = buf;
  n.connect(audioCtx.destination);
  n.start(audioCtx.currentTime + Math.random() * 0.05);
}
setInterval(() => {
  if (!countsOn || !audioCtx) return;
  // Audible count rate is divided down above ~100 cps like a real scaler.
  const rate = cps > 100 ? 100 + Math.log10(cps / 100) * 40 : cps;
  let k = rate * 0.05;
  while (k > 0) { if (Math.random() < Math.min(1, k)) click(); k -= 1; }
}, 50);

// ---------------------------------------------------------------- ctx
const ctx = {
  send,
  t: () => telemetry,
  moduleMode: id => telemetry?.modules.find(m => m.id === id)?.mode ?? "manual",
  setpoint: (id, key) => telemetry?.modules.find(m => m.id === id)?.setpoints[key],
  setMode: (module, mode) => send({ type: "set_module_mode", module, mode }),
  audioCounts(on) {
    countsOn = on;
    if (on && !audioCtx) try { audioCtx = new AudioContext(); } catch (_) { countsOn = false; }
  },
  countRate(v) { cps = v || 0; },
  scramPair,
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
  const btns = [0, 1].map(i => h("button", { class: "scram-btn", onclick: () => press(i) }, i ? "B" : "A"));
  const cover = h("button", { class: "scram-cover", onclick: () => el.classList.toggle("open"), title: "Lift cover" }, "MANUAL SCRAM");
  const el = h("div", { class: "scram-pair" }, cover, h("div", { class: "scram-btns" }, ...btns), h("div", { class: "scram-label" }, "PUSH BOTH TO SCRAM"));
  return el;
}

// ---------------------------------------------------------------- build
const tabs = $("#tabs");
const room = $("#room");
const renderers = new Map();
const annCells = new Map();

for (const P of PANELS) {
  const tab = h("button", { class: "tab", "data-tab": P.id, onclick: () => select(P.id) }, P.title);
  tabs.append(tab);
  const body = h("div", { class: "panel-body" });
  const head = h("div", { class: "panel-head", "data-section": `${P.id}.alarms` });
  if (!P.windows.length) head.style.display = "none";
  if (P.windows.length) {
    const a = annunciatorPanel(`${P.title.toUpperCase()} INDICATOR PANEL`, P.windows);
    a.cells.forEach((cell, tag) => annCells.set(tag, cell));
    head.append(a.el, h("div", { class: "ann-ctl" },
      h("button", { class: "pb-round red", onclick: () => ann.acknowledge(P.id), title: "Acknowledge this panel" }, "ACK"),
      h("button", { class: "pb-round white", onclick: () => ann.reset(P.id), title: "Reset cleared alarms" }, "RST")));
  }
  const section = h("section", { class: "panel", id: `panel-${P.id}` }, head, body);
  room.append(section);
  renderers.set(P.id, P.build(body, ctx));
}

function select(id) {
  active = id;
  localStorageSet("nr-tab", id);
  document.querySelectorAll(".tab").forEach(t => t.classList.toggle("active", t.dataset.tab === id));
  document.querySelectorAll(".panel").forEach(p => p.classList.toggle("active", p.id === `panel-${id}`));
}
select(PANELS.some(p => p.id === active) ? active : "reactor");

// Top bar.
$("#master-ack").onclick = () => ann.acknowledge();
$("#master-sil").onclick = () => ann.silence();
$("#master-rst").onclick = () => ann.reset();
$("#horn").onclick = () => { ann.setHornEnabled(!ann.hornIsEnabled()); $("#horn").classList.toggle("lit", ann.hornIsEnabled()); };
$("#horn").classList.add("lit");
$("#top-scram").append(scramPair());
document.querySelectorAll("#timectl button").forEach(b => b.onclick = async () => {
  const s = b.dataset.scale;
  if (s === "pause") return send({ type: "set_paused", paused: !telemetry?.paused });
  await send({ type: "set_paused", paused: false });
  send({ type: "set_time_scale", factor: +s });
});

function fmtTime(s) {
  s = Math.floor(s);
  return `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

function render() {
  const t = telemetry;
  // Each board section takes the look of its installed hardware package.
  document.querySelectorAll(".sect, .panel-head").forEach(el => {
    el.dataset.gen = t.sections[el.dataset.section] || "analog";
  });
  $("#unit").textContent = `${t.unit} · ${t.reactor_type} · ${t.game_mode.toUpperCase()} · ${t.scenario.replace("_", " ").toUpperCase()}`;
  $("#simtime").textContent = fmtTime(t.sim_time);
  document.querySelectorAll("#timectl button").forEach(b => {
    const s = b.dataset.scale;
    b.classList.toggle("on", s === "pause" ? t.paused : !t.paused && +s === t.time_scale);
  });

  ann.ingest(t.alarms);
  annCells.forEach((cell, tag) => { cell.className = `ann-win ${ann.windowClass(tag)}`; });
  for (const P of PANELS) {
    const st = ann.panelState(P.id);
    document.querySelector(`.tab[data-tab="${P.id}"]`).className = `tab ${P.id === active ? "active" : ""} ${st}`;
  }
  for (const [id, r] of renderers) {
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
setInterval(poll, 250);
poll();
