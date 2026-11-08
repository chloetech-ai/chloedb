//! Building a slice's files from a table ([`build`]).

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value as Json;

use rand::{Rng, SeedableRng};

use super::{check_schema, value_json, Codes, Geo, Grid, Key, Manifest, Need, Tile, TileEntry, Vectors, Words, CODES_FILE, FORMAT};
use crate::db::Db;
use crate::error::{Error, Result};
use crate::text::{Bm25Params, CollectionStats};
use crate::value::Value;

/// What to build a slice of.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    /// The table (the first one when None).
    pub table: Option<String>,
    pub key: BuildKey,
}

/// How to cut the table ([`super::Key`]).
#[derive(Clone, Debug)]
pub enum BuildKey {
    /// Into the tiles of `grid` by each row's latitude and longitude, for
    /// searches of `text` near a user.
    Geo { text: String, lat: String, lon: String, grid: Grid, bm25: Bm25Params },
    /// Into `clusters` k-means clusters of a VECTOR field (0: about the
    /// square root of the rows, at most 4,096), trained on a sample of up
    /// to [`TRAIN_SAMPLE`] rows drawn with `seed`; with every row's codes
    /// ([`super::Codes`]) unless not `codes`.
    Vector { field: String, clusters: usize, seed: u64, codes: bool },
}

/// The most rows a vector slice's k-means trains on.
pub const TRAIN_SAMPLE: usize = 50_000;

/// Rows a vector slice assigns to clusters at a time.
const BATCH: usize = 8_192;

impl BuildKey {
    pub(super) fn geo_fields<'a>(text: &'a str, lat: &'a str, lon: &'a str) -> Vec<(&'a str, Need)> {
        vec![(text, Need::Text), (lat, Need::Float), (lon, Need::Float)]
    }

    pub(super) fn vector_fields(field: &str) -> Vec<(&str, Need)> {
        vec![(field, Need::Vector)]
    }

    fn fields(&self) -> Vec<(&str, Need)> {
        match self {
            BuildKey::Geo { text, lat, lon, .. } => Self::geo_fields(text, lat, lon),
            BuildKey::Vector { field, .. } => Self::vector_fields(field),
        }
    }
}

/// A slice written ([`build`]).
#[derive(Clone, Debug)]
pub struct Built {
    pub manifest: Manifest,
    /// Bytes of `manifest.json` and of all the tile files (a vector
    /// slice's codes are [`super::Codes::bytes`]).
    pub manifest_bytes: u64,
    pub tile_bytes: u64,
    pub rows: u64,
}

/// Writes the slice of a table into directory `out` (made if missing):
/// `manifest.json` and `tiles/<x>_<y>.jsonl.gz` for every tile with rows
/// (and a vector slice's `codes.bin.gz`), all of one snapshot. Every row
/// needs a latitude and a longitude, or a vector.
pub fn build(db: &Db, opts: &BuildOptions, out: &Path) -> Result<Built> {
    let snap = db.snapshot();
    let table = opts.table.clone().unwrap_or_else(|| snap.schema().table.clone());
    let snap = snap.table(&table).ok_or_else(|| Error::Query(format!("no such table: {table}")))?;
    let schema = snap.schema().clone();
    check_schema(&schema, &opts.key.fields())?;
    let (mut key, tiles, rows, codes) = match &opts.key {
        BuildKey::Geo { text, lat, lon, grid, bm25 } => {
            let (k, t, n) = geo(db, &snap, &schema, text, lat, lon, *grid, *bm25)?;
            (k, t, n, None)
        }
        BuildKey::Vector { field, clusters, seed, codes } => vectors(db, &snap, &schema, field, *clusters, *seed, *codes)?,
    };

    std::fs::create_dir_all(out.join("tiles"))?;
    let mut entries = Vec::with_capacity(tiles.len());
    let mut tile_bytes = 0;
    for (t, (n, text)) in tiles {
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&text)?;
        let bytes = gz.finish()?;
        std::fs::write(out.join(t.path()), &bytes)?;
        tile_bytes += bytes.len() as u64;
        entries.push(TileEntry { x: t.x, y: t.y, rows: n, bytes: bytes.len() as u64 });
    }
    if let (Some(mut by_tile), Key::Vector(v)) = (codes, &mut key) {
        // In the manifest's order of tiles, which is the map's.
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        for e in &entries {
            gz.write_all(&by_tile.remove(&e.tile()).unwrap_or_default())?;
        }
        let bytes = gz.finish()?;
        std::fs::write(out.join(CODES_FILE), &bytes)?;
        if let Some(c) = v.codes.as_mut() {
            c.bytes = bytes.len() as u64;
        }
    }
    let manifest = Manifest {
        format: FORMAT,
        version: snap.txn(),
        built_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
        schema,
        key,
        tiles: entries,
    };
    let text = manifest.to_json();
    std::fs::write(out.join("manifest.json"), &text)?;
    Ok(Built { manifest, manifest_bytes: text.len() as u64, tile_bytes, rows })
}

/// Rows by tile: each tile's row count and its JSON lines.
type Tiles = BTreeMap<Tile, (u64, Vec<u8>)>;

fn push_line(tiles: &mut Tiles, t: Tile, doc: &[Value]) -> Result<()> {
    let line: Vec<Json> = doc.iter().map(value_json).collect();
    let e = tiles.entry(t).or_default();
    e.0 += 1;
    serde_json::to_writer(&mut e.1, &line).map_err(|e| Error::Query(format!("a tile's row: {e}")))?;
    e.1.push(b'\n');
    Ok(())
}

fn key_of(schema: &crate::schema::Schema, doc: &[Value]) -> String {
    schema.primary_key.map(|k| format!("{:?}", doc[k])).unwrap_or_default()
}

/// A geo slice's key and tiles: the text field's statistics, each word's
/// best score, and the rows by map tile.
#[allow(clippy::too_many_arguments)]
fn geo(db: &Db, snap: &crate::storage::Snapshot, schema: &crate::schema::Schema, text: &str, lat: &str, lon: &str, grid: Grid, bm25: Bm25Params) -> Result<(Key, Tiles, u64)> {
    let field = schema.field_index(text).expect("checked");
    // The text field's statistics over every segment, as a query on this
    // snapshot sums them, then each word's best score under them.
    let readers = db.readers(snap)?;
    let mut stats = CollectionStats::default();
    for r in &readers {
        if let Some(t) = r.text(field)? {
            stats.n_docs += t.n_docs_with_terms() as u64;
            stats.total_len += t.total_len();
            t.for_each_term(|w, info| *stats.df.entry(String::from_utf8_lossy(w).into_owned()).or_default() += info.df as u64);
        }
    }
    let mut best: HashMap<String, f32> = HashMap::with_capacity(stats.df.len());
    for r in &readers {
        if let Some(t) = r.text(field)? {
            t.for_each_term(|w, _| {
                let w = String::from_utf8_lossy(w);
                if let Some(b) = t.term_bound(&w, &stats, bm25) {
                    let e = best.entry(w.into_owned()).or_insert(0.0);
                    *e = e.max(b);
                }
            });
        }
    }
    let mut words = Words::default();
    let sorted: BTreeMap<&String, &u64> = stats.df.iter().collect();
    for (w, &df) in sorted {
        words.terms.push(w.clone());
        words.df.push(df);
        words.best.push(best.get(w).copied().unwrap_or(0.0));
    }

    // The rows, by tile, one JSON array of values per line.
    let (la, lo) = (schema.field_index(lat).expect("checked"), schema.field_index(lon).expect("checked"));
    let mut tiles = Tiles::new();
    let mut rows = 0u64;
    for doc in db.documents(snap) {
        let doc = doc?;
        let (Some(a), Some(o)) = (doc[la].as_f64(), doc[lo].as_f64()) else {
            return Err(Error::Query(format!("row {} has no {lat} or {lon}: every row of a slice needs a position", key_of(schema, &doc))));
        };
        push_line(&mut tiles, grid.tile(a, o), &doc)?;
        rows += 1;
    }
    let key = Key::Geo(Geo { text: text.into(), lat: lat.into(), lon: lon.into(), grid, k1: bm25.k1, b: bm25.b, n_docs: stats.n_docs, total_len: stats.total_len, words });
    Ok((key, tiles, rows))
}

/// Each cluster's extent, as its rows are assigned.
struct Shape {
    radii: Vec<f64>,
    angles: Vec<f64>,
    max_norms: Vec<f64>,
    min_norms: Vec<f64>,
}

/// A vector slice's key and tiles: k-means clusters of the field's vectors
/// (trained on a sample), each row in its nearest cluster, and each
/// cluster's radius.
/// A vector slice's key, tiles and each tile's rows' codes (when `codes`).
#[allow(clippy::type_complexity)]
fn vectors(db: &Db, snap: &crate::storage::Snapshot, schema: &crate::schema::Schema, field: &str, clusters: usize, seed: u64, codes: bool) -> Result<(Key, Tiles, u64, Option<BTreeMap<Tile, Vec<u8>>>)> {
    let fi = schema.field_index(field).expect("checked");
    let dim = match schema.fields[fi].ty {
        crate::schema::FieldType::Vector { dim, .. } => dim,
        _ => unreachable!("checked"),
    };
    let vector_of = |doc: &[Value]| -> Result<std::sync::Arc<[f32]>> {
        match &doc[fi] {
            Value::Vector(v) if v.len() == dim => Ok(v.clone()),
            _ => Err(Error::Query(format!("row {} has no vector in {field}: every row of a vector slice needs one", key_of(schema, doc)))),
        }
    };
    // A uniform sample of the rows (reservoir), to train on.
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut sample: Vec<std::sync::Arc<[f32]>> = Vec::new();
    let mut rows = 0u64;
    let (mut lo, mut hi) = (vec![f32::INFINITY; dim], vec![f32::NEG_INFINITY; dim]);
    for doc in db.documents(snap) {
        let v = vector_of(&doc?)?;
        rows += 1;
        for ((l, h), &x) in lo.iter_mut().zip(hi.iter_mut()).zip(v.iter()) {
            *l = l.min(x);
            *h = h.max(x);
        }
        if sample.len() < TRAIN_SAMPLE {
            sample.push(v);
        } else {
            let j = rng.gen_range(0..rows);
            if (j as usize) < TRAIN_SAMPLE {
                sample[j as usize] = v;
            }
        }
    }
    if rows == 0 {
        return Err(Error::Query(format!("table {} has no rows to slice", schema.table)));
    }
    let k = if clusters == 0 { ((rows as f64).sqrt().round() as usize).clamp(1, 4096) } else { clusters };
    let k = k.min(sample.len());
    let points: Vec<f32> = sample.iter().flat_map(|v| v.iter().copied()).collect();
    let cents = crate::sid::kmeans(&points, dim, k, 20, &mut rng);
    drop(points);
    let norms: Vec<f32> = cents.chunks_exact(dim).map(|r| r.iter().map(|x| x * x).sum()).collect();

    // Every row into its nearest cluster, a batch at a time on the cores;
    // each cluster's radius the farthest of its rows.
    let cent_norms: Vec<f64> = cents.chunks_exact(dim).map(|c| c.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt()).collect();
    let mut tiles = Tiles::new();
    let mut shape = Shape { radii: vec![0.0; k], angles: vec![0.0; k], max_norms: vec![0.0; k], min_norms: vec![f64::INFINITY; k] };
    // One byte a dimension: 255 steps from the least to the greatest
    // value, the step rounded up so that the greatest is within range.
    let step: Vec<f32> = lo
        .iter()
        .zip(&hi)
        .map(|(&l, &h)| {
            let mut s = ((h as f64 - l as f64) / 255.0) as f32;
            while (l as f64) + 255.0 * (s as f64) < h as f64 {
                s = f32::from_bits(s.to_bits() + 1);
            }
            s
        })
        .collect();
    let mut code_of: BTreeMap<Tile, Vec<u8>> = BTreeMap::new();
    let mut batch: Vec<Vec<Value>> = Vec::with_capacity(BATCH);
    let mut flush = |batch: &mut Vec<Vec<Value>>, tiles: &mut Tiles, shape: &mut Shape| -> Result<()> {
        let pts: Vec<f32> = batch.iter().flat_map(|d| d[fi].as_vector().expect("a vector").iter().copied()).collect();
        let a = crate::sid::assign(&pts, &cents, &norms, dim);
        for (doc, &c) in batch.iter().zip(&a) {
            let c = c as usize;
            let v = doc[fi].as_vector().expect("a vector");
            let m = &cents[c * dim..(c + 1) * dim];
            let d: f64 = v.iter().zip(m).map(|(&x, &m)| (x as f64 - m as f64).powi(2)).sum::<f64>().sqrt();
            let xn: f64 = v.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt();
            shape.radii[c] = shape.radii[c].max(d);
            shape.max_norms[c] = shape.max_norms[c].max(xn);
            shape.min_norms[c] = shape.min_norms[c].min(xn);
            if xn > 0.0 && cent_norms[c] > 0.0 {
                let dot: f64 = v.iter().zip(m).map(|(&x, &m)| x as f64 * m as f64).sum();
                shape.angles[c] = shape.angles[c].max((dot / (xn * cent_norms[c])).clamp(-1.0, 1.0).acos());
            } else if cent_norms[c] == 0.0 {
                shape.angles[c] = std::f64::consts::PI;
            }
            let t = Tile { x: c as i32, y: 0 };
            push_line(tiles, t, doc)?;
            if codes {
                let out = code_of.entry(t).or_default();
                out.extend(v.iter().zip(lo.iter().zip(&step)).map(|(&x, (&l, &s))| if s > 0.0 { ((x as f64 - l as f64) / s as f64).round().clamp(0.0, 255.0) as u8 } else { 0 }));
            }
        }
        batch.clear();
        Ok(())
    };
    for doc in db.documents(snap) {
        let doc = doc?;
        vector_of(&doc)?;
        batch.push(doc);
        if batch.len() == BATCH {
            flush(&mut batch, &mut tiles, &mut shape)?;
        }
    }
    flush(&mut batch, &mut tiles, &mut shape)?;
    // A cluster no row joined holds nothing: its bounds are never read.
    for n in shape.min_norms.iter_mut().filter(|n| !n.is_finite()) {
        *n = 0.0;
    }
    let centroids = cents.chunks_exact(dim).map(|c| c.to_vec()).collect();
    let Shape { radii, angles, max_norms, min_norms } = shape;
    let codes_key = codes.then(|| Codes { min: lo.clone(), step: step.clone(), bytes: 0 });
    let key = Key::Vector(Vectors { field: field.into(), dim, centroids, radii, angles, max_norms, min_norms, rows, codes: codes_key });
    Ok((key, tiles, rows, codes.then_some(code_of)))
}
