//! Fires: a region's fire pixels grouped as GOES scans come in. A pixel
//! joins the fire whose recent pixels lie within [`JOIN_KM`] of it; a
//! fire that no scan has seen for [`GAP_S`] is out. Each fire keeps how its
//! first hours went, a bin every [`BIN_S`] seconds: the power of its
//! strongest scan in the bin, its pixels and how far it had spread from
//! where it started. Those, with what it started on and the weather, are
//! its shape ([`shape`]): the vector that finds the fires that began the
//! same way.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::goes::Pixel;
use crate::places::Places;

/// A pixel within this distance of a fire's pixels of the last hours is
/// the same fire (GOES's pixels are 2 to 4 km apart over the regions).
pub const JOIN_KM: f64 = 6.0;
/// A fire unseen this long is out.
pub const GAP_S: i64 = 6 * 3600;
/// The bins of a fire's first hours.
pub const BIN_S: i64 = 900;
/// The first hours kept, in bins (twelve hours).
pub const BINS: usize = 48;
/// A fire's shape: 12 half-hour bins of power, pixels and spread for the
/// first six hours, and six numbers about where it started.
pub const SHAPE_DIM: usize = 12 * 3 + 6;
/// The first two hours (8 quarter-hour bins), for a fire younger than six.
pub const SHAPE2_DIM: usize = 8 * 3 + 6;
/// The first hour (4 quarter-hour bins), for a fire younger than two.
pub const SHAPE1_DIM: usize = 4 * 3 + 6;

/// The windows a fire is compared in: its column, the hours it covers, and
/// the bins (each of `BIN_S`) a slot of it sums.
pub const WINDOWS: [(&str, f64, usize, usize); 3] = [("shape1", 1.0, 4, 1), ("shape2", 2.0, 8, 1), ("shape", 6.0, 12, 2)];

/// The window a fire `hours` old is compared in: the longest it has
/// lived through (none before an hour).
pub fn window_for(hours: f64) -> Option<(&'static str, f64, usize, usize)> {
    WINDOWS.iter().rev().find(|w| hours >= w.1).copied()
}

#[derive(Clone, Debug, Default)]
pub struct Bin {
    /// The power of the bin's strongest scan (megawatts, its pixels summed).
    pub mw: f64,
    /// The pixels of that scan.
    pub px: u32,
    /// The farthest pixel from the start so far, km.
    pub spread: f64,
}

#[derive(Clone, Debug)]
pub struct Fire {
    pub id: String,
    pub region: String,
    pub start: i64,
    pub last: i64,
    pub lat: f64,
    pub lon: f64,
    pub place: Option<String>,
    /// Pixels by their grid cell (0.02 degrees), with when each was last
    /// seen: what a new pixel is near.
    pub cells: HashMap<(i32, i32), i64>,
    /// Every pixel ever seen, by the satellite's row and column.
    pub footprint: std::collections::HashSet<(u32, u32)>,
    pub scans: u32,
    pub peak_px: u32,
    pub peak_mw: f64,
    pub mw_now: f64,
    pub mw_hist: Vec<(i64, f64)>,
    /// Megawatt-seconds, summed over scans (each scan stands for the time
    /// to the next).
    pub energy: f64,
    pub spread: f64,
    pub bins: Vec<Bin>,
    /// Whether the start was watched: false when the fire was burning in
    /// the first scan read.
    pub seen_start: bool,
    pub changed: bool,
}

pub fn km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dy = (lat2 - lat1) * 111.32;
    let dx = (lon2 - lon1) * 111.32 * ((lat1 + lat2) / 2.0).to_radians().cos();
    (dx * dx + dy * dy).sqrt()
}

fn cell(lat: f64, lon: f64) -> (i32, i32) {
    ((lat / 0.02).floor() as i32, (lon / 0.02).floor() as i32)
}

impl Fire {
    fn new(region: &str, ts: i64, lat: f64, lon: f64, seen_start: bool, places: &Places) -> Fire {
        let d = chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default();
        let id = format!("{}-{}-{:.2}{:+.2}", if region == "chile" { "cl" } else { "ca" }, d.format("%Y%m%dT%H%M"), lat, lon).replace('+', "_");
        Fire {
            id,
            region: region.to_string(),
            start: ts,
            last: ts,
            lat,
            lon,
            place: places.nearest(lat, lon, 3.0).map(|p| p.id.clone()),
            cells: HashMap::new(),
            footprint: Default::default(),
            scans: 0,
            peak_px: 0,
            peak_mw: 0.0,
            mw_now: 0.0,
            mw_hist: Vec::new(),
            energy: 0.0,
            spread: 0.0,
            bins: Vec::new(),
            seen_start,
            changed: true,
        }
    }

    fn near(&self, lat: f64, lon: f64, since: i64) -> bool {
        let (r, c) = cell(lat, lon);
        let k = (JOIN_KM / 2.2).ceil() as i32;
        for dr in -k..=k {
            for dc in -k..=k {
                if let Some(&t) = self.cells.get(&(r + dr, c + dc)) {
                    if t >= since {
                        let (la, lo) = ((r + dr) as f64 * 0.02 + 0.01, (c + dc) as f64 * 0.02 + 0.01);
                        if km(lat, lon, la, lo) <= JOIN_KM + 1.6 {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    pub fn hours(&self) -> f64 {
        (self.last - self.start) as f64 / 3600.0
    }

    /// Square kilometres: the distinct GOES pixels it covered, each about
    /// `px_km2`. Coarse (a pixel is 4 to 9 km² over the regions), and an
    /// overestimate for small fires.
    pub fn area_km2(&self, px_km2: f64) -> f64 {
        self.footprint.len() as f64 * px_km2
    }

    /// The power an hour ago (the scan nearest to it).
    pub fn mw_1h(&self) -> f64 {
        let t = self.last - 3600;
        self.mw_hist.iter().min_by_key(|(at, _)| (at - t).abs()).filter(|(at, _)| (at - t).abs() <= 900).map_or(0.0, |x| x.1)
    }
}

/// The fires of a region, as its scans are read in order.
pub struct Tracker {
    pub region: String,
    pub fires: Vec<Fire>,
    pub first_scan: Option<i64>,
    pub last_scan: Option<i64>,
    /// Fires that went out since the last [`Tracker::take_out`].
    pub out: Vec<Fire>,
    /// Fires merged into another since the last [`Tracker::take_merged`]:
    /// (gone, kept).
    pub merged: Vec<(String, String)>,
}

impl Tracker {
    pub fn new(region: &str) -> Tracker {
        Tracker { region: region.to_string(), fires: Vec::new(), first_scan: None, last_scan: None, out: Vec::new(), merged: Vec::new() }
    }

    /// One scan's pixels: each pixel's fire id, in the pixels' order. A
    /// scan more than an hour after the last starts the watch again, so a
    /// fire burning in it is marked as not seen starting.
    pub fn scan(&mut self, ts: i64, every: i64, pixels: &[Pixel], places: &Places) -> Vec<String> {
        let gap = self.last_scan.is_some_and(|t| ts - t > 3600);
        if self.first_scan.is_none() || gap {
            self.first_scan = Some(ts);
        }
        let watching_from = self.first_scan.unwrap_or(ts);
        self.last_scan = Some(ts);
        // Fires unseen for the gap are out.
        let (live, out): (Vec<Fire>, Vec<Fire>) = std::mem::take(&mut self.fires).into_iter().partition(|f| ts - f.last <= GAP_S);
        self.fires = live;
        self.out.extend(out);
        let recent = ts - 3 * 3600;
        // Group the scan's pixels into blobs: within JOIN_KM of each other.
        let n = pixels.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(p: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while p[r] != r {
                p[r] = p[p[r]];
                r = p[r];
            }
            r
        }
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, p) in pixels.iter().enumerate() {
            grid.entry(cell(p.lat, p.lon)).or_default().push(i);
        }
        let k = (JOIN_KM / 2.2).ceil() as i32;
        for (i, p) in pixels.iter().enumerate() {
            let (r, c) = cell(p.lat, p.lon);
            for dr in -k..=k {
                for dc in -k..=k {
                    if let Some(js) = grid.get(&(r + dr, c + dc)) {
                        for &j in js {
                            if j > i && km(p.lat, p.lon, pixels[j].lat, pixels[j].lon) <= JOIN_KM {
                                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                                if a != b {
                                    parent[a] = b;
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut blobs: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            let r = root(&mut parent, i);
            blobs.entry(r).or_default().push(i);
        }
        let mut blobs: Vec<Vec<usize>> = blobs.into_values().collect();
        blobs.sort_by_key(|b| b[0]);
        let mut ids = vec![String::new(); n];
        for blob in blobs {
            // The fires it touches.
            let mut hit: Vec<usize> = (0..self.fires.len()).filter(|&f| blob.iter().any(|&i| self.fires[f].near(pixels[i].lat, pixels[i].lon, recent))).collect();
            let fi = if hit.is_empty() {
                // A new fire starts at its strongest pixel.
                let best = blob.iter().copied().max_by(|&a, &b| pixels[a].mw.unwrap_or(0.0).total_cmp(&pixels[b].mw.unwrap_or(0.0))).unwrap_or(blob[0]);
                let seen = ts > watching_from;
                self.fires.push(Fire::new(&self.region, ts, pixels[best].lat, pixels[best].lon, seen, places));
                self.fires.len() - 1
            } else {
                // Two fires that grew together become the older one.
                hit.sort_by_key(|&f| (self.fires[f].start, f));
                let keep_id = self.fires[hit[0]].id.clone();
                let mut others = hit[1..].to_vec();
                others.sort_unstable_by(|a, b| b.cmp(a));
                let merged: Vec<Fire> = others.into_iter().map(|o| self.fires.remove(o)).collect();
                let keep = self.fires.iter().position(|f| f.id == keep_id).unwrap_or(0);
                for o in merged {
                    let f = &mut self.fires[keep];
                    f.cells.extend(o.cells);
                    f.footprint.extend(o.footprint);
                    f.energy += o.energy;
                    f.spread = f.spread.max(o.spread + km(f.lat, f.lon, o.lat, o.lon));
                    f.changed = true;
                    self.merged.push((o.id, keep_id.clone()));
                }
                keep
            };
            let f = &mut self.fires[fi];
            for &i in &blob {
                let p = &pixels[i];
                f.cells.insert(cell(p.lat, p.lon), ts);
                f.footprint.insert((p.row, p.col));
                f.spread = f.spread.max(km(f.lat, f.lon, p.lat, p.lon));
                ids[i] = f.id.clone();
            }
            f.changed = true;
        }
        // The scan's totals per fire.
        let mut mw: HashMap<String, (f64, u32)> = HashMap::new();
        for (i, p) in pixels.iter().enumerate() {
            if ids[i].is_empty() {
                continue;
            }
            let e = mw.entry(ids[i].clone()).or_default();
            e.0 += p.mw.unwrap_or(0.0);
            e.1 += 1;
        }
        for f in self.fires.iter_mut() {
            let Some(&(m, px)) = mw.get(&f.id) else { continue };
            f.last = ts;
            f.scans += 1;
            f.mw_now = m;
            f.peak_mw = f.peak_mw.max(m);
            f.peak_px = f.peak_px.max(px);
            f.energy += m * every as f64;
            f.mw_hist.push((ts, m));
            f.mw_hist.retain(|(t, _)| ts - t <= 2 * 3600);
            let b = ((ts - f.start) / BIN_S) as usize;
            if b < BINS {
                if f.bins.len() <= b {
                    let last = f.bins.last().cloned().unwrap_or_default();
                    f.bins.resize(b + 1, Bin { mw: 0.0, px: 0, spread: last.spread });
                }
                let bin = &mut f.bins[b];
                if m >= bin.mw {
                    bin.mw = m;
                    bin.px = px;
                }
                bin.spread = f.spread;
            }
        }
        ids
    }

    pub fn take_out(&mut self) -> Vec<Fire> {
        std::mem::take(&mut self.out)
    }

    pub fn take_merged(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.merged)
    }
}

/// What a fire started on and in, for its shape and its row.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub people5: f64,
    pub trees: f64,
    pub shrubs: f64,
    pub grass: f64,
    pub built: f64,
    pub slope: f64,
    pub elev: f64,
    pub climate: String,
    pub burned_before: Option<i64>,
    pub town: String,
    pub adm1: String,
    pub adm2: String,
    /// The weather at the start, when known: gusts and wind (mph), where
    /// the wind comes from (degrees), relative humidity (%).
    pub gust: Option<f64>,
    pub wind: Option<f64>,
    pub wind_dir: Option<f64>,
    pub rh: Option<f64>,
}

/// The first `bins` half-hour bins (two quarter-hour bins each) of a fire,
/// and its context, as one vector. Power and pixels are on a log scale,
/// spread in tens of km, so each counts about the same.
pub fn shape_of(f: &Fire, c: &Context, slots: usize, per: usize) -> Vec<f32> {
    let bins = filled(&f.bins);
    let mut v = Vec::with_capacity(slots * 3 + 6);
    let mut last_spread = 0.0;
    for h in 0..slots {
        let group: Vec<&Bin> = (0..per).filter_map(|k| bins.get(h * per + k)).collect();
        let mw = group.iter().map(|x| x.mw).fold(0.0, f64::max);
        let px = group.iter().map(|x| x.px).max().unwrap_or(0);
        let sp = group.last().map_or(last_spread, |x| x.spread.max(last_spread));
        last_spread = sp;
        v.push(((1.0 + mw).ln() / (1.0f64 + 5000.0).ln()) as f32);
        v.push(((1.0 + px as f64).ln() / (1.0f64 + 300.0).ln()) as f32);
        v.push((sp / 20.0).min(2.0) as f32);
    }
    let w = 0.5;
    v.push((w * (c.trees + c.shrubs) / 100.0) as f32);
    v.push((w * c.grass / 100.0) as f32);
    v.push((w * (c.slope / 30.0).min(1.5)) as f32);
    v.push((w * (1.0 + c.people5).ln() / (1.0f64 + 1e6).ln()) as f32);
    v.push((w * c.gust.unwrap_or(15.0) / 60.0) as f32);
    v.push((w * c.rh.unwrap_or(40.0) / 100.0) as f32);
    v
}

/// The bins with the empty ones between two scans of the fire filled by
/// the one before: a satellite that scans every 10 or 20 minutes leaves
/// quarter hours with no scan, which are not quarter hours with no fire.
pub fn filled(bins: &[Bin]) -> Vec<Bin> {
    let last_seen = bins.iter().rposition(|b| b.px > 0);
    let mut out: Vec<Bin> = Vec::with_capacity(bins.len());
    for (i, b) in bins.iter().enumerate() {
        if b.px == 0 && last_seen.is_some_and(|l| i < l) && i > 0 {
            let prev = out[i - 1].clone();
            out.push(prev);
        } else {
            out.push(b.clone());
        }
    }
    out
}

/// The first six hours (12 half-hour slots).
pub fn shape(f: &Fire, c: &Context) -> Vec<f32> {
    shape_of(f, c, 12, 2)
}

/// The first two hours (8 quarter-hour slots).
pub fn shape2(f: &Fire, c: &Context) -> Vec<f32> {
    shape_of(f, c, 8, 1)
}

/// The first hour (4 quarter-hour slots).
pub fn shape1(f: &Fire, c: &Context) -> Vec<f32> {
    shape_of(f, c, 4, 1)
}

/// A fire as a row of `ea_fires`. `px_km2` is the size of a pixel there.
pub fn row(f: &Fire, c: &Context, px_km2: f64, state: &str) -> Value {
    let iso = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    let name = if !c.town.is_empty() {
        format!("Fire near {}", c.town)
    } else if !c.adm2.is_empty() {
        format!("Fire in {}", c.adm2)
    } else if !c.adm1.is_empty() {
        format!("Fire in {}", c.adm1)
    } else {
        // Off the places (the sea, or over the border): where it is.
        format!("Fire at {:.2}, {:.2}", f.lat, f.lon)
    };
    let first: Vec<Value> = f.bins.iter().map(|b| json!([round1(b.mw), b.px, round1(b.spread)])).collect();
    let mut r = json!({
        "id": f.id, "region": f.region, "name": name, "town": c.town, "adm1": c.adm1, "adm2": c.adm2,
        "place": f.place, "lat": round4(f.lat), "lon": round4(f.lon), "start": iso(f.start), "last": iso(f.last),
        "state": state, "seen_start": f.seen_start, "scans": f.scans, "pixels": f.footprint.len(), "peak_px": f.peak_px,
        "peak_mw": round1(f.peak_mw), "mw_now": round1(if state == "burning" { f.mw_now } else { 0.0 }), "mw_1h": round1(f.mw_1h()),
        "energy": round1(f.energy / 1e6), "area_km2": round1(f.area_km2(px_km2)), "spread_km": round1(f.spread), "hours": round1(f.hours()),
        "people5": c.people5.round() as i64, "trees": round1(c.trees), "shrubs": round1(c.shrubs), "grass": round1(c.grass),
        "built": round1(c.built), "slope": round1(c.slope), "elev": c.elev.round() as i64, "climate": c.climate,
        "burned_before": c.burned_before, "gust": c.gust.map(round1), "wind": c.wind.map(round1), "wind_dir": c.wind_dir.map(round1), "rh": c.rh.map(round1),
        "first": serde_json::to_string(&first).unwrap_or_default(),
    });
    // Only a fire whose start was watched has a shape: one burning in the
    // first scan read began before it.
    if f.seen_start && f.scans >= 2 {
        r["shape"] = json!(shape(f, c));
        r["shape2"] = json!(shape2(f, c));
        r["shape1"] = json!(shape1(f, c));
    }
    r
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

/// The context of a fire from the places around where it started.
pub fn context(f: &Fire, places: &Places) -> Context {
    let mut c = Context::default();
    if let Some(p) = places.nearest(f.lat, f.lon, 3.0) {
        c.trees = p.trees;
        c.shrubs = p.shrubs;
        c.grass = p.grass;
        c.built = p.built;
        c.slope = p.slope;
        c.elev = p.elev;
        c.climate = p.climate.clone();
        c.town = p.town.clone();
        c.adm1 = p.adm1.clone();
        c.adm2 = p.adm2.clone();
        let year = chrono::DateTime::from_timestamp(f.start, 0).map_or(0, |d| chrono::Datelike::year(&d) as i64);
        c.burned_before = p.fire_years.iter().copied().filter(|&y| y < year).max();
    }
    c.people5 = places.people_within(f.lat, f.lon, 5.0);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::places::{Place, Places};

    fn px(lat: f64, lon: f64, mw: f64, row: u32) -> Pixel {
        Pixel { row, col: row, lat, lon, mask: 10, mw: Some(mw), area: None, temp: None }
    }

    fn places() -> Places {
        Places::new(vec![Place { id: "p1".into(), lat: 34.2, lon: -118.1, people: 5000.0, town: "Altadena".into(), ..Default::default() }])
    }

    #[test]
    fn pixels_near_each_other_are_one_fire_and_a_far_one_another() {
        let mut t = Tracker::new("california");
        let p = places();
        let ids = t.scan(1000, 300, &[px(34.20, -118.10, 50.0, 1), px(34.22, -118.10, 60.0, 2), px(36.0, -119.0, 10.0, 3)], &p);
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
        assert_eq!(t.fires.len(), 2);
        // The first scan read: these began before it.
        assert!(t.fires.iter().all(|f| !f.seen_start));
        // A pixel in the next scan near the first fire joins it.
        let ids2 = t.scan(1300, 300, &[px(34.25, -118.12, 200.0, 4)], &p);
        assert_eq!(ids2[0], ids[0]);
        let f = t.fires.iter().find(|f| f.id == ids[0]).unwrap();
        assert_eq!(f.scans, 2);
        assert_eq!(f.peak_mw, 200.0);
        assert_eq!(f.footprint.len(), 3);
        // A new fire later was watched starting.
        let ids3 = t.scan(1600, 300, &[px(37.0, -120.0, 5.0, 9)], &p);
        assert!(t.fires.iter().find(|f| f.id == ids3[0]).unwrap().seen_start);
    }

    #[test]
    fn a_fire_unseen_for_the_gap_is_out() {
        let mut t = Tracker::new("california");
        let p = places();
        t.scan(0, 300, &[px(34.2, -118.1, 5.0, 1)], &p);
        t.scan(GAP_S + 300, 300, &[], &p);
        assert!(t.fires.is_empty());
        assert_eq!(t.take_out().len(), 1);
    }

    #[test]
    fn two_fires_that_grow_together_become_the_older() {
        let mut t = Tracker::new("california");
        let p = places();
        t.scan(0, 300, &[px(34.0, -118.0, 5.0, 1), px(34.0, -118.1, 5.0, 2)], &p);
        assert_eq!(t.fires.len(), 2);
        // A pixel between them, within reach of both.
        t.scan(300, 300, &[px(34.0, -118.05, 5.0, 3)], &p);
        assert_eq!(t.fires.len(), 1);
        assert_eq!(t.fires[0].footprint.len(), 3);
    }

    #[test]
    fn a_quarter_hour_with_no_scan_keeps_the_one_before() {
        let b = |px: u32| Bin { mw: px as f64, px, spread: 0.0 };
        let got: Vec<u32> = filled(&[b(3), b(0), b(5), b(0), b(0)]).iter().map(|x| x.px).collect();
        assert_eq!(got, vec![3, 3, 5, 0, 0], "a gap is filled; the end, after the last scan, is not");
    }

    #[test]
    fn a_shape_has_its_dimensions() {
        let mut t = Tracker::new("california");
        let p = places();
        for i in 0..30 {
            t.scan(i * 900, 900, &[px(34.2, -118.1, 10.0 * i as f64, i as u32)], &p);
        }
        let f = &t.fires[0];
        let c = context(f, &p);
        assert_eq!(shape(f, &c).len(), SHAPE_DIM);
        assert_eq!(shape2(f, &c).len(), SHAPE2_DIM);
        assert_eq!(shape1(f, &c).len(), SHAPE1_DIM);
        assert_eq!(window_for(0.5), None);
        assert_eq!(window_for(1.5).map(|w| w.0), Some("shape1"));
        assert_eq!(window_for(7.0).map(|w| w.0), Some("shape"));
        assert_eq!(c.town, "Altadena");
    }
}
