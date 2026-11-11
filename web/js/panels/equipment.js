// Equipment cabinet: I&C hardware (computers + panel packages) and the
// automation modules that run on them. In sandbox everything can be
// installed / removed freely; career mode will buy them instead.

import { h, plate } from "../widgets.js";

export const id = "equipment";
export const title = "Equipment";
export const windows = [];

const money = n => `$${(n / 1e6).toFixed(1)}M`;

export function build(root, ctx) {
  const computers = h("div", { class: "equip-grid" });
  const panels = h("div", { class: "equip-grid" });
  const mods = h("div", { class: "equip-grid" });
  const modeNote = h("p", { class: "note" });
  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide" }, plate("I&C HARDWARE — COMPUTERS"),
      h("p", { class: "note" }, "Computers run automation, but cannot see or drive a panel until that panel's package is installed."),
      modeNote, computers),
    h("section", { class: "sect wide" }, plate("I&C HARDWARE — PANEL PACKAGES"),
      h("p", { class: "note" }, "A panel package replaces the original analog equipment of one board area with modern controls and displays, and wires it to its computer. Only the sections it covers change; the rest of the board keeps its original instruments."),
      panels),
    h("section", { class: "sect wide" }, plate("AUTOMATION MODULES"),
      h("p", { class: "note" }, "Software modules, one per subsystem. A module can run in AUTO only when it is installed and its computer and panel package are present. MAN: you drive it. AUTO: the module does. EXT: an approved plugin does (needs the External Control Gateway)."),
      mods)));

  root.addEventListener("click", e => {
    const b = e.target.closest("button");
    if (!b) return;
    const card = b.closest("[data-id]");
    if (!card) return;
    if (b.dataset.mode) ctx.setMode(card.dataset.id, b.dataset.mode);
    else if (b.dataset.install !== undefined) {
      const installed = b.dataset.install !== "1";
      if (card.dataset.kind === "module") ctx.send({ type: "set_module_installed", module: card.dataset.id, installed });
      else ctx.send({ type: "set_upgrade_installed", upgrade: card.dataset.id, installed });
    }
  });
  root.addEventListener("change", e => {
    const el = e.target.closest("[data-sp]");
    if (!el) return;
    ctx.send({ type: "set_setpoint", module: el.closest("[data-id]").dataset.id, key: el.dataset.sp, value: +el.value });
    el.blur();
  });

  let sig = "";
  return t => {
    const career = t.game_mode === "career";
    const hwName = id => t.upgrades.find(u => u.id === id)?.name ?? id;
    const hwOn = id => t.upgrades.find(u => u.id === id)?.installed;
    const s = JSON.stringify([career, t.upgrades.map(u => u.installed), t.modules.map(m => [m.mode, m.owned, Object.keys(m.setpoints)])]);
    if (s !== sig) {
      sig = s;
      modeNote.textContent = career ? "Career mode: equipment is bought and installed through the plant shop." : "Sandbox: click INSTALLED to add or remove equipment.";
      const instBtn = on => h("button", { class: `inst ${on ? "on" : ""}`, "data-install": on ? "1" : "0", disabled: career }, on ? "INSTALLED" : "NOT INSTALLED");
      const hwCard = u => h("div", { class: `card hw ${u.kind === "computer" ? "computer" : ""} ${u.installed ? "installed" : ""}`, "data-id": u.id, "data-kind": "upgrade" },
        h("div", { class: "card-head" },
          h("div", {}, h("div", { class: "kind" }, u.kind === "computer" ? "COMPUTER" : `PANEL PACKAGE → ${hwName(u.computer)}`),
            h("div", { class: "card-name" }, u.name), h("div", { class: "price" }, money(u.price))),
          instBtn(u.installed)),
        h("div", { class: "card-sub" }, u.description),
        u.sections.length ? h("div", { class: "secs" }, `board: ${u.sections.join(", ")}`) : null,
        u.kind === "panel_package" && u.installed && !hwOn(u.computer)
          ? h("div", { class: "req" }, h("span", {}, `no link: ${hwName(u.computer)} not installed`)) : null);
      computers.replaceChildren(...t.upgrades.filter(u => u.kind === "computer").map(hwCard));
      panels.replaceChildren(...t.upgrades.filter(u => u.kind !== "computer").map(hwCard));
      mods.replaceChildren(...t.modules.map(m => h("div", { class: `card ${m.implemented ? "" : "na"} ${m.mode}`, "data-id": m.id, "data-kind": "module" },
        h("div", { class: "card-head" },
          h("div", {}, h("div", { class: "card-name" }, m.name),
            h("div", { class: "card-sub" }, `${m.panel} · ${money(m.price)}${m.implemented ? "" : " · not simulated yet"}`)),
          instBtn(m.owned)),
        h("div", { class: "req" }, ...m.requires_upgrades.map(u => h("span", { class: hwOn(u) ? "ok" : "" }, `${hwOn(u) ? "✓" : "✗"} ${hwName(u)}`))),
        h("div", { class: "card-head", style: "margin-top:6px" }, h("span", {}),
          h("div", { class: "seg-btns" }, ...["manual", "auto", "external"].map(x =>
            h("button", { "data-mode": x, class: m.mode === x ? "on" : "", disabled: x !== "manual" && (!m.implemented || !m.owned || m.requires_upgrades.some(u => !hwOn(u))) }, x === "external" ? "EXT" : x === "manual" ? "MAN" : "AUTO")))),
        h("div", { class: "card-sp" }, ...Object.entries(m.setpoints).map(([k, v]) =>
          h("label", {}, k.replaceAll("_", " "), h("input", { type: "number", step: "any", "data-sp": k, value: +v.toFixed(3) })))))));
    } else {
      for (const m of t.modules) for (const [k, v] of Object.entries(m.setpoints)) {
        const el = mods.querySelector(`[data-id="${m.id}"] [data-sp="${k}"]`);
        if (el && document.activeElement !== el) el.value = +v.toFixed(3);
      }
    }
  };
}
