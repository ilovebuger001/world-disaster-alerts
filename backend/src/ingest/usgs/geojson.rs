//! USGS ComCat GeoJSON parsing (spec §4.4 GeoJSON rules, §6.2 order).
//! RFC 7946 order is `[longitude, latitude, optional altitude/depth]`.

use crate::domain::geometry::GeoPoint;
use crate::domain::message::UsgsEvent;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum GeoJsonError {
    #[error("malformed JSON: {0}")]
    Json(String),
    #[error("not a GeoJSON Feature/FeatureCollection")]
    NotGeoJson,
}

pub struct ParsedFeatures {
    pub events: Vec<UsgsEvent>,
    /// (feature id or index, reason) for skipped/unsupported features.
    pub skipped: Vec<(String, String)>,
}

/// GeoJSON position → GeoPoint (lon, lat order) plus optional third value.
pub fn geojson_position(v: &Value) -> Result<(GeoPoint, Option<f64>), String> {
    let arr = v.as_array().ok_or("position is not an array")?;
    if arr.len() < 2 {
        return Err("position needs at least 2 values".into());
    }
    let lon = arr[0].as_f64().ok_or("longitude not a number")?;
    let lat = arr[1].as_f64().ok_or("latitude not a number")?;
    let z = arr.get(2).and_then(Value::as_f64);
    let p = GeoPoint::new(lat, lon).map_err(|e| e.to_string())?;
    Ok((p, z))
}

/// Finds the first Point in a geometry (supports nested GeometryCollection).
/// Returns Ok(None) for null geometry; Err for unsupported/invalid geometry.
fn first_point(g: &Value) -> Result<Option<(GeoPoint, Option<f64>)>, String> {
    if g.is_null() {
        return Ok(None);
    }
    match g.get("type").and_then(Value::as_str) {
        Some("Point") => geojson_position(g.get("coordinates").ok_or("missing coordinates")?).map(Some),
        Some("GeometryCollection") => {
            let geoms = g.get("geometries").and_then(Value::as_array).ok_or("missing geometries")?;
            for sub in geoms {
                if let Some(p) = first_point(sub)? {
                    return Ok(Some(p));
                }
            }
            Ok(None)
        }
        Some(t) => Err(format!("unsupported geometry type {t} for earthquake adapter")),
        None => Err("geometry without type".into()),
    }
}

fn s(p: &Value, k: &str) -> Option<String> {
    p.get(k).and_then(Value::as_str).map(str::to_string)
}
fn f(p: &Value, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64)
}
fn i(p: &Value, k: &str) -> Option<i64> {
    p.get(k).and_then(Value::as_i64)
}

pub fn parse_usgs(body: &[u8]) -> Result<ParsedFeatures, GeoJsonError> {
    let root: Value = serde_json::from_slice(body).map_err(|e| GeoJsonError::Json(e.to_string()))?;
    let features: Vec<&Value> = match root.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => root.get("features").and_then(Value::as_array).ok_or(GeoJsonError::NotGeoJson)?.iter().collect(),
        Some("Feature") => vec![&root],
        _ => return Err(GeoJsonError::NotGeoJson),
    };
    let mut events = Vec::new();
    let mut skipped = Vec::new();
    for (idx, ft) in features.into_iter().enumerate() {
        let id = ft.get("id").and_then(Value::as_str).map(str::to_string);
        let label = id.clone().unwrap_or_else(|| format!("#{idx}"));
        let Some(id) = id else {
            skipped.push((label, "feature without id".into()));
            continue;
        };
        let null = Value::Null;
        let props = ft.get("properties").unwrap_or(&null);
        let props = if props.is_null() { &null } else { props };
        let point = match first_point(ft.get("geometry").unwrap_or(&null)) {
            Ok(p) => p,
            Err(reason) => {
                skipped.push((label, reason));
                continue;
            }
        };
        let (Some(time_ms), Some(updated_ms)) = (i(props, "time"), i(props, "updated")) else {
            skipped.push((label, "missing time/updated".into()));
            continue;
        };
        let ids = s(props, "ids").map(|v| v.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect()).unwrap_or_default();
        events.push(UsgsEvent {
            id,
            ids,
            point: point.map(|(p, _)| p),
            depth_km: point.and_then(|(_, z)| z),
            mag: f(props, "mag"),
            mag_type: s(props, "magType"),
            place: s(props, "place"),
            title: s(props, "title"),
            time_ms,
            updated_ms,
            url: s(props, "url"),
            detail: s(props, "detail"),
            status: s(props, "status"),
            event_type: s(props, "type"),
            tsunami: i(props, "tsunami"),
            alert: s(props, "alert"),
            mmi: f(props, "mmi"),
            cdi: f(props, "cdi"),
            felt: i(props, "felt"),
            sig: i(props, "sig"),
            net: s(props, "net"),
        });
    }
    Ok(ParsedFeatures { events, skipped })
}
