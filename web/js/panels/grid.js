// Grid control station: dispatch demand vs unit output, load requests, AGC.

import { rating, mweScale } from "../rating.js";
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
  const demandM = roundMeter("GRID DEMAND", { min: 0, max: mweScale(), unit: "MWe net", major: 10, digits: 0 });
  const netM = roundMeter("UNIT NET OUTPUT", { min: 0, max: mweScale(), unit: "MWe net", major: 10, digits: 0, ref: true });
  const gross = readout("GROSS [MWe]", { digits: 0 });
  const net = readout("NET [MWe]", { digits: 0, size: "big" });
  const demand = readout("DEMAND [MWe]", { digits: 0, size: "big" });
  const freq = readout("GRID FREQ [Hz]", { digits: 3 });
  const rec = recorder("DEMAND vs NET", [
    { tag: "demand", label: "Demand", color: "#1c5bd6", min: 0, max: mweScale() },
    { tag: "net_mwe", label: "Net output", color: "#d6261c", min: 0, max: mweScale() },
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
  // ---- earnings
  const money = v => {
    if (v === undefined || v === null) return "—";
    const a = Math.abs(v), sign = v < 0 ? "-" : "";
    return a >= 1e6 ? `${sign}$${(a / 1e6).toFixed(2)}M` : a >= 1e3 ? `${sign}$${(a / 1e3).toFixed(1)}k` : `${sign}$${a.toFixed(0)}`;
  };
  const price = readout("POWER PRICE", { format: v => `$${v.toFixed(0)}/MWh` });
  const rate = readout("SELLING AT", { format: v => `$${v.toFixed(2)}/MWh` });
  const revH = readout("REVENUE / HOUR", { format: money, size: "big" });
  const costH = readout("RUNNING COSTS / HOUR", { format: money });
  const profitH = readout("PROFIT / HOUR", { format: money, size: "big" });
  const atDemand = readout("REVENUE AT DEMAND / HOUR", { format: money });
  const perDay = readout("PROFIT / DAY (AT THIS RATE)", { format: money });
  const month = readout("THIS MONTH: REVENUE - COSTS", { format: money });
  const bonus = h("div", { class: "req-status" }, "");
  const econNote = h("p", { class: "note" }, "");
  // ---- generator & switchyard metering
  const mva = Math.round(rating.gross * 1.13 / 0.9 / 100) * 100;
  const genMw = roundMeter("GENERATOR MW", { min: 0, max: mweScale(), unit: "MW", major: 10, digits: 0 });
  const genMvar = roundMeter("GENERATOR MVAR", { min: -mva / 2, max: mva / 2, unit: "MVAR (+ out)", major: 8, digits: 0 });
  const pf = readout("POWER FACTOR", { digits: 3 });
  const amps = readout("STATOR CURRENT [kA]", { digits: 1 });
  const genKv = readout("GEN TERMINAL [kV]", { digits: 2 });
  const yardKv = readout("SWITCHYARD [kV]", { digits: 1 });
  const fieldA = readout("FIELD CURRENT [A]", { digits: 0 });
  const genHz = readout("GEN FREQ [Hz]", { digits: 3 });

  // ---- energy & money counters (odometer style)
  const counter = (label, digits = 0, unit = "") => readout(label, { digits, unit });
  const totGross = counter("GROSS GENERATION", 0, "MWh");
  const totNet = counter("NET TO GRID", 0, "MWh");
  const totHouse = counter("STATION SERVICE", 0, "MWh");
  const totImport = counter("IMPORTED FROM GRID", 0, "MWh");
  const totRev = readout("ALL-TIME REVENUE", { format: v => money(v) });
  const totCost = readout("ALL-TIME RUNNING COSTS", { format: v => money(v) });
  const totPen = readout("ALL-TIME PENALTIES", { format: v => money(v) });
  const totProfit = readout("ALL-TIME PROFIT", { format: v => money(v), size: "big" });
  const insTarget = readout("INSTRUCTION TARGET [MWe]", { digits: 0 });
  const insTime = readout("TIME ON INSTRUCTION", { format: v => fmt(v) });
  const insOn = readout("ON TARGET", { format: v => `${v.toFixed(0)} %` });
  const insMwh = counter("DELIVERED", 0, "MWh");
  const insRev = readout("INSTRUCTION REVENUE", { format: v => money(v) });
  const insProfit = readout("INSTRUCTION PROFIT", { format: v => money(v), size: "big" });
  const histTable = h("tbody");

  const ulc = maStation("UNIT LOAD CONTROLLER", {
    module: "plant_master", pvTag: "gross_mwe", spKey: "target_mwe", spStep: 10, digits: 0, devSpan: 50, range: [0, Math.ceil(rating.gross * 1.08 / 50) * 50],
  }, ctx);

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("GRID CONTROL STATION"),
      h("div", { class: "row" }, demandM.el, netM.el,
        h("div", { class: "col" }, demand.el, net.el), h("div", { class: "col" }, gross.el, freq.el),
        h("div", { class: "col" }, plate("DISPATCH INSTRUCTION", "small"), request, h("div", { class: "row tight" }, accept.el, agc.el), perf))),
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("EARNINGS"),
      h("div", { class: "row" },
        h("div", { class: "col" }, price.el, rate.el, bonus),
        h("div", { class: "col" }, revH.el, costH.el),
        h("div", { class: "col" }, profitH.el, perDay.el),
        h("div", { class: "col" }, atDemand.el, month.el)),
      econNote),
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("GENERATOR & SWITCHYARD METERING"),
      h("div", { class: "row" }, genMw.el, genMvar.el,
        h("div", { class: "col" }, pf.el, amps.el, fieldA.el),
        h("div", { class: "col" }, genKv.el, yardKv.el, genHz.el)),
      h("p", { class: "note" }, "MVAR follows excitation: raise the voltage regulator setpoint to export reactive power (lagging power factor), lower it to absorb. Grid operators usually ask for a slightly lagging power factor around 0.95.")),
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("ENERGY & REVENUE COUNTERS"),
      h("div", { class: "row" },
        h("div", { class: "col" }, plate("ALL TIME", "small"), totGross.el, totNet.el, totHouse.el, totImport.el),
        h("div", { class: "col" }, plate("ALL TIME $", "small"), totRev.el, totCost.el, totPen.el, totProfit.el),
        h("div", { class: "col" }, plate("CURRENT DISPATCH INSTRUCTION", "small"), insTarget.el, insTime.el, insOn.el, insMwh.el, insRev.el, insProfit.el),
        h("div", { class: "col" }, plate("RECENT INSTRUCTIONS", "small"),
          h("table", { class: "bill-table" }, h("thead", {}, h("tr", {}, h("th", {}, "MWe"), h("th", {}, "hours"), h("th", {}, "on tgt"), h("th", {}, "revenue"), h("th", {}, "profit"))), histTable)))),
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
    genMw.update(R.gen_mw ?? 0);
    genMvar.update(R.gen_mvar ?? 0);
    pf.update(R.gen_pf ?? 1);
    amps.update(R.gen_stator_ka ?? 0);
    genKv.update(R.gen_voltage_kv ?? 0);
    yardKv.update(R.switchyard_kv ?? 0);
    fieldA.update(R.field_amps ?? 0);
    genHz.update(R.gen_freq_hz ?? 0);
    if (!st) return;
    if (st.totals) {
      const T = st.totals;
      totGross.update(T.gross_mwh); totNet.update(T.net_mwh); totHouse.update(T.house_mwh); totImport.update(T.import_mwh);
      totRev.update(T.revenue); totCost.update(T.costs); totPen.update(T.penalties); totProfit.update(st.totals_profit);
      const I = st.instruction;
      insTarget.update(I ? I.target_mwe : 0);
      insTime.update(I ? t.sim_time - I.started_at : 0);
      insOn.update(I && I.seconds > 0 ? 100 * I.on_target_s / I.seconds : 0);
      insMwh.update(I ? I.net_mwh : 0);
      insRev.update(I ? I.revenue : 0);
      insProfit.update(st.instruction_profit ?? 0);
      const hk = JSON.stringify(st.history.map(x => x.id));
      if (histTable.dataset.k !== hk) {
        histTable.dataset.k = hk;
        histTable.replaceChildren(...st.history.map(x => h("tr", {},
          h("td", {}, x.target_mwe.toFixed(0)),
          h("td", {}, (x.seconds / 3600).toFixed(1)),
          h("td", {}, x.seconds > 0 ? `${(100 * x.on_target_s / x.seconds).toFixed(0)} %` : "—"),
          h("td", { class: "num" }, money(x.revenue)),
          h("td", { class: "num" }, money(x.revenue - x.costs - x.penalties)))));
      }
    }
    const e = st.economics;
    if (e) {
      price.update(e.price_mwh);
      rate.update(e.price_mwh * e.multiplier);
      revH.update(e.revenue_per_h);
      costH.update(e.cost_per_h);
      profitH.update(e.profit_per_h);
      atDemand.update(e.at_demand_per_h);
      perDay.update(e.profit_per_h * 24);
      month.update(e.month_revenue == null ? null : e.month_revenue - e.month_costs);
      const [txt, cls] = !t.generator_breaker ? ["OFF-LINE: NOT SELLING", "bad"]
        : e.multiplier > 1 ? ["ON DEMAND: +5 % PRICE BONUS", "ok"]
        : e.multiplier < 1 ? ["OFF DEMAND: -15 % PRICE PENALTY", "bad"]
        : ["BASE PRICE", "warn"];
      bonus.textContent = txt;
      bonus.className = `req-status ${cls}`;
      const c = e.costs_per_h;
      econNote.textContent = `Running costs per hour: fuel ${money(c.fuel)}, cooling water ${money(c.water)}, grid power import ${money(c.grid_power)}, maintenance ${money(c.maintenance)}, staff ${money(c.staff)}. `
        + "Staying within ±3 % of committed demand earns +5 %; off demand after the deadline costs -15 %. "
        + (e.career ? "Costs are billed at month end (Career tab)." : "Sandbox: typical prices shown, no money changes hands.");
    }
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
