// Alert detail panel (loaded on first use). Order: what/where/when and what to
// do first; then impacts, areas, guidance, event-linked facilities, updates and
// sources; technical CAP/ingestion fields last, collapsed.
import { api } from "./api.js";
import { T, LANG, fmtTime } from "./i18n.js";
import { prefs } from "./store.js";
import { PRC, FAC, glyph } from "./icons.js";
import { esc, closeBtn, safeUrl, state as S, closeAlert } from "./app.js";

const $ = (id) => document.getElementById(id);

export function summaryKv(a) {
  const u = safeUrl(a.source_url);
  return `<dl class="kv"><dt>${T.agency}</dt><dd>${esc(a.source_agency)}</dd><dt>${T.status}</dt><dd>${esc(T.st[a.status] || a.status)}</dd><dt>${T.issued}</dt><dd>${esc(fmtTime(a.source_published_at_ms))}</dd></dl>${u ? `<a class="srcbtn" href="${esc(u)}" target="_blank" rel="noopener noreferrer">${T.openSrc} ↗</a>` : ""}`;
}

function firstSentences(s, n = 260) {
  s = String(s || "").trim();
  if (s.length <= n) return s;
  const cut = s.slice(0, n);
  const i = Math.max(cut.lastIndexOf(". "), cut.lastIndexOf("。"), cut.lastIndexOf(" "));
  return cut.slice(0, i > 80 ? i + 1 : n).trim() + "…";
}

export function renderDetail(d) {
  const a = d.alert;
  const c = PRC[a.display_priority] || PRC.unknown;
  const v0 = a.language_variants[0] || {};
  const title = v0.headline || v0.event || T.hz[a.hazard_type] || a.hazard_type;
  const areas = [...new Set(a.language_variants.flatMap((v) => v.area_desc || []))];
  const instr = a.language_variants.map((v) => v.instruction && { l: v.language, t: v.instruction }).filter(Boolean);
  const eventMs = a.onset_at_ms || a.start_at_ms || (a.native && +a.native.origin_time_ms) || null;
  const nat = Object.entries(a.native || {});
  const src = safeUrl(a.source_url);
  const hasGeom = (a.geometry_parts || []).some((g) => g.type === "polygon" || g.type === "circle");
  const variants = a.language_variants
    .map((v) => {
      const parts = [v.description].filter(Boolean).join("\n\n");
      return parts ? `<div class="mt">${esc(v.sender_name || a.source_agency)} · ${esc(v.language)}</div><div class="quote" lang="${esc(v.language)}">${esc(parts)}</div>` : "";
    })
    .join("");
  const geomSummary = (a.geometry_parts || []).map((g) => (g.type === "polygon" ? `polygon (${g.exterior.length} pts)` : g.type === "circle" ? `circle r=${g.radius_km} km` : g.type === "point" ? `point ${g.point.lat_deg.toFixed(3)}, ${g.point.lon_deg.toFixed(3)}` : g.type === "geocode" ? `${g.value_name}=${g.value}` : g.type)).join("; ");
  $("detailInner").innerHTML = `
  <div class="dhead"><div>
    <span class="badge" style="background:${c}">${glyph(a.hazard_type, 14)}${esc(T.hz[a.hazard_type] || a.hazard_type)} · ${esc(T.pr[a.display_priority] || a.display_priority)}</span><span class="pill2 st-${esc(a.status)}">${esc(T.st[a.status] || a.status)}</span>
    <h1>${esc(title)}</h1>
  </div>${closeBtn("dclose")}</div>
  ${a.status === "cancelled" || a.status === "expired" || a.status === "withdrawn" ? `<p class="note warn">${esc(T.st[a.status])}</p>` : ""}
  ${v0.description ? `<p class="lead" lang="${esc(v0.language)}">${esc(firstSentences(v0.description))}</p>` : ""}
  <dl class="kv">
    ${areas.length ? `<dt>${T.where}</dt><dd>${esc(areas.join(" · "))}</dd>` : ""}
    <dt>${T.agency}</dt><dd>${esc(a.source_agency)}</dd>
    ${eventMs ? `<dt>${T.eventTime}</dt><dd>${esc(fmtTime(eventMs))}</dd>` : ""}
    <dt>${T.issued}</dt><dd>${esc(fmtTime(a.source_published_at_ms))}</dd>
    ${a.source_updated_at_ms ? `<dt>${T.updatedSrc}</dt><dd>${esc(fmtTime(a.source_updated_at_ms))}</dd>` : ""}
    <dt>${T.received}</dt><dd>${esc(fmtTime(a.first_seen_at_ms))}</dd>
    <dt>${T.expires}</dt><dd>${a.expires_at_ms ? esc(fmtTime(a.expires_at_ms)) : esc(T.expiresUnknown)}</dd>
    <dt>${T.freshness}</dt><dd><span class="health ${esc(d.source_freshness)}">${esc(T.fresh[d.source_freshness] || d.source_freshness)}</span></dd>
  </dl>
  <div class="sec todo"><h2>${T.whatToDo}</h2>${instr.length ? instr.map((i) => `<div class="quote" lang="${esc(i.l)}">${esc(i.t)}</div>`).join("") : `<p class="mt">${esc(T.noInstr)}</p>`}</div>
  ${a.hazard_type === "earthquake" ? `<p class="note">${esc(T.eqNote)}</p>` : ""}
  ${d.source && !d.source.public_redistribution ? `<p class="note">${esc(T.notPublic)}</p>` : ""}
  ${variants ? `<div class="sec"><h2>${T.impacts}</h2>${variants}</div>` : ""}
  ${areas.length ? `<div class="sec"><h2>${T.areas}</h2><p>${esc(areas.join(" · "))}</p><div class="mt">${hasGeom ? T.geomOfficial : T.geomNone}</div></div>` : `<div class="sec"><div class="mt">${hasGeom ? T.geomOfficial : T.geomNone}</div></div>`}
  <div class="sec" id="guideSec"><h2>${T.guidance}</h2><p class="mt">${esc(T.loading)}</p></div>
  <div class="sec" id="facSec"><h2>${T.facilities}</h2><p class="mt">${esc(T.loading)}</p></div>
  <div class="sec" id="ctSec"></div>
  <div class="sec" id="relSec"><h2>${T.related}</h2><p class="mt">${esc(T.loading)}</p></div>
  <div class="sec"><h2>${T.updates}</h2><p>${T.revision} ${a.revision}${a.message_history && a.message_history.length ? ` · ${a.message_history.length}` : ""}</p></div>
  <div class="sec"><h2>${T.sources}</h2>
    ${src ? `<a class="srcbtn" href="${esc(src)}" target="_blank" rel="noopener noreferrer">${T.openSrc} ↗</a><div class="mt brk">${esc(src)}</div>` : ""}
    ${d.source && safeUrl(d.source.official_homepage) ? `<div class="mt"><a href="${esc(safeUrl(d.source.official_homepage))}" target="_blank" rel="noopener noreferrer">${esc(d.source.agency_name)}</a>${d.source.attribution ? " · " + esc(d.source.attribution) : ""}</div>` : ""}
  </div>
  <div class="sec" id="trSec"><h2>${T.translation}</h2>${
    d.translation_available
      ? (a.language_variants || []).some((v) => String(v.language || "").toLowerCase().startsWith(LANG))
        ? `<div class="mt">${esc(T.trSame)}</div>`
        : `<button class="menuitem" id="trBtn" type="button">${esc(T.trButton)}</button><div id="trOut"></div>`
      : `<div class="mt">${esc(T.trOff)}</div>`
  }</div>
  <details class="sec tech"><summary>${T.technical}</summary>
    <dl class="kv">
      <dt>${T.hazard}</dt><dd>${esc(a.hazard_raw)}</dd>
      ${a.severity_raw ? `<dt>Severity</dt><dd>${esc(a.severity_raw)}</dd><dt>Urgency</dt><dd>${esc(a.urgency_raw)}</dd><dt>Certainty</dt><dd>${esc(a.certainty_raw)}</dd>` : ""}
      <dt>msgType / status / scope</dt><dd>${esc(a.message_type)} / ${esc(a.source_status)} / ${esc(a.scope)}</dd>
      ${(a.response_types || []).length ? `<dt>responseType</dt><dd>${esc(a.response_types.join(", "))}</dd>` : ""}
      <dt>${T.display}</dt><dd>${esc(T.pr[a.display_priority] || a.display_priority)} (${esc(a.severity_mapping_version)})</dd>
      <dt>${T.ids}</dt><dd class="brk">${esc(a.alert_id)}<br>${esc(a.source_id)} / ${esc(a.source_record_id)}</dd>
      ${nat.map(([k, v]) => `<dt>${esc(k)}</dt><dd class="brk">${esc(v)}</dd>`).join("")}
      <dt>Geometry</dt><dd class="brk">${esc(geomSummary || "—")}</dd>
      <dt>${T.fetched}</dt><dd>${esc(fmtTime(a.fetched_at_ms))}</dd>
      <dt>First / last seen</dt><dd>${esc(fmtTime(a.first_seen_at_ms))}<br>${esc(fmtTime(a.last_seen_at_ms))}</dd>
      <dt>${T.revision}</dt><dd>${a.revision}</dd>
      <dt>Content hash</dt><dd class="brk">${esc(a.content_hash)}</dd>
      ${(a.message_history || []).length ? `<dt>History</dt><dd class="brk">${a.message_history.map(esc).join("<br>")}</dd>` : ""}
      <dt>Signature</dt><dd>${esc(T.sigNote)}</dd>
    </dl>
  </details>`;
  $("dclose").onclick = () => closeAlert();
  $("detailInner").scrollTop = 0;
}

export async function loadFacilities(a) {
  const sec = () => $("facSec");
  let body;
  try {
    body = await api.facilities(a.alert_id);
  } catch {
    if (sec()) sec().innerHTML = `<h2>${T.facilities}</h2><p class="mt">${esc(T.serverDown)}</p>`;
    return;
  }
  if (!S.selected || S.selected.id !== a.alert_id || !sec()) return;
  const items = body.items || [];
  if (S.map) S.map.setFacilities(items, (f) => `${f.name} · ${T.facSt[f.status] || f.status}`);
  sec().innerHTML =
    `<h2>${T.facilities}</h2>` +
    (items.length
      ? items
          .map(
            (f) =>
              `<div class="facrow"><span class="facico" style="background:${FAC[f.status] || FAC.unverified}">${glyph(f.kind === "hospital" ? "hospital" : "shelter", 14)}</span><span><b>${esc(f.name)}</b><br><span class="sub">${esc(T.facKind[f.kind] || f.kind)} · ${esc(T.facSt[f.status] || f.status)}${f.capacity ? " · " + f.capacity : ""}</span><br><span class="sub">${esc(f.authority)}${f.last_verified ? " · " + esc(f.last_verified) : ""} · <a href="${esc(safeUrl(f.source_url))}" target="_blank" rel="noopener noreferrer">${T.source}</a></span></span></div>`
          )
          .join("")
      : `<p class="mt">${esc(body.status === "event_not_active" ? T.facInactive : T.facNone)}</p>`);
}

export async function loadGuideInto(hazard, country) {
  const { guideHtml } = await import("./help.js");
  const html = await guideHtml(hazard, true, country);
  const sec = $("guideSec");
  if (sec && S.selected && S.selected.record && S.selected.record.hazard_type === hazard) sec.innerHTML = `<h2>${T.guidance}</h2>${html}`;
}


/** Emergency numbers: the issuing authority's country when the server could
 * determine it; otherwise the country the user chose in Help. Never guessed. */
export async function loadContacts(d) {
  const sec = $("ctSec");
  if (!sec) return;
  const { countryCard } = await import("./help.js");
  let html;
  if (d.country_contacts) {
    html = `<h2>${esc(T.contactsFor)} ${esc(d.country_contacts.name)}</h2>${countryCard(d.country_contacts)}`;
  } else {
    const mine = prefs.get("country", "");
    let c = null;
    if (mine) {
      try {
        c = (await api.contact(mine)).country;
      } catch {}
    }
    html = c
      ? `<h2>${esc(T.contactsMine)}</h2><p class="mt">${esc(T.contactsUnknown)}</p>${countryCard(c)}`
      : `<h2>${esc(T.tabEmergency)}</h2><p class="mt">${esc(T.contactsUnknown)}</p><button class="menuitem" id="ctOpen" type="button">${esc(T.openHelpDir)}</button>`;
  }
  if (!$("ctSec")) return;
  $("ctSec").innerHTML = html;
  const b = $("ctOpen");
  if (b) b.onclick = async () => (await import("./help.js")).openHelpScreen({ tab: "emergency" });
}

export async function loadRelated(a) {
  let body;
  try {
    body = await api.related(a.alert_id);
  } catch {
    if ($("relSec")) $("relSec").innerHTML = `<h2>${T.related}</h2><p class="mt">${esc(T.serverDown)}</p>`;
    return;
  }
  if (!$("relSec") || !S.selected || S.selected.id !== a.alert_id) return;
  const items = body.items || [];
  $("relSec").innerHTML =
    `<h2>${T.related}</h2>` +
    (items.length
      ? `<p class="mt">${esc(T.relatedBasis)}</p>` +
        items
          .map(
            (r) =>
              `<button class="item" data-rel="${esc(r.alert_id)}"><span class="ico" style="background:${PRC[r.display_priority] || PRC.unknown}">${glyph(r.hazard_type)}</span><span><span class="t">${esc(r.headline || T.hz[r.hazard_type] || r.hazard_type)}</span><br><span class="m">${esc(r.source_agency)} · ${esc(String(r.distance_km))} ${esc(T.km)}</span></span></button>`
          )
          .join("")
      : `<p class="mt">${esc(T.relatedNone)}</p>`);
  $("relSec").querySelectorAll("[data-rel]").forEach((b) => (b.onclick = () => (location.hash = b.dataset.rel)));
}

export function bindTranslate(a) {
  const b = $("trBtn");
  if (!b) return;
  b.onclick = async () => {
    b.disabled = true;
    const out = $("trOut");
    out.innerHTML = `<p class="mt">${esc(T.loading)}</p>`;
    try {
      const r = await api.translate(a.alert_id, LANG);
      out.innerHTML =
        `<p class="note warn">${esc(T.trLabel)} · ${esc(r.provider)}</p>` +
        Object.entries(r.items || {})
          .map(([, v]) => `<div class="quote" lang="${esc(r.target_language)}">${esc(v)}</div>`)
          .join("");
    } catch (e) {
      out.innerHTML = `<p class="mt">${esc(e.message || T.trFail)}</p>`;
      b.disabled = false;
    }
  };
}
