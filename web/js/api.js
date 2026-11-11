// Client for the NeuReactor /api/v1 endpoints (same API plugins use).

import { play } from "./sound.js";

const TOKEN = window.NR_TOKEN;

export async function api(path, body) {
  const opts = { headers: { Authorization: `Bearer ${TOKEN}` } };
  if (body !== undefined) {
    opts.method = "POST";
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(`/api/v1${path}`, opts);
  return res.json();
}

let toastEl = null;
export function toast(msg) {
  toastEl ??= document.getElementById("toast");
  if (!toastEl) return;
  toastEl.textContent = msg;
  toastEl.classList.toggle("show", !!msg);
  if (msg) setTimeout(() => { if (toastEl.textContent === msg) { toastEl.textContent = ""; toastEl.classList.remove("show"); } }, 5000);
}

export async function send(cmd) {
  try {
    const r = await api("/command", cmd);
    if (!r.ok) { toast(r.error); play("deny"); }
    return r;
  } catch (e) {
    toast("Lost connection to the NeuReactor server");
    return { ok: false };
  }
}
