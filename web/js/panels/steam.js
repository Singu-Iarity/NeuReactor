import { h, plate, roundMeter, edgewise, readout, maStation } from "../widgets.js";

export const id = "steam";
export const title = "Steam & Feed";
export const windows = [
  ["sg_lvl_dev", "SG LEVEL DEVIATION"], ["sg_lvl_low", "SG LEVEL LOW"], ["sg_arv", "SG RELIEF VALVE OPEN"], ["dump_open", "STEAM DUMP OPEN"],
];

export function build(root, ctx) {
  const lvl = roundMeter("SG LEVEL (NR)", { min: 0, max: 100, unit: "%", bands: [{ from: 0, to: 12, c: "red" }, { from: 75, to: 100, c: "yellow" }] });
  const press = roundMeter("STEAM PRESSURE", { min: 4, max: 9, unit: "MPa", major: 5, digits: 2, bands: [{ from: 7.85, to: 8.3, c: "yellow" }, { from: 8.3, to: 9, c: "red" }] });
  const steam = edgewise("STEAM FLOW", { min: 0, max: 2500, unit: "kg/s", major: 5, digits: 0 });
  const feed = edgewise("FEED FLOW", { min: 0, max: 2500, unit: "kg/s", major: 5, digits: 0 });
  const ftemp = edgewise("FEED TEMP", { min: 0, max: 300, unit: "°C", major: 6, digits: 0 });
  const total = readout("TOTAL STEAM FLOW [kg/s]", { digits: 0 });
  const fwMa = maStation("SG FEEDWATER (3-ELEMENT)", {
    module: "feedwater_level", pvTag: "sg_level_pct", spKey: "level_pct", spStep: 1, devSpan: 10,
    outTag: "feed_demand_pct", outCmd: v => ({ type: "set_feedwater", fraction: v / 100 }), outStep: 1,
  }, ctx);
  const dumpMa = maStation("STEAM DUMP", {
    module: "steam_dump", pvTag: "tavg_c", spKey: "noload_tavg_c", spStep: 0.5, devSpan: 5,
    outTag: "steam_dump_pct", outCmd: v => ({ type: "set_steam_dump", fraction: v / 100 }), outStep: 1,
  }, ctx);

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide" }, plate("STEAM GENERATORS"),
      h("div", { class: "row" }, lvl.el, press.el, h("div", { class: "col" }, total.el, steam.el, feed.el, ftemp.el))),
    h("section", { class: "sect" }, plate("FEEDWATER"), fwMa.el),
    h("section", { class: "sect" }, plate("STEAM DUMP TO CONDENSER"), dumpMa.el)));

  return t => {
    const R = t.readings;
    lvl.update(R.sg_level_pct);
    press.update(R.sg_pressure_mpa);
    steam.update(R.steam_flow_kg_s);
    feed.update(R.feed_flow_kg_s);
    ftemp.update(R.feed_temp_c);
    total.update(R.steam_flow_kg_s);
    fwMa.update(t);
    dumpMa.update(t);
  };
}
