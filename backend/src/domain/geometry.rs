//! Geometry model (spec §6.1–§6.4).
//!
//! One internal coordinate type with *named* fields. Source adapters convert
//! once at their boundary (CAP is `[lat,lon]`, GeoJSON is `[lon,lat]`).

use serde::{Deserialize, Serialize};

/// Internal coordinate. Never pass naked `(f64, f64)` tuples across adapters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat_deg: f64,
    pub lon_deg: f64,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GeoError {
    #[error("coordinate is not finite")]
    NotFinite,
    #[error("latitude {0} outside [-90, 90]")]
    LatitudeRange(f64),
    #[error("longitude {0} outside [-180, 180]")]
    LongitudeRange(f64),
    #[error("polygon needs at least 4 points (closed ring), got {0}")]
    PolygonTooShort(usize),
    #[error("circle radius must be finite and >= 0, got {0}")]
    BadRadius(f64),
}

impl GeoPoint {
    /// Validates finiteness and ranges (spec §6.1). No silent coercion.
    pub fn new(lat_deg: f64, lon_deg: f64) -> Result<Self, GeoError> {
        if !lat_deg.is_finite() || !lon_deg.is_finite() {
            return Err(GeoError::NotFinite);
        }
        if !(-90.0..=90.0).contains(&lat_deg) {
            return Err(GeoError::LatitudeRange(lat_deg));
        }
        if !(-180.0..=180.0).contains(&lon_deg) {
            return Err(GeoError::LongitudeRange(lon_deg));
        }
        Ok(Self { lat_deg, lon_deg })
    }
}

/// Axis-aligned box in lon/lat degrees. `x = longitude`, `y = latitude` (spec §6.4).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl BBox {
    pub fn contains(&self, p: &GeoPoint) -> bool {
        p.lon_deg >= self.min_lon && p.lon_deg <= self.max_lon && p.lat_deg >= self.min_lat && p.lat_deg <= self.max_lat
    }
    pub fn intersects(&self, o: &BBox) -> bool {
        self.min_lon <= o.max_lon && self.max_lon >= o.min_lon && self.min_lat <= o.max_lat && self.max_lat >= o.min_lat
    }
}

/// One affected-area part (spec §6.3). An alert holds a collection of these.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GeometryPart {
    Point {
        point: GeoPoint,
    },
    Circle {
        center: GeoPoint,
        radius_km: f64,
    },
    /// Exterior ring plus optional interior rings (holes). Rings are closed.
    Polygon {
        exterior: Vec<GeoPoint>,
        #[serde(default)]
        holes: Vec<Vec<GeoPoint>>,
    },
    /// Geocode-only area: code namespace and value preserved, no invented coordinate (§6.5).
    Geocode {
        value_name: String,
        value: String,
    },
    /// Text-only area where geometry is unavailable.
    TextOnly {
        description: String,
    },
}

const KM_PER_DEG_LAT: f64 = 111.32;
const EARTH_RADIUS_KM: f64 = 6371.0088;

/// True when consecutive ring vertices jump more than 180° in longitude,
/// i.e. the ring is drawn across the antimeridian.
pub fn ring_crosses_antimeridian(ring: &[GeoPoint]) -> bool {
    ring.windows(2).any(|w| (w[1].lon_deg - w[0].lon_deg).abs() > 180.0)
}

impl GeometryPart {
    pub fn validate(&self) -> Result<(), GeoError> {
        match self {
            GeometryPart::Polygon { exterior, holes } => {
                if exterior.len() < 4 {
                    return Err(GeoError::PolygonTooShort(exterior.len()));
                }
                for h in holes {
                    if h.len() < 4 {
                        return Err(GeoError::PolygonTooShort(h.len()));
                    }
                }
                Ok(())
            }
            GeometryPart::Circle { radius_km, .. } => {
                if !radius_km.is_finite() || *radius_km < 0.0 {
                    Err(GeoError::BadRadius(*radius_km))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Candidate envelopes for the R-tree. Antimeridian-crossing shapes are
    /// split into two boxes; geocode/text-only parts have none.
    pub fn index_boxes(&self) -> Vec<BBox> {
        match self {
            GeometryPart::Point { point } => {
                vec![BBox { min_lon: point.lon_deg, max_lon: point.lon_deg, min_lat: point.lat_deg, max_lat: point.lat_deg }]
            }
            GeometryPart::Circle { center, radius_km } => circle_boxes(center, *radius_km),
            GeometryPart::Polygon { exterior, .. } => ring_boxes(exterior),
            GeometryPart::Geocode { .. } | GeometryPart::TextOnly { .. } => vec![],
        }
    }

    /// Representative map point: source point, circle centre, or an interior
    /// point of the polygon (never a blind centroid, spec §6.3).
    pub fn representative_point(&self) -> Option<GeoPoint> {
        match self {
            GeometryPart::Point { point } => Some(*point),
            GeometryPart::Circle { center, .. } => Some(*center),
            GeometryPart::Polygon { exterior, holes } => polygon_interior_point(exterior, holes),
            _ => None,
        }
    }

    /// Exact intersection test against one (non-wrapped) viewport box.
    /// Circles use a geodesic distance check; polygons use planar lon/lat
    /// tests after bounding-box candidate selection (documented, spec §6.4).
    pub fn intersects_box(&self, b: &BBox) -> bool {
        match self {
            GeometryPart::Point { point } => b.contains(point),
            GeometryPart::Circle { center, radius_km } => {
                let nearest =
                    GeoPoint { lat_deg: center.lat_deg.clamp(b.min_lat, b.max_lat), lon_deg: center.lon_deg.clamp(b.min_lon, b.max_lon) };
                haversine_km(center, &nearest) <= *radius_km
            }
            GeometryPart::Polygon { exterior, holes } => {
                if ring_crosses_antimeridian(exterior) {
                    // Envelope test only for wrapped rings (each half is boxed separately).
                    return ring_boxes(exterior).iter().any(|e| e.intersects(b));
                }
                polygon_intersects_box(exterior, holes, b)
            }
            _ => false,
        }
    }
}

pub fn haversine_km(a: &GeoPoint, b: &GeoPoint) -> f64 {
    let (la1, la2) = (a.lat_deg.to_radians(), b.lat_deg.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.lon_deg - a.lon_deg).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.sqrt().min(1.0).asin()
}

fn circle_boxes(c: &GeoPoint, r_km: f64) -> Vec<BBox> {
    let dlat = r_km / KM_PER_DEG_LAT;
    let min_lat = (c.lat_deg - dlat).max(-90.0);
    let max_lat = (c.lat_deg + dlat).min(90.0);
    // Near the poles the circle covers every longitude.
    let cos = c.lat_deg.to_radians().cos();
    if max_lat >= 89.999 || min_lat <= -89.999 || cos < 1e-6 {
        return vec![BBox { min_lon: -180.0, max_lon: 180.0, min_lat, max_lat }];
    }
    let dlon = r_km / (KM_PER_DEG_LAT * cos);
    if dlon >= 180.0 {
        return vec![BBox { min_lon: -180.0, max_lon: 180.0, min_lat, max_lat }];
    }
    let (w, e) = (c.lon_deg - dlon, c.lon_deg + dlon);
    split_lon_range(w, e, min_lat, max_lat)
}

fn ring_boxes(ring: &[GeoPoint]) -> Vec<BBox> {
    if ring.is_empty() {
        return vec![];
    }
    let min_lat = ring.iter().map(|p| p.lat_deg).fold(f64::INFINITY, f64::min);
    let max_lat = ring.iter().map(|p| p.lat_deg).fold(f64::NEG_INFINITY, f64::max);
    if ring_crosses_antimeridian(ring) {
        // Shift western longitudes by +360 so the ring is continuous, then split.
        let shifted: Vec<f64> = ring.iter().map(|p| if p.lon_deg < 0.0 { p.lon_deg + 360.0 } else { p.lon_deg }).collect();
        let w = shifted.iter().cloned().fold(f64::INFINITY, f64::min);
        let e = shifted.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        return split_lon_range(w, e, min_lat, max_lat);
    }
    let min_lon = ring.iter().map(|p| p.lon_deg).fold(f64::INFINITY, f64::min);
    let max_lon = ring.iter().map(|p| p.lon_deg).fold(f64::NEG_INFINITY, f64::max);
    vec![BBox { min_lon, max_lon, min_lat, max_lat }]
}

/// Splits a continuous longitude range (which may exceed ±180) into boxes
/// inside [-180, 180].
fn split_lon_range(w: f64, e: f64, min_lat: f64, max_lat: f64) -> Vec<BBox> {
    if w < -180.0 {
        vec![BBox { min_lon: w + 360.0, max_lon: 180.0, min_lat, max_lat }, BBox { min_lon: -180.0, max_lon: e, min_lat, max_lat }]
    } else if e > 180.0 {
        vec![BBox { min_lon: w, max_lon: 180.0, min_lat, max_lat }, BBox { min_lon: -180.0, max_lon: e - 360.0, min_lat, max_lat }]
    } else {
        vec![BBox { min_lon: w, max_lon: e, min_lat, max_lat }]
    }
}

/// Planar even-odd point-in-ring test in lon/lat space.
pub fn point_in_ring(p: &GeoPoint, ring: &[GeoPoint]) -> bool {
    let (x, y) = (p.lon_deg, p.lat_deg);
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i].lon_deg, ring[i].lat_deg);
        let (xj, yj) = (ring[j].lon_deg, ring[j].lat_deg);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn point_in_polygon(p: &GeoPoint, exterior: &[GeoPoint], holes: &[Vec<GeoPoint>]) -> bool {
    point_in_ring(p, exterior) && !holes.iter().any(|h| point_in_ring(p, h))
}

/// Interior point via a horizontal scanline through the middle latitude:
/// the midpoint of the widest inside span is inside the polygon (incl. concave).
pub fn polygon_interior_point(exterior: &[GeoPoint], holes: &[Vec<GeoPoint>]) -> Option<GeoPoint> {
    if exterior.len() < 3 {
        return None;
    }
    let min_lat = exterior.iter().map(|p| p.lat_deg).fold(f64::INFINITY, f64::min);
    let max_lat = exterior.iter().map(|p| p.lat_deg).fold(f64::NEG_INFINITY, f64::max);
    // Try a few scanlines in case the middle one hits a vertex or a hole.
    for frac in [0.5, 0.37, 0.63, 0.25, 0.75, 0.11, 0.89] {
        let y = min_lat + (max_lat - min_lat) * frac;
        let mut xs: Vec<f64> = Vec::new();
        let rings = std::iter::once(exterior).chain(holes.iter().map(|h| h.as_slice()));
        for ring in rings {
            let n = ring.len();
            for i in 0..n {
                let a = ring[i];
                let b = ring[(i + 1) % n];
                if (a.lat_deg > y) != (b.lat_deg > y) {
                    let t = (y - a.lat_deg) / (b.lat_deg - a.lat_deg);
                    xs.push(a.lon_deg + t * (b.lon_deg - a.lon_deg));
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut best: Option<(f64, f64)> = None;
        for pair in xs.chunks(2) {
            if let [x0, x1] = pair {
                let w = x1 - x0;
                if w > 0.0 && best.map_or(true, |(bw, _)| w > bw) {
                    best = Some((w, (x0 + x1) / 2.0));
                }
            }
        }
        if let Some((_, x)) = best {
            let p = GeoPoint { lat_deg: y, lon_deg: x };
            if point_in_polygon(&p, exterior, holes) {
                return Some(p);
            }
        }
    }
    None
}

fn segments_intersect(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> bool {
    fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    }
    fn on_seg(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
        c.0 >= a.0.min(b.0) && c.0 <= a.0.max(b.0) && c.1 >= a.1.min(b.1) && c.1 <= a.1.max(b.1)
    }
    let (d1, d2) = (orient(q1, q2, p1), orient(q1, q2, p2));
    let (d3, d4) = (orient(p1, p2, q1), orient(p1, p2, q2));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && on_seg(q1, q2, p1))
        || (d2 == 0.0 && on_seg(q1, q2, p2))
        || (d3 == 0.0 && on_seg(p1, p2, q1))
        || (d4 == 0.0 && on_seg(p1, p2, q2))
}

fn polygon_intersects_box(exterior: &[GeoPoint], holes: &[Vec<GeoPoint>], b: &BBox) -> bool {
    // 1. any exterior vertex inside the box
    if exterior.iter().any(|p| b.contains(p)) {
        return true;
    }
    // 2. any box corner inside the polygon
    let corners = [
        GeoPoint { lat_deg: b.min_lat, lon_deg: b.min_lon },
        GeoPoint { lat_deg: b.min_lat, lon_deg: b.max_lon },
        GeoPoint { lat_deg: b.max_lat, lon_deg: b.max_lon },
        GeoPoint { lat_deg: b.max_lat, lon_deg: b.min_lon },
    ];
    if corners.iter().any(|c| point_in_polygon(c, exterior, holes)) {
        return true;
    }
    // 3. any edge crossing
    let box_edges = [
        ((b.min_lon, b.min_lat), (b.max_lon, b.min_lat)),
        ((b.max_lon, b.min_lat), (b.max_lon, b.max_lat)),
        ((b.max_lon, b.max_lat), (b.min_lon, b.max_lat)),
        ((b.min_lon, b.max_lat), (b.min_lon, b.min_lat)),
    ];
    let n = exterior.len();
    for i in 0..n {
        let a = exterior[i];
        let c = exterior[(i + 1) % n];
        for (q1, q2) in box_edges {
            if segments_intersect((a.lon_deg, a.lat_deg), (c.lon_deg, c.lat_deg), q1, q2) {
                return true;
            }
        }
    }
    false
}
