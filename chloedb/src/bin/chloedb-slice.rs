//! Builds a table's slice ([`chloedb::slice`]): the files a device fetches
//! to search near its user, or among the rows closest to its user's taste,
//! which any web server can serve.
//!
//! ```text
//! chloedb-slice places.strata --text body --lat lat --lon lon --lat0 40.7 --lon0 -74.0 --out slices/places
//! chloedb-slice tracks.strata --vector emb --clusters 800 --out slices/tracks
//! ```
//!
//! chloedb-server builds and serves them itself (`--slices DIR`, `POST
//! /v1/admin/slices`).

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use chloedb::db::Db;
use chloedb::slice::{build, BuildKey, BuildOptions, Grid, Key};
use chloedb::storage::DbOptions;
use chloedb::text::Bm25Params;

#[derive(Parser)]
#[command(name = "chloedb-slice", about = "Build a table's slice: a manifest and its rows by map tile or by vector cluster, for devices")]
struct Args {
    /// Database file.
    file: PathBuf,
    /// The table (the first one by default).
    #[arg(long)]
    table: Option<String>,
    /// The TEXT field searched (a geo slice).
    #[arg(long, conflicts_with = "vector")]
    text: Option<String>,
    /// The FLOAT fields of latitude and longitude.
    #[arg(long, default_value = "lat")]
    lat: String,
    #[arg(long, default_value = "lon")]
    lon: String,
    /// The grid's origin, near the middle of the city.
    #[arg(long, allow_hyphen_values = true)]
    lat0: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    lon0: Option<f64>,
    /// A tile's side, in metres.
    #[arg(long, default_value_t = 1000.0)]
    tile_m: f64,
    /// The VECTOR field searched (a vector slice).
    #[arg(long)]
    vector: Option<String>,
    /// A vector slice's clusters (0: about the square root of the rows).
    #[arg(long, default_value_t = 0)]
    clusters: usize,
    /// The seed of a vector slice's k-means.
    #[arg(long, default_value_t = 7)]
    seed: u64,
    /// Leave out a vector slice's codes of every row (`codes.bin.gz`).
    #[arg(long)]
    no_codes: bool,
    /// The directory to write (made if missing).
    #[arg(long)]
    out: PathBuf,
    /// Open it read-only, to build while another process writes it.
    #[arg(long)]
    read_only: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Args::parse();
    let db = Db::open(&a.file, DbOptions { read_only: a.read_only, ..DbOptions::default() })?;
    let key = match (a.text, a.vector) {
        (Some(text), None) => {
            let (Some(lat0), Some(lon0)) = (a.lat0, a.lon0) else { return Err("a geo slice needs --lat0 and --lon0".into()) };
            BuildKey::Geo { text, lat: a.lat, lon: a.lon, grid: Grid::new(lat0, lon0, a.tile_m)?, bm25: Bm25Params::default() }
        }
        (None, Some(field)) => BuildKey::Vector { field, clusters: a.clusters, seed: a.seed, codes: !a.no_codes },
        _ => return Err("give --text (a geo slice) or --vector (a vector slice)".into()),
    };
    let t0 = Instant::now();
    let b = build(&db, &BuildOptions { table: a.table, key }, &a.out)?;
    let m = &b.manifest;
    let what = match &m.key {
        Key::Geo(g) => format!("{} tiles, {} words", m.tiles.len(), g.words.terms.len()),
        Key::Vector(v) => format!("{} clusters of {} dimensions{}", m.tiles.len(), v.dim, v.codes.as_ref().map_or(String::new(), |c| format!(", codes {:.2} MB", c.bytes as f64 / 1e6))),
    };
    println!(
        "{}: {} rows in {what}; manifest {:.2} MB, tiles {:.2} MB; {:.1} s",
        a.out.display(),
        b.rows,
        b.manifest_bytes as f64 / 1e6,
        b.tile_bytes as f64 / 1e6,
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
