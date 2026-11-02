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
