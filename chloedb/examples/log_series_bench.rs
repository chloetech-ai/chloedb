//! Log tables over time: how segments are laid out in time as logs stream
//! in, what merges and retention cost, and how fast time-window counts,
//! buckets and percentiles answer. Results in `results/log-series/`.
//!
//! ```text
//! log_series_bench gen OUT.jsonl LINES DAYS [SEED]
//! log_series_bench stream DB IN.jsonl SECONDS RETAIN_SECONDS [OPTIONS]
//! log_series_bench load DB IN.jsonl [OPTIONS]
//! log_series_bench query DB [RUNS]
//! ```
//!
//! IN may be `gen:LINES:DAYS`, the records `gen` makes, made as they are
//! read. `gen` writes LINES synthetic log records spread over DAYS days ending
//! now, in arrival order: a daily cycle in the rate, 500 hosts, 20
//! services, five severities, a latency in microseconds with a long tail,
//! and 2% of lines arriving up to 10 minutes late. Records are the JSON
//! lines `results/log-bytes/parse.py` makes (ts, host, service, severity,
//! pid, body) plus `latency`.
//!
//! `stream` writes IN's records for SECONDS of wall time, in batches of
//! 5,000 as OTLP requests come, each stamped with the time it is written
//! (late lines a little before it), into a table with `retain` of
//! RETAIN_SECONDS (0: none): a run of retention compressed in time. After each
//! batch it flushes a full memtable and runs the merges the default
//! policy selects, as the background merger would, and prints one JSON
//! line: rows and bytes written by flushes and by merges, rows expired,
//! rows past the cutoff still on disk, and how much the segments overlap
//! in time.
//!
//! `load` writes IN's records the same way with their own times, moved so
//! the newest is now, without retention, and leaves the segments as the
//! merges left them. `query` then runs the query set on it, each query
//! RUNS times (default 7), and prints one JSON line a query: median and
//! 95th percentile milliseconds, the rows it returned, and how many
//! segments its time window overlaps out of all of them.
//!
//! `OPTIONS` are the table's (`cluster_by = 'ts'`, `compression = 'zstd'`).

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use chloedb::db::{Db, WriteOp};
use chloedb::query::Params;
use chloedb::storage::merge::{MergePolicy, MergeStats};
use chloedb::storage::DbOptions;
use chloedb::{Document, Value};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const BATCH: usize = 5_000;
const COLUMNS: &str = "ts TIMESTAMP NOT NULL, host KEYWORD, service KEYWORD, severity KEYWORD, pid INT, latency INT, body TEXT";
const HOUR: i64 = 3_600_000_000;
const DAY: i64 = 24 * HOUR;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen") => gen(Path::new(&args[2]), args[3].parse().unwrap(), args[4].parse().unwrap(), args.get(5).map_or(7, |s| s.parse().unwrap())),
        Some("stream") => stream(Path::new(&args[2]), Path::new(&args[3]), args[4].parse().unwrap(), args[5].parse().unwrap(), args.get(6).map_or("", String::as_str)),
        Some("load") => load(Path::new(&args[2]), Path::new(&args[3]), args.get(4).map_or("", String::as_str)),
        Some("query") => query(Path::new(&args[2]), args.get(3).map_or(7, |s| s.parse().unwrap())),
        _ => {
            eprintln!("usage: log_series_bench gen|stream|load|query ...");
            std::process::exit(2);
        }
    }
}

fn gen(out: &Path, lines: u64, days: i64, seed: u64) {
    let mut w = BufWriter::new(std::fs::File::create(out).unwrap());
    for r in generated(lines, days, seed) {
        writeln!(w, "{r}").unwrap();
    }
}

/// The records `gen` writes, made as they are read.
fn generated(lines: u64, days: i64, seed: u64) -> impl Iterator<Item = serde_json::Value> {
    let mut rng = StdRng::seed_from_u64(seed);
    let end = chloedb::timestamp::now();
    let start = end - days * DAY;
    let span = (end - start) as f64;
    let services: Vec<String> = (0..20).map(|i| format!("svc-{i:02}")).collect();
    let severities = [("DEBUG", 0.30), ("INFO", 0.55), ("WARN", 0.10), ("ERROR", 0.045), ("FATAL", 0.005)];
    let words = ["request", "served", "timeout", "connection", "reset", "retry", "upstream", "cache", "miss", "hit", "user", "session", "error", "failed", "ok", "slow", "query", "disk", "queue", "full"];
    let tau = 2.0 * std::f64::consts::PI;
    (0..lines).map(move |i| {
        // Arrival order, the rate half again as high at the day's peak as
        // on average and half as high at its trough, and 2% of lines late
        // by up to 10 minutes.
        let u = (i as f64 + 0.5) / lines as f64;
        let cycle = u - 0.5 * (tau * u * days as f64).sin() / (tau * days as f64);
        let mut ts = start + (cycle.clamp(0.0, 1.0) * span) as i64;
        if rng.gen_bool(0.02) {
            ts -= rng.gen_range(0..600_000_000);
        }
        let host = format!("host-{:03}", (rng.gen::<f64>().powi(2) * 500.0) as u32);
        let service = &services[(rng.gen::<f64>().powi(2) * 20.0) as usize];
        let mut x = rng.gen::<f64>();
        let severity = severities.iter().find(|(_, p)| {
            x -= p;
            x < 0.0
        });
        let severity = severity.map_or("INFO", |s| s.0);
        // Log-normal latency, 2 ms median, a long tail.
        let z: f64 = rng.sample(rand_distr::StandardNormal);
        let latency = (2_000.0 * (1.2 * z).exp()) as i64;
        let n = rng.gen_range(4..12);
        let body: Vec<&str> = (0..n).map(|_| words[(rng.gen::<f64>().powi(2) * words.len() as f64) as usize]).collect();
        serde_json::json!({"ts": ts, "host": host, "service": service, "severity": severity, "pid": rng.gen_range(1000..5000), "latency": latency, "body": body.join(" ")})
    })
}

/// Records of a JSON lines file, or of `gen:LINES:DAYS` generated as they
/// are read, as documents of the table's columns.
fn records(input: &Path) -> Box<dyn Iterator<Item = (i64, Document)>> {
    let doc = |r: serde_json::Value| {
        let ts = r["ts"].as_i64().unwrap();
        let s = |k: &str| r[k].as_str().map_or(Value::Null, |v| Value::Str(v.to_string()));
        let i = |k: &str| r[k].as_i64().map_or(Value::Null, Value::Int);
        (ts, vec![Value::Timestamp(ts), s("host"), s("service"), s("severity"), i("pid"), i("latency"), s("body")])
    };
    let name = input.to_string_lossy();
    if let Some(spec) = name.strip_prefix("gen:") {
        let (lines, days) = spec.split_once(':').expect("gen:LINES:DAYS");
        return Box::new(generated(lines.parse().unwrap(), days.parse().unwrap(), 7).map(doc));
    }
    Box::new(BufReader::new(std::fs::File::open(input).unwrap()).lines().map(move |line| doc(serde_json::from_str(&line.unwrap()).unwrap())))
}

fn create(db_path: &Path, options: &str) -> Db {
    let _ = std::fs::remove_dir_all(db_path);
    let _ = std::fs::remove_file(db_path);
    let db = Db::create_empty(db_path, DbOptions::default()).unwrap();
    let with = if options.is_empty() { String::new() } else { format!(" OPTIONS({options})") };
    db.sql(&format!("CREATE TABLE logs ({COLUMNS}){with}"), &Params::new()).unwrap();
    db
}

/// Sum of segment image bytes of table `logs`.
fn segment_bytes(db: &Db) -> u64 {
    let snap = db.storage().snapshot().table("logs").unwrap();
    snap.segments().iter().filter(|s| !s.in_memory).map(|s| s.file.image().len() as u64).sum()
}

#[derive(Default)]
struct Totals {
    merges: MergeStats,
    flushed_rows: u64,
    flushed_bytes: u64,
}

impl Totals {
    fn add(&mut self, m: MergeStats) {
        self.merges.merges += m.merges;
        self.merges.segments_in += m.segments_in;
        self.merges.docs_merged += m.docs_merged;
        self.merges.docs_expired += m.docs_expired;
        self.merges.bytes_written += m.bytes_written;
        self.merges.seconds += m.seconds;
    }
}

/// Writes one batch, flushes when the memtable is full, and merges as the
/// default policy selects.
fn write_batch(db: &Db, batch: Vec<Document>, t: &mut Totals) {
    let n = batch.len() as u64;
    db.write_ops(batch.into_iter().map(|doc| WriteOp::Insert { table: "logs".into(), doc }).collect()).unwrap();
    t.flushed_rows += n;
    if db.storage().file_stats().memtable_docs >= DbOptions::default().memtable_max_docs {
        let before = segment_bytes(db);
        db.storage().flush().unwrap();
        t.flushed_bytes += segment_bytes(db).saturating_sub(before);
        t.add(db.storage().merge(&MergePolicy::default()).unwrap());
    }
}

/// Each segment's least and greatest `ts`, and its live rows.
fn layout(db: &Db) -> Vec<(i64, i64, u32)> {
    let snap = db.storage().snapshot().table("logs").unwrap();
    let key = snap.schema().key_of("ts").unwrap();
    snap.segments()
        .iter()
        .filter(|s| !s.in_memory && s.file.n_docs() > 0)
        .filter_map(|s| {
            let c = chloedb::forward::NumericColumn::open(&s.file, &key).ok()?;
            Some((c.min()? as i64, c.max()? as i64, s.n_live))
        })
        .collect()
}

/// How much the segments overlap in time: the sum of their spans over the
/// span of all of them (1 when they tile it, larger when they overlap).
fn overlap(segs: &[(i64, i64, u32)]) -> f64 {
    let (lo, hi) = segs.iter().fold((i64::MAX, i64::MIN), |(a, b), s| (a.min(s.0), b.max(s.1)));
    if hi <= lo {
        return 0.0;
    }
    segs.iter().map(|s| (s.1 - s.0) as f64).sum::<f64>() / (hi - lo) as f64
}

fn count(db: &Db, sql: &str) -> i64 {
    match db.query(sql, &Params::new()).unwrap().rows.first().and_then(|r| r.first()) {
        Some(Value::Int(n)) => *n,
        _ => 0,
    }
}

fn round(x: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    (x * p).round() / p
}

fn stream(db_path: &Path, input: &Path, seconds: f64, retain: i64, options: &str) {
    // RETAIN_SECONDS 0: no retention, for the cost of merges alone.
    let opts = match (options.is_empty(), retain > 0) {
        (_, false) => options.to_string(),
        (true, true) => format!("retain = '{retain} seconds'"),
        (false, true) => format!("{options}, retain = '{retain} seconds'"),
    };
    let db = create(db_path, &opts);
    let mut t = Totals::default();
    let start = Instant::now();
    let mut rng = StdRng::seed_from_u64(11);
    let mut lines = 0u64;
    let mut ingested_body = 0u64;
    'outer: loop {
        let mut it = records(input).peekable();
        if it.peek().is_none() {
            break;
        }
        let mut batch = Vec::with_capacity(BATCH);
        for (_, mut doc) in it {
            let now = chloedb::timestamp::now();
            // 2% late by up to 10% of the retention.
            let late = if rng.gen_bool(0.02) { rng.gen_range(0..(retain.max(60) * 100_000)) } else { 0 };
            doc[0] = Value::Timestamp(now - late);
            if let Value::Str(b) = &doc[6] {
                ingested_body += b.len() as u64;
            }
            batch.push(doc);
            lines += 1;
            if batch.len() == BATCH {
                write_batch(&db, std::mem::take(&mut batch), &mut t);
                if start.elapsed().as_secs_f64() >= seconds {
                    break 'outer;
                }
            }
        }
        if !batch.is_empty() {
            write_batch(&db, batch, &mut t);
        }
        if start.elapsed().as_secs_f64() >= seconds {
            break;
        }
    }
    let wall = start.elapsed().as_secs_f64();
    let cutoff = if retain > 0 { chloedb::timestamp::now() - retain * 1_000_000 } else { i64::MIN };
    let segs = layout(&db);
    let on_disk_past = if retain == 0 { 0 } else { count(&db, &format!("SELECT COUNT(*) FROM logs WHERE ts < '{}'", chloedb::timestamp::format(cutoff))) };
    let on_disk = segs.iter().map(|s| s.2 as u64).sum::<u64>();
    let segs_past = segs.iter().filter(|s| s.1 < cutoff).count();
    let out = serde_json::json!({
        "mode": "stream", "input": input.file_name().unwrap().to_string_lossy(), "options": opts,
        "seconds": round(wall, 1), "retain_seconds": retain, "lines": lines,
        "lines_per_second": round(lines as f64 / wall, 0),
        "body_bytes": ingested_body,
        "flushed_bytes": t.flushed_bytes, "merges": t.merges.merges, "merge_inputs": t.merges.segments_in,
        "merge_rows_rewritten": t.merges.docs_merged, "merge_bytes_written": t.merges.bytes_written,
        "merge_seconds": round(t.merges.seconds, 1),
        "write_amplification": round((t.flushed_bytes + t.merges.bytes_written) as f64 / t.flushed_bytes.max(1) as f64, 2),
        "rows_rewritten_per_row": round(t.merges.docs_merged as f64 / lines.max(1) as f64, 2),
        "rows_expired_by_merges": t.merges.docs_expired,
        "rows_on_disk": on_disk, "rows_past_cutoff_on_disk": on_disk_past,
        "segments": segs.len(), "segments_wholly_past_cutoff": segs_past,
        "segment_overlap": round(overlap(&segs), 2),
        "segment_bytes": segment_bytes(&db),
        "commit": commit(),
    });
    println!("{out}");
}

fn load(db_path: &Path, input: &Path, options: &str) {
    let db = create(db_path, options);
    let newest = records(input).map(|(ts, _)| ts).max().unwrap();
    let shift = chloedb::timestamp::now() - newest;
    let mut t = Totals::default();
    let start = Instant::now();
    let mut lines = 0u64;
    let mut batch = Vec::with_capacity(BATCH);
    for (ts, mut doc) in records(input) {
        doc[0] = Value::Timestamp(ts + shift);
        batch.push(doc);
        lines += 1;
        if batch.len() == BATCH {
            write_batch(&db, std::mem::take(&mut batch), &mut t);
        }
    }
    if !batch.is_empty() {
        write_batch(&db, batch, &mut t);
    }
    db.storage().flush().unwrap();
    t.add(db.storage().merge(&MergePolicy::default()).unwrap());
    let wall = start.elapsed().as_secs_f64();
    let segs = layout(&db);
    let out = serde_json::json!({
        "mode": "load", "input": input.file_name().unwrap().to_string_lossy(), "options": options,
        "seconds": round(wall, 1), "lines": lines, "lines_per_second": round(lines as f64 / wall, 0),
        "flushed_bytes": t.flushed_bytes, "merges": t.merges.merges,
        "merge_rows_rewritten": t.merges.docs_merged, "merge_bytes_written": t.merges.bytes_written,
        "write_amplification": round((t.flushed_bytes + t.merges.bytes_written) as f64 / t.flushed_bytes.max(1) as f64, 2),
        "segments": segs.len(), "segment_overlap": round(overlap(&segs), 2), "segment_bytes": segment_bytes(&db),
        "commit": commit(),
    });
    println!("{out}");
    db.close().unwrap();
}

/// The query set, over windows ending at the newest row.
fn queries(newest: i64, has_latency: bool) -> Vec<(&'static str, String, i64)> {
    let last = |w: i64| format!("ts >= '{}'", chloedb::timestamp::format(newest - w));
    let mut q = vec![
        ("count_last_hour", format!("SELECT COUNT(*) FROM logs WHERE {}", last(HOUR)), HOUR),
        ("count_last_day", format!("SELECT COUNT(*) FROM logs WHERE {}", last(DAY)), DAY),
        ("count_all", "SELECT COUNT(*) FROM logs".to_string(), i64::MAX),
        ("hourly_7d", format!("SELECT date_trunc('hour', ts) AS h, COUNT(*) FROM logs WHERE {} GROUP BY h", last(7 * DAY)), 7 * DAY),
        ("minutely_by_severity_24h", format!("SELECT date_trunc('minute', ts) AS m, severity, COUNT(*) FROM logs WHERE {} GROUP BY m, severity", last(DAY)), DAY),
        ("errors_top_hosts_24h", format!("SELECT host, COUNT(*) AS n FROM logs WHERE {} AND severity = 'ERROR' GROUP BY host ORDER BY n DESC LIMIT 10", last(DAY)), DAY),
        ("match_in_hour_newest", format!("SELECT ts, host, body FROM logs WHERE MATCH(body, 'error') AND {} ORDER BY ts DESC LIMIT 100", last(HOUR)), HOUR),
        // 5-minute buckets: date_bin from the columns, and the same buckets
        // as a version without it computes them (what OpenSearch's
        // fixed_interval ran before), row by row through a WITH.
        ("bins_5m_by_severity_24h", format!("SELECT date_bin('5 minutes', ts) AS b, severity, COUNT(*) FROM logs WHERE {} GROUP BY b, severity", last(DAY)), DAY),
        (
            "bins_5m_by_severity_24h_rows",
            format!(
                "WITH r AS (SELECT (EXTRACT(epoch FROM ts) * 1000000) / 300000000 AS y, severity FROM logs WHERE {}), s AS (SELECT (y - (y % 1.0)) - CASE WHEN y % 1.0 < 0 THEN 1.0 ELSE 0.0 END AS b, severity FROM r) SELECT b, severity, COUNT(*) FROM s GROUP BY b, severity",
                last(DAY)
            ),
            DAY,
        ),
    ];
    if has_latency {
        q.push(("p99_hourly_by_service_24h", format!("SELECT date_trunc('hour', ts) AS h, service, PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency) FROM logs WHERE {} GROUP BY h, service", last(DAY)), DAY));
        q.push(("p50_p95_p99_24h", format!("SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY latency), PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency), PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency) FROM logs WHERE {}", last(DAY)), DAY));
        // Exact and approximate percentiles side by side: the exact ones as
        // PERCENTILE_DISC, one of the values, which the sketch's answers
        // are within 1% of (`accuracy` lines).
        for (p, tag) in [(0.5, "p50"), (0.99, "p99")] {
            let exact = format!("SELECT date_bin('5 minutes', ts) AS b, service, PERCENTILE_DISC({p}) WITHIN GROUP (ORDER BY latency) FROM logs WHERE {} GROUP BY b, service", last(DAY));
            let approx = format!("SELECT date_bin('5 minutes', ts) AS b, service, approx_percentile(latency, {p}) FROM logs WHERE {} GROUP BY b, service", last(DAY));
            q.push((if tag == "p50" { "p50_5m_by_service_24h_exact" } else { "p99_5m_by_service_24h_exact" }, exact, DAY));
            q.push((if tag == "p50" { "p50_5m_by_service_24h_approx" } else { "p99_5m_by_service_24h_approx" }, approx, DAY));
        }
        q.push(("p50_p95_p99_24h_approx", format!("SELECT approx_percentile(latency, 0.5), approx_percentile(latency, 0.95), approx_percentile(latency, 0.99) FROM logs WHERE {}", last(DAY)), DAY));
        q.push(("p50_p95_p99_30d_exact", "SELECT PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY latency), PERCENTILE_DISC(0.95) WITHIN GROUP (ORDER BY latency), PERCENTILE_DISC(0.99) WITHIN GROUP (ORDER BY latency) FROM logs".to_string(), i64::MAX));
        q.push(("p50_p95_p99_30d_approx", "SELECT approx_percentile(latency, 0.5), approx_percentile(latency, 0.95), approx_percentile(latency, 0.99) FROM logs".to_string(), i64::MAX));
    }
    q
}

fn query(db_path: &Path, runs: usize) {
    let db = Db::open(db_path, DbOptions::default()).unwrap();
    let segs = layout(&db);
    let newest = segs.iter().map(|s| s.1).max().unwrap();
    let has_latency = count(&db, "SELECT COUNT(*) FROM logs WHERE latency IS NOT NULL") > 0;
    let lines = count(&db, "SELECT COUNT(*) FROM logs");
    // Each `_exact` query's answer, for its `_approx` twin.
    let mut exact: std::collections::HashMap<String, Vec<Vec<Value>>> = std::collections::HashMap::new();
    for (name, sql, window) in queries(newest, has_latency) {
        let mut ms = Vec::with_capacity(runs);
        let mut rows = 0;
        let mut answer = Vec::new();
        for _ in 0..runs {
            let t = Instant::now();
            let r = db.query(&sql, &Params::new()).unwrap();
            ms.push(t.elapsed().as_secs_f64() * 1e3);
            rows = r.rows.len();
            answer = r.rows;
        }
        if let Some(base) = name.strip_suffix("_exact") {
            exact.insert(base.to_string(), answer);
        } else if let Some(want) = name.strip_suffix("_approx").and_then(|base| exact.get(base)) {
            println!("{}", accuracy(name, want, &answer));
        }
        ms.sort_by(f64::total_cmp);
        let from = newest.saturating_sub(window);
        let overlapping = segs.iter().filter(|s| s.1 >= from).count();
        let out = serde_json::json!({
            "mode": "query", "db": db_path.file_name().unwrap().to_string_lossy(), "lines": lines, "query": name, "sql": sql,
            "runs": runs, "ms_median": round(ms[runs / 2], 2), "ms_p95": round(ms[((runs as f64 * 0.95).ceil() as usize).min(runs) - 1], 2),
            "rows": rows, "segments": segs.len(), "segments_in_window": overlapping,
            "commit": commit(),
        });
        println!("{out}");
    }
}

/// How far an `_approx` query's percentiles are from its `_exact` twin's,
/// group by group (the rows' leading columns that are not numbers of a
/// percentile): the greatest and the mean relative error.
fn accuracy(name: &str, exact: &[Vec<Value>], approx: &[Vec<Value>]) -> serde_json::Value {
    let num = |v: &Value| match v {
        Value::Int(x) => Some(*x as f64),
        Value::Float(x) => Some(*x),
        _ => None,
    };
    // Groups are every column but the trailing percentiles, as many as the
    // query computes: the columns that differ between the two answers.
    let width = exact.first().map_or(0, Vec::len);
    let keys = width.saturating_sub(if name.starts_with("p50_p95_p99") { 3 } else { 1 });
    let by: std::collections::HashMap<String, &Vec<Value>> = exact.iter().map(|r| (format!("{:?}", &r[..keys]), r)).collect();
    let (mut worst, mut sum, mut n, mut missing) = (0f64, 0f64, 0u64, 0u64);
    for r in approx {
        let Some(want) = by.get(&format!("{:?}", &r[..keys])) else {
            missing += 1;
            continue;
        };
        for (g, w) in r[keys..].iter().zip(&want[keys..]) {
            if let (Some(g), Some(w)) = (num(g), num(w)) {
                let e = if w == 0.0 { (g - w).abs() } else { ((g - w) / w).abs() };
                worst = worst.max(e);
                sum += e;
                n += 1;
            }
        }
    }
    serde_json::json!({
        "mode": "accuracy", "query": name, "groups": approx.len(), "values": n, "groups_missing": missing,
        "max_relative_error": round(worst, 5), "mean_relative_error": round(sum / n.max(1) as f64, 5), "commit": commit(),
    })
}

fn commit() -> String {
    std::process::Command::new("git").args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "--short=8", "HEAD"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}
