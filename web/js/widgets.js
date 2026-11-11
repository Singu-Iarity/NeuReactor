// Control board widgets: meters, readouts, switches, lamps, synchroscope,
// annunciator windows and strip-chart recorders. Every widget returns an
// object with update(...) and is styled by the active control room theme
// (body[data-room="analog"|"digital"]).

const NS = "http://www.w3.org/2000/svg";

export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (v !== undefined && v !== null && v !== false) el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined) continue;
    el.append(c instanceof Node ? c : document.createTextNode(c));
  }
  return el;
}

function s(tag, attrs = {}, ...children) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  for (const c of children) if (c) el.append(c);
  return el;
}

export const plate = (text, cls = "") => h("div", { class: `plate ${cls}` }, text);

function fmt(v, digits) {
  if (v === undefined || v === null || Number.isNaN(v)) return "----";
  if (!Number.isFinite(v)) return v > 0 ? "Inf" : "-Inf";
  return v.toFixed(digits);
}

export function sci(v) {
  if (!(v > 0)) return "0.0E+00";
  const e = Math.floor(Math.log10(v));
  const m = v / 10 ** e;
  return `${m.toFixed(1)}E${e >= 0 ? "+" : "-"}${String(Math.abs(e)).padStart(2, "0")}`;
}

// ------------------------------------------------------------- readout

/** Seven-segment style numeric readout with an engraved label strip. */
export function readout(label, { digits = 0, unit = "", format, size = "" } = {}) {
  const val = h("span", { class: "seg-val" }, "0");
  const el = h("div", { class: `readout ${size}` },
    h("div", { class: "seg" }, val, unit ? h("span", { class: "seg-unit" }, unit) : null),
    plate(label, "small"));
  return {
    el,
    update(v) { val.textContent = format ? format(v) : fmt(v, digits); },
  };
}

// --------------------------------------------------------------- meters

/**
 * Round panel meter (white face, black bezel). In the digital room it is
 * replaced by a bar + numeric readout.
 */
export function roundMeter(label, { min = 0, max = 100, unit = "", major = 5, minor = 4, bands = [], log = false, digits = 1, ref = false, size = 150 } = {}) {
  const W = size, H = size * 0.9, cx = W / 2, cy = H * 0.56, r = W * 0.40;
  const a0 = -120, a1 = 120; // degrees from vertical
  const norm = v => {
    if (log) {
      const lv = Math.log10(Math.max(v, min));
      return (lv - Math.log10(min)) / (Math.log10(max) - Math.log10(min));
    }
    return (v - min) / (max - min);
  };
  const ang = v => a0 + (a1 - a0) * Math.max(-0.02, Math.min(1.02, norm(v)));
  const pt = (deg, rad) => [cx + rad * Math.sin(deg * Math.PI / 180), cy - rad * Math.cos(deg * Math.PI / 180)];
  const arc = (d0, d1, rad) => {
    const [x0, y0] = pt(d0, rad), [x1, y1] = pt(d1, rad);
    return `M${x0},${y0} A${rad},${rad} 0 ${d1 - d0 > 180 ? 1 : 0} 1 ${x1},${y1}`;
  };
  const svg = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "meter-face" });
  svg.append(s("rect", { x: 2, y: 2, width: W - 4, height: H - 4, rx: 8, class: "mf-bg" }));
  for (const b of bands) svg.append(s("path", { d: arc(ang(b.from), ang(b.to), r - 3), class: `mf-band ${b.c}` }));
  svg.append(s("path", { d: arc(a0, a1, r), class: "mf-arc" }));
  const nMajor = log ? Math.round(Math.log10(max) - Math.log10(min)) : major;
  for (let i = 0; i <= nMajor; i++) {
    const v = log ? min * 10 ** i : min + (max - min) * i / nMajor;
    const d = ang(v);
    const [x0, y0] = pt(d, r), [x1, y1] = pt(d, r - 9);
    svg.append(s("line", { x1: x0, y1: y0, x2: x1, y2: y1, class: "mf-tick" }));
    const [tx, ty] = pt(d, r - 19);
    const t = s("text", { x: tx, y: ty + 3, class: "mf-num" });
    t.textContent = log ? `${Math.round(Math.log10(v))}` : +v.toFixed(Math.abs(max - min) < 10 ? 1 : 0);
    svg.append(t);
    if (i < nMajor && !log) {
      for (let j = 1; j <= minor; j++) {
        const dm = ang(v + (max - min) / nMajor * j / (minor + 1));
        const [mx0, my0] = pt(dm, r), [mx1, my1] = pt(dm, r - 5);
        svg.append(s("line", { x1: mx0, y1: my0, x2: mx1, y2: my1, class: "mf-tick minor" }));
      }
    }
  }
  const lbl = s("text", { x: cx, y: cy + r * 0.66, class: "mf-label" });
  lbl.textContent = label;
  svg.append(lbl);
  if (unit || log) {
    const u = s("text", { x: cx, y: cy + r * 0.66 + 10, class: "mf-unit" });
    u.textContent = log ? `log₁₀ ${unit}` : unit;
    svg.append(u);
  }
  const refPtr = ref ? s("path", { d: `M${cx - 4},${cy - r - 9} L${cx + 4},${cy - r - 9} L${cx},${cy - r + 1}Z`, class: "mf-ref" }) : null;
  if (refPtr) svg.append(refPtr);
  const needle = s("line", { x1: cx, y1: cy + 8, x2: cx, y2: cy - r + 4, class: "mf-needle" });
  svg.append(needle, s("circle", { cx, cy, r: 5, class: "mf-hub" }));

  const bar = h("div", { class: "dbar-fill" });
  const dval = h("span", { class: "seg-val" }, "0");
  const digital = h("div", { class: "dmeter" },
    h("div", { class: "seg" }, dval, h("span", { class: "seg-unit" }, unit)),
    h("div", { class: "dbar" }, bar));
  const el = h("div", { class: "meter" }, h("div", { class: "bezel only-analog" }, svg), h("div", { class: "only-digital" }, digital), plate(label, "small only-digital"));
  return {
    el,
    update(v, refV) {
      if (v === undefined) return;
      needle.setAttribute("transform", `rotate(${ang(v)} ${cx} ${cy})`);
      if (refPtr && refV !== undefined) refPtr.setAttribute("transform", `rotate(${ang(refV)} ${cx} ${cy})`);
      bar.style.width = `${Math.max(0, Math.min(1, norm(v))) * 100}%`;
      dval.textContent = log ? sci(v) : fmt(v, digits);
    },
  };
}

/** Horizontal edgewise meter (narrow scale with a sliding pointer). */
export function edgewise(label, { min = 0, max = 100, unit = "", major = 5, bands = [], digits = 1, log = false } = {}) {
  const norm = v => log
    ? (Math.log10(Math.max(v, min)) - Math.log10(min)) / (Math.log10(max) - Math.log10(min))
    : (v - min) / (max - min);
  const scale = h("div", { class: "ew-scale" });
  for (const b of bands) {
    scale.append(h("div", { class: `ew-band ${b.c}`, style: `left:${norm(b.from) * 100}%;width:${(norm(b.to) - norm(b.from)) * 100}%` }));
  }
  const n = log ? Math.round(Math.log10(max / min)) : major;
  for (let i = 0; i <= n; i++) {
    const v = log ? min * 10 ** i : min + (max - min) * i / n;
    scale.append(h("div", { class: `ew-tick ${i === 0 ? "first" : i === n ? "last" : ""}`, style: `left:${i / n * 100}%` },
      h("span", {}, log ? `${Math.round(Math.log10(v))}` : `${+v.toFixed(Math.abs(max - min) < 10 ? 1 : 0)}`)));
  }
  const ptr = h("div", { class: "ew-ptr" });
  scale.append(ptr);
  const dval = h("span", { class: "seg-val" }, "0");
  const el = h("div", { class: "edgewise" },
    h("div", { class: "ew-window only-analog" }, scale),
    h("div", { class: "seg only-digital" }, dval, h("span", { class: "seg-unit" }, unit)),
    plate(`${label}${unit && !log ? ` [${unit}]` : log ? ` [log ${unit}]` : ""}`, "small"));
  return {
    el,
    update(v) {
      if (v === undefined) return;
      ptr.style.left = `${Math.max(-1, Math.min(101, norm(v) * 100))}%`;
      dval.textContent = log ? sci(v) : fmt(v, digits);
    },
  };
}

/** Vertical bar indicator (rod position, levels). */
export function vbar(label, { max = 100, digits = 0 } = {}) {
  const fill = h("div", { class: "vb-fill" });
  const tgt = h("div", { class: "vb-tgt" });
  const num = h("div", { class: "vb-num" }, "0");
  const el = h("div", { class: "vbar" }, h("div", { class: "vb-col" }, fill, tgt), num, h("div", { class: "vb-label" }, label));
  return {
    el,
    update(v, target) {
      fill.style.height = `${Math.max(0, Math.min(1, v / max)) * 100}%`;
      if (target !== undefined) tgt.style.bottom = `${Math.max(0, Math.min(1, target / max)) * 100}%`;
      num.textContent = fmt(v, digits);
    },
  };
}

// --------------------------------------------------------------- lamps

export function lamp(color = "red", label = "") {
  const l = h("span", { class: `lamp ${color}` });
  const el = label ? h("span", { class: "lamp-wrap" }, l, h("span", { class: "lamp-label" }, label)) : l;
  return { el, update(on, blink = false) { l.classList.toggle("on", !!on); l.classList.toggle("blink", !!blink); } };
}

// ------------------------------------------------------------ switches

/**
 * Pistol-grip (J-handle) control switch with green (left) / red (right)
 * indicating lamps, US convention: red = closed/running/open valve.
 */
export function jhandle(label, { left = "TRIP", right = "CLOSE", onLeft, onRight, center = false } = {}) {
  const g = lamp("green"), r = lamp("red");
  const handle = s("g", { class: "jh-handle" });
  handle.append(
    s("circle", { cx: 50, cy: 50, r: 24, class: "jh-boss" }),
    s("path", { d: "M44,50 L46,6 Q50,0 54,6 L56,50 Z", class: "jh-grip" }),
  );
  const svg = s("svg", { viewBox: "0 0 100 80", class: "jh-svg" }, s("rect", { x: 6, y: 6, width: 88, height: 70, rx: 6, class: "jh-plate" }));
  const tl = s("text", { x: 14, y: 20, class: "jh-pos" }); tl.textContent = left;
  const tr = s("text", { x: 86, y: 20, class: "jh-pos", "text-anchor": "end" }); tr.textContent = right;
  svg.append(tl, tr, handle);
  const zoneL = h("button", { class: "jh-zone left", title: left, onclick: () => { flick(-1); onLeft?.(); } });
  const zoneR = h("button", { class: "jh-zone right", title: right, onclick: () => { flick(1); onRight?.(); } });
  let pos = 0;
  function flick(dir) {
    handle.setAttribute("transform", `rotate(${dir * 45} 50 50)`);
    if (center) setTimeout(() => handle.setAttribute("transform", `rotate(${pos * 30} 50 50)`), 450);
  }
  const el = h("div", { class: "jhandle" },
    plate(label, "small"),
    h("div", { class: "jh-lamps" }, g.el, r.el),
    h("div", { class: "jh-body" }, svg, zoneL, zoneR));
  return {
    el,
    /** state: true = right position (closed/running), false = left. */
    update(state, disabled = false) {
      g.update(!state);
      r.update(state);
      pos = state ? 1 : -1;
      handle.setAttribute("transform", `rotate(${(center ? 0 : pos) * 30} 50 50)`);
      el.classList.toggle("disabled", disabled);
    },
  };
}

/** Rotary selector knob with labelled detent positions. */
export function selector(label, positions, onChange) {
  const knob = h("div", { class: "knob" }, h("div", { class: "knob-ptr" }));
  const n = positions.length;
  const sweep = n <= 3 ? 50 : n <= 5 ? 80 : 110;
  const angleOf = i => (n === 1 ? 0 : -sweep + 2 * sweep * i / (n - 1));
  const labels = positions.map((p, i) => h("button", { class: "sel-pos", style: `--a:${angleOf(i)}deg`, onclick: () => onChange?.(p.value ?? p) }, p.label ?? p));
  const el = h("div", { class: "selector" }, plate(label, "small"), h("div", { class: "sel-body" }, ...labels, knob));
  return {
    el,
    update(value, disabled = false) {
      const i = positions.findIndex(p => (p.value ?? p) === value);
      knob.style.transform = `rotate(${angleOf(Math.max(0, i))}deg)`;
      labels.forEach((l, j) => l.classList.toggle("on", j === i));
      el.classList.toggle("disabled", disabled);
    },
  };
}

/** Round pushbutton, optionally illuminated. */
export function pushbutton(text, { color = "white", onPress, small = false } = {}) {
  const b = h("button", { class: `pb ${color} ${small ? "small" : ""}`, onclick: () => onPress?.() });
  const el = h("div", { class: "pb-wrap" }, b, h("div", { class: "pb-label" }, text));
  return { el, update(lit, disabled = false) { b.classList.toggle("lit", !!lit); el.classList.toggle("disabled", disabled); } };
}

/**
 * Spring-return switch: hold to act. `onHold(dir)` fires when pressed
 * (dir = -1 / +1), `onRelease()` when let go. Used for RAISE/LOWER and the
 * rod IN-HOLD-OUT lever.
 */
export function springSwitch(label, { lo = "LOWER", hi = "RAISE", onHold, onRelease, repeat = 0 } = {}) {
  let timer = null;
  const stop = () => { if (timer) { clearInterval(timer); timer = null; onRelease?.(); lever.className = "ss-lever"; } };
  const start = dir => {
    stop();
    lever.className = `ss-lever ${dir < 0 ? "lo" : "hi"}`;
    onHold?.(dir);
    timer = setInterval(() => repeat ? onHold?.(dir) : null, repeat || 1000);
  };
  const mk = (dir, txt) => h("button", {
    class: `ss-btn ${dir < 0 ? "lo" : "hi"}`,
    onpointerdown: e => { e.preventDefault(); e.target.setPointerCapture?.(e.pointerId); start(dir); },
    onpointerup: stop, onpointercancel: stop, onlostpointercapture: stop,
  }, txt);
  const lever = h("div", { class: "ss-lever" });
  const el = h("div", { class: "spring" }, plate(label, "small"), h("div", { class: "ss-body" }, mk(-1, lo), lever, mk(1, hi)));
  window.addEventListener("blur", stop);
  return { el, update(disabled) { el.classList.toggle("disabled", !!disabled); } };
}

// ------------------------------------------------------- synchroscope

/**
 * Synchroscope: analog needle with SLOW/FAST and two sync lamps, or (digital
 * room) a ring of LEDs with a green SYNC LED at 12 o'clock.
 */
export function synchroscope() {
  const W = 180, c = 90, r = 68;
  const svg = s("svg", { viewBox: `0 0 ${W} ${W}`, class: "sync-analog" });
  svg.append(s("circle", { cx: c, cy: c, r: 84, class: "sy-bezel" }), s("circle", { cx: c, cy: c, r: 78, class: "sy-face" }));
  svg.append(s("path", { d: `M${c - 14},${c - r} A${r},${r} 0 0 1 ${c + 14},${c - r}`, class: "sy-window" }));
  for (let i = 0; i < 36; i++) {
    const a = i * 10 * Math.PI / 180;
    svg.append(s("line", { x1: c + Math.sin(a) * 74, y1: c - Math.cos(a) * 74, x2: c + Math.sin(a) * (i % 9 === 0 ? 64 : 69), y2: c - Math.cos(a) * (i % 9 === 0 ? 64 : 69), class: "sy-tick" }));
  }
  const tS = s("text", { x: 46, y: 62, class: "sy-txt" }); tS.textContent = "SLOW";
  const tF = s("text", { x: 134, y: 62, class: "sy-txt" }); tF.textContent = "FAST";
  const tN = s("text", { x: c, y: c + 40, class: "sy-name" }); tN.textContent = "SYNCHROSCOPE";
  svg.append(tS, tF, tN);
  const needle = s("g", {}, s("line", { x1: c, y1: c + 18, x2: c, y2: c - 66, class: "sy-needle" }), s("path", { d: `M${c - 5},${c - 54} L${c},${c - 68} L${c + 5},${c - 54}Z`, class: "sy-needle-tip" }));
  svg.append(needle, s("circle", { cx: c, cy: c, r: 7, class: "sy-hub" }));

  const ring = h("div", { class: "sync-ring" });
  const leds = [];
  for (let i = 0; i < 32; i++) {
    const d = h("span", { class: `led ${i === 0 ? "sync" : ""}`, style: `--a:${i * 360 / 32}deg` });
    leds.push(d);
    ring.append(d);
  }
  ring.append(h("div", { class: "ring-txt slow" }, "TOO SLOW"), h("div", { class: "ring-txt fast" }, "TOO FAST"), h("div", { class: "ring-txt sync" }, "SYNC"));

  const l1 = lamp("white"), l2 = lamp("white");
  const el = h("div", { class: "synchroscope" },
    h("div", { class: "only-analog sync-wrap" }, svg),
    h("div", { class: "only-digital" }, ring),
    h("div", { class: "sync-lamps" }, l1.el, h("span", { class: "lamp-label" }, "SYNC LAMPS"), l2.el));
  return {
    el,
    /** phase in degrees (-180..180), live = incoming generator energised. */
    update(phase, live) {
      const p = live ? phase : 0;
      needle.setAttribute("transform", `rotate(${p} ${c} ${c})`);
      needle.style.opacity = live ? 1 : 0.25;
      const idx = ((Math.round(p / (360 / 32)) % 32) + 32) % 32;
      leds.forEach((d, i) => d.classList.toggle("on", live && i === idx));
      // Sync lamps are bright when out of phase, dark in phase.
      const bright = live ? Math.abs(Math.sin(p * Math.PI / 360)) : 0;
      for (const l of [l1, l2]) l.el.style.setProperty("--glow", bright.toFixed(2));
      l1.update(bright > 0.15);
      l2.update(bright > 0.15);
    },
  };
}

// --------------------------------------------------------- annunciator

/**
 * Lightbox annunciator: grid of engraved windows lettered A.. by column and
 * numbered 1.. by row. `windows` = [[tag, text], ...] row-major.
 */
export function annunciatorPanel(title, windows, cols = 4) {
  const grid = h("div", { class: "ann-grid", style: `--cols:${cols}` });
  const cells = new Map();
  grid.append(h("div", { class: "ann-corner" }));
  for (let c = 0; c < cols; c++) grid.append(h("div", { class: "ann-head" }, String.fromCharCode(65 + c)));
  windows.forEach(([tag, text], i) => {
    if (i % cols === 0) grid.append(h("div", { class: "ann-head row" }, String(i / cols + 1)));
    const cell = h("div", { class: "ann-win", "data-tag": tag || "" }, text || "");
    if (tag) cells.set(tag, cell);
    grid.append(cell);
  });
  const el = h("div", { class: "annunciator" }, grid, h("div", { class: "ann-title" }, title));
  return { el, cells };
}

// ------------------------------------------------------------ recorder

/** Strip-chart recorder: pens = [{tag, label, color, min, max}]. */
export function recorder(title, pens, { width = 360, height = 180, span = 600 } = {}) {
  const cv = h("canvas", { width, height, class: "recorder-paper" });
  const legend = h("div", { class: "rec-legend" }, ...pens.map(p => h("span", { style: `--c:${p.color}` }, p.label)));
  const el = h("div", { class: "recorder" }, h("div", { class: "rec-window" }, cv), legend, plate(title, "small"));
  const data = [];
  return {
    el,
    push(t, readings) {
      if (data.length && t <= data[data.length - 1].t) return;
      data.push({ t, v: pens.map(p => readings[p.tag]) });
      while (data.length > 2 && t - data[0].t > span) data.shift();
    },
    draw() {
      const ctx = cv.getContext("2d");
      const W = cv.width, H = cv.height;
      const room = cv.closest("[data-gen]")?.dataset.gen;
      ctx.fillStyle = room === "digital" ? "#0b1410" : "#f4f0e2";
      ctx.fillRect(0, 0, W, H);
      ctx.strokeStyle = room === "digital" ? "#1f3a2c" : "#c9b98f";
      ctx.lineWidth = 1;
      for (let i = 1; i < 10; i++) { ctx.beginPath(); ctx.moveTo(W * i / 10, 0); ctx.lineTo(W * i / 10, H); ctx.stroke(); }
      for (let i = 1; i < 5; i++) { ctx.beginPath(); ctx.moveTo(0, H * i / 5); ctx.lineTo(W, H * i / 5); ctx.stroke(); }
      if (data.length < 2) return;
      const t1 = data[data.length - 1].t, t0 = t1 - span;
      pens.forEach((p, k) => {
        ctx.strokeStyle = p.color; ctx.lineWidth = 1.6; ctx.beginPath();
        data.forEach((d, i) => {
          const x = (d.t - t0) / span * W;
          let v = d.v[k];
          if (v === undefined) return;
          const n = p.log ? (Math.log10(Math.max(v, p.min)) - Math.log10(p.min)) / (Math.log10(p.max) - Math.log10(p.min)) : (v - p.min) / (p.max - p.min);
          const y = H - Math.max(0, Math.min(1, n)) * H;
          i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
        });
        ctx.stroke();
      });
    },
  };
}

// ---------------------------------------------------- M/A control station

/**
 * Manual/Auto controller station for an automation module: process value,
 * setpoint, output; M and A pushbuttons; raise/lower that moves the output
 * in MANUAL and the setpoint in AUTO.
 */
export function maStation(title, cfg, ctx) {
  const pv = h("span", { class: "seg-val" }, "0");
  const sp = h("span", { class: "seg-val" }, "0");
  const outBar = h("div", { class: "ma-out-fill" });
  const outNum = h("span", {}, "0");
  const devPtr = h("div", { class: "ma-dev-ptr" });
  const status = h("div", { class: "ma-status" }, "");
  const mBtn = h("button", { class: "ma-btn m", onclick: () => ctx.setMode(cfg.module, "manual") }, "M");
  const aBtn = h("button", { class: "ma-btn a", onclick: () => ctx.setMode(cfg.module, "auto") }, "A");
  const pick = (v, t) => (typeof v === "function" ? v(t) : v);
  const nudge = dir => {
    const t = ctx.t();
    if (!t) return;
    const mode = ctx.moduleMode(cfg.module);
    const key = pick(cfg.spKey, t);
    if (mode === "auto" && key) {
      const cur = ctx.setpoint(cfg.module, key) ?? 0;
      ctx.send({ type: "set_setpoint", module: cfg.module, key, value: +(cur + dir * cfg.spStep).toFixed(4) });
    } else if (mode === "manual" && cfg.outCmd) {
      const cur = t.readings[cfg.outTag] ?? 0;
      ctx.send(cfg.outCmd(Math.max(cfg.outMin ?? 0, Math.min(cfg.outMax ?? 100, cur + dir * cfg.outStep))));
    }
  };
  const rl = springSwitch("", { lo: "▼", hi: "▲", onHold: nudge, repeat: 250 });
  const el = h("div", { class: "ma-station" },
    h("div", { class: "ma-title" }, title),
    h("div", { class: "ma-rows" },
      cfg.pvTag ? h("div", { class: "ma-row" }, h("label", {}, "PV"), h("div", { class: "seg mini" }, pv)) : null,
      cfg.spKey || cfg.spTag ? h("div", { class: "ma-row" }, h("label", {}, "SP"), h("div", { class: "seg mini sp" }, sp)) : null,
      cfg.pvTag && (cfg.spKey || cfg.spTag) ? h("div", { class: "ma-dev" }, devPtr) : null,
      cfg.outTag ? h("div", { class: "ma-row" }, h("label", {}, "OUT"), h("div", { class: "ma-out" }, outBar), outNum) : null),
    h("div", { class: "ma-ctl" }, mBtn, aBtn, rl.el),
    status);
  return {
    el,
    update(t) {
      const mode = ctx.moduleMode(cfg.module);
      const m = t.modules.find(x => x.id === cfg.module);
      const pvV = t.readings[pick(cfg.pvTag, t)];
      const key = pick(cfg.spKey, t);
      const spV = cfg.spTag ? t.readings[cfg.spTag] : key ? m?.setpoints[key] : undefined;
      pv.textContent = fmt(pvV, cfg.digits ?? 1);
      sp.textContent = spV === undefined ? "----" : fmt(spV, cfg.digits ?? 1);
      if (spV !== undefined && pvV !== undefined) {
        const dev = Math.max(-1, Math.min(1, (pvV - spV) / (cfg.devSpan ?? 1)));
        devPtr.style.left = `${50 + dev * 48}%`;
      }
      const o = t.readings[cfg.outTag];
      if (o !== undefined) {
        outBar.style.width = `${Math.max(0, Math.min(100, (o - (cfg.outMin ?? 0)) / ((cfg.outMax ?? 100) - (cfg.outMin ?? 0)) * 100))}%`;
        outNum.textContent = o.toFixed(cfg.outDigits ?? 0);
      }
      mBtn.classList.toggle("lit", mode === "manual");
      aBtn.classList.toggle("lit", mode === "auto");
      el.classList.toggle("ext", mode === "external");
      const missing = (m?.requires_upgrades ?? []).filter(u => !t.upgrades.find(x => x.id === u)?.installed);
      const unavailable = (m && !m.owned) || missing.length > 0;
      el.classList.toggle("not-owned", unavailable);
      aBtn.disabled = unavailable;
      status.textContent = mode === "external" ? "EXTERNAL (plugin)"
        : m && !m.owned ? "MODULE NOT INSTALLED"
        : missing.length ? "NO COMPUTER LINK" : "";
      status.title = missing.map(u => t.upgrades.find(x => x.id === u)?.name).join(", ");
    },
  };
}
