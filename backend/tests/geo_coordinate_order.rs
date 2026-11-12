//! Spec §15.2 geography tests (coordinate order, ranges, geometry helpers).
use wda::domain::geometry::*;
use wda::ingest::cap::parser::{parse_cap_point, parse_circle};
use wda::ingest::usgs::geojson::geojson_position;

#[test]
fn cap_pairs_are_lat_lon() {
    let p = parse_cap_point("13.75,100.5").unwrap();
    assert_eq!(p, GeoPoint { lat_deg: 13.75, lon_deg: 100.5 });
}

#[test]
fn geojson_positions_are_lon_lat() {
    let v = serde_json::json!([100.5, 13.75, 10.0]);
    let (p, z) = geojson_position(&v).unwrap();
    assert_eq!(p, GeoPoint { lat_deg: 13.75, lon_deg: 100.5 });
    assert_eq!(z, Some(10.0));
}

#[test]
fn southern_western_negative_coordinates() {
    let p = parse_cap_point("-33.4,-70.5").unwrap();
    assert_eq!((p.lat_deg, p.lon_deg), (-33.4, -70.5));
    let (g, _) = geojson_position(&serde_json::json!([-70.5, -33.4])).unwrap();
    assert_eq!(g, p);
}

#[test]
fn boundaries_and_non_finite_values() {
    assert!(GeoPoint::new(90.0, 180.0).is_ok());
    assert!(GeoPoint::new(-90.0, -180.0).is_ok());
    assert_eq!(GeoPoint::new(90.0001, 0.0), Err(GeoError::LatitudeRange(90.0001)));
    assert_eq!(GeoPoint::new(0.0, 180.5), Err(GeoError::LongitudeRange(180.5)));
    assert_eq!(GeoPoint::new(f64::NAN, 0.0), Err(GeoError::NotFinite));
    assert!(parse_cap_point("100.5,13.75").is_err(), "swapped CAP order is out of range");
}

#[test]
fn concave_polygon_interior_point_is_inside() {
    // U shape: centroid lies in the notch (outside)
    let ring: Vec<GeoPoint> = [(0.0, 0.0), (0.0, 3.0), (3.0, 3.0), (3.0, 0.0), (2.0, 0.0), (2.0, 2.0), (1.0, 2.0), (1.0, 0.0), (0.0, 0.0)]
        .iter()
        .map(|(lon, lat)| GeoPoint { lat_deg: *lat, lon_deg: *lon })
        .collect();
    let p = polygon_interior_point(&ring, &[]).unwrap();
    assert!(point_in_polygon(&p, &ring, &[]));
}

#[test]
fn polygon_with_hole() {
    let sq = |a: f64, b: f64| -> Vec<GeoPoint> {
        [(a, a), (b, a), (b, b), (a, b), (a, a)].iter().map(|(x, y)| GeoPoint { lat_deg: *y, lon_deg: *x }).collect()
    };
    let outer = sq(0.0, 10.0);
    let hole = sq(4.0, 6.0);
    assert!(!point_in_polygon(&GeoPoint { lat_deg: 5.0, lon_deg: 5.0 }, &outer, &[hole.clone()]));
    let ip = polygon_interior_point(&outer, &[hole.clone()]).unwrap();
    assert!(point_in_polygon(&ip, &outer, &[hole]));
}

#[test]
fn circle_radius_and_bbox() {
    let c = parse_circle("0,0 111.32").unwrap();
    let part = GeometryPart::Circle { center: c.center, radius_km: c.radius_km };
    let b = part.index_boxes();
    assert_eq!(b.len(), 1);
    assert!((b[0].max_lat - 1.0).abs() < 1e-9);
    assert!((b[0].min_lon + 1.0).abs() < 1e-9);
    // geodesic final check: a box 0.9° away is inside, 1.1° away is not
    let near = BBox { min_lon: 0.9, max_lon: 2.0, min_lat: -0.1, max_lat: 0.1 };
    let far = BBox { min_lon: 1.1, max_lon: 2.0, min_lat: -0.1, max_lat: 0.1 };
    assert!(part.intersects_box(&near));
    assert!(!part.intersects_box(&far));
}

#[test]
fn near_pole_circle_covers_all_longitudes() {
    let part = GeometryPart::Circle { center: GeoPoint { lat_deg: 89.5, lon_deg: 10.0 }, radius_km: 100.0 };
    let b = part.index_boxes();
    assert_eq!((b[0].min_lon, b[0].max_lon), (-180.0, 180.0));
}

#[test]
fn text_and_geocode_only_have_no_index_box_or_point() {
    let t = GeometryPart::TextOnly { description: "x".into() };
    let g = GeometryPart::Geocode { value_name: "FIPS6".into(), value: "006113".into() };
    assert!(t.index_boxes().is_empty() && g.index_boxes().is_empty());
    assert!(t.representative_point().is_none() && g.representative_point().is_none());
}

#[test]
fn polygon_box_intersection_cases() {
    let tri: Vec<GeoPoint> =
        [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (0.0, 0.0)].iter().map(|(x, y)| GeoPoint { lat_deg: *y, lon_deg: *x }).collect();
    let part = GeometryPart::Polygon { exterior: tri, holes: vec![] };
    // box fully inside polygon
    assert!(part.intersects_box(&BBox { min_lon: 1.0, max_lon: 2.0, min_lat: 1.0, max_lat: 2.0 }));
    // box crossing an edge
    assert!(part.intersects_box(&BBox { min_lon: 4.0, max_lon: 6.0, min_lat: 4.0, max_lat: 6.0 }));
    // box in the bbox but outside the triangle
    assert!(!part.intersects_box(&BBox { min_lon: 8.0, max_lon: 9.0, min_lat: 8.0, max_lat: 9.0 }));
}
