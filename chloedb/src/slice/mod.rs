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
//! may be KEYWORD, INT, FLOAT or VECTOR. Distances are on a flat projection
//! of the city around the grid's origin ([`Grid`]), which the query's SQL
//! spells out, so the cloud and the device agree on them to the last bit.
//!
//! # Vector slices
//!
//! A slice of a VECTOR field cuts the table by meaning instead of by place
//! ([`Key::Vector`]): k-means clusters of the field's vectors, each a part
//! of the slice with its rows (a [`Tile`] whose `x` is the cluster's number
//! and `y` 0), and in the manifest each cluster's centre `c` and radius
//! `r` (the farthest of its rows from `c`). The cloud ranks every row by
//! `similarity(field, q)`, the dot product (cosine for unit vectors), over
//! all rows ([`VectorSearch::sql`] asks for an exact scan). A row `x` of a
//! cluster the device does not hold scores at most
//!
//! ```text
//! q . x = q . c + q . (x - c) <= q . c + |q| r
//! ```
//!
//! (plus a margin for float32 rounding), so when the k-th local score
//! reaches the largest of these over the clusters the device lacks, the
//! local top k is the cloud's. A device holds the clusters nearest its
//! user's taste ([`Device::plan_vector`]): the history its recommendations
//! and searches start from.
//!
//! In many dimensions those bounds are loose: a cluster's rows lie in all
//! directions from its centre, so `q . (x - c)` is far below `|q| r` for
//! every one of them. The slice also holds every row's vector as one byte a
//! dimension ([`Codes`], `codes.bin.gz`, a 16th of the float32 rows for 64
//! dimensions): a device that fetched them bounds each row it lacks by its
//! code, within half a step a dimension, and can name the few clusters
//! whose rows could still beat its k-th ([`Answer::need`]); once it holds
//! those, its answer is proved.

mod build;
mod device;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::error::{Error, Result};
use crate::query::sql::Params;
use crate::schema::{FieldType, Schema};
use crate::value::Value;

pub use build::{build, BuildKey, BuildOptions, Built};
pub use device::{Answer, Device, Outcome, Plan, Why};

/// The manifest's format, which a device checks before reading the rest
/// (2: a manifest says its [`Key`]).
pub const FORMAT: u32 = 2;

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
    /// How the table is cut into parts, and what bounds the rows of a part
    /// a device does not hold (`"kind": "geo"` or `"vector"` in the JSON).
    #[serde(flatten)]
    pub key: Key,
    pub tiles: Vec<TileEntry>,
}

/// How a slice cuts its table ([`Manifest::key`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Key {
    /// Map tiles, searched by text faded by distance ([`Search`]).
    Geo(Geo),
    /// Clusters of a VECTOR field, searched by similarity
    /// ([`VectorSearch`]).
    Vector(Vectors),
}

/// A geo slice's fields, grid and text statistics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Geo {
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
}

/// A vector slice's field and clusters: cluster `i` is tile (`i`, 0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vectors {
    /// The VECTOR field searched.
    pub field: String,
    pub dim: usize,
    /// Each cluster's centre.
    pub centroids: Vec<Vec<f32>>,
    /// Each cluster's radius: the largest distance from its centre to one
    /// of its rows.
    pub radii: Vec<f64>,
    /// Each cluster's angular radius: the largest angle, in radians,
    /// between its centre's direction and one of its rows.
    pub angles: Vec<f64>,
    /// The longest and the shortest of each cluster's vectors.
    pub max_norms: Vec<f64>,
    pub min_norms: Vec<f64>,
    /// The table's rows, which [`VectorSearch::sql`] names so that the
    /// cloud re-scores every one (its search is then exact).
    pub rows: u64,
    /// Every row's codes, in `codes.bin.gz` (None when built without).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codes: Option<Codes>,
}

/// A vector slice's codes: each row's vector as one byte a dimension,
/// `x_i` within `step_i / 2` of `min_i + code_i * step_i`. `codes.bin.gz`
/// holds them tile after tile in the manifest's order, each tile's rows in
/// the order of its file, `dim` bytes a row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Codes {
    pub min: Vec<f32>,
    pub step: Vec<f32>,
    /// The size of `codes.bin.gz`.
    pub bytes: u64,
}

/// The file of a vector slice's codes ([`Codes`]).
pub const CODES_FILE: &str = "codes.bin.gz";

/// A query's weights for scoring codes ([`Codes::query`]).
pub(crate) struct CodeQuery {
    /// `q_i * step_i`, for [`crate::vector::kernels::dot_u8_f32`].
    w: Vec<f32>,
    /// `q . min`.
    base: f64,
    /// What the code's score may fall short of the row's, as the cloud
    /// computes it, less the row's own float32 rounding (added per cluster).
    slack: f64,
}

impl Codes {
    pub(crate) fn query(&self, q: &[f32]) -> CodeQuery {
        let w: Vec<f32> = q.iter().zip(&self.step).map(|(&q, &s)| q * s).collect();
        let base: f64 = q.iter().zip(&self.min).map(|(&q, &m)| q as f64 * m as f64).sum();
        let n = q.len() as f64;
        // Half a step a dimension (a millionth more, for the step's own
        // rounding), and the float32 sum of up to 255 * |w_i| a term.
        let half: f64 = q.iter().zip(&self.step).map(|(&q, &s)| (q as f64).abs() * s as f64).sum::<f64>() * (0.5 + 1e-6);
        let sum: f64 = w.iter().map(|&w| (w as f64).abs() * 255.0).sum::<f64>() * 2.0 * n * f64::powi(2.0, -24);
        let base_err = q.iter().zip(&self.min).map(|(&q, &m)| (q as f64 * m as f64).abs()).sum::<f64>() * 1e-12;
        CodeQuery { w, base, slack: half + sum + base_err }
    }
}

impl CodeQuery {
    /// The most any of `rows` (`dim` bytes each) could score, before the
    /// cluster's own rounding margin.
    pub(crate) fn best(&self, rows: &[u8]) -> f64 {
        let dim = self.w.len();
        let mut best = f32::NEG_INFINITY;
        for code in rows.chunks_exact(dim) {
            best = best.max(crate::vector::kernels::dot_u8_f32(code, &self.w));
        }
        self.base + best as f64 + self.slack
    }
}

impl Vectors {
    /// The most a row `x` of cluster `c` (centre `m`) can score for query
    /// `q` (with `qn = |q|`), the lesser of two bounds:
    ///
    /// - the ball: `q . x = q . m + q . (x - m) <= q . m + |q| r`;
    /// - the cone: the angle between `q` and `x` is at least the angle
    ///   between `q` and `m` less the cluster's angular radius, so
    ///   `q . x <= |q| |x| cos(max(0, angle(q, m) - a))`, with `|x|` the
    ///   cluster's longest vector (its shortest, when the cosine is
    ///   negative). For unit vectors (cosine similarity) this is the
    ///   tighter one by far.
    ///
    /// Plus [`Vectors::margin`].
    pub fn bound(&self, c: usize, q: &[f32], qn: f64) -> f64 {
        let cent = &self.centroids[c];
        let dot: f64 = cent.iter().zip(q).map(|(&a, &b)| a as f64 * b as f64).sum();
        let cn: f64 = cent.iter().map(|&a| a as f64 * a as f64).sum::<f64>().sqrt();
        let (r, a, hi, lo) = (self.radii[c], self.angles[c], self.max_norms[c], self.min_norms[c]);
        let ball = dot + qn * r;
        let cone = if cn > 0.0 && qn > 0.0 {
            let theta = (dot / (qn * cn)).clamp(-1.0, 1.0).acos();
            // A microradian of slack for the arc cosines' rounding.
            let cos = (theta - a - 1e-6).max(0.0).cos();
            qn * if cos >= 0.0 { hi } else { lo } * cos
        } else {
            f64::INFINITY
        };
        // A float32 dot product of n terms is within n * 2^-24 of the sum
        // of |q_i x_i| <= |q| |x|; twice that, for the centre and the radii
        // rounded on their way here.
        ball.min(cone) + self.margin(c, qn).max(self.dim as f64 * f64::powi(2.0, -23) * qn * (cn + r))
    }

    /// A margin for the float32 rounding of the dot products the cloud
    /// computes for cluster `c`'s rows: within `dim * 2^-24 * |q| |x|` of
    /// the exact sum, twice that for the norms rounded on their way here.
    pub fn margin(&self, c: usize, qn: f64) -> f64 {
        self.dim as f64 * f64::powi(2.0, -23) * qn * self.max_norms[c]
    }
}

impl Manifest {
    pub fn from_json(text: &str) -> Result<Manifest> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| Error::Query(format!("not a slice manifest: {e}")))?;
        if m.format != FORMAT {
            return Err(Error::Query(format!("slice manifest format {}, but this build of ChloeDB reads format {FORMAT}", m.format)));
        }
        match &m.key {
            Key::Geo(g) => {
                let w = &g.words;
                if w.df.len() != w.terms.len() || w.best.len() != w.terms.len() {
                    return Err(Error::Query("a slice manifest's words, document frequencies and best scores differ in number".into()));
                }
                check_schema(&m.schema, &BuildKey::geo_fields(&g.text, &g.lat, &g.lon))?;
            }
            Key::Vector(v) => {
                let n = v.centroids.len();
                if [v.radii.len(), v.angles.len(), v.max_norms.len(), v.min_norms.len()].iter().any(|&l| l != n) || v.centroids.iter().any(|c| c.len() != v.dim) {
                    return Err(Error::Query("a slice manifest's clusters differ in number from their radii, or in dimension from the field".into()));
                }
                if m.tiles.iter().any(|t| t.y != 0 || t.x < 0 || t.x as usize >= v.centroids.len()) {
                    return Err(Error::Query("a vector slice's tile names a cluster it does not have".into()));
                }
                if let Some(c) = &v.codes {
                    if c.min.len() != v.dim || c.step.len() != v.dim || c.min.iter().chain(&c.step).any(|x| !x.is_finite()) || c.step.iter().any(|&s| s < 0.0) {
                        return Err(Error::Query("a vector slice's codes differ in dimension from the field, or are not finite".into()));
                    }
                }
                check_schema(&m.schema, &BuildKey::vector_fields(&v.field))?;
            }
        }
        Ok(m)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a manifest serialises")
    }

    /// "geo" or "vector".
    pub fn kind(&self) -> &'static str {
        match self.key {
            Key::Geo(_) => "geo",
            Key::Vector(_) => "vector",
        }
    }

    /// A geo slice's part, or an error that names what the slice is.
    pub fn geo(&self) -> Result<&Geo> {
        match &self.key {
            Key::Geo(g) => Ok(g),
            Key::Vector(v) => Err(Error::Query(format!("the slice of {} is of vector field {}: search it with a vector", self.schema.table, v.field))),
        }
    }

    /// A vector slice's part, or an error that names what the slice is.
    pub fn vectors(&self) -> Result<&Vectors> {
        match &self.key {
            Key::Vector(v) => Ok(v),
            Key::Geo(g) => Err(Error::Query(format!("the slice of {} is of places (text {}): search it with text and a position", self.schema.table, g.text))),
        }
    }

    /// The words as a map from word to (document frequency, best score);
    /// empty for a vector slice.
    pub fn word_map(&self) -> HashMap<&str, (u64, f32)> {
        match &self.key {
            Key::Geo(g) => {
                let w = &g.words;
                w.terms.iter().zip(w.df.iter().zip(&w.best)).map(|(t, (&d, &b))| (t.as_str(), (d, b))).collect()
            }
            Key::Vector(_) => HashMap::new(),
        }
    }
}

/// A query a device answers or asks the cloud: the SQL both run, its
/// parameters, and how many rows it ranks.
pub trait SliceQuery {
    fn k(&self) -> usize;
    /// The query's SQL. `m` is the slice's manifest, of the query's kind
    /// (a [`Device`] checks).
    fn sql(&self, m: &Manifest) -> String;
    /// The values of the SQL's placeholders.
    fn params(&self, m: &Manifest) -> Params;
    /// The body of the request that asks the cloud this query (`POST
    /// /v1/query`): its SQL and parameters as JSON text, which an app sends
    /// as it is (a float written by a JavaScript or Python `JSON` stays a
    /// float here, so the cloud scores with the same numbers).
    fn ask_body(&self, m: &Manifest) -> String {
        let params: serde_json::Map<String, Json> = self.params(m).0.iter().map(|(k, v)| (k.clone(), value_json(v))).collect();
        json!({"sql": self.sql(m), "params": params}).to_string()
    }
    /// Runs it on a device ([`Device::search`], [`Device::search_vector`]).
    fn answer(&self, d: &Device) -> Result<Answer>;
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

}

impl SliceQuery for Search {
    fn k(&self) -> usize {
        self.k
    }

    fn answer(&self, d: &Device) -> Result<Answer> {
        d.search(self)
    }

    /// Every column of the best `k` rows by text score faded by distance.
    fn sql(&self, m: &Manifest) -> String {
        let g = m.geo().expect("a geo slice's search");
        let dist2 = format!(
            "(({lon} - :lon) * :kx) * (({lon} - :lon) * :kx) + (({lat} - :lat) * :ky) * (({lat} - :lat) * :ky)",
            lat = g.lat,
            lon = g.lon
        );
        format!(
            "SELECT * FROM {table} ORDER BY bm25({text}, :q, syntax => 'plain', k1 => {k1:?}, b => {b:?}) / (1 + ({dist2}) / :d0sq) DESC LIMIT {k}",
            table = m.schema.table,
            text = g.text,
            k1 = g.k1,
            b = g.b,
            k = self.k
        )
    }

    fn params(&self, m: &Manifest) -> Params {
        let g = m.geo().expect("a geo slice's search");
        let mut p = Params::new();
        p.0.insert("q".into(), Value::Str(self.text.clone()));
        p.0.insert("lat".into(), Value::Float(self.lat));
        p.0.insert("lon".into(), Value::Float(self.lon));
        p.0.insert("kx".into(), Value::Float(g.grid.kx()));
        p.0.insert("ky".into(), Value::Float(KY));
        p.0.insert("d0sq".into(), Value::Float(self.d0_m * self.d0_m));
        p
    }
}

/// A search of a vector slice: the `k` rows whose vectors are most similar
/// to `vector` (the largest dot product).
#[derive(Clone, Debug, PartialEq)]
pub struct VectorSearch {
    pub vector: Vec<f32>,
    pub k: usize,
}

impl VectorSearch {
    pub fn new(vector: Vec<f32>, k: usize) -> Result<VectorSearch> {
        if k == 0 || k > 1000 {
            return Err(Error::Query(format!("a slice search asks for 1 to 1000 rows, not {k}")));
        }
        if vector.is_empty() || vector.iter().any(|x| !x.is_finite()) {
            return Err(Error::Query("a vector search needs a vector of finite numbers".into()));
        }
        Ok(VectorSearch { vector, k })
    }

    /// The vector's length.
    pub fn norm(&self) -> f64 {
        self.vector.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt()
    }
}

impl SliceQuery for VectorSearch {
    fn k(&self) -> usize {
        self.k
    }

    fn answer(&self, d: &Device) -> Result<Answer> {
        d.search_vector(self)
    }

    /// Every column of the best `k` rows by similarity, by an exact scan:
    /// every row re-scored in float32 (`rerank` as large as the table).
    fn sql(&self, m: &Manifest) -> String {
        let v = m.vectors().expect("a vector slice's search");
        format!(
            "SELECT * FROM {table} ORDER BY similarity({field}, :v, strategy => 'flat', rerank => {rows}) DESC LIMIT {k}",
            table = m.schema.table,
            field = v.field,
            rows = v.rows.max(1),
            k = self.k
        )
    }

    fn params(&self, _: &Manifest) -> Params {
        let mut p = Params::new();
        p.0.insert("v".into(), Value::Vector(self.vector.as_slice().into()));
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

/// What a manifest says, without its words or centres, for an app to show:
/// kind, table, version, built_ms, tiles (count) and rows; a geo slice's
/// grid, text, lat and lon; a vector slice's field, dim, clusters and
/// codes_bytes (the size of its codes' file, null without one).
pub fn summary(m: &Manifest) -> Json {
    let mut j = json!({
        "kind": m.kind(), "table": m.schema.table, "version": m.version, "built_ms": m.built_ms,
        "tiles": m.tiles.len(), "rows": m.tiles.iter().map(|t| t.rows).sum::<u64>(),
    });
    match &m.key {
        Key::Geo(g) => {
            j["grid"] = json!(g.grid);
            j["text"] = json!(g.text);
            j["lat"] = json!(g.lat);
            j["lon"] = json!(g.lon);
        }
        Key::Vector(v) => {
            j["field"] = json!(v.field);
            j["dim"] = json!(v.dim);
            j["clusters"] = json!(v.centroids.len());
            j["codes_bytes"] = json!(v.codes.as_ref().map(|c| c.bytes));
        }
    }
    j
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

/// The fields a slice reads, checked: a primary key, the key's fields
/// ([`BuildKey::geo_fields`], [`BuildKey::vector_fields`]), and nothing a
/// row's JSON could not carry.
fn check_schema(s: &Schema, need: &[(&str, Need)]) -> Result<()> {
    let bad = |m: String| Err(Error::Query(format!("table {}: {m}", s.table)));
    if s.primary_key.is_none() {
        return bad("a slice needs a primary key, by which a device drops a tile's rows".into());
    }
    for (name, need) in need {
        let f = s.field(name);
        let ok = match need {
            Need::Text => f.is_some_and(|f| matches!(f.ty, FieldType::Text { .. }) && f.indexed),
            Need::Float => f.is_some_and(|f| f.ty == FieldType::Float && !f.multi),
            Need::Vector => f.is_some_and(|f| matches!(f.ty, FieldType::Vector { .. }) && !f.multi),
        };
        if !ok {
            return bad(match need {
                Need::Text => format!("{name} is not an indexed TEXT field"),
                Need::Float => format!("{name} is not a single-valued FLOAT field"),
                Need::Vector => format!("{name} is not a VECTOR field of one vector a row"),
            });
        }
    }
    for f in &s.fields {
        let plain = matches!(f.ty, FieldType::Text { .. } | FieldType::Keyword | FieldType::Int | FieldType::Float) || matches!(f.ty, FieldType::Vector { .. }) && !f.multi;
        if !plain || !f.generated_from.is_empty() || f.derive.is_some() {
            return bad(format!("field {} is not one a slice carries (TEXT, KEYWORD, INT, FLOAT or VECTOR, neither generated nor derived)", f.name));
        }
        if matches!(f.ty, FieldType::Text { .. }) && !f.stored {
            return bad(format!("TEXT field {} is not stored, so a tile could not carry it", f.name));
        }
    }
    Ok(())
}

/// What a key needs of a field ([`check_schema`]).
#[derive(Clone, Copy, Debug)]
enum Need {
    Text,
    Float,
    Vector,
}

#[cfg(test)]
mod tests;
