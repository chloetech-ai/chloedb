//! Slices: the rows of a table around a device's user, shipped to the
//! device, with what it needs to answer a search near the user exactly as
//! the cloud would, or to know that it cannot and must ask the cloud.
//!
//! The cloud ranks the places of a table by text and distance,
//!
//! ```text
//! score(p) = bm25(text, q) / (1 + d(u, p)^2 / d0^2)
//! ```
//!
//! `d` the distance from the user `u` and `d0` the distance at which a
//! place counts half ([`Search::sql`] writes it, with BM25's parameters
//! spelled out, so that the device and the cloud run the same query). The
//! device holds the places of the 1 km tiles around its user and ranks them
//! the same way, with the cloud's statistics ([`ExecOptions::stats`], as a
//! shard scores with its coordinator's), so that every place it holds
//! scores exactly as in the cloud. A place it does not hold is at least `e`
//! away, `e` the distance from the user to the nearest tile it lacks, and
//! scores at most
//!
//! ```text
//! bound = (sum over the query's words of their best BM25 score) / (1 + e^2 / d0^2)
//! ```
//!
//! So when the k-th local score is at least the bound, the local top k is
//! the cloud's ([`Answer::proved`]); otherwise the device sends
//! [`Search::sql`] to the cloud. Each word's best score is its posting
//! list's bound summed over the table's segments
//! ([`crate::text::TextFieldReader::term_bound`]), which the build reads
//! instead of running a search per word.
//!
//! A slice is static files that any web server or CDN can serve
//! ([`build`]): `manifest.json` ([`Manifest`]: the schema, the grid, the
//! text field's statistics, each word's document frequency and best
//! score, the tiles and their sizes) and `tiles/<x>_<y>.jsonl.gz`, the rows
//! of each tile that has any, one JSON array of field values per line in
//! the schema's order. A [`Device`] holds a local database of the tiles
//! it fetched.
//!
//! The table needs a primary key (the device drops a tile's rows by key),
//! a TEXT field, and FLOAT latitude and longitude fields; its other fields
//! may be KEYWORD, INT or FLOAT. Distances are on a flat projection of the
//! city around the grid's origin ([`Grid`]), which the query's SQL spells
//! out, so the cloud and the device agree on them to the last bit.

mod build;
mod device;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::error::{Error, Result};
use crate::query::sql::Params;
use crate::schema::{FieldType, Schema};
use crate::value::Value;

pub use build::{build, BuildOptions, Built};
pub use device::{Answer, Device, Outcome, Plan, Why};

/// The manifest's format, which a device checks before reading the rest.
pub const FORMAT: u32 = 1;

/// Metres per degree of latitude (the mean; the projection is local).
pub const KY: f64 = 110_540.0;

/// Metres per degree of longitude at the equator.
const KX_EQUATOR: f64 = 111_320.0;

/// Tiles of `tile_m` metres on a flat projection around (`lat0`, `lon0`):
/// a point's `x` is `(lon - lon0) * kx` and `y` is `(lat - lat0) * KY`, with
/// `kx` the metres of a degree of longitude at `lat0`. Tile (i, j) covers
/// `i * tile_m <= x < (i + 1) * tile_m` and the same for `y`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    pub lat0: f64,
    pub lon0: f64,
    pub tile_m: f64,
}

/// A tile of a [`Grid`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

impl Tile {
    /// Its file in a slice's directory.
    pub fn path(&self) -> String {
        format!("tiles/{}_{}.jsonl.gz", self.x, self.y)
    }
}

impl Grid {
    pub fn new(lat0: f64, lon0: f64, tile_m: f64) -> Result<Grid> {
        if !(lat0.abs() < 85.0 && lon0.abs() <= 180.0 && tile_m.is_finite() && tile_m > 0.0) {
            return Err(Error::Query(format!("a slice's grid needs a latitude within 85 degrees, a longitude and a tile side above 0, not ({lat0}, {lon0}, {tile_m})")));
        }
        Ok(Grid { lat0, lon0, tile_m })
    }

    /// Metres per degree of longitude at the grid's origin.
    pub fn kx(&self) -> f64 {
        KX_EQUATOR * self.lat0.to_radians().cos()
    }

    /// A point's place on the projection, in metres from the origin.
    pub fn xy(&self, lat: f64, lon: f64) -> (f64, f64) {
        ((lon - self.lon0) * self.kx(), (lat - self.lat0) * KY)
    }

    pub fn tile(&self, lat: f64, lon: f64) -> Tile {
        let (x, y) = self.xy(lat, lon);
        Tile { x: (x / self.tile_m).floor() as i32, y: (y / self.tile_m).floor() as i32 }
    }

    /// The distance from point (`x`, `y`) to the nearest point of tile `t`
    /// (0 inside it).
    pub fn distance(&self, x: f64, y: f64, t: Tile) -> f64 {
        let s = self.tile_m;
        let (x0, y0) = (t.x as f64 * s, t.y as f64 * s);
        let dx = (x0 - x).max(0.0).max(x - (x0 + s));
        let dy = (y0 - y).max(0.0).max(y - (y0 + s));
        dx.hypot(dy)
    }
}

/// The words of a slice's text field: each word's document frequency over
/// the table and the most it adds to a BM25 score, in columns sorted by
/// word.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Words {
    pub terms: Vec<String>,
    pub df: Vec<u64>,
    pub best: Vec<f32>,
}

/// A tile with rows, and its file's size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileEntry {
    pub x: i32,
    pub y: i32,
    pub rows: u64,
    pub bytes: u64,
}

impl TileEntry {
    pub fn tile(&self) -> Tile {
        Tile { x: self.x, y: self.y }
    }
}

/// What a device needs besides the tiles ([`build`] writes it as
/// `manifest.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// The version of the table it was built from (its last commit), which
    /// its tiles carry too.
    pub version: u64,
    /// When it was built, in milliseconds since 1970.
    #[serde(default)]
    pub built_ms: u64,
    pub schema: Schema,
    /// The TEXT field searched, and the FLOAT fields of latitude and
    /// longitude.
    pub text: String,
    pub lat: String,
    pub lon: String,
    pub grid: Grid,
    /// BM25's parameters, which [`Search::sql`] passes to the cloud.
    pub k1: f32,
    pub b: f32,
    /// The text field's statistics over the table: documents with a term
    /// in it, and their total length in terms.
    pub n_docs: u64,
    pub total_len: u64,
    pub words: Words,
    pub tiles: Vec<TileEntry>,
}

impl Manifest {
    pub fn from_json(text: &str) -> Result<Manifest> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| Error::Query(format!("not a slice manifest: {e}")))?;
        if m.format != FORMAT {
            return Err(Error::Query(format!("slice manifest format {}, but this build of Strata reads format {FORMAT}", m.format)));
        }
        let w = &m.words;
        if w.df.len() != w.terms.len() || w.best.len() != w.terms.len() {
            return Err(Error::Query("a slice manifest's words, document frequencies and best scores differ in number".into()));
        }
        check_schema(&m.schema, &m.text, &m.lat, &m.lon)?;
        Ok(m)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a manifest serialises")
    }

    /// The words as a map from word to (document frequency, best score).
    pub fn word_map(&self) -> HashMap<&str, (u64, f32)> {
        let w = &self.words;
        w.terms.iter().zip(w.df.iter().zip(&w.best)).map(|(t, (&d, &b))| (t.as_str(), (d, b))).collect()
    }
}

/// A search near the user: the `k` best places for `text` from (`lat`,
/// `lon`), with the distance fade `d0_m`.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    pub text: String,
    pub lat: f64,
    pub lon: f64,
    pub k: usize,
    pub d0_m: f64,
}

impl Search {
    pub fn new(text: &str, lat: f64, lon: f64, k: usize, d0_m: f64) -> Result<Search> {
        if k == 0 || k > 1000 {
            return Err(Error::Query(format!("a slice search asks for 1 to 1000 places, not {k}")));
        }
        if !(d0_m.is_finite() && d0_m > 0.0) || !lat.is_finite() || !lon.is_finite() {
            return Err(Error::Query("a slice search needs a position and a distance fade above 0".into()));
        }
        Ok(Search { text: text.to_string(), lat, lon, k, d0_m })
    }

    /// The query the cloud and the device both run: every column of the
    /// best `k` rows by text score faded by distance.
    pub fn sql(&self, m: &Manifest) -> String {
        let dist2 = format!(
            "(({lon} - :lon) * :kx) * (({lon} - :lon) * :kx) + (({lat} - :lat) * :ky) * (({lat} - :lat) * :ky)",
            lat = m.lat,
            lon = m.lon
        );
        format!(
            "SELECT * FROM {table} ORDER BY bm25({text}, :q, syntax => 'plain', k1 => {k1:?}, b => {b:?}) / (1 + ({dist2}) / :d0sq) DESC LIMIT {k}",
            table = m.schema.table,
            text = m.text,
            k1 = m.k1,
            b = m.b,
            k = self.k
        )
    }

    /// The body of the request that asks the cloud this search (`POST
    /// /v1/query`): its SQL and parameters as JSON text, which an app sends
    /// as it is (a float written by a JavaScript or Python `JSON` stays a
    /// float here, so the cloud scores with the same numbers).
    pub fn ask_body(&self, m: &Manifest) -> String {
        let params: serde_json::Map<String, Json> = self.params(m).0.iter().map(|(k, v)| (k.clone(), value_json(v))).collect();
        json!({"sql": self.sql(m), "params": params}).to_string()
    }

    /// The values of [`Search::sql`]'s placeholders.
    pub fn params(&self, m: &Manifest) -> Params {
        let mut p = Params::new();
        p.0.insert("q".into(), Value::Str(self.text.clone()));
        p.0.insert("lat".into(), Value::Float(self.lat));
        p.0.insert("lon".into(), Value::Float(self.lon));
        p.0.insert("kx".into(), Value::Float(m.grid.kx()));
        p.0.insert("ky".into(), Value::Float(KY));
        p.0.insert("d0sq".into(), Value::Float(self.d0_m * self.d0_m));
        p
    }
}

/// The analysed words of a search with their multiplicities, as
/// `bm25(.., syntax => 'plain')` scores them (each word once per time it
/// occurs, stopwords dropped as the field's analyzer drops them).
pub fn query_terms(schema: &Schema, text_field: &str, q: &str) -> Result<Vec<(String, f32)>> {
    let f = schema.field(text_field).ok_or_else(|| Error::Query(format!("no field {text_field}")))?;
    let a = f.text_analyzer();
    let mut out: Vec<(String, f32)> = Vec::new();
    for t in a.query_term_seq(q, a.drops_query_stopwords()) {
        match out.iter_mut().find(|(u, _)| *u == t) {
            Some(e) => e.1 += 1.0,
            None => out.push((t, 1.0)),
        }
    }
    Ok(out)
}

/// A value as JSON, as a tile's line and the HTTP API carry it.
pub fn value_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Int(i) | Value::Timestamp(i) => Json::from(*i),
        Value::Float(x) => serde_json::Number::from_f64(*x).map_or(Json::Null, Json::Number),
        Value::Str(s) => Json::from(s.as_str()),
        Value::Bool(b) => Json::from(*b),
        Value::List(items) => Json::Array(items.iter().map(value_json).collect()),
        Value::Vector(xs) => Json::Array(xs.iter().map(|&x| serde_json::Number::from_f64(x as f64).map_or(Json::Null, Json::Number)).collect()),
    }
}

/// The fields a slice reads, checked: a primary key, TEXT, FLOAT latitude
/// and longitude, and nothing a row's JSON could not carry.
fn check_schema(s: &Schema, text: &str, lat: &str, lon: &str) -> Result<()> {
    let bad = |m: String| Err(Error::Query(format!("table {}: {m}", s.table)));
    if s.primary_key.is_none() {
        return bad("a slice needs a primary key, by which a device drops a tile's rows".into());
    }
    match s.field(text) {
        Some(f) if matches!(f.ty, FieldType::Text { .. }) && f.indexed => {}
        _ => return bad(format!("{text} is not an indexed TEXT field")),
    }
    for c in [lat, lon] {
        match s.field(c) {
            Some(f) if f.ty == FieldType::Float && !f.multi => {}
            _ => return bad(format!("{c} is not a single-valued FLOAT field")),
        }
    }
    for f in &s.fields {
        let plain = matches!(f.ty, FieldType::Text { .. } | FieldType::Keyword | FieldType::Int | FieldType::Float);
        if !plain || !f.generated_from.is_empty() || f.derive.is_some() {
            return bad(format!("field {} is not one a slice carries (TEXT, KEYWORD, INT or FLOAT, neither generated nor derived)", f.name));
        }
        if matches!(f.ty, FieldType::Text { .. }) && !f.stored {
            return bad(format!("TEXT field {} is not stored, so a tile could not carry it", f.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
