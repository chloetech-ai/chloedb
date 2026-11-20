//! Alert rules, each a SQL statement kept in `ea_rules` and run every
//! minute over the fires burning; the rows a rule returns are its alerts.
//! An alert stays open while its rule finds it and closes when it stops.
//!
//! "Now" is the newest scan read for the region, not the clock, so a
//! replay of an archive raises the alerts it would have raised then.

use anyhow::Result;
use serde_json::{json, Value};

use crate::strata::{RowExt, Strata, Trace};

pub struct Rule {
    pub id: &'static str,
    pub title: &'static str,
    pub severity: &'static str,
    pub about: &'static str,
    pub sql: &'static str,
}

const COLS: &str = "id, region, name, town, adm2, lat, lon, start, last, scans, peak_mw, mw_now, mw_1h, people5, slope, trees, shrubs, gust, outlook_km2, outlook_lo, outlook_hi, area_km2";

/// `:r` is the region and `:since_*` times before its newest scan.
pub const RULES: &[Rule] = &[
    Rule {
        id: "new-fire",
        title: "A new fire",
        severity: "warn",
        about: "A fire GOES saw start in the last three hours, in two scans or more (one pixel in one scan is too often a hot roof or the sun on glass).",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND seen_start = true AND start >= :since_3h AND scans >= 2 ORDER BY start DESC LIMIT 100",
    },
    Rule {
        id: "people-near",
        title: "10,000 people or more within 5 km",
        severity: "bad",
        about: "A fire burning now with 10,000 people or more living within 5 km of where it started (GHSL 2025).",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND scans >= 2 AND people5 >= 10000 AND last >= :since_1h ORDER BY people5 DESC LIMIT 100",
    },
    Rule {
        id: "growing",
        title: "Power doubled in an hour, past 300 MW",
        severity: "bad",
        about: "The fire's radiative power is past 300 MW and twice what it was an hour ago: it is spreading fast.",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND mw_now >= 300 AND mw_now >= 2 * mw_1h AND last >= :since_1h ORDER BY mw_now DESC LIMIT 100",
    },
    Rule {
        id: "big",
        title: "Past 1,000 MW",
        severity: "bad",
        about: "The fire's power has been past 1,000 MW in a scan: a large, intense fire.",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND peak_mw >= 1000 AND last >= :since_1h ORDER BY peak_mw DESC LIMIT 100",
    },
    Rule {
        id: "steep-windy",
        title: "Steep, covered ground and strong gusts",
        severity: "warn",
        about: "It started on slopes of 15 degrees or more, half or more of it trees and shrubs, with gusts of 35 mph or more: how fires run uphill.",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND scans >= 2 AND slope >= 15 AND trees + shrubs >= 50 AND gust >= 35 AND last >= :since_1h ORDER BY gust DESC LIMIT 100",
    },
    Rule {
        id: "could-grow",
        title: "Fires that began like it grew past 20 km²",
        severity: "warn",
        about: "The fires whose first hours looked most like this one's (power, pixels, spread, what it burns on, the wind) mostly grew past 20 km²: the median of its twenty closest.",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND outlook_km2 >= 20 AND last >= :since_1h ORDER BY outlook_km2 DESC LIMIT 100",
    },
    Rule {
        id: "first-in-years",
        title: "Burning where no fire was seen since 2001",
        severity: "info",
        about: "Its ground has no fire in NASA FIRMS's records (MODIS from 2001, VIIRS from 2012): a lot to burn.",
        sql: "SELECT {COLS} FROM ea_fires WHERE region = :r AND state = 'burning' AND scans >= 3 AND burned_before IS NULL AND trees + shrubs >= 40 AND last >= :since_1h ORDER BY peak_mw DESC LIMIT 100",
    },
];

pub fn sql_of(r: &Rule) -> String {
    r.sql.replace("{COLS}", COLS)
}

/// Writes the rules (all at once, when the table is made).
pub async fn install(db: &Strata) -> Result<()> {
    let rows: Vec<Value> = RULES
        .iter()
        .map(|r| json!({"id": r.id, "title": r.title, "severity": r.severity, "sql": sql_of(r), "about": r.about, "enabled": true}))
        .collect();
    db.upsert("ea_rules", rows).await.map_err(|e| anyhow::anyhow!("ea_rules: {e}"))?;
    Ok(())
}

fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
}

/// The newest scan read for a region (Unix seconds), 0 when none.
pub async fn clock(db: &Strata, region: &str) -> i64 {
    let rows = db.query("SELECT MAX(ts) AS t FROM ea_scans WHERE region = :r", json!({"r": region})).await.unwrap_or_default();
    rows.first().and_then(|r| chrono::DateTime::parse_from_rfc3339(&r.s("t")).ok()).map_or(0, |d| d.timestamp())
}

/// Runs every rule for each region once: opens the alerts found, keeps
/// those found again, closes the rest.
pub async fn run_once(db: &Strata) -> Result<Value> {
    let trace = Trace::new();
    let mut found: Vec<Value> = Vec::new();
    let mut summary = Vec::new();
    for region in ["california", "chile"] {
        let now = clock(db, region).await;
        if now == 0 {
            continue;
        }
        for r in RULES {
            let sql = sql_of(r);
            let mut p = serde_json::Map::new();
            p.insert("r".into(), json!(region));
            for (k, s) in [("since_1h", 3600), ("since_3h", 3 * 3600), ("since_6h", 6 * 3600)] {
                if sql.contains(&format!(":{k}")) {
                    p.insert(k.into(), json!(iso(now - s)));
                }
            }
            let t0 = std::time::Instant::now();
            let rows = match db.traced_within(&trace, r.id, &sql, Value::Object(p), Some(30_000)).await {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!(rule = r.id, error = %e, "rule");
                    continue;
                }
            };
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let _ = db
                .write(vec![
                    json!({"op": "set", "table": "ea_rules", "key": r.id, "field": "last_run", "value": iso(now)}),
                    json!({"op": "set", "table": "ea_rules", "key": r.id, "field": "last_ms", "value": (ms * 10.0).round() / 10.0}),
                    json!({"op": "set", "table": "ea_rules", "key": r.id, "field": "last_rows", "value": rows.len()}),
                ])
                .await;
            summary.push(json!({"rule": r.id, "region": region, "rows": rows.len()}));
            for x in rows {
                found.push(json!({
                    "id": format!("{}:{}", r.id, x.s("id")), "rule": r.id, "severity": r.severity, "fire": x.s("id"), "region": region,
                    "title": format!("{}: {}", r.title, x.s("name")), "detail": detail(r.id, &x),
                    "lat": x.f("lat"), "lon": x.f("lon"), "last_seen": iso(now), "state": "open",
                }));
            }
        }
    }
    // Open what was found (keeping when it was first seen), close the rest.
    let open = db.query("SELECT id, first_seen FROM ea_alerts WHERE state = 'open' LIMIT 5000", json!({})).await.unwrap_or_default();
    let mut ops = Vec::new();
    let found_ids: std::collections::HashSet<String> = found.iter().filter_map(|a| a["id"].as_str().map(str::to_string)).collect();
    for a in &open {
        if !found_ids.contains(&a.s("id")) {
            ops.push(json!({"op": "set", "table": "ea_alerts", "key": a.s("id"), "field": "state", "value": "closed"}));
        }
    }
    let first: std::collections::HashMap<String, String> = open.iter().map(|a| (a.s("id"), a.s("first_seen"))).collect();
    for mut a in found {
        let id = a["id"].as_str().unwrap_or_default().to_string();
        a["first_seen"] = json!(first.get(&id).cloned().unwrap_or_else(|| a["last_seen"].as_str().unwrap_or_default().to_string()));
        ops.push(json!({"op": "upsert", "table": "ea_alerts", "doc": a}));
    }
    let n = ops.len();
    if !ops.is_empty() {
        db.write(ops).await.map_err(|e| anyhow::anyhow!("ea_alerts: {e}"))?;
    }
    Ok(json!({"ran": summary, "written": n, "open": found_ids.len()}))
}

/// The alert in a sentence.
pub fn detail(rule: &str, x: &crate::strata::Row) -> String {
    let near = if x.s("town").is_empty() { x.s("adm2") } else { x.s("town") };
    match rule {
        "people-near" => format!("{} people live within 5 km of where it started, near {near}.", fmt_int(x.f("people5"))),
        "growing" => format!("{:.0} MW now, {:.0} MW an hour ago.", x.f("mw_now"), x.f("mw_1h")),
        "big" => format!("{:.0} MW at its strongest; {:.0} MW in the last scan.", x.f("peak_mw"), x.f("mw_now")),
        "steep-windy" => format!("Slope {:.0}°, trees and shrubs {:.0}%, gusts {:.0} mph.", x.f("slope"), x.f("trees") + x.f("shrubs"), x.f("gust")),
        "could-grow" => format!("The fires that began like it grew to {:.0} km² (half of them between {:.0} and {:.0}).", x.f("outlook_km2"), x.f("outlook_lo"), x.f("outlook_hi")),
        "first-in-years" => format!("No fire seen on this ground since 2001; {:.0}% trees and shrubs.", x.f("trees") + x.f("shrubs")),
        _ => format!("Seen in {} scans, {:.0} MW at its strongest, near {near}.", x.i("scans"), x.f("peak_mw")),
    }
}

pub fn fmt_int(x: f64) -> String {
    let s = format!("{:.0}", x);
    let (neg, digits) = s.strip_prefix('-').map_or((false, s.as_str()), |d| (true, d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if neg {
        format!("-{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_have_commas() {
        assert_eq!(fmt_int(1234567.0), "1,234,567");
        assert_eq!(fmt_int(999.0), "999");
        assert_eq!(fmt_int(-12000.0), "-12,000");
    }

    #[test]
    fn every_rule_reads_the_region() {
        for r in RULES {
            let s = sql_of(r);
            assert!(s.contains("region = :r"), "{}", r.id);
            assert!(!s.contains("{COLS}"));
        }
    }
}
