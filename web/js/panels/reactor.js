import { h, plate, roundMeter, edgewise, readout, vbar, lamp, pushbutton, selector, springSwitch, maStation } from "../widgets.js";

export const id = "reactor";
export const title = "Reactor";
export const windows = [
  ["rx_trip", "REACTOR TRIP"], ["sur", "HIGH STARTUP RATE"], ["rod_ins", "ROD INSERTION LIMIT"], ["sr_block_permitted", "SR BLOCK PERMITTED"],
  ["p6", "P-6 IR PERMISSIVE"], ["p10", "P-10 PR PERMISSIVE"], ["", ""], ["", ""],
  ["sr_blocked", "SR TRIP BLOCKED / HV OFF"], ["ir_blocked", "IR TRIP BLOCKED"], ["pr_low_blocked", "PR LO SETPT TRIP BLOCKED"], ["", ""],
];

const FIRST_OUT = [
  "Source range high flux", "Intermediate range high flux", "Power range high flux (low setpoint)", "Power range high flux (high setpoint)",
  "Pressurizer pressure high", "Pressurizer pressure low", "Pressurizer level high", "RCS low flow",
  "SG level low-low", "Overtemperature ΔT", "Turbine trip above P-9", "Manual reactor trip",
];

export function build(root, ctx) {
  // ---- nuclear instrumentation
  const sr = roundMeter("SOURCE RANGE", { min: 1, max: 1e6, log: true, unit: "cps", bands: [{ from: 1e5, to: 1e6, c: "red" }] });
  const srSur = edgewise("SR STARTUP RATE", { min: -1, max: 5, unit: "dpm", major: 6, bands: [{ from: 1, to: 5, c: "yellow" }], digits: 2 });
  const ir = roundMeter("INTERMEDIATE RANGE", { min: 1e-11, max: 1e-3, log: true, unit: "A", bands: [{ from: 2e-4, to: 1e-3, c: "red" }] });
  const irSur = edgewise("IR STARTUP RATE", { min: -1, max: 5, unit: "dpm", major: 6, bands: [{ from: 1, to: 5, c: "yellow" }], digits: 2 });
  const pr = ["N41", "N42", "N43", "N44"].map(n => edgewise(`PR ${n}`, { min: 0, max: 120, unit: "%", major: 6, bands: [{ from: 100, to: 109, c: "yellow" }, { from: 109, to: 120, c: "red" }] }));
  const hvLamp = lamp("red", "SR HV ON"), p6Lamp = lamp("white", "P-6"), p10Lamp = lamp("white", "P-10");
  const block = (channel, blocked) => ctx.send({ type: "block_nis", channel, blocked });
  const srBlk = pushbutton("SR BLOCK", { color: "amber", onPress: () => block("source_range", true) });
  const srRst = pushbutton("SR RESET", { onPress: () => block("source_range", false) });
  const irBlk = pushbutton("IR BLOCK", { color: "amber", onPress: () => block("intermediate_range", true) });
  const irRst = pushbutton("IR RESET", { onPress: () => block("intermediate_range", false) });
  const prBlk = pushbutton("PR LO BLOCK", { color: "amber", onPress: () => block("power_range_low", true) });
  const prRst = pushbutton("PR LO RESET", { onPress: () => block("power_range_low", false) });
  const avg = readout("AVG POWER RANGE [%]", { digits: 1 });
  const period = readout("REACTOR PERIOD [s]", { format: v => (Math.abs(v) < 0.03 ? "Inf" : (26.06 / v).toFixed(0)) });
  const rho = readout("REACTIVITY [pcm]", { digits: 0 });
  const mwt = readout("THERMAL POWER [MWt]", { digits: 0 });
  let speaker = false;
  const spk = pushbutton("SR SPEAKER", { onPress: () => { speaker = !speaker; ctx.audioCounts(speaker); } });

  // ---- rod control
  const bars = ["SA", "SB", "A", "B", "C", "D"].map(b => vbar(b, { max: 228 }));
  let selection = "manual";
  const sel = selector("BANK SELECT", [{ label: "MAN", value: "manual" }, "SA", "SB", "A", "B", "C", "D"], v => { selection = v; sel.update(v); });
  sel.update(selection);
  const lever = springSwitch("ROD MOTION", {
    lo: "IN", hi: "OUT",
    onHold: dir => ctx.send({ type: "rod_motion", selection, direction: dir > 0 ? "out" : "in" }),
    onRelease: () => ctx.send({ type: "rod_motion", selection, direction: "hold" }),
  });
  const rodMa = maStation("ROD CONTROL (Tavg)", { module: "rod_control", pvTag: "tavg_c", spTag: "tref_c", spKey: "tavg_full_c", spStep: 0.1, devSpan: 3 }, ctx);
  const rxReset = pushbutton("RX TRIP RESET", { onPress: () => ctx.send({ type: "reset_trip" }) });

  // ---- first out
  const fo = h("div", { class: "firstout" }, ...FIRST_OUT.map(n => h("div", { class: "fo-win", "data-trip": n }, n.toUpperCase())));

  root.append(
    h("div", { class: "board" },
      h("section", { class: "sect wide" }, plate("NUCLEAR INSTRUMENTATION"),
        h("div", { class: "row" },
          h("div", { class: "col" }, sr.el, srSur.el, h("div", { class: "row tight" }, hvLamp.el, srBlk.el, srRst.el, spk.el)),
          h("div", { class: "col" }, ir.el, irSur.el, h("div", { class: "row tight" }, p6Lamp.el, irBlk.el, irRst.el)),
          h("div", { class: "col" }, ...pr.map(p => p.el), h("div", { class: "row tight" }, p10Lamp.el, prBlk.el, prRst.el)),
          h("div", { class: "col" }, avg.el, period.el, rho.el, mwt.el))),
      h("section", { class: "sect" }, plate("ROD CONTROL"),
        h("div", { class: "rods" }, ...bars.map(b => b.el)),
        h("div", { class: "row" }, sel.el, lever.el, rodMa.el),
        h("div", { class: "row tight" }, rxReset.el, ctx.scramPair())),
      h("section", { class: "sect" }, plate("REACTOR TRIP FIRST OUT"), fo)));

  return t => {
    const R = t.readings;
    sr.update(Math.max(R.sr_cps, 1));
    srSur.update(R.sr_sur_dpm);
    ir.update(R.ir_amps);
    irSur.update(R.ir_sur_dpm);
    pr.forEach((p, i) => p.update(R[`pr_n4${i + 1}_pct`]));
    hvLamp.update(t.nis.sr_hv_on);
    p6Lamp.update(t.nis.p6);
    p10Lamp.update(t.nis.p10);
    srBlk.update(t.nis.sr_blocked, !t.nis.p6);
    irBlk.update(t.nis.ir_blocked, !t.nis.p10);
    prBlk.update(t.nis.pr_low_blocked, !t.nis.p10);
    srRst.update(false); irRst.update(false); prRst.update(false);
    spk.update(speaker);
    avg.update(R.nuclear_power_pct);
    period.update(R.startup_rate_dpm);
    rho.update(R.reactivity_pcm);
    mwt.update(R.thermal_power_mw);
    t.rods.forEach((r, i) => bars[i].update(r.position, r.target));
    const manual = ctx.moduleMode("rod_control") === "manual";
    lever.update(!manual || t.reactor_tripped);
    sel.update(selection, !manual);
    rodMa.update(t);
    rxReset.update(t.reactor_tripped);
    fo.querySelectorAll(".fo-win").forEach(w => {
      const i = t.trips.indexOf(w.dataset.trip);
      w.className = `fo-win ${i === 0 ? "first" : i > 0 ? "on" : ""}`;
    });
    ctx.countRate(R.sr_cps);
  };
}
