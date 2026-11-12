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
  const ulc = maStation("UNIT LOAD CONTROLLER", {
    module: "plant_master", pvTag: "gross_mwe", spKey: "target_mwe", spStep: 10, digits: 0, devSpan: 50, range: [0, 1250],
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
