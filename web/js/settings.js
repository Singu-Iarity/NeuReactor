// Per-browser display settings.

const KEY = "nr-settings";
const defaults = {
  /** "us": red lamp = running / closed (US plant convention).
   *  "green_on": green lamp = running / closed. */
  lamps: "us",
  /** Shake the control room with turbine vibration. */
  shake: true,
};

let state = { ...defaults };
try { Object.assign(state, JSON.parse(localStorage.getItem(KEY) || "{}")); } catch (_) { /* ignore */ }

function apply() {
  document.body.classList.toggle("lamps-green-on", state.lamps === "green_on");
}

export const settings = {
  get(k) { return state[k]; },
  set(k, v) {
    state[k] = v;
    try { localStorage.setItem(KEY, JSON.stringify(state)); } catch (_) { /* ignore */ }
    apply();
  },
};

apply();
