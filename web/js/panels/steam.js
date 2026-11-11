import { h, plate, roundMeter, edgewise, readout, jhandle, maStation } from "../widgets.js";

export const id = "steam";
export const title = "Steam & Feed";
export const cols = 6;
export const windows = [
  ["msiv_open", "MSIVs OPEN"], ["msiv_closed", "MSIVs CLOSED"], ["feed_pumps", "FEED PUMPS RUNNING"], ["dump_armed", "STEAM DUMP ARMED"], ["dump_open", "STEAM DUMP OPEN"], ["dump_blocked", "STEAM DUMP BLOCKED (NO VACUUM)"],
  ["sg_lvl_hi", "SG LEVEL HIGH"], ["sg_lvl_low", "SG LEVEL LOW"], ["sg_lvl_lolo", "SG LEVEL LO-LO"], ["sg_lvl_dev", "SG LEVEL DEVIATION"], ["flow_mismatch", "STEAM / FEED FLOW MISMATCH"], ["sg_arv", "ATMOSPHERIC RELIEF OPEN"],
  ["vac_low", "CONDENSER VACUUM LOW"], ["vac_required", "CONDENSER VACUUM REQUIRED"], ["sjae_on", "SJAEs IN SERVICE"], ["sjae_low", "SJAE MOTIVE STEAM LOW"], ["vac_pumps", "VACUUM PUMPS RUNNING"], ["cw_trip", "CW PUMP OFF"],
  ["hotwell_hi", "HOTWELL LEVEL HIGH"], ["hotwell_lo", "HOTWELL LEVEL LOW"], ["deaeration", "DEAERATION NOT EFFECTIVE"], ["pegging", "DA PEGGING STEAM ON"], ["feed_temp_low", "FEEDWATER TEMP LOW"], ["", ""],
];

export function build(root, ctx) {
  const lvl = roundMeter("SG LEVEL (NR)", { min: 0, max: 100, unit: "%", bands: [{ from: 0, to: 12, c: "red" }, { from: 75, to: 100, c: "yellow" }] });
  const press = roundMeter("STEAM PRESSURE", { min: 4, max: 9, unit: "MPa", major: 5, digits: 2, bands: [{ from: 7.85, to: 8.3, c: "yellow" }, { from: 8.3, to: 9, c: "red" }] });
  const steam = edgewise("STEAM FLOW", { min: 0, max: 2500, unit: "kg/s", major: 5, digits: 0 });
  const feed = edgewise("FEED FLOW", { min: 0, max: 2500, unit: "kg/s", major: 5, digits: 0 });
  const ftemp = edgewise("FEED TEMP", { min: 0, max: 300, unit: "°C", major: 6, digits: 0 });
  const total = readout("TOTAL STEAM FLOW [kg/s]", { digits: 0 });
  const msiv = jhandle("MAIN STEAM ISOLATION", {
    left: "CLOSE", right: "OPEN",
    onLeft: () => { if (confirm("Close the main steam isolation valves?")) ctx.send({ type: "set_msiv", open: false }); },
    onRight: () => ctx.send({ type: "set_msiv", open: true }),
  });
  const fwMa = maStation("SG FEEDWATER (3-ELEMENT)", {
    module: "feedwater_level", pvTag: "sg_level_pct", spKey: "level_pct", spStep: 1, devSpan: 10,
    outTag: "feed_demand_pct", outCmd: v => ({ type: "set_feedwater", fraction: v / 100 }), outStep: 1,
  }, ctx);
  const dumpMa = maStation("STEAM DUMP", {
    module: "steam_dump", pvTag: "tavg_c", spKey: "noload_tavg_c", spStep: 0.5, devSpan: 5,
    outTag: "steam_dump_pct", outCmd: v => ({ type: "set_steam_dump", fraction: v / 100 }), outStep: 1,
  }, ctx);

  // ---- condenser & air removal
  const vac = roundMeter("CONDENSER PRESSURE", { min: 0, max: 105, unit: "kPa abs", major: 7, digits: 1, bands: [{ from: 0, to: 10, c: "green" }, { from: 25, to: 30, c: "yellow" }, { from: 30, to: 105, c: "red" }] });
  const condT = edgewise("CONDENSING TEMP", { min: 0, max: 110, unit: "°C", major: 11, digits: 1 });
  const cwFlow = edgewise("CIRC WATER FLOW", { min: 0, max: 120, unit: "%", major: 6, digits: 0 });
  const hotwell = edgewise("HOTWELL LEVEL", { min: 0, max: 100, unit: "%", bands: [{ from: 0, to: 25, c: "yellow" }, { from: 75, to: 100, c: "yellow" }] });
  const cw = [0, 1, 2].map(i => jhandle(`CW PUMP ${i + 1}`, {
    left: "STOP", right: "START",
    onLeft: () => ctx.send({ type: "set_cw_pump", index: i, running: false }),
    onRight: () => ctx.send({ type: "set_cw_pump", index: i, running: true }),
  }));
  const vp = [0, 1].map(i => jhandle(`VACUUM PUMP ${i + 1}`, {
    left: "STOP", right: "START",
    onLeft: () => ctx.send({ type: "set_vacuum_pump", index: i, running: false }),
    onRight: () => ctx.send({ type: "set_vacuum_pump", index: i, running: true }),
  }));
  const sjae = jhandle("SJAE STEAM", {
    left: "CLOSE", right: "OPEN",
    onLeft: () => ctx.send({ type: "set_sjae", on: false }),
    onRight: () => ctx.send({ type: "set_sjae", on: true }),
  });
  const vacMa = maStation("CONDENSER VACUUM SEQ.", { module: "condenser_vacuum", pvTag: "condenser_kpa", digits: 1 }, ctx);

  // ---- deaerator
  const daP = roundMeter("DEAERATOR PRESSURE", { min: 0, max: 1.2, unit: "MPa", major: 6, digits: 3, bands: [{ from: 0, to: 0.15, c: "yellow" }] });
  const daT = edgewise("DEAERATOR TEMP", { min: 0, max: 200, unit: "°C", major: 4, digits: 0, bands: [{ from: 0, to: 111, c: "yellow" }] });
  const peg = jhandle("PEGGING STEAM", {
    left: "CLOSE", right: "OPEN",
    onLeft: () => ctx.send({ type: "set_pegging_steam", open: false }),
    onRight: () => ctx.send({ type: "set_pegging_steam", open: true }),
  });
  const daMa = maStation("DEAERATOR CONTROL", { module: "deaerator", pvTag: "da_pressure_mpa", digits: 3 }, ctx);

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "steam.sg" }, plate("STEAM GENERATORS"),
      h("div", { class: "row" }, lvl.el, press.el, h("div", { class: "col" }, total.el, steam.el, feed.el, ftemp.el), msiv.el)),
    h("section", { class: "sect", "data-section": "steam.feed" }, plate("FEEDWATER"), fwMa.el),
    h("section", { class: "sect", "data-section": "steam.dump" }, plate("STEAM DUMP TO CONDENSER"), dumpMa.el),
    h("section", { class: "sect wide", "data-section": "steam.condenser" }, plate("CONDENSER & AIR REMOVAL"),
      h("p", { class: "note" }, "Pull vacuum with the vacuum (hogging) pumps, then put the SJAEs in service (they need main steam above 1.5 MPa) and stop the pumps. The turbine cannot be latched above 25 kPa and trips above 30 kPa; steam dumps need vacuum."),
      h("div", { class: "row" }, vac.el, h("div", { class: "col" }, condT.el, cwFlow.el, hotwell.el),
        h("div", { class: "col" }, h("div", { class: "row tight" }, ...cw.map(c => c.el)), h("div", { class: "row tight" }, ...vp.map(v => v.el), sjae.el)),
        vacMa.el)),
    h("section", { class: "sect wide", "data-section": "steam.deaerator" }, plate("DEAERATOR"),
      h("div", { class: "row" }, daP.el, h("div", { class: "col" }, daT.el), peg.el, daMa.el))));

  return t => {
    const R = t.readings;
    lvl.update(R.sg_level_pct);
    press.update(R.sg_pressure_mpa);
    steam.update(R.steam_flow_kg_s);
    feed.update(R.feed_flow_kg_s);
    ftemp.update(R.feed_temp_c);
    total.update(R.steam_flow_kg_s);
    msiv.update(t.msiv_open);
    fwMa.update(t);
    dumpMa.update(t);
    vac.update(R.condenser_kpa);
    condT.update(R.condensing_temp_c);
    cwFlow.update(R.cw_flow_pct);
    hotwell.update(R.hotwell_level_pct);
    const vacAuto = ctx.moduleMode("condenser_vacuum") !== "manual";
    cw.forEach((c, i) => c.update(t.cw_pumps[i], vacAuto));
    vp.forEach((v, i) => v.update(t.vacuum_pumps[i], vacAuto));
    sjae.update(t.sjae_on, vacAuto);
    vacMa.update(t);
    daP.update(R.da_pressure_mpa);
    daT.update(R.da_temp_c);
    peg.update(t.pegging_steam, ctx.moduleMode("deaerator") !== "manual");
    daMa.update(t);
  };
}
