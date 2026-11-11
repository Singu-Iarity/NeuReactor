// Operating procedures on clipboards. Each step can carry a live check that
// ticks it automatically from telemetry; the rest are checked off by hand.

import { h } from "./widgets.js";
import { play } from "./sound.js";

const has = (t, tag) => t.alarms.some(a => a.tag === tag);
const bank = (t, b) => t.rods.find(r => r.bank === b)?.position ?? 0;

export const PROCEDURES = [
  {
    id: "op1", code: "OP-1", title: "Condenser Vacuum Establishment", tab: "steam",
    steps: [
      ["Verify all three circulating water pumps running", t => t.cw_pumps.every(Boolean)],
      ["Verify main steam pressure above 1.5 MPa (SJAE motive steam)", t => t.readings.sg_pressure_mpa > 1.5],
      ["Start both condenser vacuum (hogging) pumps", t => t.vacuum_pumps.some(Boolean) || t.readings.condenser_kpa < 20],
      ["Open SJAE motive steam", t => t.sjae_on],
      ["Wait for condenser pressure below 25 kPa (turbine latch permissive)", t => t.readings.condenser_kpa < 25],
      ["When below ~15 kPa with SJAEs in service, stop the hogging pumps", t => !t.vacuum_pumps.some(Boolean) && t.readings.condenser_kpa < 15],
      ["Verify vacuum stable below 10 kPa", t => t.readings.condenser_kpa < 10],
      ["Open deaerator pegging steam; verify DEAERATION NOT EFFECTIVE clears", t => !has(t, "deaeration")],
    ],
  },
  {
    id: "op2", code: "OP-2", title: "Reactor Startup — Hot Standby to P-10", tab: "reactor",
    steps: [
      ["Verify 4 RCPs running, PZR pressure ~15.5 MPa, Tavg ~292 °C", t => t.coolant_pumps.every(Boolean) && Math.abs(t.readings.rcs_pressure_mpa - 15.5) < 0.2],
      ["Record source range count rate (baseline for 1/M)", null],
      ["Select bank SA; set ROD SPEED; withdraw SA to 228 steps", t => bank(t, "SA") >= 228],
      ["Select bank SB; withdraw SB to 228 steps", t => bank(t, "SB") >= 228],
      ["Select MAN (control banks in overlap)", null],
      ["Withdraw control banks in steps; keep startup rate below 1 dpm", null],
      ["Reactor critical: steady positive startup rate with rods stopped", t => has(t, "critical")],
      ["P-6 lit: press SR BLOCK (source range HV off)", t => t.nis.sr_blocked],
      ["Raise power through the intermediate range to P-10", t => t.nis.p10],
      ["Press IR BLOCK and PR LO BLOCK before 25 % power", t => t.nis.ir_blocked && t.nis.pr_low_blocked],
      ["Stabilise at 8–12 % power (steam to dumps/condenser)", t => t.readings.nuclear_power_pct > 7 && t.readings.nuclear_power_pct < 15],
    ],
  },
  {
    id: "op3", code: "OP-3", title: "Turbine Roll & Synchronization", tab: "turbine",
    steps: [
      ["Condenser vacuum below 25 kPa (OP-1)", t => t.readings.condenser_kpa < 25],
      ["Reactor at 8–12 % power (OP-2)", t => t.readings.nuclear_power_pct > 7],
      ["Comms: call TURBINE HALL for roll checks", t => t.turbine_hall_ready],
      ["Press TURBINE RESET (latch)", t => !t.turbine_tripped],
      ["Close EXCITER FIELD BKR", t => t.field_breaker],
      ["Select acceleration; press 1800 (avoid holding near 1150 / 1650 rpm)", t => t.readings.turbine_speed_rpm > 1790],
      ["Verify vibration below 125 µm", t => t.readings.turbine_vibration_um < 125],
      ["Comms: request PERMISSION TO SYNC from Grid Dispatch", t => t.grid_permission],
      ["Match generator voltage to bus (AVR)", t => Math.abs(t.readings.gen_voltage_kv - t.readings.grid_voltage_kv) < 0.5],
      ["Synchroscope turning slowly FAST: close GEN BREAKER at 12 o'clock", t => t.generator_breaker],
      ["Comms: REPORT ON-LINE to Grid Dispatch", null],
    ],
  },
  {
    id: "op4", code: "OP-4", title: "Power Ascension & Load Following", tab: "grid",
    steps: [
      ["Generator on-line; turbine governor in AUTO", t => t.generator_breaker && t.modules.find(m => m.id === "turbine_governor")?.mode === "auto"],
      ["Accept the grid dispatch load request (Grid tab) or enable AGC", null],
      ["Place rod control in AUTO above 15 % turbine load", t => t.modules.find(m => m.id === "rod_control")?.mode === "auto"],
      ["Verify feedwater, pressurizer and steam dump control in AUTO", t => ["feedwater_level", "pressurizer_pressure", "pressurizer_level", "steam_dump"].every(id => t.modules.find(m => m.id === id)?.mode === "auto")],
      ["Close pegging steam once extraction holds the deaerator", t => !t.pegging_steam && !has(t, "deaeration")],
      ["Keep Tavg on program and rods above the insertion limit (dilute as xenon builds)", t => !has(t, "tavg_dev") && !has(t, "rod_ins_lo") && !has(t, "rod_ins_lolo")],
      ["Net output on demand (±3 %)", t => !has(t, "off_demand") && t.generator_breaker],
    ],
  },
  {
    id: "op5", code: "OP-5", title: "Normal Shutdown", tab: "turbine",
    steps: [
      ["Notify Grid Dispatch; reduce load at ≤ 30 MW/min", null],
      ["At minimum load, open GEN BREAKER", t => !t.generator_breaker],
      ["Trip the turbine", t => t.turbine_tripped],
      ["Steam dumps holding no-load Tavg (~292 °C)", t => Math.abs(t.readings.tavg_c - 291.7) < 3],
      ["Insert control banks (rod control MANUAL, MAN, IN)", t => ["A", "B", "C", "D"].every(b => bank(t, b) <= 0)],
      ["Insert shutdown banks or trip the reactor", t => ["SA", "SB"].every(b => bank(t, b) <= 0)],
    ],
  },
  {
    id: "aop1", code: "AOP-1", title: "Reactor Trip Recovery", tab: "reactor",
    steps: [
      ["Verify all rods fully inserted", t => t.rods.every(r => r.position <= 0)],
      ["Verify turbine tripped and generator breaker open", t => t.turbine_tripped && !t.generator_breaker],
      ["Verify steam dumps / relief valves controlling Tavg", t => Math.abs(t.readings.tavg_c - 291.7) < 5],
      ["Verify pressurizer pressure and level recovering", t => t.readings.rcs_pressure_mpa > 14.5 && t.readings.pzr_level_pct > 17],
      ["Verify SG levels recovering (feedwater)", t => t.readings.sg_level_pct > 30],
      ["Identify the first-out trip (Reactor tab) and correct the cause", null],
      ["Comms: REPORT TRIP to Grid Dispatch", null],
      ["Reset the reactor trip when conditions allow", t => !t.reactor_tripped],
    ],
  },
];

function load() {
  try { return JSON.parse(localStorage.getItem("nr-procedures") || "{}"); } catch (_) { return {}; }
}
function save(v) {
  try { localStorage.setItem("nr-procedures", JSON.stringify(v)); } catch (_) { /* ignore */ }
}

let modal = null, current = null, getT = null;
const manual = load();

function renderModal() {
  if (!modal || !current) return;
  const t = getT();
  const p = PROCEDURES.find(x => x.id === current);
  const checks = manual[p.id] || {};
  const list = modal.querySelector(".proc-steps");
  list.replaceChildren(...p.steps.map(([text, auto], i) => {
    const live = auto && t ? !!auto(t) : false;
    const done = auto ? live : !!checks[i];
    const box = h("button", { class: `proc-box ${done ? "done" : ""} ${auto ? "auto" : ""}`, title: auto ? "Checked automatically from plant indications" : "Click to check off",
      onclick: () => {
        if (auto) return;
        play("button");
        manual[p.id] = { ...checks, [i]: !checks[i] };
        save(manual);
        renderModal();
      } }, done ? "✓" : "");
    return h("li", { class: done ? "done" : "" }, box, h("span", {}, text), auto ? h("span", { class: "proc-auto" }, "AUTO") : null);
  }));
  modal.querySelector(".proc-title").textContent = `${p.code}  ${p.title}`;
  modal.querySelectorAll(".proc-tab").forEach(b => b.classList.toggle("on", b.dataset.id === current));
}

export function openProcedure(id, telemetryGetter) {
  getT = telemetryGetter;
  current = id;
  play("cover");
  if (!modal) {
    modal = h("div", { class: "proc-overlay", onclick: e => { if (e.target === modal) close(); } },
      h("div", { class: "clipboard-sheet" },
        h("div", { class: "clip" }),
        h("div", { class: "proc-tabs" }, ...PROCEDURES.map(p => h("button", { class: "proc-tab", "data-id": p.id, onclick: () => { current = p.id; play("detent"); renderModal(); } }, p.code))),
        h("div", { class: "proc-title" }),
        h("ol", { class: "proc-steps" }),
        h("div", { class: "proc-foot" },
          h("button", { class: "btn", onclick: () => { manual[current] = {}; save(manual); renderModal(); } }, "Clear manual checks"),
          h("button", { class: "btn", onclick: close }, "Close"))));
    document.body.append(modal);
    setInterval(renderModal, 1000);
  }
  modal.style.display = "flex";
  renderModal();
}

function close() {
  if (modal) modal.style.display = "none";
  current = null;
  play("cover");
}

/** Clipboard hanging off the bottom edge of a panel. */
export function clipboard(panelId, telemetryGetter) {
  const procs = PROCEDURES.filter(p => p.tab === panelId);
  const first = procs[0]?.id ?? PROCEDURES[0].id;
  return h("button", { class: "clipboard", title: "Operating procedures", onclick: () => openProcedure(first, telemetryGetter) },
    h("div", { class: "clip" }), h("div", { class: "clipboard-text" }, "PROCEDURES"),
    h("div", { class: "clipboard-sub" }, procs.map(p => p.code).join(" · ") || "ALL"));
}
