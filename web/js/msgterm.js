// Plant messaging terminal on the reactor board. Its face depends on what the
// plant has installed:
//   vintage (original plant): lamp message panel, one lit window per caller
//     location; press a window to read and acknowledge its message.
//   dated (Plant Messaging Terminal): green-screen CRT with side pushbuttons
//     (POWER, RESET, ACK, scroll) and function-key requests.
//   modern (Plant Messaging HMI + plant computer): workstation window with a
//     message table, mouse acknowledgement and free-text requests.

import { h, plate, selector } from "./widgets.js";
import { api, toast } from "./api.js";
import { play } from "./sound.js";

const REQUESTS = [
  ["turbine_roll_checks", "ROLL CHECKS", "Turbine hall: turbine roll checks"],
  ["sync_permission", "SYNC PERMIT", "Grid dispatch: permission to synchronise"],
  ["report_online", "REPORT ON-LINE", "Grid dispatch: unit on-line"],
  ["report_trip", "REPORT TRIP", "Grid dispatch: unit trip"],
  ["maintenance", "MAINT WALKDOWN", "Maintenance: walkdown"],
  ["boron_sample", "BORON SAMPLE", "Chemistry: RCS boron sample"],
];
const CALLERS = ["Turbine Hall", "Grid Dispatch", "Maintenance", "Chemistry", "Plant Operator"];
const STATIONS = ["Grid Dispatch", "Turbine Hall", "Maintenance", "Chemistry", "Plant Operator"];
const SHORT = { "Grid Dispatch": "GRID", "Turbine Hall": "TURB", "Maintenance": "MAINT", "Chemistry": "CHEM", "Plant Operator": "OPER" };

const hms = s => `${String(Math.floor(s / 3600)).padStart(2, "0")}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(Math.floor(s) % 60).padStart(2, "0")}`;

export function messagingTerminal(ctx) {
  // Modern quick buttons are labelled with their destination.
  const call = async c => { play("button"); await api("/station/call", { call: c }); ctx.pollStation(); };
  // Older systems address requests with a station selector: the request
  // goes wherever the selector points.
  const callTo = async (c, to) => {
    play("button");
    const r = await api("/station/call_to", { call: c, to });
    if (!r.ok) { toast(r.error); play("deny"); }
    ctx.pollStation();
  };
  const ack = async seq => { await api("/station/ack", { seq: seq ?? null }); ctx.pollStation(); };

  // ------------------------------------------------------------ vintage
  let shownCaller = null;
  const strip = h("div", { class: "mp-strip" }, "NO MESSAGE");
  const wins = CALLERS.map(c => {
    const loc = h("div", { class: "mp-loc" }, "");
    const w = h("button", { class: "mp-win", onclick: () => {
      play("button");
      shownCaller = c;
      const st = ctx.station();
      (st?.messages ?? []).filter(m => m.from === c && !m.acked).forEach(m => ack(m.seq));
    } }, h("div", { class: "mp-name" }, c.toUpperCase()), loc);
    return { c, w, loc };
  });
  let vDest = "Grid Dispatch";
  const vSel = selector("CALL STATION", STATIONS.map(d => ({ label: SHORT[d], value: d })), v => { vDest = v; vSel.update(vDest); });
  vSel.update(vDest);
  const vReq = REQUESTS.map(([c, label]) => h("button", { class: "mp-req", onclick: () => callTo(c, vDest) }, label));
  const vintage = h("div", { class: "msg-vintage" },
    h("div", { class: "mp-grid" }, ...wins.map(x => x.w)),
    strip,
    h("div", { class: "row tight" }, vSel.el,
      h("div", { class: "mp-reqs" }, h("div", { class: "mp-reqs-l" }, "REQUEST →"), ...vReq)));

  // ------------------------------------------------------------ dated
  let power = true, scroll = 0, dDest = 0;
  const screen = h("pre", { class: "crt-screen" });
  const fkeys = REQUESTS.map(([c, label], i) => h("button", { class: "crt-fkey", title: `${label} → selected destination`, onclick: () => { if (power) callTo(c, STATIONS[dDest]); } }, `F${i + 1}`));
  const side = (label, fn, cls = "") => h("div", { class: "crt-side" }, h("button", { class: `crt-pb ${cls}`, onclick: () => { play("button"); fn(); } }), h("span", {}, label));
  const dated = h("div", { class: "msg-dated" },
    h("div", { class: "crt-bezel" }, h("div", { class: "crt" }, screen), h("div", { class: "crt-fkeys" }, ...fkeys)),
    h("div", { class: "crt-sides" },
      side("POWER", () => { power = !power; }, "pwr"),
      side("RESET", () => { scroll = 0; }),
      side("ACK", () => { if (power) ack(null); }, "ack"),
      side("DEST", () => { if (power) { dDest = (dDest + 1) % STATIONS.length; lastSig = ""; } }, "dest"),
      side("PG UP", () => { scroll += 6; }),
      side("PG DN", () => { scroll = Math.max(0, scroll - 6); })));

  // ------------------------------------------------------------ modern
  const tbody = h("tbody");
  const input = h("input", { class: "hmi-input", type: "text", maxlength: 300, placeholder: "Type a message, e.g. \"calling for turbine runup\" — Enter to send" });
  const dest = h("select", { class: "hmi-dest", title: "Who the message goes to" },
    h("option", { value: "" }, "TO: select…"),
    ...["Grid Dispatch", "Turbine Hall", "Maintenance", "Chemistry", "Plant Operator"].map(d => h("option", { value: d }, d)));
  const send = async () => {
    const text = input.value.trim();
    if (!text) return;
    if (!dest.value) { toast("Choose who the message is for (TO:)"); play("deny"); dest.focus(); return; }
    input.value = "";
    play("button");
    const r = await api("/station/message", { text, to: dest.value });
    if (!r.ok) { toast(r.error); play("deny"); }
    ctx.pollStation();
  };
  input.addEventListener("keydown", e => { e.stopPropagation(); if (e.key === "Enter") send(); });
  const modern = h("div", { class: "msg-modern" },
    h("div", { class: "hmi-title" }, h("span", {}, "PLANT COMMUNICATIONS — UNIT 1"), h("button", { class: "hmi-btn", onclick: () => ack(null) }, "ACK ALL")),
    h("div", { class: "hmi-table-wrap" }, h("table", { class: "hmi-table" },
      h("thead", {}, h("tr", {}, h("th", {}, "TIME"), h("th", {}, "FROM"), h("th", {}, "LOCATION"), h("th", {}, "MESSAGE"), h("th", {}, "ACK"))), tbody)),
    h("div", { class: "hmi-quick" }, ...REQUESTS.map(([c, , title]) => h("button", { class: "hmi-btn", title, onclick: () => call(c) }, title.split(": ")[1]))),
    h("div", { class: "hmi-send" }, dest, input, h("button", { class: "hmi-btn primary", onclick: send }, "SEND")));

  const el = h("section", { class: "sect wide", "data-section": "reactor.messages" }, plate("PLANT MESSAGING"), vintage, dated, modern);

  let lastSig = "";
  return {
    el,
    update(t) {
      const inst = id => t.upgrades.find(u => u.id === id)?.installed;
      const era = inst("messaging_hmi") && inst("plant_process_computer") ? "modern" : inst("message_terminal") ? "dated" : "vintage";
      vintage.style.display = era === "vintage" ? "" : "none";
      dated.style.display = era === "dated" ? "" : "none";
      modern.style.display = era === "modern" ? "" : "none";
      const st = ctx.station();
      if (!st) return;
      const msgs = st.messages ?? [];            // newest first
      const sig = `${era}|${power}|${scroll}|${msgs.length && msgs[0].seq}|${st.unacked}|${shownCaller}|${Math.floor(t.sim_time)}`;
      if (sig === lastSig) return;
      lastSig = sig;

      if (era === "vintage") {
        for (const x of wins) {
          const mine = msgs.filter(m => m.from === x.c);
          x.w.classList.toggle("flash", mine.some(m => !m.acked));
          x.w.classList.toggle("lit", mine.length > 0 && x.c === shownCaller);
          x.loc.textContent = mine[0]?.location ?? "";
        }
        const m = msgs.find(m => m.from === shownCaller);
        strip.textContent = m ? `${hms(m.sim_time)}  ${m.location}:  ${m.text}` : "NO MESSAGE — press a lit window to read it";
      } else if (era === "dated") {
        if (!power) { screen.textContent = ""; screen.classList.add("off"); return; }
        screen.classList.remove("off");
        const W = 64;
        const lines = [];
        for (const m of [...msgs].reverse()) {
          const head = `${m.acked ? " " : "*"}${hms(m.sim_time)} ${m.location.padEnd(15).slice(0, 15)} `;
          let body = `${m.from.toUpperCase()}: ${m.text}`;
          let first = true;
          while (body.length) {
            const n = W - head.length;
            lines.push((first ? head : " ".repeat(head.length)) + body.slice(0, n));
            body = body.slice(n);
            first = false;
          }
        }
        const rows = 11;
        const end = Math.max(0, lines.length - scroll);
        const view = lines.slice(Math.max(0, end - rows), end);
        while (view.length < rows) view.unshift("");
        screen.textContent = [
          `PLANT MESSAGE SYSTEM  V2.1     UNIT 1      ${hms(t.sim_time)}`,
          "=".repeat(W),
          ...view,
          "=".repeat(W),
          `UNACK: ${String(st.unacked).padStart(2)}   ${scroll ? `SCROLL -${scroll}` : "LIVE"}   DEST: ${STATIONS[dDest].toUpperCase()}`,
          "F1 ROLL  F2 SYNC  F3 ONLINE  F4 TRIP  F5 MAINT  F6 BORON",
          "C:\\PMS> _",
        ].join("\n");
      } else {
        tbody.replaceChildren(...msgs.slice(0, 40).map(m => h("tr", { class: m.acked ? "" : "unacked" },
          h("td", {}, hms(m.sim_time)), h("td", {}, m.from), h("td", {}, m.location), h("td", {}, m.text),
          h("td", {}, m.acked ? "✓" : h("button", { class: "hmi-btn small", onclick: () => ack(m.seq) }, "ACK")))));
      }
    },
  };
}
