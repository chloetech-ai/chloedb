//! A device's side of a slice: a local database of the tiles it fetched,
//! searched with the cloud's statistics and checked against the bound
//! ([`Device::search`], [`Device::search_vector`]), and the cloud asked
//! when the check fails ([`Device::search_or_ask`]).

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use flate2::read::GzDecoder;
use serde::Serialize;
use serde_json::{json, Value as Json};

use super::{query_terms, value_json, Manifest, Search, SliceQuery, Tile, VectorSearch};
use crate::db::{Db, WriteOp};
use crate::error::{Error, Result};
use crate::query::exec::{ExecOptions, ResultSet};
use crate::query::sql::Params;
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
    /// Whether the cloud's table changed since the slice was built
    /// ([`Device::set_stale`]).
    stale: bool,
    /// A vector slice's codes of every row, when fetched
    /// ([`Device::add_codes`]), and where each tile's start.
    codes: Option<(Vec<u8>, HashMap<Tile, std::ops::Range<usize>>)>,
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

/// Why a search was answered where it was ([`Answer::why`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// The k-th local score reaches the bound (or the device holds every
    /// tile): the local top k is the cloud's.
    Proved,
    /// No place in the table has a word of the query, wherever it is: the
    /// answer is empty in the cloud too.
    NoMatch,
    /// The device holds fewer than k places with a word of the query.
    Short,
    /// The k-th local score is below the most a place the device lacks
    /// could score.
    Bound,
    /// The cloud's table changed since the slice was built.
    Stale,
}

impl Why {
    pub fn as_str(&self) -> &'static str {
        match self {
            Why::Proved => "proved",
            Why::NoMatch => "no_match",
            Why::Short => "short",
            Why::Bound => "bound",
            Why::Stale => "stale",
        }
    }

    /// All of them, in the order a report lists them.
    pub const ALL: [Why; 5] = [Why::Proved, Why::NoMatch, Why::Short, Why::Bound, Why::Stale];
}

/// A search answered on the device ([`Device::search`]).
#[derive(Debug)]
pub struct Answer {
    /// The local top k, every column, with the scores they were ranked by.
    pub rows: ResultSet,
    /// The most a row the device does not hold could score (minus
    /// infinity when it holds every row).
    pub bound: f64,
    /// A geo search's sum of its words' best text scores (the bound before
    /// the fade); a vector search's most any row could score, held or not.
    pub text_bound: f64,
    /// A geo search's distance from the user to the nearest tile with rows
    /// the device lacks (infinite when it holds them all, and for a vector
    /// search).
    pub edge_m: f64,
    /// Whether `rows` is the top k of the slice's table: k rows whose last
    /// scores at least `bound`, or no place anywhere has a word of the
    /// query.
    pub proved: bool,
    /// Whether the cloud's table changed since the slice was built, so that
    /// a proof holds for the slice's version of it only.
    pub stale: bool,
    /// Whether to use `rows`: proved, and the slice not stale.
    pub on_device: bool,
    pub why: Why,
    /// A vector search not proved: the clusters the device lacks whose
    /// rows could beat its k-th, highest bound first, with their files'
    /// sizes. Once it holds them all its answer is proved (their rows
    /// raise the k-th, if anything, and every other cluster's bound is
    /// below it). Empty when proved, for a geo search, and when the device
    /// holds fewer than k rows (every cluster could then add one).
    pub need: Vec<(Tile, u64)>,
}

impl Answer {
    /// The answer as JSON, for an app in another language: the rows as the
    /// HTTP API answers a query (`columns`, `rows`, `keys`, `scores`), what
    /// the device found (`on_device`, `why`, `proved`, `stale`, `bound`,
    /// `text_bound`, `edge_m`, `kth`, `need` as `[x, y, bytes]`), and
    /// `ask`, the body of the request that asks the cloud
    /// ([`Search::ask_body`]).
    pub fn to_json(&self, s: &dyn SliceQuery, m: &Manifest) -> Json {
        let finite = |x: f64| if x.is_finite() { json!(x) } else { Json::Null };
        let rs = &self.rows;
        json!({
            "columns": rs.columns,
            "rows": rs.rows.iter().map(|r| r.iter().map(value_json).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "keys": rs.keys.iter().map(value_json).collect::<Vec<_>>(),
            "scores": rs.scores.iter().map(|&x| finite(x)).collect::<Vec<_>>(),
            "on_device": self.on_device, "why": self.why, "proved": self.proved, "stale": self.stale,
            "bound": finite(self.bound), "text_bound": finite(self.text_bound), "edge_m": finite(self.edge_m),
            "kth": rs.scores.get(s.k() - 1), "matches": rs.scores.len(),
            "need": self.need.iter().map(|(t, b)| json!([t.x, t.y, b])).collect::<Vec<_>>(),
            "ask": s.ask_body(m),
        })
    }
}

/// A search answered on the device or by the cloud
/// ([`Device::search_or_ask`]).
#[derive(Debug)]
pub struct Outcome {
    /// The answer: the device's when `answer.on_device`, else the cloud's.
    pub rows: ResultSet,
    /// What the device found.
    pub answer: Answer,
}

impl Device {
    /// A device with no tiles yet, its database in `dir/<table>-<version>`.
    /// Earlier databases of the table there (of this version or another)
    /// are a cache of what the device fetched, and are deleted.
    pub fn create(dir: &Path, m: Manifest) -> Result<Device> {
        std::fs::create_dir_all(dir)?;
        let prefix = format!("{}-", m.schema.table);
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.strip_prefix(&prefix).is_some_and(|v| v.parse::<u64>().is_ok()) && e.file_type()?.is_dir() {
                std::fs::remove_dir_all(e.path())?;
            }
        }
        let own = dir.join(format!("{prefix}{}", m.version));
        std::fs::create_dir_all(&own)?;
        let db = Db::create(&own.join("slice.strata"), m.schema.clone(), DbOptions::default())?;
        let words = m.word_map().into_iter().map(|(w, v)| (w.to_string(), v)).collect();
        let tiles = m.tiles.iter().map(|t| (t.tile(), t.bytes)).collect();
        Ok(Device { db, m, words, tiles, held: BTreeMap::new(), stale: false, codes: None })
    }

    /// Adds a vector slice's codes of every row from the bytes of its
    /// `codes.bin.gz`: a search then bounds each cluster it lacks by its
    /// rows' codes as well, which is far tighter than by its centre and
    /// radius, and names the clusters that could still beat its k-th
    /// ([`Answer::need`]).
    pub fn add_codes(&mut self, gz: &[u8]) -> Result<()> {
        let v = self.m.vectors()?;
        if v.codes.is_none() {
            return Err(Error::Query(format!("the slice of {} was built without codes", self.m.schema.table)));
        }
        let mut bytes = Vec::new();
        GzDecoder::new(gz).read_to_end(&mut bytes).map_err(|e| Error::Query(format!("{}: {e}", super::CODES_FILE)))?;
        let mut at = HashMap::new();
        let mut start = 0usize;
        for e in &self.m.tiles {
            let end = start + e.rows as usize * v.dim;
            at.insert(e.tile(), start..end);
            start = end;
        }
        if start != bytes.len() {
            return Err(Error::Query(format!("{}: {} bytes for {} rows of {} dimensions", super::CODES_FILE, bytes.len(), start / v.dim.max(1), v.dim)));
        }
        self.codes = Some((bytes, at));
        Ok(())
    }

    /// Whether [`Device::add_codes`] added the codes.
    pub fn has_codes(&self) -> bool {
        self.codes.is_some()
    }

    /// Notes whether the cloud's table changed since the slice was built,
    /// as the server's `GET /v1/slices/<table>/status` says. A stale
    /// device's proofs hold for the slice's version of the table only, so
    /// [`Device::search_or_ask`] asks the cloud instead.
    pub fn set_stale(&mut self, stale: bool) {
        self.stale = stale;
    }

    pub fn is_stale(&self) -> bool {
        self.stale
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
    /// edge does not fetch and drop the same tiles over and over). Nothing,
    /// for a vector slice ([`Device::plan_vector`]).
    pub fn plan(&self, lat: f64, lon: f64, reach: i32) -> Plan {
        let Ok(geo) = self.m.geo() else { return Plan::default() };
        let g = &geo.grid;
        let u = g.tile(lat, lon);
        let (x, y) = g.xy(lat, lon);
        let ring = |t: Tile| (t.x - u.x).abs().max((t.y - u.y).abs());
        let mut fetch: Vec<(Tile, u64)> = self.tiles.iter().filter(|(t, _)| ring(**t) <= reach && !self.holds(**t)).map(|(t, b)| (*t, *b)).collect();
        fetch.sort_by(|a, b| g.distance(x, y, a.0).total_cmp(&g.distance(x, y, b.0)).then(a.0.cmp(&b.0)));
        let drop = self.held.keys().filter(|t| ring(**t) > reach + 1).copied().collect();
        Plan { fetch, drop }
    }

    /// A vector slice's clusters to fetch so that the device holds those
    /// whose rows could score highest for `taste` (a user's history as a
    /// vector, say), best first, as many as fit in `rows` rows (at least
    /// one), and those it can let go: ranked beyond what fits in half as
    /// many rows again, so that a taste that drifts does not fetch and drop
    /// the same clusters over and over.
    pub fn plan_vector(&self, taste: &[f32], rows: u64) -> Result<Plan> {
        let v = self.m.vectors()?;
        if taste.len() != v.dim || taste.iter().any(|x| !x.is_finite()) {
            return Err(Error::Query(format!("a taste of {} numbers for a field of {}", taste.len(), v.dim)));
        }
        let qn = taste.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt();
        let size: HashMap<Tile, u64> = self.m.tiles.iter().map(|e| (e.tile(), e.rows)).collect();
        let mut ranked: Vec<(f64, Tile)> = self.tiles.keys().map(|t| (v.bound(t.x as usize, taste, qn), *t)).collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let fits = |budget: u64| {
            let mut sum = 0;
            ranked.iter().take_while(|(_, t)| {
                sum += size.get(t).copied().unwrap_or(0);
                sum <= budget
            }).count().max(1)
        };
        let n = fits(rows);
        let keep = fits(rows.saturating_add(rows / 2)).max(n);
        let fetch = ranked.iter().take(n).filter(|(_, t)| !self.holds(*t)).map(|(_, t)| (*t, self.tiles[t])).collect();
        let drop = ranked.iter().skip(keep).filter(|(_, t)| self.holds(*t)).map(|(_, t)| *t).collect();
        Ok(Plan { fetch, drop })
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
        let Ok(geo) = self.m.geo() else { return f64::INFINITY };
        let g = &geo.grid;
        let (x, y) = g.xy(lat, lon);
        self.tiles.keys().filter(|t| !self.holds(**t)).map(|t| g.distance(x, y, *t)).fold(f64::INFINITY, f64::min)
    }

    /// Runs `s` on the tiles held, scored with the cloud's statistics, and
    /// says whether its answer is the cloud's ([`Answer::proved`]). Rows
    /// tied with the k-th may be other rows of the same score than the
    /// cloud's, which breaks ties by its own row order.
    pub fn search(&self, s: &Search) -> Result<Answer> {
        let m = &self.m;
        let geo = m.geo()?;
        let terms = query_terms(&m.schema, &geo.text, &s.text)?;
        let mut df = BTreeMap::new();
        let mut text_bound = 0f64;
        for (t, w) in &terms {
            let (d, b) = self.words.get(t).copied().unwrap_or((0, 0.0));
            df.insert(t.clone(), d);
            text_bound += *w as f64 * b as f64;
        }
        let stats = TermStats { fields: [(geo.text.clone(), FieldStats { n_docs: geo.n_docs, total_len: geo.total_len, df })].into() };
        let opts = ExecOptions { stats: Some(Arc::new(stats)), ..self.db.exec.clone() };
        let rows = self.db.query_with(&s.sql(m), &s.params(m), &opts)?;
        let edge_m = self.edge(s.lat, s.lon);
        let fade = if edge_m.is_finite() { 1.0 / (1.0 + edge_m * edge_m / (s.d0_m * s.d0_m)) } else { 0.0 };
        let bound = text_bound * fade;
        let proved = bound == 0.0 || rows.scores.get(s.k - 1).is_some_and(|&kth| kth >= bound);
        let why = if self.stale {
            Why::Stale
        } else if text_bound == 0.0 {
            Why::NoMatch
        } else if proved {
            Why::Proved
        } else if rows.scores.len() < s.k {
            Why::Short
        } else {
            Why::Bound
        };
        Ok(Answer { rows, bound, text_bound, edge_m, proved, stale: self.stale, on_device: proved && !self.stale, why, need: Vec::new() })
    }

    /// Runs a vector search on the clusters held, and says whether its
    /// answer is the cloud's: the k-th local score reaches the most a row
    /// of a cluster the device lacks could score, by the cluster's centre
    /// and extent ([`super::Vectors::bound`]) and, once the device has them
    /// ([`Device::add_codes`]), by its rows' codes. When it does not, the
    /// answer names the clusters to fetch to prove it ([`Answer::need`]).
    pub fn search_vector(&self, s: &VectorSearch) -> Result<Answer> {
        let v = self.m.vectors()?;
        if s.vector.len() != v.dim {
            return Err(Error::Query(format!("a query vector of {} numbers for a field of {}", s.vector.len(), v.dim)));
        }
        let rows = self.db.query(&s.sql(&self.m), &s.params(&self.m))?;
        let qn = s.norm();
        let kth = rows.scores.get(s.k - 1).copied();
        let codes = self.codes.as_ref().zip(v.codes.as_ref()).map(|((bytes, at), c)| (bytes, at, c.query(&s.vector)));
        let (mut bound, mut best) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut need = Vec::new();
        for (t, &file) in &self.tiles {
            let c = t.x as usize;
            let mut b = v.bound(c, &s.vector, qn);
            best = best.max(b);
            if self.holds(*t) {
                continue;
            }
            // The rows' codes, only where the centre's bound leaves the
            // k-th in doubt.
            if let (Some((bytes, at, cq)), Some(kth)) = (&codes, kth) {
                if b >= kth {
                    b = b.min(cq.best(&bytes[at[t].clone()]) + v.margin(c, qn));
                }
            }
            if kth.is_some_and(|kth| b > kth) {
                need.push((b, *t, file));
            }
            bound = bound.max(b);
        }
        let proved = bound == f64::NEG_INFINITY || kth.is_some_and(|kth| kth >= bound);
        let why = if self.stale {
            Why::Stale
        } else if proved {
            Why::Proved
        } else if rows.scores.len() < s.k {
            Why::Short
        } else {
            Why::Bound
        };
        need.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let need = if proved { Vec::new() } else { need.into_iter().map(|(_, t, f)| (t, f)).collect() };
        Ok(Answer { rows, bound, text_bound: best, edge_m: f64::INFINITY, proved, stale: self.stale, on_device: proved && !self.stale, why, need })
    }

    /// Runs `s` on the device ([`Device::search`] or
    /// [`Device::search_vector`]) and, when its answer is not one to use,
    /// asks the cloud with `ask(sql, params)` (which sends them to `POST
    /// /v1/query`).
    pub fn search_or_ask<Q: SliceQuery + ?Sized>(&self, s: &Q, ask: impl FnOnce(&str, &Params) -> Result<ResultSet>) -> Result<Outcome> {
        let answer = s.answer(self)?;
        if answer.on_device {
            return Ok(Outcome { rows: answer.rows.clone(), answer });
        }
        let rows = ask(&s.sql(&self.m), &s.params(&self.m))?;
        Ok(Outcome { rows, answer })
    }

    /// The local database (to read what it holds).
    pub fn db(&self) -> &Db {
        &self.db
    }
}
