// Full-screen Help: emergency numbers for every country/territory, official
// preparedness guides and map usage. Loaded on demand. Directory and guides
// are cached on the device with the date they were saved, for offline use.
import { api } from "./api.js";
import { kvGet, kvPut, prefs } from "./store.js";
import { T, LANG, fmtTime, ago } from "./i18n.js";
import { PRC, glyph } from "./icons.js";
import { esc, closeBtn, state } from "./app.js";

const $ = (id) => document.getElementById(id);
const safeUrl = (u) => (/^https:\/\//i.test(String(u || "")) ? String(u) : "");
const HZ_ORDER = ["earthquake", "tsunami", "flood", "cyclone", "storm", "tornado", "wildfire", "volcano", "landslide", "heat", "winter", "drought"];

let contacts = null; // { data, saved_at_ms, fromCache }
let guides = null;
let prevFocus = null;
let ctx = {};

async function loadCached(key, fetcher) {
  if (navigator.onLine !== false) {
    try {
      const data = await fetcher();
      kvPut(key, data);
      return { data, saved_at_ms: Date.now(), fromCache: false };
    } catch {}
  }
  const c = await kvGet(key);
  return c ? { data: c.value, saved_at_ms: c.saved_at_ms, fromCache: true } : null;
}
// Reuse the in-memory copy only while online; offline always shows the saved copy and its date.
const fresh = (x) => x && !x.fromCache && navigator.onLine !== false;
const getContacts = async () => (fresh(contacts) ? contacts : (contacts = await loadCached("contacts", api.contacts)));
const getGuides = async () => (fresh(guides) ? guides : (guides = await loadCached("guides", api.guides)));

/** Valid dial strings only: 2–15 digits with optional leading +. Never auto-dialled. */
export function telHref(dial) {
  return /^\+?[0-9]{2,15}$/.test(String(dial || "")) ? "tel:" + dial : null;
}

// ---------------------------------------------------------------- guides

function guideBody(g) {
  const list = (k) => (g[k] && g[k].length ? `<h4>${T[k]}</h4><ul>${g[k].map((x) => `<li>${esc(x)}</li>`).join("")}</ul>` : "");
  return `${LANG !== g.language && T.guideLang ? `<p class="mt">${esc(T.guideLang)}</p>` : ""}<div lang="${esc(g.language)}">${list("before")}${list("during")}${list("after")}</div>
  <p class="mt">${esc(g.authority)} · ${esc(T.guideScope)}: ${esc(g.scope)}${g.published_or_reviewed ? " · " + esc(g.published_or_reviewed) : ""} · ${esc(T.reviewed)} ${esc(g.retrieved)}
  · <a href="${esc(safeUrl(g.source_url))}" target="_blank" rel="noopener noreferrer">${T.source} ↗</a></p>`;
}

/** Guide for one hazard, for the alert detail panel (compact) or the Help screen. */
/** Best reviewed guide: the reader's language and country first, never a made-up translation. */
export function pickGuide(list, hazard, lang, country) {
  const all = (list || []).filter((x) => x.hazard === hazard);
  const l = (g) => g.language === lang, c = (g) => country && g.jurisdiction === country;
  return all.find((g) => l(g) && c(g)) || all.find(l) || all.find(c) || all[0] || null;
}

export async function guideHtml(hazard, compact, country) {
  const g = await getGuides();
  if (!g) return `<p class="mt">${esc(T.serverDown)}</p>`;
  const found = pickGuide(g.data.guides, hazard, LANG, country || prefs.get("country", ""));
  const stale = g.fromCache ? `<p class="note">${esc(T.dirOffline)} ${esc(fmtTime(g.saved_at_ms))}</p>` : "";
  if (!found) return `${stale}<p class="mt">${esc(T.guideUnavailable)}</p>`;
  if (compact) return `${stale}<details><summary>${esc(found.title)}</summary>${guideBody(found)}</details>`;
  return stale + guideBody(found);
}

async function renderGuides(body) {
  body.innerHTML = `<p class="mt">${esc(T.loading)}</p>`;
  const g = await getGuides();
  if (!g) return (body.innerHTML = `<p class="note warn">${esc(T.dirUnavailable)}</p>`);
  const d = g.data;
  const hazards = [...new Set((d.guides || []).map((x) => x.hazard))];
  const all = [...hazards.map((h) => ({ h, g: pickGuide(d.guides, h, LANG, prefs.get("country", "")) })), ...(d.unavailable || []).map((u) => ({ h: u.hazard, u }))];
  all.sort((a, b) => HZ_ORDER.indexOf(a.h) - HZ_ORDER.indexOf(b.h));
  body.innerHTML =
    (g.fromCache ? `<p class="note">${esc(T.dirOffline)} ${esc(fmtTime(g.saved_at_ms))}</p>` : "") +
    `<p class="note">${esc(d.notice)}</p>` +
    all
      .map(
        (x) =>
          `<details class="guide"><summary>${glyph(x.h, 18)} ${esc(x.g ? x.g.title : T.hz[x.h] || x.h)}</summary>${x.g ? guideBody(x.g) : `<p class="mt">${esc(T.guideUnavailable)} ${esc(x.u.reason)}</p>`}</details>`
      )
      .join("");
}

// ---------------------------------------------------------------- directory

export function countryCard(c) {
  const head = `<div class="ccard-h"><h3>${esc(c.name)} <span class="sub">${esc(c.code)}</span></h3><span class="cov cov-${esc(c.coverage)}">${esc(T.cov[c.coverage] || c.coverage)}</span></div>`;
  if (!c.contacts.length) return `<div class="ccard">${head}<p class="note warn">${esc(T.noNumbers)}</p></div>`;
  const rows = c.contacts
    .map((k) => {
      const href = telHref(k.dial);
      const meta = [k.region && `${T.region}: ${k.region}`, k.network && `${T.network}: ${k.network}`].filter(Boolean).join(" · ");
      return `<div class="crow">
        ${href ? `<a class="callbtn" href="${esc(href)}" aria-label="${esc(T.call)} ${esc(k.display)} (${esc(T.svc[k.service] || k.service)})">${esc(k.display)}</a>` : `<span class="callbtn off">${esc(k.display)}</span>`}
        <div><b>${esc(T.svc[k.service] || k.service)}</b>${meta ? `<br><span class="sub">${esc(meta)}</span>` : ""}
        ${k.instructions ? `<br><span>${esc(k.instructions)}</span>` : ""}${k.notes ? `<br><span class="sub">${esc(k.notes)}</span>` : ""}
        <br><span class="ver ver-${esc(k.verification)}">${esc(T.ver[k.verification] || k.verification)}</span>
        <span class="sub">· ${esc(k.authority)}${k.last_verified ? ` · ${esc(T.reviewed)} ${esc(k.last_verified)}` : ""} · <a href="${esc(safeUrl(k.source_url))}" target="_blank" rel="noopener noreferrer">${T.source} ↗</a></span></div></div>`;
    })
    .join("");
  return `<div class="ccard">${head}${rows}${c.notes ? `<p class="mt">${esc(c.notes)}</p>` : ""}<p class="mt">${esc(T.callNote)}</p></div>`;
}

async function renderDirectory(body, preselect) {
  body.innerHTML = `<p class="mt">${esc(T.loading)}</p>`;
  const c = await getContacts();
  if (!c) return (body.innerHTML = `<p class="note warn">${esc(T.dirUnavailable)}</p>`);
  const list = c.data.countries || [];
  const sel = preselect || prefs.get("country", "");
  body.innerHTML = `
    ${c.fromCache ? `<p class="note">${esc(T.dirOffline)} ${esc(fmtTime(c.saved_at_ms))}</p>` : `<p class="mt">${esc(T.offlineReady)} · ${esc(c.data.dataset_version)}</p>`}
    <label class="lbl" for="ctySearch">${T.pickCountry}</label>
    <div class="ctyrow"><input id="ctySearch" type="search" autocomplete="off" placeholder="${esc(T.countrySearch)}" list="ctyList">
    <button class="menuitem inline" id="ctyLoc">${T.useLocation}</button></div>
    <datalist id="ctyList">${list.map((x) => `<option value="${esc(x.name)} (${esc(x.code)})"></option>`).join("")}</datalist>
    <div id="ctySuggest"></div>
    <div id="ctyCard"></div>
    <details class="covsum"><summary>${esc(T.cov.verified)} / ${esc(T.cov.partially_verified)} / ${esc(T.cov.unavailable)}</summary>
      <p class="mt">${Object.entries(c.data.coverage || {}).map(([k, v]) => `${esc(T.cov[k] || k)}: ${v}`).join(" · ")}</p></details>`;
  const show = (code) => {
    const x = list.find((y) => y.code === code);
    $("ctyCard").innerHTML = x ? countryCard(x) : "";
    if (x) prefs.set("country", x.code);
  };
  const input = $("ctySearch");
  input.addEventListener("change", () => {
    const m = input.value.match(/\(([A-Z]{2})\)\s*$/);
    const v = input.value.trim().toLowerCase();
    const x = m ? list.find((y) => y.code === m[1]) : list.find((y) => y.name.toLowerCase() === v || y.code.toLowerCase() === v) || list.find((y) => y.name.toLowerCase().startsWith(v));
    if (x) show(x.code);
  });
  $("ctyLoc").onclick = async () => {
    const s = $("ctySuggest");
    s.innerHTML = `<p class="mt">${esc(T.loading)}</p>`;
    const me = state.me || (await new Promise((r) => navigator.geolocation ? navigator.geolocation.getCurrentPosition((p) => r({ lat: p.coords.latitude, lon: p.coords.longitude }), () => r(null), { timeout: 15000, maximumAge: 300000 }) : r(null)));
    if (!me) return (s.innerHTML = `<p class="note">${esc(T.gpsUnavail)}</p>`);
    try {
      const r = await api.reverse(me.lat, me.lon);
      const x = r.country_code && list.find((y) => y.code === r.country_code.toUpperCase());
      if (!x) throw new Error("none");
      s.innerHTML = `<p class="note">${esc(T.suggested)} <b>${esc(x.name)}</b>. ${esc(T.suggestNote)}</p>`;
      show(x.code);
    } catch {
      s.innerHTML = `<p class="note">${esc(T.suggestFail)}</p>`;
    }
  };
  if (sel) show(sel);
}

// ---------------------------------------------------------------- about

function renderAbout(body) {
  const src = state.sources || [];
  body.innerHTML = `<h3>${T.hSafe}</h3><p>${T.hSafeT}</p>
  <h3>${T.hMarkers}</h3><p>${T.hMarkersT}</p>
  <div class="fchips">${["emergency", "warning", "watch", "advisory", "information"].map((p) => `<span class="legend"><i style="background:${PRC[p]}"></i>${T.pr[p]}</span>`).join("")}</div>
  <p><span class="legend"><i class="sq" style="background:#0b8043"></i>${esc(T.facilities)}</span></p>
  <h3>${T.hFresh}</h3><p>${T.hFreshT}</p>
  <h3>${T.hSources}</h3>${src.filter((s) => s.enabled).map((s) => `<div class="srcrow"><span>${esc(s.agency_name)}<br><span class="sub">${T.lastOk}: ${s.health && s.health.last_http_success_at_ms ? esc(ago(s.health.last_http_success_at_ms)) : "—"}</span></span><span class="health ${esc(s.health && s.health.freshness_state)}">${esc(T.fresh[s.health && s.health.freshness_state] || "")}</span></div>`).join("") || `<p class="mt">—</p>`}
  <h3>${T.hCov}</h3><p>${T.hCovT}</p><p class="mt">${esc(T.notifyLimits)}</p><p class="mt">${esc(T.mapData)}</p>`;
}

// ---------------------------------------------------------------- screen

function closeHelp() {
  const h = $("help");
  h.hidden = true;
  $("app").inert = false;
  $("app").removeAttribute("aria-hidden");
  document.removeEventListener("keydown", onKey, true);
  if (location.hash === "#help") history.replaceState(null, "", location.pathname + location.search);
  if (prevFocus && prevFocus.focus) prevFocus.focus();
}
function onKey(e) {
  if (e.key === "Escape") {
    e.stopPropagation();
    closeHelp();
  }
}

export function openHelpScreen(opts) {
  ctx = opts || {};
  const h = $("help");
  if (h.hidden) prevFocus = document.activeElement;
  const tab = ctx.tab || "emergency";
  h.innerHTML = `<div class="hs-head"><h2 id="helpTitle">${T.helpTitle}</h2>${closeBtn("hclose")}
    <div class="tabs" role="tablist">${[["emergency", T.tabEmergency], ["guides", T.tabGuides], ["about", T.tabAbout]].map(([k, l]) => `<button role="tab" id="tab-${k}" aria-controls="hs-body" aria-selected="${k === tab}">${l}</button>`).join("")}</div></div>
    <div class="hs-body" id="hs-body" role="tabpanel"></div>`;
  h.hidden = false;
  $("app").inert = true;
  $("app").setAttribute("aria-hidden", "true");
  document.addEventListener("keydown", onKey, true);
  $("hclose").onclick = closeHelp;
  const show = (k) => {
    h.querySelectorAll("[role=tab]").forEach((b) => b.setAttribute("aria-selected", b.id === "tab-" + k));
    const body = $("hs-body");
    body.scrollTop = 0;
    if (k === "emergency") renderDirectory(body, ctx.country);
    else if (k === "guides") renderGuides(body);
    else {
      renderAbout(body);
      ctx.reloadMeta && ctx.reloadMeta().then(() => !h.hidden && $("tab-about").getAttribute("aria-selected") === "true" && renderAbout(body));
    }
  };
  h.querySelectorAll("[role=tab]").forEach((b) => (b.onclick = () => show(b.id.slice(4))));
  show(tab);
  $("hclose").focus();
}
