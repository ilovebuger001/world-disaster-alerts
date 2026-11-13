// Nearby-alert notifications while the page is open. Eligibility (status,
// expiry, cancellation, priority, distance) is decided by the Rust server; this
// module asks only after explicit opt-in, de-duplicates by alert revision and
// rate-limits what it shows. It is not Cell Broadcast and never claims to be.
import { api } from "./api.js";
import { prefs } from "./store.js";
import { T } from "./i18n.js";
import { esc } from "./app.js";

const POLL_MS = 120000;
const MAX_PER_WINDOW = 3;
const WINDOW_MS = 10 * 60000;
const SEEN_MAX = 300;

let timer = null;
let deps = null;
let triedLocate = false;
let shown = []; // timestamps of shown notifications (rate limit)

const supported = () => "Notification" in window;
const enabled = () => prefs.get("notify", "off") === "on" && supported() && Notification.permission === "granted";

function seen() {
  try {
    return JSON.parse(prefs.get("notifySeen", "[]"));
  } catch {
    return [];
  }
}
function remember(keys) {
  const s = seen().concat(keys);
  prefs.set("notifySeen", JSON.stringify(s.slice(-SEEN_MAX)));
}

async function show(title, body, tag, url) {
  try {
    const reg = "serviceWorker" in navigator ? await navigator.serviceWorker.getRegistration() : null;
    if (reg) await reg.showNotification(title, { body, tag, data: { url } });
    else new Notification(title, { body, tag });
  } catch {}
}

export async function check() {
  if (!enabled() || !deps) return;
  let me = deps.getMe();
  if (!me && deps.locate && !triedLocate) {
    triedLocate = true; // permission was granted earlier; ask the device once
    me = await deps.locate();
  }
  if (!me) return;
  let body;
  try {
    body = await api.eligible(me.lat, me.lon);
  } catch {
    return;
  }
  const already = new Set(seen());
  const fresh = (body.items || []).filter((i) => !already.has(i.dedupe_key));
  if (!fresh.length) return;
  remember(fresh.map((i) => i.dedupe_key));
  const now = Date.now();
  shown = shown.filter((t) => now - t < WINDOW_MS);
  const room = Math.max(0, MAX_PER_WINDOW - shown.length);
  const now_show = fresh.slice(0, room);
  for (const i of now_show) {
    shown.push(now);
    const title = `${T.pr[i.display_priority] || ""} · ${T.hz[i.hazard_type] || i.hazard_type}`;
    await show(title, `${i.headline || ""}\n${i.source_agency} · ${i.distance_km} ${T.km}`, i.dedupe_key, "#" + i.alert_id);
  }
  const rest = fresh.length - now_show.length;
  if (rest > 0 && room > 0) await show(T.newEvent, `+${rest}`, "wda-more", "#");
}

function start() {
  stop();
  if (!enabled()) return;
  timer = setInterval(check, POLL_MS);
  check();
}
function stop() {
  clearInterval(timer);
  timer = null;
}

/** Called at startup; resumes polling only if the user previously opted in. */
export function resume(d) {
  deps = d;
  document.addEventListener("wda:alert-event", (e) => {
    const p = e.detail && e.detail.display_priority;
    if (p === "emergency" || p === "warning") setTimeout(check, 1500);
  });
  if (enabled()) start();
}

export function settingsHtml() {
  if (!supported()) return `<p class="mt">${esc(T.notifyUnsupported)}</p>`;
  const on = enabled();
  return `<p class="mt">${esc(T.notifyWhy)}</p><p class="mt">${esc(T.notifyLimits)}</p>
    <button class="menuitem" id="notifyToggle" aria-pressed="${on}">${on ? T.notifyOff : T.notifyOn}</button>
    ${Notification.permission === "denied" ? `<p class="note">${esc(T.notifyDenied)}</p>` : ""}`;
}

export function bind(root, { getMe, rerender }) {
  const b = root.querySelector("#notifyToggle");
  if (!b) return;
  b.onclick = async () => {
    if (enabled()) {
      prefs.set("notify", "off");
      stop();
      return rerender();
    }
    const perm = await Notification.requestPermission();
    if (perm !== "granted") return rerender();
    const me = (deps && deps.getMe()) || (await getMe());
    if (!me) {
      alertNoLoc(root);
      return;
    }
    prefs.set("notify", "on");
    start();
    rerender();
  };
}

function alertNoLoc(root) {
  const p = document.createElement("p");
  p.className = "note";
  p.textContent = T.notifyNeedsLoc;
  root.appendChild(p);
}
