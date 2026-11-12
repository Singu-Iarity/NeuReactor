// Grid control station: dispatch demand vs unit output, load requests, AGC.

import { h, plate, roundMeter, readout, recorder, pushbutton, maStation } from "../widgets.js";
import { api } from "../api.js";
import { play } from "../sound.js";

export const id = "grid";
export const title = "Grid";
export const cols = 4;
export const windows = [
  ["grid_request", "GRID REQUEST PENDING"], ["off_demand", "OUTPUT OFF DEMAND"], ["deadline_missed", "DISPATCH DEADLINE MISSED"], ["agc", "AGC IN CONTROL"],
  ["grid_perm_ok", "SYNC PERMISSION GRANTED"], ["hall_ready", "TURBINE HALL READY"], ["call_waiting", "CALL IN PROGRESS"], ["", ""],
];

const fmt = s => `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}`;

export function build(root, ctx) {
  const demandM = roundMeter("GRID DEMAND", { min: 0, max: 1300, unit: "MWe net", major: 13, digits: 0 });
  const netM = roundMeter("UNIT NET OUTPUT", { min: 0, max: 1300, unit: "MWe net", major: 13, digits: 0, ref: true });
  const gross = readout("GROSS [MWe]", { digits: 0 });
  const net = readout("NET [MWe]", { digits: 0, size: "big" });
  const demand = readout("DEMAND [MWe]", { digits: 0, size: "big" });
  const freq = readout("GRID FREQ [Hz]", { digits: 3 });
  const rec = recorder("DEMAND vs NET", [
    { tag: "demand", label: "Demand", color: "#1c5bd6", min: 0, max: 1300 },
    { tag: "net_mwe", label: "Net output", color: "#d6261c", min: 0, max: 1300 },
  ], { span: 3600 });
  const request = h("div", { class: "request" });
  const perf = h("div", { class: "card-sub" });
  const history = h("ol", { class: "events news" });
  const accept = pushbutton("ACCEPT REQUEST", { color: "amber", onPress: async () => {
    const r = await api("/station/accept", {});
    if (r.ok) play("pickup");
    ctx.pollStation();
  } });
  const agc = pushbutton("AGC", { onPress: async () => {
    const r = await api("/station/agc", { on: !ctx.station()?.agc });
    if (!r.ok) { const { toast } = await import("../api.js"); toast(r.error); play("deny"); }
    ctx.pollStation();
  } });
  const ulc = maStation("UNIT LOAD CONTROLLER", {
    module: "plant_master", pvTag: "gross_mwe", spKey: "target_mwe", spStep: 10, digits: 0, devSpan: 50, range: [0, 1250],
  }, ctx);

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("GRID CONTROL STATION"),
      h("div", { class: "row" }, demandM.el, netM.el,
        h("div", { class: "col" }, demand.el, net.el), h("div", { class: "col" }, gross.el, freq.el),
        h("div", { class: "col" }, plate("DISPATCH INSTRUCTION", "small"), request, h("div", { class: "row tight" }, accept.el, agc.el), perf))),
    h("section", { class: "sect", "data-section": "grid.station" }, plate("UNIT LOAD CONTROL / AGC"),
      h("p", { class: "note" }, "With the Unit Load Controller (plant master) in AUTO and the Grid Dispatch Link installed, AGC follows grid demand automatically."),
      h("div", { class: "row" }, ulc.el)),
    h("section", { class: "sect", "data-section": "grid.station" }, plate("LOAD RECORDER"), rec.el),
    h("section", { class: "sect", "data-section": "grid.station" }, plate("DISPATCH LOG"), history)));

  return (t, visible) => {
    const st = ctx.station();
    const R = t.readings;
    const d = st?.demand_mwe ?? 0;
    rec.push(t.sim_time, { demand: d, net_mwe: Math.max(0, R.net_mwe) });
    if (!visible) return;   // keep recording while another tab is shown
    demandM.update(d);
    netM.update(Math.max(0, R.net_mwe), d);
    gross.update(R.gross_mwe);
    net.update(R.net_mwe);
    demand.update(d);
    freq.update(R.grid_freq_hz);
    rec.draw();
    if (!st) return;
    const p = st.pending;
    const left = dl => { const m = Math.round((dl - t.sim_time) / 60); return m >= 0 ? `${m} min left` : `${-m} min overdue`; };
    let status = null;
    if (st.demand_mwe > 0 && t.generator_breaker) {
      const off = R.net_mwe - st.demand_mwe;
      const [txt, cls] = st.reached && Math.abs(off) <= 33 ? ["ON TARGET — hold output", "ok"]
        : st.reached ? [`DRIFTED ${off > 0 ? "+" : ""}${off.toFixed(0)} MWe — correct output`, "warn"]
        : st.deadline_missed ? [`DEADLINE MISSED — ${off > 0 ? "+" : ""}${off.toFixed(0)} MWe off`, "bad"]
        : [`RAMPING — ${off > 0 ? "+" : ""}${off.toFixed(0)} MWe to go, ${left(st.deadline)}`, "warn"];
      status = h("div", { class: `req-status ${cls}` }, `Committed ${st.demand_mwe.toFixed(0)} MWe net: ${txt}`);
    }
    request.replaceChildren(...[
      p ? h("div", { class: "req-card" }, h("b", {}, `NEW: ${p.target_mwe.toFixed(0)} MWe net`),
        h("div", { class: "card-sub" }, `by ${fmt(p.deadline)} (${left(p.deadline)}) — press ACCEPT REQUEST to acknowledge`)) : null,
      status,
      !p && !status ? h("div", { class: "card-sub" }, t.generator_breaker ? "No instruction yet." : "Unit off-line: call Grid Dispatch for permission to synchronise (Comms tab).") : null,
    ].filter(Boolean));
    accept.update(!!p, !p || st.agc);
    const agcOk = t.modules.find(m => m.id === "plant_master")?.mode === "auto"
      && t.upgrades.find(u => u.id === "grid_dispatch_link")?.installed;
    agc.update(st.agc, !agcOk && !st.agc);
    ulc.update(t);
    const total = st.on_target_s + st.off_target_s;
    perf.textContent = total > 0 ? `On target ${(100 * st.on_target_s / total).toFixed(0)} % of committed time` : "";
    const msgs = st.messages.filter(m => m.from === "Grid Dispatch" || m.text.startsWith("Acknowledged"));
    history.replaceChildren(...msgs.slice(0, 40).map(m => h("li", {}, `${fmt(m.sim_time)}  ${m.from}: ${m.text}`)));
  };
}
