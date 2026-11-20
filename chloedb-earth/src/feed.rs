//! Reads GOES's scans over a region into `ea_detections`, `ea_scans` and
//! `ea_fires`: the archive from a time (every scan, or one every so many
//! minutes), then, with `live`, each new scan as NOAA publishes it (a
//! minute or two after it ends). Fires are grouped as the scans come
//! ([`crate::fires::Tracker`]); a restart takes the fires still burning
//! back from the tables, so their ids and first hours carry on.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::fires::{self, Fire, Tracker};
use crate::goes::{self, Pixel, Scan, Source};
use crate::places::Places;
use crate::strata::{RowExt, Strata};

/// A region: its box (south, north, west, east) and the size of a GOES
/// pixel there, about (km²): GOES-West sees California from 137° W and
/// GOES-East Chile from 75° W, each at a slant.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub name: &'static str,
    pub bounds: (f64, f64, f64, f64),
    pub px_km2: f64,
}

pub const REGIONS: [Region; 2] = [
    Region { name: "california", bounds: (32.4, 42.1, -124.6, -114.0), px_km2: 6.5 },
    Region { name: "chile", bounds: (-56.2, -17.4, -76.0, -66.3), px_km2: 6.0 },
];

pub fn region(name: &str) -> Option<Region> {
    REGIONS.iter().copied().find(|r| r.name == name)
}

pub struct Opts {
    pub region: Region,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub live: bool,
    /// Read one scan every this many seconds (0: every scan).
    pub every: i64,
    /// Scans fetched at once.
    pub parallel: usize,
}

fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
}

fn unix(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}

/// What one scan held.
struct Read {
    scan: Scan,
    src: Source,
    pixels: Vec<Pixel>,
    ms: f64,
}

pub struct Feed {
    db: Strata,
    places: Arc<Places>,
    region: Region,
    tracker: Tracker,
    http: reqwest::Client,
    pending: Vec<Value>,
    /// Scans since the fires were last written.
    since_fires: u32,
}

impl Feed {
    /// A feed that reads from `from` on: it takes back the fires burning
    /// up to then.
    pub async fn new(db: Strata, places: Arc<Places>, region: Region, from: i64) -> Result<Feed> {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(15)).build()?;
        let mut f = Feed { db, places, region, tracker: Tracker::new(region.name), http, pending: Vec::new(), since_fires: 0 };
        f.resume(from).await?;
        Ok(f)
    }

    /// Fires the tables say are burning but that were last seen more than
    /// 12 hours before `from`: no scan will come back to them, so they are
    /// out (a replay's, read before the present was).
    pub async fn close_stale(&self, from: i64) -> Result<()> {
        let before = iso(from - 12 * 3600);
        let r = self.db.sql(&format!("UPDATE ea_fires SET state = 'out' WHERE region = '{}' AND state = 'burning' AND last < '{before}'", self.region.name)).await?;
        if let Some(n) = r["written"].as_u64().filter(|n| *n > 0) {
            tracing::info!(region = self.region.name, fires = n, before = %before, "closed fires left burning");
        }
        Ok(())
    }

    /// The fires still burning, back from the tables: their first hours
    /// and the pixels of their last hours. Only those last seen in the 12
    /// hours before `from`: a fire burning later belongs to another stretch
    /// of the archive (a replay read before an older season), and one
    /// burning long before has gone out unseen.
    async fn resume(&mut self, from: i64) -> Result<()> {
        let rows = self
            .db
            .query("SELECT * FROM ea_fires WHERE region = :r AND state = 'burning' AND last <= :b AND last >= :a LIMIT 2000", json!({"r": self.region.name, "a": iso(from - 12 * 3600), "b": iso(from)}))
            .await?;
        for r in rows {
            let (Some(start), Some(last)) = (unix(&r.s("start")), unix(&r.s("last"))) else { continue };
            let mut f = Fire {
                id: r.s("id"),
                region: self.region.name.to_string(),
                start,
                last,
                lat: r.f("lat"),
                lon: r.f("lon"),
                place: Some(r.s("place")).filter(|p| !p.is_empty()),
                cells: Default::default(),
                footprint: Default::default(),
                scans: r.i("scans") as u32,
                peak_px: r.i("peak_px") as u32,
                peak_mw: r.f("peak_mw"),
                mw_now: r.f("mw_now"),
                mw_hist: vec![(last, r.f("mw_now"))],
                energy: r.f("energy") * 1e6,
                spread: r.f("spread_km"),
                bins: serde_json::from_str::<Vec<(f64, u32, f64)>>(&r.s("first")).unwrap_or_default().into_iter().map(|(mw, px, spread)| fires::Bin { mw, px, spread }).collect(),
                seen_start: r.get("seen_start").and_then(Value::as_bool).unwrap_or(false),
                changed: false,
            };
            let dets = self.db.query("SELECT id, lat, lon, ts FROM ea_detections WHERE fire = :f LIMIT 10000", json!({"f": f.id})).await?;
            for d in dets {
                let id = d.s("id");
                let mut parts = id.rsplitn(3, '-');
                if let (Some(c), Some(rw)) = (parts.next().and_then(|x| x.parse().ok()), parts.next().and_then(|x| x.parse().ok())) {
                    f.footprint.insert((rw, c));
                }
                if let Some(t) = unix(&d.s("ts")) {
                    let k = ((d.f("lat") / 0.02).floor() as i32, (d.f("lon") / 0.02).floor() as i32);
                    let e = f.cells.entry(k).or_insert(t);
                    *e = (*e).max(t);
                }
            }
            self.tracker.fires.push(f);
        }
        if !self.tracker.fires.is_empty() {
            tracing::info!(region = self.region.name, burning = self.tracker.fires.len(), "fires taken back");
        }
        Ok(())
    }

    /// The scans to read between two times, one every `every` seconds.
    async fn scans(&self, from: i64, to: i64, every: i64) -> Result<Vec<(Source, Scan)>> {
        let mut out: Vec<(Source, Scan)> = Vec::new();
        let mut h = from - from.rem_euclid(3600);
        // None before the first: `start - i64::MIN` wraps around and would
        // skip every scan.
        let mut last_kept: Option<i64> = None;
        while h <= to {
            let src = goes::source(self.region.name, h);
            let list = match goes::list_hour(&self.http, src, h).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(error = %e, hour = %iso(h), "listing");
                    Vec::new()
                }
            };
            for s in list {
                if s.start < from || s.start > to {
                    continue;
                }
                if every > 0 && last_kept.is_some_and(|k| s.start - k < every - 60) {
                    continue;
                }
                last_kept = Some(s.start);
                out.push((src, s));
            }
            h += 3600;
        }
        Ok(out)
    }

    /// Reads the scans in order, `parallel` fetched at once.
    pub async fn read(&mut self, scans: Vec<(Source, Scan)>, parallel: usize) -> Result<usize> {
        let http = self.http.clone();
        let bounds = self.region.bounds;
        let mut stream = futures_util::stream::iter(scans.into_iter().map(move |(src, scan)| {
            let http = http.clone();
            async move {
                let t0 = std::time::Instant::now();
                let bytes = goes::fetch(&http, src, &scan.key).await?;
                let pixels = tokio::task::spawn_blocking(move || goes::read(bytes, bounds)).await??;
                Ok::<Read, anyhow::Error>(Read { scan, src, pixels, ms: t0.elapsed().as_secs_f64() * 1000.0 })
            }
        }))
        .buffered(parallel.max(1));
        let mut n = 0;
        while let Some(r) = stream.next().await {
            match r {
                Ok(r) => {
                    self.scan(r).await?;
                    n += 1;
                }
                Err(e) => tracing::warn!(error = %e, "a scan"),
            }
        }
        self.flush(true).await?;
        Ok(n)
    }

    /// One scan's pixels as they were read (what `read` does with each
    /// scan it downloads; a test gives its own).
    pub async fn pixels(&mut self, src: Source, scan: Scan, pixels: Vec<Pixel>) -> Result<()> {
        self.scan(Read { scan, src, pixels, ms: 0.0 }).await
    }

    async fn scan(&mut self, mut r: Read) -> Result<()> {
        // Only the region's own pixels: the box GOES is read in holds its
        // neighbours too (Nevada, Argentina), and the sea.
        if !self.places.is_empty() {
            let places = self.places.clone();
            r.pixels.retain(|p| places.nearest(p.lat, p.lon, 4.0).is_some());
        }
        let ts = r.scan.start;
        let ids = self.tracker.scan(ts, r.src.every.max(300), &r.pixels, &self.places);
        let stamp = chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y%j%H%M").to_string()).unwrap_or_default();
        let (mut mw, mut kept) = (0.0, 0);
        for (p, fire) in r.pixels.iter().zip(&ids) {
            // A low-probability pixel of no fire.
            if fire.is_empty() {
                continue;
            }
            kept += 1;
            mw += p.mw.unwrap_or(0.0);
            let place = self.places.nearest(p.lat, p.lon, 3.0).map(|x| x.id.clone());
            self.pending.push(json!({"op": "upsert", "table": "ea_detections", "doc": {
                "id": format!("{}-{stamp}-{}-{}", r.src.sat, p.row, p.col), "ts": iso(ts), "sat": r.src.sat, "region": self.region.name,
                "lat": (p.lat * 1e4).round() / 1e4, "lon": (p.lon * 1e4).round() / 1e4, "mask": p.mask, "conf": p.confidence(),
                "mw": p.mw.map(|x| (x * 10.0).round() / 10.0), "area": p.area.map(|x| x.round()), "temp": p.temp.map(|x| x.round()),
                "place": place, "fire": fire,
            }}));
        }
        let burning = self.tracker.fires.iter().filter(|f| f.last == ts).count();
        self.pending.push(json!({"op": "upsert", "table": "ea_scans", "doc": {
            "id": format!("{}-{}-{stamp}", self.region.name, r.src.sat), "ts": iso(ts), "sat": r.src.sat, "region": self.region.name,
            "pixels": kept, "mw": (mw * 10.0).round() / 10.0, "fires": burning, "file": r.scan.key, "read_ms": (r.ms * 10.0).round() / 10.0,
        }}));
        self.since_fires += 1;
        if self.pending.len() >= 4000 || self.since_fires >= 12 {
            self.flush(false).await?;
        }
        Ok(())
    }

    /// Writes what is pending, and the fires that changed (with
    /// `all`, every fire still burning too).
    pub async fn flush(&mut self, all: bool) -> Result<()> {
        let region = self.region;
        for f in self.tracker.take_out() {
            let c = fires::context(&f, &self.places);
            self.pending.push(json!({"op": "upsert", "table": "ea_fires", "doc": fires::row(&f, &c, region.px_km2, "out")}));
        }
        for (gone, kept) in self.tracker.take_merged() {
            self.pending.push(json!({"op": "delete", "table": "ea_fires", "key": gone}));
            tracing::debug!(gone, kept, "merged");
        }
        // Live (the last scan within three hours), each fire's outlook from
        // the fires whose first hours were like its own, for the rules.
        let now = self.tracker.last_scan.unwrap_or(0);
        let live = chrono::Utc::now().timestamp() - now < 3 * 3600;
        let db = self.db.clone();
        for f in self.tracker.fires.iter_mut() {
            if f.changed || all {
                let c = fires::context(f, &self.places);
                let mut row = fires::row(f, &c, region.px_km2, "burning");
                if let Some((w, _, _, _)) = fires::window_for((now - f.start) as f64 / 3600.0).filter(|_| live && row.get("shape").is_some()) {
                    let v: Vec<f32> = row[w].as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|x| x as f32)).collect()).unwrap_or_default();
                    if let Ok(rows) = crate::analogs::nearest(&db, None, w, &v, &f.id, region.name, false, now, crate::analogs::K).await {
                        if rows.len() >= 5 {
                            let o = crate::analogs::outlook_of(w, 0.0, &rows);
                            row["outlook_km2"] = json!((o.median * 10.0).round() / 10.0);
                            row["outlook_lo"] = json!((o.lo * 10.0).round() / 10.0);
                            row["outlook_hi"] = json!((o.hi * 10.0).round() / 10.0);
                        }
                    }
                }
                self.pending.push(json!({"op": "upsert", "table": "ea_fires", "doc": row}));
                f.changed = false;
            }
        }
        if let Some(t) = self.tracker.last_scan {
            self.pending.push(json!({"op": "upsert", "table": "ea_feed", "doc": {"id": format!("goes-{}", region.name), "kind": "goes", "at": iso(t), "rows": self.tracker.fires.len(), "note": format!("{} fires burning", self.tracker.fires.len())}}));
        }
        let ops = std::mem::take(&mut self.pending);
        if !ops.is_empty() {
            self.db.write(ops).await?;
        }
        self.since_fires = 0;
        Ok(())
    }
}

/// The archive from `from` to `to` (or now), then with `live` each new scan.
pub async fn run(db: Strata, places: Arc<Places>, o: Opts) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let mut from = match o.from {
        Some(t) => t,
        None => {
            let rows = db.query("SELECT MAX(ts) AS t FROM ea_scans WHERE region = :r", json!({"r": o.region.name})).await.unwrap_or_default();
            // After the newest scan kept, at most a day back.
            rows.first().and_then(|r| unix(&r.s("t"))).map(|t| (t + 1).max(now - 86400)).unwrap_or(now - 7 * 3600)
        }
    };
    let mut feed = Feed::new(db.clone(), places, o.region, from).await?;
    // Fires left burning before `from` and not seen since: out.
    feed.close_stale(from).await?;
    let to = o.to.unwrap_or(now);
    // A day at a time, so the fires are written as it goes.
    while from <= to {
        let end = (from + 86400).min(to);
        let scans = feed.scans(from, end, o.every).await?;
        let t0 = std::time::Instant::now();
        let n = feed.read(scans, o.parallel).await?;
        tracing::info!(region = o.region.name, from = %iso(from), scans = n, burning = feed.tracker.fires.len(), secs = t0.elapsed().as_secs(), "archive");
        from = end + 1;
    }
    if !o.live {
        // The fires burning at the end are written as they are.
        feed.flush(true).await?;
        return Ok(());
    }
    let mut last = feed.tracker.last_scan.unwrap_or(to);
    loop {
        tokio::time::sleep(Duration::from_secs(45)).await;
        let now = chrono::Utc::now().timestamp();
        let scans = match feed.scans(last + 1, now, 0).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "listing");
                continue;
            }
        };
        if scans.is_empty() {
            continue;
        }
        let newest = scans.last().map(|s| s.1.start).unwrap_or(last);
        match feed.read(scans, 4).await {
            Ok(n) => tracing::info!(region = o.region.name, scans = n, burning = feed.tracker.fires.len(), "live"),
            Err(e) => tracing::warn!(error = %e, "live"),
        }
        last = newest;
    }
}
