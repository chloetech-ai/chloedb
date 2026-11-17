//! The app's tables, ordinary ChloeDB tables any SQL reads: a row a
//! recording with its words and the vector of how it sounds, a row a
//! moment (ten seconds of a recording, every five seconds) with its own
//! vector, and the words of sounds the site listens for.
//!
//! The vectors are CLAP's (`clap-htsat-unfused`, LAION's model of sound and
//! text), made by ChloeDB's server: `tools/prepare.py` cuts each recording
//! into its moments and asks the server for each one's vector
//! (`POST /v1/embed` with the moment's sound), and a recording's vector is
//! the mean of those of its windows that start every ten seconds, which is
//! what the server makes of a whole file. A text, or a clip someone hums,
//! is embedded by the same model through `au_labels`, whose vectors the
//! server makes from each label's sentence.

use serde_json::{json, Value};

use crate::labels::LABELS;
use crate::strata::{Result, Strata};

/// CLAP, as ChloeDB names it.
pub const MODEL: &str = "clap-htsat-unfused";

/// The length of a vector of the model.
pub const DIM: usize = 512;

pub fn tables() -> Vec<(&'static str, String)> {
    vec![
        ("au_sounds", format!("CREATE TABLE IF NOT EXISTS au_sounds (
            id KEYWORD PRIMARY KEY,
            title TEXT, words TEXT,
            author KEYWORD, author_url KEYWORD OPTIONS(indexed = false), page KEYWORD OPTIONS(indexed = false),
            source KEYWORD, licence KEYWORD, licence_url KEYWORD OPTIONS(indexed = false), lic KEYWORD,
            kind KEYWORD, tags KEYWORD[], labels KEYWORD[], genres KEYWORD[],
            seconds FLOAT, year INT, lat FLOAT, lon FLOAT,
            audio KEYWORD OPTIONS(indexed = false), peaks TEXT OPTIONS(indexed = false),
            x FLOAT, y FLOAT, moments INT,
            emb VECTOR({DIM})
        )")),
        ("au_moments", format!("CREATE TABLE IF NOT EXISTS au_moments (
            id KEYWORD PRIMARY KEY,
            sound KEYWORD NOT NULL,
            t FLOAT, seconds FLOAT,
            emb VECTOR({DIM})
        ) OPTIONS(shard_by = 'sound', cluster_by = 'sound')")),
        ("au_labels", format!("CREATE TABLE IF NOT EXISTS au_labels (
            id KEYWORD PRIMARY KEY,
            name TEXT, grp KEYWORD, prompt TEXT,
            emb VECTOR({DIM}) OPTIONS(embed = 'prompt', model = '{MODEL}')
        )")),
    ]
}

/// Makes the tables that are not there yet, and the labels when
/// `au_labels` is empty (the server embeds their sentences as they are
/// written, which needs its CLAP model).
pub async fn ensure(db: &Strata) -> Result<Vec<String>> {
    let have = db.tables().await?;
    let mut made = Vec::new();
    for (name, sql) in tables() {
        if !have.iter().any(|t| t == name) {
            db.sql(&sql).await?;
            made.push(name.to_string());
        }
    }
    let n = db.query("SELECT COUNT(*) AS n FROM au_labels", Value::Null).await?.first().and_then(|r| r.get("n").and_then(|v| v.as_i64())).unwrap_or(0);
    if (n as usize) < LABELS.len() {
        let rows: Vec<Value> = LABELS.iter().map(|(id, name, grp, prompt)| json!({"id": id, "name": name, "grp": grp, "prompt": prompt})).collect();
        db.upsert("au_labels", rows).await?;
        made.push(format!("{} labels", LABELS.len()));
    }
    Ok(made)
}

/// How many rows each table holds.
pub async fn counts(db: &Strata) -> Value {
    let mut out = serde_json::Map::new();
    for (name, _) in tables() {
        let n = db.query(&format!("SELECT COUNT(*) AS n FROM {name}"), Value::Null).await.ok().and_then(|r| r.first().and_then(|r| r.get("n").cloned())).unwrap_or(Value::Null);
        out.insert(name.to_string(), n);
    }
    Value::Object(out)
}
