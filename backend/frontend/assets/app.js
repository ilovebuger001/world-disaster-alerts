// World Disaster Alerts — map-first SPA (spec §11, §64, §65, Phase 3).
// Startup order (spec §64.10): shell → cached snapshot → map async → stream.
import { api, latest, ApiError } from "./api.js";
import { prefs, saveSnapshot, loadSnapshot } from "./store.js";
import { T, LANG, setLang, langPref, fmtTime, ago } from "./i18n.js";
import { createMap, PRC, glyph } from "./map.js";

const $ = (id) => document.getElementById(id);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const HZ = ["earthquake", "flood", "cyclone", "storm", "wildfire", "volcano", "tsunami", "heat", "landslide"];
const STS = ["active", "upcoming", "expired", "cancelled"];
const RANGES = { all: 0, h6: 6 * 3600e3, h24: 24 * 3600e3, d7: 7 * 24 * 3600e3 };
const WORLD_VIEW = { west: -180, south: -85, east: 180, north: 85, zoom: 2 };

// ---- state (spec §65: small and explicit) ----
const S = {
  app: "initializing", // initializing | online | partial | stale | offline
  meta: null,
  sources: [],
  map: null,
  view: null,
  snapshot: null, // last viewport response
  items: [],
  nextCursor: null,
  selected: null, // { id, revision, record }
  openedByApp: false,
  filters: { types: new Set(), statuses: new Set(), range: "all", get sinceMs() { return RANGES[this.range] ? Date.now() - RANGES[this.range] : 0; } },
  style: prefs.get("mapStyle", "map"),
  me: null, // device-local only, never persisted or uploaded
  lastSync: null,
  fromCache: false,
  seq: 0,
  es: null
};
const fetchViewport = latest();
const fetchSearch = latest();

// ---------------- status ----------------
function renderStatus() {
  const el = $("status");
  let cls, text;
  if (!navigator.onLine || S.app === "offline") {
    cls = "offline";
    text = T.offline + (S.lastSync ? " · " + T.lastSync + " " + fmtTime(S.lastSync) : "");
  } else {
    const o = (S.meta && S.meta.freshness && S.meta.freshness.overall) || "unknown";
    cls = o === "live" ? "live" : o === "partial" ? "partial" : o === "delayed" ? "delayed" : o === "stale" ? "stale" : "";
    text = (T.fresh[o] || o) + (S.lastSync ? " · " + T.upd + " " + ago(S.lastSync) : "");
  }
  el.className = "status " + cls;
  $("statusText").textContent = text;
}

function toast(t) {
  const el = $("toast");
  el.textContent = t;
  el.classList.add("on");
  clearTimeout(toast.t);
  toast.t = setTimeout(() => el.classList.remove("on"), 3200);
}

// ---------------- data ----------------
function currentView() {
  return (S.map && S.map.getViewport()) || S.view || WORLD_VIEW;
}

async function refresh({ append = false } = {}) {
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
    if (!(e instanceof ApiError)) {
      S.app = "offline";
      await useCache();
    } else {
      toast(e.message);
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
  try {
    S.meta = await api.meta();
    const s = await api.sources();
    S.sources = s.sources || [];
  } catch {}
  renderStatus();
}

// ---------------- stream (spec §10.2) ----------------
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
    if (d.event_sequence <= S.seq) return; // duplicate
    S.seq = d.event_sequence;
    scheduleRefresh();
    if (S.selected && d.alert_id === S.selected.id && d.revision !== S.selected.revision) loadDetail(d.alert_id);
    if (d.display_priority === "emergency" || d.display_priority === "warning") toast(T.newEvent);
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
    refresh(); // full snapshot, then reconnect from its sequence
  });
  es.onopen = () => {
    if (S.app === "offline") {
      S.app = "online";
      refresh();
    }
  };
  es.onerror = () => {
    if (!navigator.onLine) {
      S.app = "offline";
      renderStatus();
    }
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
  // Keep the selected alert's marker visible even when it is inside a cluster.
  if (sel && !features.some((f) => f.alert_id === sel.alert_id)) {
    const p = (sel.geometry_parts || []).map(repPoint).find(Boolean);
    if (p) features.push({ type: "alert", alert_id: sel.alert_id, lat: p.lat, lon: p.lon, hazard_type: sel.hazard_type, display_priority: sel.display_priority });
  }
  S.map.setFeatures(features, S.selected && S.selected.id, labelFor);
}

function repPoint(g) {
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

async function initMap() {
  const ml = $("maploading");
  try {
    S.map = await createMap($("map"), {
      lang: LANG,
      style: S.style === "satellite" ? "satellite" : "map",
      mobile: matchMedia("(max-width:760px)").matches,
      onIdle: () => refresh(),
      onFeature: (f) => {
        if (f.type === "cluster") S.map.fitBBox(f.bbox);
        else openAlert(f.alert_id, true);
      }
    });
    ml.remove();
    applyStyleBtn();
  } catch (e) {
    ml.textContent = e.message === "nokey" ? T.noKey : T.mapFail;
    S.map = null;
    refresh();
    openList(true);
  }
}
document.addEventListener("wda:maps-auth-failure", () => {
  $("map").innerHTML = "";
  S.map = null;
  toast(T.mapFail);
  openList(true);
});

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
  $("detailInner").innerHTML = `<div class="mt">…</div>`;
  await loadDetail(id, true);
}

async function loadDetail(id, focus) {
  try {
    const d = await api.detail(id);
    if (!S.selected || S.selected.id !== id) return;
    S.selected.revision = d.alert.revision;
    S.selected.record = d.alert;
    renderDetail(d);
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
      `<div class="dhead"><h1>${esc(cached ? cached.headline || "" : T.notFound)}</h1><button class="x" id="dclose" aria-label="${T.close}">×</button></div>` +
      (cached ? `<p class="note">${esc(T.cached)} · ${esc(fmtTime(S.lastSync))}</p>${summaryKv(cached)}` : `<p class="note">${esc(e.message || T.notFound)}</p>`);
    $("dclose").onclick = () => closeAlert();
  }
  renderNoMarker();
}

function summaryKv(a) {
  return `<dl class="kv"><dt>${T.agency}</dt><dd>${esc(a.source_agency)}</dd><dt>${T.status}</dt><dd>${esc(T.st[a.status] || a.status)}</dd><dt>${T.published}</dt><dd>${esc(fmtTime(a.source_published_at_ms))}</dd></dl>${a.source_url ? `<a class="srcbtn" href="${esc(a.source_url)}" target="_blank" rel="noopener noreferrer">${T.openSrc} ↗</a>` : ""}`;
}

function renderDetail(d) {
  const a = d.alert;
  const c = PRC[a.display_priority] || PRC.unknown;
  const v0 = a.language_variants[0] || {};
  const title = v0.headline || v0.event || T.hz[a.hazard_type] || a.hazard_type;
  const nat = Object.entries(a.native || {}).filter(([k]) => !k.startsWith("cap_") && k !== "usgs_ids" && k !== "detail_url" && k !== "origin_time_ms");
  const cap = a.severity_raw ? `<dt>Severity</dt><dd>${esc(a.severity_raw)}</dd><dt>Urgency</dt><dd>${esc(a.urgency_raw)}</dd><dt>Certainty</dt><dd>${esc(a.certainty_raw)}</dd>` : "";
  const hasGeom = (a.geometry_parts || []).some((g) => g.type === "polygon" || g.type === "circle");
  const variants = a.language_variants
    .map((v) => {
      const parts = [v.headline, v.description, v.instruction].filter(Boolean).join("\n\n");
      return `<div class="mt">${esc(v.sender_name || a.source_agency)} · ${esc(v.language)}</div><div class="quote" lang="${esc(v.language)}">${esc(parts || v.event || "")}</div>`;
    })
    .join("");
  const areas = [...new Set(a.language_variants.flatMap((v) => v.area_desc || []))];
  $("detailInner").innerHTML = `
  <div class="dhead"><div><span class="badge" style="background:${c}">${glyph(a.hazard_type, 14)}${esc(T.pr[a.display_priority] || a.display_priority)}</span><span class="pill2">${esc(T.st[a.status] || a.status)}</span>
  <h1>${esc(title)}</h1><div class="sub">${esc(areas.join(" · "))}</div></div>
  <button class="x" id="dclose" aria-label="${T.close}">×</button></div>
  ${a.hazard_type === "earthquake" ? `<p class="note">${esc(T.eqNote)}</p>` : ""}
  ${d.source && !d.source.public_redistribution ? `<p class="note">${esc(T.notPublic)}</p>` : ""}
  <dl class="kv">
    <dt>${T.agency}</dt><dd>${esc(a.source_agency)}</dd>
    <dt>${T.hazard}</dt><dd>${esc(T.hz[a.hazard_type] || a.hazard_type)}<div class="sub">${esc(a.hazard_raw)}</div></dd>
    ${nat.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("")}
    ${cap}
    <dt>${T.display}</dt><dd>${esc(T.pr[a.display_priority] || a.display_priority)} <span class="sub">(${esc(a.severity_mapping_version)})</span></dd>
    <dt>${T.published}</dt><dd>${esc(fmtTime(a.source_published_at_ms))}</dd>
    ${a.source_updated_at_ms ? `<dt>${T.updatedSrc}</dt><dd>${esc(fmtTime(a.source_updated_at_ms))}</dd>` : ""}
    <dt>${T.fetched}</dt><dd>${esc(fmtTime(a.fetched_at_ms))}</dd>
    <dt>${T.expires}</dt><dd>${a.expires_at_ms ? esc(fmtTime(a.expires_at_ms)) : esc(T.expiresUnknown)}</dd>
    <dt>${T.freshness}</dt><dd><span class="health ${esc(d.source_freshness)}">${esc(T.fresh[d.source_freshness] || d.source_freshness)}</span></dd>
    <dt>${T.revision}</dt><dd>${a.revision}</dd>
  </dl>
  <div class="sec"><h2>${T.original}</h2>${variants}</div>
  <div class="sec"><h2>${T.translation}</h2><div class="mt">${T.trUnavailable}</div></div>
  <div class="sec"><div class="mt">${hasGeom ? T.geomOfficial : T.geomNone}</div>
  ${a.native && a.native.cap_identifier ? `<div class="mt">${T.sigNote}</div>` : ""}
  <a class="srcbtn" href="${esc(a.source_url)}" target="_blank" rel="noopener noreferrer">${T.openSrc} ↗</a>
  <div class="mt" style="margin-top:6px;word-break:break-all">${esc(a.source_url)}</div></div>`;
  $("dclose").onclick = () => closeAlert();
  $("detailInner").scrollTop = 0;
}

function closeAlert(fromHistory) {
  S.selected = null;
  $("app").classList.remove("has-detail", "sheet-full");
  if (S.map) S.map.clearGeometry();
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
function dist(a, b) {
  const R = 6371, r = Math.PI / 180;
  const dl = (b.lat - a.lat) * r, dn = (b.lon - a.lon) * r;
  const h = Math.sin(dl / 2) ** 2 + Math.cos(a.lat * r) * Math.cos(b.lat * r) * Math.sin(dn / 2) ** 2;
  return 2 * R * Math.asin(Math.min(1, Math.sqrt(h)));
}

function renderList() {
  const p = $("listPanel");
  let items = S.items.slice();
  if (S.me) {
    items = items.map((i) => ({ ...i, _d: i.point ? dist(S.me, i.point) : Infinity })).sort((a, b) => a._d - b._d);
  }
  p.innerHTML =
    `<h2>${S.me ? T.nearMe : T.listTitle}${S.snapshot && !S.fromCache ? ` (${S.snapshot.total_matching})` : ""}<button class="x" id="lclose" aria-label="${T.close}">×</button></h2>` +
    (S.fromCache ? `<p class="note">${esc(T.cached)} · ${esc(T.lastSync)} ${esc(fmtTime(S.lastSync))}</p>` : "") +
    (items.length
      ? items
          .map(
            (a) =>
              `<button class="item" data-id="${esc(a.alert_id)}"><span class="ico" style="background:${PRC[a.display_priority] || PRC.unknown}">${glyph(a.hazard_type)}</span><span><span class="t">${esc(a.headline || T.hz[a.hazard_type] || a.hazard_type)}</span><br><span class="m">${esc(T.pr[a.display_priority] || "")} · ${esc(T.st[a.status] || a.status)} · ${esc(a.source_agency)}</span><br><span class="m">${esc(ago(a.source_updated_at_ms || a.source_published_at_ms))}${a._d != null && isFinite(a._d) ? " · " + Math.round(a._d) + " " + T.km : ""} · ${esc(T.fresh[a.source_freshness] || "")}</span></span></button>`
          )
          .join("")
      : `<p>${esc(T.listEmpty)}</p>`) +
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
    ["filters", "help", "settings"].forEach(closeFloat);
    renderList();
  }
}
$("listBtn").onclick = () => openList(!$("listPanel").classList.contains("open"));

// ---------------- floats ----------------
function closeFloat(id) {
  $(id).classList.remove("open");
  if (id === "help") $("helpBtn").setAttribute("aria-pressed", "false");
}
function closeFloats() {
  ["filters", "help", "settings"].forEach(closeFloat);
  openList(false);
}
function toggleFloat(id, render) {
  const o = !$(id).classList.contains("open");
  if (o) {
    closeFloats();
    render();
  }
  $(id).classList.toggle("open", o);
  if (id === "help") $("helpBtn").setAttribute("aria-pressed", o);
}

function renderFilters() {
  const f = S.filters;
  const n = f.types.size + f.statuses.size + (f.range !== "all" ? 1 : 0);
  $("filters").innerHTML = `<h2>${T.filters}<button class="x" id="fclose" aria-label="${T.close}">×</button></h2>
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

function renderHelp() {
  $("help").innerHTML = `<h2>${T.helpTitle}<button class="x" id="hclose" aria-label="${T.close}">×</button></h2>
  <h3>${T.hSafe}</h3><p>${T.hSafeT}</p>
  <h3>${T.hMarkers}</h3><p>${T.hMarkersT}</p>
  <div class="fchips" style="margin:6px 0">${["emergency", "warning", "watch", "advisory", "information"].map((p) => `<span style="display:inline-flex;align-items:center;gap:5px;font-size:.8rem"><i style="width:10px;height:10px;border-radius:50%;background:${PRC[p]};display:inline-block"></i>${T.pr[p]}</span>`).join("")}</div>
  <ul style="list-style:none;padding:0;columns:2">${HZ.map((h) => `<li style="display:flex;align-items:center;gap:8px">${glyph(h, 16)} ${T.hz[h]}</li>`).join("")}</ul>
  <h3>${T.hFresh}</h3><p>${T.hFreshT}</p>
  <h3>${T.hSources}</h3>${S.sources
    .filter((s) => s.enabled)
    .map((s) => `<div class="srcrow"><span>${esc(s.agency_name)}<br><span class="sub">${T.lastOk}: ${s.health.last_http_success_at_ms ? esc(ago(s.health.last_http_success_at_ms)) : "—"}</span></span><span class="health ${esc(s.health.freshness_state)}">${esc(T.fresh[s.health.freshness_state] || s.health.freshness_state)}</span></div>`)
    .join("")}
  <h3>${T.hCov}</h3><p>${T.hCovT}</p>`;
  $("hclose").onclick = () => closeFloat("help");
}
$("helpBtn").onclick = () => toggleFloat("help", () => (loadMeta().then(() => $("help").classList.contains("open") && renderHelp()), renderHelp()));

function renderSettings() {
  const th = prefs.get("theme", "auto");
  const lp = langPref();
  $("settings").innerHTML = `<h2>${T.sTitle}<button class="x" id="sclose" aria-label="${T.close}">×</button></h2>
  <h3>${T.sLang}</h3><div class="seg">${[["auto", T.auto], ["th", "ไทย"], ["en", "English"]].map(([k, n]) => `<button data-lg="${k}" aria-pressed="${lp === k}">${n}</button>`).join("")}</div>
  <h3>${T.sTheme}</h3><div class="seg">${["auto", "light", "dark"].map((k) => `<button data-th="${k}" aria-pressed="${th === k}">${T[k]}</button>`).join("")}</div>`;
  $("sclose").onclick = () => closeFloat("settings");
  $("settings").querySelectorAll("[data-th]").forEach((b) => (b.onclick = () => (prefs.set("theme", b.dataset.th), applyTheme(), renderSettings())));
  $("settings").querySelectorAll("[data-lg]").forEach((b) => (b.onclick = () => (prefs.set("lang", b.dataset.lg), setLang(), applyI18n(), renderSettings(), renderListIfOpen())));
}
$("langBtn").onclick = () => toggleFloat("settings", renderSettings);
function applyTheme() {
  const t = prefs.get("theme", "auto");
  const d = t === "dark" || (t === "auto" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = d ? "dark" : "light";
}
matchMedia("(prefers-color-scheme: dark)").addEventListener?.("change", () => prefs.get("theme", "auto") === "auto" && applyTheme());
window.addEventListener("languagechange", () => {
  if (langPref() === "auto") {
    setLang();
    applyI18n();
  }
});

// ---------------- search (areas + disasters only) ----------------
const q = $("q"), sg = $("sugg");
let opts = [], act = -1, searchTimer = null;
q.addEventListener("input", () => {
  $("qclear").hidden = !q.value;
  clearTimeout(searchTimer);
  searchTimer = setTimeout(doSearch, 250); // debounce
});
async function doSearch() {
  const v = q.value.trim();
  if (!v) return closeSugg();
  let body;
  try {
    body = await fetchSearch(api.searchPath(v.slice(0, 64)));
  } catch (e) {
    if (e.name === "AbortError") return;
    body = { areas: [], disasters: S.items.filter((i) => (i.headline || "").toLowerCase().includes(v.toLowerCase())).slice(0, 5) };
  }
  opts = [...body.areas.map((p) => ({ k: "a", p })), ...body.disasters.map((d) => ({ k: "d", d }))];
  act = opts.length ? 0 : -1;
  let i = 0, h = "";
  if (body.areas.length) h += `<h4>${T.areas}</h4>` + body.areas.map((p) => `<div role="option" id="o${i}" data-i="${i++}"><svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 21s-7-6.5-7-12a7 7 0 0 1 14 0c0 5.5-7 12-7 12z" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="12" cy="9" r="2.5" fill="currentColor"/></svg><span>${esc(LANG === "th" ? p.name_th : p.name_en)}<br><span class="sub">${esc(LANG === "th" ? p.name_en : p.name_th)}</span></span></div>`).join("");
  if (body.disasters.length) h += `<h4>${T.disasters}</h4>` + body.disasters.map((d) => `<div role="option" id="o${i}" data-i="${i++}"><span class="ico" style="background:${PRC[d.display_priority] || PRC.unknown};width:28px;height:28px">${glyph(d.hazard_type, 15)}</span><span>${esc(d.headline || T.hz[d.hazard_type])}<br><span class="sub">${esc(T.pr[d.display_priority] || "")} · ${esc(d.source_agency || "")}</span></span></div>`).join("");
  if (!opts.length) h = `<div class="none">${T.noResult}</div>`;
  sg.innerHTML = h;
  sg.classList.add("open");
  $("searchbox").setAttribute("aria-expanded", "true");
  sg.querySelectorAll("[role=option]").forEach((o) => (o.onmousedown = (e) => (e.preventDefault(), pick(+o.dataset.i))));
  hl();
}
function hl() {
  sg.querySelectorAll("[role=option]").forEach((o) => o.setAttribute("aria-selected", +o.dataset.i === act));
  if (act >= 0) q.setAttribute("aria-activedescendant", "o" + act);
}
function closeSugg() {
  sg.classList.remove("open");
  $("searchbox").setAttribute("aria-expanded", "false");
  q.removeAttribute("aria-activedescendant");
}
function pick(i) {
  const o = opts[i];
  if (!o) return;
  closeSugg();
  q.blur();
  if (o.k === "a") {
    q.value = LANG === "th" ? o.p.name_th : o.p.name_en;
    if (S.selected) closeAlert();
    if (S.map) S.map.panTo(o.p.lat, o.p.lon, o.p.zoom);
  } else {
    q.value = o.d.headline || "";
    if (S.map && o.d.point) S.map.ensureVisible(o.d.point.lat, o.d.point.lon, 6);
    openAlert(o.d.alert_id, true);
  }
}
q.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") (act = Math.min(opts.length - 1, act + 1)), hl(), e.preventDefault();
  else if (e.key === "ArrowUp") (act = Math.max(0, act - 1)), hl(), e.preventDefault();
  else if (e.key === "Enter") act >= 0 && pick(act), e.preventDefault();
  else if (e.key === "Escape") closeSugg();
});
q.addEventListener("blur", () => setTimeout(closeSugg, 120));
$("qclear").onclick = () => ((q.value = ""), ($("qclear").hidden = true), closeSugg(), q.focus());

// ---------------- controls ----------------
function applyStyleBtn() {
  const sat = S.style === "satellite";
  $("styleBtn").setAttribute("aria-pressed", sat);
  const t = sat ? T.styleMap : T.styleSat;
  $("styleBtn").setAttribute("aria-label", t);
  $("styleBtn").title = t;
}
$("styleBtn").onclick = () => {
  S.style = S.style === "satellite" ? "map" : "satellite";
  prefs.set("mapStyle", S.style);
  if (S.map) S.map.setStyle(S.style);
  applyStyleBtn();
};
$("zin").onclick = () => S.map && S.map.zoomBy(1);
$("zout").onclick = () => S.map && S.map.zoomBy(-1);
$("worldBtn").onclick = () => S.map && S.map.world();
$("locBtn").onclick = () => {
  // Explicit tap only; computed locally, never uploaded or persisted (spec §11.5).
  if (!navigator.geolocation) return toast(T.locFail);
  toast(T.locWhy);
  navigator.geolocation.getCurrentPosition(
    (p) => {
      S.me = { lat: p.coords.latitude, lon: p.coords.longitude };
      if (S.map) {
        S.map.setMe(S.me.lat, S.me.lon, p.coords.accuracy);
        S.map.panTo(S.me.lat, S.me.lon, 9);
      }
      openList(true);
    },
    () => toast(T.locFail),
    { enableHighAccuracy: false, timeout: 10000, maximumAge: 60000 }
  );
};
window.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || document.activeElement === q) return;
  const open = ["settings", "filters", "help"].find((id) => $(id).classList.contains("open"));
  if (open) closeFloat(open);
  else if (S.selected) closeAlert();
  else if ($("listPanel").classList.contains("open")) openList(false);
});
window.addEventListener("online", () => ((S.app = "online"), refresh(), loadMeta()));
window.addEventListener("offline", () => ((S.app = "offline"), renderStatus()));
setInterval(renderStatus, 15000);

function applyI18n() {
  q.placeholder = T.searchPh;
  $("langCode").textContent = LANG.toUpperCase();
  const al = (id, t) => ($(id).setAttribute("aria-label", t), ($(id).title = t));
  al("listBtn", T.list);
  al("worldBtn", T.world);
  al("locBtn", T.locate);
  al("helpBtn", T.help);
  al("filterBtn", T.filters);
  applyStyleBtn();
  renderStatus();
  if (S.selected && S.selected.record) loadDetail(S.selected.id);
}

// ---------------- startup ----------------
setLang();
applyI18n();
$("maploading").textContent = "…";
(async () => {
  const cached = await loadSnapshot();
  if (cached) {
    S.lastSync = cached.saved_at_ms;
    S.items = cached.items || [];
    S.fromCache = true;
  }
  renderStatus();
  loadMeta();
  initMap(); // async; first idle triggers the viewport fetch
  const h = decodeURIComponent(location.hash.slice(1));
  if (/^[0-9a-f]{8,64}$/.test(h)) openAlert(h, false);
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("/service-worker.js").catch(() => {});
})();
