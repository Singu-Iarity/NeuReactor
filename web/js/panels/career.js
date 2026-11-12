// Career office: start a career, build the plant within budget, shop for
// equipment, research technology early, and follow the calendar.

import { h, plate } from "../widgets.js";
import { api, toast } from "../api.js";
import { settings } from "../settings.js";

export const id = "career";
export const title = "Career";
export const windows = [];

const M = n => `$${(n / 1e6).toFixed(n >= 1e8 ? 0 : 1)}M`;
const ERA_LABEL = { vintage: "Vintage", dated: "Dated", modern: "Modern" };
const TIER_LABEL = { basic: "Basic", quality: "Quality", premium: "Premium" };

async function post(action, body = {}) {
  const r = await api(`/career/${action}`, body);
  if (!r.ok) toast(r.error);
  return r.ok;
}

export function build(root, ctx) {
  const setup = h("section", { class: "sect wide" });
  const office = h("div", { class: "board" });
  root.append(h("div", { class: "board" }, setup), office);

  // ---- new career form
  const year = h("select", {}, ...[1968, 1975, 1982, 1988, 1995, 2005].map(y => h("option", { value: y, selected: y === 1975 }, `${y}`)));
  const diff = h("select", {}, h("option", { value: "easy" }, "Easy (generous budget)"), h("option", { value: "normal", selected: true }, "Normal"), h("option", { value: "hard" }, "Hard (tight budget)"));
  setup.append(plate("NEW CAREER"),
    h("p", { class: "note" }, "Start in a chosen year with a construction budget. Equip the unit: each component in an era (vintage / dated / modern) and quality tier (basic / quality / premium), each automation module as an analog cabinet or computer software. Equipment two eras apart from what it connects to needs a conversion kit. After commissioning, sell power, research new technology, and upgrade during outages."),
    h("div", { class: "row tight" }, h("label", {}, "Start year ", year), h("label", {}, "Difficulty ", diff),
      h("button", { class: "btn big", onclick: async () => {
        if (await post("new", { start_year: +year.value, difficulty: diff.value })) { lastSig = ""; poll(); }
      } }, "START CAREER")));

  // ---- office sections
  const status = h("div", { class: "career-status" });
  const actions = h("div", { class: "row tight" });
  const news = h("ol", { class: "events news" });
  const research = h("div", { class: "equip-grid" });
  const researchOpts = h("div", { class: "research-list" });
  // Shop size picker per component card (defaults to the installed size or
  // the unit size).
  const sizeSel = new Map();
  const SIZES = ["mwe600", "mwe900", "mwe1150", "mwe1300"];
  const sizeLabel = sz => `${sz.slice(3)} MWe`;
  const confirmToggle = h("label", { class: "shop-toggle" },
    h("input", { type: "checkbox", checked: settings.get("confirmBuy"), onchange: e => settings.set("confirmBuy", e.target.checked) }),
    " Confirm each purchase");
  const shopComp = h("div", { class: "equip-grid" });
  const shopCtl = h("div", { class: "equip-grid" });
  const shopHw = h("div", { class: "equip-grid" });
  office.append(
    h("section", { class: "sect wide" }, plate("PLANT OFFICE"), status, actions),
    h("section", { class: "sect" }, plate("NOTIFICATIONS"), news),
    h("section", { class: "sect" }, plate("RESEARCH & DEVELOPMENT"), research, researchOpts),
    h("section", { class: "sect wide" }, plate("SHOP — PLANT COMPONENTS"), confirmToggle,
      h("p", { class: "note" }, "During operation, major components can only be swapped in an outage (reactor shut down, generator off-line). Replaced equipment is sold for 20 %; during construction swaps are fully refunded."),
      h("p", { class: "note" }, "Unit size: the reactor core sets it (fuel assemblies, control rods, rated MWe). Bigger cores need bigger components: anything smaller than the core derates the unit, and rod drives smaller than the core cannot move its rods. A new core in operation also needs the plant computers and automation rescaled (priced in). Sizes arrive over the years: 600 MWe 1965, 900 MWe 1968, 1150 MWe 1972, 1300 MWe 1982."),
      shopComp),
    h("section", { class: "sect wide" }, plate("SHOP — AUTOMATION CONTROLLERS"), h("p", { class: "note" }, "Analog cabinets need no computer but hold their setpoints less precisely (vintage < dated, basic < premium). Digital software is precise but needs its computer and panel package."), shopCtl),
    h("section", { class: "sect wide" }, plate("SHOP — I&C HARDWARE"), shopHw));

  const confirmBuy = async q => {
    if (settings.get("confirmBuy")) {
      const kits = q.kits.length ? `\n+ ${q.kits.map(k => `${k.name} (${M(k.price)})`).join("\n+ ")}` : "";
      const refund = q.refund ? `\n− refund ${M(q.refund)}` : "";
      if (!confirm(`Buy ${q.label} for ${M(q.price)}${kits}${refund}\nTotal ${M(q.total)}?`)) return;
    }
    if (await post("buy", q.purchase)) {
      if (!settings.get("confirmBuy")) toast(`Bought ${q.label}: ${M(q.total)}`);
      lastSig = ""; poll();
    }
  };

  const optBtn = q => h("button", {
    class: `opt ${q.owned ? "owned" : ""} ${q.available ? "" : "locked"} ${q.kits.length ? "kit" : ""}`,
    disabled: !q.available || q.owned,
    title: q.available ? (q.kits.length ? `Needs: ${q.kits.map(k => k.name).join(", ")}` : "") : `Available ${q.year} (or research it)`,
    onclick: () => confirmBuy(q),
  }, h("span", { class: "opt-l" }, q.label), h("span", { class: "opt-p" }, q.owned ? "INSTALLED" : q.available ? M(q.total) : `${q.year}`),
     q.perf ? h("span", { class: "opt-perf" }, `${(q.perf * 100).toFixed(1)}%`) : null);

  function renderShopComponents(v) {
    shopComp.replaceChildren(...v.shop.components.map(s => {
      const inst = s.installed;
      const sel = sizeSel.get(s.slot) || (inst && s.sized ? inst.size : v.unit_size) || "mwe1150";
      const under = s.sized && inst && s.slot !== "reactor_core" && SIZES.indexOf(inst.size) < SIZES.indexOf(v.unit_size);
      const picker = s.sized ? h("div", { class: "size-pick" }, h("span", {}, "SIZE"),
        ...SIZES.map(sz => h("button", {
          class: `size-btn ${sz === sel ? "on" : ""} ${sz === v.unit_size ? "unit" : ""}`,
          title: sz === v.unit_size ? "Matches the reactor core" : "",
          onclick: () => { sizeSel.set(s.slot, sz); renderShopComponents(v); },
        }, sizeLabel(sz)))) : null;
      const opts = s.options.filter(q => !s.sized || q.purchase.size === sel);
      return h("div", { class: `card ${under ? "warn" : ""}` },
        h("div", { class: "card-name" }, s.name),
        h("div", { class: "card-sub" }, `${s.effect} · installed: ${inst ? `${ERA_LABEL[inst.era]} ${TIER_LABEL[inst.tier]}${s.sized ? ` ${sizeLabel(inst.size)}` : ""}` : "NONE"}${under ? " · UNDERSIZED FOR CORE" : ""}`),
        picker,
        h("div", { class: "opt-grid" }, ...opts.map(optBtn)));
    }));
  }

  let lastSig = "", view = null;
  async function poll() {
    let r;
    try { r = await api("/career"); } catch (_) { return; }
    const top = document.getElementById("career-top");
    if (!r.active) {
      setup.style.display = ""; office.style.display = "none";
      if (top) top.textContent = "";
      view = null;
      return;
    }
    view = r.career;
    setup.style.display = "none"; office.style.display = "";
    if (top) top.textContent = `${view.date} · ${M(view.money)}`;
    const v = view;
    status.replaceChildren(...[
      h("div", { class: "kpis" },
        kpi("DATE", v.date), kpi("FUNDS", M(v.money)), kpi("PHASE", v.phase === "build" ? "CONSTRUCTION" : "OPERATING"),
        kpi("UNIT SIZE", sizeLabel(v.unit_size)),
        kpi("POWER PRICE", `$${v.price_mwh}/MWh`), kpi("O&M", `${M(v.om_per_day)}/day`),
        kpi("REVENUE", M(v.revenue_total)), kpi("SPENT", M(v.costs_total))),
      v.phase === "operating" ? h("div", { class: "bills" },
        h("div", { class: "card-name" }, "THIS MONTH (billed at month end)"),
        h("table", { class: "bill-table" },
          ...[["Fuel", v.bills.fuel], ["Cooling water", v.bills.water], ["Grid power import", v.bills.grid_power],
            ["Maintenance", v.bills.maintenance], ["Staff", v.bills.staff], ["Diesel fuel (paid)", v.bills.diesel_fuel],
            ["Fines & penalties (paid)", v.bills.penalties], ["Revenue so far", v.month_revenue]]
            .map(([k, x]) => h("tr", {}, h("td", {}, k), h("td", { class: "num" }, M(x))))),
        v.statements.length ? h("div", { class: "card-sub" }, `Last statement ${v.statements[0].month}: charged ${M(
          ["fuel", "water", "grid_power", "maintenance", "staff"].reduce((a, k) => a + v.statements[0].bills[k], 0))}, revenue ${M(v.statements[0].revenue)}`) : null) : null,
      v.phase === "build" && v.missing_components.length
        ? h("p", { class: "note warn" }, `Still to equip: ${v.missing_components.map(s => s.replaceAll("_", " ")).join(", ")}`) : null,
    ].filter(Boolean));
    actions.replaceChildren(...[
      v.phase === "build" ? h("button", { class: "btn big", disabled: v.missing_components.length > 0, onclick: async () => { if (await post("commission")) { lastSig = ""; poll(); } } }, "COMMISSION UNIT") : null,
      h("button", { class: "btn", onclick: async () => { if (confirm("Abandon this career and return to sandbox?")) { await post("quit"); lastSig = ""; poll(); } } }, "QUIT CAREER"),
    ].filter(Boolean));
    news.replaceChildren(...v.notifications.map(n => h("li", {}, `${n.date}  ${n.text}`)));
    // Funding levels: [speed, extra spend per day × normal daily cost, label].
    const FUNDING = [[1, 0, "NORMAL"], [2, 1.5, "ACCELERATED ×2"], [3.5, 4, "CRASH ×3.5"]];
    research.replaceChildren(...v.research.map(p => {
      const lvl = p.funding || 0;
      const daily = p.cost / Math.max(1, p.days);
      const left = Math.max(0, p.days - p.progress_days) / FUNDING[lvl][0];
      return h("div", { class: "card" },
        h("div", { class: "card-name" }, p.name),
        h("div", { class: "progress" }, h("div", { style: `width:${Math.min(100, p.progress_days / p.days * 100)}%` })),
        h("div", { class: "card-sub" }, `${p.progress_days.toFixed(0)} / ${p.days.toFixed(0)} research-days · about ${left.toFixed(0)} days left`),
        h("div", { class: "size-pick" }, h("span", {}, "FUNDING"),
          ...FUNDING.map(([, extra, label], i) => h("button", {
            class: `size-btn ${i === lvl ? "on" : ""}`,
            title: extra ? `+${M(daily * extra)} per day` : "No extra cost",
            onclick: async () => { if (await post("research_funding", { unlock: p.unlock, level: i })) poll(); },
          }, extra ? `${label} +${M(daily * extra * 30)}/mo` : label))));
    }));

    const sig = JSON.stringify([v.phase, v.money > 0, v.year, v.shop, v.research_options.length, v.research.length]);
    if (sig === lastSig) return;
    lastSig = sig;
    researchOpts.replaceChildren(...v.research_options.sort((a, b) => a.year - b.year).map(o => h("div", { class: "research-row" },
      h("span", { class: "r-name" }, o.name), h("span", { class: "r-year" }, `normally ${o.year}`),
      h("span", { class: "r-cost" }, `${M(o.cost)} · ${o.days.toFixed(0)} d`),
      o.prerequisite ? h("span", { class: "r-pre" }, `needs ${o.prerequisite}`)
        : h("button", { class: "btn", onclick: async () => { if (await post("research", o.unlock)) { lastSig = ""; poll(); } } }, "RESEARCH"))));
    renderShopComponents(v);
    shopCtl.replaceChildren(...v.shop.controllers.map(m => h("div", { class: "card" },
      h("div", { class: "card-name" }, m.name),
      h("div", { class: "card-sub" }, `${m.panel} · installed: ${m.installed ? (m.installed.kind === "digital" ? "Digital" : `Analog ${ERA_LABEL[m.installed.era]} ${TIER_LABEL[m.installed.tier]}`) : "NONE"}`),
      m.requires.length ? h("div", { class: "card-sub" }, `digital needs: ${m.requires.map(r => r.replaceAll("_", " ")).join(" + ")}`) : null,
      h("div", { class: "opt-grid" }, ...m.options.map(optBtn)))));
    shopHw.replaceChildren(...v.shop.hardware.map(q => h("div", { class: "card" }, h("div", { class: "opt-grid one" }, optBtn(q)))));
  }
  const kpi = (k, val) => h("div", { class: "kpi" }, h("div", { class: "kpi-k" }, k), h("div", { class: "kpi-v" }, val));

  setInterval(poll, 1000);
  poll();
  return () => {};
}
