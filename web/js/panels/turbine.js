import { h, plate, roundMeter, edgewise, readout, lamp, jhandle, selector, pushbutton, synchroscope, maStation } from "../widgets.js";

export const id = "turbine";
export const title = "Turbine-Generator";
export const windows = [
  ["turb_trip", "TURBINE TRIP"], ["overspeed", "TURBINE OVERSPEED"], ["reverse_power", "GEN REVERSE POWER"], ["gen_lockout", "GEN LOCKOUT 86G"],
  ["gen_damage", "GENERATOR DAMAGE"], ["field_open", "FIELD BREAKER OPEN"], ["sync_blocked", "PHASE Δ OUT OF RANGE"], ["ready_sync", "READY TO SYNC"],
];

const ACCEL = { S: 60, M: 150, F: 300 };
const PRESETS = [["STOP", 0], ["500", 500], ["1000", 1000], ["1500", 1500], ["1800", 1800]];

export function build(root, ctx) {
  const load = readout("GENERATOR LOAD [MWe]", { digits: 0, size: "big" });
  const rpmR = readout("TURBINE SPEED [rpm]", { digits: 0 });
  const steamP = readout("MAIN STEAM PRESS [MPa]", { digits: 2 });
  const speed = roundMeter("TURBINE SPEED", { min: 0, max: 2100, unit: "rpm", major: 7, digits: 0, bands: [{ from: 1890, to: 1980, c: "yellow" }, { from: 1980, to: 2100, c: "red" }] });
  const valve = roundMeter("CONTROL VALVE", { min: 0, max: 100, unit: "%", digits: 1 });
  const fine = edgewise("SPEED (SYNC RANGE)", { min: 1770, max: 1830, unit: "rpm", major: 6, digits: 1, bands: [{ from: 1797, to: 1803, c: "blue" }] });
  const genV = edgewise("GENERATOR VOLTS", { min: 0, max: 26, unit: "kV", major: 13, digits: 2 });
  const busV = edgewise("GRID / BUS VOLTS", { min: 0, max: 26, unit: "kV", major: 13, digits: 2 });
  const genF = edgewise("GENERATOR FREQ", { min: 59, max: 61, unit: "Hz", major: 4, digits: 3 });
  const busF = edgewise("GRID FREQ", { min: 59, max: 61, unit: "Hz", major: 4, digits: 3 });
  const health = edgewise("GENERATOR CONDITION", { min: 0, max: 100, unit: "%", bands: [{ from: 0, to: 50, c: "red" }, { from: 50, to: 90, c: "yellow" }] });

  const sp = (key, value) => ctx.send({ type: "set_setpoint", module: "turbine_governor", key, value });
  const accel = selector("ACCELERATION", ["S", "M", "F"], v => sp("accel_rpm_min", ACCEL[v]));
  const presets = PRESETS.map(([label, rpm]) => pushbutton(label, { color: label === "STOP" ? "red" : "white", onPress: () => sp("speed_rpm", rpm) }));
  const gov = maStation("TURBINE GOVERNOR", {
    module: "turbine_governor",
    pvTag: t => (t.generator_breaker ? "gross_mwe" : "turbine_speed_rpm"),
    spKey: t => (t.generator_breaker ? "target_mwe" : "speed_rpm"),
    spStep: 1, digits: 0, devSpan: 30,
    outTag: "turbine_valve_pct", outDigits: 1, outStep: 0.1,
    outCmd: v => ({ type: "set_turbine_valve", fraction: v / 100 }),
  }, ctx);
  const ramp = selector("LOAD RATE", [{ label: "10", value: 10 }, { label: "30", value: 30 }, { label: "60", value: 60 }, { label: "120", value: 120 }], v => sp("ramp_mw_min", v));
  const trip = pushbutton("TURBINE TRIP", { color: "red", onPress: () => ctx.send({ type: "trip_turbine" }) });
  const latch = pushbutton("TURBINE RESET", { color: "white", onPress: () => ctx.send({ type: "reset_turbine" }) });

  const scope = synchroscope();
  const syncCheck = lamp("white", "SYNC CHECK");
  const breaker = jhandle("GEN BREAKER", {
    left: "TRIP", right: "CLOSE",
    onLeft: () => ctx.send({ type: "set_generator_breaker", closed: false }),
    onRight: () => ctx.send({ type: "set_generator_breaker", closed: true }),
  });
  const phaseWin = h("div", { class: "ann-win single", "data-tag": "sync_blocked" }, "PHASE Δ OUT OF RANGE");
  const syncMa = maStation("AUTO SYNCHRONIZER", { module: "generator_sync", pvTag: "sync_phase_deg", digits: 0 }, ctx);
  const field = jhandle("EXCITER FIELD BKR", {
    left: "TRIP", right: "CLOSE",
    onLeft: () => ctx.send({ type: "set_field_breaker", closed: false }),
    onRight: () => ctx.send({ type: "set_field_breaker", closed: true }),
  });
  const avr = maStation("VOLTAGE REGULATOR (AVR)", {
    module: "excitation", pvTag: "gen_voltage_kv", spKey: "voltage_kv", spStep: 0.05, digits: 2, devSpan: 1.5,
    outTag: "excitation_pct", outStep: 0.5, outDigits: 1, outCmd: v => ({ type: "set_excitation", fraction: v / 100 }),
  }, ctx);

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "turbine.control" }, plate("TURBINE CONTROL"),
      h("div", { class: "row" }, load.el, rpmR.el, steamP.el),
      h("div", { class: "row" }, speed.el, valve.el, h("div", { class: "col" }, fine.el, health.el))),
    h("section", { class: "sect", "data-section": "turbine.accel" }, plate("AUTOMATIC ACCELERATION CONTROL"),
      h("div", { class: "row" }, accel.el, h("div", { class: "presets" }, ...presets.map(p => p.el))),
      h("div", { class: "row" }, gov.el, h("div", { class: "col" }, ramp.el, h("div", { class: "row tight" }, trip.el, latch.el)))),
    h("section", { class: "sect", "data-section": "turbine.sync" }, plate("GENERATOR SYNCHRONIZING"),
      h("div", { class: "row" }, scope.el,
        h("div", { class: "col" }, genV.el, busV.el, genF.el, busF.el)),
      h("div", { class: "row" }, h("div", { class: "col center" }, breaker.el, phaseWin, syncCheck.el), syncMa.el)),
    h("section", { class: "sect", "data-section": "turbine.excitation" }, plate("EXCITATION"), h("div", { class: "row" }, field.el, avr.el))));

  return t => {
    const R = t.readings;
    load.update(R.gross_mwe);
    rpmR.update(R.turbine_speed_rpm);
    steamP.update(R.sg_pressure_mpa);
    speed.update(R.turbine_speed_rpm);
    valve.update(R.turbine_valve_pct);
    fine.update(R.turbine_speed_rpm);
    genV.update(R.gen_voltage_kv); busV.update(R.grid_voltage_kv);
    genF.update(R.gen_freq_hz); busF.update(R.grid_freq_hz);
    health.update(R.generator_health_pct);
    const gm = t.modules.find(m => m.id === "turbine_governor");
    const accelSp = gm?.setpoints.accel_rpm_min;
    accel.update(Object.keys(ACCEL).find(k => ACCEL[k] === accelSp));
    ramp.update(gm?.setpoints.ramp_mw_min);
    const govAuto = ctx.moduleMode("turbine_governor") === "auto";
    presets.forEach((p, i) => p.update(govAuto && !t.generator_breaker && gm?.setpoints.speed_rpm === PRESETS[i][1], !govAuto || t.generator_breaker));
    gov.update(t);
    trip.update(t.turbine_tripped);
    latch.update(false, !t.turbine_tripped);
    const live = !t.generator_breaker && R.gen_voltage_kv > 1;
    scope.update(R.sync_phase_deg, live);
    syncCheck.update(t.sync_check_active);
    breaker.update(t.generator_breaker, t.generator_lockout);
    phaseWin.className = `ann-win single ${t.sync_blocked ? "alarm fast" : ""}`;
    syncMa.update(t);
    field.update(t.field_breaker);
    avr.update(t);
  };
}
