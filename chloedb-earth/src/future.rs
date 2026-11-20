//! Your place in 2071-2100: each place's climate now and later, from
//! CHELSA 2.1 (`tools/climate.py`, the table ea_climate), and where its
//! later climate can be found today.
//!
//! A place's climate is a vector of six standardised numbers (the year's
//! mean temperature, the hottest month's highs, the coldest month's lows,
//! the year's rain, how unevenly it falls and the summer's rain), once for
//! 1981-2010 (`clim`) and once for 2071-2100 (`clim70`). The places whose
//! `clim` is nearest a place's `clim70` are where its future climate is
//! today; those whose `clim70` is nearest its `clim` are where its climate
//! today will be.

use std::collections::HashMap;

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::api::ApiError;
use crate::pages::Ctx;
use crate::strata::{Row, RowExt, Strata};

type R<T> = Result<T, ApiError>;

/// Farther than this, in standard deviations of today's climates, no place
/// of the two regions has the climate today (tools/climate.py's NOVEL).
pub const NOVEL: f64 = 1.0;

/// The columns of a place's climate, without its vectors.
pub const COLUMNS: &str = "id, region, lat, lon, town, t_mean, t_mean_70, t_hot, t_hot_70, t_hot_70_lo, t_hot_70_hi, t_cold, t_cold_70, t_cold_70_lo, t_cold_70_hi, \
    rain, rain_70, rain_70_lo, rain_70_hi, rain_cv, rain_cv_70, rain_summer, rain_summer_70, kg, kg_70, kg_70_agree, analog, analog_region, analog_town, analog_km, analog_d, novel, moved";

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// A region's map: each place's change, as columns, and a few numbers
/// over all of them.
pub async fn layer(db: &Strata, region: &str) -> crate::strata::Result<Value> {
    let rows = db
        .scroll(
            "SELECT lat, lon, t_hot, t_hot_70, t_cold, t_cold_70, rain, rain_70, kg, kg_70, analog_km, analog_d, moved FROM ea_climate WHERE region = :r",
            json!({"r": region}),
            10_000,
        )
        .await?;
    let n = rows.len();
    let (mut lat, mut lon, mut hot, mut cold, mut rain, mut km, mut novel, mut kg) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let (mut dh, mut dc, mut dr) = (vec![], vec![], vec![]);
    let (mut changed, mut nov) = (0usize, 0usize);
    for r in &rows {
        lat.push((r.f("lat") * 1e4).round() / 1e4);
        lon.push((r.f("lon") * 1e4).round() / 1e4);
        let h = r.f("t_hot_70") - r.f("t_hot");
        let c = r.f("t_cold_70") - r.f("t_cold");
        let p = if r.f("rain") > 0.0 { 100.0 * (r.f("rain_70") / r.f("rain") - 1.0) } else { 0.0 };
        hot.push((h * 10.0).round() as i64);
        cold.push((c * 10.0).round() as i64);
        rain.push(p.round() as i64);
        km.push(r.f("analog_km").round() as i64);
        let is_novel = r.f("analog_d") > NOVEL;
        novel.push(is_novel as u8);
        nov += is_novel as usize;
        let (a, b) = (r.s("kg"), r.s("kg_70"));
        changed += (a != b && !b.is_empty()) as usize;
        kg.push(if a != b && !b.is_empty() { format!("{a}→{b}") } else { String::new() });
        dh.push(h);
        dc.push(c);
        dr.push(p);
    }
    Ok(json!({
        "region": region, "n": n, "lat": lat, "lon": lon, "hot": hot, "cold": cold, "rain": rain, "km": km, "novel": novel, "kg": kg,
        "median": {"hot": median(dh).map(|x| (x * 10.0).round() / 10.0), "cold": median(dc).map(|x| (x * 10.0).round() / 10.0), "rain": median(dr).map(|x| x.round()),
                   "km": median(km.iter().map(|x| *x as f64).collect())},
        "class_changed": changed, "novel_n": nov,
    }))
}

fn vector(r: &Row, k: &str) -> Option<Vec<f64>> {
    let v: Vec<f64> = r.get(k)?.as_array()?.iter().filter_map(Value::as_f64).collect();
    (v.len() == 6).then_some(v)
}

/// Each of the places found, with where it is from the place asked about.
fn near(c: &Ctx<'_>, from: &Row, rows: Vec<Row>) -> Vec<Value> {
    let index: HashMap<&str, &crate::places::Place> = c.places.all.iter().map(|p| (p.id.as_str(), p)).collect();
    let (la, lo) = (from.f("lat"), from.f("lon"));
    rows.into_iter()
        .filter(|r| r.s("id") != from.s("id"))
        .map(|r| {
            let id = r.s("id");
            let p = index.get(id.as_str());
            let mut v = Value::Object(r.clone());
            v["d"] = json!(((-r.f("s")) * 100.0).round() / 100.0);
            v["km"] = json!((crate::fires::km(la, lo, r.f("lat"), r.f("lon")) * 10.0).round() / 10.0);
            v["dir"] = json!(crate::watch::direction(la, lo, r.f("lat"), r.f("lon")));
            v["adm1"] = json!(p.map(|p| p.adm1.as_str()).unwrap_or(""));
            v["adm2"] = json!(p.map(|p| p.adm2.as_str()).unwrap_or(""));
            v["kind"] = json!(p.map(|p| p.kind.as_str()).unwrap_or(""));
            if let Some(o) = v.as_object_mut() {
                o.remove("s");
            }
            v
        })
        .collect()
}

/// A place's climate now and in 2071-2100, the places whose climate
/// today is like its later one, and those whose later climate is like
/// its today's: anywhere (`scope` "all") or in its own region.
pub async fn place(c: &Ctx<'_>, id: &str, scope: &str) -> R<Value> {
    let rows = c.rows("its climate, now and in 2071-2100", format!("SELECT {COLUMNS}, clim, clim70 FROM ea_climate WHERE id = :id"), json!({"id": id})).await?;
    let Some(row) = rows.into_iter().next() else { return Err(ApiError(StatusCode::NOT_FOUND, format!("no climate for place {id}"))) };
    let (Some(now), Some(later)) = (vector(&row, "clim"), vector(&row, "clim70")) else {
        return Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("place {id} has no climate vectors")));
    };
    let region = row.s("region");
    let wh = if scope == "region" { "WHERE region = :r" } else { "" };
    // One place a town, so that the list is not one valley's squares.
    let (like_later, goes_to) = tokio::join!(
        c.rows(
            "where its 2071-2100 climate is today",
            format!("SELECT id, region, town, lat, lon, kg, t_hot, t_cold, rain, rain_summer, score() AS s FROM ea_climate {wh} ORDER BY distance(clim, :v) ASC MAX 1 PER town LIMIT 10"),
            json!({"v": later, "r": region}),
        ),
        c.rows(
            "where its climate today will be in 2071-2100",
            format!("SELECT id, region, town, lat, lon, kg_70, t_hot_70, t_cold_70, rain_70, rain_summer_70, score() AS s FROM ea_climate {wh} ORDER BY distance(clim70, :v) ASC MAX 1 PER town LIMIT 10"),
            json!({"v": now, "r": region}),
        ),
    );
    let like_later = near(c, &row, like_later?);
    let goes_to = near(c, &row, goes_to?);
    let mut me = row.clone();
    me.remove("clim");
    me.remove("clim70");
    Ok(json!({"climate": me, "scope": if scope == "region" { "region" } else { "all" }, "novel": NOVEL, "like_later": like_later, "goes_to": goes_to}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_value() {
        assert_eq!(median(vec![3.0, 1.0, f64::NAN, 2.0]), Some(2.0));
        assert_eq!(median(vec![]), None);
    }
}
