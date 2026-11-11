// Communication / maintenance panel: plant telephones.

import { h, plate, lamp } from "../widgets.js";
import { api } from "../api.js";
import { play } from "../sound.js";

export const id = "comms";
export const title = "Comms";
export const windows = [];

const fmt = s => `${Math.floor(s / 3600)}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(Math.floor(s) % 60).padStart(2, "0")}`;

const LINES = [
  { party: "TURBINE HALL", calls: [["turbine_roll_checks", "REQUEST ROLL CHECKS", "Turning gear, drains, gland seals. Needed before latching the turbine (career)."]] },
  { party: "GRID DISPATCH", calls: [
    ["sync_permission", "PERMISSION TO SYNC", "Call at synchronous speed. Needed before closing the generator breaker (career)."],
    ["report_online", "REPORT ON-LINE", ""],
    ["report_trip", "REPORT TRIP", ""],
  ] },
  { party: "MAINTENANCE", calls: [["maintenance", "REQUEST WALKDOWN", "Field inspection of plant equipment."]] },
  { party: "CHEMISTRY", calls: [["boron_sample", "RCS BORON SAMPLE", "Lab sample of RCS boron (takes ~10 min)."]] },
];

export function build(root, ctx) {
  const log = h("ol", { class: "events news phone-log" });
  const hall = lamp("green", "TURBINE HALL READY");
  const perm = lamp("green", "SYNC PERMISSION");
  const sample = h("div", { class: "kpi" }, h("div", { class: "kpi-k" }, "LAST BORON SAMPLE"), h("div", { class: "kpi-v" }, "—"));
  const phones = LINES.map(line => h("div", { class: "phone" },
    h("div", { class: "phone-head" }, "☎ ", line.party),
    ...line.calls.map(([call, label, note]) => h("div", { class: "phone-call" },
      h("button", { class: "btn", onclick: async () => {
        play("pickup");
        await api("/station/call", { call });
        ctx.pollStation();
      } }, label),
      note ? h("span", { class: "card-sub" }, note) : null))));

  root.append(h("div", { class: "board" },
    h("section", { class: "sect wide", "data-section": "comms.phones" }, plate("COMMUNICATIONS & MAINTENANCE"),
      h("p", { class: "note" }, "In career mode the turbine hall must complete roll checks before the turbine can be latched, and grid dispatch must grant permission before you synchronise (closing without it costs a fine). In sandbox the calls are optional."),
      h("div", { class: "row tight" }, hall.el, perm.el, sample),
      h("div", { class: "phones" }, ...phones)),
    h("section", { class: "sect wide", "data-section": "comms.phones" }, plate("CALL LOG"), log)));

  return t => {
    hall.update(t.turbine_hall_ready);
    perm.update(t.grid_permission);
    const st = ctx.station();
    if (!st) return;
    sample.lastChild.textContent = st.boron_sample != null ? `${st.boron_sample.toFixed(0)} ppm` : "—";
    log.replaceChildren(...st.messages.slice(0, 60).map(m => h("li", { class: m.from === "Control Room" ? "out" : "in" }, `${fmt(m.sim_time)}  ${m.from}: ${m.text}`)));
  };
}
