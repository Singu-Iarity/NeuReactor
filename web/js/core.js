// Top-down core map. The layout comes from the server and depends on the
// unit size (121 to 241 fuel assemblies, 29 to 69 rod cluster control
// assemblies). Each RCCA belongs to a bank and to group 1 (quadrants 1 & 3)
// or group 2 (quadrants 2 & 4).

import { h, tween } from "./widgets.js";
import { api } from "./api.js";

/**
 * Builds the core map. `onPick(rcca)` fires when a rod is clicked.
 * Analog room: a field of small selsyn dials; digital: coloured cells.
 */
export function coreMap(onPick) {
  const grid = h("div", { class: "core-grid" });
  const cells = [];
  const all = [];   // every fuel assembly, row-major (matches the server's CET order)
  const tLabel = el => { const l = h("div", { class: "cet-num" }); el.append(l); all.push({ el, l }); return el; };
  const title = h("div", { class: "core-legend" }, "");
  // The layout (assemblies, RCCA banks and groups) comes from the server
  // and depends on the unit size.
  api("/core_layout").then(layout => {
    const N = layout.grid;
    grid.style.setProperty("--n", N);
    title.textContent = `${layout.assemblies.length} FUEL ASSEMBLIES · ${layout.rccas} RCCAs`;
    const at = new Map(layout.assemblies.map(a => [`${a.x},${a.y}`, a]));
    for (let y = 0; y < N; y++) {
      for (let x = 0; x < N; x++) {
        const a = at.get(`${x},${y}`);
        if (!a) { grid.append(h("div", { class: "core-cell empty" })); continue; }
        if (!a.rcca) { grid.append(tLabel(h("div", { class: "core-cell fuel" }))); continue; }
        const rod = { x, y, bank: a.rcca[0], group: a.rcca[1], id: `${String.fromCharCode(65 + y)}${x + 1}` };
        const ptr = h("div", { class: "selsyn-ptr" });
        const num = h("div", { class: "rod-num" }, "0");
        const cell = h("button", { class: `core-cell rod bank-${rod.bank}`, title: `${rod.id}: bank ${rod.bank}, group ${rod.group}`, onclick: () => onPick?.(rod) },
          h("div", { class: "selsyn only-analog" }, ptr), num, h("div", { class: "rod-tag" }, `${rod.bank}${rod.group}`));
        const set = tween(v => {
          ptr.style.transform = `rotate(${-135 + 270 * v / 228}deg)`;
          cell.style.setProperty("--ins", `${(1 - v / 228) * 100}%`);
        }, 0.2);
        cells.push({ rod, cell, num, set });
        grid.append(tLabel(cell));
      }
    }
  });
  const legend = h("div", { class: "core-legend" }, "SELSYN ROD POSITION · steps withdrawn (0 = fully inserted, 228 = out)");
  const el = h("div", { class: "coremap" }, title, grid, legend);
  let tempMode = false;
  return {
    el,
    /** Show core exit temperatures (array, row-major) instead of rod positions; null = rods. */
    showTemps(temps) {
      const on = !!temps?.length;
      if (on !== tempMode) {
        tempMode = on;
        grid.classList.toggle("temps", on);
        legend.textContent = on ? "CORE EXIT THERMOCOUPLES · °C per fuel assembly (blue = cooler, red = hotter than core average)"
          : "SELSYN ROD POSITION · steps withdrawn (0 = fully inserted, 228 = out)";
        if (!on) all.forEach(a => { a.el.style.removeProperty("--cet"); a.l.textContent = ""; });
      }
      if (!on) return;
      const avg = temps.reduce((a, b) => a + b, 0) / temps.length;
      all.forEach((a, i) => {
        const t = temps[i];
        if (t === undefined) return;
        const d = Math.max(-1, Math.min(1, (t - avg) / 10));
        const hue = 220 - (d + 1) * 110;   // blue (cool) → green → red (hot)
        a.el.style.setProperty("--cet", `hsl(${hue} 75% ${38 + 12 * Math.abs(d)}%)`);
        a.l.textContent = t.toFixed(0);
        a.el.title = `${t.toFixed(1)} °C`;
      });
    },
    update(rods, selected) {
      const byBank = Object.fromEntries(rods.map(r => [r.bank, r]));
      for (const c of cells) {
        const b = byBank[c.rod.bank];
        if (!b) continue;
        const pos = c.rod.group === 2 ? b.group2 : b.position;
        c.set(pos);
        c.num.textContent = Math.round(pos);
        const items = selected === "manual" ? null : selected.split(",");
        const sel = items === null ? !["SA", "SB"].includes(c.rod.bank)
          : items.includes(c.rod.bank) || items.includes(`${c.rod.bank}${c.rod.group}`);
        c.cell.classList.toggle("selected", sel);
        c.cell.classList.toggle("dev", Math.abs(b.group2 - b.position) > 12);
      }
    },
  };
}
