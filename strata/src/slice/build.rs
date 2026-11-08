//! Building a slice's files from a table ([`build`]).

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value as Json;

use super::{check_schema, value_json, Grid, Manifest, Tile, TileEntry, Words, FORMAT};
use crate::db::Db;
use crate::error::{Error, Result};
use crate::text::{Bm25Params, CollectionStats};

/// What to build a slice of.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    /// The table (the first one when None).
    pub table: Option<String>,
    pub text: String,
    pub lat: String,
    pub lon: String,
    pub grid: Grid,
    pub bm25: Bm25Params,
}

/// A slice written ([`build`]).
#[derive(Clone, Debug)]
pub struct Built {
    pub manifest: Manifest,
    /// Bytes of `manifest.json` and of all the tile files.
    pub manifest_bytes: u64,
    pub tile_bytes: u64,
    pub rows: u64,
}

/// Writes the slice of a table into directory `out` (made if missing):
/// `manifest.json` and `tiles/<x>_<y>.jsonl.gz` for every tile with rows,
/// all of one snapshot. Every row needs a latitude and a longitude.
pub fn build(db: &Db, opts: &BuildOptions, out: &Path) -> Result<Built> {
    let snap = db.snapshot();
    let table = opts.table.clone().unwrap_or_else(|| snap.schema().table.clone());
    let snap = snap.table(&table).ok_or_else(|| Error::Query(format!("no such table: {table}")))?;
    let schema = snap.schema().clone();
    check_schema(&schema, &opts.text, &opts.lat, &opts.lon)?;
    let field = schema.field_index(&opts.text).expect("checked");

    // The text field's statistics over every segment, as a query on this
    // snapshot sums them, then each word's best score under them.
    let readers = db.readers(&snap)?;
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
                if let Some(b) = t.term_bound(&w, &stats, opts.bm25) {
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
    let (lat, lon) = (schema.field_index(&opts.lat).expect("checked"), schema.field_index(&opts.lon).expect("checked"));
    let mut tiles: BTreeMap<Tile, (u64, Vec<u8>)> = BTreeMap::new();
    let mut rows = 0u64;
    for doc in db.documents(&snap) {
        let doc = doc?;
        let (Some(a), Some(o)) = (doc[lat].as_f64(), doc[lon].as_f64()) else {
            let key = schema.primary_key.map(|k| format!("{:?}", doc[k])).unwrap_or_default();
            return Err(Error::Query(format!("row {key} has no {} or {}: every row of a slice needs a position", opts.lat, opts.lon)));
        };
        let line: Vec<Json> = doc.iter().map(value_json).collect();
        let e = tiles.entry(opts.grid.tile(a, o)).or_default();
        e.0 += 1;
        serde_json::to_writer(&mut e.1, &line).map_err(|e| Error::Query(format!("a tile's row: {e}")))?;
        e.1.push(b'\n');
        rows += 1;
    }

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
    let manifest = Manifest {
        format: FORMAT,
        version: snap.txn(),
        built_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
        schema,
        text: opts.text.clone(),
        lat: opts.lat.clone(),
        lon: opts.lon.clone(),
        grid: opts.grid,
        k1: opts.bm25.k1,
        b: opts.bm25.b,
        n_docs: stats.n_docs,
        total_len: stats.total_len,
        words,
        tiles: entries,
    };
    let text = manifest.to_json();
    std::fs::write(out.join("manifest.json"), &text)?;
    Ok(Built { manifest, manifest_bytes: text.len() as u64, tile_bytes, rows })
}
