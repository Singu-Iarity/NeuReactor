// Top-down core map: a 15×15 four-loop PWR core (193 fuel assemblies) with
// 53 rod cluster control assemblies (RCCAs). Each RCCA belongs to a bank and
// to group 1 (quadrants 1 & 3) or group 2 (quadrants 2 & 4).

import { h, tween } from "./widgets.js";

const N = 15, C = 7;
// Assemblies per row (core outline).
const ROW_WIDTH = [7, 11, 13, 13, 15, 15, 15, 15, 15, 15, 15, 13, 13, 11, 7];

/** Orbits of symmetric RCCA locations (dx, dy from centre) → bank. */
const ORBITS = [
  [[0, 0], "D"], [[2, 2], "D"],
  [[2, 0], "C"], [[4, 4], "C"],
  [[4, 0], "B"], [[6, 0], "B"],
  [[4, 2], "A"],
  [[6, 2], "SA"], [[3, 1], "SA"],
  [[6, 4], "SB"],
];

function orbit([dx, dy]) {
  const pts = new Set();
  for (const [a, b] of [[dx, dy], [dy, dx]]) {
    for (const sx of [1, -1]) for (const sy of [1, -1]) pts.add(`${a * sx},${b * sy}`);
  }
  return [...pts].map(p => p.split(",").map(Number));
}

export const RCCAS = [];
for (const [d, bank] of ORBITS) {
  for (const [dx, dy] of orbit(d)) {
    // Group 1: quadrants 1 and 3 (and the +y axis / centre); group 2: 2 and 4.
    const group = dx === 0 ? 1 : dy === 0 ? 2 : dx * dy > 0 ? 1 : 2;
    RCCAS.push({ x: C + dx, y: C - dy, bank, group, id: `${String.fromCharCode(72 + dy)}${C + dx + 1}` });
  }
}

function inCore(x, y) {
  const w = ROW_WIDTH[y];
  return Math.abs(x - C) <= (w - 1) / 2;
}

/**
 * Builds the core map. `onPick(rcca)` fires when a rod is clicked.
 * Analog room: a field of small selsyn dials; digital: coloured cells.
 */
export function coreMap(onPick) {
  const grid = h("div", { class: "core-grid", style: `--n:${N}` });
  const cells = [];
  for (let y = 0; y < N; y++) {
    for (let x = 0; x < N; x++) {
      if (!inCore(x, y)) { grid.append(h("div", { class: "core-cell empty" })); continue; }
      const rod = RCCAS.find(r => r.x === x && r.y === y);
      if (!rod) { grid.append(h("div", { class: "core-cell fuel" })); continue; }
      const ptr = h("div", { class: "selsyn-ptr" });
      const num = h("div", { class: "rod-num" }, "0");
      const cell = h("button", { class: `core-cell rod bank-${rod.bank}`, title: `${rod.id}: bank ${rod.bank}, group ${rod.group}`, onclick: () => onPick?.(rod) },
        h("div", { class: "selsyn only-analog" }, ptr), num, h("div", { class: "rod-tag" }, `${rod.bank}${rod.group}`));
      const set = tween(v => {
        ptr.style.transform = `rotate(${-135 + 270 * v / 228}deg)`;
        cell.style.setProperty("--ins", `${(1 - v / 228) * 100}%`);
      }, 0.2);
      cells.push({ rod, cell, num, set });
      grid.append(cell);
    }
  }
  const el = h("div", { class: "coremap" }, grid,
    h("div", { class: "core-legend" }, "SELSYN ROD POSITION · steps withdrawn (0 = fully inserted, 228 = out)"));
  return {
    el,
    update(rods, selected) {
      const byBank = Object.fromEntries(rods.map(r => [r.bank, r]));
      for (const c of cells) {
        const b = byBank[c.rod.bank];
        if (!b) continue;
        const pos = c.rod.group === 2 ? b.group2 : b.position;
        c.set(pos);
        c.num.textContent = Math.round(pos);
        const sel = selected === "manual" ? !["SA", "SB"].includes(c.rod.bank)
          : selected === c.rod.bank || selected === `${c.rod.bank}${c.rod.group}`;
        c.cell.classList.toggle("selected", sel);
        c.cell.classList.toggle("dev", Math.abs(b.group2 - b.position) > 12);
      }
    },
  };
}
