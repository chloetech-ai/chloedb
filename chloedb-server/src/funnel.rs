//! A query's funnel ([`chloedb::query::funnel`]), on a server or a
//! coordinator: `POST /v1/funnel` runs one query and says what happened
//! between its text and its rows, stage by stage, with the time each took
//! and the rows each kept and dropped; `GET /v1/queries/funnel` lays the
//! time of the queries the server ran on the same stages, together and by
//! statement.
//!
//! One query runs as a query does, in a trace of its own: its understood
//! words ([`Service::understood_sql`]) and expanded rankings
//! ([`Service::expanded_sql`]) first, each a span, then the statement
//! bound (the texts where vectors go embedded, each a span), then run with
//! its steps and their CPU time. Its rows come from the funnel's probes
//! ([`chloedb::query::funnel::Probes`]), run after it as queries of plans
//! (through a coordinator, over its shards as any query), quiet: none of
//! them is in the query log, and none counts in the time shown. The words
//! of each BM25 input are shown as the terms they became, with the rows
//! that hold each ([`Service::term_stats`]), and what "did you mean"
//! would correct ([`Service::correct`]). A blend (`FROM blend(...)`)
//! shows its sources and the fill's receipts.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use chloedb::query::funnel::{self as fun, SpanTime, Stage, Times};
use chloedb::query::ir::Plan;
use chloedb::query::exec::{ExecOptions, ResultSet};
use chloedb::query::Params;
use chloedb::schema::Schema;
use serde_json::{json, Value as Json};

use crate::keys::Principal;
use crate::service::{After, QueryReq, Service};
use crate::telemetry::{self, Finished, Span, Trace};
use crate::{json as js, ApiError};

/// The rows of the answer a funnel shows.
pub const ANSWER_ROWS: usize = 20;

/// The queries `GET /v1/queries/funnel` lists, the slowest first.
pub const LISTED: usize = 50;

/// What a funnel asks: the query, as `POST /v1/query` takes it.
pub struct Ask {
    pub sql: String,
    pub params: Params,
    pub timeout_ms: u64,
    pub budget: chloedb::query::budget::Budget,
}

fn span_times(spans: &[Span]) -> Vec<SpanTime<'_>> {
    spans.iter().map(|s| SpanTime { name: &s.name, start_us: s.start_us as f64, dur_us: s.dur_us as f64, call: s.shard.is_some() }).collect()
}

/// The end of the last span or step, from the request's start: a request's
/// wall time where its log's starts later (an understood query's is the
/// time of its rewritten statement).
fn end_us(spans: &[Span], steps: &[chloedb::query::steps::Step]) -> f64 {
    let a = spans.iter().map(|s| (s.start_us + s.dur_us) as f64).fold(0.0, f64::max);
    let b = steps.iter().map(|s| s.start_us + s.us).fold(0.0, f64::max);
    a.max(b)
}

/// The stages of one query's time, as the page shows them: every stage,
/// with its milliseconds and its share of the whole.
fn stages_json(t: &Times) -> Json {
    let total: f64 = t.iter().sum();
    Json::Array(
        Stage::ALL
            .iter()
            .map(|&s| {
                let us = t[s.index()];
                json!({"stage": s, "name": s.name(), "says": s.says(), "ms": round3(us / 1e3), "share": if total > 0.0 { (us / total * 1e4).round() / 1e4 } else { 0.0 }})
            })
            .collect(),
    )
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A query that ran in the funnel: its time, as its log would keep it.
struct Timed {
    times: Times,
    total_us: f64,
    spans: Vec<Span>,
    steps: Vec<chloedb::query::steps::Step>,
    plan: Vec<String>,
}

fn timed(trace: &Trace, t0: Instant, a: Option<&ResultSet>) -> Timed {
    let spans = trace.spans();
    let steps = a.map(|rs| rs.stats.steps.clone()).unwrap_or_default();
    let total_us = (t0.elapsed().as_secs_f64() * 1e6).max(end_us(&spans, &steps));
    let times = fun::timings(&steps, &span_times(&spans), total_us);
    Timed { times, total_us, spans, steps, plan: a.map(|rs| rs.stats.plan.clone()).unwrap_or_default() }
}

impl Timed {
    fn json(&self) -> Json {
        json!({
            "took_ms": round3(self.total_us / 1e3),
            "stages": stages_json(&self.times),
            "spans": self.spans,
            "steps": self.steps,
            "plan": self.plan,
        })
    }
}

/// `POST /v1/funnel`: query `ask` run once, in a trace of its own, and
/// its funnel.
pub async fn one(svc: &Arc<Service>, who: &Principal, ask: Ask) -> Result<Json, ApiError> {
    let trace = Trace::new();
    telemetry::traced(trace.clone(), run(svc.clone(), who.clone(), ask, trace)).await
}

async fn run(svc: Arc<Service>, who: Principal, ask: Ask, trace: Arc<Trace>) -> Result<Json, ApiError> {
    let t0 = trace.t0();
    svc.statement_fits(&ask.sql)?;
    let timeout = svc.timeout_of(ask.timeout_ms);
    let budget = svc.budget_for(Some(&who), ask.budget);
    // As a client's query, kept out of the query log: the funnel's own
    // runs are not the traffic it shows.
    let mut req = QueryReq { sql: ask.sql.clone(), params: ask.params.clone(), timeout_ms: ask.timeout_ms, who: Some(who.clone()), budget: ask.budget, quiet: true, ..Default::default() };
    let mut text = serde_json::Map::new();
    // The words first: what they name, and the terms of their first rows.
    if let Some((sql, lines)) = svc.understood_sql(&req).await? {
        req.sql = sql;
        text.insert("understood".into(), json!(lines));
    }
    if let Some((sql, lines)) = svc.expanded_sql(&req).await? {
        req.sql = sql;
        text.insert("expanded".into(), json!(lines));
    }
    // A blend: its sources' rows and the fill.
    if chloedb::query::blend::may_blend(&req.sql) {
        if let Some((call, cols)) = chloedb::query::blend::table_call_sql(&req.sql, &req.params)? {
            let tb = Instant::now();
            let (def, version) = svc.blend_def(&call.name, call.version).await?;
            let k = call.limit.unwrap_or(def.k);
            let b = svc.run_blend(&req, &def, version, &call.params, k, chloedb::query::blend::Want::default()).await;
            telemetry::record("blend", tb, if b.is_ok() { "ok" } else { "error" });
            let b = b?;
            let rs = b.result(&cols)?;
            let t = timed(&trace, t0, None);
            let mut out = t.json();
            out["kind"] = json!("blend");
            out["sql"] = json!(ask.sql);
            out["ran"] = json!(req.sql);
            out["text"] = Json::Object(text);
            out["blend"] = b.receipts();
            // A node (a blend stacked under this one) has no query of its own.
            out["sources"] = json!(def.sources.iter().map(|s| json!({"name": s.name, "sql": (!s.sql.is_empty()).then_some(&s.sql), "node": s.blend.is_some()})).collect::<Vec<_>>());
            out["params"] = json!(telemetry::params_json(&call.params));
            out["answer"] = answer(&rs, t.total_us / 1e3);
            return Ok(out);
        }
    }
    // Bound, its texts embedded: the plan's span, the embedding's inside it.
    let tp = Instant::now();
    let plan = svc.bound_read(&req.sql, &req.params, timeout, budget).await.and_then(|p| svc.open_ranking_models(Arc::new(p), false));
    telemetry::record("plan", tp, if plan.is_ok() { "ok" } else { "error" });
    let plan = plan?;
    let embedded = embedded(&svc, &req, &trace.spans());
    if !embedded.is_empty() {
        text.insert("embedded".into(), json!(embedded));
    }
    // The run, with its steps' CPU time.
    let mut ran = req.clone();
    (ran.plan, ran.profile) = (Some(plan.clone()), true);
    let a = svc.query(ran).await?;
    let t = timed(&trace, t0, Some(&a.rs));
    let mut out = t.json();
    out["sql"] = json!(ask.sql);
    out["ran"] = json!(req.sql);
    out["answer"] = answer(&a.rs, t.total_us / 1e3);
    if plan.root.uses_operators() {
        out["kind"] = json!("compound");
        out["text"] = Json::Object(text);
        out["notes"] = json!(["a query of several tables (a join, WITH, a subquery, UNION) is shown by its time: its rows are its parts'"]);
        return Ok(out);
    }
    let schema = svc.readable_table(plan.table()).await?;
    let select = plan.to_select(&schema)?;
    // The rows: the probes, after the time is taken.
    let depth = match &svc.backend {
        crate::service::Backend::Local(db) => db.exec.fusion_depth,
        crate::service::Backend::Cluster(_) => ExecOptions::default().fusion_depth,
    };
    let probes = fun::Probes::of(&schema, &select, depth);
    let rows = probe_rows(&svc, &req, &schema, &probes).await;
    out["kind"] = json!(probes.kind);
    // What each search's words became.
    let searches = searches(&svc, &req, &plan, &schema, &rows, ask.timeout_ms).await;
    if !searches.is_empty() {
        text.insert("searches".into(), json!(searches));
    }
    out["text"] = Json::Object(text);
    out["rows"] = serde_json::to_value(&rows).map_err(ApiError::internal)?;
    Ok(out)
}

/// The answer as the page shows it: its first rows.
fn answer(rs: &ResultSet, took_ms: f64) -> Json {
    let mut j = js::result(rs, took_ms);
    let n = rs.rows.len();
    if let Some(rows) = j.get_mut("rows").and_then(Json::as_array_mut) {
        rows.truncate(ANSWER_ROWS);
    }
    for k in ["stats", "keys", "scores"] {
        if let Some(o) = j.as_object_mut() {
            o.remove(k);
        }
    }
    j["total_rows"] = json!(n);
    j
}

/// The texts `req` embeds, each with the time its span took, in order.
fn embedded(svc: &Service, req: &QueryReq, spans: &[Span]) -> Vec<Json> {
    use chloedb::query::embed_sql::{self, Request};
    if !embed_sql::may_embed_with(&req.sql, &req.params) {
        return Vec::new();
    }
    let catalog = svc.readable();
    let refs: Vec<&Schema> = catalog.iter().map(|t| &**t).collect();
    let Ok(requests) = embed_sql::requests(&req.sql, &refs, &req.params) else { return Vec::new() };
    let mut seen = Vec::new();
    for r in requests {
        if !seen.contains(&r) {
            seen.push(r);
        }
    }
    let mut times = spans.iter().filter(|s| s.name == "embed").map(|s| s.dur_us as f64 / 1e3);
    seen.into_iter()
        .map(|r| {
            let ms = times.next().map(round3);
            match r {
                Request::Field { table, field, text } => {
                    let model = svc.models().iter().find(|m| m.table.eq_ignore_ascii_case(&table) && m.field.eq_ignore_ascii_case(&field)).map(|m| m.spec.model.clone());
                    json!({"table": table, "field": field, "model": model, "text": text, "ms": ms})
                }
                Request::Model { model, text } => json!({"model": model, "text": text, "ms": ms}),
            }
        })
        .collect()
}

/// A probe's answer: its plan run as a query of `like`'s, quiet.
async fn probe(svc: &Arc<Service>, like: &QueryReq, schema: &Schema, p: fun::Probe) -> (String, Result<ResultSet, ApiError>) {
    let mut r = like.clone();
    // The plan's own: the SQL only names it.
    (r.plan, r.params, r.scroll, r.hold, r.profile, r.own_read, r.cache_key, r.skip_steps) = (Some(Arc::new(Plan::of_select(schema, &p.select))), Params::new(), None, false, false, true, None, true);
    r.sql = format!("-- funnel probe {}\n{}", p.name, like.sql);
    (p.name, svc.query(r).await.map(|a| a.rs))
}

/// [`fun::run`]'s two rounds, each round's probes at once.
async fn probe_rows(svc: &Arc<Service>, like: &QueryReq, schema: &Schema, probes: &fun::Probes) -> fun::Rows {
    let mut answers = HashMap::new();
    let mut failed = Vec::new();
    for round in 0..2 {
        let ps = match round {
            0 => probes.first(),
            _ => probes.second(&answers),
        };
        let done = futures::future::join_all(ps.into_iter().map(|p| probe(svc, like, schema, p))).await;
        for (name, r) in done {
            match r {
                Ok(rs) => {
                    answers.insert(name, rs);
                }
                Err(e) => failed.push(format!("probe {name}: {}", e.message)),
            }
        }
    }
    let mut rows = probes.finish(&answers);
    rows.notes.extend(failed);
    rows
}

/// Each BM25 input's words: the terms they became, with the rows that
/// hold each, and what "did you mean" corrects.
async fn searches(svc: &Arc<Service>, req: &QueryReq, plan: &Plan, schema: &Schema, rows: &fun::Rows, timeout_ms: u64) -> Vec<Json> {
    let texts: Vec<(String, String)> = rows.inputs.iter().filter(|i| i.kind == "bm25").filter_map(|i| Some((i.field.clone()?, i.text.clone()?))).collect();
    if texts.is_empty() {
        return Vec::new();
    }
    let stats = svc.term_stats(&req.sql, Some(plan), &req.params, &After::Any, timeout_ms).await.ok().map(|s| s.0);
    let mut out = Vec::new();
    for (field, text) in texts {
        let Some(f) = schema.field(&field) else { continue };
        let df: HashMap<String, u64> = stats.as_ref().and_then(|s| s.fields.get(&field)).map(|s| s.df.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default();
        let terms = fun::terms_of(&text, f.text_analyzer(), &df);
        let corrected = svc.correct(&schema.table, &field, &text, timeout_ms).await.ok();
        let docs = stats.as_ref().and_then(|s| s.fields.get(&field)).map(|s| s.n_docs);
        out.push(json!({"field": field, "text": text, "docs": docs, "terms": terms, "corrected": corrected}));
    }
    out
}

// ---------------------------------------------------------------- many

/// One logged query's time on the stages, and its wall time.
fn times_of(f: &Finished) -> (Times, f64) {
    let total_us = (f.ms * 1e3).max(end_us(&f.trace, &f.steps));
    (fun::timings(&f.steps, &span_times(&f.trace), total_us), total_us)
}

/// The table a logged query reads, from its text.
fn table_of(sql: &str) -> Option<String> {
    crate::scoped::named_tables(sql).into_iter().next()
}

/// `GET /v1/queries/funnel`: the stages' time over the queries the server
/// ran (`since` a time in ms, `table`, `fingerprint`, `kind`), the slowest
/// listed with their stages; `id` one logged query's.
/// A query the answer cache answered: it ran no stage of its own.
fn cached(f: &Finished) -> bool {
    f.steps.iter().any(|s| s.op == "answer cache")
}

pub fn many(svc: &Service, q: &HashMap<String, String>, since: u64, limit: usize) -> Result<Json, ApiError> {
    if let Some(id) = q.get("id").filter(|s| !s.is_empty()) {
        let id: u64 = id.parse().map_err(|_| ApiError::bad("id is a query's number in the query log"))?;
        let f = svc.query_log.finished(id).ok_or_else(|| ApiError { code: None, status: 404, message: format!("query {id} is no longer kept: the server keeps the latest {} and the slowest {}", telemetry::RECENT, telemetry::SLOW) })?;
        let (t, total_us) = times_of(&f);
        let rows: BTreeMap<String, u64> = fun::step_rows(&f.steps).into_iter().map(|(s, n)| (fun::stage_key(s), n)).collect();
        return Ok(json!({
            "query": f.summary(),
            "took_ms": round3(total_us / 1e3),
            "stages": stages_json(&t),
            "stage_rows": rows,
            "spans": f.trace,
            "steps": f.steps,
            "plan": f.plan,
        }));
    }
    let want = |k: &str| q.get(k).map(|s| s.as_str()).filter(|s| !s.is_empty());
    let (table, fp, kind) = (want("table"), want("fingerprint"), want("kind"));
    let qs: Vec<Arc<Finished>> = svc
        .query_log
        .recent(since, telemetry::RECENT)
        .into_iter()
        // A client's queries: not a shard's to its coordinator, nor one
        // that never ran.
        .filter(|f| !f.shard && f.outcome != "refused" && f.kind != "invalid")
        .filter(|f| kind.is_none_or(|k| f.kind == k))
        .filter(|f| fp.is_none_or(|p| f.fingerprint == p))
        .filter(|f| table.is_none_or(|t| table_of(&f.sql).is_some_and(|n| n.eq_ignore_ascii_case(t))))
        .take(limit)
        .collect();
    let each: Vec<(Arc<Finished>, Times, f64)> = qs.into_iter().map(|f| {
        let (t, total) = times_of(&f);
        (f, t, total)
    }).collect();
    let all = fun::aggregate(&each.iter().map(|(_, t, n)| (*t, *n)).collect::<Vec<_>>());
    // The rows each stage ended with, on average over the queries whose
    // steps say: the queries of one statement make one funnel, and those
    // of many shapes one only together.
    let rows_of = |ix: &mut dyn Iterator<Item = &Arc<Finished>>| -> BTreeMap<String, Json> {
        let mut rows: BTreeMap<Stage, (u64, u64)> = BTreeMap::new();
        let (mut answered, mut n) = (0u64, 0u64);
        for f in ix {
            for (s, k) in fun::step_rows(&f.steps) {
                let e = rows.entry(s).or_default();
                (e.0, e.1) = (e.0 + k, e.1 + 1);
            }
            (answered, n) = (answered + f.rows as u64, n + 1);
        }
        let mut out: BTreeMap<String, Json> = rows.into_iter().map(|(s, (sum, n))| (fun::stage_key(s), json!({"mean": round3(sum as f64 / n as f64), "queries": n}))).collect();
        if n > 0 {
            out.insert("answered".into(), json!({"mean": round3(answered as f64 / n as f64), "queries": n}));
        }
        out
    };
    let stage_rows = rows_of(&mut each.iter().map(|(f, _, _)| f));
    // By statement: the queries of one fingerprint together.
    let mut by: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, (f, _, _)) in each.iter().enumerate() {
        by.entry(f.fingerprint.clone()).or_default().push(i);
    }
    let mut statements: Vec<Json> = by
        .into_iter()
        .map(|(fp, ix)| {
            let a = fun::aggregate(&ix.iter().map(|&i| (each[i].1, each[i].2)).collect::<Vec<_>>());
            let first = &each[ix[0]].0;
            let stages: serde_json::Map<String, Json> = a.stages.iter().filter(|s| s.mean_ms > 0.0).map(|s| (fun::stage_key(s.stage), json!(s.mean_ms))).collect();
            let rows = rows_of(&mut ix.iter().map(|&i| &each[i].0));
            let hits = ix.iter().filter(|&&i| cached(&each[i].0)).count();
            json!({"fingerprint": fp, "sql": first.sql, "table": table_of(&first.sql), "queries": a.queries, "cached": hits, "mean_ms": a.mean_ms, "p95_ms": a.p95_ms, "stages": stages, "rows": rows, "last_id": first.id})
        })
        .collect();
    statements.sort_by(|a, b| {
        let t = |j: &Json| j["mean_ms"].as_f64().unwrap_or(0.0) * j["queries"].as_f64().unwrap_or(0.0);
        t(b).total_cmp(&t(a))
    });
    let mut slowest: Vec<&(Arc<Finished>, Times, f64)> = each.iter().collect();
    slowest.sort_by(|a, b| b.2.total_cmp(&a.2));
    let listed: Vec<Json> = slowest
        .into_iter()
        .take(LISTED)
        .map(|(f, t, total)| json!({"id": f.id, "ts_ms": f.ts_ms, "sql": f.sql, "kind": f.kind, "outcome": f.outcome, "rows": f.rows, "ms": round3(total / 1e3), "stages": fun::times_json(t), "params": f.params, "cached": cached(f)}))
        .collect();
    Ok(json!({
        "queries": all.queries,
        "cached": each.iter().filter(|(f, _, _)| cached(f)).count(),
        "mean_ms": all.mean_ms,
        "p50_ms": all.p50_ms,
        "p95_ms": all.p95_ms,
        "stages": all.stages,
        "stage_rows": stage_rows,
        "statements": statements,
        "slowest": listed,
        "kept": telemetry::RECENT,
        "now_ms": telemetry::now_ms(),
    }))
}
