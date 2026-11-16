//! Loads the rows `prepare` wrote into a server's tables, a few tens of
//! megabytes of JSON lines a request (`POST /v1/import`), in the order the
//! references need: people, papers, then the graphs and the events.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use crate::strata::Strata;

pub const ORDER: [&str; 11] = ["ax_authors", "ax_papers", "ax_topics", "ax_concepts", "ax_cats", "ax_venues", "ax_complete", "ax_cites", "ax_coauthors", "ax_events", "ax_feeds"];

/// The rows of the graph's nodes a line of papers or people gives: its key.
fn node_line(line: &[u8]) -> Option<Vec<u8>> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    let id = v.get("id")?.as_str()?;
    let mut out = serde_json::json!({"id": id}).to_string().into_bytes();
    out.push(b'\n');
    Some(out)
}

async fn import(db: &Strata, t: &str, body: Vec<u8>, progress: &mut impl FnMut(&str)) -> Result<(), String> {
    // A server behind on its writes (429) is asked again two seconds later,
    // for up to 20 minutes; other failures are tried five times.
    let (mut tries, mut busy) = (0, 0);
    loop {
        match db.import_lines(t, body.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) if e.status == 429 && busy < 600 => {
                busy += 1;
                if busy % 30 == 1 {
                    progress(&format!("{t}: {e}; waiting for it"));
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(e) if tries < 5 && (e.status == 0 || e.status >= 500) => {
                tries += 1;
                progress(&format!("{t}: {e}; trying again"));
                tokio::time::sleep(std::time::Duration::from_secs(2 << tries)).await;
            }
            Err(e) => return Err(format!("{t}: {e}")),
        }
    }
}

/// The tables whose rows carry `by_year` (few rows each).
const SMALL: [&str; 4] = ["ax_topics", "ax_concepts", "ax_cats", "ax_venues"];

/// Loads `dir/<table>.jsonl` for each table in `only` (all when empty),
/// each into its server (the papers' and people's keys into the graph
/// server's nodes too), after its first `skip` lines (to go on with a load
/// that stopped: the rows a table has are the file's first lines, in order;
/// a request the server refuses part way is sent whole again, so keep them
/// small, `--chunk-mb 1`, where the server is slow to take them);
/// `progress` hears each request.
pub async fn load(search: &Strata, graph: &Strata, dir: &Path, only: &[String], chunk_mb: usize, skip: u64, mut progress: impl FnMut(&str)) -> Result<u64, String> {
    let mut total = 0u64;
    for t in ORDER {
        let db = if crate::schema::on_search(t) { search } else { graph };
        let nodes = t == "ax_papers" || t == "ax_authors";
        let mut node_buf: Vec<u8> = Vec::new();
        if !only.is_empty() && !only.iter().any(|o| o == t) {
            continue;
        }
        let path = dir.join(format!("{t}.jsonl"));
        let Ok(f) = std::fs::File::open(&path) else {
            progress(&format!("{t}: no {}, skipped", path.display()));
            continue;
        };
        let mut r = BufReader::with_capacity(1 << 20, f);
        let t0 = Instant::now();
        let mut rows = 0u64;
        let mut buf: Vec<u8> = Vec::with_capacity(chunk_mb << 20);
        let mut n = 0u64;
        let mut line = Vec::new();
        for _ in 0..skip {
            line.clear();
            if r.read_until(b'\n', &mut line).map_err(|e| format!("{}: {e}", path.display()))? == 0 {
                break;
            }
        }
        if skip > 0 {
            progress(&format!("{t}: the first {skip} lines skipped"));
        }
        loop {
            line.clear();
            let got = r.read_until(b'\n', &mut line).map_err(|e| format!("{}: {e}", path.display()))?;
            if got > 0 {
                if SMALL.contains(&t) {
                    // Counts by year are written as an object; the table keeps
                    // them as JSON text.
                    let mut v: serde_json::Value = serde_json::from_slice(&line).map_err(|e| format!("{}: {e}", path.display()))?;
                    if let Some(o) = v.get_mut("by_year").filter(|x| x.is_object()) {
                        *o = serde_json::Value::String(o.to_string());
                    }
                    buf.extend_from_slice(v.to_string().as_bytes());
                    buf.push(b'\n');
                } else {
                    buf.extend_from_slice(&line);
                }
                if nodes {
                    if let Some(x) = node_line(&line) {
                        node_buf.extend_from_slice(&x);
                    }
                }
                n += 1;
            }
            if (got == 0 && !buf.is_empty()) || buf.len() >= chunk_mb << 20 {
                let body = std::mem::replace(&mut buf, Vec::with_capacity(chunk_mb << 20));
                if nodes && !node_buf.is_empty() {
                    let nb = std::mem::take(&mut node_buf);
                    let (mut q1, mut q2) = (|_: &str| {}, |_: &str| {});
                    let (a, b) = tokio::join!(import(db, t, body, &mut q1), import(graph, "ax_nodes", nb, &mut q2));
                    a?;
                    b?;
                } else {
                    import(db, t, body, &mut progress).await?;
                }
                rows += n;
                n = 0;
                let s = t0.elapsed().as_secs_f64();
                progress(&format!("{t}: {rows} rows ({:.0} a second)", rows as f64 / s.max(0.001)));
            }
            if got == 0 {
                break;
            }
        }
        total += rows;
    }
    Ok(total)
}
