//! USGS earthquake adapter (spec §4.2, §7.1 earthquakes).
//! Catalog events are post-event information; magnitude stays in provider
//! terminology and is never converted into a warning zone.
pub mod geojson;

use crate::domain::alert::{AlertRecord, AlertStatus, LanguageVariant};
use crate::domain::geometry::GeometryPart;
use crate::domain::lifecycle::{
    append_history_only, content_hash, record_message, stable_id, ApplyCtx, ApplyOutcome, LifecycleStore, SkipReason,
};
use crate::domain::message::{MessageBody, SourceMessage, UsgsEvent};
use crate::domain::severity::MapInput;
use std::collections::BTreeMap;

pub struct ParsedBatch {
    pub messages: Vec<SourceMessage>,
    pub rejected: Vec<String>,
}

pub fn parse(source_id: &str, source_url: &str, body: &[u8], fetched_at_ms: i64) -> Result<ParsedBatch, geojson::GeoJsonError> {
    let parsed = geojson::parse_usgs(body)?;
    let messages = parsed
        .events
        .into_iter()
        .map(|ev| {
            let canonical = serde_json::to_vec(&ev).unwrap_or_default();
            SourceMessage {
                // One immutable message per (event id, updated) version.
                message_key: stable_id(&[source_id, "usgs", &ev.id, &ev.updated_ms.to_string()]),
                source_id: source_id.to_string(),
                provider_record_id: ev.id.clone(),
                source_url: ev.url.clone().unwrap_or_else(|| source_url.to_string()),
                fetched_at_ms,
                content_hash: content_hash(&canonical),
                body: MessageBody::Usgs(ev),
            }
        })
        .collect();
    Ok(ParsedBatch { messages, rejected: parsed.skipped.into_iter().map(|(id, r)| format!("{id}: {r}")).collect() })
}

pub fn ref_key(source_id: &str, provider_id: &str) -> String {
    format!("usgs\u{1f}{source_id}\u{1f}{provider_id}")
}

pub fn apply_usgs(store: &mut dyn LifecycleStore, msg: &SourceMessage, ev: &UsgsEvent, ctx: &ApplyCtx) -> anyhow::Result<ApplyOutcome> {
    let mut out = ApplyOutcome::default();
    let key = msg.message_key.clone();
    if let Some(r) = record_message(store, msg)? {
        out.skipped.push((key, r));
        return Ok(out);
    }
    // Follow preferred-id changes through the `ids` list.
    let mut target: Option<String> = None;
    for pid in std::iter::once(&ev.id).chain(ev.ids.iter()) {
        if let Some(id) = store.get_ref(&ref_key(ctx.source_id, pid))? {
            target = Some(id);
            break;
        }
    }
    let id = target.unwrap_or_else(|| stable_id(&[ctx.source_id, "usgs", &ev.id]));
    let existing = store.get_alert(&id)?;
    if let Some(ex) = &existing {
        if ev.updated_ms <= ex.applied_message_time_ms {
            append_history_only(store, ex, &key)?;
            out.skipped.push((key, SkipReason::OlderThanApplied));
            return Ok(out);
        }
    }
    let withdrawn = ev.status.as_deref() == Some("deleted");
    let hazard = "earthquake".to_string();
    let priority =
        ctx.mapping.priority(&MapInput { source_id: ctx.source_id, hazard_type: &hazard, severity: None, urgency: None, certainty: None });
    let mut native = BTreeMap::new();
    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            native.insert(k.to_string(), v);
        }
    };
    put("magnitude", ev.mag.map(|m| m.to_string()));
    put("magnitude_type", ev.mag_type.clone());
    put("depth_km", ev.depth_km.map(|d| d.to_string()));
    put("origin_time_ms", Some(ev.time_ms.to_string()));
    put("usgs_status", ev.status.clone());
    put("usgs_event_type", ev.event_type.clone());
    put("tsunami_flag", ev.tsunami.map(|t| t.to_string()));
    put("pager_alert", ev.alert.clone());
    put("mmi", ev.mmi.map(|v| v.to_string()));
    put("cdi", ev.cdi.map(|v| v.to_string()));
    put("felt_reports", ev.felt.map(|v| v.to_string()));
    put("significance", ev.sig.map(|v| v.to_string()));
    put("network", ev.net.clone());
    put("detail_url", ev.detail.clone());
    put("usgs_ids", Some(ev.ids.join(",")));

    let geometry_parts = match ev.point {
        Some(p) => vec![GeometryPart::Point { point: p }],
        None => vec![GeometryPart::TextOnly { description: ev.place.clone().unwrap_or_default() }],
    };
    let mut history = existing.as_ref().map(|e| e.message_history.clone()).unwrap_or_default();
    history.push(key.clone());
    let rec = AlertRecord {
        alert_id: id.clone(),
        source_id: ctx.source_id.to_string(),
        source_record_id: ev.id.clone(),
        source_agency: ctx.source_agency.to_string(),
        source_url: msg.source_url.clone(),
        source_published_at_ms: ev.time_ms,
        source_updated_at_ms: Some(ev.updated_ms),
        fetched_at_ms: msg.fetched_at_ms,
        first_seen_at_ms: existing.as_ref().map_or(ctx.now_ms, |e| e.first_seen_at_ms),
        last_seen_at_ms: ctx.now_ms,
        revision: existing.as_ref().map_or(1, |e| e.revision + 1),
        status: if withdrawn { AlertStatus::Withdrawn } else { AlertStatus::Active },
        source_status: ev.status.clone().unwrap_or_default(),
        message_type: if existing.is_some() { "update".into() } else { "event".into() },
        scope: "public".into(),
        language_variants: vec![LanguageVariant {
            language: "en".into(),
            event: ev.event_type.clone(),
            headline: ev.title.clone(),
            description: ev.place.clone(),
            instruction: None,
            sender_name: Some(ctx.source_agency.to_string()),
            web: ev.url.clone(),
            area_desc: ev.place.clone().into_iter().collect(),
        }],
        hazard_type: hazard,
        hazard_raw: ev.event_type.clone().unwrap_or_else(|| "earthquake".into()),
        severity_raw: None,
        urgency_raw: None,
        certainty_raw: None,
        severity_mapping_version: ctx.mapping.mapping_version.clone(),
        display_priority: priority,
        response_types: vec![],
        start_at_ms: Some(ev.time_ms),
        onset_at_ms: None,
        expires_at_ms: None,
        geometry_parts,
        native,
        original_payload_ref: key,
        message_history: history,
        content_hash: msg.content_hash.clone(),
        applied_message_time_ms: ev.updated_ms,
        cancelled: false,
    };
    store.put_alert(&rec)?;
    for pid in std::iter::once(&ev.id).chain(ev.ids.iter()) {
        let k = ref_key(ctx.source_id, pid);
        if store.get_ref(&k)?.is_none() {
            store.put_ref(&k, &id)?;
        }
    }
    out.changed.push(rec);
    Ok(out)
}
