//! BGP updates into `bb_updates`, a row per prefix announced or withdrawn
//! by a peer of a RIS collector, each checked as it arrives (RPKI, the
//! usual origins, whose space it is). Two ways in:
//!
//! - **RIS's archive**: the MRT file each collector writes every five
//!   minutes, read from a time on until the newest one written;
//! - **RIS Live**: the same messages as they happen, a few seconds after
//!   the collector hears them, once the archive is caught up.
//!
//! With `--live` it also polls RIPE Atlas's latest rounds and IODA's
//! outages, so every table stays current.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rustc_hash::FxHashMap;
use serde_json::{json, Value};

use crate::load;
use crate::mrt::{Prefix, Update};
use crate::refdata::Reference;
use crate::strata::Strata;

pub const COLLECTORS: &[&str] = &[
    "rrc00", "rrc01", "rrc03", "rrc04", "rrc05", "rrc06", "rrc07", "rrc10", "rrc11", "rrc12", "rrc13", "rrc14", "rrc15", "rrc16", "rrc18", "rrc19", "rrc20", "rrc21", "rrc22", "rrc23", "rrc24", "rrc25", "rrc26",
];

/// Five-minute buckets since 2020-01-01, the middle part of a row's key.
const BUCKET0: i64 = 1_577_836_800 / 300;

fn collector_index(c: &str) -> i64 {
    c.trim_start_matches("rrc").parse::<i64>().unwrap_or(99)
}

/// A row's key: the collector, the five minutes, and the update's place in
/// them (archive files from 0, RIS Live from 2^25), so a file read again
/// writes the same keys.
pub fn key(collector: &str, ts: i64, idx: i64) -> i64 {
    (collector_index(collector) << 52) | ((ts / 300 - BUCKET0).max(0) << 26) | (idx & ((1 << 26) - 1))
}

fn iso_us(ts: u32, us: u32) -> String {
    chrono::DateTime::from_timestamp(ts as i64, us * 1000)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

/// The last origin each prefix was announced with, to give a withdrawal
/// the network it took away.
#[derive(Default)]
pub struct Last {
    origin: FxHashMap<Prefix, u32>,
}

/// One update as a row.
pub fn row(r: &Reference, last: &mut Last, collector: &str, id: i64, u: &Update) -> Value {
    let (lo, hi) = u.prefix.range();
    let mut v = json!({
        "id": id, "ts": iso_us(u.ts, u.us), "collector": collector, "peer": u.peer_ip, "peer_as": u.peer_as,
        "kind": if u.announce { "A" } else { "W" }, "prefix": u.prefix.to_string(), "af": if u.prefix.v6 { 6 } else { 4 },
        "plen": u.prefix.len, "lo": lo, "hi": hi,
    });
    if u.announce {
        let c = r.check(&u.prefix, u.origin);
        last.origin.insert(u.prefix, u.origin);
        v["origin"] = json!(u.origin);
        v["path"] = json!(u.path);
        v["route"] = json!(u.path.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" "));
        v["hops"] = json!(dedup_len(&u.path));
        v["rpki"] = json!(c.rpki);
        v["seen"] = json!(c.seen);
        v["owner"] = json!(c.owner);
        v["country"] = json!(r.country(u.origin));
        if !u.communities.is_empty() {
            v["comms"] = json!(u.communities.iter().map(|(a, b)| format!("{a}:{b}")).collect::<Vec<_>>());
        }
    } else {
        let origin = last.origin.get(&u.prefix).copied().or_else(|| r.usual.get(&u.prefix).and_then(|o| o.first().copied()));
        if let Some(o) = origin {
            v["origin"] = json!(o);
            v["country"] = json!(r.country(o));
            let c = r.check(&u.prefix, o);
            v["owner"] = json!(c.owner);
        }
    }
    v
}

fn dedup_len(p: &[u32]) -> usize {
    let mut n = 0;
    let mut last = None;
    for a in p {
        if Some(a) != last {
            n += 1;
            last = Some(a);
        }
    }
    n
}

pub struct Opts {
    pub dir: PathBuf,
    pub collectors: Vec<String>,
    /// Read the archive from this time (seconds); else start live.
    pub from: Option<i64>,
    /// Stop the archive here (seconds) rather than at the newest file.
    pub to: Option<i64>,
    /// After the archive, follow RIS Live, and poll Atlas and IODA.
    pub live: bool,
    pub atlas: Vec<u64>,
}

async fn note(db: &Strata, id: &str, kind: &str, at: i64, rows: usize, note: &str) {
    let at = chrono::DateTime::from_timestamp(at, 0).map(|d| d.to_rfc3339()).unwrap_or_default();
    let _ = db.upsert("bb_feed", vec![json!({"id": id, "kind": kind, "at": at, "rows": rows, "note": note})]).await;
}

/// Reads one archive file of `collector` into rows, written in chunks.
pub async fn archive_file(db: &Strata, r: &Reference, last: &mut Last, collector: &str, path: &std::path::Path) -> Result<usize> {
    let mut ups = Vec::new();
    let p = path.to_path_buf();
    let read = tokio::task::spawn_blocking(move || {
        let mut v = Vec::new();
        crate::mrt::read_gz(&p, |u| v.push(u)).map(|_| v)
    })
    .await??;
    ups.extend(read);
    // Rows are made a chunk at a time: a peer that resets sends its whole
    // table again, and a file then holds millions of updates.
    for (c, chunk) in ups.chunks(50_000).enumerate() {
        let rows: Vec<Value> = chunk.iter().enumerate().map(|(i, u)| row(r, last, collector, key(collector, u.ts as i64, (c * 50_000 + i) as i64), u)).collect();
        load::import(db, "bb_updates", rows).await?;
    }
    Ok(ups.len())
}

pub async fn run(db: Strata, r: Arc<Reference>, o: Opts) -> Result<()> {
    let mut last = Last::default();
    let http = crate::fetch::client();
    if let Some(from) = o.from {
        let mut t = from - from % 300;
        let stop = o.to.unwrap_or(i64::MAX);
        'files: while t < stop {
            for c in &o.collectors {
                let (dir, cc, http2) = (o.dir.clone(), c.clone(), http.clone());
                let got = tokio::task::spawn_blocking(move || crate::fetch::ris_file(&http2, &dir, &cc, t)).await??;
                let Some(path) = got else {
                    if o.to.is_some() {
                        tracing::warn!(collector = %c, t, "no archive file");
                        continue;
                    }
                    tracing::info!(collector = %c, t, "archive caught up");
                    break 'files;
                };
                let t0 = Instant::now();
                let n = archive_file(&db, &r, &mut last, c, &path).await?;
                // Read and kept as rows: the file is not needed again.
                let _ = std::fs::remove_file(&path);
                tracing::info!(collector = %c, file = %path.display(), rows = n, secs = t0.elapsed().as_secs_f32(), "archive file");
                note(&db, &format!("ris:{c}"), "archive", t + 300, n, &path.file_name().unwrap_or_default().to_string_lossy()).await;
            }
            t += 300;
        }
    }
    if !o.live {
        return Ok(());
    }
    // Atlas and IODA every few minutes, beside the stream.
    {
        let (db, r, dir, msms) = (db.clone(), r.clone(), o.dir.clone(), o.atlas.clone());
        tokio::spawn(async move {
            loop {
                if let Err(e) = poll_sources(&db, &r, &dir, &msms).await {
                    tracing::warn!(error = %e, "polling Atlas and IODA");
                }
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
        });
    }
    let mut tasks = Vec::new();
    for c in o.collectors.clone() {
        let (db, r) = (db.clone(), r.clone());
        tasks.push(tokio::spawn(async move { live(db, r, c).await }));
    }
    for t in tasks {
        t.await??;
    }
    Ok(())
}

/// The latest Atlas rounds and the last two days of IODA, written by key.
pub async fn poll_sources(db: &Strata, r: &Reference, dir: &std::path::Path, msms: &[u64]) -> Result<()> {
    let (dir2, msms2) = (dir.to_path_buf(), msms.to_vec());
    tokio::task::spawn_blocking(move || crate::fetch::run(crate::fetch::Opts { dir: dir2, only: vec!["ioda".into()], atlas: msms2, fresh: Duration::ZERO })).await??;
    let msms3 = msms.to_vec();
    let dir3 = dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = crate::fetch::client();
        for m in msms3 {
            crate::fetch::get(&c, &format!("https://atlas.ripe.net/api/v2/measurements/{m}/latest/?format=json"), &dir3.join(format!("atlas/{m}.json")))?;
        }
        Ok(())
    })
    .await??;
    load::run(db, load::Opts { dir, only: vec!["atlas".into(), "ioda".into()] }, r).await?;
    note(db, "atlas", "poll", chrono::Utc::now().timestamp(), 0, "").await;
    Ok(())
}

/// RIS Live's messages for one collector, written every second; the
/// connection is made again when it drops.
async fn live(db: Strata, r: Arc<Reference>, collector: String) -> Result<()> {
    use futures_util::StreamExt;
    let http = reqwest::Client::builder().user_agent(crate::fetch::UA).connect_timeout(Duration::from_secs(20)).build()?;
    let mut last = Last::default();
    let mut idx: FxHashMap<i64, i64> = FxHashMap::default();
    let mut wait = Duration::from_secs(1);
    loop {
        let url = format!("https://ris-live.ripe.net/v1/stream/?format=json&client=chloedb-backbone&host={collector}&type=UPDATE");
        let resp = match http.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(collector, status = %r.status(), "RIS Live");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
                continue;
            }
            Err(e) => {
                tracing::warn!(collector, error = %e, "RIS Live");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        tracing::info!(collector, "RIS Live: following");
        wait = Duration::from_secs(1);
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut rows: Vec<Value> = Vec::new();
        let mut flushed = Instant::now();
        let mut noted = Instant::now();
        let mut total = 0usize;
        loop {
            let next = tokio::time::timeout(Duration::from_secs(60), stream.next()).await;
            let chunk = match next {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    tracing::warn!(collector, error = %e, "RIS Live: dropped");
                    break;
                }
                Ok(None) | Err(_) => {
                    tracing::warn!(collector, "RIS Live: ended or silent");
                    break;
                }
            };
            buf.extend_from_slice(&chunk);
            while let Some(i) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=i).collect();
                let Ok(m) = serde_json::from_slice::<Value>(&line) else { continue };
                // RIS Live sends every collector's messages whatever the
                // filter says, so they are picked here.
                if !m["data"]["host"].as_str().is_some_and(|h| h.split('.').next() == Some(collector.as_str())) {
                    continue;
                }
                for u in from_live(&m["data"]) {
                    let b = u.ts as i64 / 300;
                    let n = idx.entry(b).or_insert(1 << 25);
                    *n += 1;
                    rows.push(row(&r, &mut last, &collector, key(&collector, u.ts as i64, *n), &u));
                }
            }
            if (flushed.elapsed() >= Duration::from_secs(1) || rows.len() >= 50_000) && !rows.is_empty() {
                total += rows.len();
                if let Err(e) = load::import(&db, "bb_updates", std::mem::take(&mut rows)).await {
                    tracing::warn!(collector, error = %e, "RIS Live: write");
                }
                if idx.len() > 4 {
                    let newest = *idx.keys().max().unwrap_or(&0);
                    idx.retain(|k, _| *k >= newest - 2);
                }
                if noted.elapsed() >= Duration::from_secs(5) {
                    noted = Instant::now();
                    note(&db, &format!("ris:{collector}"), "live", chrono::Utc::now().timestamp(), total, "RIS Live").await;
                }
                flushed = Instant::now();
            }
        }
        tokio::time::sleep(wait).await;
    }
}

/// A RIS Live message's updates, in the archive's shape.
pub fn from_live(d: &Value) -> Vec<Update> {
    if d["type"].as_str() != Some("UPDATE") {
        return Vec::new();
    }
    let t = d["timestamp"].as_f64().unwrap_or(0.0);
    let ts = t as u32;
    let us = ((t - ts as f64) * 1e6) as u32;
    let peer_as = d["peer_asn"].as_str().and_then(|s| s.parse().ok()).or(d["peer_asn"].as_u64().map(|x| x as u32)).unwrap_or(0);
    let peer_ip = d["peer"].as_str().unwrap_or("").to_string();
    let mut path = Vec::new();
    let mut origin = 0;
    for (i, a) in d["path"].as_array().into_iter().flatten().enumerate() {
        let last = i + 1 == d["path"].as_array().map_or(0, |v| v.len());
        match a {
            Value::Number(n) => {
                let x = n.as_u64().unwrap_or(0) as u32;
                path.push(x);
                if last {
                    origin = x;
                }
            }
            Value::Array(set) => {
                let xs: Vec<u32> = set.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
                if last {
                    origin = if xs.len() == 1 { xs[0] } else { 0 };
                }
                path.extend(xs);
            }
            _ => {}
        }
    }
    let communities: Vec<(u16, u16)> = d["community"].as_array().into_iter().flatten().filter_map(|c| Some((c[0].as_u64()? as u16, c[1].as_u64()? as u16))).collect();
    let mut out = Vec::new();
    for p in d["withdrawals"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).filter_map(Prefix::parse) {
        out.push(Update { ts, us, peer_as, peer_ip: peer_ip.clone(), announce: false, prefix: p, path: Vec::new(), origin: 0, communities: Vec::new() });
    }
    for a in d["announcements"].as_array().into_iter().flatten() {
        for p in a["prefixes"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).filter_map(Prefix::parse) {
            out.push(Update { ts, us, peer_as, peer_ip: peer_ip.clone(), announce: true, prefix: p, path: path.clone(), origin, communities: communities.clone() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_do_not_collide_across_collectors_and_files() {
        let a = key("rrc00", 1794876600, 5);
        let b = key("rrc01", 1794876600, 5);
        let c = key("rrc00", 1794876900, 5);
        assert!(a != b && a != c && b != c);
        assert!(a > 0 && key("rrc26", 1894876600, (1 << 26) - 1) > 0);
    }

    #[test]
    fn reads_a_live_message() {
        let m: Value = serde_json::from_str(r#"{"timestamp":1794883477.710,"peer":"45.134.89.1","peer_asn":"34927","host":"rrc00.ripe.net","type":"UPDATE","path":[34927,6517,7087],"community":[[1299,4000]],"announcements":[{"next_hop":"45.134.89.1","prefixes":["200.23.137.0/24"]}],"withdrawals":["10.0.0.0/8"]}"#).unwrap();
        let u = from_live(&m);
        assert_eq!(u.len(), 2);
        assert!(!u[0].announce && u[1].announce);
        assert_eq!(u[1].origin, 7087);
        assert_eq!(u[1].peer_as, 34927);
        assert_eq!(u[1].communities, vec![(1299, 4000)]);
    }
}
