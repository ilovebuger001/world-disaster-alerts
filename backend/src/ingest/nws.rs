//! US National Weather Service alerts adapter (`https://api.weather.gov/alerts/active`).
//!
//! The NWS API publishes CAP 1.2 alerts as GeoJSON whose properties carry the
//! CAP fields one-to-one (identifier, sender, sent, status, messageType,
//! references, severity/urgency/certainty, headline, description, instruction,
//! areaDesc, geocodes). Each feature is converted into a [`CapMessage`] so the
//! exact CAP lifecycle (update / cancel / expiry, Actual + Public only) applies.
//! Nothing is invented: absent fields stay absent, zone-only alerts keep their
//! UGC/SAME codes and area description without a fabricated polygon.
//! NWS content is a US Government work (public domain); attribution is shown.

use crate::domain::geometry::GeoPoint;
use crate::domain::lifecycle::{content_hash, stable_id};
use crate::domain::message::{CapArea, CapInfo, CapMessage, CapRef, MessageBody, SourceMessage};
use crate::domain::parse_ts_ms;
use serde_json::Value;

/// Upper bound on geocodes kept per alert (large winter/heat products list hundreds of zones).
pub const MAX_GEOCODES: usize = 120;

#[derive(Debug, thiserror::Error)]
pub enum NwsError {
    #[error("malformed JSON: {0}")]
    Json(String),
    #[error("not a GeoJSON FeatureCollection")]
    NotCollection,
}

pub struct ParsedBatch {
    pub messages: Vec<SourceMessage>,
    pub rejected: Vec<String>,
}

fn s(p: &Value, k: &str) -> Option<String> {
    p.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

fn ring(v: &Value) -> Result<Vec<GeoPoint>, String> {
    let arr = v.as_array().ok_or("ring is not an array")?;
    let mut out = Vec::with_capacity(arr.len());
    for pos in arr {
        let (p, _) = crate::ingest::usgs::geojson::geojson_position(pos)?;
        out.push(p);
    }
    if out.len() < 4 {
        return Err("polygon ring needs at least 4 positions".into());
    }
    if out.first() != out.last() {
        return Err("polygon ring is not closed".into());
    }
    Ok(out)
}

/// Exterior rings of a Polygon / MultiPolygon geometry (null → none).
fn polygons(g: &Value) -> Result<Vec<Vec<GeoPoint>>, String> {
    if g.is_null() {
        return Ok(vec![]);
    }
    let coords = g.get("coordinates").ok_or("geometry without coordinates")?;
    match g.get("type").and_then(Value::as_str) {
        Some("Polygon") => Ok(vec![ring(coords.get(0).ok_or("empty polygon")?)?]),
        Some("MultiPolygon") => {
            coords.as_array().ok_or("bad multipolygon")?.iter().map(|poly| ring(poly.get(0).ok_or("empty polygon")?)).collect()
        }
        Some(t) => Err(format!("unsupported geometry type {t}")),
        None => Err("geometry without type".into()),
    }
}

/// Converts one NWS feature to a CAP message. Errors are reported, never coerced.
pub fn feature_to_cap(f: &Value) -> Result<CapMessage, String> {
    let p = f.get("properties").ok_or("feature without properties")?;
    let identifier = s(p, "id").ok_or("missing id")?;
    let sender = s(p, "sender").ok_or("missing sender")?;
    let sent = s(p, "sent").ok_or("missing sent")?;
    let sent_ms = parse_ts_ms(&sent).ok_or_else(|| format!("invalid sent '{sent}'"))?;
    let status = s(p, "status").ok_or("missing status")?;
    let msg_type = s(p, "messageType").ok_or("missing messageType")?;
    for t in ["effective", "onset", "expires", "ends"] {
        if let Some(v) = s(p, t) {
            if parse_ts_ms(&v).is_none() {
                return Err(format!("invalid {t} '{v}'"));
            }
        }
    }
    let references = p
        .get("references")
        .and_then(Value::as_array)
        .map(|refs| {
            refs.iter()
                .filter_map(|r| Some(CapRef { sender: s(r, "sender")?, identifier: s(r, "identifier")?, sent: s(r, "sent")? }))
                .collect()
        })
        .unwrap_or_default();

    let mut geocodes = Vec::new();
    if let Some(g) = p.get("geocode") {
        for name in ["UGC", "SAME"] {
            for v in g.get(name).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if geocodes.len() < MAX_GEOCODES {
                    geocodes.push((name.to_string(), v.to_string()));
                }
            }
        }
    }
    let polys = polygons(f.get("geometry").unwrap_or(&Value::Null))?;
    let mut parameters: Vec<(String, String)> = Vec::new();
    if let Some(obj) = p.get("parameters").and_then(Value::as_object) {
        for (k, v) in obj {
            for item in v.as_array().into_iter().flatten().filter_map(Value::as_str).take(5) {
                parameters.push((k.clone(), item.to_string()));
            }
        }
    }
    if let Some(e) = s(p, "ends") {
        parameters.push(("NWS_ends".into(), e));
    }
    let info = CapInfo {
        language: s(p, "language").unwrap_or_else(|| "en-US".into()),
        categories: s(p, "category").into_iter().collect(),
        event: s(p, "event").ok_or("missing event")?,
        response_types: s(p, "response").into_iter().collect(),
        urgency: s(p, "urgency").unwrap_or_else(|| "Unknown".into()),
        severity: s(p, "severity").unwrap_or_else(|| "Unknown".into()),
        certainty: s(p, "certainty").unwrap_or_else(|| "Unknown".into()),
        audience: None,
        event_codes: p
            .get("eventCode")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .flat_map(|(k, v)| v.as_array().into_iter().flatten().filter_map(Value::as_str).map(|x| (k.clone(), x.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        effective: s(p, "effective"),
        onset: s(p, "onset"),
        expires: s(p, "expires"),
        sender_name: s(p, "senderName"),
        headline: s(p, "headline"),
        description: s(p, "description"),
        instruction: s(p, "instruction"),
        web: s(p, "web").or_else(|| s(p, "@id")),
        contact: None,
        parameters,
        areas: vec![CapArea {
            area_desc: s(p, "areaDesc").unwrap_or_default(),
            polygons: polys,
            circles: vec![],
            geocodes,
            altitude: None,
            ceiling: None,
        }],
    };
    Ok(CapMessage {
        identifier,
        sender,
        sent,
        sent_ms,
        status,
        msg_type,
        source: Some("api.weather.gov".into()),
        scope: s(p, "scope").unwrap_or_else(|| "Public".into()),
        restriction: None,
        addresses: None,
        codes: s(p, "code").into_iter().collect(),
        note: None,
        references,
        incidents: None,
        infos: vec![info],
        has_signature: false,
    })
}

pub fn parse(source_id: &str, source_url: &str, body: &[u8], fetched_at_ms: i64) -> Result<ParsedBatch, NwsError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| NwsError::Json(e.to_string()))?;
    if v.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
        return Err(NwsError::NotCollection);
    }
    let mut messages = Vec::new();
    let mut rejected = Vec::new();
    for (i, f) in v.get("features").and_then(Value::as_array).into_iter().flatten().enumerate() {
        match feature_to_cap(f) {
            Ok(cap) => {
                let canonical = serde_json::to_vec(&cap).unwrap_or_default();
                let url = f.get("id").and_then(Value::as_str).filter(|u| u.starts_with("https://")).unwrap_or(source_url).to_string();
                messages.push(SourceMessage {
                    message_key: stable_id(&[source_id, "cap", &cap.cap_ref().key()]),
                    source_id: source_id.to_string(),
                    provider_record_id: cap.identifier.clone(),
                    source_url: url,
                    fetched_at_ms,
                    content_hash: content_hash(&canonical),
                    body: MessageBody::Cap(cap),
                });
            }
            Err(e) => rejected.push(format!("feature {i}: {e}")),
        }
    }
    Ok(ParsedBatch { messages, rejected })
}
