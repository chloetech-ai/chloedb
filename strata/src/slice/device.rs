//! A device's side of a slice: a local database of the tiles it fetched,
//! searched with the cloud's statistics and checked against the bound
//! ([`Device::search`]).

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use flate2::read::GzDecoder;
use serde_json::Value as Json;

use super::{query_terms, Manifest, Search, Tile};
use crate::db::{Db, WriteOp};
use crate::error::{Error, Result};
use crate::query::exec::{ExecOptions, ResultSet};
use crate::query::stats::{FieldStats, TermStats};
use crate::storage::DbOptions;
use crate::value::Value;

/// A device holding some of a slice's tiles.
pub struct Device {
    db: Db,
    m: Manifest,
    words: HashMap<String, (u64, f32)>,
    /// The tiles with rows, by tile.
    tiles: HashMap<Tile, u64>,
    /// The tiles held, with the keys of their rows.
    held: BTreeMap<Tile, Vec<Value>>,
}

/// Which tiles to fetch and which to let go ([`Device::plan`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Tiles with rows in the square around the user that the device lacks,
    /// nearest first, and their bytes.
    pub fetch: Vec<(Tile, u64)>,
    /// Tiles held that are now farther than one ring beyond the square.
    pub drop: Vec<Tile>,
}

/// A search answered on the device ([`Device::search`]).
#[derive(Debug)]
pub struct Answer {
    /// The local top k, every column, with the scores they were ranked by.
    pub rows: ResultSet,
    /// The most a place the device does not hold could score.
    pub bound: f64,
    /// The sum of the query's words' best text scores (the bound before
    /// the fade).
    pub text_bound: f64,
    /// The distance from the user to the nearest tile with rows the device
    /// lacks (infinite when it holds them all).
    pub edge_m: f64,
    /// Whether `rows` is the cloud's top k: k rows whose last scores at
    /// least `bound`, or no place anywhere has a word of the query.
    pub proved: bool,
}

impl Device {
    /// A device with no tiles yet, its database in directory `dir`.
    pub fn create(dir: &Path, m: Manifest) -> Result<Device> {
        std::fs::create_dir_all(dir)?;
        let db = Db::create(&dir.join("slice.strata"), m.schema.clone(), DbOptions::default())?;
        let words = m.word_map().into_iter().map(|(w, v)| (w.to_string(), v)).collect();
        let tiles = m.tiles.iter().map(|t| (t.tile(), t.bytes)).collect();
        Ok(Device { db, m, words, tiles, held: BTreeMap::new() })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.m
    }

    /// The tiles held, and how many rows each holds.
    pub fn held(&self) -> impl Iterator<Item = (Tile, usize)> + '_ {
        self.held.iter().map(|(t, k)| (*t, k.len()))
    }

    pub fn holds(&self, t: Tile) -> bool {
        self.held.contains_key(&t)
    }

    /// The tiles to fetch so that the device holds the square of
    /// `2 * reach + 1` tiles on a side around the user's tile, and those it
    /// can let go (beyond one more ring, so that walking along a tile's
    /// edge does not fetch and drop the same tiles over and over).
    pub fn plan(&self, lat: f64, lon: f64, reach: i32) -> Plan {
        let g = &self.m.grid;
        let u = g.tile(lat, lon);
        let (x, y) = g.xy(lat, lon);
        let ring = |t: Tile| (t.x - u.x).abs().max((t.y - u.y).abs());
        let mut fetch: Vec<(Tile, u64)> = self.tiles.iter().filter(|(t, _)| ring(**t) <= reach && !self.holds(**t)).map(|(t, b)| (*t, *b)).collect();
        fetch.sort_by(|a, b| g.distance(x, y, a.0).total_cmp(&g.distance(x, y, b.0)).then(a.0.cmp(&b.0)));
        let drop = self.held.keys().filter(|t| ring(**t) > reach + 1).copied().collect();
        Plan { fetch, drop }
    }

    /// Adds a tile's rows from its file's bytes (gzipped JSON lines, as
    /// [`super::build`] wrote them). A tile already held is left alone.
    pub fn add_tile(&mut self, t: Tile, gz: &[u8]) -> Result<usize> {
        if self.holds(t) {
            return Ok(0);
        }
        let mut text = String::new();
        GzDecoder::new(gz).read_to_string(&mut text).map_err(|e| Error::Query(format!("tile {}_{}: {e}", t.x, t.y)))?;
        let s = &self.m.schema;
        let pk = s.primary_key.expect("a slice's table has a primary key");
        let mut docs = Vec::new();
        for line in text.lines().filter(|l| !l.is_empty()) {
            let vals: Vec<Json> = serde_json::from_str(line).map_err(|e| Error::Query(format!("tile {}_{}: {e}", t.x, t.y)))?;
            if vals.len() != s.fields.len() {
                return Err(Error::Query(format!("tile {}_{}: a row of {} values for {} fields", t.x, t.y, vals.len(), s.fields.len())));
            }
            let obj: serde_json::Map<String, Json> = s.fields.iter().map(|f| f.name.clone()).zip(vals).collect();
            docs.push(crate::ingest::document(s, &Json::Object(obj))?);
        }
        let keys: Vec<Value> = docs.iter().map(|d| d[pk].clone()).collect();
        if !docs.is_empty() {
            self.db.load(docs)?;
        }
        let n = keys.len();
        self.held.insert(t, keys);
        Ok(n)
    }

    /// Lets a tile go: deletes its rows.
    pub fn drop_tile(&mut self, t: Tile) -> Result<()> {
        let Some(keys) = self.held.remove(&t) else { return Ok(()) };
        if keys.is_empty() {
            return Ok(());
        }
        let table = self.m.schema.table.clone();
        self.db.write_ops(keys.into_iter().map(|key| WriteOp::Delete { table: table.clone(), key }).collect())?;
        Ok(())
    }

    /// The distance from (`lat`, `lon`) to the nearest tile with rows that
    /// the device does not hold: every place it lacks is at least this far.
    pub fn edge(&self, lat: f64, lon: f64) -> f64 {
        let g = &self.m.grid;
        let (x, y) = g.xy(lat, lon);
        self.tiles.keys().filter(|t| !self.holds(**t)).map(|t| g.distance(x, y, *t)).fold(f64::INFINITY, f64::min)
    }

    /// Runs `s` on the tiles held, scored with the cloud's statistics, and
    /// says whether its answer is the cloud's ([`Answer::proved`]). Rows
    /// tied with the k-th may be other rows of the same score than the
    /// cloud's, which breaks ties by its own row order.
    pub fn search(&self, s: &Search) -> Result<Answer> {
        let m = &self.m;
        let terms = query_terms(&m.schema, &m.text, &s.text)?;
        let mut df = BTreeMap::new();
        let mut text_bound = 0f64;
        for (t, w) in &terms {
            let (d, b) = self.words.get(t).copied().unwrap_or((0, 0.0));
            df.insert(t.clone(), d);
            text_bound += *w as f64 * b as f64;
        }
        let stats = TermStats { fields: [(m.text.clone(), FieldStats { n_docs: m.n_docs, total_len: m.total_len, df })].into() };
        let opts = ExecOptions { stats: Some(Arc::new(stats)), ..self.db.exec.clone() };
        let rows = self.db.query_with(&s.sql(m), &s.params(m), &opts)?;
        let edge_m = self.edge(s.lat, s.lon);
        let fade = if edge_m.is_finite() { 1.0 / (1.0 + edge_m * edge_m / (s.d0_m * s.d0_m)) } else { 0.0 };
        let bound = text_bound * fade;
        let proved = bound == 0.0 || rows.scores.get(s.k - 1).is_some_and(|&kth| kth >= bound);
        Ok(Answer { rows, bound, text_bound, edge_m, proved })
    }

    /// The local database (to read what it holds).
    pub fn db(&self) -> &Db {
        &self.db
    }
}
