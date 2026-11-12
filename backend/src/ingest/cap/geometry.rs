//! CAP area → canonical geometry parts (spec §6.3, §6.5).
//! An area is the union of its polygons, circles and geocodes; an area with
//! none of them becomes a text-only part. Nothing is invented.

use crate::domain::geometry::GeometryPart;
use crate::domain::message::CapInfo;

pub fn geometry_from_infos(infos: &[CapInfo]) -> Vec<GeometryPart> {
    let mut parts: Vec<GeometryPart> = Vec::new();
    let mut push = |p: GeometryPart| {
        // The same areas are commonly repeated per language <info>; keep one copy.
        if !parts.contains(&p) {
            parts.push(p);
        }
    };
    for info in infos {
        for area in &info.areas {
            let before = area.polygons.len() + area.circles.len() + area.geocodes.len();
            for ring in &area.polygons {
                push(GeometryPart::Polygon { exterior: ring.clone(), holes: vec![] });
            }
            for c in &area.circles {
                push(GeometryPart::Circle { center: c.center, radius_km: c.radius_km });
            }
            for (n, v) in &area.geocodes {
                push(GeometryPart::Geocode { value_name: n.clone(), value: v.clone() });
            }
            if before == 0 {
                push(GeometryPart::TextOnly { description: area.area_desc.clone() });
            }
        }
    }
    parts
}
