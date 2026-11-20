//! Writes over HTTP to a server or a cluster: how many a second, at what
//! latency, from 1 to hundreds of clients, as rows one a request, batches,
//! SQL updates, counter adds and streams, through the coordinator or
//! straight to each shard's primary.
//!
//! ```text
//! cd chloedb-server
//! cargo run --release --bin chloedb-write-bench -- --url http://coordinator:7700 --setup \
//!     --workloads insert,batch:100,add,sql-update,stream:100 --clients 1,4,16,64,256 --seconds 20 --out writes.jsonl
//! ```
//!
//! Every client is a task on this process's runtime, with a connection of
//! its own, sending its next request once its last one is answered (a
//! closed loop), for `--seconds` after a warm-up of a second. The
//! workloads, on table `--table` (`wb`: an id, a text of 20 words, a
//! counter and a price):
//!
//! - `insert`: `POST /v1/write` of one new row.
//! - `batch:N`: `POST /v1/write` of N new rows (one transaction a shard).
//! - `add`: `POST /v1/write` adding 1 to the counter of one of `--keys`
//!   rows, at random.
//! - `hot-add`: the same, every client on one of 16 rows.
//! - `sql-update`: `POST /v1/sql` `UPDATE wb SET price = :p WHERE id = :k`
//!   of one of the rows, at random.
//! - `sql-add`: `UPDATE wb SET n = n + 1 WHERE id = :k`, a row at random;
//!   `sql-hot-add` the same, every client on one of 16 rows.
//! - `stream:N`: one `POST /v1/stream` a client for the whole run, N new
//!   rows a chunk of its body, counted as the server answers each batch.
//!
//! With `--direct`, the client reads the cluster's route
//! (`GET /v1/cluster/route` on the coordinator: its slot map and each
//! shard's primary) and sends each row to its shard's primary itself, a
//! batch split by shard, so no write goes through the coordinator; a
//! write a shard refuses as no longer its own (a slot moved, a primary
//! changed) reads the route again and goes again.
//!
//! `--setup` creates the table (dropped first) and loads `--keys` rows
//! for the updates. Each run appends a line of JSON to `--out`: the
//! workload, clients, rows a second, requests a second, latency
//! percentiles, errors, and the CPU seconds each server of the cluster
//! used during it (from the coordinator's `/v1/cluster`), with
//! `--label` naming the setup (shards, acks).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use clap::Parser;
use futures::StreamExt;
use rand::{Rng, SeedableRng};
use serde_json::{json, Value as Json};
use chloedb::Value;
use chloedb_server::slots::SlotMap;

#[derive(Parser, Clone)]
struct Args {
    /// The server or coordinator.
    #[arg(long)]
    url: String,
    /// A token or API key (`$CHLOEDB_TOKEN`).
    #[arg(long, env = "CHLOEDB_TOKEN")]
    token: Option<String>,
    #[arg(long, default_value = "wb")]
    table: String,
    /// Comma-separated: insert, batch:N, add, hot-add, sql-update, sql-add, sql-hot-add, stream:N.
    #[arg(long, default_value = "insert,batch:100,add,sql-update,stream:100")]
    workloads: String,
    /// Comma-separated client counts.
    #[arg(long, default_value = "1,4,16,64,256")]
    clients: String,
    #[arg(long, default_value_t = 20.0)]
    seconds: f64,
    /// Rows loaded for the updates and adds.
    #[arg(long, default_value_t = 100_000)]
    keys: usize,
    /// Create the table and load `--keys` rows first.
    #[arg(long)]
    setup: bool,
    /// Send each row straight to its shard's primary.
    #[arg(long)]
    direct: bool,
    /// Appended to, a line a run.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Names the setup in each line (shards, acks, build).
    #[arg(long, default_value = "")]
    label: String,
}

/// Where a shard-aware client sends each row: the cluster's slot map and
/// each shard's primary, by HTTP URL.
struct Route {
    map: SlotMap,
    primaries: Vec<String>,
}

struct Ctx {
    args: Args,
    http: reqwest::Client,
    route: RwLock<Option<Arc<Route>>>,
    /// Rows inserted so far, for new keys.
    next: AtomicU64,
    run: u64,
}

impl Ctx {
    fn req(&self, url: &str) -> reqwest::RequestBuilder {
        let r = self.http.post(url);
        match &self.args.token {
            Some(t) => r.bearer_auth(t),
            None => r,
        }
    }

    async fn read_route(&self) -> Result<Arc<Route>, String> {
        let mut r = self.http.get(format!("{}/v1/cluster/route", self.args.url));
        if let Some(t) = &self.args.token {
            r = r.bearer_auth(t);
        }
        let a = r.send().await.map_err(|e| e.to_string())?;
        let status = a.status();
        let j: Json = a.json().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("route: {status} {j}"));
        }
        let map = SlotMap::from_json(&j["map"])?;
        let primaries = j["shards"].as_array().ok_or("route: no shards")?.iter().map(|s| s["primary_http"].as_str().unwrap_or_default().to_string()).collect();
        let route = Arc::new(Route { map, primaries });
        *self.route.write().unwrap() = Some(route.clone());
        Ok(route)
    }

    fn route(&self) -> Option<Arc<Route>> {
        self.route.read().unwrap().clone()
    }

    /// Sends `ops` (each with its key) to `/v1/write`: to the coordinator,
    /// or split by shard to the primaries, at once. The rows written.
    async fn write(&self, ops: Vec<(String, Json)>) -> Result<u64, String> {
        if !self.args.direct {
            let body = json!({"ops": ops.into_iter().map(|(_, o)| o).collect::<Vec<_>>()});
            return self.post(&format!("{}/v1/write", self.args.url), &body).await;
        }
        for attempt in 0..5 {
            let route = match self.route() {
                Some(r) => r,
                None => self.read_route().await?,
            };
            let mut by_shard: BTreeMap<usize, Vec<Json>> = BTreeMap::new();
            for (k, o) in &ops {
                by_shard.entry(route.map.shard_of(&Value::Str(k.clone()))).or_default().push(o.clone());
            }
            let calls = by_shard.into_iter().map(|(s, ops)| {
                let url = format!("{}/v1/write", route.primaries[s]);
                async move { self.post(&url, &json!({"ops": ops})).await }
            });
            let answers = futures::future::join_all(calls).await;
            // A shard that no longer holds a row, or no longer takes
            // writes: the route is read again. (A real client sends only
            // the parts that failed, with idempotency keys.)
            if let Some(e) = answers.iter().find_map(|a| a.as_ref().err()) {
                if attempt < 4 && (e.starts_with("400") || e.starts_with("409") || e.starts_with("503") || e.starts_with("send")) {
                    let _ = self.read_route().await;
                    continue;
                }
                return Err(e.clone());
            }
            return Ok(answers.into_iter().map(|a| a.unwrap_or(0)).sum());
        }
        unreachable!()
    }

    async fn post(&self, url: &str, body: &Json) -> Result<u64, String> {
        let a = self.req(url).json(body).send().await.map_err(|e| format!("send: {e}"))?;
        let status = a.status();
        let j: Json = a.json().await.map_err(|e| format!("{} read: {e}", status.as_u16()))?;
        if !status.is_success() {
            return Err(format!("{} {}", status.as_u16(), j["error"].as_str().unwrap_or(&j.to_string())));
        }
        Ok(j["written"].as_u64().unwrap_or(0))
    }

    fn new_row(&self, rng: &mut impl Rng) -> (String, Json) {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let id = format!("r{}-{n}", self.run);
        let body: Vec<String> = (0..20).map(|_| format!("w{}", rng.gen_range(0..5000))).collect();
        let doc = json!({"id": id, "body": body.join(" "), "n": 0, "price": rng.gen_range(1.0..100.0)});
        (id, json!({"op": "insert", "table": self.args.table, "doc": doc}))
    }
}

#[derive(Default)]
struct Tally {
    lat_us: Vec<u64>,
    rows: u64,
    requests: u64,
    errors: u64,
    first_error: Option<String>,
}

async fn client(ctx: Arc<Ctx>, workload: String, id: usize, measuring: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Tally {
    let mut rng = rand::rngs::StdRng::seed_from_u64(id as u64 ^ ctx.run.rotate_left(17));
    let mut t = Tally::default();
    if let Some(n) = workload.strip_prefix("stream:") {
        return stream(ctx.clone(), n.parse().unwrap_or(100), &mut rng, measuring, stop).await;
    }
    while !stop.load(Ordering::Relaxed) {
        let t0 = Instant::now();
        let a = match workload.as_str() {
            "insert" => ctx.write(vec![ctx.new_row(&mut rng)]).await,
            w if w.starts_with("batch:") => {
                let n: usize = w[6..].parse().unwrap_or(100);
                ctx.write((0..n).map(|_| ctx.new_row(&mut rng)).collect()).await
            }
            "add" | "hot-add" => {
                let k = format!("k{}", rng.gen_range(0..if workload == "add" { ctx.args.keys } else { 16 }));
                let op = json!({"op": "add", "table": ctx.args.table, "key": k, "field": "n", "value": 1});
                ctx.write(vec![(k, op)]).await
            }
            "sql-update" | "sql-add" | "sql-hot-add" => {
                let k = match workload.as_str() {
                    "sql-hot-add" => format!("k{}", rng.gen_range(0..16)),
                    _ => format!("k{}", rng.gen_range(0..ctx.args.keys)),
                };
                let body = match workload.as_str() {
                    "sql-update" => json!({"sql": format!("UPDATE {} SET price = :p WHERE id = :k", ctx.args.table), "params": {"p": rng.gen_range(1.0..100.0), "k": k}}),
                    _ => json!({"sql": format!("UPDATE {} SET n = n + 1 WHERE id = :k", ctx.args.table), "params": {"k": k}}),
                };
                let url = match (ctx.args.direct, ctx.route()) {
                    (true, Some(r)) => r.primaries[r.map.shard_of(&Value::Str(k))].clone(),
                    _ => ctx.args.url.clone(),
                };
                ctx.post(&format!("{url}/v1/sql"), &body).await
            }
            w => panic!("no workload {w}"),
        };
        if !measuring.load(Ordering::Relaxed) {
            continue;
        }
        t.requests += 1;
        t.lat_us.push(t0.elapsed().as_micros() as u64);
        match a {
            Ok(n) => t.rows += n,
            Err(e) => {
                t.errors += 1;
                t.first_error.get_or_insert(e);
            }
        }
    }
    t
}

/// One long `/v1/stream` of new rows, `n` a chunk, until `stop`: rows
/// counted from the batches the server answers while `measuring`.
async fn stream(ctx: Arc<Ctx>, n: usize, rng: &mut rand::rngs::StdRng, measuring: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Tally {
    let mut t = Tally::default();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(4);
    let rows: Vec<Vec<u8>> = (0..64)
        .map(|_| {
            let mut chunk = Vec::new();
            for _ in 0..n {
                let (_, op) = ctx.new_row(rng);
                chunk.extend(serde_json::to_vec(&op).unwrap());
                chunk.push(b'\n');
            }
            chunk
        })
        .collect();
    let gen_ctx = ctx.clone();
    let gen_stop = stop.clone();
    let mut seed = rng.gen::<u64>();
    tokio::spawn(async move {
        let mut i = 0usize;
        while !gen_stop.load(Ordering::Relaxed) {
            // New keys each chunk: the template's ids, renumbered.
            let base = gen_ctx.next.fetch_add(n as u64, Ordering::Relaxed);
            let text = String::from_utf8_lossy(&rows[i % rows.len()]).replace("\"id\":\"r", &format!("\"id\":\"s{seed}-{base}-"));
            seed = seed.wrapping_add(1);
            i += 1;
            if tx.send(Ok(text.into_bytes())).await.is_err() {
                break;
            }
        }
    });
    let body = reqwest::Body::wrap_stream(tokio_stream::wrappers::ReceiverStream::new(rx));
    let a = match ctx.req(&format!("{}/v1/stream", ctx.args.url)).header("content-type", "application/x-ndjson").body(body).send().await {
        Ok(a) => a,
        Err(e) => {
            t.errors += 1;
            t.first_error = Some(e.to_string());
            return t;
        }
    };
    let mut lines = a.bytes_stream();
    let mut buf = Vec::new();
    let mut last = Instant::now();
    while let Some(chunk) = lines.next().await {
        let Ok(chunk) = chunk else { break };
        buf.extend_from_slice(&chunk);
        while let Some(p) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=p).collect();
            let Ok(j) = serde_json::from_slice::<Json>(&line) else { continue };
            if j.get("error").is_some() {
                t.errors += 1;
                t.first_error.get_or_insert(j.to_string());
                continue;
            }
            if j.get("batch").is_some() && measuring.load(Ordering::Relaxed) {
                t.rows += j["written"].as_u64().unwrap_or(0);
                t.requests += 1;
                t.lat_us.push(last.elapsed().as_micros() as u64);
            }
            last = Instant::now();
        }
    }
    t
}

fn pct(v: &[u64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v[((v.len() - 1) as f64 * p).round() as usize] as f64 / 1e3
}

/// Each server's CPU seconds, by its URL, from the coordinator's view
/// (or the server's own health, on one server).
async fn cpu(ctx: &Ctx) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    let mut r = ctx.http.get(format!("{}/v1/cluster", ctx.args.url));
    if let Some(t) = &ctx.args.token {
        r = r.bearer_auth(t);
    }
    let Ok(a) = r.send().await else { return out };
    let Ok(j) = a.json::<Json>().await else { return out };
    for s in j["shards"].as_array().into_iter().flatten() {
        for c in s["replicas"].as_array().into_iter().flatten() {
            if let Some(x) = c["detail"]["system"]["cpu_seconds"].as_f64() {
                out.insert(format!("{} {}", c["url"].as_str().unwrap_or(""), c["role"].as_str().unwrap_or("")), x);
            }
        }
    }
    // The coordinator's own, from its metrics.
    let mut r = ctx.http.get(format!("{}/metrics", ctx.args.url));
    if let Some(t) = &ctx.args.token {
        r = r.bearer_auth(t);
    }
    if let Ok(a) = r.send().await {
        if let Ok(text) = a.text().await {
            if let Some(x) = text.lines().find_map(|l| l.strip_prefix("process_cpu_seconds_total ")).and_then(|v| v.trim().parse::<f64>().ok()) {
                out.insert("coordinator".into(), x);
            }
        }
    }
    out
}

async fn setup(ctx: &Ctx) -> Result<(), String> {
    let sql = |s: String| json!({"sql": s});
    // A cluster just started may still be electing primaries: wait up to a minute.
    let t0 = Instant::now();
    while let Err(e) = ctx.post(&format!("{}/v1/sql", ctx.args.url), &sql("SELECT 1".into())).await {
        if t0.elapsed() > Duration::from_secs(60) {
            return Err(format!("not ready after 60 s: {e}"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let _ = ctx.post(&format!("{}/v1/sql", ctx.args.url), &sql(format!("DROP TABLE IF EXISTS {}", ctx.args.table))).await;
    ctx.post(&format!("{}/v1/sql", ctx.args.url), &sql(format!("CREATE TABLE {} (id KEYWORD PRIMARY KEY, body TEXT, n INT, price FLOAT)", ctx.args.table))).await?;
    let t0 = Instant::now();
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    for start in (0..ctx.args.keys).step_by(2000) {
        let ops: Vec<Json> = (start..(start + 2000).min(ctx.args.keys))
            .map(|i| {
                let body: Vec<String> = (0..20).map(|_| format!("w{}", rng.gen_range(0..5000))).collect();
                json!({"op": "insert", "table": ctx.args.table, "doc": {"id": format!("k{i}"), "body": body.join(" "), "n": 0, "price": 10.0}})
            })
            .collect();
        ctx.post(&format!("{}/v1/write", ctx.args.url), &json!({"ops": ops})).await?;
    }
    eprintln!("set up {} with {} rows in {:.1} s", ctx.args.table, ctx.args.keys, t0.elapsed().as_secs_f64());
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let http = reqwest::Client::builder().pool_max_idle_per_host(1024).timeout(Duration::from_secs(120)).tcp_nodelay(true).build().unwrap();
    let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64 % 1_000_000_000;
    let mut ctx = Ctx { args: args.clone(), http, route: RwLock::new(None), next: AtomicU64::new(0), run };
    if args.setup {
        if let Err(e) = setup(&ctx).await {
            eprintln!("setup failed: {e}");
            std::process::exit(1);
        }
    }
    if args.direct {
        match ctx.read_route().await {
            Ok(r) => eprintln!("route: {} shards, primaries {:?}", r.primaries.len(), r.primaries),
            Err(e) => {
                eprintln!("no route: {e}");
                std::process::exit(1);
            }
        }
    }
    let workloads: Vec<String> = args.workloads.split(',').map(str::to_string).collect();
    let clients: Vec<usize> = args.clients.split(',').map(|c| c.parse().unwrap()).collect();
    for w in &workloads {
        for &c in &clients {
            ctx.run += 1;
            let ctx_arc = Arc::new(Ctx { args: ctx.args.clone(), http: ctx.http.clone(), route: RwLock::new(ctx.route()), next: AtomicU64::new(0), run: ctx.run * 1000 + c as u64 });
            let (measuring, stop) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
            let tasks: Vec<_> = (0..c).map(|i| tokio::spawn(client(ctx_arc.clone(), w.clone(), i, measuring.clone(), stop.clone()))).collect();
            tokio::time::sleep(Duration::from_secs(1)).await;
            let cpu0 = cpu(&ctx_arc).await;
            let t0 = Instant::now();
            measuring.store(true, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_secs_f64(args.seconds)).await;
            measuring.store(false, Ordering::Relaxed);
            let wall = t0.elapsed().as_secs_f64();
            let cpu1 = cpu(&ctx_arc).await;
            stop.store(true, Ordering::Relaxed);
            let mut all = Tally::default();
            for t in tasks {
                let t = t.await.unwrap();
                all.lat_us.extend(t.lat_us);
                all.rows += t.rows;
                all.requests += t.requests;
                all.errors += t.errors;
                if all.first_error.is_none() {
                    all.first_error = t.first_error;
                }
            }
            all.lat_us.sort_unstable();
            let used: BTreeMap<String, f64> = cpu1.iter().filter_map(|(k, v)| cpu0.get(k).map(|v0| (k.clone(), ((v - v0) / wall * 100.0).round() / 100.0))).collect();
            let line = json!({
                "label": args.label, "workload": w, "clients": c, "direct": args.direct, "seconds": wall,
                "rows_per_s": all.rows as f64 / wall, "requests_per_s": all.requests as f64 / wall,
                "p50_ms": pct(&all.lat_us, 0.5), "p90_ms": pct(&all.lat_us, 0.9), "p99_ms": pct(&all.lat_us, 0.99), "max_ms": pct(&all.lat_us, 1.0),
                "errors": all.errors, "first_error": all.first_error, "cores_used": used,
            });
            println!("{:12} {:>4} clients {:>10.0} rows/s {:>9.0} req/s  p50 {:>7.2} ms  p99 {:>8.2} ms  errors {}", w, c, all.rows as f64 / wall, all.requests as f64 / wall, pct(&all.lat_us, 0.5), pct(&all.lat_us, 0.99), all.errors);
            if let Some(e) = &all.first_error {
                println!("    first error: {e}");
            }
            if let Some(p) = &args.out {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
                writeln!(f, "{line}").unwrap();
            }
        }
    }
}
