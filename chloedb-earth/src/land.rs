//! The Land finder: places to build on (a solar farm, most often), found
//! by one statement that keeps the places within the limits asked for
//! (slope, the grid, a road, protected land, towns) and ranks them by how
//! much they look like the solar farms already built in the region, like
//! words, or by how near the grid is.
//!
//! The columns come from `tools/land.py`: OpenStreetMap's power lines,
//! substations, main roads and solar farms, and PAD-US's protected land
//! in California (OpenStreetMap's in Chile).

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::api::ApiError;
use crate::pages::{Ctx, PLACE_SMALL};
use crate::strata::{RowExt, Strata};

type R<T> = Result<T, ApiError>;

/// What a place's land row shows, beside the place's own columns.
pub const LAND: &str = "slope, elev, sub_km, sub_kv, sub_name, line_km, line_kv, road_km, road_name, solar, solar_km, solar_name, protected, protected_strict, protected_name";

/// The mean look of a region's solar farms: the places at least a third
/// covered by one (none when the region has fewer than five).
pub async fn solar_look(db: &Strata, region: &str) -> crate::strata::Result<Option<Vec<f32>>> {
    let rows = db.query("SELECT look FROM ea_places WHERE region = :r AND solar >= 0.33 LIMIT 500", json!({"r": region})).await?;
    let looks: Vec<Vec<f32>> = rows
        .iter()
        .filter_map(|r| r.get("look").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect::<Vec<f32>>()))
        .filter(|v| !v.is_empty())
        .collect();
    if looks.len() < 5 {
        return Ok(None);
    }
    let mut m = vec![0f32; looks[0].len()];
    for v in &looks {
        for (a, b) in m.iter_mut().zip(v) {
            *a += b;
        }
    }
    let n = m.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    Ok(Some(m.into_iter().map(|x| x / n).collect()))
}

/// The limits and the ranking asked for.
#[derive(serde::Deserialize, Default, Clone)]
pub struct Ask {
    #[serde(default)]
    pub region: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Within this many km of the point.
    pub km: Option<f64>,
    /// Degrees, at most.
    pub slope: Option<f64>,
    /// Km to a substation, at most.
    pub sub: Option<f64>,
    /// The substation's kV, at least.
    pub kv: Option<f64>,
    /// Km to a main road, at most.
    pub road: Option<f64>,
    /// "none" (under a tenth protected), "strict" (under a tenth strictly) or "any".
    #[serde(default)]
    pub protected: String,
    /// Per cent built, at most.
    pub built: Option<f64>,
    /// "solar" (like its solar farms), "grid" (nearest a substation) or "words" (like `q`).
    #[serde(default)]
    pub rank: String,
    #[serde(default)]
    pub q: String,
}

fn num(x: Option<f64>, lo: f64, hi: f64) -> Option<f64> {
    x.filter(|v| v.is_finite()).map(|v| v.clamp(lo, hi))
}

/// The WHERE of the limits, and its parameters.
pub fn limits(region: &str, a: &Ask) -> (String, Value) {
    // Not a solar farm already, nor a lake or snow.
    let mut wh = vec!["region = :r".to_string(), "solar = 0".to_string(), "water < 20".to_string(), "snow < 20".to_string()];
    let mut p = json!({"r": region});
    let mut add = |cond: &str, k: &str, v: Value| {
        wh.push(cond.to_string());
        p[k] = v;
    };
    if let Some(v) = num(a.slope, 0.0, 90.0) {
        add("slope <= :slope", "slope", json!(v));
    }
    if let Some(v) = num(a.sub, 0.0, 500.0) {
        add("sub_km <= :sub", "sub", json!(v));
    }
    if let Some(v) = num(a.kv, 0.0, 1000.0) {
        add("sub_kv >= :kv", "kv", json!(v));
    }
    if let Some(v) = num(a.road, 0.0, 500.0) {
        add("road_km <= :road", "road", json!(v));
    }
    if let Some(v) = num(a.built, 0.0, 100.0) {
        add("built <= :built", "built", json!(v));
    }
    match a.protected.as_str() {
        "any" => {}
        "strict" => add("protected_strict < :prot", "prot", json!(0.1)),
        _ => add("protected < :prot", "prot", json!(0.1)),
    }
    if let (Some(lat), Some(lon), Some(km)) = (a.lat, a.lon, num(a.km, 1.0, 400.0)) {
        let (s, n, w, e) = crate::watch::bbox(lat, lon, km);
        for (k, v) in [("s", s), ("n", n), ("w", w), ("e", e)] {
            p[k] = json!(v);
        }
        wh.push("lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e".into());
    }
    (wh.join(" AND "), p)
}

pub async fn find(c: &Ctx<'_>, region: &str, a: &Ask, solar: Option<&[f32]>) -> R<Value> {
    let (wh, mut p) = limits(region, a);
    let rank = match a.rank.as_str() {
        "grid" => "grid",
        "words" if !a.q.trim().is_empty() => "words",
        _ if solar.is_some() => "solar",
        _ => "grid",
    };
    let order = match rank {
        "grid" => "ORDER BY sub_km ASC".to_string(),
        _ => {
            p["v"] = if rank == "words" { c.words(a.q.trim()) } else { json!(solar) };
            "ORDER BY similarity(look, :v) DESC".to_string()
        }
    };
    let score = if rank == "grid" { "" } else { ", score() AS score" };
    let circle = match (a.lat, a.lon, num(a.km, 1.0, 400.0)) {
        (Some(lat), Some(lon), Some(km)) => Some((lat, lon, km)),
        _ => None,
    };
    // Around a point the box is cut to its circle afterwards, so more are read
    // and counted one by one.
    let (limit, count_sql) = match circle {
        Some(_) => (240, format!("SELECT lat, lon FROM ea_places WHERE {wh} LIMIT 10000")),
        None => (60, format!("SELECT COUNT(*) AS n FROM ea_places WHERE {wh}")),
    };
    let (rows, count, farms) = tokio::join!(
        c.rows("the places within the limits, best first", format!("SELECT {PLACE_SMALL}, {LAND}{score} FROM ea_places WHERE {wh} {order} LIMIT {limit}"), p.clone()),
        c.soft("how many places are within the limits", count_sql, p.clone()),
        c.soft("the solar farms built in the region", "SELECT id, lat, lon, solar, solar_name FROM ea_places WHERE region = :r AND solar > 0 LIMIT 3000", json!({"r": region})),
    );
    let mut rows = rows?;
    // Within the circle, not only its box.
    // A box's 10,000 places (the most a query returns) leaves some out.
    let more = circle.is_some() && count.len() >= 10_000;
    let within = match circle {
        Some((lat, lon, km)) => {
            rows.retain(|r| crate::fires::km(lat, lon, r.f("lat"), r.f("lon")) <= km);
            rows.truncate(60);
            for r in rows.iter_mut() {
                let d = crate::fires::km(lat, lon, r.f("lat"), r.f("lon"));
                r.insert("km".into(), json!((d * 10.0).round() / 10.0));
                r.insert("dir".into(), json!(crate::watch::direction(lat, lon, r.f("lat"), r.f("lon"))));
            }
            count.iter().filter(|r| crate::fires::km(lat, lon, r.f("lat"), r.f("lon")) <= km).count() as i64
        }
        None => count.first().map(|r| r.i("n")).unwrap_or(0),
    };
    Ok(json!({"region": region, "rank": rank, "places": rows, "within": within, "more": more,
              "farms": farms, "solar_look": solar.is_some()}))
}

/// The cache of each region's solar farms' look.
pub type Looks = tokio::sync::RwLock<HashMap<&'static str, Option<std::sync::Arc<Vec<f32>>>>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_become_one_where() {
        let a = Ask { slope: Some(5.0), sub: Some(10.0), protected: "strict".into(), lat: Some(35.0), lon: Some(-119.0), km: Some(50.0), ..Default::default() };
        let (wh, p) = limits("california", &a);
        assert!(wh.starts_with("region = :r AND solar = 0 AND water < 20 AND snow < 20 AND slope <= :slope AND sub_km <= :sub AND protected_strict < :prot AND lat >= :s"), "{wh}");
        assert_eq!(p["slope"], 5.0);
        let (wh, _) = limits("chile", &Ask::default());
        assert_eq!(wh, "region = :r AND solar = 0 AND water < 20 AND snow < 20 AND protected < :prot");
        let (_, p) = limits("chile", &Ask { slope: Some(-4.0), ..Default::default() });
        assert_eq!(p["slope"], 0.0, "clamped");
    }
}
