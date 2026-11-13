//! The relevance autopilot over HTTP ([`chloedb::autopilot`]): a run
//! replays the rank log's queries on one snapshot of this server's
//! database and answers with the report; with `"judge": true`, the rows
//! the two rankings show that no one graded are graded by Claude first
//! (UMBRELA's prompt, [`crate::judge`]), written to the judgments table,
//! and the measure is made again. The report is kept as a proposal, which
//! is applied (`CREATE OR REPLACE RANK PROFILE`) or dismissed later.
//!
//! The replay reads one snapshot, which a server holds for its own
//! database: a coordinator, whose shards each have their own, does not run
//! the autopilot yet; each shard's server does.
//!
//! | route | body | answer |
//! |---|---|---|
//! | `POST /v1/relevance/autopilot` | the options of [`chloedb::autopilot::Options`], and `judge`, `model`, `api_key`, `keep` | `{report, proposal, judged}` |
//! | `GET /v1/relevance/proposals` | | `{proposals: [...]}`, latest first |
//! | `GET /v1/relevance/proposals/:id` | | the proposal with its proof |
//! | `POST /v1/relevance/proposals/:id/apply` | `{force?}` | the proposal, applied |
//! | `POST /v1/relevance/proposals/:id/dismiss` | | the proposal, dismissed |

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value as Json};
use chloedb::autopilot::{self, Options};
use chloedb::db::Db;
use chloedb::query::Params;

use crate::judge::{self, JudgeReq};
use crate::keys::Principal;
use crate::service::{After, Service};
use crate::ApiError;

const COORDINATOR: &str = "the relevance autopilot replays queries on one snapshot of a database: run it on a single server or on each shard's server, not on a coordinator";

fn db(svc: &Service) -> Result<Arc<Db>, ApiError> {
    svc.local_db().cloned().ok_or_else(|| ApiError::bad(COORDINATOR))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> chloedb::Result<T> + Send + 'static) -> Result<T, ApiError> {
    Ok(tokio::task::spawn_blocking(f).await.map_err(ApiError::internal)??)
}

/// Upserts `docs` to `table`, made by `create` if it has none, through the
/// server's writer (so that they are logged and replicated as writes are).
async fn upsert(svc: &Arc<Service>, who: &Principal, create: &str, table: &str, docs: Vec<Json>) -> Result<(), ApiError> {
    if docs.is_empty() {
        return Ok(());
    }
    if svc.table(table).await.is_err() {
        svc.sql(who, create, &Params::new(), After::Any, 0).await?;
    }
    let ops = docs.into_iter().map(|d| json!({"op": "upsert", "table": table, "doc": d})).collect();
    svc.write_ops(ops, None).await?;
    Ok(())
}

/// `POST /v1/relevance/autopilot`.
pub async fn run(svc: &Arc<Service>, who: &Principal, mut b: Json) -> Result<Json, ApiError> {
    let m = b.as_object_mut().map(std::mem::take).unwrap_or_default();
    let mut rest = serde_json::Map::new();
    let (mut judge_rows, mut model, mut api_key, mut keep) = (false, None, None, true);
    for (k, v) in m {
        match k.as_str() {
            "judge" => judge_rows = v.as_bool().ok_or_else(|| ApiError::bad("judge is true or false"))?,
            "keep" => keep = v.as_bool().ok_or_else(|| ApiError::bad("keep is true or false"))?,
            "model" => model = v.as_str().filter(|s| !s.is_empty()).map(String::from),
            "api_key" => api_key = v.as_str().filter(|s| !s.is_empty()).map(String::from),
            _ => {
                rest.insert(k, v);
            }
        }
    }
    let o = Options::from_json(&Json::Object(rest))?;
    let db = db(svc)?;
    let (d, opts) = (db.clone(), o.clone());
    let mut report = blocking(move || d.autopilot(&opts)).await?;
    let mut judged = Json::Null;
    if judge_rows && !report.holes.is_empty() {
        // The holes by query, each query's graded in one request.
        let mut by: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        for h in &report.holes {
            by.entry(h.qid.clone()).or_insert_with(|| (h.query.clone(), Vec::new())).1.push(h.row_key.clone());
        }
        let now = chloedb::timestamp::now();
        let (mut docs, mut errors, mut used) = (Vec::new(), Vec::new(), None);
        for (qid, (query, keys)) in by {
            let req = JudgeReq { table: report.table.clone(), query: query.clone(), keys: keys.iter().map(|k| json!(k)).collect(), fields: Vec::new(), model: model.clone(), api_key: api_key.clone(), who: Some(who.clone()) };
            let j = match judge::run(svc, req).await {
                Ok(j) => j,
                // No key, or the model refused: the report stands as measured.
                Err(e) => {
                    errors.push(e.message);
                    break;
                }
            };
            used = j.get("model").cloned();
            for g in j.get("grades").and_then(|g| g.as_array()).into_iter().flatten() {
                match (g.get("key").and_then(|k| k.as_str()), g.get("grade").and_then(|x| x.as_f64())) {
                    (Some(k), Some(grade)) => docs.push(autopilot::judgment_doc(&qid, &query, k, grade, "claude", now)),
                    _ => errors.extend(g.get("error").and_then(|e| e.as_str()).map(String::from)),
                }
            }
        }
        let n = docs.len();
        upsert(svc, who, &autopilot::judgments_sql(&o.judgments)?, &o.judgments, docs).await?;
        if n > 0 {
            let (d, opts) = (db.clone(), o.clone());
            report = blocking(move || d.autopilot(&opts)).await?;
        }
        errors.truncate(5);
        judged = json!({"rows": n, "model": used, "errors": errors});
    }
    let mut proposal = Json::Null;
    if keep && report.profile.is_some() {
        let now = chloedb::timestamp::now();
        let id = autopilot::proposal_id(&report, now);
        upsert(svc, who, &autopilot::proposals_sql(autopilot::PROPOSALS)?, autopilot::PROPOSALS, vec![report.proposal_doc(&id, now)]).await?;
        proposal = json!(id);
    }
    Ok(json!({"report": report.to_json(), "proposal": proposal, "judged": judged}))
}

/// `GET /v1/relevance/proposals`: the latest 100, without their proofs.
pub async fn list(svc: &Arc<Service>) -> Result<Json, ApiError> {
    let db = db(svc)?;
    let rows = blocking(move || {
        if db.storage().table(autopilot::PROPOSALS).is_none() {
            return Ok(Vec::new());
        }
        let rs = db.query(&format!("SELECT id, target, profile, ts, status, verdict, gain, before, after FROM {} ORDER BY ts DESC LIMIT 100", autopilot::PROPOSALS), &Params::new())?;
        Ok(rs.rows.iter().map(|r| Json::Object(rs.columns.iter().zip(r).map(|(c, v)| (c.clone(), autopilot::cell_json(v))).collect())).collect::<Vec<_>>())
    })
    .await?;
    Ok(json!({"proposals": rows}))
}

/// A proposal with its proof as JSON.
async fn one(db: Arc<Db>, id: String) -> Result<Json, ApiError> {
    let mut p = blocking(move || db.proposal(&id, true)).await.map_err(|e| if e.message.contains("no proposal ") { ApiError { code: None, status: 404, message: e.message } } else { e })?;
    if let Some(s) = p.get("proof").and_then(|s| s.as_str()) {
        p["proof"] = serde_json::from_str(s).unwrap_or(Json::Null);
    }
    Ok(p)
}

/// `GET /v1/relevance/proposals/:id`.
pub async fn get(svc: &Arc<Service>, id: String) -> Result<Json, ApiError> {
    one(db(svc)?, id).await
}

/// `POST /v1/relevance/proposals/:id/apply`: its statement, while the
/// profile is the one it was measured against ([`autopilot::check_apply`]).
pub async fn apply(svc: &Arc<Service>, who: &Principal, id: String, force: bool) -> Result<Json, ApiError> {
    let db = db(svc)?;
    let row = one(db.clone(), id.clone()).await?;
    let s = |k: &str| row.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let now = db.snapshot().table(&s("target")).and_then(|t| t.rank_profile(&s("profile")).map(|p| p.hash()));
    let statement = autopilot::check_apply(&row, now.as_deref(), force)?;
    svc.sql(who, &statement, &Params::new(), After::Any, 0).await?;
    status(svc, who, &id, "applied").await?;
    one(db, id).await
}

/// `POST /v1/relevance/proposals/:id/dismiss`.
pub async fn dismiss(svc: &Arc<Service>, who: &Principal, id: String) -> Result<Json, ApiError> {
    let db = db(svc)?;
    let row = one(db.clone(), id.clone()).await?;
    if row.get("status").and_then(|s| s.as_str()) != Some("open") {
        return Err(ApiError::bad(format!("proposal {id} is {}: only an open proposal is dismissed", row["status"])));
    }
    status(svc, who, &id, "dismissed").await?;
    one(db, id).await
}

async fn status(svc: &Arc<Service>, who: &Principal, id: &str, to: &str) -> Result<(), ApiError> {
    let p = Params::new().set("s", chloedb::Value::Str(to.into())).set("id", chloedb::Value::Str(id.into()));
    svc.sql(who, &format!("UPDATE {} SET status = :s WHERE id = :id", autopilot::PROPOSALS), &p, After::Any, 0).await?;
    Ok(())
}
