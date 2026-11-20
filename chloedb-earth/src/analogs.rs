//! Fires like this one: a fire's first hours (its power, its pixels and
//! how far it spread, every quarter hour, and the ground and weather where
//! it started) as a vector, and the fires whose first hours were nearest,
//! by `ORDER BY distance(shape2, :v) ASC`. What those fires became (the
//! area GOES saw burn, how long they burned) is this fire's outlook: the
//! middle of their sizes, and the range their middle half spans.
//!
//! A fire is compared in the longest window it has lived through: its
//! first hour, two hours or six ([`crate::fires::WINDOWS`]), against the
//! same window of fires that are out. Only fires whose end was known at
//! the moment count, so the backtest ([`backtest`]) sees no future.

use anyhow::Result;
use serde_json::{json, Value};

use crate::fires::{window_for, WINDOWS};
use crate::strata::{Row, RowExt, Strata, Trace};

pub const K: usize = 20;

pub const ANALOG_COLS: &str = "id, region, name, town, adm2, lat, lon, start, last, hours, area_km2, peak_mw, peak_px, spread_km, people5, trees, shrubs, slope, climate";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outlook {
    pub window: String,
    pub hours: f64,
    pub n: usize,
    /// The median, 25th and 75th percentile of the analogs' areas (km²).
    pub median: f64,
    pub lo: f64,
    pub hi: f64,
    /// The same of how many hours they burned.
    pub hours_median: f64,
    /// The share of them that grew past 10 km².
    pub over10: f64,
}

pub fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let x = q * (sorted.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x - x.floor());
    let a = sorted[i];
    let b = sorted[(i + 1).min(sorted.len() - 1)];
    a + (b - a) * f
}

pub fn outlook_of(window: &str, hours: f64, rows: &[Row]) -> Outlook {
    let mut a: Vec<f64> = rows.iter().map(|r| r.f("area_km2")).collect();
    a.sort_by(f64::total_cmp);
    let mut h: Vec<f64> = rows.iter().map(|r| r.f("hours")).collect();
    h.sort_by(f64::total_cmp);
    Outlook {
        window: window.to_string(),
        hours,
        n: rows.len(),
        median: quantile(&a, 0.5),
        lo: quantile(&a, 0.25),
        hi: quantile(&a, 0.75),
        hours_median: quantile(&h, 0.5),
        over10: if a.is_empty() { 0.0 } else { a.iter().filter(|x| **x > 10.0).count() as f64 / a.len() as f64 },
    }
}

pub fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
}

/// The `k` fires whose first `window` was nearest to `v`, of those out by
/// `known` (Unix seconds) and, unless `anywhere`, in `region`.
#[allow(clippy::too_many_arguments)]
pub async fn nearest(db: &Strata, trace: Option<&Trace>, window: &str, v: &[f32], not: &str, region: &str, anywhere: bool, known: i64, k: usize) -> crate::strata::Result<Vec<Row>> {
    let wh = if anywhere { "" } else { "AND region = :r " };
    let sql = format!("SELECT {ANALOG_COLS}, score() AS score FROM ea_fires WHERE state = 'out' AND seen_start = true AND id <> :id {wh}AND last <= :known ORDER BY distance({window}, :v) ASC LIMIT {k}");
    let p = json!({"v": v.iter().map(|x| ((*x as f64) * 1e5).round() / 1e5).collect::<Vec<_>>(), "id": not, "r": region, "known": iso(known)});
    match trace {
        Some(t) => db.traced_within(t, &format!("the {k} fires whose first {} were nearest", window_words(window)), &sql, p, Some(30_000)).await,
        None => db.query_within(&sql, p, Some(30_000)).await,
    }
}

pub fn window_words(w: &str) -> &'static str {
    match w {
        "shape1" => "hour",
        "shape2" => "two hours",
        _ => "six hours",
    }
}

/// A row's vector of `window` (`[f32]` from the JSON list).
pub fn vector(r: &Row, window: &str) -> Vec<f32> {
    r.get(window).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default()
}

/// The outlook of one burning fire whose first hours are `row`'s.
pub async fn outlook(db: &Strata, trace: Option<&Trace>, row: &Row, now: i64) -> Option<(Outlook, Vec<Row>)> {
    let start = chrono::DateTime::parse_from_rfc3339(&row.s("start")).ok()?.timestamp();
    let age = (now - start) as f64 / 3600.0;
    let (window, hours, _, _) = window_for(age)?;
    let v = vector(row, window);
    if v.is_empty() {
        return None;
    }
    let rows = nearest(db, trace, window, &v, &row.s("id"), &row.s("region"), false, now, K).await.ok()?;
    if rows.len() < 5 {
        return None;
    }
    Some((outlook_of(window, hours, &rows), rows))
}

// ------------------------------------------------------------- backtest

/// How well the analogs foretold the fires that came after them: every
/// fire watched from its start, in each window it lived through, matched
/// against the fires out before that moment, its real area compared with
/// the analogs' middle half; and the same with every earlier fire that
/// lived as long (what one knows with no shape), as the baseline.
pub async fn backtest(db: &Strata, region: &str, limit: usize) -> Result<Value> {
    let mut all: Vec<Row> = db
        .scroll(
            "SELECT id, region, start, last, hours, area_km2, shape1, shape2, shape FROM ea_fires WHERE region = :r AND state = 'out' AND seen_start = true",
            json!({"r": region}),
            2000,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let t = |r: &Row, k: &str| chrono::DateTime::parse_from_rfc3339(&r.s(k)).map_or(0, |d| d.timestamp());
    all.sort_by_key(|r| t(r, "start"));
    let mut out = Vec::new();
    for (window, hours, _, _) in WINDOWS {
        let lived: Vec<&Row> = all.iter().filter(|r| r.f("hours") >= hours && !vector(r, window).is_empty()).collect();
        // The last `limit` of them, each with at least 30 fires out before.
        let mut cases = Vec::new();
        for (i, f) in lived.iter().enumerate() {
            let at = t(f, "start") + (hours * 3600.0) as i64;
            let before: Vec<f64> = all.iter().filter(|r| t(r, "last") <= at && r.f("hours") >= hours && r.s("id") != f.s("id")).map(|r| r.f("area_km2")).collect();
            if before.len() >= 30 {
                cases.push((i, at, before));
            }
        }
        let skip = cases.len().saturating_sub(limit);
        let (mut n, mut inside, mut inside_base) = (0usize, 0usize, 0usize);
        let (mut width, mut width_base, mut err, mut err_base) = (0.0, 0.0, 0.0, 0.0);
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        for (i, at, mut before) in cases.into_iter().skip(skip) {
            let f = lived[i];
            let rows = nearest(db, None, window, &vector(f, window), &f.s("id"), region, false, at, K).await.unwrap_or_default();
            // Analogs that lived through the window, as the baseline's do.
            let rows: Vec<Row> = rows.into_iter().filter(|r| r.f("hours") >= hours).collect();
            if rows.len() < 5 {
                continue;
            }
            let o = outlook_of(window, hours, &rows);
            before.sort_by(f64::total_cmp);
            let (blo, bmed, bhi) = (quantile(&before, 0.25), quantile(&before, 0.5), quantile(&before, 0.75));
            let real = f.f("area_km2");
            n += 1;
            inside += usize::from(real >= o.lo && real <= o.hi);
            inside_base += usize::from(real >= blo && real <= bhi);
            let lw = |lo: f64, hi: f64| ((1.0 + hi) / (1.0 + lo)).ln();
            width += lw(o.lo, o.hi);
            width_base += lw(blo, bhi);
            err += ((1.0 + o.median) / (1.0 + real)).ln().abs();
            err_base += ((1.0 + bmed) / (1.0 + real)).ln().abs();
            pairs.push((o.median, real));
        }
        if n == 0 {
            continue;
        }
        let nf = n as f64;
        let row = json!({
            "id": format!("{region}-{window}"), "region": region, "window": window, "hours": hours, "fires": n,
            "covered": inside as f64 / nf, "base_covered": inside_base as f64 / nf,
            "width": (width / nf).exp(), "base_width": (width_base / nf).exp(),
            "error": (err / nf).exp(), "base_error": (err_base / nf).exp(),
            "spearman": spearman(&pairs), "at": iso(chrono::Utc::now().timestamp()),
        });
        out.push(row);
    }
    if !out.is_empty() {
        db.upsert("ea_backtest", out.clone()).await.map_err(|e| anyhow::anyhow!("ea_backtest: {e}"))?;
    }
    Ok(json!({"region": region, "fires": all.len(), "windows": out}))
}

/// Spearman's rank correlation of the pairs.
pub fn spearman(pairs: &[(f64, f64)]) -> f64 {
    if pairs.len() < 3 {
        return 0.0;
    }
    fn ranks(xs: &[f64]) -> Vec<f64> {
        let mut idx: Vec<usize> = (0..xs.len()).collect();
        idx.sort_by(|&a, &b| xs[a].total_cmp(&xs[b]));
        let mut r = vec![0.0; xs.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && xs[idx[j + 1]] == xs[idx[i]] {
                j += 1;
            }
            let avg = (i + j) as f64 / 2.0;
            for k in i..=j {
                r[idx[k]] = avg;
            }
            i = j + 1;
        }
        r
    }
    let a = ranks(&pairs.iter().map(|p| p.0).collect::<Vec<_>>());
    let b = ranks(&pairs.iter().map(|p| p.1).collect::<Vec<_>>());
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    if va == 0.0 || vb == 0.0 {
        0.0
    } else {
        cov / (va * vb).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_and_ranks() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(quantile(&v, 0.5), 3.0);
        assert_eq!(quantile(&v, 0.25), 2.0);
        assert!((spearman(&[(1.0, 10.0), (2.0, 20.0), (3.0, 30.0)]) - 1.0).abs() < 1e-9);
        assert!((spearman(&[(1.0, 30.0), (2.0, 20.0), (3.0, 10.0)]) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_outlook_is_the_middle_of_the_analogs() {
        let rows: Vec<Row> = [0.5, 2.0, 4.0, 12.0, 40.0].iter().map(|a| serde_json::from_value(json!({"area_km2": a, "hours": 3.0})).unwrap()).collect();
        let o = outlook_of("shape2", 2.0, &rows);
        assert_eq!((o.median, o.lo, o.hi, o.n), (4.0, 2.0, 12.0, 5));
        assert!((o.over10 - 0.4).abs() < 1e-9);
    }
}
