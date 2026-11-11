// Career office: start a career, build the plant within budget, shop for
// equipment, research technology early, and follow the calendar.

import { h, plate } from "../widgets.js";
import { api, toast } from "../api.js";

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
  const shopComp = h("div", { class: "equip-grid" });
  const shopCtl = h("div", { class: "equip-grid" });
  const shopHw = h("div", { class: "equip-grid" });
  office.append(
    h("section", { class: "sect wide" }, plate("PLANT OFFICE"), status, actions),
    h("section", { class: "sect" }, plate("NOTIFICATIONS"), news),
    h("section", { class: "sect" }, plate("RESEARCH & DEVELOPMENT"), research, researchOpts),
    h("section", { class: "sect wide" }, plate("SHOP — PLANT COMPONENTS"), h("p", { class: "note" }, "During operation, major components can only be swapped in an outage (reactor shut down, generator off-line). Replaced equipment is sold for 20 %; during construction swaps are fully refunded."), shopComp),
    h("section", { class: "sect wide" }, plate("SHOP — AUTOMATION CONTROLLERS"), h("p", { class: "note" }, "Analog cabinets need no computer but hold their setpoints less precisely (vintage < dated, basic < premium). Digital software is precise but needs its computer and panel package."), shopCtl),
    h("section", { class: "sect wide" }, plate("SHOP — I&C HARDWARE"), shopHw));

  const confirmBuy = async q => {
    const kits = q.kits.length ? `\n+ ${q.kits.map(k => `${k.name} (${M(k.price)})`).join("\n+ ")}` : "";
    const refund = q.refund ? `\n− refund ${M(q.refund)}` : "";
    if (!confirm(`Buy ${q.label} for ${M(q.price)}${kits}${refund}\nTotal ${M(q.total)}?`)) return;
    if (await post("buy", q.purchase)) { lastSig = ""; poll(); }
  };

  const optBtn = q => h("button", {
    class: `opt ${q.owned ? "owned" : ""} ${q.available ? "" : "locked"} ${q.kits.length ? "kit" : ""}`,
    disabled: !q.available || q.owned,
    title: q.available ? (q.kits.length ? `Needs: ${q.kits.map(k => k.name).join(", ")}` : "") : `Available ${q.year} (or research it)`,
    onclick: () => confirmBuy(q),
  }, h("span", { class: "opt-l" }, q.label), h("span", { class: "opt-p" }, q.owned ? "INSTALLED" : q.available ? M(q.total) : `${q.year}`),
     q.perf ? h("span", { class: "opt-perf" }, `${(q.perf * 100).toFixed(1)}%`) : null);

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
    status.replaceChildren(
      h("div", { class: "kpis" },
        kpi("DATE", v.date), kpi("FUNDS", M(v.money)), kpi("PHASE", v.phase === "build" ? "CONSTRUCTION" : "OPERATING"),
        kpi("POWER PRICE", `$${v.price_mwh}/MWh`), kpi("O&M", `${M(v.om_per_day)}/day`),
        kpi("REVENUE", M(v.revenue_total)), kpi("SPENT", M(v.costs_total))),
      v.phase === "build" && v.missing_components.length
        ? h("p", { class: "note warn" }, `Still to equip: ${v.missing_components.map(s => s.replaceAll("_", " ")).join(", ")}`) : null);
    actions.replaceChildren(
      v.phase === "build" ? h("button", { class: "btn big", disabled: v.missing_components.length > 0, onclick: async () => { if (await post("commission")) { lastSig = ""; poll(); } } }, "COMMISSION UNIT") : null,
      h("button", { class: "btn", onclick: async () => { if (confirm("Abandon this career and return to sandbox?")) { await post("quit"); lastSig = ""; poll(); } } }, "QUIT CAREER"));
    news.replaceChildren(...v.notifications.map(n => h("li", {}, `${n.date}  ${n.text}`)));
    research.replaceChildren(...v.research.map(p => h("div", { class: "card" },
      h("div", { class: "card-name" }, p.name),
      h("div", { class: "progress" }, h("div", { style: `width:${Math.min(100, p.progress_days / p.days * 100)}%` })),
      h("div", { class: "card-sub" }, `${p.progress_days.toFixed(0)} / ${p.days.toFixed(0)} days`))));

    const sig = JSON.stringify([v.phase, v.money > 0, v.year, v.shop, v.research_options.length, v.research.length]);
    if (sig === lastSig) return;
    lastSig = sig;
    researchOpts.replaceChildren(...v.research_options.sort((a, b) => a.year - b.year).map(o => h("div", { class: "research-row" },
      h("span", { class: "r-name" }, o.name), h("span", { class: "r-year" }, `normally ${o.year}`),
      h("span", { class: "r-cost" }, `${M(o.cost)} · ${o.days.toFixed(0)} d`),
      o.prerequisite ? h("span", { class: "r-pre" }, `needs ${o.prerequisite}`)
        : h("button", { class: "btn", onclick: async () => { if (await post("research", o.unlock)) { lastSig = ""; poll(); } } }, "RESEARCH"))));
    shopComp.replaceChildren(...v.shop.components.map(s => h("div", { class: "card" },
      h("div", { class: "card-name" }, s.name),
      h("div", { class: "card-sub" }, `${s.effect} · installed: ${s.installed ? `${ERA_LABEL[s.installed.era]} ${TIER_LABEL[s.installed.tier]}` : "NONE"}`),
      h("div", { class: "opt-grid" }, ...s.options.map(optBtn)))));
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
