//! One-off copies of tables between this server and the clusters in the
//! hub's book ([`crate::hub`]), and the tables a cluster made from the
//! Cloud view brings from this server once it is ready.
//!
//! A copy is one SQLite file: the source's export of the tables (their rows
//! and their kept Strata schemas: fields, key, shard and cluster keys),
//! sent as it is read into the target's import, which makes the tables it
//! lacks and writes each row by its key into those it has. Run again, a
//! copy writes the same rows again; it keeps no offset, and it does not
//! follow later writes (that would be replication between clusters).
//!
//! The source holds the rows: this server, or a connection that is one
//! server. A cluster's coordinators hold none (their shards' servers do),
//! so a cluster is a target, not yet a source.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::keys::{Principal, Scope};
use crate::service::Service;
use crate::ApiError;

/// One end of a copy: this server, or a connection in the hub's book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    Here,
    Connection(String),
}

impl End {
    /// `""` or `null` for this server, else a connection's name.
    pub fn parse(v: &Json) -> Result<End, ApiError> {
        match v {
            Json::Null => Ok(End::Here),
            Json::String(s) if s.trim().is_empty() => Ok(End::Here),
            Json::String(s) => Ok(End::Connection(s.trim().to_string())),
            _ => Err(ApiError::bad("from and to are a connection's name, or \"\" for this server")),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            End::Here => "this server".into(),
            End::Connection(n) => n.clone(),
        }
    }
}

/// How large a target's answer may be read.
const ANSWER_MAX: usize = 16 << 20;

/// Copies `tables` from `from` to `to` ([the module](self)), as `who`. The
/// target's import answer, with what was copied and how long it took.
pub async fn copy(svc: &Arc<Service>, who: &Principal, from: &End, to: &End, tables: &[String]) -> Result<Json, ApiError> {
    who.may(Scope::Admin, "a copy between clusters")?;
    if from == to {
        return Err(ApiError::bad(format!("from and to are both {}", from.describe())));
    }
    if tables.is_empty() {
        return Err(ApiError::bad("name the tables to copy"));
    }
    if let Some(t) = tables.iter().find(|t| t.is_empty() || !t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
        return Err(ApiError::bad(format!("{t:?} is not a table's name")));
    }
    let t0 = Instant::now();
    let list = tables.join(",");
    // The file, as the source sends it.
    let (file, bytes): (axum::body::Body, Option<u64>) = match from {
        End::Here => {
            let opts = chloedb::sqlite::ExportOptions { tables: tables.to_vec(), fts: false, indexes: false, meta: true };
            let (dir, path, _) = svc.export_sqlite(opts).await?;
            let data = tokio::fs::read(&path).await.map_err(ApiError::internal)?;
            drop(dir);
            let n = data.len() as u64;
            (axum::body::Body::from(data), Some(n))
        }
        End::Connection(n) => {
            let k = kube_for(svc, n).await?;
            let r = svc.hub.forward(n, reqwest::Method::GET, &format!("/v1/export?tables={list}&fts=false&indexes=false"), &axum::http::HeaderMap::new(), reqwest::Body::from(""), k.as_ref()).await?;
            if !r.status().is_success() {
                let why = error_of(r).await;
                let hint = if why.contains("coordinator holds no rows") { ": it is a cluster, whose coordinators hold no rows; copy from one server" } else { "" };
                return Err(ApiError { status: 502, message: format!("{n} did not export {list}: {why}{hint}") });
            }
            (r.into_body(), None)
        }
    };
    // Into the target.
    let mut answer = match to {
        End::Here => {
            let data = axum::body::to_bytes(file, svc.max_body).await.map_err(|e| ApiError { status: 502, message: format!("the file from {}: {e} (this server takes at most {} MiB)", from.describe(), svc.max_body >> 20) })?;
            let opts = chloedb::sqlite::ImportOptions { tables: tables.to_vec(), schemas: Vec::new(), blobs_base64: false };
            let mut j = svc.import_sqlite(who, data, opts, false, 0).await?;
            let written: u64 = j["tables"].as_array().map_or(0, |t| t.iter().filter_map(|t| t["rows"].as_u64()).sum());
            j["written"] = json!(written);
            j
        }
        End::Connection(n) => {
            let k = kube_for(svc, n).await?;
            let mut h = axum::http::HeaderMap::new();
            h.insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static("application/vnd.sqlite3"));
            let body = reqwest::Body::wrap_stream(file.into_data_stream());
            let r = svc.hub.forward(n, reqwest::Method::POST, &format!("/v1/import?tables={list}"), &h, body, k.as_ref()).await?;
            if !r.status().is_success() {
                let status = r.status().as_u16();
                let why = error_of(r).await;
                let hint = if status == 413 { ": the file is larger than it takes (--max-body-mb)" } else { "" };
                return Err(ApiError { status: 502, message: format!("{n} did not import {list}: {why}{hint}") });
            }
            let raw = axum::body::to_bytes(r.into_body(), ANSWER_MAX).await.map_err(|e| ApiError { status: 502, message: format!("{n}'s answer: {e}") })?;
            serde_json::from_slice(&raw).map_err(|e| ApiError { status: 502, message: format!("{n}'s answer: {e}") })?
        }
    };
    answer["from"] = json!(from.describe());
    answer["to"] = json!(to.describe());
    if let Some(b) = bytes {
        answer["bytes"] = json!(b);
    }
    answer["took_ms"] = json!(t0.elapsed().as_secs_f64() * 1e3);
    Ok(answer)
}

/// The error a refused call answered: its `error`, else its text.
async fn error_of(r: axum::response::Response) -> String {
    let status = r.status().as_u16();
    let raw = axum::body::to_bytes(r.into_body(), 64 << 10).await.unwrap_or_default();
    let text = serde_json::from_slice::<Json>(&raw).ok().and_then(|j| j["error"].as_str().map(String::from)).unwrap_or_else(|| String::from_utf8_lossy(&raw).trim().chars().take(300).collect());
    if text.is_empty() {
        format!("{status}")
    } else {
        format!("{text} ({status})")
    }
}

/// The client of the Kubernetes API a connection is reached through, if it
/// is reached that way ([`crate::hub::ViaKube`]).
pub async fn kube_for(svc: &Arc<Service>, name: &str) -> Result<Option<crate::operator::kube::Kube>, ApiError> {
    let Some(v) = svc.hub.get(name).and_then(|c| c.kube) else { return Ok(None) };
    let k = svc.clouds.kube_of(&v.made, &svc.aws, &svc.digitalocean).await.map_err(|e| ApiError { status: 502, message: format!("{name}: {e}") })?;
    Ok(Some(k))
}

/// The cluster `id` made from the Cloud view, in the hub's book through its
/// Kubernetes API with a key of this hub's own ([`crate::hub::Hub::adopt`]):
/// the connection, and whether it is new.
pub async fn adopt_made(svc: &Arc<Service>, id: &str, name: Option<String>, color: Option<String>, by: &str) -> Result<(crate::hub::Connection, bool), ApiError> {
    use crate::cloud::clusters::Phase;
    let m = svc.clouds.get(id).ok_or_else(|| ApiError { status: 404, message: format!("no cluster {id} was made here") })?;
    if !matches!(m.phase, Phase::Starting | Phase::Ready) {
        return Err(ApiError { status: 409, message: format!("{} is {:?}: its command center opens once ChloeDB is installed on it", m.cluster, m.phase).to_lowercase() });
    }
    let k = svc.clouds.kube_of(id, &svc.aws, &svc.digitalocean).await.map_err(|e| ApiError { status: 502, message: format!("{}: {e}", m.cluster) })?;
    let via = crate::hub::ViaKube { made: m.id.clone(), provider: m.provider.clone(), cluster: m.cluster.clone(), region: m.region.clone(), namespace: crate::cloud::NAMESPACE.to_string(), service: m.name.clone() };
    let had = svc.hub.list().iter().any(|c| c.kube.as_ref().is_some_and(|v| v.made == m.id));
    let c = svc.hub.adopt(via, name, color, &k, by).await?;
    Ok((c, !had))
}

/// How long a cluster made here may take to be ready before the tables it
/// was to bring are given up on, and how many times a copy that fails is
/// tried. It is looked at as often as the Cloud view's loop moves it on.
const BRING_WAIT: Duration = Duration::from_secs(3 * 3600);
const BRING_TRIES: u32 = 3;

/// Once the cluster `id` made here is ready, copies the tables it was made
/// to bring from this server into it, through its Kubernetes API; noted on
/// its steps whatever happens. Nothing when it brings nothing or brought
/// them already. On the runtime of the caller.
pub fn bring_when_ready(svc: Arc<Service>, id: String) {
    use crate::cloud::clusters::Phase;
    tokio::spawn(async move {
        let started = Instant::now();
        let every = svc.clouds.every();
        let m = loop {
            let Some(m) = svc.clouds.get(&id) else { return };
            if m.bring.is_empty() || m.brought.is_some() {
                return;
            }
            match m.phase {
                Phase::Ready => break m,
                Phase::Deleting | Phase::Deleted | Phase::Failed => {
                    let _ = svc.clouds.brought(&id, format!("did not bring {}: it is {:?}", m.bring.join(", "), m.phase).to_lowercase(), json!({"error": "not ready"}));
                    return;
                }
                _ => {}
            }
            if started.elapsed() > BRING_WAIT {
                let _ = svc.clouds.brought(&id, format!("did not bring {}: not ready after {} hours", m.bring.join(", "), BRING_WAIT.as_secs() / 3600), json!({"error": "not ready in time"}));
                return;
            }
            tokio::time::sleep(every).await;
        };
        // As the person who asked for the cluster, with what a key of
        // theirs here could do (they made a cluster: an admin's).
        let who = Principal { name: format!("{} (bringing tables)", m.by), key: None, scope: Scope::Admin, root: false, tables: None };
        let mut last = String::new();
        for i in 0..BRING_TRIES {
            if i > 0 {
                tokio::time::sleep(every * 2).await;
            }
            let r = async {
                let (c, _) = adopt_made(&svc, &id, None, None, &m.by).await?;
                copy(&svc, &who, &End::Here, &End::Connection(c.name), &m.bring).await
            }
            .await;
            match r {
                Ok(j) => {
                    let rows = j["written"].as_u64().unwrap_or(0);
                    let secs = j["took_ms"].as_f64().unwrap_or(0.0) / 1e3;
                    tracing::info!(cluster = %m.cluster, tables = %m.bring.join(","), rows, "brought tables from this server");
                    let _ = svc.clouds.brought(&id, format!("brought {} from this server: {rows} rows in {secs:.1} s", m.bring.join(", ")), j);
                    return;
                }
                Err(e) => {
                    tracing::warn!(cluster = %m.cluster, "bringing tables: {}", e.message);
                    last = e.message;
                }
            }
        }
        let _ = svc.clouds.brought(&id, format!("could not bring {}: {last}", m.bring.join(", ")), json!({"error": last}));
    });
}
