//! Clear sky: GOES's clear sky mask (ABI L2 ACM) and its smoke and dust
//! (ADP), read for each place: GOES-West's CONUS sector every five
//! minutes over California (`ACMC`, `ADPC`) and GOES-East's full disk
//! every ten over Chile (`ACMF`, `ADPF`).
//!
//! A place is a 3.84 km square, and a 2 km mask pixel falls in one square
//! or none: the pixels of each square are found once, from the first file
//! (the satellite's grid does not move), and every scan after is a count
//! of the clear ones (mask 0 and 1, clear and probably clear) among them.
//!
//! Each place keeps a row of `ea_sky`: the sky over it now, and its last
//! 30 days, each day the share of its daylight scans that were clear and
//! each night the share of its dark ones (the sun 12 degrees down or more:
//! dark enough for stars). Days and nights are the place's own, by the sun
//! over it. Only the places whose sky changed are written each scan.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use netcdf_reader::NcFile;
use serde_json::{json, Value};

use crate::feed::Region;
use crate::goes::{self, Projection, Scan, Source};
use crate::places::Places;
use crate::strata::{Row, RowExt, Strata};

pub const DAYS: usize = 30;
/// Half a place's side, in degrees of latitude.
const HALF: f64 = 3.84 / 2.0 / 111.32;
/// The sun this high is daylight; this low, a dark night.
pub const DAY_SUN: f64 = 5.0;
pub const NIGHT_SUN: f64 = -12.0;
/// A night or a day this clear (per cent of its scans) is a clear one.
pub const GOOD: i64 = 80;

pub const G18_ACM: Source = Source { sat: "G18", bucket: "noaa-goes18", product: "ABI-L2-ACMC", every: 300 };
pub const G18_ADP: Source = Source { sat: "G18", bucket: "noaa-goes18", product: "ABI-L2-ADPC", every: 300 };
pub const G19_ACM: Source = Source { sat: "G19", bucket: "noaa-goes19", product: "ABI-L2-ACMF", every: 600 };
pub const G19_ADP: Source = Source { sat: "G19", bucket: "noaa-goes19", product: "ABI-L2-ADPF", every: 600 };

/// The mask and the smoke of a region's satellite.
pub fn sources(region: &str) -> (Source, Source) {
    if region == "chile" {
        (G19_ACM, G19_ADP)
    } else {
        (G18_ACM, G18_ADP)
    }
}

/// How high the sun is (degrees) over a point at a time (Unix seconds):
/// the Astronomical Almanac's low-precision formulas, good to a few
/// hundredths of a degree, which is plenty to tell day from night.
pub fn sun(lat: f64, lon: f64, t: i64) -> f64 {
    let n = (t - 946_728_000) as f64 / 86400.0;
    let g = (357.529 + 0.985_600_28 * n).to_radians();
    let q = 280.459 + 0.985_647_36 * n;
    let l = (q + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();
    let e = (23.439 - 0.000_000_36 * n).to_radians();
    let ra = (e.cos() * l.sin()).atan2(l.cos());
    let dec = (e.sin() * l.sin()).asin();
    let gmst = (18.697_374_558 + 24.065_709_824_419_08 * n).rem_euclid(24.0) * 15.0;
    let ha = (gmst + lon).to_radians() - ra;
    let (la, d) = (lat.to_radians(), dec);
    (la.sin() * d.sin() + la.cos() * d.cos() * ha.cos()).asin().to_degrees()
}

/// A place's own day (by the sun over it, days since 1970), and the day
/// whose evening a night belongs to.
pub fn solar_day(lon: f64, t: i64) -> i64 {
    (t + (lon / 15.0 * 3600.0) as i64).div_euclid(86400)
}
pub fn night_of(lon: f64, t: i64) -> i64 {
    (t + (lon / 15.0 * 3600.0) as i64 - 43200).div_euclid(86400)
}

/// The sky over one place.
#[derive(Clone, Debug, PartialEq)]
pub struct Sky {
    pub id: String,
    pub region: String,
    pub lat: f64,
    pub lon: f64,
    pub town: String,
    /// The last scan, Unix seconds.
    pub t: i64,
    /// Shares of its pixels clear and with smoke in the last scan (-1: not known).
    pub clear: f64,
    pub smoke: f64,
    pub sun: f64,
    /// The day the last of `days` and `nights` is (days since 1970).
    pub end: i64,
    /// Per cent of each day's daylight scans clear, and of each night's
    /// dark ones; -1 when none was read. The oldest first.
    pub days: Vec<i64>,
    pub nights: Vec<i64>,
    /// The sums behind the last day and night.
    pub dsum: f64,
    pub dn: i64,
    pub nsum: f64,
    pub nn: i64,
}

fn mean(v: &[i64]) -> Option<f64> {
    let k: Vec<i64> = v.iter().copied().filter(|x| *x >= 0).collect();
    (!k.is_empty()).then(|| k.iter().sum::<i64>() as f64 / k.len() as f64)
}

impl Sky {
    pub fn new(id: &str, region: &str, lat: f64, lon: f64, town: &str) -> Sky {
        Sky { id: id.into(), region: region.into(), lat, lon, town: town.into(), t: 0, clear: -1.0, smoke: -1.0, sun: 0.0, end: 0, days: vec![-1; DAYS], nights: vec![-1; DAYS], dsum: 0.0, dn: 0, nsum: 0.0, nn: 0 }
    }

    pub fn from_row(r: &Row) -> Sky {
        let list = |k: &str| -> Vec<i64> {
            let mut v: Vec<i64> = r.get(k).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_i64().unwrap_or(-1)).collect()).unwrap_or_default();
            v.resize(DAYS, -1);
            v
        };
        Sky {
            id: r.s("id"),
            region: r.s("region"),
            lat: r.f("lat"),
            lon: r.f("lon"),
            town: r.s("town"),
            t: crate::pages::unix(&r.s("t")).unwrap_or(0),
            clear: r.get("clear").and_then(Value::as_f64).unwrap_or(-1.0),
            smoke: r.get("smoke").and_then(Value::as_f64).unwrap_or(-1.0),
            sun: r.f("sun"),
            end: r.i("day_end"),
            days: list("days"),
            nights: list("nights"),
            dsum: r.f("dsum"),
            dn: r.i("dn"),
            nsum: r.f("nsum"),
            nn: r.i("nn"),
        }
    }

    pub fn doc(&self) -> Value {
        let good = |v: &[i64]| v.iter().filter(|x| **x >= GOOD).count();
        let r2 = |x: f64| (x * 100.0).round() / 100.0;
        json!({
            "id": self.id, "region": self.region, "lat": self.lat, "lon": self.lon, "town": self.town,
            "t": crate::pages::iso(self.t), "clear": r2(self.clear), "smoke": r2(self.smoke), "sun": (self.sun * 10.0).round() / 10.0,
            "day_end": self.end, "days": self.days, "nights": self.nights,
            "dsum": r2(self.dsum), "dn": self.dn, "nsum": r2(self.nsum), "nn": self.nn,
            "clear_days": mean(&self.days).map(|x| x.round()), "clear_nights": mean(&self.nights).map(|x| x.round()),
            "good_days": good(&self.days), "good_nights": good(&self.nights),
        })
    }

    /// Moves the 30 days on to `day`.
    fn advance(&mut self, day: i64) {
        if self.end == 0 {
            self.end = day;
            return;
        }
        if day <= self.end {
            return;
        }
        let by = ((day - self.end) as usize).min(DAYS);
        self.days.drain(..by);
        self.nights.drain(..by);
        self.days.resize(DAYS, -1);
        self.nights.resize(DAYS, -1);
        self.end = day;
        self.dsum = 0.0;
        self.dn = 0;
        self.nsum = 0.0;
        self.nn = 0;
    }

    /// One scan's look: `clear` the share of its pixels clear (none when
    /// the scan had none over it), `smoke` the share with smoke.
    pub fn see(&mut self, t: i64, clear: Option<f64>, smoke: Option<f64>) {
        if t < self.t {
            return;
        }
        self.t = t;
        self.sun = sun(self.lat, self.lon, t);
        self.clear = clear.unwrap_or(-1.0);
        self.smoke = smoke.unwrap_or(-1.0);
        let Some(c) = clear else { return };
        if self.sun >= DAY_SUN {
            let d = solar_day(self.lon, t);
            if d < self.end {
                return;
            }
            self.advance(d);
            self.dsum += c;
            self.dn += 1;
            self.days[DAYS - 1] = (100.0 * self.dsum / self.dn as f64).round() as i64;
        } else if self.sun <= NIGHT_SUN {
            let d = night_of(self.lon, t);
            if d < self.end {
                return;
            }
            self.advance(d);
            self.nsum += c;
            self.nn += 1;
            self.nights[DAYS - 1] = (100.0 * self.nsum / self.nn as f64).round() as i64;
        }
    }

    /// Whether the row changed enough since `before` to write it again.
    pub fn moved(&self, before: &Sky) -> bool {
        let tenth = |x: f64| (x * 10.0).round() as i64;
        // Today's and tonight's shares move a little with each scan: written
        // when they moved 5 points, or every three hours.
        let moved5 = |a: &[i64], b: &[i64]| a.iter().zip(b).any(|(x, y)| (x - y).abs() >= 5);
        tenth(self.clear) != tenth(before.clear)
            || tenth(self.smoke) != tenth(before.smoke)
            || self.end != before.end
            || moved5(&self.days, &before.days)
            || moved5(&self.nights, &before.nights)
            || (self.t - before.t) >= 3 * 3600
    }
}

/// Which pixels of a file's grid fall in which place: a window of rows
/// and columns, and for each place of the region the pixels inside it
/// (offsets in the window).
pub struct Layout {
    pub key: (usize, usize, i64, i64),
    pub window: (usize, usize, usize, usize),
    pub cells: Vec<Vec<u32>>,
}

fn grid(f: &NcFile) -> Result<(Vec<f64>, Vec<f64>, Projection)> {
    let x: Vec<f64> = f.read_variable_unpacked("x").map_err(|e| anyhow!("x: {e}"))?.iter().copied().collect();
    let y: Vec<f64> = f.read_variable_unpacked("y").map_err(|e| anyhow!("y: {e}"))?.iter().copied().collect();
    let p = f.variable("goes_imager_projection").map_err(|e| anyhow!("projection: {e}"))?;
    let attr = |k: &str| p.attribute(k).and_then(|a| a.value.as_f64()).ok_or_else(|| anyhow!("projection's {k}"));
    let proj = Projection { h: attr("perspective_point_height")? + attr("semi_major_axis")?, req: attr("semi_major_axis")?, rpol: attr("semi_minor_axis")?, lon0: attr("longitude_of_projection_origin")?.to_radians() };
    Ok((x, y, proj))
}

fn grid_key(x: &[f64], y: &[f64]) -> (usize, usize, i64, i64) {
    (x.len(), y.len(), (x.first().copied().unwrap_or(0.0) * 1e7) as i64, (y.first().copied().unwrap_or(0.0) * 1e7) as i64)
}

impl Layout {
    /// The places' pixels, from a file of the grid.
    pub fn build(f: &NcFile, region: &Region, places: &[&crate::places::Place], all: &Places) -> Result<Layout> {
        let (x, y, proj) = grid(f)?;
        let Some(((r0, r1), (c0, c1))) = proj.window(&x, &y, region.bounds) else { bail!("{} is not on this grid", region.name) };
        let index: HashMap<&str, usize> = places.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
        let mut cells = vec![Vec::new(); places.len()];
        let nc = c1 - c0;
        for r in r0..r1 {
            for c in c0..c1 {
                let Some((lat, lon)) = proj.to_lat_lon(x[c], y[r]) else { continue };
                let Some(p) = all.nearest(lat, lon, 2.8) else { continue };
                let Some(&i) = index.get(p.id.as_str()) else { continue };
                let dl = HALF / p.lat.to_radians().cos().max(0.2);
                if (lat - p.lat).abs() <= HALF && (lon - p.lon).abs() <= dl {
                    cells[i].push(((r - r0) * nc + (c - c0)) as u32);
                }
            }
        }
        Ok(Layout { key: grid_key(&x, &y), window: (r0, r1, c0, c1), cells })
    }

    fn slice(&self) -> netcdf_reader::NcSliceInfo {
        let (r0, r1, c0, c1) = self.window;
        netcdf_reader::NcSliceInfo {
            selections: vec![
                netcdf_reader::NcSliceInfoElem::Slice { start: r0 as u64, end: r1 as u64, step: 1 },
                netcdf_reader::NcSliceInfoElem::Slice { start: c0 as u64, end: c1 as u64, step: 1 },
            ],
        }
    }

    /// Each place's share of clear pixels (`ACM` 0 or 1 of the pixels with
    /// a value), or none.
    pub fn clear(&self, f: &NcFile) -> Result<Vec<Option<f64>>> {
        let a: ndarray::ArrayD<u8> = f.read_variable_slice("ACM", &self.slice()).map_err(|e| anyhow!("ACM: {e}"))?;
        let flat = a.as_slice_memory_order().map(<[u8]>::to_vec).unwrap_or_else(|| a.iter().copied().collect());
        Ok(self.cells.iter().map(|px| share(px, |i| flat.get(i).copied().filter(|v| *v <= 3).map(|v| v <= 1))).collect())
    }

    /// Each place's share of pixels with smoke, among those the product
    /// judged (by day only).
    pub fn smoke(&self, f: &NcFile) -> Result<Vec<Option<f64>>> {
        let a: ndarray::ArrayD<i8> = f.read_variable_slice("Smoke", &self.slice()).map_err(|e| anyhow!("Smoke: {e}"))?;
        let flat: Vec<i8> = a.iter().copied().collect();
        Ok(self.cells.iter().map(|px| share(px, |i| flat.get(i).copied().filter(|v| *v == 0 || *v == 1).map(|v| v == 1))).collect())
    }
}

fn share(px: &[u32], at: impl Fn(usize) -> Option<bool>) -> Option<f64> {
    let (mut yes, mut n) = (0u32, 0u32);
    for &i in px {
        if let Some(b) = at(i as usize) {
            n += 1;
            yes += b as u32;
        }
    }
    (n > 0).then(|| yes as f64 / n as f64)
}

pub struct Opts {
    pub region: Region,
    /// The archive from this time; left out, after the last scan read (or
    /// the last hour).
    pub from: Option<i64>,
    pub live: bool,
    /// Read a scan every this many seconds.
    pub every: i64,
}

fn iso(t: i64) -> String {
    crate::pages::iso(t)
}

/// Reads the sky over a region's places into `ea_sky`.
pub async fn run(db: Strata, places: Arc<Places>, o: Opts) -> Result<()> {
    let region = o.region;
    let mine: Vec<&crate::places::Place> = places.all.iter().filter(|p| p.region == region.name).collect();
    if mine.is_empty() {
        bail!("no places in {}: load them first", region.name);
    }
    // The rows there are, so the days carry on.
    let rows = db.scroll("SELECT * FROM ea_sky WHERE region = :r", json!({"r": region.name}), 10_000).await.map_err(|e| anyhow!("{e}"))?;
    // From a time given, the days are read again from there: what the rows
    // hold is dropped (a scan older than a row's last one would be skipped).
    let kept: HashMap<String, Sky> = if o.from.is_some() { HashMap::new() } else { rows.iter().map(|r| (r.s("id"), Sky::from_row(r))).collect() };
    let mut sky: Vec<Sky> = mine.iter().map(|p| kept.get(&p.id).cloned().unwrap_or_else(|| Sky::new(&p.id, region.name, p.lat, p.lon, &p.town))).collect();
    let mut written: Vec<Option<Sky>> = mine.iter().map(|p| kept.get(&p.id).cloned()).collect();
    let fed = db.query("SELECT at FROM ea_feed WHERE id = :id", json!({"id": format!("sky-{}", region.name)})).await.map_err(|e| anyhow!("{e}"))?;
    let mut last = fed.first().and_then(|r| crate::pages::unix(&r.s("at"))).unwrap_or(0);
    let now = chrono::Utc::now().timestamp();
    let mut from = o.from.unwrap_or(if last > 0 { last + 1 } else { now - 3600 }).max(now - DAYS as i64 * 86400);
    if o.from.is_some() {
        last = last.min(from - 1);
    }
    let http = reqwest::Client::builder().timeout(Duration::from_secs(180)).connect_timeout(Duration::from_secs(15)).build()?;
    let (acm, adp) = sources(region.name);
    let mut layout: Option<Layout> = None;
    let every = o.every.max(acm.every);
    tracing::info!(region = region.name, places = sky.len(), rows = kept.len(), from = %iso(from), every, "sky");
    loop {
        let now = chrono::Utc::now().timestamp();
        // The scans to read: each hour's listing, one scan every `every`.
        let mut todo: Vec<Scan> = Vec::new();
        let mut h = from - from.rem_euclid(3600);
        while h <= now {
            match goes::list_hour(&http, acm, h).await {
                Ok(s) => {
                    for sc in s {
                        if sc.start > last && sc.start >= from && todo.last().map_or(sc.start - last >= every - 30, |p| sc.start - p.start >= every - 30) {
                            todo.push(sc);
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "sky listing"),
            }
            // A scan every few hours needs only those hours' listings.
            h += 3600 * (every / 3600).max(1);
        }
        for sc in todo {
            let t0 = std::time::Instant::now();
            let bytes = match goes::fetch(&http, acm, &sc.key).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, key = %sc.key, "sky fetch");
                    continue;
                }
            };
            // The smoke of the same scan, when NOAA has it.
            let smoke_bytes = match goes::list_hour(&http, adp, sc.start).await {
                Ok(list) => match list.into_iter().find(|s| (s.start - sc.start).abs() < 60) {
                    Some(s) => goes::fetch(&http, adp, &s.key).await.ok(),
                    None => None,
                },
                Err(_) => None,
            };
            let reg = region;
            let lay = layout.take();
            let mine_owned: Vec<crate::places::Place> = if lay.is_none() { mine.iter().map(|p| (*p).clone()).collect() } else { Vec::new() };
            let all = places.clone();
            let res = tokio::task::spawn_blocking(move || -> Result<(Layout, Vec<Option<f64>>, Vec<Option<f64>>)> {
                let f = NcFile::from_vec(bytes).map_err(|e| anyhow!("not a NetCDF file: {e}"))?;
                let lay = match lay {
                    Some(l) => l,
                    None => {
                        let refs: Vec<&crate::places::Place> = mine_owned.iter().collect();
                        Layout::build(&f, &reg, &refs, &all)?
                    }
                };
                let clear = lay.clear(&f)?;
                let smoke = match smoke_bytes.map(NcFile::from_vec) {
                    Some(Ok(s)) => {
                        let (x, y, _) = grid(&s)?;
                        if grid_key(&x, &y) == lay.key {
                            lay.smoke(&s).unwrap_or_else(|_| vec![None; lay.cells.len()])
                        } else {
                            vec![None; lay.cells.len()]
                        }
                    }
                    _ => vec![None; lay.cells.len()],
                };
                Ok((lay, clear, smoke))
            })
            .await?;
            let (lay, clear, smoke) = match res {
                Ok(x) => x,
                Err(e) => {
                    tracing::warn!(error = %e, key = %sc.key, "sky read");
                    continue;
                }
            };
            if layout.is_none() && lay.cells.iter().filter(|c| c.is_empty()).count() * 10 > lay.cells.len() {
                tracing::warn!(region = region.name, empty = lay.cells.iter().filter(|c| c.is_empty()).count(), "places with no mask pixel");
            }
            for (i, s) in sky.iter_mut().enumerate() {
                s.see(sc.start, clear[i], smoke[i]);
            }
            layout = Some(lay);
            let mut docs = Vec::new();
            for (i, s) in sky.iter().enumerate() {
                if written[i].as_ref().is_none_or(|w| s.moved(w)) {
                    docs.push(s.doc());
                    written[i] = Some(s.clone());
                }
            }
            let n = docs.len();
            let mut ops: Vec<Value> = docs.into_iter().map(|d| json!({"op": "upsert", "table": "ea_sky", "doc": d})).collect();
            let clear_now = clear.iter().flatten().filter(|c| **c >= 0.5).count();
            ops.push(json!({"op": "upsert", "table": "ea_feed", "doc": {"id": format!("sky-{}", region.name), "kind": "sky", "at": iso(sc.start), "rows": n, "note": format!("{clear_now} places clear")}}));
            db.write(ops).await.map_err(|e| anyhow!("{e}"))?;
            last = sc.start;
            tracing::info!(region = region.name, scan = %iso(sc.start), written = n, clear = clear_now, ms = t0.elapsed().as_millis() as u64, "sky scan");
        }
        if !o.live {
            return Ok(());
        }
        from = last + 1;
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

// ------------------------------------------------------------------ the app

/// A place's sky as the app keeps it in memory.
#[derive(Clone, Debug, Default)]
pub struct Lite {
    pub clear: f64,
    pub smoke: f64,
    pub sun: f64,
    pub t: i64,
    pub clear_days: Option<f64>,
    pub clear_nights: Option<f64>,
    pub good_days: i64,
    pub good_nights: i64,
    pub end: i64,
    pub days: Vec<i64>,
    pub nights: Vec<i64>,
}

impl Lite {
    fn from_row(r: &Row) -> Lite {
        let s = Sky::from_row(r);
        Lite {
            clear: s.clear,
            smoke: s.smoke,
            sun: s.sun,
            t: s.t,
            clear_days: r.get("clear_days").and_then(Value::as_f64),
            clear_nights: r.get("clear_nights").and_then(Value::as_f64),
            good_days: r.i("good_days"),
            good_nights: r.i("good_nights"),
            end: s.end,
            days: s.days,
            nights: s.nights,
        }
    }

    pub fn json(&self) -> Value {
        json!({"clear": self.clear, "smoke": self.smoke, "sun": self.sun, "t": crate::pages::iso(self.t), "clear_days": self.clear_days, "clear_nights": self.clear_nights,
               "good_days": self.good_days, "good_nights": self.good_nights, "end": self.end, "days": self.days, "nights": self.nights, "words": words(self)})
    }
}

/// The sky now, in a few words.
pub fn words(l: &Lite) -> &'static str {
    if l.clear < 0.0 {
        "not seen"
    } else if l.smoke >= 0.3 {
        "smoke"
    } else if l.clear >= 0.8 {
        "clear"
    } else if l.clear >= 0.4 {
        "partly cloudy"
    } else {
        "cloudy"
    }
}

/// A region's skies in memory: each place's, and the map's layer.
pub struct Mem {
    pub region: String,
    pub t: i64,
    pub by_id: HashMap<String, Lite>,
    pub layer: Arc<String>,
}

pub async fn read(db: &Strata, places: &Places, region: &str) -> crate::strata::Result<Mem> {
    // Each row becomes its Lite as its page arrives: the 100,000 rows held
    // as JSON, with their 30 days and nights, took the app past 1 GB.
    let by_id: HashMap<String, Lite> = db
        .scroll_map("SELECT * FROM ea_sky WHERE region = :r", json!({"r": region}), 2_000, |r| Some((r.s("id"), Lite::from_row(&r))))
        .await?
        .into_iter()
        .collect();
    let t = by_id.values().map(|l| l.t).max().unwrap_or(0);
    let (mut lat, mut lon, mut c, mut sm, mut n, mut d) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    let tenth = |x: f64| if x < 0.0 { Value::Null } else { json!((x * 10.0).round() as i64) };
    for p in places.all.iter().filter(|p| p.region == region) {
        let l = by_id.get(&p.id);
        lat.push((p.lat * 1e4).round() / 1e4);
        lon.push((p.lon * 1e4).round() / 1e4);
        c.push(l.map_or(Value::Null, |l| tenth(l.clear)));
        sm.push(l.map_or(Value::Null, |l| tenth(l.smoke)));
        n.push(l.and_then(|l| l.clear_nights).map_or(Value::Null, |x| json!(x)));
        d.push(l.and_then(|l| l.clear_days).map_or(Value::Null, |x| json!(x)));
    }
    let layer = json!({"region": region, "t": if t > 0 { Value::String(crate::pages::iso(t)) } else { Value::Null }, "lat": lat, "lon": lon, "clear": c, "smoke": sm, "nights": n, "days": d}).to_string();
    Ok(Mem { region: region.to_string(), t, by_id, layer: Arc::new(layer) })
}

/// What a search for a clear place asks for: now, the most clear nights
/// of 30, or the most clear days.
pub fn rank(want: &str, l: &Lite) -> f64 {
    match want {
        "nights" => l.clear_nights.unwrap_or(-1.0) + l.good_nights as f64 * 0.01,
        "days" => l.clear_days.unwrap_or(-1.0) + l.good_days as f64 * 0.01,
        _ => {
            if l.clear < 0.0 {
                -1.0
            } else {
                l.clear * 100.0 - l.smoke.max(0.0) * 50.0 + l.clear_nights.unwrap_or(0.0) * 0.01
            }
        }
    }
}

/// Places with a clear sky: near a point (within `km`) or anywhere in the
/// region, ranked by the sky asked for (`now`, `nights` or `days`); with
/// words, among the 300 places that look most like them.
pub struct Find<'a> {
    pub region: &'a str,
    pub at: Option<(f64, f64)>,
    pub km: f64,
    pub want: &'a str,
    pub q: &'a str,
}

pub async fn find(c: &crate::pages::Ctx<'_>, mem: &Mem, f: Find<'_>) -> Result<Value, crate::api::ApiError> {
    let near = |lat: f64, lon: f64| f.at.map(|(a, b)| crate::fires::km(a, b, lat, lon));
    let inside = |lat: f64, lon: f64| near(lat, lon).is_none_or(|d| d <= f.km);
    // (place id, the look's rank among the words' places)
    let mut cands: Vec<(String, Option<usize>)> = Vec::new();
    if !f.q.trim().is_empty() {
        let mut p = json!({"r": f.region, "v": c.words(f.q.trim())});
        let mut wh = "region = :r".to_string();
        if let Some((lat, lon)) = f.at {
            let (s, n, w, e) = crate::watch::bbox(lat, lon, f.km);
            wh.push_str(" AND lat >= :s AND lat <= :n AND lon >= :w AND lon <= :e");
            for (k, v) in [("s", s), ("n", n), ("w", w), ("e", e)] {
                p[k] = json!(v);
            }
        }
        let rows = c.rows("the places that look like the words", format!("SELECT id, lat, lon FROM ea_places WHERE {wh} ORDER BY similarity(look, :v) DESC LIMIT 300"), p).await?;
        cands.extend(rows.iter().filter(|r| inside(r.f("lat"), r.f("lon"))).enumerate().map(|(i, r)| (r.s("id"), Some(i))));
    } else {
        cands.extend(c.places.all.iter().filter(|p| p.region == f.region && inside(p.lat, p.lon)).map(|p| (p.id.clone(), None)));
    }
    let index: HashMap<&str, &crate::places::Place> = c.places.all.iter().filter(|p| p.region == f.region).map(|p| (p.id.as_str(), p)).collect();
    let n = cands.len();
    let seen = cands.iter().filter(|(id, _)| mem.by_id.get(id).is_some_and(|l| l.clear >= 0.0)).count();
    let clear = cands.iter().filter(|(id, _)| mem.by_id.get(id).is_some_and(|l| l.clear >= 0.8)).count();
    let mut scored: Vec<(f64, f64, &str)> = cands
        .iter()
        .filter_map(|(id, look)| {
            let l = mem.by_id.get(id)?;
            let r = rank(f.want, l);
            if r < 0.0 {
                return None;
            }
            let p = index.get(id.as_str())?;
            let bonus = look.map_or(0.0, |i| 20.0 * (1.0 - i as f64 / 300.0));
            Some((r + bonus, near(p.lat, p.lon).unwrap_or(0.0), id.as_str()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
    let out: Vec<Value> = scored
        .iter()
        .take(30)
        .filter_map(|(_, d, id)| {
            let p = index.get(*id)?;
            let l = mem.by_id.get(*id)?;
            Some(json!({"id": p.id, "town": p.town, "adm1": p.adm1, "adm2": p.adm2, "lat": p.lat, "lon": p.lon, "kind": p.kind, "elev": p.elev, "people": p.people.round(),
                "km": f.at.map(|_| (d * 10.0).round() / 10.0), "dir": f.at.map(|(a, b)| crate::watch::direction(a, b, p.lat, p.lon)), "sky": l.json()}))
        })
        .collect();
    Ok(json!({"region": f.region, "want": f.want, "q": f.q, "km": f.km, "places": out, "looked": n, "seen": seen, "clear_now": clear,
              "t": if mem.t > 0 { Value::String(crate::pages::iso(mem.t)) } else { Value::Null }}))
}

#[cfg(test)]
mod lite_tests {
    use super::*;

    #[test]
    fn words_and_ranks() {
        let mut l = Lite { clear: 0.9, smoke: 0.0, clear_nights: Some(70.0), good_nights: 12, ..Default::default() };
        assert_eq!(words(&l), "clear");
        let a = rank("now", &l);
        l.smoke = 0.5;
        assert_eq!(words(&l), "smoke");
        assert!(rank("now", &l) < a);
        assert!((rank("nights", &l) - 70.12).abs() < 1e-9);
        l.clear = -1.0;
        assert_eq!(words(&l), "not seen");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_is_up_at_noon_and_down_at_midnight() {
        // Paradise, 20 November 2026: noon PST is 20:00 UTC.
        let noon = 1_795_204_800;
        let s = sun(39.76, -121.62, noon);
        assert!(s > 25.0 && s < 35.0, "{s}");
        assert!(sun(39.76, -121.62, noon + 12 * 3600) < -50.0);
        // A place's day and the night after it.
        assert_eq!(solar_day(-121.62, noon), night_of(-121.62, noon + 10 * 3600));
    }

    #[test]
    fn days_and_nights_roll_on() {
        let mut s = Sky::new("p", "california", 39.76, -121.62, "Paradise");
        let noon = 1_795_204_800;
        s.see(noon, Some(1.0), Some(0.0));
        s.see(noon + 3600, Some(0.0), None);
        assert_eq!(s.days[DAYS - 1], 50);
        // That night, clear.
        s.see(noon + 8 * 3600, Some(1.0), None);
        assert_eq!(s.nights[DAYS - 1], 100);
        assert_eq!(s.days[DAYS - 1], 50, "the night belongs to the same day");
        // Three days later: the day moves on, the gap stays unknown.
        s.see(noon + 3 * 86400, Some(1.0), None);
        assert_eq!(&s.days[DAYS - 4..], &[50, -1, -1, 100]);
        assert_eq!(&s.nights[DAYS - 4..], &[100, -1, -1, -1]);
        let d = s.doc();
        assert_eq!(d["good_days"], 1);
        assert_eq!(d["clear_days"], 75.0);
        let mut back = Sky::from_row(d.as_object().unwrap());
        back.sun = s.sun;
        assert_eq!(back, s);
        // Nothing older is taken.
        s.see(noon, Some(0.0), None);
        assert_eq!(s.days[DAYS - 1], 100);
    }
}
