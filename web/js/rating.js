// Unit rating, read once at start-up (the page rebuilds itself when the unit
// size changes). Panels use it to scale meters for the installed unit size.

export const rating = {
  size: "mwe1150",
  gross: 1150,          // rated gross output [MWe]
  thermal: 3411,        // rated core thermal power [MWt]
  scale: 1,             // thermal power relative to the 1150 MWe reference
};

export function setRating(t) {
  if (!t) return;
  rating.size = t.unit_size || rating.size;
  rating.gross = t.rated_gross_mwe || rating.gross;
  rating.thermal = t.rated_thermal_mw || rating.thermal;
  rating.scale = rating.thermal / 3411;
}

/** Round a meter span up to a tidy number (1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8 × 10^n). */
export function nice(x) {
  const e = 10 ** Math.floor(Math.log10(x));
  const m = x / e;
  const step = [1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].find(s => s >= m - 1e-9);
  return step * e;
}

/** Full-scale for a quantity that is `ref` on the 1150 MWe reference unit. */
export const scaled = ref => nice(ref * rating.scale);

/** Full-scale for electrical output meters [MWe]. */
export const mweScale = () => nice(rating.gross * 1.12);
