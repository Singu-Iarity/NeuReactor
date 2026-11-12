import { mweScale } from "../rating.js";
import { h, plate, recorder, pushbutton } from "../widgets.js";
import { api } from "../api.js";
import * as sound from "../sound.js";
import { settings } from "../settings.js";

export const id = "computer";
export const title = "Plant Computer";
export const windows = [["training", "TRAINING MODE: RPS BYPASSED"], ["ic_scale", "I&C NOT SCALED FOR CORE SIZE"], ["", ""], ["", ""]];

function fmtTime(s) {
  s = Math.floor(s);
  return `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

export function build(root, ctx) {
  const recs = [
    recorder("NUCLEAR POWER", [
      { tag: "ir_amps", label: "IR current (log)", color: "#7a3ad6", min: 1e-11, max: 1e-3, log: true },
      { tag: "nuclear_power_pct", label: "Power % (0–120)", color: "#d6261c", min: 0, max: 120 },
    ]),
    recorder("PRIMARY", [
      { tag: "tavg_c", label: "Tavg 270–330 °C", color: "#d6261c", min: 270, max: 330 },
      { tag: "rcs_pressure_mpa", label: "Press 12–18 MPa", color: "#1c5bd6", min: 12, max: 18 },
      { tag: "pzr_level_pct", label: "PZR lvl %", color: "#1f8a3a", min: 0, max: 100 },
    ]),
    recorder("SECONDARY / GENERATOR", [
      { tag: "sg_level_pct", label: "SG lvl %", color: "#1f8a3a", min: 0, max: 100 },
      { tag: "gross_mwe", label: `MWe 0–${mweScale()}`, color: "#d6261c", min: 0, max: mweScale() },
      { tag: "turbine_speed_rpm", label: "rpm 0–2000", color: "#1c5bd6", min: 0, max: 2000 },
    ]),
  ];
  const events = h("ol", { class: "events", reversed: true });
  const plugins = h("div", { class: "plugins" });
  const sc = (name, label) => pushbutton(label, { onPress: () => { if (confirm(`Load scenario "${label}"? The current plant state is discarded.`)) ctx.send({ type: "load_scenario", name }); } });
  const hs = sc("hot_standby", "HOT STANDBY"), fp = sc("full_power", "FULL POWER");
  const analog = pushbutton("ORIGINAL PLANT", { onPress: () => { if (confirm("Strip the unit back to its original analog, manual equipment?")) ctx.send({ type: "set_control_room", generation: "analog" }); } });
  const digital = pushbutton("FULLY MODERNISED", { onPress: () => ctx.send({ type: "set_control_room", generation: "digital" }) });
  const training = pushbutton("TRAINING MODE", { color: "amber", onPress: () => ctx.send({ type: "set_training_mode", enabled: !ctx.t().training_mode }) });
  const repair = pushbutton("REPAIR GENERATOR", { onPress: () => ctx.send({ type: "repair_generator" }) });
  const lampBtn = pushbutton("LAMPS: GREEN = ON", { onPress: () => settings.set("lamps", settings.get("lamps") === "green_on" ? "us" : "green_on") });
  const shakeBtn = pushbutton("SCREEN SHAKE", { onPress: () => settings.set("shake", !settings.get("shake")) });
  const sndBtns = ["ambience", "alarms", "effects"].map(k => pushbutton(`${k.toUpperCase()} SOUND`, {
    onPress: () => { sound.unlock(); sound.settings.setPref(k, !sound.settings.prefs[k]); },
  }));

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "computer.recorders" }, plate("RECORDERS"), h("div", { class: "row" }, ...recs.map(r => r.el))),
    h("section", { class: "sect", "data-section": "computer.log" }, plate("EVENT LOG"), events),
    h("section", { class: "sect", "data-section": "computer.instructor" }, plate("SANDBOX / INSTRUCTOR STATION"),
      h("div", { class: "row tight" }, hs.el, fp.el, analog.el, digital.el, training.el, repair.el),
      plate("DISPLAY", "small"), h("div", { class: "row tight" }, lampBtn.el, shakeBtn.el),
      h("p", { class: "note" }, "Lamp convention: US plants use red = running/closed and green = stopped/open. Light the button to use green = running instead."),
      plate("SOUND", "small"), h("div", { class: "row tight" }, ...sndBtns.map(b => b.el)),
      plate("PLUGINS", "small"), plugins)));

  plugins.addEventListener("click", async e => {
    const b = e.target.closest("button[data-plugin]");
    if (!b) return;
    await api("/plugins/approve", { id: b.dataset.plugin, approve: b.dataset.approve === "1" });
    pollPlugins();
  });

  let lastSeq = 0;
  async function pollEvents() {
    try {
      const ev = await api(`/events?since=${lastSeq}`);
      for (const e of ev) {
        lastSeq = Math.max(lastSeq, e.seq);
        events.prepend(h("li", { class: e.severity }, `${fmtTime(e.sim_time)}  ${e.text}`));
      }
      while (events.children.length > 300) events.lastChild.remove();
    } catch (_) { /* reported by telemetry poll */ }
  }
  async function pollPlugins() {
    try {
      const list = await api("/plugins");
      plugins.replaceChildren(...(list.length ? list.map(p => h("div", { class: "plugin" },
        h("div", {}, h("b", {}, p.name), h("div", { class: "card-sub" }, p.scopes.join(", "))),
        h("div", {},
          p.approved ? null : h("button", { class: "btn", "data-plugin": p.id, "data-approve": "1" }, "Approve"),
          h("button", { class: "btn", "data-plugin": p.id, "data-approve": "0" }, p.approved ? "Revoke" : "Deny"))))
        : [h("p", { class: "note" }, "No plugins connected.")]));
    } catch (_) {}
  }
  setInterval(pollEvents, 1000);
  setInterval(pollPlugins, 2000);
  pollEvents(); pollPlugins();

  return (t, visible) => {
    for (const r of recs) r.push(t.sim_time, t.readings);
    if (!visible) return;   // keep recording while another tab is shown
    recs.forEach(r => r.draw());
    hs.update(t.scenario === "hot_standby"); fp.update(t.scenario === "full_power");
    analog.update(t.upgrades.every(u => !u.installed)); digital.update(t.upgrades.every(u => u.installed));
    training.update(t.training_mode);
    lampBtn.update(settings.get("lamps") === "green_on");
    shakeBtn.update(settings.get("shake"));
    ["ambience", "alarms", "effects"].forEach((k, i) => sndBtns[i].update(sound.settings.prefs[k]));
    repair.update(false, t.readings.generator_health_pct >= 100);
    if (t.game_mode === "career") [hs, fp, analog, digital, training, repair].forEach(b => b.update(false, true));
  };
}
