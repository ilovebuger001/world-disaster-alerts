// World Disaster Alerts — map-first client. All data comes from the Rust
// server; nothing is simulated. Startup: shell → cached snapshot → map → stream.
import { api, latest, ApiError, backendConfigured } from "./api.js";
import { prefs, saveSnapshot, loadSnapshot } from "./store.js";
import { T, LANG, setLang, langPref, LANGS, fmtTime, ago } from "./i18n.js";
import { PRC, glyph } from "./icons.js";

const $ = (id) => document.getElementById(id);
export const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
export const safeUrl = (u) => (/^https?:\/\//i.test(String(u || "")) ? String(u) : "");
const HZ = ["earthquake", "flood", "cyclone", "storm", "wildfire", "volcano", "tsunami", "heat", "landslide"];
const STS = ["active", "upcoming", "expired", "cancelled"];
const RANGES = { all: 0, h6: 6 * 3600e3, h24: 24 * 3600e3, d7: 7 * 24 * 3600e3 };
const WORLD_VIEW = { west: -180, south: -85, east: 180, north: 85, zoom: 2 };
const CLOSE_SVG = `<svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/></svg>`;
export const closeBtn = (id) => `<button class="x" id="${id}" type="button" aria-label="${esc(T.close)}" title="${esc(T.close)}">${CLOSE_SVG}</button>`;

const S = {
  app: "initializing", // initializing | online | partial | offline | unconfigured | unreachable
  meta: null,
  sources: [],
  map: null,
  view: null,
  snapshot: null,
  items: [],
  nextCursor: null,
  selected: null,
  openedByApp: false,
  filters: { types: new Set(), statuses: new Set(), range: "all", get sinceMs() { return RANGES[this.range] ? Date.now() - RANGES[this.range] : 0; } },
  me: null, // device-local; never persisted
  watchId: null,
  lastSync: null,
  fromCache: false,
  seq: 0,
  es: null
};
export const state = S;
const fetchViewport = latest();
const fetchSearch = latest();

// ---------------- status ----------------
function renderStatus() {
  const el = $("status");
  let cls = "", text;
  if (S.app === "unconfigured") {
    cls = "offline";
    text = T.backend;
  } else if (!navigator.onLine || S.app === "offline") {
    cls = "offline";
    text = T.offline + (S.lastSync ? " · " + T.lastSync + " " + fmtTime(S.lastSync) : "");
  } else if (S.app === "unreachable") {
    cls = "offline";
    text = T.serverDown + (S.lastSync ? " · " + T.lastSync + " " + fmtTime(S.lastSync) : "");
  } else if (S.app === "initializing") {
    text = T.loading;
  } else {
    const o = (S.meta && S.meta.freshness && S.meta.freshness.overall) || "unknown";
    cls = o === "live" ? "live" : o === "partial" ? "partial" : o === "delayed" ? "delayed" : o === "stale" ? "stale" : "";
    text = (T.fresh[o] || o) + (S.lastSync ? " · " + T.upd + " " + ago(S.lastSync) : "");
  }
  el.className = "status " + cls;
  $("statusText").textContent = text;
  $("retryBtn").hidden = !(S.app === "unreachable" || S.app === "offline");
}

export function toast(t) {
  const el = $("toast");
  el.textContent = t;
  el.classList.add("on");
  clearTimeout(toast.t);
  toast.t = setTimeout(() => el.classList.remove("on"), 4000);
}

// ---------------- data ----------------
function currentView() {
  return (S.map && S.map.getViewport()) || S.view || WORLD_VIEW;
}

async function refresh({ append = false } = {}) {
  if (!backendConfigured()) return;
  const v = currentView();
  S.view = v;
  try {
    const body = await fetchViewport(api.viewportPath(v, S.filters, append ? S.nextCursor : null));
    S.snapshot = body;
    S.items = append ? S.items.concat(body.items) : body.items;
    S.nextCursor = body.next_cursor;
    S.lastSync = Date.now();
    S.fromCache = false;
    S.app = body.freshness === "live" ? "online" : "partial";
    if (!append) {
      if (!S.es) connectStream(body.event_sequence);
      saveSnapshot({ items: S.items, event_sequence: body.event_sequence, freshness: body.freshness, view: v });
    }
    renderMap();
    renderListIfOpen();
    renderNoMarker();
  } catch (e) {
    if (e.name === "AbortError") return;
    if (e instanceof ApiError && e.status !== 502 && e.status !== 503 && e.status !== 504) toast(e.message);
    else {
      S.app = navigator.onLine ? "unreachable" : "offline";
      await useCache();
    }
  }
  renderStatus();
}

async function useCache() {
  const c = await loadSnapshot();
  if (!c) return;
  S.items = c.items || [];
  S.nextCursor = null;
  S.lastSync = c.saved_at_ms;
  S.fromCache = true;
  renderListIfOpen();
  if (!$("listPanel").classList.contains("open")) openList(true);
}

async function loadMeta() {
  if (!backendConfigured()) return;
  try {
    S.meta = await api.meta();
    const s = await api.sources();
    S.sources = s.sources || [];
    if (S.app === "unreachable" || S.app === "initializing") S.app = "partial";
  } catch (e) {
    if (!(e instanceof ApiError)) S.app = navigator.onLine ? "unreachable" : "offline";
  }
  renderStatus();
}

// ---------------- stream ----------------
function connectStream(after) {
  if (S.es) S.es.close();
  S.seq = after;
  if (!("EventSource" in window)) return;
  const es = new EventSource(api.streamUrl(after));
  S.es = es;
  const onAlert = (ev) => {
    let d;
    try {
      d = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (d.event_sequence <= S.seq) return;
    S.seq = d.event_sequence;
    scheduleRefresh();
    if (S.selected && d.alert_id === S.selected.id && d.revision !== S.selected.revision) loadDetail(d.alert_id);
    if (d.display_priority === "emergency" || d.display_priority === "warning") toast(T.newEvent);
    document.dispatchEvent(new CustomEvent("wda:alert-event", { detail: d }));
  };
  es.addEventListener("alert.upsert", onAlert);
  es.addEventListener("alert.remove", onAlert);
  es.addEventListener("source.status", (ev) => {
    try {
      const d = JSON.parse(ev.data);
      if (d.event_sequence > S.seq) S.seq = d.event_sequence;
    } catch {}
    loadMeta();
  });
  es.addEventListener("stream.reset", () => {
    es.close();
    S.es = null;
    refresh();
  });
  es.onopen = () => {
    if (S.app === "offline" || S.app === "unreachable") {
      S.app = "partial";
      refresh();
    }
  };
  es.onerror = () => {
    if (!navigator.onLine) S.app = "offline";
    renderStatus();
  };
}
let refreshTimer = null;
function scheduleRefresh() {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => refresh(), 600);
}

// ---------------- map ----------------
function labelFor(f) {
  if (f.type === "cluster") return `${f.count} · ${T.pr[f.top_priority] || ""}`;
  return `${T.hz[f.hazard_type] || f.hazard_type} · ${T.pr[f.display_priority] || ""}`;
}

function renderMap() {
  if (!S.map || !S.snapshot) return;
  const features = S.snapshot.features.slice();
  const sel = S.selected && S.selected.record;
  if (sel && !features.some((f) => f.alert_id === sel.alert_id)) {
    const p = (sel.geometry_parts || []).map(repPoint).find(Boolean);
    if (p) features.push({ type: "alert", alert_id: sel.alert_id, lat: p.lat, lon: p.lon, hazard_type: sel.hazard_type, display_priority: sel.display_priority });
  }
  S.map.setFeatures(features, S.selected && S.selected.id, labelFor);
}

export function repPoint(g) {
  if (g.type === "point") return { lat: g.point.lat_deg, lon: g.point.lon_deg };
  if (g.type === "circle") return { lat: g.center.lat_deg, lon: g.center.lon_deg };
  if (g.type === "polygon" && g.exterior.length) {
    const xs = g.exterior.map((p) => p.lon_deg), ys = g.exterior.map((p) => p.lat_deg);
    return { lat: (Math.min(...ys) + Math.max(...ys)) / 2, lon: (Math.min(...xs) + Math.max(...xs)) / 2 };
  }
  return null;
}

function renderNoMarker() {
  const n = $("nomark");
  if (!S.snapshot || S.selected) return (n.hidden = true);
  if (S.snapshot.total_matching === 0) {
    n.textContent = T.noMarker;
    n.hidden = false;
  } else if (S.snapshot.candidates_capped) {
    n.textContent = T.capped;
    n.hidden = false;
  } else n.hidden = true;
}

function theme() {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

async function initMap() {
  const ml = $("maploading");
  ml.textContent = T.loading;
  try {
    const { createMap } = await import("./map.js");
    S.map = await createMap($("map"), {
      lang: LANG,
      theme: theme(),
      mobile: matchMedia("(max-width:760px)").matches,
      onIdle: () => refresh(),
      onFeature: (f) => {
        if (f.type === "cluster") S.map.fitBBox(f.bbox);
        else openAlert(f.alert_id, true);
      },
      onFacility: (f) => toast(`${f.name} · ${T.facSt[f.status] || f.status}`)
    });
    ml.remove();
    refresh();
  } catch {
    ml.textContent = T.mapFail;
    S.map = null;
    refresh();
    openList(true);
  }
}

// ---------------- detail ----------------
async function openAlert(id, push) {
  const wasOpen = !!S.selected;
  S.selected = { id, revision: null, record: null };
  $("app").classList.add("has-detail");
  $("app").classList.remove("sheet-full");
  closeFloats();
  if (push && location.hash !== "#" + id) {
    if (wasOpen && S.openedByApp) history.replaceState(null, "", "#" + id);
    else {
      S.openedByApp = true;
      history.pushState(null, "", "#" + id);
    }
  }
  if (!push) S.openedByApp = false;
  $("detailInner").innerHTML = `<div class="dhead"><div><h1>${esc(T.loading)}</h1></div>${closeBtn("dclose")}</div>`;
  $("dclose").onclick = () => closeAlert();
  await loadDetail(id, true);
}

async function loadDetail(id, focus) {
  try {
    const [d, D] = await Promise.all([api.detail(id), import("./detail.js")]);
    if (!S.selected || S.selected.id !== id) return;
    S.selected.revision = d.alert.revision;
    S.selected.record = d.alert;
    D.renderDetail(d);
    D.loadFacilities(d.alert);
    D.loadGuideInto(d.alert.hazard_type);
    if (S.map) {
      S.map.showGeometry(d.alert.geometry_parts, PRC[d.alert.display_priority] || PRC.unknown);
      const p = (d.alert.geometry_parts || []).map(repPoint).find(Boolean);
      if (focus && p) S.map.ensureVisible(p.lat, p.lon, 6);
      renderMap();
    }
  } catch (e) {
    if (!S.selected || S.selected.id !== id) return;
    const cached = S.items.find((i) => i.alert_id === id);
    $("detailInner").innerHTML =
      `<div class="dhead"><div><h1>${esc(cached ? cached.headline || "" : T.notFound)}</h1></div>${closeBtn("dclose")}</div>` +
      (cached ? `<p class="note">${esc(T.cached)} · ${esc(fmtTime(S.lastSync))}</p><dl class="kv"><dt>${T.agency}</dt><dd>${esc(cached.source_agency)}</dd><dt>${T.status}</dt><dd>${esc(T.st[cached.status] || cached.status)}</dd><dt>${T.issued}</dt><dd>${esc(fmtTime(cached.source_published_at_ms))}</dd></dl>` : `<p class="note">${esc(e.message || T.notFound)}</p>`);
    $("dclose").onclick = () => closeAlert();
  }
  renderNoMarker();
}

export function closeAlert(fromHistory) {
  S.selected = null;
  $("app").classList.remove("has-detail", "sheet-full");
  if (S.map) {
    S.map.clearGeometry();
    S.map.setFacilities([], () => "");
  }
  renderMap();
  renderNoMarker();
  if (!fromHistory && location.hash) {
    if (S.openedByApp) {
      S.openedByApp = false;
      history.back();
    } else history.replaceState(null, "", location.pathname + location.search);
  }
}
$("sheetToggle").onclick = () => $("app").classList.toggle("sheet-full");
window.addEventListener("popstate", () => {
  const h = decodeURIComponent(location.hash.slice(1));
  if (h && /^[0-9a-f]{8,64}$/.test(h)) {
    if (!S.selected || S.selected.id !== h) openAlert(h, false);
  } else if (S.selected) {
    S.openedByApp = false;
    closeAlert(true);
  }
});

// ---------------- list ----------------
export function dist(a, b) {
  const R = 6371, r = Math.PI / 180;
  const dl = (b.lat - a.lat) * r, dn = (b.lon - a.lon) * r;
  const h = Math.sin(dl / 2) ** 2 + Math.cos(a.lat * r) * Math.cos(b.lat * r) * Math.sin(dn / 2) ** 2;
  return 2 * R * Math.asin(Math.min(1, Math.sqrt(h)));
}

function renderList() {
  const p = $("listPanel");
  let items = S.items.slice();
  if (S.me) items = items.map((i) => ({ ...i, _d: i.point ? dist(S.me, i.point) : Infinity })).sort((a, b) => a._d - b._d);
  p.innerHTML =
    `<div class="phead"><h2>${S.me ? T.nearMe : T.listTitle}${S.snapshot && !S.fromCache ? ` (${S.snapshot.total_matching})` : ""}</h2>${closeBtn("lclose")}</div>` +
    (S.fromCache ? `<p class="note">${esc(T.cached)} · ${esc(T.lastSync)} ${esc(fmtTime(S.lastSync))}</p>` : "") +
    (S.app === "unconfigured" ? `<p class="note warn">${esc(T.backend)}</p>` : "") +
    (items.length
      ? items
          .map(
            (a) =>
              `<button class="item" data-id="${esc(a.alert_id)}"><span class="ico" style="background:${PRC[a.display_priority] || PRC.unknown}">${glyph(a.hazard_type)}</span><span><span class="t">${esc(a.headline || T.hz[a.hazard_type] || a.hazard_type)}</span><br><span class="m">${esc(T.pr[a.display_priority] || "")} · ${esc(T.st[a.status] || a.status)} · ${esc(a.source_agency)}</span><br><span class="m">${esc(ago(a.source_updated_at_ms || a.source_published_at_ms))}${a._d != null && isFinite(a._d) ? " · " + Math.round(a._d) + " " + T.km : ""} · ${esc(T.fresh[a.source_freshness] || "")}</span></span></button>`
          )
          .join("")
      : S.app === "unconfigured" ? "" : `<p>${esc(T.listEmpty)}</p>`) +
    (S.nextCursor && !S.fromCache ? `<button class="more" id="more">${T.more}</button>` : "");
  $("lclose").onclick = () => openList(false);
  p.querySelectorAll(".item").forEach((b) => (b.onclick = () => openAlert(b.dataset.id, true)));
  const more = $("more");
  if (more) more.onclick = () => refresh({ append: true });
}
function renderListIfOpen() {
  if ($("listPanel").classList.contains("open")) renderList();
}
function openList(on) {
  $("listPanel").classList.toggle("open", on);
  $("listBtn").setAttribute("aria-pressed", on);
  if (on) {
    ["filters", "settings", "locMenu"].forEach(closeFloat);
    renderList();
  }
}
$("listBtn").onclick = () => openList(!$("listPanel").classList.contains("open"));

// ---------------- floats ----------------
function closeFloat(id) {
  $(id).classList.remove("open");
}
function closeFloats() {
  ["filters", "settings", "locMenu"].forEach(closeFloat);
  openList(false);
}
function toggleFloat(id, render) {
  const o = !$(id).classList.contains("open");
  if (o) {
    closeFloats();
    render();
  }
  $(id).classList.toggle("open", o);
  if (o) $(id).querySelector("button")?.focus();
}

function renderFilters() {
  const f = S.filters;
  const n = f.types.size + f.statuses.size + (f.range !== "all" ? 1 : 0);
  $("filters").innerHTML = `<div class="phead"><h2>${T.filters}</h2>${closeBtn("fclose")}</div>
  <h3>${T.fHazard}</h3><div class="fchips">${HZ.map((h) => `<button class="fchip" data-hz="${h}" aria-pressed="${f.types.has(h)}">${esc(T.hz[h])}</button>`).join("")}</div>
  <h3>${T.fStatus}</h3><div class="fchips">${STS.map((s) => `<button class="fchip" data-st="${s}" aria-pressed="${f.statuses.has(s)}">${esc(T.st[s])}</button>`).join("")}</div>
  <h3>${T.fTime}</h3><div class="seg">${[["all", T.tAll], ["h6", T.t6], ["h24", T.t24], ["d7", T.t7]].map(([k, l]) => `<button data-tr="${k}" aria-pressed="${f.range === k}">${l}</button>`).join("")}</div>
  ${n ? `<button class="clear" id="fclear">${T.clear}</button>` : ""}`;
  $("fclose").onclick = () => closeFloat("filters");
  const toggle = (set, v) => (set.has(v) ? set.delete(v) : set.add(v));
  $("filters").querySelectorAll("[data-hz]").forEach((b) => (b.onclick = () => (toggle(f.types, b.dataset.hz), afterFilter())));
  $("filters").querySelectorAll("[data-st]").forEach((b) => (b.onclick = () => (toggle(f.statuses, b.dataset.st), afterFilter())));
  $("filters").querySelectorAll("[data-tr]").forEach((b) => (b.onclick = () => ((f.range = b.dataset.tr), afterFilter())));
  if (n) $("fclear").onclick = () => (f.types.clear(), f.statuses.clear(), (f.range = "all"), afterFilter());
  $("filterBtn").classList.toggle("on", n > 0);
}
function afterFilter() {
  renderFilters();
  refresh();
}
$("filterBtn").onclick = () => toggleFloat("filters", renderFilters);

// ---------------- full-screen help ----------------
async function openHelp(tab, country) {
  const { openHelpScreen } = await import("./help.js");
  openHelpScreen({ tab, country, sources: S.sources, me: S.me, reloadMeta: loadMeta });
}
$("helpBtn").onclick = () => openHelp("emergency");

// ---------------- settings ----------------
const LANG_NAMES = { th: "ไทย", en: "English" };
async function renderSettings() {
  const th = prefs.get("theme", "auto");
  const lp = langPref();
  const notify = await import("./notify.js");
  $("settings").innerHTML = `<div class="phead"><h2>${T.sTitle}</h2>${closeBtn("sclose")}</div>
  <h3>${T.sLang}</h3><div class="seg">${[["auto", T.auto], ...LANGS.map((l) => [l, LANG_NAMES[l] || l])].map(([k, n]) => `<button data-lg="${k}" aria-pressed="${lp === k}">${n}</button>`).join("")}</div>
  <h3>${T.sTheme}</h3><div class="seg">${["auto", "light", "dark"].map((k) => `<button data-th="${k}" aria-pressed="${th === k}">${T[k]}</button>`).join("")}</div>
  <h3>${T.sNotify}</h3>${notify.settingsHtml()}
  <h3>${T.sAbout}</h3><p class="mt">${esc(T.mapData)}</p>`;
  $("sclose").onclick = () => closeFloat("settings");
  $("settings").querySelectorAll("[data-th]").forEach((b) => (b.onclick = () => (prefs.set("theme", b.dataset.th), applyTheme(), renderSettings())));
  $("settings").querySelectorAll("[data-lg]").forEach((b) => (b.onclick = () => (prefs.set("lang", b.dataset.lg), setLang().then(() => (applyI18n(), renderSettings(), renderListIfOpen())))));
  notify.bind($("settings"), { getMe: locateOnce, rerender: renderSettings });
}
$("langBtn").onclick = () => toggleFloat("settings", renderSettings);
function applyTheme() {
  const t = prefs.get("theme", "auto");
  const d = t === "dark" || (t === "auto" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = d ? "dark" : "light";
  if (S.map) S.map.setTheme(theme());
}
matchMedia("(prefers-color-scheme: dark)").addEventListener?.("change", () => prefs.get("theme", "auto") === "auto" && applyTheme());
window.addEventListener("languagechange", () => {
  if (langPref() === "auto") setLang().then(applyI18n);
});

// ---------------- search ----------------
const q = $("q"), sg = $("sugg");
let opts = [], act = -1, searchTimer = null;
q.addEventListener("input", () => {
  $("qclear").hidden = !q.value;
  clearTimeout(searchTimer);
  searchTimer = setTimeout(doSearch, 250);
});
const PIN_SVG = `<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 21s-7-6.5-7-12a7 7 0 0 1 14 0c0 5.5-7 12-7 12z" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="12" cy="9" r="2.5" fill="currentColor"/></svg>`;
function parseCoords(s) {
  const m = s.trim().match(/^(-?\d+(?:\.\d+)?)\s*[, ]\s*(-?\d+(?:\.\d+)?)$/);
  if (!m) return null;
  const lat = +m[1], lon = +m[2];
  return Math.abs(lat) <= 90 && Math.abs(lon) <= 180 ? { lat, lon } : null;
}
async function doSearch() {
  const v = q.value.trim();
  if (!v) return closeSugg();
  let body;
  try {
    body = await fetchSearch(api.searchPath(v.slice(0, 64)));
  } catch (e) {
    if (e.name === "AbortError") return;
    const c = parseCoords(v);
    body = { areas: [], countries: [], coordinates: c, disasters: S.items.filter((i) => (i.headline || "").toLowerCase().includes(v.toLowerCase())).slice(0, 5) };
  }
  renderSugg(body, v);
}
function renderSugg(body, v) {
  opts = [];
  let h = "";
  const opt = (o, inner) => {
    const i = opts.push(o) - 1;
    return `<div role="option" id="o${i}" data-i="${i}">${inner}</div>`;
  };
  if (body.coordinates) h += `<h4>${T.coords}</h4>` + opt({ k: "c", c: body.coordinates }, `${PIN_SVG}<span>${body.coordinates.lat.toFixed(5)}, ${body.coordinates.lon.toFixed(5)}<br><span class="sub">${T.goTo}</span></span>`);
  if ((body.areas || []).length) h += `<h4>${T.areas}</h4>` + body.areas.map((p) => opt({ k: "a", p }, `${PIN_SVG}<span>${esc(LANG === "th" ? p.name_th : p.name_en)}<br><span class="sub">${esc(LANG === "th" ? p.name_en : p.name_th)}</span></span>`)).join("");
  if ((body.countries || []).length) h += `<h4>${T.countries}</h4>` + body.countries.map((c) => opt({ k: "n", c }, `<span class="flagcode">${esc(c.code)}</span><span>${esc(c.name)}<br><span class="sub">☎ ${esc(T.openDir)} · ${esc(T.cov[c.coverage] || c.coverage)}</span></span>`)).join("");
  if ((body.disasters || []).length) h += `<h4>${T.disasters}</h4>` + body.disasters.map((d) => opt({ k: "d", d }, `<span class="ico" style="background:${PRC[d.display_priority] || PRC.unknown};width:28px;height:28px">${glyph(d.hazard_type, 15)}</span><span>${esc(d.headline || T.hz[d.hazard_type])}<br><span class="sub">${esc(T.pr[d.display_priority] || "")} · ${esc(d.source_agency || "")}</span></span>`)).join("");
  if (v.length >= 3 && !body.coordinates && backendConfigured()) h += opt({ k: "g", q: v }, `<svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7" fill="none" stroke="currentColor" stroke-width="2"/><path d="M20 20l-4-4" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg><span>${esc(v)}<br><span class="sub">${T.addrHint}</span></span>`);
  if (!opts.length) h = `<div class="none">${T.noResult}</div>`;
  act = opts.length ? 0 : -1;
  sg.innerHTML = h;
  sg.classList.add("open");
  $("searchbox").setAttribute("aria-expanded", "true");
  sg.querySelectorAll("[role=option]").forEach((o) => (o.onmousedown = (e) => (e.preventDefault(), pick(+o.dataset.i))));
  hl();
}
async function geocode(v) {
  sg.innerHTML = `<div class="none">${esc(T.loading)}</div>`;
  sg.classList.add("open");
  let body;
  try {
    body = await api.geocode(v, LANG);
  } catch (e) {
    const msg = e.code === "geocoder_disabled" ? T.addrOff : e.code === "geocoder_busy" ? T.addrBusy : e.message || T.serverDown;
    sg.innerHTML = `<div class="none">${esc(msg)}</div>`;
    opts = [];
    return;
  }
  opts = [];
  const items = body.items || [];
  let h = items.length ? `<h4>${T.addresses}</h4>` : `<div class="none">${esc(T.addrNone)}</div>`;
  for (const it of items) {
    const i = opts.push({ k: "r", r: it }) - 1;
    h += `<div role="option" id="o${i}" data-i="${i}">${PIN_SVG}<span>${esc(it.name)}</span></div>`;
  }
  if (body.attribution) h += `<div class="none sub">${esc(body.attribution)}</div>`;
  act = opts.length ? 0 : -1;
  sg.innerHTML = h;
  sg.querySelectorAll("[role=option]").forEach((o) => (o.onmousedown = (e) => (e.preventDefault(), pick(+o.dataset.i))));
  hl();
}
function hl() {
  sg.querySelectorAll("[role=option]").forEach((o) => o.setAttribute("aria-selected", +o.dataset.i === act));
  if (act >= 0) q.setAttribute("aria-activedescendant", "o" + act);
  else q.removeAttribute("aria-activedescendant");
}
function closeSugg() {
  sg.classList.remove("open");
  $("searchbox").setAttribute("aria-expanded", "false");
  q.removeAttribute("aria-activedescendant");
}
function pick(i) {
  const o = opts[i];
  if (!o) return;
  if (o.k === "g") return geocode(o.q);
  closeSugg();
  q.blur();
  if (o.k === "a") {
    q.value = LANG === "th" ? o.p.name_th : o.p.name_en;
    if (S.selected) closeAlert();
    if (S.map) S.map.panTo(o.p.lat, o.p.lon, o.p.zoom);
  } else if (o.k === "c") {
    if (S.selected) closeAlert();
    if (S.map) S.map.panTo(o.c.lat, o.c.lon, 11);
  } else if (o.k === "r") {
    q.value = o.r.name;
    if (S.selected) closeAlert();
    if (S.map) o.r.bbox ? S.map.fitBounds(o.r.bbox.south, o.r.bbox.west, o.r.bbox.north, o.r.bbox.east) : S.map.panTo(o.r.lat, o.r.lon, 12);
  } else if (o.k === "n") {
    q.value = o.c.name;
    openHelp("emergency", o.c.code);
  } else {
    q.value = o.d.headline || "";
    if (S.map && o.d.point) S.map.ensureVisible(o.d.point.lat, o.d.point.lon, 6);
    openAlert(o.d.alert_id, true);
  }
}
q.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") (act = Math.min(opts.length - 1, act + 1)), hl(), e.preventDefault();
  else if (e.key === "ArrowUp") (act = Math.max(0, act - 1)), hl(), e.preventDefault();
  else if (e.key === "Enter") {
    e.preventDefault();
    if (act >= 0) pick(act);
    else if (q.value.trim().length >= 3) geocode(q.value.trim());
  } else if (e.key === "Escape") closeSugg();
});
q.addEventListener("blur", () => setTimeout(closeSugg, 150));
$("qclear").onclick = () => ((q.value = ""), ($("qclear").hidden = true), closeSugg(), q.focus());

// ---------------- location (once / live) ----------------
function geoError(err) {
  if (!navigator.onLine) return toast(T.gpsOffline);
  toast(err && err.code === 1 ? T.gpsDenied : err && err.code === 3 ? T.gpsTimeout : T.gpsUnavail);
}
function applyPosition(p, pan) {
  S.me = { lat: p.coords.latitude, lon: p.coords.longitude, acc: p.coords.accuracy };
  if (S.map) {
    S.map.setMe(S.me.lat, S.me.lon, p.coords.accuracy);
    if (pan) S.map.panTo(S.me.lat, S.me.lon, 10);
  }
  renderListIfOpen();
}
export function locateOnce() {
  return new Promise((resolve) => {
    if (!navigator.geolocation) return toast(T.gpsUnsupported), resolve(null);
    navigator.geolocation.getCurrentPosition(
      (p) => (applyPosition(p, true), resolve(S.me)),
      (err) => (geoError(err), resolve(null)),
      { enableHighAccuracy: false, timeout: 15000, maximumAge: 60000 }
    );
  });
}
function startTracking() {
  if (!navigator.geolocation) return toast(T.gpsUnsupported);
  let first = true;
  S.watchId = navigator.geolocation.watchPosition(
    (p) => (applyPosition(p, first), (first = false)),
    (err) => {
      geoError(err);
      if (err.code === 1) stopTracking();
    },
    { enableHighAccuracy: true, timeout: 30000, maximumAge: 10000 }
  );
  updateLocBtn();
}
function stopTracking() {
  if (S.watchId != null) navigator.geolocation.clearWatch(S.watchId);
  S.watchId = null;
  updateLocBtn();
}
function updateLocBtn() {
  const on = S.watchId != null;
  $("locBtn").classList.toggle("tracking", on);
  $("locBtn").setAttribute("aria-pressed", on);
  $("trackBadge").hidden = !on;
}
function renderLocMenu() {
  const tracking = S.watchId != null;
  $("locMenu").innerHTML = `<div class="phead"><h2>${T.locate}</h2>${closeBtn("lmclose")}</div><p class="mt">${esc(T.locWhy)}</p>
  ${tracking ? `<button class="menuitem" id="gpsStop">${T.gpsStop}</button>` : `<button class="menuitem" id="gpsOnce">${T.gpsOnce}</button><button class="menuitem" id="gpsLive">${T.gpsLive}</button>`}
  ${S.me ? `<button class="menuitem" id="gpsClear">${T.clear}</button>` : ""}`;
  $("lmclose").onclick = () => closeFloat("locMenu");
  const b = (id, fn) => $(id) && ($(id).onclick = () => (closeFloat("locMenu"), fn()));
  b("gpsOnce", () => locateOnce().then((m) => m && openList(true)));
  b("gpsLive", startTracking);
  b("gpsStop", stopTracking);
  b("gpsClear", () => (stopTracking(), (S.me = null), S.map && S.map.clearMe(), renderListIfOpen()));
}
$("locBtn").onclick = () => toggleFloat("locMenu", renderLocMenu);
$("trackBadge").onclick = stopTracking;

// ---------------- controls ----------------
$("zin").onclick = () => S.map && S.map.zoomBy(1);
$("zout").onclick = () => S.map && S.map.zoomBy(-1);
$("worldBtn").onclick = () => S.map && S.map.world();
$("retryBtn").onclick = () => (loadMeta(), refresh());
window.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || document.activeElement === q || !$("help").hidden) return;
  const open = ["settings", "filters", "locMenu"].find((id) => $(id).classList.contains("open"));
  if (open) {
    closeFloat(open);
    const btn = { settings: "langBtn", filters: "filterBtn", locMenu: "locBtn" }[open];
    $(btn).focus();
  } else if (S.selected) closeAlert();
  else if ($("listPanel").classList.contains("open")) openList(false);
});
window.addEventListener("online", () => ((S.app = "partial"), refresh(), loadMeta()));
window.addEventListener("offline", () => ((S.app = "offline"), renderStatus()));
setInterval(renderStatus, 15000);

function applyI18n() {
  q.placeholder = T.searchPh;
  q.setAttribute("aria-label", T.searchPh);
  $("langCode").textContent = LANG.toUpperCase();
  const al = (id, t) => ($(id).setAttribute("aria-label", t), ($(id).title = t));
  al("listBtn", T.list);
  al("worldBtn", T.world);
  al("locBtn", T.locate);
  al("helpBtn", T.help);
  al("filterBtn", T.filters);
  al("langBtn", T.sTitle);
  al("qclear", T.clear);
  al("retryBtn", T.retry);
  al("trackBadge", T.gpsStop);
  $("retryBtn").textContent = T.retry;
  $("trackBadge").textContent = T.tracking + " ×";
  $("map").setAttribute("aria-label", T.mapData);
  if (S.map) S.map.setLang(LANG);
  renderStatus();
  if (S.selected && S.selected.record) loadDetail(S.selected.id);
}

// ---------------- startup ----------------
(async () => {
  await setLang();
  applyI18n();
  if (!backendConfigured()) {
    S.app = "unconfigured";
    renderStatus();
    initMap();
    if (!matchMedia("(max-width:760px)").matches) openList(true);
    return;
  }
  const cached = await loadSnapshot();
  if (cached) {
    S.lastSync = cached.saved_at_ms;
    S.items = cached.items || [];
    S.fromCache = true;
  }
  renderStatus();
  loadMeta();
  initMap();
  const h = decodeURIComponent(location.hash.slice(1));
  if (/^[0-9a-f]{8,64}$/.test(h)) openAlert(h, false);
  else if (h === "help") openHelp("emergency");
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("service-worker.js").catch(() => {});
  (await import("./notify.js")).resume({ getMe: () => S.me, locate: locateOnce });
})();
