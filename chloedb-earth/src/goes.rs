//! GOES's fire product (ABI L2 FDC), as NOAA publishes it on AWS: one
//! NetCDF-4 file a scan, every five minutes over the United States
//! (`FDCC`, GOES-West's CONUS sector) and every ten over the whole disk
//! (`FDCF`, GOES-East, which sees Chile). A file's `Mask` says what each
//! pixel is (10 to 15 a fire, by how sure; 30 to 35 the same, filtered over
//! time), `Power` its fire radiative power in megawatts, `Area` and `Temp`
//! the fire's size and temperature when they could be worked out. Pixels
//! sit on the satellite's fixed grid of scan angles, turned into latitude
//! and longitude here as NOAA's product guide gives it.

use anyhow::{anyhow, bail, Context, Result};
use netcdf_reader::NcFile;

/// A satellite and product, and the bucket that has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub sat: &'static str,
    pub bucket: &'static str,
    pub product: &'static str,
    /// Seconds between scans.
    pub every: i64,
}

pub const G16_FULL: Source = Source { sat: "G16", bucket: "noaa-goes16", product: "ABI-L2-FDCF", every: 600 };
pub const G16_CONUS: Source = Source { sat: "G16", bucket: "noaa-goes16", product: "ABI-L2-FDCC", every: 300 };
pub const G17_CONUS: Source = Source { sat: "G17", bucket: "noaa-goes17", product: "ABI-L2-FDCC", every: 300 };
pub const G18_CONUS: Source = Source { sat: "G18", bucket: "noaa-goes18", product: "ABI-L2-FDCC", every: 300 };
pub const G19_FULL: Source = Source { sat: "G19", bucket: "noaa-goes19", product: "ABI-L2-FDCF", every: 600 };

/// The satellite that watched `region` at `t` (Unix seconds): GOES-West
/// over California (GOES-17 from February 2019, GOES-18 from January
/// 2023), GOES-East's full disk over Chile (GOES-16, GOES-19 from April
/// 2025).
pub fn source(region: &str, t: i64) -> Source {
    match region {
        "chile" => {
            if t >= 1_744_000_000 {
                G19_FULL
            } else {
                G16_FULL
            }
        }
        _ => {
            if t >= 1_672_790_400 {
                G18_CONUS
            } else if t >= 1_549_929_600 {
                G17_CONUS
            } else {
                G16_CONUS
            }
        }
    }
}

/// One pixel a scan said was burning.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixel {
    pub row: u32,
    pub col: u32,
    pub lat: f64,
    pub lon: f64,
    pub mask: i16,
    /// Megawatts, when the product could work it out.
    pub mw: Option<f64>,
    /// Square metres burning inside the pixel.
    pub area: Option<f64>,
    /// Kelvin.
    pub temp: Option<f64>,
}

impl Pixel {
    /// How sure the product is, in a word: 10 and 30 are processed fires,
    /// 11 and 31 saturated ones (the hottest), 12 and 32 seen through
    /// cloud, then high, medium and low probability.
    pub fn confidence(&self) -> &'static str {
        match self.mask % 20 {
            10 | 11 => "high",
            13 => "high",
            12 | 14 => "medium",
            _ => "low",
        }
    }
}

/// Whether a mask value is a fire: processed (10, 30), saturated (11,
/// 31), cloudy (12, 32), and high and medium probability (13, 14, 33, 34).
/// Low probability (15, 35) is left out: most such pixels are hot ground or
/// cloud edges, with no power worked out (a whole desert valley on a summer
/// night), and beside a real fire they would count as fire burning.
pub fn is_fire(m: i16) -> bool {
    (10..=14).contains(&m) || (30..=34).contains(&m)
}

/// A scan's fire pixels inside `bounds` (south, north, west, east).
pub fn read(bytes: Vec<u8>, bounds: (f64, f64, f64, f64)) -> Result<Vec<Pixel>> {
    let f = NcFile::from_vec(bytes).map_err(|e| anyhow!("not a NetCDF file: {e}"))?;
    let x = f.read_variable_unpacked("x").map_err(|e| anyhow!("x: {e}"))?;
    let y = f.read_variable_unpacked("y").map_err(|e| anyhow!("y: {e}"))?;
    let (nx, ny) = (x.len(), y.len());
    let p = f.variable("goes_imager_projection").map_err(|e| anyhow!("projection: {e}"))?;
    let attr = |k: &str| p.attribute(k).and_then(|a| a.value.as_f64()).ok_or_else(|| anyhow!("projection's {k}"));
    let proj = Projection {
        h: attr("perspective_point_height")? + attr("semi_major_axis")?,
        req: attr("semi_major_axis")?,
        rpol: attr("semi_minor_axis")?,
        lon0: attr("longitude_of_projection_origin")?.to_radians(),
    };
    let (s, n, w, e) = bounds;
    // The rows and columns the bounds can fall on: a full disk is 29 million
    // pixels a variable, and a region a corner of it.
    let xs: Vec<f64> = x.iter().copied().collect();
    let ys: Vec<f64> = y.iter().copied().collect();
    let Some(((wr0, wr1), (wc0, wc1))) = proj.window(&xs, &ys, bounds) else { return Ok(Vec::new()) };
    let slice = |r0: usize, r1: usize, c0: usize, c1: usize| netcdf_reader::NcSliceInfo {
        selections: vec![
            netcdf_reader::NcSliceInfoElem::Slice { start: r0 as u64, end: r1 as u64, step: 1 },
            netcdf_reader::NcSliceInfoElem::Slice { start: c0 as u64, end: c1 as u64, step: 1 },
        ],
    };
    let mask: ndarray::ArrayD<i16> = f.read_variable_slice("Mask", &slice(wr0, wr1, wc0, wc1)).map_err(|e| anyhow!("Mask: {e}"))?;
    if mask.ndim() != 2 {
        bail!("Mask has {} dimensions", mask.ndim());
    }
    let mut kept = Vec::new();
    for (ix, &m) in mask.indexed_iter() {
        if !is_fire(m) {
            continue;
        }
        let (r, c) = (ix[0] + wr0, ix[1] + wc0);
        if r >= ny || c >= nx {
            continue;
        }
        let Some((lat, lon)) = proj.to_lat_lon(xs[c], ys[r]) else { continue };
        if lat < s || lat > n || lon < w || lon > e {
            continue;
        }
        kept.push((r, c, lat, lon, m));
    }
    if kept.is_empty() {
        return Ok(Vec::new());
    }
    // Only the window around the pixels kept: a full disk's variables are
    // 29 million values each, and a region's fires sit in a corner of it.
    let (r0, r1) = (kept.iter().map(|k| k.0).min().unwrap_or(0), kept.iter().map(|k| k.0).max().unwrap_or(0) + 1);
    let (c0, c1) = (kept.iter().map(|k| k.1).min().unwrap_or(0), kept.iter().map(|k| k.1).max().unwrap_or(0) + 1);
    let window = netcdf_reader::NcSliceInfo {
        selections: vec![
            netcdf_reader::NcSliceInfoElem::Slice { start: r0 as u64, end: r1 as u64, step: 1 },
            netcdf_reader::NcSliceInfoElem::Slice { start: c0 as u64, end: c1 as u64, step: 1 },
        ],
    };
    let power = f.read_variable_slice_unpacked_masked("Power", &window).ok();
    let area = f.read_variable_slice_unpacked_masked("Area", &window).ok();
    let temp = f.read_variable_slice_unpacked_masked("Temp", &window).ok();
    let at = |a: &Option<ndarray::ArrayD<f64>>, r: usize, c: usize| a.as_ref().and_then(|a| a.get([r - r0, c - c0]).copied()).filter(|v| v.is_finite());
    Ok(kept
        .into_iter()
        .map(|(r, c, lat, lon, m)| Pixel {
            row: r as u32,
            col: c as u32,
            lat,
            lon,
            mask: m,
            mw: at(&power, r, c),
            area: at(&area, r, c),
            temp: at(&temp, r, c),
        })
        .collect())
}

/// The fixed grid's geometry: the satellite's height above the Earth's
/// centre, the Earth's radii and the longitude under the satellite.
#[derive(Clone, Copy, Debug)]
pub struct Projection {
    pub h: f64,
    pub req: f64,
    pub rpol: f64,
    pub lon0: f64,
}

impl Projection {
    /// The scan angles (radians) of a point on the Earth, `None` when the
    /// satellite cannot see it.
    pub fn to_scan(&self, lat: f64, lon: f64) -> Option<(f64, f64)> {
        let (req2, rpol2) = (self.req * self.req, self.rpol * self.rpol);
        let phi = (rpol2 / req2 * lat.to_radians().tan()).atan();
        let e2 = (req2 - rpol2) / req2;
        let rc = self.rpol / (1.0 - e2 * phi.cos().powi(2)).sqrt();
        let dl = lon.to_radians() - self.lon0;
        let sx = self.h - rc * phi.cos() * dl.cos();
        let sy = -rc * phi.cos() * dl.sin();
        let sz = rc * phi.sin();
        // Hidden behind the Earth's limb.
        if self.h * (self.h - sx) < sy * sy + req2 / rpol2 * sz * sz {
            return None;
        }
        Some(((-sy / (sx * sx + sy * sy + sz * sz).sqrt()).asin(), (sz / sx).atan()))
    }

    /// The rows and columns (end exclusive) of the grid `x`, `y` that the
    /// bounds (south, north, west, east) fall on, a few pixels wider.
    pub fn window(&self, x: &[f64], y: &[f64], (s, n, w, e): (f64, f64, f64, f64)) -> Option<((usize, usize), (usize, usize))> {
        let (mut xa, mut xb, mut ya, mut yb) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for i in 0..=8 {
            for j in 0..=8 {
                let lat = s + (n - s) * i as f64 / 8.0;
                let lon = w + (e - w) * j as f64 / 8.0;
                if let Some((sx, sy)) = self.to_scan(lat, lon) {
                    xa = xa.min(sx);
                    xb = xb.max(sx);
                    ya = ya.min(sy);
                    yb = yb.max(sy);
                }
            }
        }
        if xa > xb {
            return None;
        }
        let span = |v: &[f64], a: f64, b: f64| -> Option<(usize, usize)> {
            let idx: Vec<usize> = (0..v.len()).filter(|&i| v[i] >= a && v[i] <= b).collect();
            let (lo, hi) = (*idx.first()?, *idx.last()?);
            Some((lo.saturating_sub(4), (hi + 5).min(v.len())))
        };
        let pad = 4.0 * (x.get(1).zip(x.first()).map(|(b, a)| (b - a).abs()).unwrap_or(5.6e-5));
        Some((span(y, ya - pad, yb + pad)?, span(x, xa - pad, xb + pad)?))
    }

    /// Latitude and longitude (degrees) of the scan angles `x` and `y`
    /// (radians), or `None` off the Earth's edge.
    pub fn to_lat_lon(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (sx_, cx) = x.sin_cos();
        let (sy_, cy) = y.sin_cos();
        let k = (self.req * self.req) / (self.rpol * self.rpol);
        let a = sx_ * sx_ + cx * cx * (cy * cy + k * sy_ * sy_);
        let b = -2.0 * self.h * cx * cy;
        let c = self.h * self.h - self.req * self.req;
        let d = b * b - 4.0 * a * c;
        if d < 0.0 {
            return None;
        }
        let rs = (-b - d.sqrt()) / (2.0 * a);
        let sx = rs * cx * cy;
        let sy = -rs * sx_;
        let sz = rs * cx * sy_;
        let lat = (k * sz / ((self.h - sx).powi(2) + sy * sy).sqrt()).atan().to_degrees();
        let lon = (self.lon0 - (sy / (self.h - sx)).atan()).to_degrees();
        Some((lat, lon))
    }
}

/// One scan's file in a bucket.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scan {
    /// The scan's start, Unix seconds.
    pub start: i64,
    pub key: String,
}

/// The scan start in a key's name (`..._s20250070201170_...`: year, day of
/// the year, hours, minutes, seconds and tenths).
pub fn start_of(key: &str) -> Option<i64> {
    let i = key.find("_s")? + 2;
    let s = key.get(i..i + 13)?;
    let year: i32 = s[0..4].parse().ok()?;
    let doy: u32 = s[4..7].parse().ok()?;
    let (h, m, sec): (u32, u32, u32) = (s[7..9].parse().ok()?, s[9..11].parse().ok()?, s[11..13].parse().ok()?);
    let d = chrono::NaiveDate::from_yo_opt(year, doy)?.and_hms_opt(h, m, sec)?;
    Some(d.and_utc().timestamp())
}

/// The scans of an hour (`t` any time in it), oldest first.
pub async fn list_hour(http: &reqwest::Client, src: Source, t: i64) -> Result<Vec<Scan>> {
    let d = chrono::DateTime::from_timestamp(t, 0).ok_or_else(|| anyhow!("a time"))?;
    let prefix = format!("{}/{}", src.product, d.format("%Y/%j/%H/"));
    let url = format!("https://{}.s3.amazonaws.com/?list-type=2&prefix={prefix}", src.bucket);
    let body = http.get(&url).send().await.with_context(|| url.clone())?.error_for_status()?.text().await?;
    let mut out: Vec<Scan> = keys(&body)
        .into_iter()
        .filter_map(|k| start_of(&k).map(|start| Scan { start, key: k }))
        .collect();
    out.sort();
    Ok(out)
}

/// The `<Key>`s of an S3 listing.
pub fn keys(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<Key>") {
        rest = &rest[i + 5..];
        let Some(j) = rest.find("</Key>") else { break };
        out.push(rest[..j].to_string());
        rest = &rest[j + 6..];
    }
    out
}

/// A scan's bytes.
pub async fn fetch(http: &reqwest::Client, src: Source, key: &str) -> Result<Vec<u8>> {
    let url = format!("https://{}.s3.amazonaws.com/{key}", src.bucket);
    let mut wait = std::time::Duration::from_millis(500);
    for attempt in 0..4 {
        match http.get(&url).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => match r.bytes().await {
                Ok(b) => return Ok(b.to_vec()),
                Err(e) if attempt < 3 => tracing::debug!(error = %e, "goes body"),
                Err(e) => return Err(e).context(url),
            },
            Err(e) if attempt < 3 => tracing::debug!(error = %e, "goes fetch"),
            Err(e) => return Err(e).context(url),
        }
        tokio::time::sleep(wait).await;
        wait *= 2;
    }
    bail!("{url}: no answer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_names_its_start() {
        let k = "ABI-L2-FDCC/2025/007/02/OR_ABI-L2-FDCC-M6_G18_s20250070201170_e20250070203543_c20250070204107.nc";
        // Day 7 of 2025 is 7 January; 02:01:17 UTC.
        assert_eq!(start_of(k), Some(1_736_215_277));
    }

    #[test]
    fn a_listing_gives_its_keys() {
        let xml = "<ListBucketResult><Contents><Key>a/b.nc</Key><Size>1</Size></Contents><Contents><Key>a/c.nc</Key></Contents></ListBucketResult>";
        assert_eq!(keys(xml), vec!["a/b.nc".to_string(), "a/c.nc".to_string()]);
    }

    #[test]
    fn the_point_under_the_satellite_is_on_the_equator_at_its_longitude() {
        let p = Projection { h: 35_786_023.0 + 6_378_137.0, req: 6_378_137.0, rpol: 6_356_752.31414, lon0: (-137.0f64).to_radians() };
        let (lat, lon) = p.to_lat_lon(0.0, 0.0).unwrap();
        assert!(lat.abs() < 1e-9 && (lon + 137.0).abs() < 1e-9);
        // Off the disk's edge there is no Earth.
        assert!(p.to_lat_lon(0.2, 0.0).is_none());
    }

    #[test]
    fn low_probability_pixels_are_not_fires() {
        assert!([10, 11, 12, 13, 14, 30, 31, 32, 33, 34].iter().all(|&m| is_fire(m)));
        assert!([15, 35, 0, 40, 50, 100].iter().all(|&m| !is_fire(m)));
    }

    #[test]
    fn the_satellite_by_region_and_time() {
        assert_eq!(source("california", 1_736_215_277).sat, "G18");
        assert_eq!(source("california", 1_600_000_000).sat, "G17");
        assert_eq!(source("chile", 1_706_900_000).sat, "G16");
        assert_eq!(source("chile", 1_790_000_000).sat, "G19");
    }
}
