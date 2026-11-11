// NeuReactor control room client. Talks to the local server via the same
// /api/v1 endpoints that plugins use (with the operator token).

const TOKEN = window.NR_TOKEN;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

let telemetry = null;
let lastEventSeq = 0;
const history = [];
const HISTORY_MAX = 600;

async function api(path, body) {
  const opts = { headers: { Authorization: `Bearer ${TOKEN}` } };
  if (body !== undefined) {
    opts.method = "POST";
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(`/api/v1${path}`, opts);
  return res.json();
}

async function send(cmd) {
  const r = await api("/command", cmd);
  toast(r.ok ? "" : `Rejected: ${r.error}`);
  return r;
}

function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  if (msg) setTimeout(() => { if (t.textContent === msg) t.textContent = ""; }, 5000);
}

// ---------------------------------------------------------------- gauges

function drawGauge(cv, value, refValue) {
  const ctx = cv.getContext("2d");
  const w = cv.width, h = cv.height;
  const min = +cv.dataset.min, max = +cv.dataset.max;
  const cx = w / 2, cy = h * 0.84, r = Math.min(w * 0.42, h * 0.62);
  const a0 = Math.PI * 1.15, a1 = Math.PI * 1.85;
  const ang = v => a0 + (a1 - a0) * Math.max(0, Math.min(1, (v - min) / (max - min)));
  ctx.clearRect(0, 0, w, h);
  ctx.lineWidth = 1;
  ctx.strokeStyle = "#222";
  ctx.fillStyle = "#222";
  ctx.font = "10px sans-serif";
  ctx.textAlign = "center";
  ctx.beginPath(); ctx.arc(cx, cy, r, a0, a1); ctx.stroke();
  for (let i = 0; i <= 10; i++) {
    const v = min + (max - min) * i / 10, a = ang(v), major = i % 2 === 0;
    const r1 = r - (major ? 9 : 5);
    ctx.beginPath();
    ctx.moveTo(cx + Math.cos(a) * r, cy + Math.sin(a) * r);
    ctx.lineTo(cx + Math.cos(a) * r1, cy + Math.sin(a) * r1);
    ctx.stroke();
    if (major) ctx.fillText(+v.toFixed(1), cx + Math.cos(a) * (r - 18), cy + Math.sin(a) * (r - 18) + 3);
  }
  ctx.font = "bold 11px sans-serif";
  ctx.fillText(cv.dataset.label, cx, 14);
  ctx.font = "10px sans-serif";
  if (refValue !== undefined) {
    const a = ang(refValue);
    ctx.strokeStyle = "#1a7a2a"; ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(cx + Math.cos(a) * (r + 1), cy + Math.sin(a) * (r + 1));
    ctx.lineTo(cx + Math.cos(a) * (r - 10), cy + Math.sin(a) * (r - 10));
    ctx.stroke();
  }
  if (value !== undefined) {
    const a = ang(value);
    ctx.strokeStyle = "#b8231a"; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(cx + Math.cos(a) * (r - 4), cy + Math.sin(a) * (r - 4)); ctx.stroke();
    ctx.fillStyle = "#222"; ctx.beginPath(); ctx.arc(cx, cy, 4, 0, 7); ctx.fill();
    ctx.font = "bold 12px monospace";
    ctx.fillText(value.toFixed(Math.abs(max - min) < 20 ? 2 : 1), cx, cy + 14);
  }
}

// ----------------------------------------------------------------- trends

const TRENDS = [
  { tag: "nuclear_power_pct", label: "Power %", c: "#f0a830", min: 0, max: 120 },
  { tag: "tavg_c", label: "Tavg °C (260–340)", c: "#e0453a", min: 260, max: 340 },
  { tag: "rcs_pressure_mpa", label: "RCS MPa (12–18)", c: "#6aa8e0", min: 12, max: 18 },
  { tag: "pzr_level_pct", label: "PZR level %", c: "#5cc46c", min: 0, max: 100 },
  { tag: "sg_level_pct", label: "SG level %", c: "#c08ae0", min: 0, max: 100 },
  { tag: "gross_mwe", label: "MWe (0–1300)", c: "#e8e2cf", min: 0, max: 1300 },
];

function drawTrend() {
  const cv = $("#trend");
  const w = (cv.width = cv.clientWidth), h = cv.height;
  const ctx = cv.getContext("2d");
  ctx.clearRect(0, 0, w, h);
  ctx.strokeStyle = "#233"; ctx.lineWidth = 1;
  for (let i = 1; i < 4; i++) { ctx.beginPath(); ctx.moveTo(0, h * i / 4); ctx.lineTo(w, h * i / 4); ctx.stroke(); }
  if (history.length < 2) return;
  for (const t of TRENDS) {
    ctx.strokeStyle = t.c; ctx.lineWidth = 1.5; ctx.beginPath();
    history.forEach((s, i) => {
      const x = (i / (HISTORY_MAX - 1)) * w;
      const v = (s[t.tag] - t.min) / (t.max - t.min);
      const y = h - Math.max(0, Math.min(1, v)) * h;
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    });
    ctx.stroke();
  }
  const t0 = history[0].t, t1 = history[history.length - 1].t;
  ctx.fillStyle = "#8c958d"; ctx.font = "11px monospace";
  ctx.fillText(`${fmtTime(t0)} … ${fmtTime(t1)}`, 6, 14);
}

// ----------------------------------------------------------------- render

function fmtTime(s) {
  s = Math.floor(s);
  const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, sec = s % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
}

const ANNUNCIATORS = [
  ["rx_trip", "Reactor trip"], ["turb_trip", "Turbine trip"], ["tavg_dev", "Tavg/Tref dev"],
  ["p_high", "PZR press high"], ["p_low", "PZR press low"], ["porv", "PORV open"],
  ["pzr_lvl_dev", "PZR level dev"], ["sg_lvl_dev", "SG level dev"], ["sur", "High SUR"],
  ["rod_ins", "Rod insertion limit"], ["rcp", "RCP stopped"], ["training", "Training mode"],
];

function buildStatic() {
  $("#annunciators").innerHTML = ANNUNCIATORS.map(([tag, text]) => `<div class="tile" data-alarm="${tag}">${text}</div>`).join("");
  $("#trend-legend").innerHTML = TRENDS.map(t => `<span style="--c:${t.c}">${t.label}</span>`).join("");
  $("#rcps").innerHTML = [0, 1, 2, 3].map(i => `<button class="pump" data-rcp="${i}">RCP ${i + 1}</button>`).join("");
}

function moduleMode(id) {
  return telemetry?.modules.find(m => m.id === id)?.mode ?? "manual";
}

function render() {
  const t = telemetry;
  if (!t) return;
  const R = t.readings;
  $("#unit").textContent = `${t.unit} · ${t.reactor_type} · ${t.game_mode}`;
  $("#simtime").textContent = fmtTime(t.sim_time);
  $$("#timectl button").forEach(b => {
    const s = b.dataset.scale;
    b.classList.toggle("on", s === "pause" ? t.paused : !t.paused && +s === t.time_scale);
  });
  $("#training").checked = t.training_mode;

  for (const el of $$("[data-tag]")) {
    const v = R[el.dataset.tag];
    if (el.tagName === "CANVAS") {
      drawGauge(el, v, el.dataset.ref ? R[el.dataset.ref] : undefined);
    } else if (v !== undefined) {
      el.textContent = v.toFixed(+(el.dataset.fmt ?? 1));
    }
  }

  // Annunciators.
  const active = new Map(t.alarms.map(a => [a.tag, a.severity]));
  $$("[data-alarm]").forEach(el => {
    el.className = "tile " + (active.get(el.dataset.alarm) ?? "");
    if (el.dataset.alarm === "rx_trip" && t.trips.length) el.title = t.trips.join("\n");
  });

  // Module badges and actuator locks.
  $$("[data-module]").forEach(el => {
    const m = moduleMode(el.dataset.module);
    el.textContent = m.toUpperCase();
    el.className = `badge ${m}`;
  });
  $$("[data-actuator]").forEach(el => {
    const locked = moduleMode(el.dataset.actuator) !== "manual";
    if (el.classList.contains("seg")) el.classList.toggle("locked", locked);
    else el.disabled = locked;
  });

  // Sliders follow the plant unless being dragged.
  $$("input[type=range][data-read]").forEach(el => {
    const v = R[el.dataset.read];
    if (v === undefined) return;
    if (document.activeElement !== el) el.value = v;
    el.nextElementSibling.textContent = v.toFixed(el.step && el.step < 1 ? 1 : 0);
  });

  // Rods.
  const rods = $("#rods");
  if (!rods.children.length) {
    rods.innerHTML = t.rods.map(r => `<div class="rod"><div class="shaft"><div class="ins"></div><div class="tgt"></div></div><b>${r.bank}</b><span></span></div>`).join("");
    $("#rod-bank").innerHTML = t.rods.map(r => `<option>${r.bank}</option>`).join("");
  }
  t.rods.forEach((r, i) => {
    const el = rods.children[i];
    $(".ins", el).style.height = `${(1 - r.position / r.max_steps) * 100}%`;
    $(".tgt", el).style.top = `${(1 - r.target / r.max_steps) * 100}%`;
    $("span", el).textContent = Math.round(r.position);
  });

  $$("#boration button").forEach(b => b.classList.toggle("on", b.dataset.mode === t.boration));
  $$("[data-rcp]").forEach(b => b.classList.toggle("on", t.coolant_pumps[+b.dataset.rcp]));
  $("#breaker-state").textContent = t.generator_breaker ? "CLOSED" : "OPEN";

  const gov = t.modules.find(m => m.id === "turbine_governor");
  if (gov && document.activeElement !== $("#gov-target")) $("#gov-target").value = Math.round(gov.setpoints.target_mwe);
  if (gov && document.activeElement !== $("#gov-ramp")) $("#gov-ramp").value = gov.setpoints.ramp_mw_min;

  renderModules(t.modules);
}

let moduleSig = "";
function renderModules(mods) {
  const sig = JSON.stringify(mods.map(m => [m.id, m.mode, m.owned]));
  const host = $("#modules");
  if (sig !== moduleSig) {
    moduleSig = sig;
    host.innerHTML = mods.map(m => `
      <div class="module ${m.implemented ? "" : "na"}" data-id="${m.id}">
        <div class="head">
          <div><div class="name">${m.name}</div><div class="panelname">${m.panel}${m.implemented ? "" : " · not simulated yet"}${m.owned ? "" : " · not installed"}</div></div>
          <div class="seg">${["manual", "auto", "external"].map(x => `<button data-mode="${x}" class="${m.mode === x ? "on" : ""}" ${m.implemented ? "" : "disabled"}>${x[0].toUpperCase() + x.slice(1, 3)}</button>`).join("")}</div>
        </div>
        <div class="sp">${Object.entries(m.setpoints).map(([k, v]) => `<label>${k} <input data-sp="${k}" type="number" step="any" value="${+v.toFixed(2)}"></label>`).join("")}</div>
      </div>`).join("");
  } else {
    // Refresh setpoint values that are not being edited.
    for (const m of mods) {
      for (const [k, v] of Object.entries(m.setpoints)) {
        const el = host.querySelector(`[data-id="${m.id}"] [data-sp="${k}"]`);
        if (el && document.activeElement !== el) el.value = +v.toFixed(2);
      }
    }
  }
}

// ------------------------------------------------------------------ input

function wire() {
  $("#scram").onclick = () => send({ type: "scram" });
  $("#reset").onclick = () => send({ type: "reset_trip" });
  $("#training").onchange = e => send({ type: "set_training_mode", enabled: e.target.checked });
  $$("#timectl button").forEach(b => b.onclick = async () => {
    const s = b.dataset.scale;
    if (s === "pause") return send({ type: "set_paused", paused: !telemetry?.paused });
    await send({ type: "set_paused", paused: false });
    send({ type: "set_time_scale", factor: +s });
  });

  $$("input[type=range][data-cmd]").forEach(el => {
    el.onchange = () => {
      const key = el.dataset.key ?? "fraction";
      const v = key === "fraction" ? el.value / 100 : +el.value;
      send({ type: el.dataset.cmd, [key]: v });
      el.blur();
    };
  });

  $("#rod-move").onclick = () => send({ type: "move_rod_bank", bank: $("#rod-bank").value, target: +$("#rod-target").value });
  $$("#boration button").forEach(b => b.onclick = () => {
    if (!$("#boration").classList.contains("locked")) send({ type: "set_boration", mode: b.dataset.mode });
  });
  $("#rcps").onclick = e => {
    const b = e.target.closest("[data-rcp]");
    if (b) send({ type: "set_coolant_pump", index: +b.dataset.rcp, running: !telemetry.coolant_pumps[+b.dataset.rcp] });
  };
  $("#breaker-close").onclick = () => send({ type: "set_generator_breaker", closed: true });
  $("#breaker-open").onclick = () => send({ type: "set_generator_breaker", closed: false });
  $("#gov-set").onclick = async () => {
    await send({ type: "set_setpoint", module: "turbine_governor", key: "ramp_mw_min", value: +$("#gov-ramp").value });
    send({ type: "set_setpoint", module: "turbine_governor", key: "target_mwe", value: +$("#gov-target").value });
    document.activeElement.blur();
  };

  $("#modules").addEventListener("click", e => {
    const b = e.target.closest("button[data-mode]");
    if (!b) return;
    const id = b.closest("[data-id]").dataset.id;
    send({ type: "set_module_mode", module: id, mode: b.dataset.mode });
  });
  $("#modules").addEventListener("change", e => {
    const el = e.target.closest("[data-sp]");
    if (!el) return;
    const id = el.closest("[data-id]").dataset.id;
    send({ type: "set_setpoint", module: id, key: el.dataset.sp, value: +el.value });
    el.blur();
  });

  $("#plugins").addEventListener("click", async e => {
    const b = e.target.closest("button[data-plugin]");
    if (!b) return;
    await api("/plugins/approve", { id: b.dataset.plugin, approve: b.dataset.approve === "1" });
    pollPlugins();
  });
}

// ---------------------------------------------------------------- polling

async function pollTelemetry() {
  try {
    telemetry = await api("/telemetry");
    const s = { t: telemetry.sim_time };
    for (const tr of TRENDS) s[tr.tag] = telemetry.readings[tr.tag];
    if (!history.length || s.t > history[history.length - 1].t) {
      history.push(s);
      if (history.length > HISTORY_MAX) history.shift();
    }
    render();
    drawTrend();
  } catch (e) {
    toast("Lost connection to the NeuReactor server");
  }
}

async function pollEvents() {
  try {
    const ev = await api(`/events?since=${lastEventSeq}`);
    const list = $("#events");
    for (const e of ev) {
      lastEventSeq = Math.max(lastEventSeq, e.seq);
      const li = document.createElement("li");
      li.className = e.severity;
      li.textContent = `${fmtTime(e.sim_time)}  ${e.text}`;
      list.prepend(li);
    }
    while (list.children.length > 200) list.lastChild.remove();
  } catch (_) { /* server down: telemetry poll reports it */ }
}

async function pollPlugins() {
  try {
    const list = await api("/plugins");
    $("#plugins").innerHTML = list.length
      ? list.map(p => `<div class="module"><div class="head"><div><div class="name">${escapeHtml(p.name)}</div>
          <div class="panelname">${p.scopes.join(", ")}</div></div>
          <div>${p.approved ? "" : `<button class="btn" data-plugin="${p.id}" data-approve="1">Approve</button>`}
          <button class="btn" data-plugin="${p.id}" data-approve="0">${p.approved ? "Revoke" : "Deny"}</button></div></div></div>`).join("")
      : `<p class="dim">No plugins connected.</p>`;
  } catch (_) {}
}

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

buildStatic();
wire();
pollTelemetry();
setInterval(pollTelemetry, 250);
setInterval(pollEvents, 1000);
setInterval(pollPlugins, 2000);
pollPlugins();
