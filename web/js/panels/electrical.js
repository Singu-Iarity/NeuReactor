// Electrical distribution: switchyard, station service transformers, 6.9 kV
// buses, safety buses, emergency diesels and the 125 V DC system.
//
//   GRID ── SUT ──[bus_a_sut]──┐            ┌──[bus_b_uat]── UAT ── GEN
//                              BUS A ─[tie]─ BUS B
//                 GEN ── UAT ──[bus_a_uat]──┘
//   BUS A ─[safety1_feed]─ SAFETY 1 ─[safety_tie]─ SAFETY 2 ─[safety2_feed]─ BUS B
//                 EDG 1 ─[edg1]─┘                   └─[edg2]─ EDG 2
//   SAFETY 1/2 ── chargers ── 125 V DC ── battery

import { rating } from "../rating.js";
import { h, plate, roundMeter, edgewise, readout, jhandle, lamp, pushbutton, vbar, maStation } from "../widgets.js";
import { api, toast } from "../api.js";
import { play } from "../sound.js";

export const id = "electrical";
export const title = "Electrical";
export const cols = 6;
export const windows = [
  ["loop", "LOSS OF OFFSITE POWER"], ["sut_trip", "STARTUP XFMR TRIP / LOCKOUT"], ["sut_overload", "STARTUP XFMR OVERLOAD"], ["uat_trip", "UNIT AUX XFMR TRIP / LOCKOUT"], ["uat_overload", "UNIT AUX XFMR OVERLOAD"], ["importing", "IMPORTING GRID POWER"],
  ["bus_a_dead", "6.9 kV BUS A DE-ENERGISED"], ["bus_b_dead", "6.9 kV BUS B DE-ENERGISED"], ["s1_uv", "SAFETY BUS 1 UNDERVOLTAGE"], ["s2_uv", "SAFETY BUS 2 UNDERVOLTAGE"], ["tie_closed", "BUS A-B TIE CLOSED"], ["batt_discharge", "BATTERY DISCHARGING"],
  ["edg1_run", "EDG 1 RUNNING"], ["edg2_run", "EDG 2 RUNNING"], ["edg1_fuel", "EDG 1 FUEL LOW"], ["edg2_fuel", "EDG 2 FUEL LOW"], ["dc_low", "125 V DC BUS LOW VOLTAGE"], ["batt_low", "STATION BATTERY LOW"],
  ["abt_auto", "AUTO BUS TRANSFER IN SERVICE"], ["ups_battery", "VITAL BUS ON UPS / BATTERY"], ["", ""], ["", ""], ["", ""], ["", ""],
];

const SVGNS = "http://www.w3.org/2000/svg";
const s = (tag, attrs = {}) => {
  const e = document.createElementNS(SVGNS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
};

/** Breakers: name → [label, x, y] on the mimic. */
const BREAKERS = {
  sut: ["SUT", 70, 50],
  bus_a_sut: ["52-A1", 130, 120],
  bus_a_uat: ["52-A2", 230, 120],
  bus_b_uat: ["52-B1", 390, 120],
  tie_a_b: ["52-T", 310, 180],
  safety1_feed: ["52-S1", 170, 240],
  safety2_feed: ["52-S2", 450, 240],
  safety_tie: ["52-ST", 310, 300],
  edg1: ["52-D1", 120, 340],
  edg2: ["52-D2", 500, 340],
};

/** Conductors: [segments, energised-by]. Energisation keys come from the telemetry. */
const WIRES = [
  ["M70,22 L70,40", "grid"],
  ["M70,60 L70,90 L130,90 L130,110", "sut"],
  ["M130,130 L130,180", "bus_a"],
  ["M300,22 L300,70", "gen"],
  ["M300,70 L230,70 L230,110", "uat"],
  ["M300,70 L390,70 L390,110", "uat"],
  ["M230,130 L230,180", "bus_a"],
  ["M390,130 L390,180", "bus_b"],
  ["M100,180 L300,180", "bus_a"],
  ["M320,180 L520,180", "bus_b"],
  ["M170,180 L170,230", "bus_a"],
  ["M450,180 L450,230", "bus_b"],
  ["M170,250 L170,300", "safety1"],
  ["M450,250 L450,300", "safety2"],
  ["M100,300 L300,300", "safety1"],
  ["M320,300 L520,300", "safety2"],
  ["M120,300 L120,330", "safety1"],
  ["M500,300 L500,330", "safety2"],
  ["M120,350 L120,372", "edg1"],
  ["M500,350 L500,372", "edg2"],
  ["M240,300 L240,340 L310,340 L310,360", "dc"],
  ["M380,300 L380,340 L310,340", "dc"],
];

const LABELS = [
  ["GRID 345 kV", 70, 14], ["MAIN GEN", 300, 14], ["SUT", 92, 82], ["UAT", 300, 62],
  ["BUS A", 96, 184, "end"], ["6.9 kV", 96, 172, "end"], ["BUS B", 524, 184, "start"], ["6.9 kV", 524, 172, "start"],
  ["SAFETY 1", 96, 304, "end"], ["4.16 kV", 96, 292, "end"], ["SAFETY 2", 524, 304, "start"], ["4.16 kV", 524, 292, "start"],
  ["EDG 1", 120, 386], ["EDG 2", 500, 386], ["125 V DC / BATTERY", 310, 376],
];

function mimic(onClick) {
  const svg = s("svg", { viewBox: "0 0 600 395", class: "oneline" });
  const wires = WIRES.map(([d, key]) => {
    const p = s("path", { d, class: "ol-wire" });
    svg.append(p);
    return { p, key };
  });
  for (const [t, x, y, anchor = "middle"] of LABELS) {
    const e = s("text", { x, y, class: "ol-label", "text-anchor": anchor });
    e.textContent = t;
    svg.append(e);
  }
  const sources = {
    grid: s("circle", { cx: 70, cy: 30, r: 8, class: "ol-src" }),
    gen: s("circle", { cx: 300, cy: 30, r: 9, class: "ol-src" }),
    edg1: s("circle", { cx: 120, cy: 360, r: 9, class: "ol-src" }),
    edg2: s("circle", { cx: 500, cy: 360, r: 9, class: "ol-src" }),
  };
  Object.values(sources).forEach(c => svg.append(c));
  const bat = s("rect", { x: 296, y: 356, width: 28, height: 10, class: "ol-src" });
  svg.append(bat);
  const brk = {};
  for (const [name, [label, x, y]] of Object.entries(BREAKERS)) {
    const g = s("g", { class: "ol-brk", transform: `translate(${x},${y})` });
    const r = s("rect", { x: -10, y: -10, width: 20, height: 20, rx: 2 });
    const t = s("text", { x: 14, y: 4, class: "ol-tag" });
    t.textContent = label;
    g.append(r, t);
    g.addEventListener("click", () => onClick(name));
    svg.append(g);
    brk[name] = g;
  }
  return {
    el: svg,
    update(e, gen) {
      const live = {
        grid: e.grid_available,
        sut: e.grid_available && !e.sut_tripped && e.breakers.sut,
        gen, uat: gen && !e.uat_tripped,
        bus_a: e.buses.bus_a, bus_b: e.buses.bus_b,
        safety1: e.buses.safety1, safety2: e.buses.safety2,
        edg1: e.diesels[0].state === "running", edg2: e.diesels[1].state === "running",
        dc: e.buses.dc,
      };
      for (const w of wires) w.p.classList.toggle("live", !!live[w.key]);
      for (const [k, c] of Object.entries(sources)) c.classList.toggle("live", !!live[k]);
      bat.classList.toggle("live", e.buses.dc);
      bat.classList.toggle("discharging", !e.buses.safety1 && !e.buses.safety2);
      for (const [k, g] of Object.entries(brk)) g.classList.toggle("closed", !!e.breakers[k]);
    },
  };
}

export function build(root, ctx) {
  const setBreaker = (breaker, closed) => ctx.send({ type: "set_elec_breaker", breaker, closed });
  const one = mimic(name => {
    const e = ctx.t()?.electrical;
    if (!e) return;
    play("jhandle");
    setBreaker(name, !e.breakers[name]);
  });

  const sutM = roundMeter("STARTUP XFMR LOAD", { min: 0, max: 80 * rating.scale, unit: "MW", major: 8, digits: 1, bands: [{ from: 50 * rating.scale, to: 60 * rating.scale, c: "yellow" }, { from: 60 * rating.scale, to: 80 * rating.scale, c: "red" }] });
  const uatM = roundMeter("UNIT AUX XFMR LOAD", { min: 0, max: 120 * rating.scale, unit: "MW", major: 6, digits: 1, bands: [{ from: 80 * rating.scale, to: 96 * rating.scale, c: "yellow" }, { from: 96 * rating.scale, to: 120 * rating.scale, c: "red" }] });
  const house = readout("HOUSE LOAD [MW]", { digits: 1, size: "big" });
  const busV = [
    ["bus_a_kv", "6.9 kV BUS A", 8], ["bus_b_kv", "6.9 kV BUS B", 8],
    ["safety1_kv", "4.16 kV SAFETY 1", 5], ["safety2_kv", "4.16 kV SAFETY 2", 5],
  ].map(([tag, label, max]) => ({ tag, m: edgewise(label, { min: 0, max, unit: "kV", major: 4, digits: 2, bands: [{ from: 0, to: max * 0.8, c: "yellow" }] }) }));
  const imp = readout("GRID IMPORT [MW]", { digits: 1 });
  const reset = pushbutton("XFMR LOCKOUT RESET (86)", { color: "amber", onPress: () => ctx.send({ type: "reset_transformers" }) });

  const brkSw = Object.entries(BREAKERS).map(([name, [label]]) => [name, jhandle(`${label} ${name.replace(/_/g, " ").toUpperCase()}`, {
    left: "TRIP", right: "CLOSE",
    onLeft: () => setBreaker(name, false),
    onRight: () => setBreaker(name, true),
  })]);

  // ---- diesels
  const edg = [0, 1].map(i => {
    const run = jhandle(`EDG ${i + 1}`, {
      left: "STOP", right: "START",
      onLeft: () => ctx.send({ type: "set_diesel", index: i, running: false }),
      onRight: () => ctx.send({ type: "set_diesel", index: i, running: true }),
    });
    const ready = lamp("white", "UP TO SPEED");
    const fuel = edgewise(`EDG ${i + 1} FUEL`, { min: 0, max: 100, unit: "%", major: 5, digits: 0, bands: [{ from: 0, to: 20, c: "yellow" }] });
    const load = readout(`EDG ${i + 1} LOAD [MW]`, { digits: 2 });
    return { run, ready, fuel, load, el: h("div", { class: "col" }, run.el, ready.el, fuel.el, load.el) };
  });
  const seq = maStation("DIESEL LOAD SEQUENCER", { module: "diesel_sequencer", pvTag: "edg_load_mw", digits: 2 }, ctx);
  const abt = maStation("AUTO BUS TRANSFER", { module: "bus_transfer", pvTag: "house_load_mw", digits: 1 }, ctx);
  const battCap = readout("BATTERY CAPACITY [h]", { digits: 1 });
  const battLeft = readout("TIME ON BATTERY LEFT [h]", { format: v => (v === null ? "—" : v.toFixed(1)) });
  const indLamp = lamp("white", "VITAL BUS / ANNUNCIATOR POWER");
  const fuelBtn = pushbutton("ORDER DIESEL FUEL", {
    onPress: async () => {
      const t = ctx.t();
      if (t?.game_mode === "career") {
        const r = await api("/career/diesel_fuel", {});
        if (!r.ok) { toast(r.error); play("deny"); } else play("pickup");
      } else {
        ctx.send({ type: "refuel_diesels" });
      }
    },
  });

  // ---- DC
  const batt = vbar("BATTERY CHARGE", { max: 100, digits: 0 });
  const dcV = edgewise("125 V DC BUS", { min: 90, max: 140, unit: "V", major: 5, digits: 1, bands: [{ from: 90, to: 108, c: "red" }, { from: 108, to: 118, c: "yellow" }] });
  const charger = lamp("white", "CHARGERS ON");
  const dcOk = lamp("green", "DC BUS NORMAL");

  // ---- instructor (sandbox)
  const loop = pushbutton("LOSS OF OFFSITE POWER", { color: "amber", onPress: () => {
    const t = ctx.t();
    const lost = !t?.electrical?.grid_available;
    if (lost || confirm("Simulate a loss of offsite power?")) ctx.send({ type: "set_offsite_power", available: lost });
  } });

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "electrical.oneline" }, plate("STATION ELECTRICAL ONE-LINE"),
      h("div", { class: "row" }, one.el,
        h("div", { class: "col" }, sutM.el, uatM.el),
        h("div", { class: "col" }, house.el, imp.el, reset.el, loop.el)),
      h("p", { class: "note" }, "Bus A can be fed from the startup transformer; bus B only from the unit auxiliary transformer (generator on-line) or through the A-B tie. Starting several large motors at once can overload the startup transformer. Click a breaker on the mimic or use its control switch."),
      plate("BUS VOLTMETERS", "small"),
      h("div", { class: "row" }, ...busV.map(b => b.m.el), abt.el),
      h("p", { class: "note" }, "Fast bus transfer to the startup transformer on a generator trip is built-in protective relaying. The Auto Bus Transfer module adds recovery: re-energise the buses when offsite power returns, put the safety buses back on their normal feeds (stopping the diesels), and move house load to the unit auxiliary transformer once the unit is above 15 % load."),
      plate("BREAKER CONTROL SWITCHES", "small"),
      h("div", { class: "row tight wrap" }, ...brkSw.map(([, sw]) => sw.el))),
    h("section", { class: "sect", "data-section": "electrical.diesels" }, plate("EMERGENCY DIESEL GENERATORS"),
      h("div", { class: "row" }, edg[0].el, edg[1].el, h("div", { class: "col" }, seq.el, fuelBtn.el))),
    h("section", { class: "sect", "data-section": "electrical.dc" }, plate("125 V DC & BATTERY"),
      h("div", { class: "row" }, batt.el, dcV.el, h("div", { class: "col" }, charger.el, dcOk.el, indLamp.el, battCap.el, battLeft.el)),
      h("p", { class: "note" }, "Annunciators and indicating lamps run from the vital instrument bus. Without Vital AC UPS inverters it is fed from the safety buses and goes dark on a loss of AC until a diesel picks up; with them it rides through on the battery. Better batteries (Career shop) last longer."))));

  return (t, visible) => {
    const e = t.electrical;
    if (!e || !visible) return;
    const R = t.readings;
    one.update(e, t.generator_breaker);
    const q3 = (e.transformer_quality ?? 1) ** 3;
    sutM.update(R.sut_load_mw ?? e.sut_load_mw, e.sut_rating * q3);
    uatM.update(R.uat_load_mw ?? e.uat_load_mw, e.uat_rating * q3);
    house.update(e.house_load_mw);
    busV.forEach(b => b.m.update(R[b.tag] ?? 0));
    imp.update(e.import_mw);
    reset.update(e.sut_tripped || e.uat_tripped);
    loop.update(!e.grid_available, t.game_mode === "career");
    for (const [name, sw] of brkSw) sw.update(!!e.breakers[name]);
    e.diesels.forEach((d, i) => {
      const x = edg[i];
      x.run.update(d.state !== "stopped");
      x.ready.update(d.state === "running", d.state === "starting");
      x.fuel.update(100 * d.fuel_l / d.fuel_capacity_l);
      x.load.update(d.load_mw);
    });
    seq.update(t);
    abt.update(t);
    battCap.update(e.battery_hours ?? 2);
    const discharging = !e.buses.safety1 && !e.buses.safety2;
    battLeft.update(discharging ? (e.battery_pct / 100) * (e.battery_hours ?? 2) : null);
    indLamp.update(t.indication_power !== false);
    const full = e.diesels.every(d => d.fuel_l >= d.fuel_capacity_l - 1);
    fuelBtn.update(false, full);
    batt.update(e.battery_pct);
    dcV.update(e.dc_volts);
    charger.update(e.buses.safety1 || e.buses.safety2);
    dcOk.update(e.buses.dc && e.dc_volts > 118);
  };
}
