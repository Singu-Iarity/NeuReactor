import { h, plate, roundMeter, edgewise, lamp, jhandle, springSwitch, maStation } from "../widgets.js";

export const id = "rcs";
export const title = "RCS";
export const windows = [
  ["p_high", "PZR PRESS HIGH"], ["p_low", "PZR PRESS LOW"], ["porv", "PZR PORV OPEN"], ["pzr_lvl_dev", "PZR LEVEL DEVIATION"],
  ["tavg_dev", "TAVG / TREF DEVIATION"], ["rcs_low_flow", "RCS LOW FLOW"], ["rcp", "RCP STOPPED"], ["", ""],
];

export function build(root, ctx) {
  const tavg = roundMeter("TAVG", { min: 270, max: 330, unit: "°C", major: 6, ref: true, bands: [{ from: 318, to: 330, c: "red" }] });
  const press = roundMeter("PZR PRESSURE", { min: 12, max: 18, unit: "MPa", major: 6, digits: 2, bands: [{ from: 16.2, to: 17.2, c: "yellow" }, { from: 17.2, to: 18, c: "red" }, { from: 12, to: 13.7, c: "red" }] });
  const lvl = roundMeter("PZR LEVEL", { min: 0, max: 100, unit: "%", ref: true, bands: [{ from: 92, to: 100, c: "red" }, { from: 0, to: 17, c: "yellow" }] });
  const thot = edgewise("T HOT", { min: 260, max: 340, unit: "°C", major: 8 });
  const tcold = edgewise("T COLD", { min: 260, max: 340, unit: "°C", major: 8 });
  const dt = edgewise("CORE ΔT", { min: 0, max: 120, unit: "%", major: 6, bands: [{ from: 115, to: 120, c: "red" }] });
  const flow = edgewise("RCS FLOW", { min: 0, max: 120, unit: "%", major: 6, bands: [{ from: 0, to: 90, c: "red" }] });
  const pMa = maStation("PZR PRESSURE CTRL", {
    module: "pressurizer_pressure", pvTag: "rcs_pressure_mpa", spKey: "pressure_mpa", spStep: 0.01, digits: 2, devSpan: 0.3,
    outTag: "pzr_heaters_pct", outCmd: v => ({ type: "set_pressurizer_heaters", fraction: v / 100 }), outStep: 2,
  }, ctx);
  const spray = edgewise("PZR SPRAY VALVE", { min: 0, max: 100, unit: "%" });
  const sprayRl = springSwitch("SPRAY", {
    lo: "CLOSE", hi: "OPEN", repeat: 250,
    onHold: dir => ctx.send({ type: "set_pressurizer_spray", fraction: Math.max(0, Math.min(1, (ctx.t().readings.pzr_spray_pct + dir * 2) / 100)) }),
  });
  const porv = lamp("red", "PORV OPEN");
  const rcps = [0, 1, 2, 3].map(i => jhandle(`RCP ${i + 1}`, {
    left: "STOP", right: "START",
    onLeft: () => ctx.send({ type: "set_coolant_pump", index: i, running: false }),
    onRight: () => ctx.send({ type: "set_coolant_pump", index: i, running: true }),
  }));

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "rcs.main" }, plate("REACTOR COOLANT SYSTEM"),
      h("div", { class: "row" }, tavg.el, press.el, lvl.el,
        h("div", { class: "col" }, thot.el, tcold.el, dt.el, flow.el))),
    h("section", { class: "sect", "data-section": "rcs.pzr" }, plate("PRESSURIZER"),
      h("div", { class: "row" }, pMa.el, h("div", { class: "col" }, spray.el, sprayRl.el, porv.el))),
    h("section", { class: "sect", "data-section": "rcs.rcp" }, plate("REACTOR COOLANT PUMPS"), h("div", { class: "row" }, ...rcps.map(r => r.el)))));

  return t => {
    const R = t.readings;
    tavg.update(R.tavg_c, R.tref_c);
    press.update(R.rcs_pressure_mpa);
    lvl.update(R.pzr_level_pct, R.pzr_level_program_pct);
    thot.update(R.thot_c); tcold.update(R.tcold_c); dt.update(R.delta_t_pct); flow.update(R.rcs_flow_pct);
    pMa.update(t);
    spray.update(R.pzr_spray_pct);
    sprayRl.update(ctx.moduleMode("pressurizer_pressure") !== "manual");
    porv.update(t.alarms.some(a => a.tag === "porv"));
    rcps.forEach((r, i) => r.update(t.coolant_pumps[i]));
  };
}
