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
  const changeMw = h("input", { class: "save-name", type: "number", min: 0, step: 25, style: "width:90px", title: "New committed output [MWe net]" });
  changeMw.addEventListener("keydown", e => e.stopPropagation());
  const requestChange = async () => {
    const v = +changeMw.value;
    if (!(v > 0)) { const { toast } = await import("../api.js"); toast("Enter the output you want, in MWe net"); return; }
    play("pickup");
    const r = await api("/station/load_change", { target_mwe: v });
    if (!r.ok) { const { toast } = await import("../api.js"); toast(r.error); play("deny"); }
    ctx.pollStation();
  };
  const changeBox = h("div", { class: "row tight" }, changeMw, h("button", { class: "btn", onclick: requestChange }, "REQUEST CHANGE"));
  // Base-load block: hold a fixed output for hours or days.
  const blMw = h("input", { class: "save-name", type: "number", min: 0, step: 25, style: "width:90px", title: "Base-load output [MWe net] (blank = current commitment)" });
  blMw.addEventListener("keydown", e => e.stopPropagation());
  const blHours = h("select", { class: "hmi-dest" }, ...[["", "grid decides"], [12, "12 h"], [24, "24 h"], [72, "3 days"], [168, "1 week"]].map(([v, l]) => h("option", { value: v }, l)));
  blHours.value = "24";
  // Long-term contract (PPA).
  const ppaMw = h("input", { class: "save-name", type: "number", min: 0, step: 25, style: "width:90px", title: "Contract output [MWe net] (blank = grid proposes)" });
  ppaMw.addEventListener("keydown", e => e.stopPropagation());
  const ppaMonths = h("select", { class: "hmi-dest" }, ...[[1, "1 month"], [3, "3 months"], [6, "6 months"], [12, "1 year"], [24, "2 years"], [60, "5 years"]].map(([v, l]) => h("option", { value: v }, l)));
  ppaMonths.value = "12";
  const ppaShape = h("select", { class: "hmi-dest" }, h("option", { value: "flat" }, "flat"), h("option", { value: "shaped" }, "seasonal"));
  const ppaStatus = h("div", { class: "card-sub" }, "");
  const requestContract = async () => {
    play("pickup");
    const body = { months: +ppaMonths.value, shaped: ppaShape.value === "shaped" };
    if (+ppaMw.value > 0) body.target_mwe = +ppaMw.value;
    const r = await api("/station/contract", body);
    if (!r.ok) { const { toast } = await import("../api.js"); toast(r.error); play("deny"); }
    ctx.pollStation();
  };
  const terminateContract = async () => {
    if (!confirm("Terminate the contract early? The termination fee is 10 % of the remaining contract value.")) return;
    const r = await api("/station/contract", { terminate: true });
    if (!r.ok) { const { toast } = await import("../api.js"); toast(r.error); }
    ctx.pollStation();
  };
  const ppaBox = h("div", {}, h("div", { class: "row tight" }, ppaMw, ppaMonths, ppaShape,
    h("button", { class: "btn", onclick: requestContract }, "REQUEST CONTRACT"),
    h("button", { class: "btn", onclick: terminateContract }, "TERMINATE")), ppaStatus);
  // System demand and grid offers.
  const sysCal = h("div", { class: "sys-cal" }, "");
  const sysLoad = readout("SYSTEM LOAD [% OF PEAK]", { digits: 0, size: "big" });
  const sysState = h("div", { class: "req-status" }, "");
  const fcBars = h("div", { class: "fc-bars" });
  // Wholesale market: real-time price, day-ahead prices, tariff toggle.
  const spot = readout("REAL-TIME PRICE", { format: v => `$${v.toFixed(2)}/MWh`, size: "big" });
  const daBars = h("div", { class: "fc-bars" });
  const daRange = h("div", { class: "card-sub" }, "");
  const mktMode = h("select", { class: "hmi-dest", title: "Sell at the wholesale market price, or at a fixed tariff" },
    h("option", { value: "market" }, "WHOLESALE MARKET"), h("option", { value: "tariff" }, "FIXED TARIFF"));
  mktMode.addEventListener("change", async () => {
    play("button");
    await api("/station/market", { on: mktMode.value === "market" });
    ctx.pollStation();
  });
  const bidsBox = h("div", {});
  const offerBox = h("div", {});
  const blStatus = h("div", { class: "card-sub" }, "");
  const requestBaseload = async () => {
    play("pickup");
    const body = {};
    if (blHours.value) body.hours = +blHours.value;
    if (+blMw.value > 0) body.target_mwe = +blMw.value;
    if (body.hours === undefined && body.target_mwe === undefined) body.hours = 24;
    const r = await api("/station/baseload", body);
    if (!r.ok) { const { toast } = await import("../api.js"); toast(r.error); play("deny"); }
    ctx.pollStation();
  };
  const cancelBaseload = async () => { await api("/station/baseload", { cancel: true }); ctx.pollStation(); };
  const baseBox = h("div", {}, h("div", { class: "row tight" }, blMw, blHours,
    h("button", { class: "btn", onclick: requestBaseload }, "REQUEST BASE-LOAD"),
    h("button", { class: "btn", onclick: cancelBaseload }, "CANCEL")), blStatus);
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
  const price = readout("MARKET PRICE NOW", { format: v => `$${v.toFixed(0)}/MWh` });
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
        h("div", { class: "col" }, plate("DISPATCH INSTRUCTION", "small"), request, h("div", { class: "row tight" }, accept.el, agc.el), perf,
          offerBox,
          plate("ASK DISPATCH FOR A NEW COMMITMENT", "small"), changeBox,
          plate("BASE-LOAD BLOCK", "small"), baseBox,
          plate("LONG-TERM CONTRACT (PPA)", "small"), ppaBox, bidsBox,
          h("p", { class: "note" }, "A power purchase agreement sells a fixed output for months or years: lower price the longer the term (about 93 % of market for a month down to 80 % for five years), +3 % if shaped by season (more in winter, less in spring / autumn). No dispatch calls while it runs. Shortfalls (including trips) are bought in at 1.2x market, billed daily; early termination costs 10 % of the remaining value."),
          h("p", { class: "note" }, "Base-load: hold a fixed output for hours or days with no load-following requests. Paid at 92 % of the price while on target (no flexibility premium)."),
          h("p", { class: "note" }, "Below your commitment = derate: usually approved, you pay replacement power (an hour of the shortfall at market price). Above = offer more: taken only when the system needs it, with a capacity premium on the extra output; capped at what your unit can make.")))),
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("SYSTEM DEMAND"),
      h("div", { class: "row" },
        h("div", { class: "col" }, sysCal, sysLoad.el, sysState),
        h("div", { class: "col" }, plate("NEXT 24 HOURS", "small"), fcBars)),
      h("p", { class: "note" }, "Network demand follows the time of day (overnight trough, evening peak), the week (quieter weekends) and the season (winter and summer peaks). Dispatch asks more of you as demand rises, won't let you derate at peak, only takes extra output when it's needed, and sizes base-load blocks to what the system can absorb through every night and weekend in the period.")),
    h("section", { class: "sect wide", "data-section": "grid.station" }, plate("WHOLESALE MARKET"),
      h("div", { class: "row" },
        h("div", { class: "col" }, mktMode, spot.el),
        h("div", { class: "col" }, plate("DAY-AHEAD PRICES, NEXT 24 HOURS", "small"), daBars, daRange)),
      h("p", { class: "note" }, "The unit bids into the market automatically as a price-taker (nuclear runs whatever the price), so there is nothing to bid by hand. The price follows system demand: cheap overnight and on windy days, dear at the evening peak, occasionally below zero on windy spring nights, when every MWh sold costs money. Dispatch performance bonuses and penalties still apply on top. A long-term contract replaces the market price with its fixed price: a hedge. FIXED TARIFF sells everything at the flat base price instead.")),
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
      rate.update(e.tariff_mwh * e.multiplier);
      revH.update(e.revenue_per_h);
      costH.update(e.cost_per_h);
      profitH.update(e.profit_per_h);
      atDemand.update(e.at_demand_per_h);
      perDay.update(e.profit_per_h * 24);
      month.update(e.month_revenue == null ? null : e.month_revenue - e.month_costs);
      const pm = e.performance;
      const [txt, cls] = !t.generator_breaker ? ["OFF-LINE: NOT SELLING", "bad"]
        : st.contract ? [`CONTRACT PRICE: ${(st.contract.price_mult * 100).toFixed(0)} % OF AVERAGE MARKET`, "ok"]
        : st.baseload_until && Math.abs(pm - 0.92) < 1e-6 ? ["BASE-LOAD ON TARGET: 92 % PRICE", "ok"]
        : pm > 1 ? ["ON DEMAND: +5 % PRICE BONUS", "ok"]
        : pm < 1 ? ["OFF DEMAND: -15 % PRICE PENALTY", "bad"]
        : [e.market ? "MARKET PRICE" : "BASE PRICE", "warn"];
      bonus.textContent = txt;
      bonus.className = `req-status ${cls}`;
      const c = e.costs_per_h;
      econNote.textContent = `Running costs per hour: fuel ${money(c.fuel)}, cooling water ${money(c.water)}, grid power import ${money(c.grid_power)}, maintenance ${money(c.maintenance)}, staff ${money(c.staff)}. `
        + "Staying within ±3 % of committed demand earns +5 %; off demand after the deadline costs -15 %. "
        + (e.career ? "Costs are billed at month end (Career tab)." : "Sandbox: typical prices shown, no money changes hands.");
    }
    if (st.day_ahead) {
      if (document.activeElement !== mktMode) mktMode.value = st.market ? "market" : "tariff";
      spot.update(st.spot_price);
      const dk = st.day_ahead.map(v => v.toFixed(1)).join();
      if (daBars.dataset.k !== dk) {
        daBars.dataset.k = dk;
        const base = st.economics?.tariff_mwh ?? 45;
        daBars.replaceChildren(...st.day_ahead.map((v, i) => h("div", { class: "fc-bar", title: `+${i} h: $${v.toFixed(2)}/MWh`,
          style: `height:${Math.max(4, Math.min(100, (v + 10) / (2.4 * base) * 100))}%;background:${v < 0 ? "#6b4bb3" : v > 1.3 * base ? "#c62f25" : v > base ? "#d39b1f" : "#3a8a50"}` })));
        const lo = Math.min(...st.day_ahead), hi = Math.max(...st.day_ahead);
        daRange.textContent = `low $${lo.toFixed(0)} · high $${hi.toFixed(0)} per MWh${lo < 0 ? " · NEGATIVE PRICES AHEAD" : ""}`;
      }
    }
    const bids = st.bids || [];
    const bk = bids.map(b => `${b.buyer}:${b.price_mult}`).join("|");
    if (bidsBox.dataset.k !== bk) {
      bidsBox.dataset.k = bk;
      bidsBox.replaceChildren(...(bids.length ? [h("div", { class: "req-card" },
        h("b", {}, "OTHER BUYERS' BIDS (take it or leave it)"),
        ...bids.map((b, i) => h("div", { class: "row tight" },
          h("span", { class: "card-sub", style: "flex:1" }, `${b.buyer}: ${b.months} months ${b.shaped ? "seasonal" : `flat ${b.mw.toFixed(0)} MWe`} at ${(b.price_mult * 100).toFixed(1)} % of market, shortfalls charged at ${b.penalty.toFixed(1)}x`),
          h("button", { class: "btn", onclick: async () => {
            play("button");
            const r = await api("/station/contract", { accept_bid: i });
            if (!r.ok) { const { toast } = await import("../api.js"); toast(r.error); play("deny"); }
            ctx.pollStation();
          } }, "SIGN"))),
        h("div", { class: "card-sub" }, "Higher prices come with stiffer shortfall charges: a trip costs more with a 24/7 industrial buyer. Bids lapse with the dispatch offer."))] : []));
    }
    if (st.calendar) {
      sysCal.textContent = `${st.calendar} · ${st.season}`;
      sysLoad.update(100 * st.system_load);
      sysState.textContent = st.grid_state;
      sysState.className = `req-status ${st.system_load >= 0.92 ? "bad" : st.system_load >= 0.82 ? "warn" : "ok"}`;
      const fk = st.forecast.map(v => v.toFixed(2)).join();
      if (fcBars.dataset.k !== fk) {
        fcBars.dataset.k = fk;
        fcBars.replaceChildren(...st.forecast.map((v, i) => h("div", { class: "fc-bar", title: `+${i} h: ${(v * 100).toFixed(0)} %`,
          style: `height:${Math.max(4, (v - 0.4) / 0.6 * 100)}%;background:${v >= 0.92 ? "#c62f25" : v >= 0.82 ? "#d39b1f" : "#3a8a50"}` })));
      }
    }
    const o = st.offer;
    // Prefill the matching request box with dispatch's numbers once per offer.
    const ok = o ? `${o.kind}:${o.mw}:${o.hours}:${o.last}` : "";
    if (ok && offerBox.dataset.k !== ok) {
      if (o.kind === "contract") {
        ppaMw.value = o.mw;
        ppaMonths.value = String(o.months);
        ppaShape.value = o.shaped ? "shaped" : "flat";
      } else if (o.kind === "baseload") {
        blMw.value = o.mw;
        if ([...blHours.options].some(x => +x.value === o.hours)) blHours.value = String(o.hours);
      } else {
        changeMw.value = o.mw;
      }
    }
    offerBox.dataset.k = ok;
    offerBox.replaceChildren(...(o ? [h("div", { class: "req-card" }, ...[
      h("b", {}, `GRID OFFER${o.last ? " (FINAL)" : ""}: ${o.kind === "contract" ? `${o.months}-month ${o.shaped ? "seasonal" : "flat"} contract at ${(o.price_mult * 100).toFixed(0)} % of market`
        : o.kind === "baseload" ? `base-load ${o.mw.toFixed(0)} MWe for ${o.hours.toFixed(0)} h` : `commitment ${o.mw.toFixed(0)} MWe net`}`),
      o.kind === "contract" ? h("div", { class: "card-sub" }, `Monthly targets [MWe]: ${o.profile.map(v => v.toFixed(0)).join(" · ")}`) : null,
      h("div", { class: "card-sub" }, `expires in ${Math.max(0, (o.expires_at - t.sim_time) / 60).toFixed(0)} min`),
      h("div", { class: "card-sub" }, o.kind === "contract"
        ? "To counter: change the output in the LONG-TERM CONTRACT box (same term and shape) and press REQUEST CONTRACT. A different term or shape gets a fresh offer."
        : o.kind === "baseload"
        ? "To counter: change the output and/or duration in the BASE-LOAD BLOCK box below and press REQUEST BASE-LOAD. Same duration: they may meet you in the middle once. Different duration: they make a fresh offer sized for it."
        : "To counter: put your output in the NEW COMMITMENT box below and press REQUEST CHANGE. They may meet you in the middle once, then hold firm."),
      h("div", { class: "row tight" },
        h("button", { class: "btn", onclick: async () => { play("button"); await api("/station/offer", { accept: true }); ctx.pollStation(); } }, "ACCEPT"),
        h("button", { class: "btn", onclick: async () => { play("button"); await api("/station/offer", { accept: false }); ctx.pollStation(); } }, "DECLINE"))].filter(Boolean))] : []));
    const c = st.contract;
    ppaStatus.textContent = c
      ? `CONTRACT: month ${c.month + 1} of ${c.months}, target ${c.profile[c.month].toFixed(0)} MWe at ${(c.price_mult * 100).toFixed(0)} % of market (${c.buyer}), ends in ${((c.end_t - t.sim_time) / 86400).toFixed(0)} days${st.shortfall_owed > 1 ? ` · shortfall owed $${(st.shortfall_owed / 1e3).toFixed(0)}k` : ""}`
      : "No contract.";
    blStatus.textContent = st.baseload_until ? `BASE-LOAD at ${st.demand_mwe.toFixed(0)} MWe until ${fmt(st.baseload_until)} (${((st.baseload_until - t.sim_time) / 3600).toFixed(1)} h left)` : "Load-following dispatch.";
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
