//! Streaming, bounded CAP 1.2 parser (spec §4.4, §38.2, §38.3).
//!
//! * namespace `urn:oasis:names:tc:emergency:cap:1.2` is required;
//! * DTDs are rejected, no entity/resource resolution;
//! * depth / element / text / info / area / vertex limits;
//! * multiple `<info>`, `<area>`, polygons, circles, geocodes preserved;
//! * malformed messages are rejected (quarantined), never coerced.
//!
//! A document may contain one `<alert>` or several (e.g. inside a feed);
//! each alert is returned independently so one bad alert does not hide others.

use crate::domain::geometry::GeoPoint;
use crate::domain::message::{CapArea, CapCircle, CapInfo, CapMessage, CapRef};
use crate::domain::parse_ts_ms;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;

pub const CAP12_NS: &[u8] = b"urn:oasis:names:tc:emergency:cap:1.2";

#[derive(Clone, Debug)]
pub struct CapLimits {
    pub max_depth: usize,
    pub max_elements: usize,
    pub max_text_bytes: usize,
    pub max_infos: usize,
    pub max_areas: usize,
    pub max_polygons: usize,
    pub max_vertices: usize,
}

impl Default for CapLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_elements: 50_000,
            max_text_bytes: 64 * 1024,
            max_infos: 16,
            max_areas: 64,
            max_polygons: 256,
            max_vertices: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CapError {
    #[error("XML error: {0}")]
    Xml(String),
    #[error("DTD / DOCTYPE is not allowed")]
    DtdRejected,
    #[error("limit exceeded: {0}")]
    Limit(&'static str),
    #[error("missing required field: {0}")]
    Missing(&'static str),
    #[error("invalid value for {field}: {value}")]
    Invalid { field: &'static str, value: String },
    #[error("no CAP 1.2 <alert> element found")]
    NoAlert,
}

#[derive(Default)]
struct AlertBuilder {
    identifier: Option<String>,
    sender: Option<String>,
    sent: Option<String>,
    status: Option<String>,
    msg_type: Option<String>,
    source: Option<String>,
    scope: Option<String>,
    restriction: Option<String>,
    addresses: Option<String>,
    codes: Vec<String>,
    note: Option<String>,
    references: Option<String>,
    incidents: Option<String>,
    infos: Vec<CapInfo>,
    has_signature: bool,
    error: Option<CapError>,
    polygons: usize,
    vertices: usize,
}

const STATUS: &[&str] = &["Actual", "Exercise", "System", "Test", "Draft"];
const MSG_TYPES: &[&str] = &["Alert", "Update", "Cancel", "Ack", "Error"];
const SCOPES: &[&str] = &["Public", "Restricted", "Private"];
const URGENCY: &[&str] = &["Immediate", "Expected", "Future", "Past", "Unknown"];
const SEVERITY: &[&str] = &["Extreme", "Severe", "Moderate", "Minor", "Unknown"];
const CERTAINTY: &[&str] = &["Observed", "Likely", "Possible", "Unlikely", "Unknown", "Very Likely"];

/// Parse all CAP 1.2 alerts in `body`. Returns an error only for document-level
/// failures (malformed XML, DTD, global limits); per-alert problems are
/// returned as `Err` items.
pub fn parse_cap_document(body: &[u8], limits: &CapLimits) -> Result<Vec<Result<CapMessage, CapError>>, CapError> {
    let mut reader = NsReader::from_reader(body);
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;

    let mut buf = Vec::new();
    let mut out: Vec<Result<CapMessage, CapError>> = Vec::new();
    // Path of local names *within* the current <alert>, alert itself excluded.
    let mut path: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut elements = 0usize;
    let mut in_alert: Option<(usize, AlertBuilder)> = None; // (depth of <alert>, builder)
    let mut text = String::new();
    // Scratch for nested pair elements (eventCode, parameter, geocode).
    let mut pair_name: Option<String> = None;
    let mut pair_value: Option<String> = None;

    loop {
        let (ns, ev) = reader.read_resolved_event_into(&mut buf).map_err(|e| CapError::Xml(e.to_string()))?;
        match ev {
            Event::DocType(_) => return Err(CapError::DtdRejected),
            Event::Start(e) => {
                depth += 1;
                elements += 1;
                if depth > limits.max_depth {
                    return Err(CapError::Limit("xml depth"));
                }
                if elements > limits.max_elements {
                    return Err(CapError::Limit("xml element count"));
                }
                let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let is_cap = matches!(ns, ResolveResult::Bound(ref n) if n.as_ref() == CAP12_NS);
                text.clear();
                match &mut in_alert {
                    None => {
                        if local == "alert" && is_cap {
                            in_alert = Some((depth, AlertBuilder::default()));
                            path.clear();
                        }
                    }
                    Some((_, b)) => {
                        if local == "Signature" && !is_cap {
                            b.has_signature = true;
                        }
                        path.push(local.clone());
                        let p: Vec<&str> = path.iter().map(String::as_str).collect();
                        match p.as_slice() {
                            ["info"] => {
                                if b.infos.len() >= limits.max_infos {
                                    b.error.get_or_insert(CapError::Limit("info blocks"));
                                } else {
                                    b.infos.push(CapInfo::default());
                                }
                            }
                            ["info", "area"] => {
                                if let Some(info) = b.infos.last_mut() {
                                    if info.areas.len() >= limits.max_areas {
                                        b.error.get_or_insert(CapError::Limit("area blocks"));
                                    } else {
                                        info.areas.push(CapArea {
                                            area_desc: String::new(),
                                            polygons: vec![],
                                            circles: vec![],
                                            geocodes: vec![],
                                            altitude: None,
                                            ceiling: None,
                                        });
                                    }
                                }
                            }
                            ["info", "eventCode"] | ["info", "parameter"] | ["info", "area", "geocode"] => {
                                pair_name = None;
                                pair_value = None;
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::Text(t) => {
                if in_alert.is_some() {
                    let s = t.unescape().map_err(|e| CapError::Xml(e.to_string()))?;
                    if text.len() + s.len() > limits.max_text_bytes {
                        return Err(CapError::Limit("text size"));
                    }
                    text.push_str(&s);
                }
            }
            Event::CData(c) => {
                if in_alert.is_some() {
                    let s = String::from_utf8_lossy(&c.into_inner()).into_owned();
                    if text.len() + s.len() > limits.max_text_bytes {
                        return Err(CapError::Limit("text size"));
                    }
                    text.push_str(&s);
                }
            }
            Event::End(_) => {
                let closing_alert = matches!(&in_alert, Some((d, _)) if *d == depth);
                if closing_alert {
                    let (_, b) = in_alert.take().expect("alert builder");
                    out.push(finish(b));
                    path.clear();
                } else if let Some((_, b)) = &mut in_alert {
                    let value = std::mem::take(&mut text);
                    let p: Vec<&str> = path.iter().map(String::as_str).collect();
                    assign(b, &p, value, &mut pair_name, &mut pair_value, limits);
                    path.pop();
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if out.is_empty() {
        return Err(CapError::NoAlert);
    }
    Ok(out)
}

fn some(v: String) -> Option<String> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn assign(
    b: &mut AlertBuilder,
    p: &[&str],
    v: String,
    pair_name: &mut Option<String>,
    pair_value: &mut Option<String>,
    limits: &CapLimits,
) {
    match p {
        ["identifier"] => b.identifier = some(v),
        ["sender"] => b.sender = some(v),
        ["sent"] => b.sent = some(v),
        ["status"] => b.status = some(v),
        ["msgType"] => b.msg_type = some(v),
        ["source"] => b.source = some(v),
        ["scope"] => b.scope = some(v),
        ["restriction"] => b.restriction = some(v),
        ["addresses"] => b.addresses = some(v),
        ["code"] => b.codes.push(v),
        ["note"] => b.note = some(v),
        ["references"] => b.references = some(v),
        ["incidents"] => b.incidents = some(v),
        // pair children
        ["info", "eventCode", "valueName"] | ["info", "parameter", "valueName"] | ["info", "area", "geocode", "valueName"] => {
            *pair_name = Some(v)
        }
        ["info", "eventCode", "value"] | ["info", "parameter", "value"] | ["info", "area", "geocode", "value"] => *pair_value = Some(v),
        ["info", "eventCode"] => {
            if let Some(i) = b.infos.last_mut() {
                i.event_codes.push((pair_name.take().unwrap_or_default(), pair_value.take().unwrap_or_default()));
            }
        }
        ["info", "parameter"] => {
            if let Some(i) = b.infos.last_mut() {
                i.parameters.push((pair_name.take().unwrap_or_default(), pair_value.take().unwrap_or_default()));
            }
        }
        ["info", "area", "geocode"] => {
            if let Some(a) = b.infos.last_mut().and_then(|i| i.areas.last_mut()) {
                a.geocodes.push((pair_name.take().unwrap_or_default(), pair_value.take().unwrap_or_default()));
            }
        }
        ["info", field] => {
            if let Some(i) = b.infos.last_mut() {
                match *field {
                    "language" => i.language = v,
                    "category" => i.categories.push(v),
                    "event" => i.event = v,
                    "responseType" => i.response_types.push(v),
                    "urgency" => i.urgency = v,
                    "severity" => i.severity = v,
                    "certainty" => i.certainty = v,
                    "audience" => i.audience = some(v),
                    "effective" => i.effective = some(v),
                    "onset" => i.onset = some(v),
                    "expires" => i.expires = some(v),
                    "senderName" => i.sender_name = some(v),
                    "headline" => i.headline = some(v),
                    "description" => i.description = some(v),
                    "instruction" => i.instruction = some(v),
                    "web" => i.web = some(v),
                    "contact" => i.contact = some(v),
                    _ => {}
                }
            }
        }
        ["info", "area", field] => {
            let Some(area) = b.infos.last_mut().and_then(|i| i.areas.last_mut()) else {
                return;
            };
            match *field {
                "areaDesc" => area.area_desc = v,
                "altitude" => area.altitude = some(v),
                "ceiling" => area.ceiling = some(v),
                "polygon" => {
                    b.polygons += 1;
                    if b.polygons > limits.max_polygons {
                        b.error.get_or_insert(CapError::Limit("polygon count"));
                        return;
                    }
                    match parse_polygon(&v) {
                        Ok(ring) => {
                            b.vertices += ring.len();
                            if b.vertices > limits.max_vertices {
                                b.error.get_or_insert(CapError::Limit("vertex count"));
                                return;
                            }
                            area.polygons.push(ring)
                        }
                        Err(e) => {
                            b.error.get_or_insert(e);
                        }
                    }
                }
                "circle" => match parse_circle(&v) {
                    Ok(c) => area.circles.push(c),
                    Err(e) => {
                        b.error.get_or_insert(e);
                    }
                },
                _ => {}
            }
        }
        _ => {}
    }
}

/// CAP pair order is `latitude,longitude` (spec §6.2). Converted once here.
pub fn parse_cap_point(s: &str) -> Result<GeoPoint, CapError> {
    let (lat, lon) = s.split_once(',').ok_or(CapError::Invalid { field: "coordinate", value: s.to_string() })?;
    let lat: f64 = lat.trim().parse().map_err(|_| CapError::Invalid { field: "latitude", value: lat.to_string() })?;
    let lon: f64 = lon.trim().parse().map_err(|_| CapError::Invalid { field: "longitude", value: lon.to_string() })?;
    GeoPoint::new(lat, lon).map_err(|e| CapError::Invalid { field: "coordinate", value: e.to_string() })
}

/// `<polygon>`: whitespace-delimited `lat,lon` pairs; at least four, first == last.
pub fn parse_polygon(s: &str) -> Result<Vec<GeoPoint>, CapError> {
    let pts = s.split_whitespace().map(parse_cap_point).collect::<Result<Vec<_>, _>>()?;
    if pts.len() < 4 {
        return Err(CapError::Invalid { field: "polygon", value: format!("{} points", pts.len()) });
    }
    if pts.first() != pts.last() {
        return Err(CapError::Invalid { field: "polygon", value: "ring not closed".into() });
    }
    Ok(pts)
}

/// `<circle>`: `lat,lon radius_km`.
pub fn parse_circle(s: &str) -> Result<CapCircle, CapError> {
    let mut it = s.split_whitespace();
    let (Some(c), Some(r), None) = (it.next(), it.next(), it.next()) else {
        return Err(CapError::Invalid { field: "circle", value: s.to_string() });
    };
    let center = parse_cap_point(c)?;
    let radius_km: f64 = r.parse().map_err(|_| CapError::Invalid { field: "circle radius", value: r.to_string() })?;
    if !radius_km.is_finite() || radius_km < 0.0 {
        return Err(CapError::Invalid { field: "circle radius", value: r.to_string() });
    }
    Ok(CapCircle { center, radius_km })
}

fn parse_references(s: &str) -> Result<Vec<CapRef>, CapError> {
    s.split_whitespace()
        .map(|t| {
            let mut parts = t.splitn(3, ',');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(a), Some(b), Some(c)) if !a.is_empty() && !b.is_empty() && !c.is_empty() => {
                    Ok(CapRef { sender: a.to_string(), identifier: b.to_string(), sent: c.to_string() })
                }
                _ => Err(CapError::Invalid { field: "references", value: t.to_string() }),
            }
        })
        .collect()
}

fn check_enum(field: &'static str, v: &str, allowed: &[&str]) -> Result<(), CapError> {
    if allowed.contains(&v) {
        Ok(())
    } else {
        Err(CapError::Invalid { field, value: v.to_string() })
    }
}

fn finish(b: AlertBuilder) -> Result<CapMessage, CapError> {
    if let Some(e) = b.error {
        return Err(e);
    }
    let identifier = b.identifier.ok_or(CapError::Missing("identifier"))?;
    let sender = b.sender.ok_or(CapError::Missing("sender"))?;
    let sent = b.sent.ok_or(CapError::Missing("sent"))?;
    let sent_ms = parse_ts_ms(&sent).ok_or(CapError::Invalid { field: "sent", value: sent.clone() })?;
    let status = b.status.ok_or(CapError::Missing("status"))?;
    check_enum("status", &status, STATUS)?;
    let msg_type = b.msg_type.ok_or(CapError::Missing("msgType"))?;
    check_enum("msgType", &msg_type, MSG_TYPES)?;
    let scope = b.scope.ok_or(CapError::Missing("scope"))?;
    check_enum("scope", &scope, SCOPES)?;
    let references = match &b.references {
        Some(r) => parse_references(r)?,
        None => vec![],
    };
    if matches!(msg_type.as_str(), "Update" | "Cancel") && references.is_empty() {
        return Err(CapError::Missing("references (required for Update/Cancel)"));
    }
    let mut infos = b.infos;
    for i in &mut infos {
        if i.language.is_empty() {
            i.language = "en-US".to_string(); // CAP 1.2 default
        }
        if i.categories.is_empty() {
            return Err(CapError::Missing("info.category"));
        }
        if i.event.is_empty() {
            return Err(CapError::Missing("info.event"));
        }
        check_enum("urgency", &i.urgency, URGENCY)?;
        check_enum("severity", &i.severity, SEVERITY)?;
        check_enum("certainty", &i.certainty, CERTAINTY)?;
        for (f, v) in [("effective", &i.effective), ("onset", &i.onset), ("expires", &i.expires)] {
            if let Some(v) = v {
                if parse_ts_ms(v).is_none() {
                    return Err(CapError::Invalid { field: f_static(f), value: v.clone() });
                }
            }
        }
        for a in &i.areas {
            if a.area_desc.is_empty() {
                return Err(CapError::Missing("area.areaDesc"));
            }
        }
    }
    Ok(CapMessage {
        identifier,
        sender,
        sent,
        sent_ms,
        status,
        msg_type,
        source: b.source,
        scope,
        restriction: b.restriction,
        addresses: b.addresses,
        codes: b.codes,
        note: b.note,
        references,
        incidents: b.incidents,
        infos,
        has_signature: b.has_signature,
    })
}

fn f_static(f: &str) -> &'static str {
    match f {
        "effective" => "effective",
        "onset" => "onset",
        _ => "expires",
    }
}
