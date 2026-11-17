//! `load`: PD12M's records and vectors, with the kinds and themes
//! `prepare` made, into the servers' tables, as JSON lines sent in chunks
//! (`POST /v1/import`), two at a time, without a file between.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::{json, Value};

use crate::pd12m::{self, Vectors, PER_FILE, PER_VECTORS};
use crate::prepare;
use crate::schema;
use crate::strata::Strata;

pub struct Options {
    pub data: PathBuf,
    pub prepared: PathBuf,
    pub rows: usize,
    /// Rows a request.
    pub chunk: usize,
    /// Go on from this picture (the rows before it are in already).
    pub from: usize,
}

/// One picture's row as a JSON line.
pub fn line(n: usize, r: &pd12m::Record, kind: &str, theme: usize, v: &[f32]) -> String {
    let mp = (r.width as f64 * r.height as f64) / 1e6;
    let doc = json!({
        "id": r.id, "n": n, "url": r.url, "title": pd12m::title_of(&r.caption), "caption": r.caption,
        "source": r.source, "licence": r.licence(), "width": r.width, "height": r.height, "mp": (mp * 100.0).round() / 100.0,
        "shape": pd12m::shape_of(r.width, r.height), "size": pd12m::size_of(mp), "kind": kind, "theme": theme,
        "hash": r.hash, "mime": r.mime,
    });
    let mut s = doc.to_string();
    s.pop();
    s.push_str(",\"clip\":[");
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{:.5}", x);
    }
    s.push_str("]}\n");
    s
}

pub async fn run(db: &Strata, o: &Options) -> Result<(), String> {
    schema::create(db).await.map_err(|e| e.to_string())?;
    let themes: Value = serde_json::from_str(&std::fs::read_to_string(o.prepared.join("themes.json")).map_err(|e| format!("themes.json: {e} (run prepare first)"))?).map_err(|e| e.to_string())?;
    let labels = prepare::read_labels(&o.prepared)?;
    let rows = o.rows.min(labels.len() / 2);
    if o.from == 0 {
        let mut body = Vec::new();
        for t in themes["themes"].as_array().into_iter().flatten() {
            let doc = json!({
                "id": t["id"], "name": t["name"], "n": t["n"], "grp": t["group"], "row": t["row"], "col": t["col"],
                "reps": t["reps"], "near": t["near"], "kinds": t["kinds"].to_string(), "centre": t["centre"],
            });
            body.extend(doc.to_string().into_bytes());
            body.push(b'\n');
        }
        db.import_lines("img_themes", body).await.map_err(|e| e.to_string())?;
        eprintln!("themes: {}", themes["themes"].as_array().map_or(0, |a| a.len()));
    }
    let files = pd12m::files(&o.data)?;
    let t0 = Instant::now();
    let mut sent = o.from;
    let mut pending: Vec<tokio::task::JoinHandle<Result<(), String>>> = Vec::new();
    let mut body = String::new();
    let mut in_body = 0;
    let mut vectors: Option<(usize, Vectors)> = None;
    for (fi, path) in files.iter().enumerate() {
        let first = fi * PER_FILE;
        if first >= rows {
            break;
        }
        if first + PER_FILE <= o.from {
            continue;
        }
        let recs = pd12m::records(path)?;
        let vf = fi / (PER_VECTORS / PER_FILE);
        if vectors.as_ref().map(|v| v.0) != Some(vf) {
            let mut v = Vectors::open(&pd12m::vector_file(&o.data, fi))?;
            v.skip(first - vf * PER_VECTORS)?;
            vectors = Some((vf, v));
        }
        let v = &mut vectors.as_mut().expect("opened").1;
        for (j, r) in recs.iter().enumerate() {
            let n = first + j;
            if n >= rows {
                break;
            }
            let vec = v.next_row()?.ok_or_else(|| format!("{} ends before row {n}", path.display()))?;
            if n < o.from {
                continue;
            }
            let (kind, theme) = prepare::label(&labels, n).ok_or_else(|| format!("no label for row {n}"))?;
            body.push_str(&line(n, r, kind, theme, &vec));
            in_body += 1;
            if in_body == o.chunk {
                if pending.len() >= 2 {
                    pending.remove(0).await.map_err(|e| e.to_string())??;
                }
                let (db, b) = (db.clone(), std::mem::take(&mut body).into_bytes());
                pending.push(tokio::spawn(async move { send(&db, b).await }));
                sent += in_body;
                in_body = 0;
                let s = t0.elapsed().as_secs_f64();
                eprintln!("  {sent} of {rows} ({:.0} a second)", (sent - o.from) as f64 / s.max(0.001));
            }
        }
    }
    if in_body > 0 {
        send(db, body.into_bytes()).await?;
        sent += in_body;
    }
    for p in pending {
        p.await.map_err(|e| e.to_string())??;
    }
    eprintln!("loaded {} pictures in {:.0} s", sent - o.from, t0.elapsed().as_secs_f64());
    Ok(())
}

async fn send(db: &Strata, body: Vec<u8>) -> Result<(), String> {
    let mut tries = 0;
    loop {
        match db.import_lines("img_pictures", body.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) if tries < 5 && (e.status == 0 || e.status == 429 || e.status >= 500) => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(2 * tries)).await;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}
