//! Real names for the fires GOES saw: a fire takes the name of the CAL
//! FIRE perimeter it matches, the same year, an alarm date within three
//! days before and two days after the GOES start, and its centre in the
//! outline (or within 3 km of it, since GOES pixels are 2 km at best).
//! Perimeters come out weeks after a fire, so a fire without a match stays
//! as it was and is tried again.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::strata::{Row, RowExt, Strata};

fn day(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}

/// The perimeter a fire matches among `cands` (rows of ea_perimeters),
/// the largest when several do.
pub fn pick<'a>(lat: f64, lon: f64, start: i64, cands: &'a [Row]) -> Option<&'a Row> {
    let mut best: Option<(&Row, f64)> = None;
    for p in cands {
        let Some(alarm) = day(&p.s("alarm")) else { continue };
        if start < alarm - 2 * 86400 || start > alarm + 3 * 86400 {
            continue;
        }
        if lat < p.f("s") - 0.03 || lat > p.f("n") + 0.03 || lon < p.f("w") - 0.03 || lon > p.f("e") + 0.03 {
            continue;
        }
        let polys = crate::geo::parse(&p.s("shape"));
        let near = crate::geo::contains(&polys, lat, lon) || crate::geo::nearest_vertex(&polys, lat, lon).is_some_and(|v| v.0 <= 3.0);
        if near && best.is_none_or(|b| p.f("acres") > b.1) {
            best = Some((p, p.f("acres")));
        }
    }
    best.map(|b| b.0)
}

/// Names the fires of California that have none yet. Returns how many.
pub async fn run(db: &Strata) -> Result<usize> {
    let fires = db
        .scroll("SELECT id, lat, lon, start, perimeter FROM ea_fires WHERE region = 'california'", json!({}), 10_000)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let mut ops: Vec<Value> = Vec::new();
    let mut named = 0;
    for f in &fires {
        if !f.s("perimeter").is_empty() {
            continue;
        }
        let Some(start) = day(&f.s("start")) else { continue };
        let (lat, lon) = (f.f("lat"), f.f("lon"));
        let a = chrono::DateTime::from_timestamp(start - 4 * 86400, 0).map(|d| d.to_rfc3339()).unwrap_or_default();
        let b = chrono::DateTime::from_timestamp(start + 3 * 86400, 0).map(|d| d.to_rfc3339()).unwrap_or_default();
        // The outlines whose box holds the fire's centre, give or take
        // 0.03 degrees (GOES's pixels are 2 km at best).
        let cands = match db
            .query(
                "SELECT id, name, alarm, acres, s, n, w, e, shape FROM ea_perimeters WHERE alarm >= :a AND alarm <= :b AND s <= :n AND n >= :s AND w <= :e AND e >= :w LIMIT 50",
                json!({"a": a, "b": b, "s": lat - 0.03, "n": lat + 0.03, "w": lon - 0.03, "e": lon + 0.03}),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(fire = %f.s("id"), error = %e, "perimeters for a fire");
                continue;
            }
        };
        let Some(p) = pick(lat, lon, start, &cands) else { continue };
        ops.push(json!({"op": "set", "table": "ea_fires", "key": f.s("id"), "field": "perimeter", "value": p.s("id")}));
        if !p.s("name").is_empty() {
            ops.push(json!({"op": "set", "table": "ea_fires", "key": f.s("id"), "field": "name", "value": p.s("name")}));
        }
        named += 1;
        // Written as they come, so that a run cut short keeps its names.
        if ops.len() >= 400 {
            db.write(std::mem::take(&mut ops)).await.map_err(|e| anyhow!("{e}"))?;
        }
    }
    db.write(ops).await.map_err(|e| anyhow!("{e}"))?;
    Ok(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn per(name: &str, alarm: &str, acres: f64) -> Row {
        let v = json!({"id": name, "name": name, "alarm": alarm, "acres": acres, "s": 39.0, "n": 40.0, "w": -122.0, "e": -121.0,
            "shape": r#"{"type":"Polygon","coordinates":[[[-122,39],[-122,40],[-121,40],[-121,39],[-122,39]]]}"#});
        v.as_object().unwrap().clone().into_iter().collect()
    }

    #[test]
    fn a_fire_takes_the_name_of_the_perimeter_it_started_in() {
        let start = day("2024-07-24T21:00:00Z").unwrap();
        let c = vec![per("Small", "2024-07-24T00:00:00Z", 10.0), per("Park", "2024-07-24T00:00:00Z", 429_603.0), per("Old", "2023-07-24T00:00:00Z", 9e9), per("Later", "2024-08-30T00:00:00Z", 9e9)];
        assert_eq!(pick(39.8, -121.6, start, &c).map(|p| p.s("name")), Some("Park".into()));
        assert!(pick(41.0, -121.6, start, &c).is_none(), "outside every outline");
    }
}
