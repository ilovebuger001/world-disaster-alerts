//! CAP lifecycle (spec §5.3):
//! * only `status=Actual` + `scope=Public` produce public records;
//! * Ack/Error never become markers;
//! * Update/Cancel follow the exact `(sender, identifier, sent)` references;
//! * history is kept, cancellations deactivate without deleting;
//! * duplicates are idempotent, out-of-order messages never roll state back.

use super::geometry::geometry_from_infos;
use crate::domain::alert::{AlertRecord, DisplayPriority, LanguageVariant};
use crate::domain::lifecycle::{
    append_history_only, derive_status, record_message, stable_id, ApplyCtx, ApplyOutcome, LifecycleStore, SkipReason,
};
use crate::domain::message::{CapMessage, SourceMessage};
use crate::domain::parse_ts_ms;
use crate::domain::severity::MapInput;
use std::collections::BTreeMap;

pub fn cap_ref_key(source_id: &str, k: &str) -> String {
    format!("cap\u{1f}{source_id}\u{1f}{k}")
}

pub fn apply_cap(store: &mut dyn LifecycleStore, msg: &SourceMessage, cap: &CapMessage, ctx: &ApplyCtx) -> anyhow::Result<ApplyOutcome> {
    let mut out = ApplyOutcome::default();
    let key = msg.message_key.clone();
    if let Some(r) = record_message(store, msg)? {
        out.skipped.push((key, r));
        return Ok(out);
    }
    // Rule 5 (spec §1.2): test/exercise/draft/system and non-public never shown.
    if cap.status != "Actual" {
        out.skipped.push((key, SkipReason::NotActual(cap.status.clone())));
        return Ok(out);
    }
    if cap.scope != "Public" {
        out.skipped.push((key, SkipReason::NotPublic(cap.scope.clone())));
        return Ok(out);
    }
    if !matches!(cap.msg_type.as_str(), "Alert" | "Update" | "Cancel") {
        out.skipped.push((key, SkipReason::NonAlertMessageType(cap.msg_type.clone())));
        return Ok(out);
    }

    let own = cap_ref_key(ctx.source_id, &cap.cap_ref().key());
    let ref_keys: Vec<String> = cap.references.iter().map(|r| cap_ref_key(ctx.source_id, &r.key())).collect();

    // Targets: an alert already claimed by this message (out-of-order arrival)
    // plus every alert reachable through exact references.
    let mut targets: Vec<String> = Vec::new();
    for k in std::iter::once(&own).chain(ref_keys.iter()) {
        if let Some(id) = store.get_ref(k)? {
            if !targets.contains(&id) {
                targets.push(id);
            }
        }
    }
    if targets.is_empty() {
        // First message of a thread (or an Update/Cancel whose originals have
        // not arrived yet): the thread is identified by this message.
        targets.push(stable_id(&[ctx.source_id, "cap", &cap.cap_ref().key()]));
    }

    for id in &targets {
        let existing = store.get_alert(id)?;
        if let Some(ex) = &existing {
            if cap.sent_ms < ex.applied_message_time_ms {
                append_history_only(store, ex, &key)?;
                out.skipped.push((key.clone(), SkipReason::OlderThanApplied));
                continue;
            }
            if ex.cancelled && cap.msg_type != "Cancel" {
                append_history_only(store, ex, &key)?;
                out.skipped.push((key.clone(), SkipReason::AlreadyCancelled));
                continue;
            }
        }
        let rec = build_record(id, existing.as_ref(), msg, cap, ctx);
        store.put_alert(&rec)?;
        out.changed.push(rec);
    }

    // Register this message and its references so later/earlier messages of
    // the same thread resolve to the same alert(s).
    for id in &targets {
        if store.get_ref(&own)?.is_none() {
            store.put_ref(&own, id)?;
        }
        for k in &ref_keys {
            if store.get_ref(k)?.is_none() {
                store.put_ref(k, id)?;
            }
        }
    }
    Ok(out)
}

fn build_record(id: &str, existing: Option<&AlertRecord>, msg: &SourceMessage, cap: &CapMessage, ctx: &ApplyCtx) -> AlertRecord {
    let is_cancel = cap.msg_type == "Cancel";
    let mut history = existing.map(|e| e.message_history.clone()).unwrap_or_default();
    if !history.contains(&msg.message_key) {
        history.push(msg.message_key.clone());
    }
    let revision = existing.map_or(1, |e| e.revision + 1);
    let first_seen = existing.map_or(ctx.now_ms, |e| e.first_seen_at_ms);

    // A Cancel may carry no info; keep the existing wording/geometry then.
    let use_infos = !cap.infos.is_empty();
    let primary = cap.infos.first();

    let (language_variants, geometry_parts, hazard_type, hazard_raw, sev, urg, cer, priority, resp, start, onset, expires, agency_name) =
        if use_infos {
            let variants: Vec<LanguageVariant> = cap
                .infos
                .iter()
                .map(|i| LanguageVariant {
                    language: i.language.clone(),
                    event: Some(i.event.clone()),
                    headline: i.headline.clone(),
                    description: i.description.clone(),
                    instruction: i.instruction.clone(),
                    sender_name: i.sender_name.clone(),
                    web: i.web.clone(),
                    area_desc: i.areas.iter().map(|a| a.area_desc.clone()).collect(),
                })
                .collect();
            let p = primary.expect("info");
            let mut texts: Vec<&str> = vec![p.event.as_str()];
            texts.extend(p.categories.iter().map(String::as_str));
            let hazard = ctx.mapping.hazard(&texts);
            // Highest display priority across info blocks (deterministic).
            let priority = cap
                .infos
                .iter()
                .map(|i| {
                    ctx.mapping.priority(&MapInput {
                        source_id: ctx.source_id,
                        hazard_type: &hazard,
                        severity: Some(&i.severity),
                        urgency: Some(&i.urgency),
                        certainty: Some(&i.certainty),
                    })
                })
                .min_by_key(|p| p.rank())
                .unwrap_or(DisplayPriority::Unknown);
            // CAP: effective defaults to sent when absent.
            let start = p.effective.as_deref().and_then(parse_ts_ms).or(Some(cap.sent_ms));
            (
                variants,
                geometry_from_infos(&cap.infos),
                hazard,
                format!("{} | {}", p.event, p.categories.join(",")),
                Some(p.severity.clone()),
                Some(p.urgency.clone()),
                Some(p.certainty.clone()),
                priority,
                p.response_types.clone(),
                start,
                p.onset.as_deref().and_then(parse_ts_ms),
                p.expires.as_deref().and_then(parse_ts_ms),
                p.sender_name.clone(),
            )
        } else if let Some(e) = existing {
            (
                e.language_variants.clone(),
                e.geometry_parts.clone(),
                e.hazard_type.clone(),
                e.hazard_raw.clone(),
                e.severity_raw.clone(),
                e.urgency_raw.clone(),
                e.certainty_raw.clone(),
                e.display_priority,
                e.response_types.clone(),
                e.start_at_ms,
                e.onset_at_ms,
                e.expires_at_ms,
                None,
            )
        } else {
            (
                vec![],
                vec![],
                "other".into(),
                String::new(),
                None,
                None,
                None,
                DisplayPriority::Unknown,
                vec![],
                Some(cap.sent_ms),
                None,
                None,
                None,
            )
        };

    let cancelled = is_cancel || existing.is_some_and(|e| e.cancelled);
    let status = derive_status(ctx.now_ms, start, onset, expires, cancelled, false);
    let web = cap.infos.iter().find_map(|i| i.web.clone());
    let mut native = BTreeMap::new();
    native.insert("cap_identifier".into(), cap.identifier.clone());
    native.insert("cap_sender".into(), cap.sender.clone());
    native.insert("cap_sent".into(), cap.sent.clone());
    native.insert("cap_signature_present".into(), cap.has_signature.to_string());
    native.insert("cap_signature_verified".into(), "false".into());
    if let Some(n) = &cap.note {
        native.insert("cap_note".into(), n.clone());
    }

    AlertRecord {
        alert_id: id.to_string(),
        source_id: ctx.source_id.to_string(),
        source_record_id: cap.identifier.clone(),
        source_agency: agency_name.unwrap_or_else(|| ctx.source_agency.to_string()),
        source_url: web.unwrap_or_else(|| msg.source_url.clone()),
        source_published_at_ms: existing.map_or(cap.sent_ms, |e| e.source_published_at_ms),
        source_updated_at_ms: if existing.is_some() { Some(cap.sent_ms) } else { None },
        fetched_at_ms: msg.fetched_at_ms,
        first_seen_at_ms: first_seen,
        last_seen_at_ms: ctx.now_ms,
        revision,
        status,
        source_status: cap.status.clone(),
        message_type: cap.msg_type.clone(),
        scope: cap.scope.clone(),
        language_variants,
        hazard_type,
        hazard_raw,
        severity_raw: sev,
        urgency_raw: urg,
        certainty_raw: cer,
        severity_mapping_version: ctx.mapping.mapping_version.clone(),
        display_priority: priority,
        response_types: resp,
        start_at_ms: start,
        onset_at_ms: onset,
        expires_at_ms: expires,
        geometry_parts,
        native,
        original_payload_ref: msg.message_key.clone(),
        message_history: history,
        content_hash: msg.content_hash.clone(),
        applied_message_time_ms: cap.sent_ms,
        cancelled,
    }
}
