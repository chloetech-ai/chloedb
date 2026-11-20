//! Fire watch: places people keep an eye on (a home, a parent's house, a
//! farm), each checked against GOES's fire pixels as the scans come in.
//!
//! A watch is a row of `ea_watches`: a point, a name, the distance it asks
//! about and two choices (a fire coming 2 km closer in an hour, a fire
//! passing 1,000 MW). There are no accounts: a browser makes a long random
//! key, the watcher, and a watch belongs to the key that made it. The key
//! is in the address of the watcher's Atom feed, so the feed needs nothing
//! else.
//!
//! Every minute `serve` reads the scans that came in since it last looked
//! and, for each watch in the region, the nearest fire pixel of the hour
//! up to each scan. What it finds is written to `ea_watch_alerts`, once per
//! watch, fire and kind (a fire within the distance asked, within 5 km,
//! over the place, past 1,000 MW; and a fire 2 km closer, once an hour).
//! The page shows them, the browser notifies, the feed carries them.

use crate::api::ApiError;
use crate::feed::REGIONS;
use crate::fires::km;
use crate::pages::{iso, Ctx};
use crate::places::Places;
use crate::strata::{Row, RowExt, Strata};
use anyhow::{anyhow, Result};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Watches one watcher may keep.
pub const MAX_PER_WATCHER: usize = 25;
/// Watches in all, so a script cannot fill the table.
pub const MAX_WATCHES: usize = 20_000;
/// The distances a watch may ask about.
pub const KMS: [f64; 5] = [5.0, 10.0, 15.0, 25.0, 50.0];
/// A fire pixel this close is over the place (a GOES pixel is 2 to 3 km a side).
pub const HERE_KM: f64 = 1.5;
/// Farther than this, no fire is near.
pub const FAR_KM: f64 = 50.0;
/// A fire this strong in one scan is large and intense.
pub const BIG_MW: f64 = 1000.0;
/// A fire this much closer than an hour before is coming.
pub const CLOSER_KM: f64 = 2.0;
/// Alerts are kept and not raised again for this long.
const KEEP_S: i64 = 30 * 86400;
/// How far a place's look must move between two pictures (1 minus their
/// cosine) to be told as a change: about the 90th centile of the moves
/// between the two-yearly pictures of the places with years (0.089).
pub const CHANGE_STEP: f64 = 0.09;
/// Two pictures further apart than this may be of different seasons, so
/// they are not compared.
pub const CHANGE_GAP_S: i64 = 120 * 86400;
/// New pictures are looked for this far back (inside KEEP_S, so an alert
/// is still there to be seen when its picture is).
const CHANGE_BACK_S: i64 = 25 * 86400;

/// A key a browser made: letters, digits and dashes, long enough not to be
/// guessed.
pub fn key_ok(k: &str) -> bool {
    (20..=64).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The region a point is in, if Earth watches it.
pub fn region_of(lat: f64, lon: f64) -> Option<&'static str> {
    REGIONS.iter().find(|r| lat >= r.bounds.0 && lat <= r.bounds.1 && lon >= r.bounds.2 && lon <= r.bounds.3).map(|r| r.name)
}

/// The nearest allowed distance to the one asked.
pub fn snap_km(k: f64) -> f64 {
    KMS.iter().copied().min_by(|a, b| (a - k).abs().total_cmp(&(b - k).abs())).unwrap_or(10.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Watch {
    pub id: String,
    pub watcher: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub km: f64,
    pub region: String,
    pub closer: bool,
    pub power: bool,
    /// A clear night over it, by GOES's cloud mask.
    pub sky: bool,
    /// A new picture of its place that looks unlike the one before.
    pub change: bool,
    pub place: String,
    pub created: String,
}

fn flag(r: &Row, k: &str) -> bool {
    match r.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

impl Watch {
    pub fn from_row(r: &Row) -> Watch {
        Watch {
            id: r.s("id"),
            watcher: r.s("watcher"),
            name: r.s("name"),
            lat: r.f("lat"),
            lon: r.f("lon"),
            km: r.f("km"),
            region: r.s("region"),
            closer: flag(r, "closer"),
            power: flag(r, "power"),
            sky: flag(r, "sky"),
            change: flag(r, "change"),
            place: r.s("place"),
            created: r.s("created"),
        }
    }

    /// The row as stored.
    pub fn doc(&self) -> Value {
        json!({"id": self.id, "watcher": self.watcher, "name": self.name, "lat": self.lat, "lon": self.lon, "km": self.km,
               "region": self.region, "closer": self.closer, "power": self.power, "sky": self.sky, "change": self.change, "place": self.place, "created": self.created})
    }

    /// As the page sees it: not the watcher's key.
    pub fn public(&self) -> Value {
        let mut v = self.doc();
        if let Some(o) = v.as_object_mut() {
            o.remove("watcher");
        }
        v
    }
}

/// A fire pixel: its scan's time, where, its power and its fire.
#[derive(Clone, Debug)]
pub struct Px {
    pub t: i64,
    pub lat: f64,
    pub lon: f64,
    pub mw: f64,
    pub fire: String,
}

pub fn pixels(rows: &[Row]) -> Vec<Px> {
    rows.iter()
        .filter_map(|r| Some(Px { t: crate::pages::unix(&r.s("ts"))?, lat: r.f("lat"), lon: r.f("lon"), mw: r.f("mw"), fire: r.s("fire") }))
        .collect()
}

/// The nearest fire to a place in the hour up to a moment.
#[derive(Clone, Debug)]
pub struct Near {
    pub km: f64,
    pub dir: &'static str,
    pub lat: f64,
    pub lon: f64,
    pub fire: String,
    pub t: i64,
}

const WORDS: [(&str, &str); 8] = [("N", "north"), ("NE", "north-east"), ("E", "east"), ("SE", "south-east"), ("S", "south"), ("SW", "south-west"), ("W", "west"), ("NW", "north-west")];

pub fn direction(lat: f64, lon: f64, lat2: f64, lon2: f64) -> &'static str {
    let c = crate::geo::compass(lat, lon, lat2, lon2);
    WORDS.iter().find(|(k, _)| *k == c).map_or("", |w| w.1)
}

pub fn nearest(lat: f64, lon: f64, px: &[Px], at: i64) -> Option<Near> {
    px.iter()
        .filter(|p| p.t <= at && p.t > at - 3600)
        .map(|p| (km(lat, lon, p.lat, p.lon), p))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(d, p)| Near { km: d, dir: direction(lat, lon, p.lat, p.lon), lat: p.lat, lon: p.lon, fire: p.fire.clone(), t: p.t })
}

/// How near the same fire was about an hour before a moment (from 40 to 90
/// minutes before): none when it was not burning then, a new fire rather
/// than one coming closer.
pub fn hour_before(lat: f64, lon: f64, px: &[Px], fire: &str, at: i64) -> Option<f64> {
    px.iter().filter(|p| p.fire == fire && p.t > at - 5400 && p.t <= at - 2400).map(|p| km(lat, lon, p.lat, p.lon)).min_by(|a, b| a.total_cmp(b))
}

/// A fire's power in one scan.
fn power_of(px: &[Px], fire: &str, t: i64) -> f64 {
    px.iter().filter(|p| p.t == t && p.fire == fire).map(|p| p.mw).sum()
}

fn n0(x: f64) -> String {
    crate::rules::fmt_int(x.round())
}

fn n1(x: f64) -> String {
    if x < 10.0 {
        format!("{x:.1}")
    } else {
        n0(x)
    }
}

/// Where a place stands at a moment: the nearest fire of the last hour and
/// how that changed in the hour before.
pub fn status(w: &Watch, px: &[Px], now: i64) -> Value {
    let Some(nb) = nearest(w.lat, w.lon, px, now).filter(|n| n.km <= FAR_KM) else {
        return json!({"sev": "clear", "pill": "Clear", "say": format!("No fire within {} km.", n0(FAR_KM)), "sub": "", "km": null});
    };
    let prev = nearest(w.lat, w.lon, px, now - 3600);
    let trend = match &prev {
        Some(p) if p.km - nb.km > 0.5 => format!("It came {} km closer in the last hour.", n1(p.km - nb.km)),
        Some(p) if nb.km - p.km > 0.5 => format!("It moved {} km away in the last hour.", n1(nb.km - p.km)),
        Some(_) => "About as far as an hour ago.".to_string(),
        None => "First seen in the last hour.".to_string(),
    };
    let (sev, pill, say) = if nb.km <= HERE_KM {
        ("alert", "Fire here".to_string(), "Fire pixels over this place.".to_string())
    } else if nb.km <= w.km {
        ("alert", format!("Within {} km", n0(w.km)), format!("Fire {} km {}.", n1(nb.km), nb.dir))
    } else {
        ("warn", "Nearby".to_string(), format!("Fire {} km {}.", n1(nb.km), nb.dir))
    };
    json!({"sev": sev, "pill": pill, "say": say, "sub": trend, "km": (nb.km * 10.0).round() / 10.0, "dir": nb.dir,
           "fire": nb.fire, "lat": nb.lat, "lon": nb.lon, "seen": crate::pages::iso(nb.t),
           "prev_km": prev.map(|p| (p.km * 10.0).round() / 10.0)})
}

/// What a watch raises over some scans: each alert once per watch, fire
/// and kind (a fire 2 km closer, once an hour). `seen` holds the alerts
/// already raised; the new ones are added to it.
pub fn alerts(w: &Watch, scans: &[i64], px: &[Px], seen: &mut HashSet<String>) -> Vec<Value> {
    let mut out = Vec::new();
    let name = w.name.clone();
    for &s in scans {
        let Some(nb) = nearest(w.lat, w.lon, px, s) else { continue };
        let mut add = |key: String, kind: &str, title: String, body: String| {
            let id = format!("{}-{key}", w.id);
            if seen.insert(id.clone()) {
                out.push(json!({"id": id, "watch": w.id, "watcher": w.watcher, "region": w.region, "kind": kind,
                    "t": crate::pages::iso(s), "sent": crate::pages::iso(chrono::Utc::now().timestamp()),
                    "title": title, "body": body, "km": (nb.km * 10.0).round() / 10.0, "dir": nb.dir,
                    "fire": nb.fire, "lat": nb.lat, "lon": nb.lon, "name": name}));
            }
        };
        if nb.km <= w.km {
            add(format!("in-{}", nb.fire), "in", format!("Fire {} km {} of {name}", n1(nb.km), nb.dir), format!("You asked to hear about fires within {} km.", n0(w.km)));
        }
        if nb.km <= 5.0 && w.km > 5.0 {
            add(format!("5-{}", nb.fire), "near", format!("Fire within 5 km of {name}"), format!("{} km {}.", n1(nb.km), nb.dir));
        }
        if nb.km <= HERE_KM {
            add(format!("here-{}", nb.fire), "here", format!("Fire pixels over {name}"), format!("The nearest is {} km {}: a GOES pixel is 2 to 3 km across, so the fire may be on the place.", n1(nb.km), nb.dir));
        }
        if w.power && nb.km <= FAR_KM {
            let mw = power_of(px, &nb.fire, s);
            if mw >= BIG_MW {
                add(format!("mw-{}", nb.fire), "power", format!("A fire {} km {} of {name} passed 1,000 MW", n1(nb.km), nb.dir), format!("{} MW in one scan: a large, intense fire.", n0(mw)));
            }
        }
        if w.closer && nb.km > HERE_KM && nb.km <= FAR_KM {
            if let Some(p) = hour_before(w.lat, w.lon, px, &nb.fire, s) {
                if p - nb.km >= CLOSER_KM {
                    add(format!("closer-{}", s / 3600), "closer", format!("The fire came {} km closer to {name} in an hour", n1(p - nb.km)), format!("Now {} km {}.", n1(nb.km), nb.dir));
                }
            }
        }
    }
    out
}

/// A clear night over a watch's place: dark, the last scan clear, and
/// most of the night's scans so far clear. Once a night.
pub fn clear_night(w: &Watch, sky: &Row, seen: &mut HashSet<String>) -> Option<Value> {
    let tonight = sky.get("nights").and_then(Value::as_array).and_then(|a| a.last()).and_then(Value::as_i64).unwrap_or(-1);
    if sky.f("sun") > crate::sky::NIGHT_SUN || sky.f("clear") < 0.9 || sky.i("nn") < 2 || tonight < crate::sky::GOOD {
        return None;
    }
    let id = format!("{}-clear-{}", w.id, sky.i("day_end"));
    if !seen.insert(id.clone()) {
        return None;
    }
    let t = sky.s("t");
    Some(json!({"id": id, "watch": w.id, "watcher": w.watcher, "region": w.region, "kind": "clear", "t": t,
        "sent": crate::pages::iso(chrono::Utc::now().timestamp()),
        "title": format!("A clear night over {}", w.name), "body": format!("{tonight}% of tonight's scans so far were clear over it, and the last one at least 90%."),
        "km": null, "dir": "", "fire": "", "lat": w.lat, "lon": w.lon, "name": w.name}))
}

/// A new picture of a watch's place unlike the one before it: told once.
pub fn look_changed(w: &Watch, new: &Row, before: &Row, seen: &mut HashSet<String>) -> Option<Value> {
    let id = format!("{}-look-{}", w.id, new.s("id"));
    let (t1, t0) = (crate::pages::unix(&new.s("taken"))?, crate::pages::unix(&before.s("taken"))?);
    if t1 <= t0 || t1 - t0 > CHANGE_GAP_S || seen.contains(&id) {
        return None;
    }
    let step = 1.0 - cosine(&look(new), &look(before))?;
    if step < CHANGE_STEP {
        return None;
    }
    seen.insert(id.clone());
    let day = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%-d %B %Y").to_string()).unwrap_or_default();
    let (l1, l0) = (new.s("label"), before.s("label"));
    let read = if l1 != l0 { format!(" It reads as {l1}, and before as {l0}.") } else { String::new() };
    Some(json!({"id": id, "watch": w.id, "watcher": w.watcher, "region": w.region, "kind": "look", "t": iso(t1),
        "sent": iso(chrono::Utc::now().timestamp()),
        "title": format!("{} looks different", w.name),
        "body": format!("Sentinel-2's picture of {} is unlike the one of {} (it moved {:.2}; most new pictures move under {CHANGE_STEP}).{read}", day(t1), day(t0), step),
        "km": null, "dir": "", "fire": "", "lat": w.lat, "lon": w.lon, "name": w.name}))
}

fn look(r: &Row) -> Vec<f64> {
    r.get("look").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
}

fn cosine(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.is_empty() || a.len() != b.len() {
        return None;
    }
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    (aa > 0.0 && bb > 0.0).then(|| ab / (aa.sqrt() * bb.sqrt()))
}

/// The pictures already judged for a watch (its id and the picture's), so
/// a new picture's vectors are read once.
static JUDGED: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);

/// The nearest fire, scan by scan: (time, km within `FAR_KM`, or none).
pub fn track(w: &Watch, scans: &[i64], px: &[Px]) -> Vec<Value> {
    scans
        .iter()
        .map(|&s| {
            let d = nearest(w.lat, w.lon, px, s).map(|n| n.km).filter(|k| *k <= FAR_KM);
            json!([s, d.map(|k| (k * 10.0).round() / 10.0)])
        })
        .collect()
}

/// A box around a point, `r` km on each side: (south, north, west, east).
pub fn bbox(lat: f64, lon: f64, r: f64) -> (f64, f64, f64, f64) {
    let dlat = r / 111.0;
    let dlon = r / (111.0 * lat.to_radians().cos().abs().max(0.2));
    (lat - dlat, lat + dlat, lon - dlon, lon + dlon)
}

/// The pixels within `r` km of a point between two moments.
pub async fn pixels_near(db: &Strata, region: &str, lat: f64, lon: f64, r: f64, from: i64, to: i64) -> crate::strata::Result<Vec<Px>> {
    let (s, n, w, e) = bbox(lat, lon, r);
    let rows = db
        .scroll(
            "SELECT ts, lat, lon, mw, fire FROM ea_detections WHERE region = :r AND ts > :a AND ts <= :b AND lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e",
            json!({"r": region, "a": crate::pages::iso(from), "b": crate::pages::iso(to), "s": s, "n": n, "w": w, "e": e}),
            10_000,
        )
        .await?;
    Ok(pixels(&rows))
}

/// The scans of a region between two moments.
pub async fn scans(db: &Strata, region: &str, from: i64, to: i64) -> crate::strata::Result<Vec<i64>> {
    let rows = db
        .query("SELECT ts FROM ea_scans WHERE region = :r AND ts > :a AND ts <= :b ORDER BY ts LIMIT 2000", json!({"r": region, "a": crate::pages::iso(from), "b": crate::pages::iso(to)}))
        .await?;
    let mut t: Vec<i64> = rows.iter().filter_map(|r| crate::pages::unix(&r.s("ts"))).collect();
    t.dedup();
    Ok(t)
}

/// One look at a region: the scans after `since` (the last hour's, the
/// first time), each watch judged on each. Returns the newest scan seen
/// and the alerts written.
pub async fn check(db: &Strata, region: &str, since: i64) -> Result<(i64, usize)> {
    let newest = db.query("SELECT MAX(ts) AS t FROM ea_scans WHERE region = :r", json!({"r": region})).await.map_err(|e| anyhow!("{e}"))?;
    let now = newest.first().and_then(|r| crate::pages::unix(&r.s("t"))).unwrap_or(0);
    if now <= since || now == 0 {
        return Ok((since.max(now), 0));
    }
    // After a pause, at most the last six hours: alerts of a fire long
    // past help no one.
    let from = if since > 0 { since.max(now - 6 * 3600) } else { now - 3600 };
    let n = judge(db, region, from, now).await?;
    Ok((now, n))
}

/// Each watch of a region judged on the scans after `from` up to `now`;
/// what it raises written once. Returns the alerts written.
pub async fn judge(db: &Strata, region: &str, from: i64, now: i64) -> Result<usize> {
    let watches: Vec<Watch> = db
        .scroll("SELECT * FROM ea_watches WHERE region = :r", json!({"r": region}), 1000)
        .await
        .map_err(|e| anyhow!("{e}"))?
        .iter()
        .map(Watch::from_row)
        .collect();
    if watches.is_empty() {
        return Ok(0);
    }
    let mut seen: HashSet<String> = db
        .scroll("SELECT id FROM ea_watch_alerts WHERE region = :r AND t >= :a", json!({"r": region, "a": crate::pages::iso(now - KEEP_S)}), 10_000)
        .await
        .map_err(|e| anyhow!("{e}"))?
        .iter()
        .map(|r| r.s("id"))
        .collect();
    let mut out = Vec::new();
    // Clear nights, from the sky over each watch's place.
    let ids: Vec<String> = watches.iter().filter(|w| w.sky && !w.place.is_empty()).map(|w| w.place.clone()).collect();
    if !ids.is_empty() {
        let rows = db.query("SELECT id, t, clear, sun, nights, nn, day_end FROM ea_sky WHERE id IN (:ids) LIMIT 10000", json!({"ids": ids})).await.map_err(|e| anyhow!("{e}"))?;
        let by: std::collections::HashMap<String, &Row> = rows.iter().map(|r| (r.s("id"), r)).collect();
        for w in watches.iter().filter(|w| w.sky) {
            if let Some(r) = by.get(&w.place) {
                out.extend(clear_night(w, r, &mut seen));
            }
        }
    }
    // A change in how each watch's place looks, from its newest pictures.
    let places: HashSet<String> = watches.iter().filter(|w| w.change && !w.place.is_empty()).map(|w| w.place.clone()).collect();
    if !places.is_empty() {
        let fresh = db
            .query("SELECT id, place, taken FROM ea_years WHERE place IN (:ids) AND taken >= :a LIMIT 10000", json!({"ids": places.iter().collect::<Vec<_>>(), "a": iso(now - CHANGE_BACK_S)}))
            .await
            .map_err(|e| anyhow!("{e}"))?;
        for r in &fresh {
            let todo: Vec<&Watch> = {
                let mut j = JUDGED.lock().unwrap_or_else(|e| e.into_inner());
                let j = j.get_or_insert_with(HashSet::new);
                watches.iter().filter(|w| w.change && w.place == r.s("place") && j.insert(format!("{}/{}", w.id, r.s("id")))).collect()
            };
            if todo.is_empty() {
                continue;
            }
            let two = db
                .query("SELECT id, taken, label, look FROM ea_years WHERE place = :p AND taken <= :t ORDER BY taken DESC LIMIT 2", json!({"p": r.s("place"), "t": r.s("taken")}))
                .await
                .map_err(|e| anyhow!("{e}"))?;
            if let [new, before] = &two[..] {
                for w in todo {
                    out.extend(look_changed(w, new, before, &mut seen));
                }
            }
        }
    }
    let scans = scans(db, region, from, now).await.map_err(|e| anyhow!("{e}"))?;
    // The pixels from an hour before the first scan judged (for "closer"),
    // across the region: a few thousand at most on a bad day.
    let rows = db
        .scroll(
            "SELECT ts, lat, lon, mw, fire FROM ea_detections WHERE region = :r AND ts > :a AND ts <= :b",
            json!({"r": region, "a": crate::pages::iso(from - 2 * 3600), "b": crate::pages::iso(now)}),
            10_000,
        )
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let px = pixels(&rows);
    for w in &watches {
        // Only the pixels that could matter to this watch.
        let (s, n, we, e) = bbox(w.lat, w.lon, FAR_KM + 5.0);
        let near: Vec<Px> = px.iter().filter(|p| p.lat >= s && p.lat <= n && p.lon >= we && p.lon <= e).cloned().collect();
        if !near.is_empty() {
            out.extend(alerts(w, &scans, &near, &mut seen));
        }
    }
    let n = out.len();
    if n > 0 {
        db.upsert("ea_watch_alerts", out).await.map_err(|e| anyhow!("{e}"))?;
    }
    Ok(n)
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A watcher's alerts as an Atom feed, newest first.
pub fn atom(base: &str, watcher: &str, alerts: &[Row]) -> String {
    let updated = alerts.first().map(|a| a.s("t")).filter(|t| !t.is_empty()).unwrap_or_else(|| "2026-01-01T00:00:00Z".to_string());
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    out.push_str("  <title>Earth fire watch</title>\n");
    out.push_str(&format!("  <id>{}/v1/watches/feed/{}</id>\n", xml(base), xml(watcher)));
    out.push_str(&format!("  <link rel=\"alternate\" href=\"{}/#/watch\"/>\n", xml(base)));
    out.push_str(&format!("  <updated>{}</updated>\n", xml(&updated)));
    out.push_str("  <author><name>Earth, from NOAA GOES fire pixels</name></author>\n");
    for a in alerts {
        out.push_str("  <entry>\n");
        out.push_str(&format!("    <id>{}/alert/{}</id>\n", xml(base), xml(&a.s("id"))));
        out.push_str(&format!("    <title>{}</title>\n", xml(&a.s("title"))));
        out.push_str(&format!("    <updated>{}</updated>\n", xml(&a.s("t"))));
        let fire = a.s("fire");
        if !fire.is_empty() {
            out.push_str(&format!("    <link rel=\"alternate\" href=\"{}/#/fire/{}\"/>\n", xml(base), xml(&fire)));
        }
        out.push_str(&format!("    <summary>{}</summary>\n", xml(&a.s("body"))));
        out.push_str("  </entry>\n");
    }
    out.push_str("</feed>\n");
    out
}

// ------------------------------------------------------------------ pages

type R<T> = std::result::Result<T, ApiError>;

fn bad(m: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, m.into())
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// A name as a person typed it: trimmed, no control characters, at most 60
/// letters.
pub fn clean_name(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(60).collect()
}

fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    let a = h.finish();
    let mut h2 = std::collections::hash_map::RandomState::new().build_hasher();
    h2.write_u64(a);
    format!("w{:016x}{:08x}", a, h2.finish() as u32)
}

/// The place under a point, as a watch's card and page say it.
pub fn place_facts(places: &Places, lat: f64, lon: f64) -> Value {
    match places.nearest(lat, lon, 3.0) {
        Some(p) => json!({"id": p.id, "town": p.town, "adm1": p.adm1, "adm2": p.adm2, "kind": p.kind, "trees": p.trees, "shrubs": p.shrubs,
            "grass": p.grass, "built": p.built, "slope": p.slope, "elev": p.elev, "people": p.people.round(), "climate": p.climate, "fire_years": p.fire_years}),
        None => Value::Null,
    }
}

/// Where a point is, in words: "Paradise, Butte County" or its coordinates.
pub fn where_words(places: &Places, lat: f64, lon: f64) -> String {
    match places.nearest(lat, lon, 8.0) {
        Some(p) => {
            let mut bits: Vec<&str> = vec![];
            for b in [p.town.as_str(), p.adm2.as_str(), p.adm1.as_str()] {
                if !b.is_empty() && !bits.contains(&b) {
                    bits.push(b);
                }
            }
            bits.truncate(2);
            bits.join(", ")
        }
        None => format!("{lat:.3}, {lon:.3}"),
    }
}

/// Each region's newest scan, and whether the feed is live (a scan in the
/// last half hour).
async fn clocks(c: &Ctx<'_>, regions: &[String]) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for r in regions {
        if !out.iter().any(|(x, _): &(String, i64)| x == r) {
            out.push((r.clone(), c.clock(r).await));
        }
    }
    out
}

fn clock_json(clocks: &[(String, i64)]) -> Value {
    let wall = chrono::Utc::now().timestamp();
    let mut m = serde_json::Map::new();
    for (r, t) in clocks {
        m.insert(r.clone(), json!({"now": if *t > 0 { Value::String(iso(*t)) } else { Value::Null }, "live": *t > 0 && wall - t < 1800}));
    }
    Value::Object(m)
}

async fn own(c: &Ctx<'_>, watcher: &str, id: &str) -> R<Watch> {
    if !key_ok(watcher) {
        return Err(bad("a watcher's key is 20 to 64 letters, digits and dashes"));
    }
    let rows = c.rows("the watch", "SELECT * FROM ea_watches WHERE id = :id", json!({"id": id})).await?;
    match rows.first().map(Watch::from_row) {
        Some(w) if w.watcher == watcher => Ok(w),
        _ => Err(ApiError(StatusCode::NOT_FOUND, format!("no watch {id} of yours"))),
    }
}

/// A watcher's watches, each where it stands now, and their alerts.
pub async fn mine(c: &Ctx<'_>, watcher: &str) -> R<Value> {
    if !key_ok(watcher) {
        return Err(bad("a watcher's key is 20 to 64 letters, digits and dashes"));
    }
    let (watches, alerts) = tokio::join!(
        c.rows("your watches", "SELECT * FROM ea_watches WHERE watcher = :w ORDER BY created LIMIT 50", json!({"w": watcher})),
        c.rows("your alerts", "SELECT id, watch, kind, t, sent, title, body, km, dir, fire, lat, lon, name, region FROM ea_watch_alerts WHERE watcher = :w ORDER BY t DESC LIMIT 60", json!({"w": watcher})),
    );
    let watches: Vec<Watch> = watches?.iter().map(Watch::from_row).collect();
    let alerts = alerts?;
    let regions: Vec<String> = watches.iter().map(|w| w.region.clone()).collect();
    let clocks = clocks(c, &regions).await;
    let mut out = Vec::new();
    for (r, now) in &clocks {
        let rows = c.all("the fire pixels of the last two hours", "SELECT ts, lat, lon, mw, fire FROM ea_detections WHERE region = :r AND ts > :a", json!({"r": r, "a": iso(now - 7200)})).await;
        let px = pixels(&rows);
        for w in watches.iter().filter(|w| &w.region == r) {
            let mut v = w.public();
            v["status"] = status(w, &px, *now);
            v["where"] = json!(where_words(c.places, w.lat, w.lon));
            v["alerts"] = json!(alerts.iter().filter(|a| a.s("watch") == w.id).count());
            out.push(v);
        }
    }
    // As they were made.
    out.sort_by_key(|v| v["created"].as_str().unwrap_or("").to_string());
    Ok(json!({"watches": out, "alerts": alerts, "regions": clock_json(&clocks), "max": MAX_PER_WATCHER, "kms": KMS}))
}

#[derive(serde::Deserialize, Default)]
pub struct NewWatch {
    #[serde(default)]
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub km: Option<f64>,
    pub closer: Option<bool>,
    pub power: Option<bool>,
    pub sky: Option<bool>,
    pub change: Option<bool>,
}

pub async fn create(c: &Ctx<'_>, watcher: &str, b: NewWatch) -> R<Value> {
    if !key_ok(watcher) {
        return Err(bad("a watcher's key is 20 to 64 letters, digits and dashes"));
    }
    if !b.lat.is_finite() || !b.lon.is_finite() {
        return Err(bad("a watch needs a lat and a lon"));
    }
    let Some(region) = region_of(b.lat, b.lon) else {
        return Err(bad("Earth watches California and Chile: this point is in neither"));
    };
    let (n_mine, n_all) = tokio::join!(
        c.rows("how many watches you keep", "SELECT COUNT(*) AS n FROM ea_watches WHERE watcher = :w", json!({"w": watcher})),
        c.rows("how many watches there are", "SELECT COUNT(*) AS n FROM ea_watches", json!({})),
    );
    if n_mine?.first().map_or(0, |r| r.i("n")) as usize >= MAX_PER_WATCHER {
        return Err(bad(format!("a browser keeps at most {MAX_PER_WATCHER} watches: remove one first")));
    }
    if n_all?.first().map_or(0, |r| r.i("n")) as usize >= MAX_WATCHES {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "Earth keeps as many watches as it can check: try again later".into()));
    }
    let mut name = clean_name(&b.name);
    if name.is_empty() {
        name = where_words(c.places, b.lat, b.lon);
    }
    let w = Watch {
        id: new_id(),
        watcher: watcher.to_string(),
        name,
        lat: (b.lat * 1e5).round() / 1e5,
        lon: (b.lon * 1e5).round() / 1e5,
        km: snap_km(b.km.unwrap_or(10.0)),
        region: region.to_string(),
        closer: b.closer.unwrap_or(true),
        power: b.power.unwrap_or(true),
        sky: b.sky.unwrap_or(false),
        change: b.change.unwrap_or(false),
        place: c.places.nearest(b.lat, b.lon, 3.0).map(|p| p.id.clone()).unwrap_or_default(),
        created: iso(chrono::Utc::now().timestamp()),
    };
    c.db.upsert("ea_watches", vec![w.doc()]).await?;
    c.trace.push("the watch, written", "upsert into ea_watches", &json!({"id": w.id}), 1, 0.0, None);
    Ok(json!({"watch": w.public()}))
}

#[derive(serde::Deserialize, Default)]
pub struct Change {
    pub name: Option<String>,
    pub km: Option<f64>,
    pub closer: Option<bool>,
    pub power: Option<bool>,
    pub sky: Option<bool>,
    pub change: Option<bool>,
    #[serde(default)]
    pub remove: bool,
}

pub async fn change(c: &Ctx<'_>, watcher: &str, id: &str, b: Change) -> R<Value> {
    let mut w = own(c, watcher, id).await?;
    if b.remove {
        let gone = c.soft("its alerts", "SELECT id FROM ea_watch_alerts WHERE watch = :id LIMIT 10000", json!({"id": id})).await;
        let mut ops = vec![json!({"op": "delete", "table": "ea_watches", "key": id})];
        ops.extend(gone.iter().map(|r| json!({"op": "delete", "table": "ea_watch_alerts", "key": r.s("id")})));
        c.db.write(ops).await?;
        return Ok(json!({"removed": id}));
    }
    if let Some(n) = b.name {
        let n = clean_name(&n);
        if !n.is_empty() {
            w.name = n;
        }
    }
    if let Some(k) = b.km {
        w.km = snap_km(k);
    }
    if let Some(x) = b.closer {
        w.closer = x;
    }
    if let Some(x) = b.power {
        w.power = x;
    }
    if let Some(x) = b.sky {
        w.sky = x;
    }
    if let Some(x) = b.change {
        w.change = x;
    }
    c.db.upsert("ea_watches", vec![w.doc()]).await?;
    Ok(json!({"watch": w.public()}))
}

/// Heat for a map: each spot once, at its newest.
fn spots(px: &[Px]) -> Vec<Value> {
    let mut m: std::collections::HashMap<(i64, i64), &Px> = std::collections::HashMap::new();
    for p in px {
        let k = ((p.lat * 1000.0).round() as i64, (p.lon * 1000.0).round() as i64);
        if m.get(&k).is_none_or(|o| o.t < p.t) {
            m.insert(k, p);
        }
    }
    let mut v: Vec<&Px> = m.into_values().collect();
    v.sort_by_key(|p| p.t);
    v.iter().map(|p| json!({"lat": p.lat, "lon": p.lon, "mw": p.mw, "ts": iso(p.t), "fire": p.fire})).collect()
}

/// The fires GOES saw start within 25 km of a point since 2023.
async fn fires_near(c: &Ctx<'_>, region: &str, lat: f64, lon: f64) -> Vec<Value> {
    let (s, n, w, e) = bbox(lat, lon, 25.0);
    let rows = c
        .soft(
            "the fires that started within 25 km",
            "SELECT id, name, start, area_km2, peak_mw, hours, lat, lon, state FROM ea_fires WHERE region = :r AND lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e AND pixels >= 3 ORDER BY start DESC LIMIT 60",
            json!({"r": region, "s": s, "n": n, "w": w, "e": e}),
        )
        .await;
    rows.into_iter()
        .filter_map(|r| {
            let d = km(lat, lon, r.f("lat"), r.f("lon"));
            (d <= 25.0).then(|| {
                let mut v = Value::Object(r.clone());
                v["km"] = json!(r1(d));
                v["dir"] = json!(direction(lat, lon, r.f("lat"), r.f("lon")));
                v
            })
        })
        .take(12)
        .collect()
}

/// One watch: where it stands, the nearest fire scan by scan over the last
/// 12 hours, its alerts, the heat near it, the place and the fires that
/// started near it.
pub async fn one(c: &Ctx<'_>, watcher: &str, id: &str) -> R<Value> {
    let w = own(c, watcher, id).await?;
    let now = c.clock(&w.region).await;
    let from = now - 12 * 3600;
    let t0 = std::time::Instant::now();
    let (px, sc) = tokio::join!(pixels_near(c.db, &w.region, w.lat, w.lon, FAR_KM + 5.0, from - 3600, now), scans(c.db, &w.region, from, now));
    let (px, sc) = (px?, sc?);
    c.trace.push("the fire pixels within 55 km, 13 hours", "SELECT ts, lat, lon, mw, fire FROM ea_detections WHERE region = :r AND ts > :a AND ts <= :b AND lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e", &json!({"r": w.region}), px.len(), t0.elapsed().as_secs_f64() * 1000.0, None);
    let (alerts, fires) = tokio::join!(
        c.rows("its alerts", "SELECT id, kind, t, sent, title, body, km, dir, fire, lat, lon FROM ea_watch_alerts WHERE watch = :id ORDER BY t DESC LIMIT 100", json!({"id": id})),
        fires_near(c, &w.region, w.lat, w.lon),
    );
    let recent: Vec<Px> = px.iter().filter(|p| p.t > now - 3 * 3600).cloned().collect();
    Ok(json!({
        "watch": w.public(), "where": where_words(c.places, w.lat, w.lon), "status": status(&w, &px, now),
        "now": iso(now), "regions": clock_json(&[(w.region.clone(), now)]),
        "track": track(&w, &sc, &px), "alerts": alerts?, "pixels": spots(&recent),
        "place": place_facts(c.places, w.lat, w.lon), "fires": fires, "kms": KMS,
    }))
}

/// The example: the Park Fire's first 12 hours (24 July 2024, near Chico),
/// as three made-up watches would have lived them.
pub const EXAMPLE_T0: &str = "2024-07-24T22:01:17Z";
pub const EXAMPLE_FIRE: &str = "ca-20240724T2201-39.79-121.76";
pub const EXAMPLE: [(&str, &str, f64, f64, f64); 3] = [
    ("mom", "Mom's house", 39.866, -121.745, 15.0),
    ("cohasset", "Cabin in Cohasset", 39.8755, -121.771, 10.0),
    ("paradise", "Our house in Paradise", 39.7596, -121.6219, 10.0),
];

pub async fn example(c: &Ctx<'_>) -> R<Value> {
    let t0 = crate::pages::unix(EXAMPLE_T0).unwrap_or(0);
    let to = t0 + 12 * 3600;
    let watches: Vec<Watch> = EXAMPLE
        .iter()
        .map(|(id, name, lat, lon, k)| Watch { id: id.to_string(), watcher: String::new(), name: name.to_string(), lat: *lat, lon: *lon, km: *k, region: "california".into(), closer: true, power: true, sky: false, change: false, place: String::new(), created: String::new() })
        .collect();
    let (la, lo) = (39.83, -121.71);
    let tq = std::time::Instant::now();
    let (px, sc) = tokio::join!(pixels_near(c.db, "california", la, lo, FAR_KM + 15.0, t0 - 3 * 3600, to), scans(c.db, "california", t0 - 1800, to));
    let (px, sc) = (px?, sc?);
    c.trace.push("the fire pixels near Chico, 24 July 2024", "SELECT ts, lat, lon, mw, fire FROM ea_detections WHERE region = 'california' AND ts > :a AND ts <= :b AND lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e", &json!({"a": iso(t0 - 3 * 3600), "b": iso(to)}), px.len(), tq.elapsed().as_secs_f64() * 1000.0, None);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for w in &watches {
        let mut v = w.public();
        v["where"] = json!(where_words(c.places, w.lat, w.lon));
        v["track"] = json!(track(w, &sc, &px));
        v["frames"] = json!(sc.iter().map(|&s| status(w, &px, s)).collect::<Vec<_>>());
        let mut a = alerts(w, &sc, &px, &mut seen);
        for x in &mut a {
            if let Some(o) = x.as_object_mut() {
                o.remove("watcher");
                o.remove("sent");
            }
        }
        v["alerts"] = json!(a);
        v["place"] = place_facts(c.places, w.lat, w.lon);
        out.push(v);
    }
    let heat: Vec<Value> = px.iter().filter(|p| p.t >= t0 - 1800).map(|p| json!([p.t - t0, p.lat, p.lon, p.mw])).collect();
    Ok(json!({"t0": EXAMPLE_T0, "fire": EXAMPLE_FIRE, "scans": sc.iter().map(|s| s - t0).collect::<Vec<_>>(), "watches": out, "pixels": heat, "kms": KMS}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(km: f64) -> Watch {
        Watch { id: "w1".into(), watcher: "k".repeat(24), name: "the house".into(), lat: 39.866, lon: -121.745, km, region: "california".into(), closer: true, power: true, sky: true, change: false, place: String::new(), created: String::new() }
    }
    fn p(t: i64, lat: f64, lon: f64, mw: f64) -> Px {
        Px { t, lat, lon, mw, fire: "f1".into() }
    }

    #[test]
    fn keys_and_regions() {
        assert!(key_ok(&"a1-".repeat(8)));
        assert!(!key_ok("short"));
        assert!(!key_ok(&format!("{}/x", "a".repeat(30))));
        assert_eq!(region_of(39.8, -121.7), Some("california"));
        assert_eq!(region_of(-33.0, -71.5), Some("chile"));
        assert_eq!(region_of(48.8, 2.3), None);
        assert_eq!(snap_km(12.0), 10.0);
        assert_eq!(snap_km(100.0), 50.0);
    }

    #[test]
    fn a_fire_coming_closer_raises_each_alert_once() {
        let w = w(15.0);
        // A fire 20 km south, then 12, then 4, then over the place, a scan
        // every 20 minutes.
        let px = vec![
            p(0, 39.866 - 0.18, -121.745, 200.0),
            p(1200, 39.866 - 0.108, -121.745, 400.0),
            p(2400, 39.866 - 0.036, -121.745, 1200.0),
            p(3600, 39.866 - 0.005, -121.745, 900.0),
            p(4800, 39.866 - 0.005, -121.745, 900.0),
        ];
        let scans = [0, 1200, 2400, 3600, 4800];
        let mut seen = HashSet::new();
        let got = alerts(&w, &scans, &px, &mut seen);
        let kinds: Vec<&str> = got.iter().map(|a| a["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["in", "near", "power", "closer", "here"], "{got:#?}");
        assert_eq!(got[0]["title"], "Fire 12 km south of the house");
        // Judged again, nothing new.
        assert!(alerts(&w, &scans, &px, &mut seen).is_empty());
        let st = status(&w, &px, 4800);
        assert_eq!(st["pill"], "Fire here");
        assert_eq!(st["sev"], "alert");
        let far = status(&w, &px, 20000);
        assert_eq!(far["pill"], "Clear");
        let tr = track(&w, &scans, &px);
        assert_eq!(tr.len(), 5);
        assert_eq!(tr[0][1], 20.0);
    }

    #[test]
    fn a_clear_night_once() {
        let mut w = w(10.0);
        w.place = "p1".into();
        let mut r = Row::new();
        for (k, v) in [("t", json!("2026-11-20T08:00:00Z")), ("clear", json!(1.0)), ("sun", json!(-40.0)), ("nn", json!(3)), ("day_end", json!(20776)), ("nights", json!([-1, 90]))] {
            r.insert(k.into(), v);
        }
        let mut seen = HashSet::new();
        let a = clear_night(&w, &r, &mut seen).expect("a clear night");
        assert_eq!(a["kind"], "clear");
        assert!(clear_night(&w, &r, &mut seen).is_none(), "once a night");
        r.insert("sun".into(), json!(10.0));
        assert!(clear_night(&w, &r, &mut HashSet::new()).is_none(), "not by day");
    }

    #[test]
    fn the_feed_is_atom() {
        let mut a = Row::new();
        a.insert("id".into(), json!("w1-in-f1"));
        a.insert("title".into(), json!("Fire 9 km south of <home> & barn"));
        a.insert("t".into(), json!("2024-07-24T22:31:17Z"));
        a.insert("fire".into(), json!("f1"));
        a.insert("body".into(), json!("You asked"));
        let x = atom("https://earth.chloetech.ai", "key", &[a]);
        assert!(x.contains("<entry>") && x.contains("&lt;home&gt; &amp; barn") && x.contains("#/fire/f1"));
        assert!(x.contains("<updated>2024-07-24T22:31:17Z</updated>"));
    }
}
