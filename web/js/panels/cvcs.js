import { h, plate, roundMeter, edgewise, readout, selector, springSwitch, maStation } from "../widgets.js";

export const id = "cvcs";
export const title = "CVCS";
export const windows = [
  ["borating", "BORATION IN PROGRESS"], ["diluting", "DILUTION IN PROGRESS"], ["charging_low", "CHARGING FLOW LOW"], ["", ""],
];

export function build(root, ctx) {
  const boron = roundMeter("RCS BORON", { min: 0, max: 2000, unit: "ppm", major: 4, digits: 0 });
  const mode = selector("BORATION MODE", [{ label: "DILUTE", value: "dilute" }, { label: "OFF", value: "off" }, { label: "BORATE", value: "borate" }],
    v => ctx.send({ type: "set_boration", mode: v }));
  const bMa = maStation("AUTO MAKEUP (BORON)", { module: "boron_control", spKey: "band_low_steps", spStep: 5, digits: 0 }, ctx);
  const chMa = maStation("PZR LEVEL / CHARGING", {
    module: "pressurizer_level", pvTag: "pzr_level_pct", spTag: "pzr_level_program_pct", spKey: "level_full_pct", spStep: 0.5, devSpan: 10,
    outTag: "charging_kg_s", outMax: 20, outDigits: 1, outCmd: v => ({ type: "set_charging", kg_s: v }), outStep: 0.5,
  }, ctx);
  const charging = edgewise("CHARGING FLOW", { min: 0, max: 20, unit: "kg/s", major: 4 });
  const letdown = edgewise("LETDOWN FLOW", { min: 0, max: 15, unit: "kg/s", major: 3 });
  const ldRl = springSwitch("LETDOWN", {
    lo: "LESS", hi: "MORE", repeat: 300,
    onHold: dir => ctx.send({ type: "set_letdown", kg_s: Math.max(0, Math.min(15, ctx.t().readings.letdown_kg_s + dir * 0.5)) }),
  });
  const ppm = readout("BORON [ppm]", { digits: 0 });

  root.append(h("div", { class: "board" },
    h("section", { class: "sect" }, plate("BORON CONCENTRATION CONTROL"), h("div", { class: "row" }, boron.el, h("div", { class: "col" }, ppm.el, mode.el), bMa.el)),
    h("section", { class: "sect" }, plate("CHARGING & LETDOWN"), h("div", { class: "row" }, chMa.el, h("div", { class: "col" }, charging.el, letdown.el, ldRl.el)))));

  return t => {
    const R = t.readings;
    boron.update(R.boron_ppm);
    ppm.update(R.boron_ppm);
    mode.update(t.boration, ctx.moduleMode("boron_control") !== "manual");
    bMa.update(t);
    chMa.update(t);
    charging.update(R.charging_kg_s);
    letdown.update(R.letdown_kg_s);
  };
}
