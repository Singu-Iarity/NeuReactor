import { h, plate } from "../widgets.js";

export const id = "automation";
export const title = "Automation";
export const windows = [];

export function build(root, ctx) {
  const rack = h("div", { class: "rack" });
  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide" }, plate("AUTOMATION MODULE CABINET"),
      h("p", { class: "note" }, "Each subsystem has its own controller module. MANUAL: you drive the actuators. AUTO: the built-in controller does. EXT: an approved plugin does (needs the External Control Gateway). In career mode, modules must be bought and installed first."),
      rack)));

  rack.addEventListener("click", e => {
    const b = e.target.closest("button[data-mode]");
    if (b) ctx.setMode(b.closest("[data-id]").dataset.id, b.dataset.mode);
  });
  rack.addEventListener("change", e => {
    const el = e.target.closest("[data-sp]");
    if (!el) return;
    ctx.send({ type: "set_setpoint", module: el.closest("[data-id]").dataset.id, key: el.dataset.sp, value: +el.value });
    el.blur();
  });

  let sig = "";
  return t => {
    const s = JSON.stringify(t.modules.map(m => [m.id, m.mode, m.owned, Object.keys(m.setpoints)]));
    if (s !== sig) {
      sig = s;
      rack.replaceChildren(...t.modules.map(m => h("div", { class: `card ${m.implemented ? "" : "na"} ${m.mode}`, "data-id": m.id },
        h("div", { class: "card-head" },
          h("div", {}, h("div", { class: "card-name" }, m.name), h("div", { class: "card-sub" }, `${m.panel}${m.implemented ? "" : " · not simulated yet"}${m.owned ? "" : " · not installed"}`)),
          h("div", { class: "seg-btns" }, ...["manual", "auto", "external"].map(x =>
            h("button", { "data-mode": x, class: m.mode === x ? "on" : "", disabled: !m.implemented && x !== "manual" }, x === "external" ? "EXT" : x.toUpperCase())))),
        h("div", { class: "card-sp" }, ...Object.entries(m.setpoints).map(([k, v]) =>
          h("label", {}, k.replaceAll("_", " "), h("input", { type: "number", step: "any", "data-sp": k, value: +v.toFixed(3) })))))));
    } else {
      for (const m of t.modules) for (const [k, v] of Object.entries(m.setpoints)) {
        const el = rack.querySelector(`[data-id="${m.id}"] [data-sp="${k}"]`);
        if (el && document.activeElement !== el) el.value = +v.toFixed(3);
      }
    }
  };
}
