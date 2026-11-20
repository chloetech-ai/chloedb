//! Outlines of fires (GeoJSON Polygon and MultiPolygon, as the perimeters
//! table keeps them): is a point inside one, how far is it, in which
//! direction.

use serde_json::Value;

/// A polygon: its outer ring, then its holes, as (lon, lat).
pub type Poly = Vec<Vec<(f64, f64)>>;

/// The polygons of a GeoJSON geometry text. A GeometryCollection (what
/// making an outline valid leaves when it also had stray lines) gives the
/// polygons inside it.
pub fn parse(shape: &str) -> Vec<Poly> {
    match serde_json::from_str::<Value>(shape) {
        Ok(v) => polygons(&v),
        Err(_) => Vec::new(),
    }
}

fn polygons(v: &Value) -> Vec<Poly> {
    let ring = |r: &Value| -> Vec<(f64, f64)> {
        r.as_array().map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect()).unwrap_or_default()
    };
    let poly = |p: &Value| -> Poly { p.as_array().map(|rs| rs.iter().map(ring).collect()).unwrap_or_default() };
    match v.get("type").and_then(Value::as_str) {
        Some("Polygon") => vec![poly(&v["coordinates"])],
        Some("MultiPolygon") => v["coordinates"].as_array().map(|ps| ps.iter().map(poly).collect()).unwrap_or_default(),
        Some("GeometryCollection") => v["geometries"].as_array().map(|gs| gs.iter().flat_map(polygons).collect()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn in_ring(ring: &[(f64, f64)], lon: f64, lat: f64) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether the point is inside one of the polygons (outside its holes).
pub fn contains(polys: &[Poly], lat: f64, lon: f64) -> bool {
    polys.iter().any(|p| p.first().is_some_and(|o| in_ring(o, lon, lat)) && !p.iter().skip(1).any(|h| in_ring(h, lon, lat)))
}

/// The nearest vertex of the outlines to a point: its distance (km) and
/// its position, (lat, lon).
pub fn nearest_vertex(polys: &[Poly], lat: f64, lon: f64) -> Option<(f64, f64, f64)> {
    polys
        .iter()
        .flat_map(|p| p.first())
        .flatten()
        .map(|&(x, y)| (crate::fires::km(lat, lon, y, x), y, x))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// The compass point (N, NE, ...) of the way from one point to another.
pub fn compass(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> &'static str {
    let dy = lat2 - lat1;
    let dx = (lon2 - lon1) * ((lat1 + lat2) / 2.0).to_radians().cos();
    let deg = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][((deg + 22.5) / 45.0) as usize % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r#"{"type":"Polygon","coordinates":[[[0,0],[0,2],[2,2],[2,0],[0,0]],[[0.8,0.8],[0.8,1.2],[1.2,1.2],[1.2,0.8],[0.8,0.8]]]}"#;

    #[test]
    fn points_in_and_out_of_an_outline() {
        let p = parse(SQUARE);
        assert!(contains(&p, 0.3, 0.3));
        assert!(!contains(&p, 1.0, 1.0), "a hole");
        assert!(!contains(&p, 3.0, 3.0));
        assert!(parse("not json").is_empty());
        let gc = format!(r#"{{"type":"GeometryCollection","geometries":[{{"type":"LineString","coordinates":[[0,0],[1,1]]}},{SQUARE}]}}"#);
        assert!(contains(&parse(&gc), 0.3, 0.3), "the polygons inside a collection");
    }

    #[test]
    fn distance_and_direction() {
        let p = parse(SQUARE);
        let (d, lat, lon) = nearest_vertex(&p, 2.0, 5.0).unwrap();
        assert!((lat, lon) == (2.0, 2.0) && d > 300.0);
        assert_eq!(compass(35.0, -120.0, 35.0, -119.0), "E");
        assert_eq!(compass(35.0, -120.0, 34.0, -120.0), "S");
    }
}
