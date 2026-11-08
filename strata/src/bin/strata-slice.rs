//! Builds a table's slice ([`strata::slice`]): the files a device fetches
//! to search near its user, which any web server can serve.
//!
//! ```text
//! strata-slice places.strata --text body --lat lat --lon lon --lat0 40.7 --lon0 -74.0 --out slices/places
//! ```
//!
//! strata-server builds and serves them itself (`--slices DIR`, `POST
//! /v1/admin/slices`).

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use strata::db::Db;
use strata::slice::{build, BuildOptions, Grid};
use strata::storage::DbOptions;
use strata::text::Bm25Params;

#[derive(Parser)]
#[command(name = "strata-slice", about = "Build a table's slice: a manifest and its rows by map tile, for devices")]
struct Args {
    /// Database file.
    file: PathBuf,
    /// The table (the first one by default).
    #[arg(long)]
    table: Option<String>,
    /// The TEXT field searched.
    #[arg(long)]
    text: String,
    /// The FLOAT fields of latitude and longitude.
    #[arg(long, default_value = "lat")]
    lat: String,
    #[arg(long, default_value = "lon")]
    lon: String,
    /// The grid's origin, near the middle of the city.
    #[arg(long, allow_hyphen_values = true)]
    lat0: f64,
    #[arg(long, allow_hyphen_values = true)]
    lon0: f64,
    /// A tile's side, in metres.
    #[arg(long, default_value_t = 1000.0)]
    tile_m: f64,
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
    let opts = BuildOptions { table: a.table, text: a.text, lat: a.lat, lon: a.lon, grid: Grid::new(a.lat0, a.lon0, a.tile_m)?, bm25: Bm25Params::default() };
    let t0 = Instant::now();
    let b = build(&db, &opts, &a.out)?;
    let m = &b.manifest;
    println!(
        "{}: {} rows in {} tiles, {} words; manifest {:.2} MB, tiles {:.2} MB; {:.1} s",
        a.out.display(),
        b.rows,
        m.tiles.len(),
        m.words.terms.len(),
        b.manifest_bytes as f64 / 1e6,
        b.tile_bytes as f64 / 1e6,
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
