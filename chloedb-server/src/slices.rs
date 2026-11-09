//! The slices a server builds and serves ([`chloedb::slice`]), one directory
//! a table under `--slices`: what each holds (read from its manifest once),
//! whether its table changed since it was built, and how devices use it.
//!
//! The server counts what it serves (manifests, and each tile's downloads)
//! and what devices report of their searches (`POST
//! /v1/slices/<table>/usage`: how many they answered themselves, and why
//! they asked the cloud for the rest). The counts are kept in memory since
//! the server started, as the read counts of the index inspector are.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::slice::{BuildKey, Key, Manifest, TileEntry, Why};

use crate::ApiError;

/// Devices a table's usage tells apart; more are counted, not named.
const MAX_DEVICES: usize = 10_000;
/// One-minute buckets of usage kept: the last hour.
const MINUTES: usize = 60;

/// What a slice holds: its manifest without the words or centres.
#[derive(Clone, Debug)]
pub struct Info {
    pub table: String,
    pub version: u64,
    pub built_ms: u64,
    /// How it was cut, as a rebuild takes it (a vector slice's seed is
    /// not kept: a rebuild draws with the default).
    pub key: BuildKey,
    /// What the command center shows of the key: a geo slice's fields,
    /// grid, BM25 parameters and words; a vector slice's field, dimension
    /// and clusters.
    pub detail: Json,
    pub tiles: Vec<TileEntry>,
    pub rows: u64,
    pub manifest_bytes: u64,
    pub tile_bytes: u64,
}

/// A vector slice's k-means seed when a rebuild does not say.
pub const SEED: u64 = 7;

impl Info {
    pub fn of(m: &Manifest, manifest_bytes: u64) -> Info {
        let (key, detail) = match &m.key {
            Key::Geo(g) => (
                BuildKey::Geo { text: g.text.clone(), lat: g.lat.clone(), lon: g.lon.clone(), grid: g.grid, bm25: chloedb::text::Bm25Params { k1: g.k1, b: g.b } },
                json!({
                    "kind": "geo", "text": g.text, "lat": g.lat, "lon": g.lon, "grid": g.grid, "kx": g.grid.kx(), "ky": chloedb::slice::KY,
                    "k1": g.k1, "b": g.b, "n_docs": g.n_docs, "words": g.words.terms.len(),
                }),
            ),
            Key::Vector(v) => (
                BuildKey::Vector { field: v.field.clone(), clusters: v.centroids.len(), seed: SEED, codes: v.codes.is_some() },
                json!({"kind": "vector", "field": v.field, "dim": v.dim, "clusters": v.centroids.len(), "codes_bytes": v.codes.as_ref().map(|c| c.bytes)}),
            ),
        };
        Info {
            table: m.schema.table.clone(),
            version: m.version,
            built_ms: m.built_ms,
            key,
            detail,
            rows: m.tiles.iter().map(|t| t.rows).sum(),
            tile_bytes: m.tiles.iter().map(|t| t.bytes).sum(),
            tiles: m.tiles.clone(),
            manifest_bytes,
        }
    }
}

/// One minute of a slice's use.
#[derive(Clone, Copy, Debug, Default)]
struct Minute {
    at_ms: u64,
    searches: u64,
    on_device: u64,
    tiles: u64,
    bytes: u64,
}

/// How devices used a slice since the server started.
#[derive(Debug, Default)]
struct Usage {
    manifests: u64,
    manifest_bytes: u64,
    /// Downloads of a vector slice's codes, and their bytes.
    codes: u64,
    codes_bytes: u64,
    /// Downloads of each tile.
    tiles: HashMap<(i32, i32), u64>,
    tile_bytes: u64,
    statuses: u64,
    reports: u64,
    searches: u64,
    /// Searches by why they were answered where they were ([`Why::ALL`]'s order).
    why: [u64; 5],
    device_ms: f64,
    cloud_ms: f64,
    devices: HashSet<String>,
    first_ms: u64,
    last_ms: u64,
    minutes: VecDeque<Minute>,
}

impl Usage {
    fn minute(&mut self, now: u64) -> &mut Minute {
        let at = now / 60_000 * 60_000;
        if self.minutes.back().is_none_or(|m| m.at_ms != at) {
            self.minutes.push_back(Minute { at_ms: at, ..Minute::default() });
            while self.minutes.len() > MINUTES {
                self.minutes.pop_front();
            }
        }
        if self.first_ms == 0 {
            self.first_ms = now;
        }
        self.last_ms = now;
        self.minutes.back_mut().expect("pushed")
    }

    fn on_device(&self) -> u64 {
        self.why[0] + self.why[1]
    }

    fn summary(&self) -> Json {
        let why: serde_json::Map<String, Json> = Why::ALL.iter().zip(self.why).map(|(w, n)| (w.as_str().to_string(), json!(n))).collect();
        let asked = self.searches - self.on_device().min(self.searches);
        json!({
            "manifests": self.manifests, "manifest_bytes": self.manifest_bytes, "codes": self.codes, "codes_bytes": self.codes_bytes,
            "tile_downloads": self.tiles.values().sum::<u64>(), "tiles_downloaded": self.tiles.len(), "tile_bytes": self.tile_bytes,
            "statuses": self.statuses, "reports": self.reports, "devices": self.devices.len(),
            "searches": self.searches, "on_device": self.on_device(), "asked": asked, "why": why,
            "device_ms": if self.searches > 0 { json!(self.device_ms / self.searches as f64) } else { Json::Null },
            "cloud_ms": if asked > 0 { json!(self.cloud_ms / asked as f64) } else { Json::Null },
            "first_ms": self.first_ms, "last_ms": self.last_ms,
        })
    }
}

/// A slice this server built since it started: the table's write count
/// before the build took its snapshot, to tell whether it changed since.
#[derive(Clone, Copy, Debug)]
struct Mark {
    ops: u64,
}

#[derive(Debug, Default)]
struct Entry {
    info: Option<Arc<Info>>,
    mark: Option<Mark>,
    usage: Usage,
}

/// The slices under one directory.
#[derive(Debug)]
pub struct Slices {
    dir: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Slices {
    /// The slices in `dir`, which is made if missing.
    pub fn new(dir: &Path) -> Result<Slices, ApiError> {
        std::fs::create_dir_all(dir).map_err(|e| ApiError::bad(format!("create {}: {e}", dir.display())))?;
        Ok(Slices { dir: dir.to_path_buf(), entries: Mutex::new(HashMap::new()) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory of a table's slice, if the name is one a table may have.
    pub fn dir_of(&self, table: &str) -> Result<PathBuf, ApiError> {
        if table.is_empty() || !table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ApiError::bad(format!("{table:?} is not a table's name")));
        }
        Ok(self.dir.join(table))
    }

    /// The tables with a slice, by name.
    pub fn tables(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("manifest.json").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.') && self.dir_of(n).is_ok())
            .collect();
        out.sort();
        out
    }

    /// What a table's slice holds, read from its manifest the first time.
    pub fn info(&self, table: &str) -> Result<Arc<Info>, ApiError> {
        if let Some(i) = self.entries.lock().unwrap().get(table).and_then(|e| e.info.clone()) {
            return Ok(i);
        }
        let path = self.dir_of(table)?.join("manifest.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ApiError { status: 404, message: format!("table {table} has no slice (build it with POST /v1/admin/slices)") }),
            Err(e) => return Err(ApiError::internal(e)),
        };
        let m = Manifest::from_json(&text)?;
        let info = Arc::new(Info::of(&m, text.len() as u64));
        self.entries.lock().unwrap().entry(table.to_string()).or_default().info = Some(info.clone());
        Ok(info)
    }

    /// Notes a slice just built, and the table's write count before it.
    pub fn built(&self, info: Info, ops_before: u64) {
        let mut es = self.entries.lock().unwrap();
        let e = es.entry(info.table.clone()).or_default();
        e.info = Some(Arc::new(info));
        e.mark = Some(Mark { ops: ops_before });
    }

    /// Deletes a table's slice; whether it had one.
    pub fn remove(&self, table: &str) -> Result<bool, ApiError> {
        let dir = self.dir_of(table)?;
        self.entries.lock().unwrap().remove(table);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(ApiError::internal(e)),
        }
    }

    pub fn note_manifest(&self, table: &str, bytes: u64) {
        let now = now_ms();
        let mut es = self.entries.lock().unwrap();
        let u = &mut es.entry(table.to_string()).or_default().usage;
        u.minute(now);
        u.manifests += 1;
        u.manifest_bytes += bytes;
    }

    pub fn note_codes(&self, table: &str, bytes: u64) {
        let now = now_ms();
        let mut es = self.entries.lock().unwrap();
        let u = &mut es.entry(table.to_string()).or_default().usage;
        u.minute(now).bytes += bytes;
        u.codes += 1;
        u.codes_bytes += bytes;
    }

    pub fn note_tile(&self, table: &str, x: i32, y: i32, bytes: u64) {
        let now = now_ms();
        let mut es = self.entries.lock().unwrap();
        let u = &mut es.entry(table.to_string()).or_default().usage;
        let m = u.minute(now);
        m.tiles += 1;
        m.bytes += bytes;
        *u.tiles.entry((x, y)).or_default() += 1;
        u.tile_bytes += bytes;
    }

    /// Adds a device's report: `{device?, searches, why: {proved, no_match,
    /// short, bound, stale}, device_ms?, cloud_ms?}`, counts since its
    /// last report (`device_ms` and `cloud_ms` their sums).
    pub fn report(&self, table: &str, b: &Json) -> Result<(), ApiError> {
        self.info(table)?;
        let n = |j: Option<&Json>, what: &str| -> Result<u64, ApiError> {
            match j {
                None | Some(Json::Null) => Ok(0),
                Some(v) => v.as_u64().filter(|&x| x <= 1_000_000_000).ok_or_else(|| ApiError::bad(format!("{what} is a count, not {v}"))),
            }
        };
        let ms = |k: &str| -> Result<f64, ApiError> {
            match b.get(k) {
                None | Some(Json::Null) => Ok(0.0),
                Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| ApiError::bad(format!("{k} is a sum of milliseconds, not {v}"))),
            }
        };
        let mut why = [0u64; 5];
        if let Some(w) = b.get("why") {
            let w = w.as_object().ok_or_else(|| ApiError::bad("why is an object of counts by reason"))?;
            for (k, v) in w {
                let i = Why::ALL.iter().position(|x| x.as_str() == k).ok_or_else(|| ApiError::bad(format!("{k:?} is not a reason (proved, no_match, short, bound, stale)")))?;
                why[i] = n(Some(v), k)?;
            }
        }
        let searches = n(b.get("searches"), "searches")?.max(why.iter().sum());
        let (device_ms, cloud_ms) = (ms("device_ms")?, ms("cloud_ms")?);
        let device = b.get("device").and_then(|d| d.as_str()).map(|d| d.chars().take(64).collect::<String>());
        let now = now_ms();
        let mut es = self.entries.lock().unwrap();
        let u = &mut es.entry(table.to_string()).or_default().usage;
        let m = u.minute(now);
        m.searches += searches;
        m.on_device += why[0] + why[1];
        u.reports += 1;
        u.searches += searches;
        for (a, b) in u.why.iter_mut().zip(why) {
            *a += b;
        }
        u.device_ms += device_ms;
        u.cloud_ms += cloud_ms;
        if let Some(d) = device {
            if u.devices.len() < MAX_DEVICES {
                u.devices.insert(d);
            }
        }
        Ok(())
    }

    /// Whether a table's slice is its current version: unchanged since the
    /// build, by the write count this server noted before it (a slice it
    /// built since it started), else by the database's version. A write
    /// to any table changes the database's version, so a slice the server
    /// did not build since it started may be called stale when it is not.
    fn fresh(&self, info: &Info, db: &Db) -> bool {
        let mark = self.entries.lock().unwrap().get(&info.table).and_then(|e| e.mark);
        match mark {
            Some(m) => db.storage().activity(&info.table).map(|a| a.ops) == Some(m.ops),
            None => db.snapshot().txn() == info.version,
        }
    }

    /// Whether a table's slice is its current version ([`Slices::fresh`]).
    pub fn is_fresh(&self, table: &str, db: &Db) -> Result<bool, ApiError> {
        Ok(self.fresh(&*self.info(table)?, db))
    }

    /// What a device asks before it trusts its slice: `{table, version,
    /// built_ms, fresh, changed_ms}`. A device whose manifest is of
    /// another version fetches the new one; one that is not `fresh` has
    /// proofs that hold for the slice's version of the table only.
    pub fn status(&self, table: &str, db: &Db) -> Result<Json, ApiError> {
        let i = self.info(table)?;
        let fresh = self.fresh(&i, db);
        {
            let mut es = self.entries.lock().unwrap();
            let u = &mut es.entry(table.to_string()).or_default().usage;
            u.minute(now_ms());
            u.statuses += 1;
        }
        let changed = db.storage().activity(table).map_or(0, |a| a.last_ms);
        Ok(json!({"table": i.table, "version": i.version, "built_ms": i.built_ms, "fresh": fresh, "changed_ms": if fresh { Json::Null } else { json!(changed) }}))
    }

    /// A slice and its use, for the command center: what it holds, whether
    /// it is fresh, and the counts; `tiles` adds each tile's rows, bytes
    /// and downloads, and the last hour by minute.
    pub fn describe(&self, table: &str, db: &Db, tiles: bool) -> Result<Json, ApiError> {
        let i = self.info(table)?;
        let fresh = self.fresh(&i, db);
        let changed = db.storage().activity(table).map_or(0, |a| a.last_ms);
        let es = self.entries.lock().unwrap();
        let empty = Usage::default();
        let u = es.get(table).map_or(&empty, |e| &e.usage);
        let mut out = json!({
            "table": i.table, "version": i.version, "built_ms": i.built_ms, "fresh": fresh, "changed_ms": changed,
            "rows": i.rows, "tiles": i.tiles.len(), "manifest_bytes": i.manifest_bytes, "tile_bytes": i.tile_bytes,
            "largest_tile_bytes": i.tiles.iter().map(|t| t.bytes).max().unwrap_or(0),
            "usage": u.summary(),
        });
        if let (Json::Object(o), Json::Object(d)) = (&mut out, &i.detail) {
            o.extend(d.clone());
        }
        if tiles {
            out["tile_list"] = Json::Array(i.tiles.iter().map(|t| json!([t.x, t.y, t.rows, t.bytes, u.tiles.get(&(t.x, t.y)).copied().unwrap_or(0)])).collect());
            out["minutes"] = Json::Array(u.minutes.iter().map(|m| json!({"at_ms": m.at_ms, "searches": m.searches, "on_device": m.on_device, "tiles": m.tiles, "bytes": m.bytes})).collect());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_add_up_and_bad_ones_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let s = Slices::new(dir.path()).unwrap();
        assert_eq!(s.report("places", &json!({"searches": 1})).unwrap_err().status, 404);
        std::fs::create_dir_all(dir.path().join("places")).unwrap();
        let schema = chloedb::schema::Schema::new(
            "places",
            vec![
                chloedb::schema::Field::new("id", chloedb::schema::FieldType::Int),
                chloedb::schema::Field::new("body", chloedb::schema::FieldType::Text { positions: false }),
                chloedb::schema::Field::new("lat", chloedb::schema::FieldType::Float),
                chloedb::schema::Field::new("lon", chloedb::schema::FieldType::Float),
            ],
            Some("id"),
        )
        .unwrap();
        let m = Manifest {
            format: chloedb::slice::FORMAT,
            version: 3,
            built_ms: 1,
            schema,
            key: Key::Geo(chloedb::slice::Geo {
                text: "body".into(),
                lat: "lat".into(),
                lon: "lon".into(),
                grid: chloedb::slice::Grid::new(40.7, -74.0, 1000.0).unwrap(),
                k1: 1.2,
                b: 0.75,
                n_docs: 2,
                total_len: 4,
                words: Default::default(),
            }),
            tiles: vec![TileEntry { x: 0, y: 0, rows: 2, bytes: 90 }],
        };
        std::fs::write(dir.path().join("places/manifest.json"), m.to_json()).unwrap();
        s.report("places", &json!({"device": "a", "searches": 5, "why": {"proved": 3, "bound": 2}, "device_ms": 10.0, "cloud_ms": 8.0})).unwrap();
        s.report("places", &json!({"device": "b", "why": {"no_match": 1, "stale": 1}})).unwrap();
        s.note_tile("places", 0, 0, 90);
        s.note_tile("places", 0, 0, 90);
        for bad in [json!({"searches": -1}), json!({"why": {"maybe": 1}}), json!({"why": 3}), json!({"device_ms": "x"})] {
            assert_eq!(s.report("places", &bad).unwrap_err().status, 400, "{bad}");
        }
        let es = s.entries.lock().unwrap();
        let u = &es["places"].usage;
        assert_eq!((u.searches, u.on_device(), u.devices.len(), u.reports), (7, 4, 2, 2));
        let sum = u.summary();
        assert_eq!((sum["asked"].as_u64(), sum["tile_downloads"].as_u64(), sum["tile_bytes"].as_u64()), (Some(3), Some(2), Some(180)));
        assert_eq!(sum["why"]["stale"], json!(1));
        assert_eq!(sum["cloud_ms"], json!(8.0 / 3.0));
    }
}
