//! Makes the tables, the rules and the words, and loads the rows the
//! tools wrote (`tools/places.py`, `tools/years.py`), a few megabytes of
//! JSON lines a request (`POST /v1/import`).

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::schema;
use crate::strata::Strata;

/// Makes the tables that are not there yet, and the rules and words when
/// their tables are new.
pub async fn create(db: &Strata) -> Result<Vec<String>> {
    let have = db.tables().await.map_err(|e| anyhow!("{e}"))?;
    let mut made = Vec::new();
    for (name, sql, _) in schema::tables() {
        if have.iter().any(|t| t == name) {
            continue;
        }
        match db.sql(&sql).await {
            Ok(_) => made.push(name.to_string()),
            // A server without the model (built without `vision`): the app
            // runs, and searches by words or pictures say why they fail.
            Err(e) if name == "ea_words" => tracing::warn!(error = %e, "ea_words: no model for words and pictures"),
            Err(e) => return Err(anyhow!("{name}: {e}")),
        }
    }
    // Tables made before a column was added: the column, once.
    if have.iter().any(|t| t == "ea_fires") && db.query("SELECT perimeter FROM ea_fires LIMIT 1", json!({})).await.is_err() {
        if let Err(e) = db.sql("ALTER TABLE ea_fires ADD COLUMN perimeter KEYWORD").await {
            let m = e.to_string();
            if !m.contains("exist") && !m.contains("duplicate") {
                tracing::warn!(error = %m, "ea_fires.perimeter");
            }
        }
    }
    if have.iter().any(|t| t == "ea_places") && db.query("SELECT sub_km FROM ea_places LIMIT 1", json!({})).await.is_err() {
        for (c, t) in schema::LAND_COLUMNS {
            if let Err(e) = db.sql(&format!("ALTER TABLE ea_places ADD COLUMN {c} {t}")).await {
                let m = e.to_string();
                if !m.contains("exist") && !m.contains("duplicate") {
                    tracing::warn!(error = %m, column = c, "ea_places");
                }
            }
        }
    }
    for col in ["sky", "change"] {
        if have.iter().any(|t| t == "ea_watches") && db.query(&format!("SELECT {col} FROM ea_watches LIMIT 1"), json!({})).await.is_err() {
            if let Err(e) = db.sql(&format!("ALTER TABLE ea_watches ADD COLUMN {col} BOOL")).await {
                let m = e.to_string();
                if !m.contains("exist") && !m.contains("duplicate") {
                    tracing::warn!(error = %m, column = col, "ea_watches");
                }
            }
        }
    }
    // The rules and the words when their tables are new or empty (a run
    // stopped between making a table and writing its rows).
    let empty = |t: &'static str| async move { db.query(&format!("SELECT COUNT(*) AS n FROM {t}"), json!({})).await.ok().and_then(|r| r.first().map(|x| crate::strata::RowExt::i(x, "n"))).unwrap_or(0) == 0 };
    if made.iter().any(|t| t == "ea_rules") || empty("ea_rules").await {
        crate::rules::install(db).await?;
    }
    let words_there = made.iter().any(|t| t == "ea_words") || have.iter().any(|t| t == "ea_words");
    if words_there && (made.iter().any(|t| t == "ea_words") || empty("ea_words").await) {
        if let Err(e) = words(db).await {
            tracing::warn!(error = %e, "the words of changes");
        }
    }
    Ok(made)
}

/// The kinds of land a change is read against: each place's look in a
/// year is named by the nearest of these (SigLIP's own words for them).
pub const LABELS: &[(&str, &str)] = &[
    ("forest", "a satellite photo of a dense forest"),
    ("burn scar", "a satellite photo of a burned forest, a dark burn scar after a wildfire"),
    ("shrubland", "a satellite photo of dry shrubland and chaparral on hills"),
    ("grassland", "a satellite photo of grassland and pasture"),
    ("orchards", "a satellite photo of orchards, rows of fruit trees"),
    ("vineyards", "a satellite photo of vineyards"),
    ("fields", "a satellite photo of farm fields and crops"),
    ("bare fields", "a satellite photo of bare plowed fields"),
    ("suburbs", "a satellite photo of suburban houses and streets"),
    ("city", "a satellite photo of a dense city"),
    ("industry", "a satellite photo of warehouses and industrial buildings"),
    ("solar farm", "a satellite photo of a solar farm, rows of solar panels"),
    ("mine", "a satellite photo of an open pit mine"),
    ("evaporation ponds", "a satellite photo of brine evaporation ponds at a lithium mine"),
    ("desert", "a satellite photo of bare desert"),
    ("salt flat", "a satellite photo of a white salt flat"),
    ("lake", "a satellite photo of a lake"),
    ("dry lake", "a satellite photo of a dried up lake bed"),
    ("river", "a satellite photo of a river"),
    ("coast", "a satellite photo of a coastline and beach"),
    ("snow", "a satellite photo of snow covered mountains"),
    ("glacier", "a satellite photo of a glacier"),
    ("clouds", "a satellite photo covered by clouds"),
    ("plantation", "a satellite photo of a pine or eucalyptus plantation"),
    ("clear cut", "a satellite photo of clear cut forest, logged land"),
];

/// The labels, written as rows of `ea_words`: the server embeds each
/// sentence with SigLIP as it is written.
pub async fn words(db: &Strata) -> Result<()> {
    let rows: Vec<serde_json::Value> = LABELS.iter().map(|(k, t)| json!({"id": k, "text": t, "kind": "label"})).collect();
    db.upsert("ea_words", rows).await.map_err(|e| anyhow!("ea_words: {e}"))?;
    Ok(())
}

/// The files `load` reads, in order: `<dir>/<table>*.jsonl`.
pub const ORDER: [&str; 6] = ["ea_places", "ea_years", "ea_changes", "ea_perimeters", "ea_burns", "ea_climate"];

/// Loads `dir/<table>*.jsonl` for each table in `only` (all when empty).
pub async fn run(db: &Strata, dir: &Path, only: &[String], chunk_mb: usize) -> Result<u64> {
    create(db).await?;
    let mut total = 0u64;
    for t in ORDER {
        if !only.is_empty() && !only.iter().any(|o| o == t) {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(t) && n.ends_with(".jsonl")))
            .collect();
        files.sort();
        for path in files {
            total += import(db, t, &path, chunk_mb).await?;
        }
    }
    Ok(total)
}

async fn import(db: &Strata, table: &str, path: &Path, chunk_mb: usize) -> Result<u64> {
    let f = std::fs::File::open(path)?;
    let mut r = BufReader::with_capacity(1 << 20, f);
    let t0 = Instant::now();
    let (mut rows, mut n) = (0u64, 0u64);
    let cap = chunk_mb.max(1) << 20;
    let mut buf: Vec<u8> = Vec::with_capacity(cap);
    let mut line = Vec::new();
    loop {
        line.clear();
        let got = r.read_until(b'\n', &mut line)?;
        if got > 0 {
            buf.extend_from_slice(&line);
            n += 1;
        }
        if (got == 0 && !buf.is_empty()) || buf.len() >= cap {
            let body = std::mem::replace(&mut buf, Vec::with_capacity(cap));
            let mut tries = 0;
            loop {
                match db.import_lines(table, body.clone()).await {
                    Ok(_) => break,
                    Err(e) if tries < 5 && (e.status == 0 || e.status == 429 || e.status >= 500) => {
                        tries += 1;
                        tracing::warn!(table, error = %e, "trying again");
                        tokio::time::sleep(std::time::Duration::from_secs(2 << tries)).await;
                    }
                    Err(e) => return Err(anyhow!("{table}: {e}")),
                }
            }
            rows += n;
            n = 0;
            tracing::info!(table, file = %path.display(), rows, per_s = (rows as f64 / t0.elapsed().as_secs_f64().max(0.001)).round(), "loading");
        }
        if got == 0 {
            break;
        }
    }
    Ok(rows)
}
