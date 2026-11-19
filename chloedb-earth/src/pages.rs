//! What each page shows, each a few statements: Live (the fires burning
//! now and the pixels of the last hour), a replay of a past day, a fire
//! (its footprint, its first hours and the fires that began like it), the
//! past fires, a place (what lies on it, the places that look like it,
//! here and in the other region, the fires near it, how it changed), search
//! by words and by a picture, a place mixed with words, Climate twins,
//! Changed places, the map's layers, the rules and how it is built.

use serde_json::{json, Value};

use crate::analogs;
use crate::api::ApiError;
use crate::places::Places;
use crate::strata::{Row, RowExt, Strata, Trace};

pub struct Ctx<'a> {
    pub db: &'a Strata,
    pub trace: &'a Trace,
    pub places: &'a Places,
}

type R<T> = Result<T, ApiError>;

impl Ctx<'_> {
    pub async fn rows(&self, what: &str, sql: impl AsRef<str>, params: Value) -> R<Vec<Row>> {
        Ok(self.db.traced(self.trace, what, sql.as_ref(), params).await?)
    }

    /// Every row, past a query's 10,000: a scroll of pages of 10,000.
    pub async fn all(&self, what: &str, sql: impl AsRef<str>, params: Value) -> Vec<Row> {
        let t0 = std::time::Instant::now();
        let rows = self.db.scroll(sql.as_ref(), params.clone(), 10_000).await;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match rows {
            Ok(r) => {
                self.trace.push(what, sql.as_ref(), &params, r.len(), ms, Some("a scroll, pages of 10,000"));
                r
            }
            Err(e) => {
                self.trace.push(what, sql.as_ref(), &params, 0, ms, Some(&e.message));
                Vec::new()
            }
        }
    }

    /// As `rows`, an empty answer when ChloeDB refuses.
    pub async fn soft(&self, what: &str, sql: impl AsRef<str>, params: Value) -> Vec<Row> {
        self.db.traced(self.trace, what, sql.as_ref(), params).await.unwrap_or_default()
    }

    /// The vector of `text` by the model of `ea_words` (SigLIP so400m, the
    /// model of the places' pictures), as a query's parameter.
    pub fn words(&self, text: &str) -> Value {
        json!({"embed": prompt(text), "table": "ea_words", "field": "emb"})
    }

    /// The newest scan read for a region (Unix seconds).
    pub async fn clock(&self, region: &str) -> i64 {
        let rows = self.soft("the newest scan", "SELECT MAX(ts) AS t FROM ea_scans WHERE region = :r", json!({"r": region})).await;
        rows.first().and_then(|r| unix(&r.s("t"))).unwrap_or(0)
    }
}

/// SigLIP reads a search best as a caption of the picture it wants.
pub fn prompt(text: &str) -> String {
    let t = text.trim();
    let l = t.to_lowercase();
    if l.starts_with("a satellite") || l.starts_with("satellite") || l.starts_with("an aerial") {
        t.to_string()
    } else {
        format!("a satellite photo of {t}")
    }
}

pub fn floats(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default()
}

pub fn unix(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}

pub fn iso(t: i64) -> String {
    analogs::iso(t)
}

fn param(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|x| json!(((*x as f64) * 1e6).round() / 1e6)).collect())
}

fn norm(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

fn bad(m: impl Into<String>) -> ApiError {
    ApiError(axum::http::StatusCode::BAD_REQUEST, m.into())
}

fn region_ok(r: &str) -> R<&'static str> {
    match r {
        "california" => Ok("california"),
        "chile" => Ok("chile"),
        _ => Err(bad("the regions are california and chile")),
    }
}

pub fn other(r: &str) -> &'static str {
    if r == "chile" {
        "california"
    } else {
        "chile"
    }
}

pub const FIRE_CARD: &str = "id, region, name, town, adm1, adm2, place, lat, lon, start, last, state, seen_start, scans, pixels, peak_px, peak_mw, mw_now, mw_1h, energy, area_km2, spread_km, hours, people5, trees, shrubs, grass, built, slope, elev, climate, burned_before, gust, wind, rh, outlook_km2, outlook_lo, outlook_hi";

pub const PLACE_CARD: &str = "id, region, country, adm1, adm2, town, town_km, town_pop, lat, lon, cell, taken, product, kind, trees, shrubs, grass, crops, built, bare, snow, water, wetland, elev, elev_min, elev_max, slope, steep, people, climate, climate_group, mine, crop, crop_pct, fire_years, fire_px, last_fire, twin, twin_sim, words, about";

/// A small card of a place, for lists.
pub const PLACE_SMALL: &str = "id, region, adm1, adm2, town, lat, lon, kind, climate, trees, shrubs, crops, built, bare, water, people, product";

// ------------------------------------------------------------------ live

/// The days kept as replays: a region, a window and where to look.
pub struct Replay {
    pub id: &'static str,
    pub region: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub from: &'static str,
    pub to: &'static str,
    pub center: (f64, f64),
    pub zoom: f64,
}

pub const REPLAYS: &[Replay] = &[
    Replay {
        id: "la-2025",
        region: "california",
        title: "Los Angeles, 7 January 2025",
        about: "The Palisades and Eaton fires, driven by Santa Ana winds, from the first pixel GOES-18 saw on the morning of the 7th through the next day.",
        from: "2025-01-07T17:00:00Z",
        to: "2025-01-09T06:00:00Z",
        center: (34.12, -118.35),
        zoom: 9.6,
    },
    Replay {
        id: "valparaiso-2024",
        region: "chile",
        title: "Valparaíso, 2 February 2024",
        about: "The fires above Viña del Mar and Quilpué, Chile's deadliest, as GOES-16 saw them every ten minutes through the afternoon and night.",
        from: "2024-02-02T15:00:00Z",
        to: "2024-02-04T06:00:00Z",
        center: (-33.07, -71.45),
        zoom: 10.0,
    },
];

/// The fires burning in a region now, the pixels of its last hour, its
/// scans of the last day and the alerts open.
pub async fn live(c: &Ctx<'_>, region: &str) -> R<Value> {
    let region = region_ok(region)?;
    let now = c.clock(region).await;
    let since1 = iso(now - 3600);
    let since24 = iso(now - 86400);
    let (fires, pixels, scans, alerts) = tokio::join!(
        c.soft("the fires burning", format!("SELECT {FIRE_CARD} FROM ea_fires WHERE region = :r AND state = 'burning' ORDER BY peak_mw DESC LIMIT 300"), json!({"r": region})),
        c.soft("the fire pixels of the last hour", "SELECT lat, lon, mw, fire, ts, conf FROM ea_detections WHERE region = :r AND ts >= :since ORDER BY ts DESC LIMIT 6000", json!({"r": region, "since": since1})),
        c.soft("the scans of the last day", "SELECT ts, sat, pixels, mw, fires FROM ea_scans WHERE region = :r AND ts >= :since ORDER BY ts LIMIT 600", json!({"r": region, "since": since24})),
        c.soft("the alerts open", "SELECT id, rule, severity, fire, title, detail, lat, lon, first_seen, last_seen FROM ea_alerts WHERE region = :r AND state = 'open' ORDER BY last_seen DESC LIMIT 100", json!({"r": region})),
    );
    let wall = chrono::Utc::now().timestamp();
    Ok(json!({
        "region": region, "now": iso(now), "lag_s": if now > 0 { wall - now } else { -1 },
        "live": now > 0 && wall - now < 1800,
        "fires": fires, "pixels": pixels, "scans": scans, "alerts": alerts,
        "replays": REPLAYS.iter().map(|r| json!({"id": r.id, "region": r.region, "title": r.title})).collect::<Vec<_>>(),
    }))
}

/// A replay: every fire pixel GOES saw in its window, and the scans, for
/// the page to play.
pub async fn replay(c: &Ctx<'_>, id: &str) -> R<Value> {
    let Some(r) = REPLAYS.iter().find(|r| r.id == id) else { return Err(bad(format!("no replay {id}"))) };
    let (scans, pixels, fires) = tokio::join!(
        c.soft("the scans of the window", "SELECT ts, sat, pixels, mw, fires FROM ea_scans WHERE region = :r AND ts >= :a AND ts < :b ORDER BY ts LIMIT 2000", json!({"r": r.region, "a": r.from, "b": r.to})),
        c.all("its fire pixels", "SELECT ts, lat, lon, mw, fire FROM ea_detections WHERE region = :r AND ts >= :a AND ts < :b ORDER BY ts", json!({"r": r.region, "a": r.from, "b": r.to})),
        c.soft("the fires that burned in it", format!("SELECT {FIRE_CARD} FROM ea_fires WHERE region = :r AND start < :b AND last >= :a ORDER BY peak_mw DESC LIMIT 200"), json!({"r": r.region, "a": r.from, "b": r.to})),
    );
    // Pixels as columns: [t (seconds from the start), lat, lon, mw, fire index].
    let t0 = unix(r.from).unwrap_or(0);
    let mut fire_ix: Vec<String> = Vec::new();
    let cols: Vec<Value> = pixels
        .iter()
        .map(|p| {
            let f = p.s("fire");
            let i = fire_ix.iter().position(|x| *x == f).unwrap_or_else(|| {
                fire_ix.push(f.clone());
                fire_ix.len() - 1
            });
            json!([unix(&p.s("ts")).unwrap_or(t0) - t0, (p.f("lat") * 1e4).round() / 1e4, (p.f("lon") * 1e4).round() / 1e4, (p.f("mw") * 10.0).round() / 10.0, i])
        })
        .collect();
    Ok(json!({
        "id": r.id, "region": r.region, "title": r.title, "about": r.about, "from": r.from, "to": r.to,
        "center": [r.center.0, r.center.1], "zoom": r.zoom,
        "scans": scans, "pixels": cols, "fire_ids": fire_ix, "fires": fires,
    }))
}

// ------------------------------------------------------------------ fires

/// One fire: its row, its pixels, its first hours, the fires that began
/// like it and what they became, the place it started on, its alerts.
pub async fn fire(c: &Ctx<'_>, id: &str, at_hours: Option<f64>, anywhere: bool) -> R<Value> {
    let rows = c.rows("the fire", "SELECT *, shape, shape2, shape1 FROM ea_fires WHERE id = :id", json!({"id": id})).await?;
    let Some(f) = rows.into_iter().next() else { return Err(ApiError(axum::http::StatusCode::NOT_FOUND, format!("no fire {id}"))) };
    let start = unix(&f.s("start")).unwrap_or(0);
    let last = unix(&f.s("last")).unwrap_or(start);
    let (pixels, alerts, place) = tokio::join!(
        c.soft("its pixels", "SELECT ts, lat, lon, mw, conf FROM ea_detections WHERE fire = :f ORDER BY ts LIMIT 10000", json!({"f": id})),
        c.soft("its alerts", "SELECT id, rule, severity, title, detail, first_seen, last_seen, state FROM ea_alerts WHERE fire = :f LIMIT 50", json!({"f": id})),
        c.soft("the place it started on", format!("SELECT {PLACE_SMALL} FROM ea_places WHERE id = :p"), json!({"p": f.s("place")})),
    );
    // The window: the one asked for, or the longest it lived through.
    let age = at_hours.unwrap_or((last - start) as f64 / 3600.0);
    let mut similar = Vec::new();
    let mut outlook = Value::Null;
    if let Some((w, hours, _, _)) = crate::fires::window_for(age) {
        let v = analogs::vector(&f, w);
        if !v.is_empty() {
            // Fires out before this one started: what was known then.
            let known = if f.s("state") == "out" { start + (hours * 3600.0) as i64 } else { last };
            let got = analogs::nearest(c.db, Some(c.trace), w, &v, id, &f.s("region"), anywhere, known, analogs::K).await.unwrap_or_default();
            if got.len() >= 5 {
                let o = analogs::outlook_of(w, hours, &got);
                outlook = json!({"window": w, "hours": hours, "words": analogs::window_words(w), "n": o.n, "median": o.median, "lo": o.lo, "hi": o.hi, "hours_median": o.hours_median, "over10": o.over10, "known": iso(known)});
            }
            similar = got;
        }
    }
    let backtest = c.soft("how well this has foretold fires before", "SELECT * FROM ea_backtest WHERE region = :r ORDER BY hours LIMIT 10", json!({"r": f.s("region")})).await;
    let mut fire = f.clone();
    for k in ["shape", "shape1", "shape2"] {
        fire.remove(k);
    }
    Ok(json!({"fire": fire, "pixels": pixels, "alerts": alerts, "place": place.first(), "similar": similar, "outlook": outlook, "age_h": age, "backtest": backtest}))
}

/// The past fires of a region: the largest first, or those of a year, or
/// by name.
pub async fn fires(c: &Ctx<'_>, region: &str, year: Option<i32>, q: &str, sort: &str) -> R<Value> {
    let region = region_ok(region)?;
    let mut wh = String::from("region = :r AND area_km2 >= 1");
    let mut p = json!({"r": region});
    if let Some(y) = year {
        wh.push_str(" AND start >= :a AND start < :b");
        p["a"] = json!(format!("{y}-01-01"));
        p["b"] = json!(format!("{}-01-01", y + 1));
    }
    let order = match sort {
        "recent" => "start DESC",
        "power" => "peak_mw DESC",
        "people" => "people5 DESC",
        _ => "area_km2 DESC",
    };
    let sql = if q.trim().is_empty() {
        format!("SELECT {FIRE_CARD} FROM ea_fires WHERE {wh} ORDER BY {order} LIMIT 200")
    } else {
        p["q"] = json!(q.trim());
        format!("SELECT {FIRE_CARD}, score() AS score FROM ea_fires WHERE {wh} AND match(name, :q) ORDER BY bm25(name, :q) DESC LIMIT 200")
    };
    let (list, years) = tokio::join!(
        c.rows("the fires", sql, p.clone()),
        c.soft("fires a year", "SELECT date_trunc('year', start) AS year, COUNT(*) AS fires, SUM(area_km2) AS km2, MAX(peak_mw) AS peak FROM ea_fires WHERE region = :r AND area_km2 >= 1 GROUP BY year ORDER BY year LIMIT 40", json!({"r": region})),
    );
    Ok(json!({"region": region, "year": year, "q": q, "sort": sort, "fires": list?, "years": years}))
}

// ----------------------------------------------------------------- places

/// One place: what lies on it, the places that look like it (here and in
/// the other region), the fires near it, its years.
pub async fn place(c: &Ctx<'_>, id: &str) -> R<Value> {
    let rows = c.rows("the place", format!("SELECT {PLACE_CARD} FROM ea_places WHERE id = :id"), json!({"id": id})).await?;
    let Some(p) = rows.into_iter().next() else { return Err(ApiError(axum::http::StatusCode::NOT_FOUND, format!("no place {id}"))) };
    let region = p.s("region");
    let (lat, lon) = (p.f("lat"), p.f("lon"));
    // Places that look like it, away from it: its own fragments overlap.
    let away = "(lat < :s OR lat > :n OR lon < :w OR lon > :e)";
    let me = json!({"vector_of": id, "table": "ea_places", "field": "look"});
    let box_ = |d: f64| (lat - d, lat + d, lon - d, lon + d);
    let (s, n, w, e) = box_(0.08);
    let (fs, fnn, fw, fe) = box_(0.06);
    let (like, twins, near, years, change) = tokio::join!(
        c.soft("places here that look like it", format!("SELECT {PLACE_SMALL}, score() AS score FROM ea_places WHERE region = :r AND {away} ORDER BY similarity(look, :v) DESC LIMIT 12"), json!({"r": region, "v": me, "s": s, "n": n, "w": w, "e": e})),
        c.soft("its twins in the other region", format!("SELECT {PLACE_SMALL}, score() AS score FROM ea_places WHERE region = :r ORDER BY similarity(look, :v) DESC LIMIT 12"), json!({"r": other(&region), "v": me})),
        c.soft("fires near it", format!("SELECT {FIRE_CARD} FROM ea_fires WHERE lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e ORDER BY start DESC LIMIT 30"), json!({"s": fs, "n": fnn, "w": fw, "e": fe})),
        c.soft("its years", "SELECT id, year, taken, scene, cloud, ndvi, label FROM ea_years WHERE place = :p ORDER BY year LIMIT 20", json!({"p": id})),
        c.soft("how it changed", "SELECT id, first, last, drift, step_year, step, from_label, to_label, arc FROM ea_changes WHERE id = :p", json!({"p": id})),
    );
    Ok(json!({"place": p, "like": like, "twins": twins, "fires": near, "years": years, "change": change.first()}))
}

pub struct Filters {
    pub region: String,
    pub climate: String,
    pub kind: String,
    pub max_people: Option<f64>,
    pub min_people: Option<f64>,
    pub min_trees: Option<f64>,
    pub burned: Option<bool>,
}

impl Filters {
    fn sql(&self, p: &mut Value) -> String {
        let mut wh = Vec::new();
        if !self.region.is_empty() {
            wh.push("region = :region".to_string());
            p["region"] = json!(self.region);
        }
        if !self.climate.is_empty() {
            wh.push("climate = :climate".to_string());
            p["climate"] = json!(self.climate);
        }
        if !self.kind.is_empty() {
            wh.push("kind = :kind".to_string());
            p["kind"] = json!(self.kind);
        }
        if let Some(x) = self.max_people {
            wh.push("people <= :maxp".to_string());
            p["maxp"] = json!(x);
        }
        if let Some(x) = self.min_people {
            wh.push("people >= :minp".to_string());
            p["minp"] = json!(x);
        }
        if let Some(x) = self.min_trees {
            wh.push("trees >= :mint".to_string());
            p["mint"] = json!(x);
        }
        match self.burned {
            Some(true) => wh.push("fire_px > 0".to_string()),
            Some(false) => wh.push("fire_px = 0".to_string()),
            None => {}
        }
        if wh.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wh.join(" AND "))
        }
    }
}

/// Reads the region out of the words: "vineyards in chile".
pub fn split_region(q: &str) -> (String, Option<&'static str>) {
    let l = q.to_lowercase();
    for (w, r) in [(" in chile", "chile"), (" in california", "california"), (" in ca", "california")] {
        if let Some(i) = l.rfind(w) {
            if i + w.len() == l.len() {
                return (q[..i].trim().to_string(), Some(r));
            }
        }
    }
    (q.trim().to_string(), None)
}

/// Places by words: the pictures nearest to the words' vector (SigLIP),
/// the descriptions by BM25 (with the names), or both fused by rank (the
/// picture and what covers it, without the names: the default).
pub async fn search(c: &Ctx<'_>, q: &str, mode: &str, mut f: Filters) -> R<Value> {
    let (words, r) = split_region(q);
    if let Some(r) = r {
        f.region = r.to_string();
    }
    if words.is_empty() {
        return Err(bad("search for what a place looks like: vineyards on hills, a lithium mine, a burn scar"));
    }
    // By words alone, no vector: a server without the model answers it.
    let mut p = if mode == "words" { json!({"q": words}) } else { json!({"v": c.words(&words), "q": words}) };
    let wh = f.sql(&mut p);
    let order = match mode {
        "words" => "bm25(words, :q) DESC",
        "look" => "similarity(look, :v) DESC",
        _ => "rrf(similarity(look, :v), bm25(about, :q)) DESC",
    };
    let sql = format!("SELECT {PLACE_SMALL}, score() AS score FROM ea_places {wh} ORDER BY {order} LIMIT 60");
    let rows = c.rows(&format!("places that look like \u{201c}{words}\u{201d}"), sql, p).await?;
    let mode = if matches!(mode, "words" | "look") { mode } else { "both" };
    Ok(json!({"q": q, "words": words, "prompt": prompt(&words), "mode": mode, "region": f.region, "places": rows}))
}

/// Places that look like a picture (a JPEG or PNG, base64).
pub async fn picture(c: &Ctx<'_>, image_b64: &str, f: Filters) -> R<Value> {
    let mut p = json!({"v": {"embed": {"image": image_b64}, "table": "ea_words", "field": "emb"}});
    let wh = f.sql(&mut p);
    let sql = format!("SELECT {PLACE_SMALL}, score() AS score FROM ea_places {wh} ORDER BY similarity(look, :v) DESC LIMIT 60");
    let rows = c.rows("places that look like the picture", sql, p).await?;
    Ok(json!({"region": f.region, "places": rows}))
}

/// A place mixed with words: the places that look most like it whose
/// description (what covers them, their terrain, crops and mines) has the
/// words of `plus` and none of `minus`, searched for in a region. When no
/// description has them (Chile has no crop map), the words become a
/// picture instead: the mean look of the places a search for them finds
/// first, added to the place's own by weight `w`.
pub async fn mix(c: &Ctx<'_>, id: &str, plus: &str, minus: &str, w: f32, f: Filters) -> R<Value> {
    let rows = c.rows("the place's vector", "SELECT id, look FROM ea_places WHERE id = :id", json!({"id": id})).await?;
    let Some(base) = rows.first() else { return Err(ApiError(axum::http::StatusCode::NOT_FOUND, format!("no place {id}"))) };
    let mut v = floats(&base["look"]);
    if v.is_empty() {
        return Err(bad("the place has no picture's vector"));
    }
    norm(&mut v);
    let (add, take) = (match_words(plus), match_words(minus));
    if add.is_empty() && take.is_empty() {
        return Err(bad("words to add or take away: vineyards, a lake, houses"));
    }
    let what = match (add.is_empty(), take.is_empty()) {
        (false, true) => format!("it, with \u{201c}{}\u{201d}", plus.trim()),
        (true, false) => format!("it, without \u{201c}{}\u{201d}", minus.trim()),
        _ => format!("it, with \u{201c}{}\u{201d}, without \u{201c}{}\u{201d}", plus.trim(), minus.trim()),
    };
    // By the words: its look, among the places described with them (all
    // the words, fewer while that finds too few).
    let mut p = json!({"v": param(&v), "id": id});
    let mut wh = f.sql(&mut p);
    let mut and = vec!["id <> :id".to_string()];
    if !take.is_empty() {
        and.push("NOT MATCH(about, :take)".into());
        p["take"] = json!(take.join(" OR "));
    }
    // The guarded MATCH last: older servers refused one followed by another AND.
    if !add.is_empty() {
        and.push("MATCH(about, :add, min => 1, min_rows => 24)".into());
        p["add"] = json!(add.join(" "));
    }
    wh = if wh.is_empty() { format!("WHERE {}", and.join(" AND ")) } else { format!("{wh} AND {}", and.join(" AND ")) };
    let found = c.rows(&what, format!("SELECT {PLACE_SMALL}, score() AS score FROM ea_places {wh} ORDER BY similarity(look, :v) DESC LIMIT 48"), p).await?;
    if found.len() >= 6 || add.is_empty() {
        return Ok(json!({"id": id, "plus": plus, "minus": minus, "how": "words", "region": f.region, "places": found}));
    }
    // By the look: no description has the words.
    let looks = c.rows(&format!("the places that look most like \u{201c}{}\u{201d}", plus.trim()), "SELECT look FROM ea_places ORDER BY rrf(similarity(look, :t), bm25(about, :q)) DESC LIMIT 24", json!({"t": c.words(plus.trim()), "q": plus.trim()})).await?;
    let mut mean = vec![0f32; v.len()];
    for r in &looks {
        let mut x = floats(&r["look"]);
        if x.len() == mean.len() {
            norm(&mut x);
            mean.iter_mut().zip(&x).for_each(|(m, y)| *m += y);
        }
    }
    norm(&mut mean);
    v.iter_mut().zip(&mean).for_each(|(x, y)| *x += w * y);
    norm(&mut v);
    let mut p = json!({"v": param(&v), "id": id});
    let mut wh = f.sql(&mut p);
    let mut and = vec!["id <> :id".to_string()];
    if !take.is_empty() {
        and.push("NOT MATCH(about, :take)".into());
        p["take"] = json!(take.join(" OR "));
    }
    wh = if wh.is_empty() { format!("WHERE {}", and.join(" AND ")) } else { format!("{wh} AND {}", and.join(" AND ")) };
    let found = c.rows(&what, format!("SELECT {PLACE_SMALL}, score() AS score FROM ea_places {wh} ORDER BY similarity(look, :v) DESC LIMIT 48"), p).await?;
    Ok(json!({"id": id, "plus": plus, "minus": minus, "how": "look", "w": w, "region": f.region, "places": found}))
}

/// The words of a mix, for MATCH: letters and digits only (so no word is
/// read as an operator), lower case, without the short ones.
fn match_words(s: &str) -> Vec<String> {
    s.split(|ch: char| !ch.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() > 2 && !matches!(w.as_str(), "and" | "the" | "with" | "not" | "near"))
        .take(8)
        .collect()
}

/// Climate twins: the places of one region whose pictures look most like
/// a place of the other, by climate.
pub async fn twins(c: &Ctx<'_>, from: &str, climate: &str, kind: &str) -> R<Value> {
    let from = region_ok(from)?;
    let mut p = json!({"r": from});
    let mut wh = "region = :r AND twin_score > -1".to_string();
    if !climate.is_empty() {
        wh.push_str(" AND climate = :c");
        p["c"] = json!(climate);
    }
    if !kind.is_empty() {
        wh.push_str(" AND kind = :k");
        p["k"] = json!(kind);
    }
    let (best, climates) = tokio::join!(
        c.rows("the closest twins", format!("SELECT {PLACE_SMALL}, twin, twin_sim, twin_score FROM ea_places WHERE {wh} ORDER BY twin_score DESC LIMIT 150"), p),
        c.soft("each climate's places", "SELECT region, climate, COUNT(*) AS places, AVG(twin_sim) AS sim FROM ea_places GROUP BY region, climate ORDER BY places DESC LIMIT 80", json!({})),
    );
    let best = best?;
    let ids: Vec<String> = best.iter().map(|r| r.s("twin")).filter(|t| !t.is_empty()).collect();
    let theirs = if ids.is_empty() { Vec::new() } else { c.soft("their twins", format!("SELECT {PLACE_SMALL} FROM ea_places WHERE id IN (:ids)"), json!({"ids": ids})).await };
    // The pairs whose land the layers agree on first (a forest's twin a
    // forest): a picture can look like another's and be something else (a
    // snowy forest, deep water).
    let mut pairs: Vec<(bool, Value)> = best
        .iter()
        .map(|a| {
            let b = theirs.iter().find(|t| t.s("id") == a.s("twin"));
            let same = b.is_some_and(|b| b.s("kind") == a.s("kind"));
            (same, json!({"a": a, "b": b, "sim": a.f("twin_sim"), "score": a.f("twin_score"), "same_kind": same}))
        })
        .collect();
    pairs.sort_by_key(|(same, _)| !*same);
    let pairs: Vec<Value> = pairs.into_iter().take(36).map(|(_, v)| v).collect();
    Ok(json!({"from": from, "to": other(from), "climate": climate, "kind": kind, "pairs": pairs, "climates": climates}))
}

// ---------------------------------------------------------------- changed

/// "fields became a solar farm": the words before and after.
pub fn split_change(q: &str) -> Option<(String, String)> {
    let l = q.to_lowercase();
    for sep in [" became ", " turned into ", " into ", " to ", " -> ", " → ", "->", "→"] {
        if let Some(i) = l.find(sep) {
            let (a, b) = (q[..i].trim(), q[i + sep.len()..].trim());
            if !a.is_empty() && !b.is_empty() {
                return Some((a.to_string(), b.to_string()));
            }
        }
    }
    None
}

pub const CHANGE_CARD: &str = "id, region, lat, lon, town, adm1, first, last, drift, step_year, step, from_label, to_label, arc";

/// Changed places: the most changed, by labels, or searched by what they
/// were and what they became.
pub async fn changed(c: &Ctx<'_>, q: &str, region: &str, from: &str, to: &str) -> R<Value> {
    let mut p = json!({});
    let mut wh = Vec::new();
    if !region.is_empty() {
        wh.push("region = :r");
        p["r"] = json!(region_ok(region)?);
    }
    if !from.is_empty() {
        wh.push("from_label = :fl");
        p["fl"] = json!(from);
    }
    if !to.is_empty() {
        wh.push("to_label = :tl");
        p["tl"] = json!(to);
    }
    let wh = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
    let (sql, what) = match split_change(q) {
        Some((a, b)) => {
            p["a"] = c.words(&a);
            p["b"] = c.words(&b);
            (format!("SELECT {CHANGE_CARD}, score() AS score FROM ea_changes {wh} ORDER BY similarity(before, :a) + similarity(after, :b) DESC LIMIT 48"), format!("places that were \u{201c}{a}\u{201d} and became \u{201c}{b}\u{201d}"))
        }
        None if !q.trim().is_empty() => {
            p["b"] = c.words(q);
            (format!("SELECT {CHANGE_CARD}, score() AS score FROM ea_changes {wh} ORDER BY similarity(after, :b) - similarity(before, :b) DESC LIMIT 48"), format!("places that became \u{201c}{}\u{201d}", q.trim()))
        }
        None => (format!("SELECT {CHANGE_CARD} FROM ea_changes {wh} ORDER BY drift DESC LIMIT 48"), "the places that changed most".to_string()),
    };
    let (rows, labels) = tokio::join!(
        c.rows(&what, sql, p),
        c.soft("the changes by label", "SELECT from_label, to_label, COUNT(*) AS places FROM ea_changes WHERE from_label <> to_label GROUP BY from_label, to_label ORDER BY places DESC LIMIT 30", json!({})),
    );
    Ok(json!({"q": q, "what": what, "region": region, "places": rows?, "labels": labels}))
}

/// The places whose change went the same way: the nearest direction from
/// their first look to their last.
pub async fn same_path(c: &Ctx<'_>, id: &str) -> R<Value> {
    let rows = c.rows("the change", format!("SELECT {CHANGE_CARD} FROM ea_changes WHERE id = :id"), json!({"id": id})).await?;
    let Some(me) = rows.into_iter().next() else { return Err(ApiError(axum::http::StatusCode::NOT_FOUND, format!("no change at {id}"))) };
    let (years, same) = tokio::join!(
        c.soft("its years", "SELECT id, year, taken, scene, cloud, ndvi, label FROM ea_years WHERE place = :p ORDER BY year LIMIT 20", json!({"p": id})),
        c.soft("places that changed the same way", format!("SELECT {CHANGE_CARD}, score() AS score FROM ea_changes WHERE id <> :id ORDER BY similarity(delta, :v) DESC LIMIT 24"), json!({"id": id, "v": {"vector_of": id, "table": "ea_changes", "field": "delta"}})),
    );
    Ok(json!({"change": me, "years": years, "same": same}))
}

// ----------------------------------------------------------------- layers

/// The layers the map can paint, each a column of the places.
pub const LAYERS: &[(&str, &str, &str)] = &[
    ("trees", "Trees", "% of the place under trees (ESA WorldCover 2021)"),
    ("shrubs", "Shrubs", "% under shrubland"),
    ("grass", "Grass", "% under grassland"),
    ("crops", "Farms", "% under cropland"),
    ("built", "Built", "% built up"),
    ("bare", "Bare", "% bare or sparse"),
    ("water", "Water", "% water"),
    ("snow", "Snow and ice", "% snow and ice"),
    ("people", "People", "people living on it (GHSL 2025)"),
    ("elev", "Terrain", "mean height, metres (Copernicus GLO-90)"),
    ("slope", "Slope", "mean slope, degrees"),
    ("mine", "Mines", "% under mines (Maus et al. 2022)"),
    ("fires", "Fire history", "years since 2001 with a fire seen (NASA FIRMS)"),
    ("climate", "Climate", "Köppen-Geiger class, 1991-2020 (Beck et al. 2023)"),
    ("kind", "What it is", "the main land cover"),
    ("crop", "Crops", "the main crop (USDA CDL 2024, California only)"),
];

/// A layer of a region as columns (lat, lon, value), from the places kept
/// in memory.
pub fn layer(places: &Places, name: &str, region: &str) -> R<Value> {
    let region = region_ok(region)?;
    let Some(&(_, title, about)) = LAYERS.iter().find(|l| l.0 == name) else { return Err(bad(format!("no layer {name}"))) };
    let mut lat = Vec::new();
    let mut lon = Vec::new();
    let mut val: Vec<Value> = Vec::new();
    let mut codes: Vec<String> = Vec::new();
    let mut code = |s: &str| -> Value {
        if s.is_empty() {
            return Value::Null;
        }
        let i = codes.iter().position(|c| c == s).unwrap_or_else(|| {
            codes.push(s.to_string());
            codes.len() - 1
        });
        json!(i)
    };
    for p in places.all.iter().filter(|p| p.region == region) {
        let v = match name {
            "trees" => json!(p.trees.round()),
            "shrubs" => json!(p.shrubs.round()),
            "grass" => json!(p.grass.round()),
            "crops" => json!(p.crops.round()),
            "built" => json!(p.built.round()),
            "bare" => json!(p.bare.round()),
            "water" => json!(p.water.round()),
            "snow" => json!(p.snow.round()),
            "people" => json!(p.people.round()),
            "elev" => json!(p.elev.round()),
            "slope" => json!((p.slope * 10.0).round() / 10.0),
            "mine" => json!((p.mine * 10.0).round() / 10.0),
            "fires" => json!(p.fire_years.len()),
            "climate" => code(&p.climate),
            "kind" => code(&p.kind),
            _ => code(&p.crop),
        };
        lat.push((p.lat * 1e4).round() / 1e4);
        lon.push((p.lon * 1e4).round() / 1e4);
        val.push(v);
    }
    Ok(json!({"layer": name, "title": title, "about": about, "region": region, "lat": lat, "lon": lon, "v": val, "codes": codes}))
}

// ------------------------------------------------------------ the others

pub async fn rules(c: &Ctx<'_>) -> R<Value> {
    let (rules, open) = tokio::join!(
        c.rows("the rules", "SELECT id, title, severity, sql, about, enabled, last_run, last_ms, last_rows FROM ea_rules LIMIT 100", json!({})),
        c.soft("the alerts open", "SELECT id, rule, severity, fire, region, title, detail, first_seen, last_seen FROM ea_alerts WHERE state = 'open' ORDER BY last_seen DESC LIMIT 200", json!({})),
    );
    Ok(json!({"rules": rules?, "open": open}))
}

/// Where the data comes from, how much of it there is, and how it is kept.
pub async fn data(c: &Ctx<'_>) -> R<Value> {
    let tables = crate::schema::tables();
    let mut counts = Vec::new();
    for (name, _, about) in &tables {
        let n = c.soft(&format!("rows of {name}"), format!("SELECT COUNT(*) AS n FROM {name}"), json!({})).await;
        counts.push(json!({"table": name, "rows": n.first().map(|r| r.i("n")).unwrap_or(0), "about": about}));
    }
    let (feeds, sats, regions, backtest) = tokio::join!(
        c.soft("where each feed stands", "SELECT id, kind, at, rows, note FROM ea_feed LIMIT 20", json!({})),
        c.soft("scans by satellite", "SELECT region, sat, COUNT(*) AS scans, MIN(ts) AS first, MAX(ts) AS last, SUM(pixels) AS pixels FROM ea_scans GROUP BY region, sat LIMIT 20", json!({})),
        c.soft("places by region", "SELECT region, COUNT(*) AS places, SUM(people) AS people FROM ea_places GROUP BY region LIMIT 5", json!({})),
        c.soft("the backtest", "SELECT * FROM ea_backtest ORDER BY region, hours LIMIT 20", json!({})),
    );
    Ok(json!({"tables": counts, "feeds": feeds, "satellites": sats, "regions": regions, "backtest": backtest, "sources": SOURCES.iter().map(|s| json!({"name": s.0, "what": s.1, "licence": s.2, "url": s.3})).collect::<Vec<_>>()}))
}

/// (name, what it gives, its licence, where it is).
pub const SOURCES: &[(&str, &str, &str, &str)] = &[
    ("Major TOM Core-S2RGB-SigLIP", "Sentinel-2 pictures of the whole Earth cut into 3.84 km squares, each with its SigLIP so400m vector", "CC BY-SA 4.0 (Copernicus Sentinel data)", "https://huggingface.co/datasets/Major-TOM/Core-S2RGB-SigLIP"),
    ("GOES-16, -17, -18 and -19 Fire Detection (ABI L2 FDC)", "every fire pixel of every scan: CONUS every 5 minutes, the full disk every 10", "public domain (NOAA)", "https://registry.opendata.aws/noaa-goes/"),
    ("NASA FIRMS", "every fire MODIS (from 2001) and VIIRS (from 2012) saw, by country and year", "public domain (NASA)", "https://firms.modaps.eosdis.nasa.gov/"),
    ("ESA WorldCover 2021", "land cover at 10 m", "CC BY 4.0", "https://esa-worldcover.org/"),
    ("Copernicus DEM GLO-90", "terrain at 90 m", "free licence (ESA, Copernicus)", "https://registry.opendata.aws/copernicus-dem/"),
    ("GHSL GHS-POP 2025", "people at 100 m", "CC BY 4.0 (European Commission JRC)", "https://human-settlement.emergency.copernicus.eu/"),
    ("Köppen-Geiger V2 (Beck et al. 2023)", "climate classes at 1 km, 1991-2020", "CC BY 4.0", "https://www.gloh2o.org/koppen/"),
    ("Global mining polygons v2 (Maus et al. 2022)", "where the mines are", "CC BY-SA 4.0", "https://doi.pangaea.de/10.1594/PANGAEA.942325"),
    ("USDA Cropland Data Layer 2024", "the crop of every 30 m field in California", "public domain (USDA NASS)", "https://www.nass.usda.gov/Research_and_Science/Cropland/"),
    ("geoBoundaries", "regions, counties and comunas", "CC BY 3.0 IGO (Chile), public domain (US)", "https://www.geoboundaries.org/"),
    ("GeoNames", "towns and their populations", "CC BY 4.0", "https://www.geonames.org/"),
    ("EOX Sentinel-2 cloudless 2016", "the map's picture of the ground", "CC BY 4.0 (EOX IT Services, Copernicus Sentinel data 2016)", "https://s2maps.eu/"),
    ("Element 84 Earth Search (Sentinel-2 L2A)", "each place's picture every year, for how it changed", "free licence (Copernicus Sentinel data)", "https://earth-search.aws.element84.com/v1"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_read_for_a_region_and_a_change() {
        assert_eq!(split_region("vineyards on hills in Chile"), ("vineyards on hills".to_string(), Some("chile")));
        assert_eq!(split_region("a lithium mine"), ("a lithium mine".to_string(), None));
        assert_eq!(split_change("fields became a solar farm"), Some(("fields".to_string(), "a solar farm".to_string())));
        assert_eq!(split_change("forest → burn scar"), Some(("forest".to_string(), "burn scar".to_string())));
        assert_eq!(split_change("suburbs"), None);
        assert_eq!(prompt("vineyards"), "a satellite photo of vineyards");
        assert_eq!(prompt("Satellite image of a port"), "Satellite image of a port");
    }

    #[test]
    fn a_layer_is_columns_of_one_region() {
        use crate::places::Place;
        let p = Places::new(vec![
            Place { id: "a".into(), region: "chile".into(), lat: -33.0, lon: -71.0, trees: 40.4, climate: "Csb".into(), ..Default::default() },
            Place { id: "b".into(), region: "california".into(), lat: 34.0, lon: -118.0, trees: 3.0, climate: "Csa".into(), ..Default::default() },
        ]);
        let v = layer(&p, "trees", "chile").unwrap();
        assert_eq!(v["v"], json!([40.0]));
        let v = layer(&p, "climate", "california").unwrap();
        assert_eq!((v["v"].clone(), v["codes"].clone()), (json!([0]), json!(["Csa"])));
        assert!(layer(&p, "nope", "chile").is_err());
    }
}
