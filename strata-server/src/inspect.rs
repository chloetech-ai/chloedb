//! The index inspector (`GET /v1/index`, the command center's Index
//! view): a table's statistics ([`strata::inspect`]), how fresh it is,
//! and how often its documents are retrieved and its keys asked for
//! ([`crate::reads`]), for this server, or on a coordinator for each
//! replica of each shard, each shard, and the whole cluster.
//!
//! A coordinator asks every replica for its own ([`node`]) over gRPC, as a
//! profile of kind `index` ([`crate::profiling::Kind::Index`]; no protocol
//! change: a server older than this one answers that it has no such
//! profile, and its replica shows that error). A shard's data is its
//! primary's (the first replica that answered, without one); its reads are
//! its replicas' added up: what each answered its own clients and what it
//! answered through this coordinator. The cluster's data is its shards'
//! added up, with the bytes of every copy beside them.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value as Json};
use strata::inspect::{Options, TableStats, MAX_SAMPLE};

use crate::reads::{At, TableReads};
use crate::service::{Backend, Service};
use crate::{telemetry, ApiError};

/// What the inspector is asked for.
#[derive(Clone, Debug)]
pub struct Asked {
    /// The table (the first without one).
    pub table: Option<String>,
    /// Documents each server samples.
    pub sample: usize,
    pub seed: u64,
    /// Sketch the distinct terms.
    pub terms: bool,
    /// The most read keys to list.
    pub top: usize,
}

impl Asked {
    /// From a JSON object of the route's parameters (strings or numbers):
    /// `table`, `sample` (2000), `seed`, `terms` (true), `top` (25).
    pub fn of(q: &Json) -> Result<Asked, ApiError> {
        let num = |k: &str, d: u64| -> Result<u64, ApiError> {
            match q.get(k) {
                None | Some(Json::Null) => Ok(d),
                Some(Json::String(s)) if s.is_empty() => Ok(d),
                Some(Json::String(s)) => s.parse().map_err(|_| ApiError::bad(format!("{k} is a whole number, not {s:?}"))),
                Some(v) => v.as_u64().ok_or_else(|| ApiError::bad(format!("{k} is a whole number, not {v}"))),
            }
        };
        let sample = num("sample", strata::inspect::SAMPLE as u64)?;
        if sample > MAX_SAMPLE as u64 {
            return Err(ApiError::bad(format!("sample is at most {MAX_SAMPLE} documents")));
        }
        let terms = match q.get("terms") {
            None | Some(Json::Null) => true,
            Some(Json::Bool(b)) => *b,
            Some(Json::String(s)) => !matches!(s.as_str(), "false" | "0" | "off" | "no"),
            Some(v) => return Err(ApiError::bad(format!("terms is true or false, not {v}"))),
        };
        Ok(Asked {
            table: q.get("table").and_then(|t| t.as_str()).filter(|t| !t.is_empty()).map(str::to_string),
            sample: sample as usize,
            seed: num("seed", Options::default().seed)?,
            terms,
            top: num("top", 25)?.min(crate::reads::TOP as u64) as usize,
        })
    }

    /// As a profile request's body.
    pub fn body(&self) -> Json {
        json!({"table": self.table, "sample": self.sample, "seed": self.seed, "terms": self.terms})
    }
}

/// This server's statistics of the table, its freshness and the reads it
/// answered its own clients, with the distinct-term sketches and the read
/// summaries whole, for a coordinator to merge.
pub async fn node(svc: &Arc<Service>, a: &Asked) -> Result<Json, ApiError> {
    let Backend::Local(db) = &svc.backend else {
        return Err(ApiError::bad("a coordinator holds no table itself; its answer covers its shards"));
    };
    let schema = match &a.table {
        Some(t) => svc.table(t).await?,
        None => svc.first_table(),
    };
    let name = schema.table.clone();
    let opts = Options { sample: a.sample, seed: a.seed, terms: a.terms };
    let (db2, table) = (db.clone(), name.clone());
    let stats = tokio::task::spawn_blocking(move || {
        let snap = db2.snapshot();
        let view = snap.table(&table).ok_or_else(|| ApiError::bad(format!("no such table: {table}")))?;
        strata::inspect::table(&view, &opts).map_err(ApiError::from)
    })
    .await
    .map_err(|e| ApiError::internal(format!("the statistics failed: {e}")))??;
    let activity = db.storage().activity(&name).unwrap_or_default();
    let mut reads = TableReads::default();
    for (at, r) in svc.reads.of(&name) {
        if at == At::here() {
            reads.merge(&r);
        }
    }
    let started = svc.started.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    Ok(json!({
        "table": name,
        "stats": stats,
        "freshness": {
            "last_write_ms": (activity.last_ms > 0).then_some(activity.last_ms),
            "writes": activity.ops,
            "commits": activity.commits,
            "modified_ms": db.storage().modified_ms(),
            "started_ms": started,
        },
        "reads": reads,
        "reads_on": svc.reads.on(),
        "role": svc.role().as_str_name().to_lowercase(),
        "at_ms": telemetry::now_ms(),
    }))
}

/// One server's part of an answer.
struct Part {
    stats: Option<TableStats>,
    reads: TableReads,
    fresh: Fresh,
    /// Bytes of every copy.
    copies: u64,
}

/// When a table last changed.
#[derive(Clone, Copy, Default)]
struct Fresh {
    last_write_ms: Option<u64>,
    modified_ms: Option<u64>,
    writes: u64,
    started_ms: Option<u64>,
}

impl Fresh {
    fn of(j: &Json) -> Fresh {
        let f = &j["freshness"];
        Fresh { last_write_ms: f["last_write_ms"].as_u64(), modified_ms: f["modified_ms"].as_u64(), writes: f["writes"].as_u64().unwrap_or(0), started_ms: f["started_ms"].as_u64() }
    }

    /// The later of both; the writes `writes` says (a shard's are its
    /// primary's, the cluster's its shards' summed).
    fn latest(self, o: Fresh, writes: u64) -> Fresh {
        Fresh {
            last_write_ms: self.last_write_ms.max(o.last_write_ms),
            modified_ms: self.modified_ms.max(o.modified_ms),
            writes,
            started_ms: match (self.started_ms, o.started_ms) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }

    fn json(&self) -> Json {
        json!({"last_write_ms": self.last_write_ms, "modified_ms": self.modified_ms, "writes": self.writes, "started_ms": self.started_ms})
    }
}

impl Part {
    /// A server's answer ([`node`]), with the reads `extra` it served
    /// through this coordinator.
    fn of(j: &Json, extra: Option<&TableReads>) -> Result<Part, ApiError> {
        let stats: TableStats = serde_json::from_value(j["stats"].clone()).map_err(|e| ApiError::internal(format!("statistics that do not read: {e}")))?;
        let mut reads: TableReads = serde_json::from_value(j["reads"].clone()).unwrap_or_default();
        if let Some(x) = extra {
            reads.merge(x);
        }
        let copies = stats.bytes.total;
        Ok(Part { stats: Some(stats), reads, fresh: Fresh::of(j), copies })
    }

    /// For people: the statistics finished, the reads summarised.
    fn json(&self, top: usize) -> Json {
        let mut stats = self.stats.clone();
        if let Some(s) = &mut stats {
            s.finish(false);
        }
        let live = stats.as_ref().map(|s| s.docs.live);
        json!({
            "stats": stats,
            "freshness": self.fresh.json(),
            "reads": {"docs": self.reads.docs.summary(live, top), "keys": self.reads.keys.summary(live, top)},
            "copies_bytes": self.copies,
        })
    }
}

// ----- core: every table, cheaply (`GET /v1/index/tables`)

/// How long [`tables`] keeps its answer.
pub const TABLES_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// The window of [`tables`]' writes and reads a minute: 5 minutes.
pub const RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(300);

/// Each table's writes and reads counted at a look of [`tables`], by
/// table, and when.
pub type Counts = std::collections::VecDeque<(u64, HashMap<String, (u64, u64)>)>;

/// Every table's size and traffic: of this server, or on a coordinator of
/// each shard (its primary's data) and the cluster, with how skewed the
/// shards are, how far each copy is behind, and how much each table grew
/// over a day and a week (the log store's hourly `tables` rows). No
/// document is sampled; each table is one call to each copy. Kept
/// [`TABLES_EVERY`].
pub async fn tables(svc: &Arc<Service>) -> Result<Json, ApiError> {
    let mut kept = svc.tables_kept.lock().await;
    if let Some((at, j)) = kept.as_ref() {
        if at.elapsed() < TABLES_EVERY {
            return Ok(j.clone());
        }
    }
    let now = telemetry::now_ms();
    let names: Vec<String> = svc.catalog_now().await.iter().map(|s| s.table.clone()).collect();
    // On a coordinator, each copy as the cluster's snapshot knows it: its
    // short name, and since when it does not answer.
    let mut copies: HashMap<String, (Json, Json, bool)> = HashMap::new();
    if let Backend::Cluster(c) = &svc.backend {
        let snap = c.snapshot().await;
        for r in snap["shards"].as_array().into_iter().flatten().flat_map(|s| s["replicas"].as_array().into_iter().flatten()) {
            if let Some(u) = r["url"].as_str() {
                copies.insert(u.to_string(), (r["short"].clone(), r["down_since_ms"].clone(), r["status"] == "down"));
            }
        }
    }
    // Writes and reads a minute over the last minutes: each table's counts
    // now against those of the earliest look within [`RATE_WINDOW`], at
    // least 30 s ago.
    let then = {
        let mut counts = svc.tables_counts.lock().unwrap_or_else(|e| e.into_inner());
        counts.retain(|(at, _)| now.saturating_sub(*at) <= RATE_WINDOW.as_millis() as u64);
        counts.iter().find(|(at, _)| now.saturating_sub(*at) >= 30_000).cloned()
    };
    let mut now_counts = HashMap::new();
    let mut out = Vec::new();
    for t in names {
        let a = Asked { table: Some(t.clone()), sample: 0, seed: Options::default().seed, terms: false, top: 0 };
        let j = match answer(svc, a).await {
            Ok(j) => j,
            Err(e) => {
                out.push(json!({"table": t, "error": e.message}));
                continue;
            }
        };
        let of = |scope: &Json| (scope["stats"]["docs"]["live"].as_u64().unwrap_or(0), scope["stats"]["bytes"]["total"].as_u64().unwrap_or(0), scope["stats"]["segments"].as_array().map_or(0, |s| s.len()));
        let (docs, bytes, segments) = of(&j["cluster"]);
        let fresh = &j["cluster"]["freshness"];
        let (writes, reads) = (fresh["writes"].as_u64().unwrap_or(0), j["cluster"]["reads"]["docs"]["total"].as_u64().unwrap_or(0));
        now_counts.insert(t.clone(), (writes, reads));
        // Since the earlier look; or, servers started within the window,
        // since they started. A count that went down (a server restarted)
        // gives none.
        let started = fresh["started_ms"].as_u64().unwrap_or(0);
        let since = match &then {
            Some((at, c)) => c.get(&t).map(|&(w, r)| (*at, w, r)),
            None if started > 0 && now.saturating_sub(started) <= RATE_WINDOW.as_millis() as u64 => Some((started, 0, 0)),
            None => None,
        };
        let window_ms = since.map(|(at, _, _)| now.saturating_sub(at)).filter(|ms| *ms >= 30_000);
        let per_min = |n: u64, before: Option<u64>| {
            let (Some(ms), Some(before)) = (window_ms, before) else { return None };
            n.checked_sub(before).map(|d| (d as f64 / (ms as f64 / 60_000.0) * 10.0).round() / 10.0)
        };
        let shards: Vec<Json> = j["shards"].as_array().into_iter().flatten().map(|s| {
            let (d, b, _) = of(&s["scope"]);
            json!({"shard": s["shard"], "docs": d, "bytes": b})
        }).collect();
        // The shard furthest over the mean, by documents.
        let skew = (shards.len() > 1).then(|| {
            let mean = shards.iter().filter_map(|s| s["docs"].as_f64()).sum::<f64>() / shards.len() as f64;
            let top = shards.iter().max_by(|a, b| a["docs"].as_f64().unwrap_or(0.0).total_cmp(&b["docs"].as_f64().unwrap_or(0.0)));
            top.map(|s| json!({"shard": s["shard"], "over_mean": if mean > 0.0 { ((s["docs"].as_f64().unwrap_or(0.0) / mean - 1.0) * 100.0).round() / 100.0 } else { 0.0 }}))
        }).flatten();
        // Copies behind their primary, and copies that did not answer,
        // which may be any distance behind.
        let mut stale = Vec::new();
        for s in j["shards"].as_array().into_iter().flatten() {
            for r in s["replicas"].as_array().into_iter().flatten() {
                let url = r["url"].as_str().unwrap_or_default();
                let (short, since, down) = copies.get(url).cloned().unwrap_or((Json::Null, Json::Null, false));
                if down || !r["error"].is_null() {
                    stale.push(json!({"shard": s["shard"], "copy": url, "short": short, "down": true, "since_ms": since, "error": r["error"]}));
                } else if r["behind"].as_u64().is_some_and(|b| b > 0) {
                    stale.push(json!({"shard": s["shard"], "copy": url, "short": short, "behind_txn": r["behind"]}));
                }
            }
        }
        out.push(json!({
            "table": t, "docs": docs, "bytes": bytes, "segments": segments, "shards": shards, "skew": skew,
            "writes_per_min": per_min(writes, since.map(|s| s.1)), "reads_per_min": per_min(reads, since.map(|s| s.2)), "rate_window_s": window_ms.map(|ms| ms / 1000),
            "last_write_ms": fresh["last_write_ms"], "stale": stale,
        }));
    }
    // Growth: the size an hourly row of the log store had a day and a week
    // ago (this server's rows; a coordinator keeps them of the cluster).
    if let Some(store) = &svc.log_store {
        for (key, ago) in [("d1_bytes", 86_400_000u64), ("d7_bytes", 7 * 86_400_000)] {
            let rows = store.kept_rows(crate::logstore::TABLE_SIZES, now.saturating_sub(ago + 3_600_000), now.saturating_sub(ago), None, 1000).await.unwrap_or_default();
            for t in &mut out {
                let then = rows.iter().find(|r| r["name"] == t["table"]).and_then(|r| r["bytes"].as_u64());
                if let (Some(then), Some(b)) = (then, t["bytes"].as_u64()) {
                    t["growth"][key] = json!(b as i64 - then as i64);
                }
            }
        }
    }
    svc.tables_counts.lock().unwrap_or_else(|e| e.into_inner()).push_back((now, now_counts));
    let j = json!({"tables": out, "at_ms": now, "cached_s": TABLES_EVERY.as_secs()});
    *kept = Some((std::time::Instant::now(), j.clone()));
    Ok(j)
}

/// Keeps each table's size in the log store's `tables`, an hour apart
/// (growth for [`tables`]).
pub async fn keep_sizes(svc: &Arc<Service>) {
    let Some(store) = svc.log_store.clone() else { return };
    let Ok(j) = tables(svc).await else { return };
    let now = telemetry::now_ms();
    for t in j["tables"].as_array().into_iter().flatten() {
        if let (Some(name), Some(docs), Some(bytes)) = (t["table"].as_str(), t["docs"].as_u64(), t["bytes"].as_u64()) {
            store.keep_table_size(now, name, docs, bytes, t["segments"].as_u64().unwrap_or(0));
        }
    }
}
// ----- end core

/// The answer of `GET /v1/index`: this server's ([`node`]) as the
/// `cluster` scope with no shards, or a coordinator's cluster, shards and
/// replicas.
pub async fn answer(svc: &Arc<Service>, a: Asked) -> Result<Json, ApiError> {
    let t0 = std::time::Instant::now();
    let reads_on = svc.reads.on();
    match &svc.backend {
        Backend::Local(_) => {
            let n = node(svc, &a).await?;
            let part = Part::of(&n, None)?;
            Ok(json!({
                "role": "server", "table": n["table"], "server_role": n["role"], "reads_on": reads_on,
                "cluster": part.json(a.top), "shards": [], "sample": a.sample, "at_ms": telemetry::now_ms(), "took_ms": t0.elapsed().as_secs_f64() * 1e3,
            }))
        }
        Backend::Cluster(c) => {
            let schema = match &a.table {
                Some(t) => svc.table(t).await?,
                None => svc.first_table(),
            };
            let mut asked = a.clone();
            asked.table = Some(schema.table.clone());
            let nodes = c.inspect(&asked.body()).await;
            let through: Vec<(At, TableReads)> = svc.reads.of(&schema.table);
            let extra = |shard: &str, url: &str| through.iter().find(|(at, _)| at.shard == shard && at.replica == url).map(|(_, r)| r);
            let mut cluster: Option<Part> = None;
            let mut shards = Vec::new();
            for shard in nodes {
                let mut replicas = Vec::new();
                let mut parts: Vec<(bool, Part)> = Vec::new();
                for r in &shard.replicas {
                    let mut j = json!({"url": r.url, "role": r.role, "takes_writes": r.takes_writes});
                    match r.answer.as_ref().map_err(|e| e.clone()).and_then(|n| Part::of(n, extra(&shard.url, &r.url)).map(|p| (n, p)).map_err(|e| e.message)) {
                        Ok((n, p)) => {
                            j["server_role"] = n["role"].clone();
                            j["reads_on"] = n["reads_on"].clone();
                            j["scope"] = p.json(a.top);
                            parts.push((r.takes_writes, p));
                        }
                        Err(e) => {
                            // Its reads through here still count.
                            j["error"] = json!(e);
                            if let Some(x) = extra(&shard.url, &r.url) {
                                parts.push((false, Part { stats: None, reads: x.clone(), fresh: Fresh::default(), copies: 0 }));
                            }
                        }
                    }
                    replicas.push(j);
                }
                // The shard: its primary's data, every copy's reads.
                let data = parts.iter().find(|(w, p)| *w && p.stats.is_some()).or_else(|| parts.iter().find(|(_, p)| p.stats.is_some())).map(|(_, p)| p);
                let mut part = Part { stats: data.and_then(|p| p.stats.clone()), reads: TableReads::default(), fresh: Fresh::default(), copies: 0 };
                let writes = data.map_or(0, |p| p.fresh.writes);
                for (_, p) in &parts {
                    part.reads.merge(&p.reads);
                    part.fresh = part.fresh.latest(p.fresh, writes);
                    part.copies += p.copies;
                }
                // How far behind its primary each copy is.
                let head = data.and_then(|p| p.stats.as_ref()).and_then(|s| s.seq_no);
                for j in &mut replicas {
                    if let (Some(h), Some(s)) = (head, j["scope"]["stats"]["seq_no"].as_u64()) {
                        j["behind"] = json!(h.saturating_sub(s));
                    }
                }
                shards.push(json!({"shard": shard.index, "url": shard.url, "replicas": replicas, "scope": part.json(a.top)}));
                cluster = Some(match cluster.take() {
                    None => part,
                    Some(mut all) => {
                        match (&mut all.stats, &part.stats) {
                            (Some(x), Some(y)) => x.merge(y),
                            (None, Some(y)) => all.stats = Some(y.clone()),
                            _ => {}
                        }
                        all.reads.merge(&part.reads);
                        let writes = all.fresh.writes + part.fresh.writes;
                        all.fresh = all.fresh.latest(part.fresh, writes);
                        all.copies += part.copies;
                        all
                    }
                });
            }
            let cluster = cluster.unwrap_or(Part { stats: None, reads: TableReads::default(), fresh: Fresh::default(), copies: 0 });
            Ok(json!({
                "role": "coordinator", "table": schema.table, "reads_on": reads_on,
                "cluster": cluster.json(a.top), "shards": shards, "sample": a.sample, "at_ms": telemetry::now_ms(), "took_ms": t0.elapsed().as_secs_f64() * 1e3,
            }))
        }
    }
}

/// A shard's replicas' answers to a coordinator's inspection.
pub struct ShardNodes {
    pub index: usize,
    pub url: String,
    pub replicas: Vec<ReplicaNode>,
}

/// A replica's answer: its [`node`], or why it gave none.
pub struct ReplicaNode {
    pub url: String,
    pub role: String,
    pub takes_writes: bool,
    pub answer: Result<Json, String>,
}
