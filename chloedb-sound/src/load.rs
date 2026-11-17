//! Loads the rows `tools/prepare.py` wrote into a server's tables, a few
//! megabytes of JSON lines a request (`POST /v1/import`): the recordings,
//! then their moments.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use crate::strata::Strata;

pub const ORDER: [&str; 2] = ["au_sounds", "au_moments"];

/// Loads `dir/<table>.jsonl` for each table in `only` (all when empty);
/// `progress` hears each request.
pub async fn load(db: &Strata, dir: &Path, only: &[String], chunk_mb: usize, mut progress: impl FnMut(&str)) -> Result<u64, String> {
    let mut total = 0u64;
    for t in ORDER {
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
        loop {
            line.clear();
            let got = r.read_until(b'\n', &mut line).map_err(|e| format!("{}: {e}", path.display()))?;
            if got > 0 {
                buf.extend_from_slice(&line);
                n += 1;
            }
            if (got == 0 && !buf.is_empty()) || buf.len() >= chunk_mb << 20 {
                let body = std::mem::replace(&mut buf, Vec::with_capacity(chunk_mb << 20));
                let mut tries = 0;
                loop {
                    match db.import_lines(t, body.clone()).await {
                        Ok(_) => break,
                        Err(e) if tries < 5 && (e.status == 0 || e.status == 429 || e.status >= 500) => {
                            tries += 1;
                            progress(&format!("{t}: {e}; trying again"));
                            tokio::time::sleep(std::time::Duration::from_secs(2 << tries)).await;
                        }
                        Err(e) => return Err(format!("{t}: {e}")),
                    }
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
