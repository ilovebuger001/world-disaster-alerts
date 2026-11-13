// Alert detail panel (loaded on first use). Order: what/where/when and what to
// do first; then impacts, areas, guidance, event-linked facilities, updates and
// sources; technical CAP/ingestion fields last, collapsed.
import { api } from "./api.js";
import { T, fmtTime } from "./i18n.js";
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
  <div class="sec"><h2>${T.updates}</h2><p>${T.revision} ${a.revision}${a.message_history && a.message_history.length ? ` · ${a.message_history.length}` : ""}</p></div>
  <div class="sec"><h2>${T.sources}</h2>
    ${src ? `<a class="srcbtn" href="${esc(src)}" target="_blank" rel="noopener noreferrer">${T.openSrc} ↗</a><div class="mt brk">${esc(src)}</div>` : ""}
    ${d.source && safeUrl(d.source.official_homepage) ? `<div class="mt"><a href="${esc(safeUrl(d.source.official_homepage))}" target="_blank" rel="noopener noreferrer">${esc(d.source.agency_name)}</a>${d.source.attribution ? " · " + esc(d.source.attribution) : ""}</div>` : ""}
  </div>
  <div class="sec"><h2>${T.translation}</h2><div class="mt">${T.trUnavailable}</div></div>
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

export async function loadGuideInto(hazard) {
  const { guideHtml } = await import("./help.js");
  const html = await guideHtml(hazard, true);
  const sec = $("guideSec");
  if (sec && S.selected && S.selected.record && S.selected.record.hazard_type === hazard) sec.innerHTML = `<h2>${T.guidance}</h2>${html}`;
}

