//! The HTTP JSON API, Prometheus metrics and the query UI.
//!
//! | route | body | answer |
//! |---|---|---|
//! | `GET /` | | the UI |
//! | `GET /livez`, `/readyz` | | `{status}`: 503 when a restart is needed; `/readyz` also while the server drains (`--drain-ms`) |
//! | `GET /v1/health`, `/v1/stats`, `/v1/config` | | JSON (`/v1/health`: 503 when "down" or "failing") |
//! | `GET /v1/schema?table=name` | | a table's schema (the first table's without `table`) |
//! | `GET /v1/tables` | | `{tables: [...]}`: every table's schema and rank profiles, the first first |
//! | `GET /v1/tables/:table/paths?field=attrs&path=dims&where=cond` | | `{table, field, rows, paths: [{path, keys, rows, types: {string, number, bool}, distinct, min?, max?, values?: [{value, rows}]}], truncated}`: the paths a JSON field's objects hold ([`strata::query::paths`]), under `path` if given, among the rows `where` matches; from a coordinator, every shard's summed |
//! | `GET /metrics` | | Prometheus text |
//! | `POST /v1/query` | `{sql, params?, timeout_ms?, shard?, stats?, snapshot?, min_seq_no?, min_seq_nos?, log?, trace?, profile?}` | rows, and in `stats.steps` the query's timed steps (with `profile`, their CPU time too, and with `trace` or `profile` the request's spans in `trace`); with `log` (`true` or `{table?, request?, context?, variant?, candidates?, ts?, interleave?: {sql, variants?}}`) also `request`, `logged` and, interleaved, `teams` ([`strata::rank_log`]) |
//! | `POST /v1/sql` | `{sql, params?, timeout_ms?, min_seq_no?, min_seq_nos?, log?}` | rows, or `{written, seq_no?, seq_nos?}` |
//! | `POST /v1/rank_log/export` | `{table?, since?, until?, events?, history?, window?, grades?}` | NDJSON: one record per logged row ([`strata::rank_log::export`]) |
//! | `POST /v1/rank_log/interleaving` | the export's body | `{pairs: [{variants, requests, acted, wins, ties, delta, p}], took_ms}`: which of two interleaved rankings the users preferred ([`strata::rank_log::Credit`]) |
//! | `POST /v1/write` | `{ops: [...]}` (see `proto/strata.proto`; each op writes the first table, or its `table`) | `{written, seq_no?, seq_nos?}` |
//! | `GET /v1/kv/:table/:key?min_seq_no=n` | | `{table, key, found, row?, seq_no?}`: the row with that primary key, read without SQL, as `SELECT *` answers it ([`Service::get_rows`]); 404 when no live row has it (or it has expired) |
//! | `POST /v1/kv/get` | `{keys, table?, timeout_ms?, min_seq_no?, min_seq_nos?}` | `{table, columns, rows, seq_nos, took_ms}`: per key, in their order, its row as an object (null for none) and its number; a coordinator asks each key's shard |
//! | `PUT /v1/kv/:table/:key?if_seq_no=n` | the row's fields as an object (the key may be left out) | `{written, seq_no?}`: the row written in place of any with the key (an upsert; with `if_seq_no`, only while the row's number is that) |
//! | `DELETE /v1/kv/:table/:key?if_seq_no=n&routing=v` | | `{deleted, written, seq_no?}` |
//! | `POST /v1/plan` | `{plan, timeout_ms?, min_seq_no?, min_seq_nos?}` (a plan as `/v1/explain` gives it) | rows, or `{written, seq_no?, seq_nos?}` |
//! | `POST /v1/import?format=jsonl&table=name` | rows as JSON Lines, JSON, CSV or TSV (of the first table without `table`) | `{written, skipped_fields}` |
//! | `POST /v1/import?format=sqlite&tables=a,b&plan=true` | a SQLite file (`application/vnd.sqlite3`; recognised by its first bytes too) | `{tables, skipped, written}`, or with `plan` the plan ([`strata::sqlite`]) |
//! | `POST /v1/import?format=parquet&table=name&key=id&plan=true` | a Parquet file (`application/vnd.apache.parquet`; recognised by its first bytes too) | `{table, rows, created, skipped, notes, written}`, or with `plan` the plan ([`strata::parquet`]) |
//! | `GET /v1/export?tables=a,b&fts=false&indexes=false&schemas=false` | | the database as a SQLite file |
//! | `GET /v1/export?format=parquet&tables=name` | | one table as a Parquet file, keeping its Strata schema |
//! | `POST /v1/schema/check` | a schema, in either JSON form | `{schema, short}`, or an error |
//! | `POST /v1/explain` | `{sql, params?, hits?}` | the bound query, its plan IR and the executor's plan; with `hits`, the top rows with their `score()` and `features()` |
//! | `POST /v1/embed` | `{text, field?, table?}` | `{table, field, vector}`: the text's vector by an embedded field's model; `vectors`, a vector a token, for a multi-vector field's (ColBERT) |
//! | `GET /v1/models` | | `{dir, models: [{name, dim, hosted, what, ready, error}]}`: the embedding models a schema can name, and whether each would open here |
//! | `GET /v1/media/<sha256>` | | the bytes of the file a row's MEDIA value names, by its hash, read as writes read files (`--media-store`, `--media-fetch`); 404 when no row names it, 400 from a coordinator |
//! | `GET /v1/synonyms`, `PUT /v1/synonyms` | `{text}` (Solr format) | the synonym list |
//! | `POST /v1/term_stats` | `{sql, params?, timeout_ms?, min_seq_no?, min_seq_nos?}` | `{stats, snapshot}` |
//! | `POST /v1/snapshot` | | `{snapshot, txn, ttl_s}` |
//! | `POST /v1/agent` | `{question, model?, effort?, api_key?, max_steps?}` | an answer and the steps taken |
//! | `POST /v1/judge` | `{query, keys, table?, fields?, model?, api_key?}` | `{grades: [{key, grade, error?}], model, usage}`: each row graded 0 to 3 for the query by a Claude model ([`crate::judge`]) |
//! | `GET /v1/queries` | | `{queries, running}`: the queries running now, the longest first, each with its trace so far |
//! | `DELETE /v1/queries/:id` | | `{cancelled}`: the query stops at its next check and answers 499 |
//! | `GET /v1/queries/recent?since=ms&limit=n`, `/v1/queries/slow?limit=n` | | `{queries}`: the latest that ended (or the slow ones), the newest first, each with its trace |
//! | `GET /v1/queries/request/:id` | | `{queries}`: those a request (its `x-request-id`) ran |
//! | `GET /v1/queries/finished/:id` | | one ended query with its whole profile: trace, steps ([`strata::query::steps`]) and plan lines (the lists give each without them, unless `full=true`) |
//! | `GET /v1/profile/settings`, `PUT /v1/profile/settings` | `{slow_ms?, cpu_all?}` | the slow threshold (until restarted) and whether every query's CPU time is measured |
//! | `POST /v1/profile/cpu?format=folded&node=` | `{seconds?, hz?, sql?, params?, clients?}` | a CPU profile ([`crate::profiler`]) of the whole server for `seconds`, or of `sql` run again and again for that long by `clients` callers at once (1 to 64): every thread's stacks, sampled `hz` times a CPU second; `format=folded` answers folded lines (flamegraph.pl, speedscope); 409 while another is taken |
//! | `GET /v1/profile/memory?node=` | | the heap ([`crate::heap`]: bytes in use and their peak, bytes allocated, allocations and frees), the resident memory and its peak, and the threads |
//! | `POST /v1/profile/heap?format=folded&value=in_use&node=` | `{seconds?, every?, sql?, params?, clients?}` | an allocation profile ([`crate::heap::take`]): about one allocation in every `every` bytes (default 2 MiB) a thread allocates, sampled for `seconds`, with its stack; by stack, the bytes allocated and those still in use at the end; a query in a loop as for a CPU profile |
//! | `GET /v1/profile/threads?node=` | | each thread of the process: its name, state, CPU time and context switches (Linux); on a coordinator, each of these four with `node` (a replica URL of `/v1/cluster`) is that server's, asked over gRPC (`Profile`) |
//! | `GET /v1/queries/stats?from=&to=&limit=n`, `GET /v1/queries/stats/:fingerprint`, `POST /v1/queries/stats/reset` | | statistics per statement shape over a window of the last day, by time spent ([`crate::telemetry::Statements`]) |
//! | `GET /v1/timeseries?since=ms` | | `{interval_ms, points}`: rates and gauges every few seconds, the last hour ([`crate::telemetry::Series`]) |
//! | `GET /v1/timeseries?from=&to=` | | the same over any range: memory's points when it holds `from`, else a point a minute from the log store's `load` (`source`) |
//! | `GET /v1/index/tables` | | every table's documents, bytes and segments, by shard, its skew, writes and reads a minute, growth over a day and a week, and copies behind; kept 30 s ([`crate::inspect::tables`]) |
//! | `POST /v1/admin/copies/verify`, `POST /v1/admin/copies/checkpoint` | `{url, timeout_ms?}`, `{url, name, layout?, link_from?}` | on a coordinator, of a copy (its URL or short name): its shard's primary compares the replicas' rows with its own (`{replicas: [{url, in_sync, compared, differing_slots, first_differing_slot, seq_no, error}]}`), or the copy writes a checkpoint into its own `--backup-dir` (`{checkpoint}`) |
//! | `POST /v1/admin/plan` | `{shards?: [{shard, copies}], add_shards?, remove_shards?, moves?}` | on own machines, the steps of a change of the cluster's shape, rows to copy, an estimate, what clients feel and what to check first; changes nothing ([`crate::coord::plan`]) |
//! | `GET /v1/admin/kube/versions?limit=30` | | the images of the cluster's repository a person may pick, newest first, each `current`, `newer`, `tracked`; without Kubernetes too ([`crate::operator::upgrade::Versions`]) |
//! | `GET /v1/load?from=&to=&copies=all` | | on a coordinator, each copy's load a minute from its own log store; copies that did not answer under `missing` |
//! | `GET /v1/logs?after=seq&level=warn&request=id&q=text&limit=n` | | `{lines, last, filter, filter_expires_ms, filter_set_by}`: the latest log lines ([`crate::obs::ring`]) |
//! | `GET /v1/logs/filter`, `PUT /v1/logs/filter?node=` | `{filter, expires_s?}` (as `--log`) | `{filter, expires_ms, set_by, set_ms, previous}`: what is logged, for `expires_s` (30 min by default; 0: until set again) and then the filter before; on a coordinator, `node` (a copy's URL or short name) sets that copy's |
//! | `POST /v1/logs/query` | `{sql, params?, timeout_ms?, scope?}` (SQL over tables `logs` and `queries`) | rows of the server's log store ([`crate::logstore`]); on a coordinator, of every server's merged, with `nodes`: each server's rows, time and error (`"scope": "local"`: its own store alone) |
//! | `GET /v1/logs/store` | | `{store, schemas, nodes?}`: the log store (its path, rows, bytes, retention, what it wrote, dropped and deleted; null without one), its tables, and on a coordinator every server's |
//! | `GET /v1/system`, `/v1/cluster`, `/v1/routes` | | the process and its limits; the cluster's copies; requests by route and queries by kind |
//! | `GET /v1/whoami` | | `{name, key, scope, root, cluster, caller}`: who the request's credential is ([`crate::keys`]) |
//! | `GET /v1/events?from=&to=&kind=&limit=`, `POST /v1/events/notes` | `{text, ts_ms?, where?}` | what happened, the newest first: copies down and back, new primaries, changes, notes ([`crate::events`]) |
//! | `GET /v1/health/fleet` | | `{status, headline, since_ms, checks, problems, coordinators}`: whether the cluster is all right and what is wrong first ([`crate::fleet`]) |
//! | `GET /v1/admin/keys`, `POST /v1/admin/keys`, `DELETE /v1/admin/keys/:id`, `POST /v1/admin/keys/:id/rotate` | `{name, scope, expires_days?, owner?, note?}`, rotate `{grace_minutes?}` | the API keys, how many were revoked, and how the last merge of keys went with each server of the cluster (`peers`, [`crate::keysync`]); a new one's secret is answered once; each key with its owner, note and last address, and the revoked ones (who revoked them, when); a rotation answers a new key of the same name and scope, the old one expiring after the grace (60 min) |
//! | `GET /v1/admin/audit?kind=&from=&to=&who=&q=&limit=n` | | the changes, notes and refused requests, the newest first, with `kind` (change, note, refusal), who, addr, client, reason, object, before and after; older than memory holds from the log store's `audit` |
//! | `GET /v1/alerts`, `PUT /v1/alerts/rules` | `{rules}` | the alert rules with their states and the latest changes ([`crate::alerts`]) |
//! | `GET /v1/alerts/:rule`, `POST /v1/alerts/:rule/ack`, `POST`/`DELETE /v1/alerts/:rule/silence`, `POST /v1/alerts/:rule/notes` | `{note}`, `{minutes, reason}`, `{text}` | one alert with its events (7 days), acknowledged, silenced, noted (write scope) |
//!
//! Every answer carries the request's id in `x-request-id`: the one the
//! client sent, if it is 1 to 64 letters, digits, '-', '_', '.' or ':', or
//! a new one ([`crate::obs`]).
//!
//! With a token or a cluster token set, or an API key made, every route but
//! `/`, `/livez`, `/readyz` and `/v1/health` needs `Authorization: Bearer
//! <token or key>`, and a key the scope of its route ([`route_scope`]). Every route checks the `Host` header
//! and, on a request that can change something, the `Origin` header
//! ([`crate::auth`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use futures::StreamExt;
use axum::extract::{DefaultBodyLimit, MatchedPath, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json as AxJson, Router};
use serde_json::{json, Value as Json};
use tracing::Instrument;

use strata::ingest::Format;
use strata::storage::Layout;

use crate::auth::{self, Access, Secret};
use crate::replication::Acks;
use crate::service::{has_source, After, QueryReq, Service, SqlAnswer, Stream, Written, STREAM_OPS};
use crate::keys::{self, Principal, Scope};
use crate::{agent, json as js, judge, obs, profiling, telemetry, ui, ApiError};


/// Why a request failed on the server's side, for [`observe`] to log.
#[derive(Clone)]
struct Failure(Arc<str>);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let failure = (self.status >= 500).then(|| Failure(self.message.as_str().into()));
        let client = (400..500).contains(&self.status).then(|| ClientError(self.message.as_str().into()));
        let mut r = (code, AxJson(json!({"error": self.message}))).into_response();
        if let Some(c) = client {
            r.extensions_mut().insert(c);
        }
        // Refused for the server's load: when to try again.
        if self.status == 429 {
            r.headers_mut().insert(header::RETRY_AFTER, header::HeaderValue::from(crate::service::RETRY_AFTER.as_secs()));
        }
        if let Some(f) = failure {
            r.extensions_mut().insert(f);
        }
        r
    }
}

type Res = Result<AxJson<Json>, ApiError>;

#[derive(Clone)]
struct App {
    svc: Arc<Service>,
    token: Option<Secret>,
    cluster: Option<Secret>,
    hosts: Option<Arc<[String]>>,
}

impl App {
    /// Whether callers must prove themselves by more than a key: a token or
    /// the cluster's.
    fn guarded(&self) -> bool {
        self.token.is_some() || self.cluster.is_some()
    }
}

pub fn router(svc: Arc<Service>, access: &Access) -> Router {
    // Rows come in writes and imports; every other body is small.
    let rows = DefaultBodyLimit::max(svc.max_body);
    let app = App { svc, token: access.token.clone(), cluster: access.cluster.clone(), hosts: access.hosts.clone().map(Into::into) };
    let api = Router::new()
        .route("/v1/stats", get(stats))
        .route("/v1/schema", get(schema))
        .route("/v1/tables", get(tables))
        .route("/v1/tables/:table/paths", get(json_paths))
        .route("/v1/index", get(index))
        .route("/v1/index/reads/reset", post(reset_reads))
        .route("/v1/config", get(config))
        .route("/metrics", get(metrics))
        .route("/v1/query", post(query))
        .route("/v1/sql", post(sql))
        .route("/v1/write", post(write).layer(rows.clone()))
        .route("/v1/kv/get", post(kv_get))
        .route("/v1/kv/:table/:key", get(kv_one).merge(put(kv_put)).merge(axum::routing::delete(kv_delete)).layer(rows.clone()))
        .route("/v1/plan", post(run_plan).layer(rows.clone()))
        .route("/v1/import", post(import).layer(rows))
        .route("/v1/export", get(export))
        // A stream's body has no end set.
        .route("/v1/stream", post(stream).layer(DefaultBodyLimit::disable()))
        .route("/v1/sources", get(sources))
        .route("/v1/sources/*name", get(source))
        .route("/v1/schema/check", post(schema_check))
        .route("/v1/explain", post(explain))
        .route("/v1/embed", post(embed))
        .route("/v1/models", get(models))
        .route("/v1/media/:sha256", get(media_file))
        .route("/v1/synonyms", get(synonyms).merge(put(put_synonyms)))
        .route("/v1/term_stats", post(term_stats))
        .route("/v1/sid/next", post(sid_next))
        .route("/v1/rank_log/export", post(export_rank_log))
        .route("/v1/rank_log/interleaving", post(interleaving))
        .route("/v1/snapshot", post(snapshot))
        .route("/v1/admin/checkpoint", post(admin_checkpoint))
        .route("/v1/admin/check", post(admin_check))
        .route("/v1/admin/vacuum", post(admin_vacuum))
        .route("/v1/admin/verify", post(admin_verify))
        .route("/v1/admin/promote", post(admin_promote))
        .route("/v1/admin/shards", post(add_shard))
        .route("/v1/admin/shards/:i", axum::routing::delete(remove_shard))
        .route("/v1/admin/shards/:i/replicas", post(add_replica).merge(axum::routing::delete(remove_replica)))
        .route("/v1/admin/shards/:i/drain", post(drain_shard))
        .route("/v1/admin/shards/:i/promote", post(promote_in_shard))
        .route("/v1/admin/rebalance", post(rebalance))
        .route("/v1/admin/moves", get(moves).merge(post(move_slots)))
        .route("/v1/admin/moves/stop", post(stop_moves))
        .route("/v1/admin/moves/finish", post(finish_move))
        .route("/v1/agent", post(agent_run))
        .route("/v1/judge", post(judge))
        .route("/v1/queries", get(live_queries))
        .route("/v1/queries/:id", axum::routing::delete(cancel_query))
        .route("/v1/queries/recent", get(recent_queries))
        .route("/v1/queries/slow", get(slow_queries))
        .route("/v1/queries/request/:id", get(request_queries))
        .route("/v1/queries/finished/:id", get(finished_query))
        .route("/v1/profile/settings", get(profile_settings).merge(put(put_profile_settings)))
        .route("/v1/profile/cpu", post(cpu_profile))
        .route("/v1/profile/memory", get(memory))
        .route("/v1/profile/heap", post(heap_profile))
        .route("/v1/profile/threads", get(threads))
        .route("/v1/queries/stats", get(query_stats))
        .route("/v1/queries/stats/reset", post(reset_query_stats))
        // workload (alerts agent): one statement over a window
        .route("/v1/queries/stats/:fingerprint", get(query_stat))
        .route("/v1/timeseries", get(timeseries))
        .route("/v1/logs", get(logs))
        .route("/v1/logs/filter", put(log_filter).merge(get(log_filter_now)))
        .route("/v1/logs/query", post(log_query))
        .route("/v1/logs/store", get(log_store))
        .route("/v1/system", get(system))
        .route("/v1/cluster", get(cluster))
        .route("/v1/kube", get(kube))
        .route("/v1/admin/kube", post(kube_change))
        .route("/v1/admin/kube/plan", post(kube_plan))
        .route("/v1/admin/kube/resume", post(kube_resume))
        .route("/v1/admin/kube/upgrade", post(kube_upgrade))
        .route("/v1/admin/kube/autoscale/dismiss", post(autoscale_dismiss))
        .route("/v1/admin/autoscale/replay", post(autoscale_replay))
        .route("/v1/admin/aws", get(aws_status))
        .route("/v1/admin/aws/credentials", put(aws_save).merge(axum::routing::delete(aws_forget)))
        .route("/v1/admin/aws/regions", get(aws_regions))
        .route("/v1/admin/aws/machines", get(aws_machines))
        .route("/v1/admin/aws/plan", post(aws_plan))
        .route("/v1/admin/aws/validate", post(aws_validate))
        .route("/v1/admin/digitalocean", get(do_status))
        .route("/v1/admin/digitalocean/token", put(do_save).merge(axum::routing::delete(do_forget)))
        .route("/v1/admin/digitalocean/regions", get(do_regions))
        .route("/v1/admin/digitalocean/machines", get(do_machines))
        .route("/v1/admin/digitalocean/plan", post(do_plan))
        .route("/v1/admin/digitalocean/validate", post(do_validate))
        .route("/v1/admin/cloud/clusters", get(cloud_list).post(cloud_create))
        .route("/v1/admin/cloud/clusters/:id", get(cloud_one).merge(axum::routing::delete(cloud_forget)))
        .route("/v1/admin/cloud/clusters/:id/delete", post(cloud_delete))
        .route("/v1/routes", get(routes))
        .route("/v1/whoami", get(whoami))
        .route("/v1/admin/keys", get(list_keys).merge(post(create_key)))
        .route("/v1/admin/keys/:id", axum::routing::delete(revoke_key))
        .route("/v1/admin/audit", get(audit_log))
        // alerts (alerts agent)
        .route("/v1/alerts", get(alerts))
        .route("/v1/alerts/rules", put(put_alert_rules))
        .route("/v1/alerts/:rule", get(alert))
        .route("/v1/alerts/:rule/ack", post(ack_alert))
        .route("/v1/alerts/:rule/silence", post(silence_alert).delete(unsilence_alert))
        .route("/v1/alerts/:rule/notes", post(note_alert))
        // ----- core: the rebuilt command center's contract (fleet health,
        // events, time beyond an hour, versions, plans, tables)
        .route("/v1/health/fleet", get(fleet_health))
        .route("/v1/events", get(events_list))
        .route("/v1/events/notes", post(events_note))
        .route("/v1/admin/keys/:id/rotate", post(rotate_key))
        .route("/v1/load", get(copies_load))
        .route("/v1/admin/kube/versions", get(kube_versions))
        .route("/v1/admin/plan", post(admin_plan))
        .route("/v1/index/tables", get(index_tables))
        .route("/v1/admin/copies/verify", post(copy_verify))
        .route("/v1/admin/copies/checkpoint", post(copy_checkpoint))
        // ----- end core
        .route_layer(middleware::from_fn_with_state(app.clone(), auth));
    Router::new()
        .route("/", get(ui::index))
        .route("/classic", get(ui::classic))
        .route("/ui/*file", get(ui::file))
        .route("/v1/health", get(health))
        .route("/livez", get(live))
        .route("/readyz", get(ready))
        .merge(api)
        .layer(middleware::map_response(close_when_draining))
        .layer(middleware::from_fn_with_state(app.clone(), browsers))
        .layer(DefaultBodyLimit::max(crate::service::SMALL_BODY))
        .layer(middleware::from_fn_with_state(app.clone(), observe))
        .with_state(app)
}

/// Gives the request its id (answered in `x-request-id`), serves it in a
/// span that names it, counts it by route and status, and logs it: at
/// debug level, or as an error when it failed on the server's side. The
/// log viewer's own requests (`/v1/logs...`) are not logged as answered:
/// a viewer following the logs would otherwise read lines of its reading.
/// 
async fn observe(State(app): State<App>, req: Request, next: Next) -> Response {
    let id = obs::request_id(header_str(req.headers(), header::HeaderName::from_static(obs::HEADER)));
    let route = req.extensions().get::<MatchedPath>().map_or("other", |p| p.as_str()).to_string();
    let span = tracing::info_span!("request", id = %id, method = %req.method(), route = %route);
    let t0 = Instant::now();
    let peer = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|c| c.0);
    let caller = caller_of(req.headers(), peer, &app.svc.trusted_proxies);
    let ui = caller.command_center();
    let inner = keys::calling(caller, obs::scope(Some(id.clone()), next.run(req)));
    let run = crate::service::DEADLINE.scope(crate::service::Deadline::default(), async move { (inner.await, crate::service::deadline_of_request()) });
    let (mut res, deadline) = telemetry::traced(telemetry::Trace::new(), run).instrument(span.clone()).await;
    let secs = t0.elapsed().as_secs_f64();
    let status = res.status().as_u16();
    // The command center's own requests are not the clients' traffic
    // (`/v1/routes`, the series' HTTP rates).
    if !ui {
        app.svc.metrics.http(&route, status, secs);
    }
    if let Ok(v) = header::HeaderValue::from_str(&id) {
        res.headers_mut().insert(obs::HEADER, v);
    }
    let _in = span.enter();
    match res.extensions().get::<Failure>() {
        // The client's own deadline passed: its choice, not a fault.
        Some(Failure(why)) if deadline.is_some() => {
            static LIMIT: obs::RateLimit = obs::RateLimit::new(10);
            if let Some(held) = LIMIT.admit() {
                let ms = deadline.unwrap_or(0);
                tracing::warn!(status, deadline_ms = ms, error = %why, held_back = held, "client deadline of {ms} ms passed");
            }
        }
        Some(Failure(why)) => obs::server_error(status, why),
        None if route.starts_with("/v1/logs") => {}
        None => tracing::debug!(status, secs, "answered"),
    }
    res
}

fn header_str<'a>(headers: &'a HeaderMap, name: header::HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

// ----- core: who is calling (command center rebuild)

/// A network as `--trust-proxy` names it: an address, or an address and
/// a prefix length (`10.0.0.0/8`, `::1`).
pub fn parse_net(s: &str) -> Result<(std::net::IpAddr, u8), String> {
    let (ip, bits) = match s.trim().split_once('/') {
        Some((ip, b)) => (ip, Some(b)),
        None => (s.trim(), None),
    };
    let ip: std::net::IpAddr = ip.parse().map_err(|_| format!("{s:?} is not an address or a network (10.0.0.0/8)"))?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let bits = match bits {
        Some(b) => b.parse::<u8>().ok().filter(|b| *b <= max).ok_or_else(|| format!("{s:?}: the prefix length is 0 to {max}"))?,
        None => max,
    };
    Ok((ip, bits))
}

/// Whether `ip` is in network `net`.
fn in_net(ip: std::net::IpAddr, net: (std::net::IpAddr, u8)) -> bool {
    use std::net::IpAddr;
    let (bits, a, b) = match (ip, net.0) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (net.1 as u32, u32::from(a) as u128, u32::from(b) as u128),
        (IpAddr::V6(a), IpAddr::V6(b)) => (net.1 as u32, u128::from(a), u128::from(b)),
        _ => return false,
    };
    let width = if ip.is_ipv4() { 32 } else { 128 };
    if bits == 0 {
        return true;
    }
    let shift = width - bits.min(width);
    (a >> shift) == (b >> shift)
}

/// A header's value as text, `%XX` escapes decoded (the command center
/// encodes a reason that is not ASCII).
fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(name)?.to_str().ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).chars().take(1000).collect())
}

/// Who a request is from ([`keys::Caller`]): its address (the peer, or
/// the first of `X-Forwarded-For` when the peer is a proxy in `trusted`),
/// its client (`x-strata-client`), its app (`x-strata-app`, else the
/// product of its `User-Agent`) and its reason (`x-strata-reason`).
pub fn caller_of(headers: &HeaderMap, peer: Option<std::net::SocketAddr>, trusted: &[(std::net::IpAddr, u8)]) -> keys::Caller {
    let mut addr = peer.map(|p| p.ip());
    if let Some(p) = addr.filter(|ip| trusted.iter().any(|n| in_net(*ip, *n))) {
        let forwarded = header_str(headers, header::HeaderName::from_static("x-forwarded-for")).and_then(|v| v.split(',').next()).and_then(|v| v.trim().parse().ok());
        addr = Some(forwarded.unwrap_or(p));
    }
    let client = header_text(headers, "x-strata-client").map(|c| c.to_ascii_lowercase()).filter(|c| c == keys::COMMAND_CENTER);
    let app = header_text(headers, "x-strata-app").or_else(|| header_str(headers, header::USER_AGENT).and_then(keys::product));
    keys::Caller { addr: addr.map(|a| a.to_string()), client, app, reason: header_text(headers, "x-strata-reason") }
}

/// A copy of a coordinator's cluster, by its URL or short name (`s1-0`):
/// its shard, URL, short name, and its shard's primary.
struct CopyOf {
    shard: u64,
    url: String,
    short: String,
    primary: Option<String>,
}

fn copy_of(snap: &Json, node: &str) -> Result<CopyOf, ApiError> {
    for s in snap["shards"].as_array().into_iter().flatten() {
        for r in s["replicas"].as_array().into_iter().flatten() {
            if r["url"] == node || r["short"] == node {
                return Ok(CopyOf {
                    shard: s["shard"].as_u64().unwrap_or(0),
                    url: r["url"].as_str().unwrap_or_default().to_string(),
                    short: r["short"].as_str().unwrap_or(node).to_string(),
                    primary: s["primary"].as_str().map(String::from),
                });
            }
        }
    }
    Err(ApiError { status: 404, message: format!("no copy {node} in this cluster: GET /v1/cluster lists each copy's url and short name") })
}

/// Asks the copy `url` of the cluster to do `kind` (a kind of `Profile`
/// that is not a profile), with `body`.
async fn ask_copy(c: &crate::coord::Coordinator, url: &str, kind: crate::profiling::Kind, body: Json) -> Result<Json, ApiError> {
    let a = crate::profiling::Asked { kind, body, folded: false, in_use: false };
    match c.profile(url, a).await? {
        crate::profiling::Answer::Json(j) => Ok(j),
        crate::profiling::Answer::Folded(..) => Err(ApiError::internal("the copy answered folded lines")),
    }
}

/// `POST /v1/admin/copies/verify {"url", "timeout_ms"?}` on a coordinator:
/// the primary of that copy's shard compares its replicas' rows with its
/// own (as its `POST /v1/admin/verify`), and answers
/// `{replicas: [{url, in_sync, compared, differing_slots,
/// first_differing_slot, seq_no, error}]}`.
async fn copy_verify(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let c = app.svc.coordinator()?;
    let copy = copy_of(&*c.snapshot().await, body_str(&b, "url")?)?;
    let primary = copy.primary.clone().ok_or_else(|| ApiError { status: 409, message: format!("shard {} has no primary up to compare its copies", copy.shard) })?;
    let by = keys::audit_name(&who(w));
    keys::note_change(keys::Change { object: Some(format!("copy {}", copy.short)), after: Some(json!({"url": copy.url, "primary": primary})), ..Default::default() });
    let mut body = json!({"by": by});
    if let Some(t) = b.get("timeout_ms").and_then(Json::as_u64) {
        body["timeout_ms"] = json!(t);
    }
    let j = ask_copy(c, &primary, crate::profiling::Kind::Verify, body).await?;
    Ok(audited(j, format!("verified shard {}'s copies against its primary, for {}", copy.shard, copy.short)))
}

/// `POST /v1/admin/copies/checkpoint {"url", "name", "layout"?,
/// "link_from"?}` on a coordinator: that copy writes a checkpoint of its
/// database into its own `--backup-dir` (as its
/// `POST /v1/admin/checkpoint`), and answers `{checkpoint}`.
async fn copy_checkpoint(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let c = app.svc.coordinator()?;
    let copy = copy_of(&*c.snapshot().await, body_str(&b, "url")?)?;
    let name = body_str(&b, "name")?.to_string();
    let layout = b["layout"].as_str().unwrap_or("file").to_string();
    let by = keys::audit_name(&who(w));
    keys::note_change(keys::Change { object: Some(format!("copy {}", copy.short)), after: Some(json!({"url": copy.url, "checkpoint": name, "layout": layout})), ..Default::default() });
    let body = json!({"name": name, "layout": layout, "link_from": b["link_from"], "by": by});
    let j = ask_copy(c, &copy.url, crate::profiling::Kind::Checkpoint, body).await?;
    Ok(audited(j, format!("checkpoint {name} of {}", copy.short)))
}

/// `GET /v1/index/tables` ([`crate::inspect::tables`]).
async fn index_tables(State(app): State<App>) -> Res {
    Ok(AxJson(crate::inspect::tables(&app.svc).await?))
}

/// `GET /v1/health/fleet` ([`crate::fleet`]).
async fn fleet_health(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.fleet_health().await)
}

/// An event row of the log store's `events`, as `GET /v1/events` has it.
fn event_of_row(r: &Json) -> Json {
    let parsed = |k: &str| r[k].as_str().and_then(|t| serde_json::from_str::<Json>(t).ok());
    let mut e = json!({"id": r["id"], "ts_ms": r["ts_ms"], "kind": r["kind"], "severity": r["severity"], "title": r["title"], "node": r["node"]});
    for (k, v) in [("detail", parsed("detail")), ("where", parsed("place")), ("who", r.get("who").cloned()), ("until_ms", r.get("until_ms").cloned())] {
        if let Some(v) = v.filter(|v| !v.is_null()) {
            e[k] = v;
        }
    }
    e
}

/// `GET /v1/events?from=&to=&kind=a,b&limit=` ([`crate::events`]): the
/// events from `from` to `to` (ms; the last 24 hours by default), the
/// newest first; from the log store for what memory no longer holds. Who
/// did what is the admin's to read.
async fn events_list(State(app): State<App>, w: Option<axum::Extension<Principal>>, Query(q): Params) -> Res {
    let now = telemetry::now_ms();
    let to = param_u64(&q, "to", now)?;
    let from = param_u64(&q, "from", to.saturating_sub(86_400_000))?;
    let limit = param_u64(&q, "limit", 500)?.clamp(1, 5000) as usize;
    let kinds: Vec<String> = q.get("kind").map(|k| k.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    if kinds.iter().any(|k| !k.chars().all(|c| c.is_ascii_lowercase() || c == '_')) {
        return Err(ApiError::bad("kind is a list of event kinds, such as kind=copy_down,primary_changed"));
    }
    let (kept, oldest) = app.svc.events.range(from, to, &kinds, limit);
    let mut events: Vec<Json> = kept.iter().map(|r| json!(**r)).collect();
    let mut source = vec!["memory"];
    let mut error = None;
    // What memory no longer holds (or never held, before a restart).
    let held = app.svc.events.held_from();
    if let (Some(store), true) = (&app.svc.log_store, from < held) {
        let and = (!kinds.is_empty()).then(|| format!("kind IN ({})", kinds.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ")));
        match store.kept_rows(crate::logstore::EVENTS, from, held.min(to), and, limit).await {
            Ok(rows) => {
                let ids: std::collections::HashSet<i64> = kept.iter().map(|r| r.id).collect();
                events.extend(rows.iter().filter(|r| r["id"].as_i64().is_none_or(|id| !ids.contains(&id))).map(event_of_row));
                source.push("log_store");
            }
            Err(e) => error = Some(format!("the log store did not answer: {}", e.message)),
        }
    }
    events.sort_by(|a, b| b["ts_ms"].as_u64().cmp(&a["ts_ms"].as_u64()).then(b["id"].as_i64().cmp(&a["id"].as_i64())));
    events.truncate(limit);
    if who(w).may(Scope::Admin, "").is_err() {
        for e in &mut events {
            if let Some(o) = e.as_object_mut() {
                o.remove("who");
            }
        }
    }
    let mut j = json!({"events": events, "from_ms": from, "to_ms": to, "source": source, "memory_from_ms": held, "oldest_in_memory_ms": oldest});
    if let Some(e) = error {
        j["error"] = json!(e);
    }
    Ok(AxJson(j))
}

/// `POST /v1/events/notes {"text", "ts_ms"?, "where"?}`: a note on the
/// timeline, by whoever sent it, audited.
async fn events_note(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Res {
    let text = b["text"].as_str().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| ApiError::bad("a note needs its \"text\""))?;
    if text.chars().count() > 2000 {
        return Err(ApiError::bad("a note is 2000 characters at most"));
    }
    let now = telemetry::now_ms();
    let ts = match b.get("ts_ms") {
        None | Some(Json::Null) => now,
        Some(v) => v.as_u64().filter(|t| *t <= now + 60_000).ok_or_else(|| ApiError::bad("ts_ms is a time in ms since 1970, not one to come"))?,
    };
    let where_ = match b.get("where") {
        None | Some(Json::Null) => Json::Null,
        Some(Json::Object(m)) => {
            let ok = m.iter().all(|(k, v)| match k.as_str() {
                "shard" => v.is_u64(),
                "copy" | "table" | "rule" => v.is_string(),
                _ => false,
            });
            if !ok {
                return Err(ApiError::bad("where is {\"shard\"?: n, \"copy\"?, \"table\"?, \"rule\"?}"));
            }
            Json::Object(m.clone())
        }
        Some(_) => return Err(ApiError::bad("where is an object: {\"shard\"?, \"copy\"?, \"table\"?, \"rule\"?}")),
    };
    let by = keys::audit_name(&who(w));
    let e = crate::events::Event { kind: "note".into(), severity: "info".into(), title: text.to_string(), detail: Json::Null, where_, who: Some(by) };
    let r = app.svc.events.add_at(e, ts, None);
    keys::note_change(keys::Change { kind: Some(keys::NOTE_KIND), object: Some("timeline".into()), after: Some(json!({"text": text, "ts_ms": ts})), ..Default::default() });
    keys::note(format!("noted on the timeline: {}", text.chars().take(100).collect::<String>()));
    Ok(AxJson(json!({"event": *r})))
}

// ----- end core

/// Who the request is from ([`crate::keys`]), and whether that may call
/// its route ([`route_scope`]); what the request then does (a statement, a
/// plan, a logged query, an import that creates tables) is checked by the
/// service, as for gRPC. Requests that need the admin scope, refused ones,
/// and ones the service notes ([`keys::note`]) go to the audit log.
async fn auth(State(app): State<App>, headers: HeaderMap, mut req: Request, next: Next) -> Response {
    let t0 = Instant::now();
    let route = req.extensions().get::<MatchedPath>().map_or("other", |p| p.as_str()).to_string();
    let method = req.method().clone();
    let who = app.svc.keys.resolve(app.token.as_ref(), app.cluster.as_ref(), header_str(&headers, header::AUTHORIZATION));
    let need = route_scope(&method, &route);
    let refused = match &who {
        None => Some(ApiError { status: 401, message: "missing or wrong token".into() }),
        Some(w) => w.may(need, &format!("{method} {route}")).err(),
    };
    let (res, note, change) = match refused {
        Some(e) => (e.into_response(), None, keys::Change::default()),
        None => {
            if let Some(w) = &who {
                req.extensions_mut().insert(w.clone());
            }
            // Who the request's queries are from ([`telemetry::Caller`]).
            let caller = telemetry::Caller::of(&who.clone().unwrap_or_else(Principal::open), keys::caller().as_ref());
            telemetry::calling(Arc::new(caller), keys::noting_change(next.run(req))).await
        }
    };
    let status = res.status().as_u16();
    // What the service noted of the request ([`keys::note`]), or its
    // handler.
    let detail = note.or_else(|| res.extensions().get::<Audited>().map(|a| a.0.clone()));
    // Changes and refusals: a read answered (GET, HEAD) is not audited,
    // unless something noted it.
    let refusal = status == 401 || status == 403;
    // A plan asked (`POST .../plan`) changes nothing: a read too.
    let plan = matches!(route.as_str(), "/v1/admin/plan" | "/v1/admin/kube/plan");
    let read = (method == axum::http::Method::GET || method == axum::http::Method::HEAD || plan) && status < 400;
    let caller = keys::caller().unwrap_or_default();
    if refusal || detail.is_some() || (need == Scope::Admin && !read) {
        let kind = if refusal { keys::REFUSAL } else { change.kind.unwrap_or(keys::CHANGE) };
        // A refused credential that is a key known here (expired, revoked):
        // its name and id, so that Access names the client still sending it.
        let known = match &who {
            None => app.svc.keys.identify(header_str(&headers, header::AUTHORIZATION)),
            Some(_) => None,
        };
        let detail = match (&known, detail) {
            (Some((_, name, why)), None) => Some(format!("refused: the key {name} is {why}")),
            (_, d) => d,
        };
        app.svc.audit_add(keys::Entry {
            ts_ms: telemetry::now_ms(),
            who: who.as_ref().map_or_else(|| known.as_ref().map_or("unknown".into(), |k| k.1.clone()), keys::audit_name),
            key: who.as_ref().and_then(|w| w.key.clone()).or_else(|| known.as_ref().map(|k| k.0.clone())),
            scope: who.as_ref().map(|w| w.scope.name()),
            method: method.to_string(),
            route: route.clone(),
            status,
            ms: t0.elapsed().as_secs_f64() * 1e3,
            request_id: obs::current().map(|id| id.to_string()),
            detail,
            kind: kind.into(),
            addr: caller.addr.clone(),
            client: caller.named(),
            reason: caller.reason.clone(),
            object: change.object,
            before: change.before,
            after: change.after,
            actor: change.actor.unwrap_or("person").into(),
        });
    }
    // A client's mistake (4xx), in one line at info, ten a second at most.
    if (400..500).contains(&status) {
        static LIMIT: obs::RateLimit = obs::RateLimit::new(10);
        if let (Some(e), Some(held)) = (res.extensions().get::<ClientError>(), LIMIT.admit()) {
            let key = who.as_ref().map(keys::audit_name);
            tracing::info!(route = %route, status, error = %e.0, key = key.as_deref().unwrap_or("unknown"), held_back = held, "client error");
        }
    }
    res
}

/// Why a request failed on the client's side (4xx), for [`auth`] to log.
#[derive(Clone)]
struct ClientError(Arc<str>);

/// What the audit log says a request did, set on its answer.
#[derive(Clone)]
struct Audited(String);

/// The answer `j`, with what the audit log says of it.
fn audited(j: Json, detail: String) -> Response {
    let mut r = AxJson(j).into_response();
    r.extensions_mut().insert(Audited(detail));
    r
}

/// The scope a route needs ([`crate::keys`]). What shows other clients'
/// SQL (the running and latest queries, logs) is the admin's.
fn route_scope(method: &axum::http::Method, route: &str) -> Scope {
    use axum::http::Method;
    match route {
        "/v1/write" | "/v1/import" | "/v1/stream" | "/v1/agent" | "/v1/judge" => Scope::Write,
        "/v1/kv/:table/:key" if method == Method::PUT || method == Method::DELETE => Scope::Write,
        "/v1/export" => Scope::Admin,
        "/v1/synonyms" if method == Method::PUT => Scope::Admin,
        "/v1/alerts/rules" => Scope::Admin,
        // alerts (alerts agent): acknowledging, silencing and noting write.
        "/v1/alerts/:rule/ack" | "/v1/alerts/:rule/silence" | "/v1/alerts/:rule/notes" => Scope::Write,
        "/v1/events/notes" => Scope::Admin,
        // The inspector names the keys most read.
        r if r.starts_with("/v1/admin/") || r.starts_with("/v1/queries") || r.starts_with("/v1/logs") || r.starts_with("/v1/profile") || r.starts_with("/v1/index") => Scope::Admin,
        _ => Scope::Read,
    }
}

/// Who a request is from (the open server's root when no auth ran).
fn who(req_who: Option<axum::Extension<Principal>>) -> Principal {
    req_who.map_or_else(Principal::open, |axum::Extension(p)| p)
}

/// Keeps out pages on other sites: one that reached this server through
/// DNS rebinding (a `Host` it does not answer to), and one that posts to
/// it (an `Origin` other than its own), which a browser lets any page do
/// without a token.
async fn browsers(State(app): State<App>, headers: HeaderMap, req: Request, next: Next) -> Response {
    let safe = req.method().is_safe();
    let host = header_str(&headers, header::HOST).map(str::to_string).or_else(|| req.uri().authority().map(|a| a.to_string()));
    if let Some(why) = auth::browser_check(app.hosts.as_deref(), host.as_deref(), header_str(&headers, header::ORIGIN), safe) {
        return ApiError { status: 403, message: why }.into_response();
    }
    next.run(req).await
}

fn body_str<'a>(b: &'a Json, k: &str) -> Result<&'a str, ApiError> {
    b.get(k).and_then(|v| v.as_str()).ok_or_else(|| ApiError::bad(format!("the body needs a string {k:?}")))
}

fn body_u64(b: &Json, k: &str) -> u64 {
    b.get(k).and_then(|v| v.as_u64()).unwrap_or(0)
}

/// The writes a read must see ([`After`]): the body's `min_seq_no`, the
/// sequence number a server's write answered, or `min_seq_nos`, the
/// numbers a coordinator's answered, one per shard.
fn after(b: &Json) -> Result<After, ApiError> {
    let seq = |v: &Json, what: &str| v.as_u64().ok_or_else(|| ApiError::bad(format!("{what} is a sequence number: a whole number from 0")));
    let min = match b.get("min_seq_no") {
        None | Some(Json::Null) => 0,
        Some(v) => seq(v, "min_seq_no")?,
    };
    let mins = match b.get("min_seq_nos") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(a)) => a.iter().map(|v| seq(v, "each of min_seq_nos")).collect::<Result<_, _>>()?,
        Some(_) => return Err(ApiError::bad("min_seq_nos is a list of sequence numbers, one per shard, as a coordinator's write answers seq_nos")),
    };
    After::of(min, mins)
}

async fn health(State(app): State<App>) -> Response {
    let h = app.svc.health().await;
    let code = if matches!(h["status"].as_str(), Some("down" | "failing")) { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
    (code, AxJson(h)).into_response()
}

/// Liveness and readiness: 503 when the server cannot write until it is
/// restarted ([`Service::failing`]). Neither asks a coordinator's shards,
/// so a slow shard cannot take every coordinator out of service at once;
/// `/v1/health` shows them.
async fn live(State(app): State<App>) -> Response {
    match app.svc.failing() {
        Some(why) => (StatusCode::SERVICE_UNAVAILABLE, AxJson(json!({"status": "failing", "error": why}))).into_response(),
        None => AxJson(json!({"status": "ok"})).into_response(),
    }
}

/// Live, and not stopping: a server serving out its drain time
/// ([`crate::server::draining`]) answers 503, so that a load balancer
/// sends it no more clients.
async fn ready(state: State<App>) -> Response {
    if crate::server::draining() {
        return (StatusCode::SERVICE_UNAVAILABLE, AxJson(json!({"status": "draining", "error": "the server is stopping: it ends the requests under way and takes no new ones"}))).into_response();
    }
    live(state).await
}

/// While the server drains, each answer closes its connection: the
/// client's next request opens another, which a load balancer sends to a
/// server that stays.
async fn close_when_draining(mut r: Response) -> Response {
    if crate::server::draining() {
        r.headers_mut().insert(axum::http::header::CONNECTION, axum::http::HeaderValue::from_static("close"));
    }
    r
}

async fn stats(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.stats().await)
}

/// A table's schema: `?table=`, else the first table's.
async fn schema(State(app): State<App>, Query(q): Query<std::collections::HashMap<String, String>>) -> Res {
    let s = match q.get("table") {
        Some(t) => app.svc.table(t).await?,
        None => app.svc.first_table(),
    };
    Ok(AxJson(table_json(&app.svc, &s)))
}

/// Every table's schema, the first first.
async fn tables(State(app): State<App>) -> AxJson<Json> {
    let tables: Vec<Json> = app.svc.catalog().iter().map(|s| table_json(&app.svc, s)).collect();
    AxJson(json!({"tables": tables}))
}

/// `GET /v1/index?table=&sample=&seed=&terms=&top=`: the index
/// inspector's statistics of a table ([`crate::inspect`]): this server's,
/// or a coordinator's per replica, per shard and for its cluster.
async fn index(State(app): State<App>, Query(q): Params) -> Res {
    let a = crate::inspect::Asked::of(&json!(q))?;
    Ok(AxJson(crate::inspect::answer(&app.svc, a).await?))
}

/// `POST /v1/index/reads/reset`: this server's read counts start again
/// ([`crate::reads`]).
async fn reset_reads(State(app): State<App>) -> AxJson<Json> {
    app.svc.reads.reset();
    AxJson(json!({"reset": true, "at_ms": crate::telemetry::now_ms()}))
}

/// `GET /v1/tables/:table/paths?field=&path=&where=`: a JSON field's
/// paths listing ([`strata::query::paths`]).
async fn json_paths(State(app): State<App>, axum::extract::Path(table): axum::extract::Path<String>, Query(q): Params) -> Res {
    let t0 = Instant::now();
    let field = q.get("field").filter(|f| !f.is_empty()).ok_or_else(|| ApiError::bad("name the JSON field: ?field=attrs"))?;
    let prefix = q.get("path").or_else(|| q.get("prefix")).filter(|p| !p.is_empty() && *p != "$");
    let call = strata::query::paths::Call {
        table,
        field: field.clone(),
        prefix: prefix.map(|p| strata::json::parse_path(p)).transpose()?,
        filter: q.get("where").filter(|w| !w.trim().is_empty()).cloned(),
    };
    let paths = app.svc.json_paths(&call, &strata::query::Params::new(), &crate::service::After::Any, param_u64(&q, "timeout_ms", 0)?).await?;
    let mut j = paths.to_json();
    j["table"] = json!(call.table);
    j["field"] = json!(call.field);
    j["took_ms"] = json!(t0.elapsed().as_secs_f64() * 1e3);
    Ok(AxJson(j))
}

fn table_json(svc: &crate::service::Service, s: &strata::schema::Schema) -> Json {
    json!({
        "table": s.table,
        "schema": s,
        "short": s.to_short_json(),
        // The CREATE TABLE statement that makes it.
        "sql": strata::query::ddl::create_sql(s),
        "primary_key": s.primary_key.map(|k| s.fields[k].name.clone()),
        "shard_by": s.shard_by,
        // Its rank profiles, by name.
        "profiles": svc.rank_profiles().into_iter().filter(|(t, _)| t.eq_ignore_ascii_case(&s.table)).map(|(_, p)| p).collect::<Vec<_>>(),
        // Each embedded VECTOR field: its sources, model, and whether this
        // server can embed text for it.
        "embeddings": svc.models().iter().filter(|m| m.table.eq_ignore_ascii_case(&s.table)).map(|m| json!({
            "field": m.field, "from": m.spec.from, "model": m.spec.model,
            "loaded": m.model.is_ok(), "error": m.model.as_ref().err(),
            // A model behind an API: the requests it sent.
            "usage": m.model.as_ref().ok().and_then(|e| e.usage()).map(|u| json!({"requests": u.requests, "retries": u.retries, "failures": u.failures, "texts": u.texts, "tokens": u.tokens})),
            // Rows stored without a vector, filled in once the model opened.
            "backfill": svc.backfill_of(&m.table, &m.field).map(|b| json!({"state": b.state, "missing": b.missing, "embedded": b.embedded, "error": b.error})),
        })).collect::<Vec<_>>(),
        // Each derived TEXT field: its MEDIA field, model, and whether this
        // server makes its text (null on a coordinator: its shards do).
        "derived": s.fields.iter().filter_map(|f| f.derive.as_ref().map(|d| json!({
            "field": f.name, "from": d.from, "model": d.model,
            "loaded": svc.local_db().map(|db| db.deriver_in(&s.table, &f.name).is_some()),
        }))).collect::<Vec<_>>(),
    })
}

async fn config(State(app): State<App>) -> AxJson<Json> {
    let a = &app.svc.agent;
    let rules = app.svc.synonyms().map_or(0, |s| s.rules());
    AxJson(json!({"agent": {"model": a.model, "server_key": a.api_key.is_some(), "models": agent::MODELS}, "synonyms": {"rules": rules, "editable": app.svc.local_db().is_some()}}))
}

async fn metrics(State(app): State<App>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], app.svc.prometheus().await)
}

async fn query(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Res {
    let stats = match b.get("stats") {
        None | Some(Json::Null) => None,
        Some(s) => Some(serde_json::from_value(s.clone()).map_err(|e| ApiError::bad(format!("bad stats: {e}")))?),
    };
    let req = QueryReq {
        sql: body_str(&b, "sql")?.to_string(),
        params: app.svc.params(b.get("params")).await?,
        timeout_ms: body_u64(&b, "timeout_ms"),
        shard: b.get("shard").and_then(|v| v.as_bool()).unwrap_or(false),
        stats,
        snapshot: body_u64(&b, "snapshot"),
        partial: b.get("partial").and_then(|v| v.as_bool()),
        after: after(&b)?,
        profile: b.get("profile").and_then(|v| v.as_bool()) == Some(true),
        ..Default::default()
    };
    if let Some(log) = b.get("log") {
        if req.shard {
            return Err(ApiError::bad("a shard's query is not logged: log through the coordinator"));
        }
        return logged(&app, &who(w), req, log).await;
    }
    let a = app.svc.query(req).await?;
    let mut j = answer(&a);
    // `"trace": true`: the timed steps the request took; `"profile":
    // true` answers them too, beside its steps' CPU time.
    if b.get("trace").and_then(|v| v.as_bool()) == Some(true) || b.get("profile").and_then(|v| v.as_bool()) == Some(true) {
        j["trace"] = json!(telemetry::trace().map(|t| t.spans()).unwrap_or_default());
    }
    Ok(AxJson(j))
}

/// A query run and logged ([`Service::query_logged`]): its answer, with
/// the request's id (`"request"`), an interleaved query's teams
/// (`"teams"`: per row, the ranking that placed it) and the log's write
/// (`"logged"`: the rows, and the write's sequence numbers as a write's
/// answer gives them).
async fn logged(app: &App, who: &Principal, req: QueryReq, log: &Json) -> Res {
    let log = strata::rank_log::Log::from_json(log)?;
    let (a, served, w) = app.svc.query_logged(who, req, log).await?;
    let mut j = answer(&a);
    j["request"] = json!(served.request);
    if !served.teams.is_empty() {
        j["teams"] = json!(served.teams);
    }
    let mut l = json!({"rows": w.rows});
    if let Some(s) = w.seq_no {
        l["seq_no"] = json!(s);
    }
    if !w.seq_nos.is_empty() {
        l["seq_nos"] = json!(w.seq_nos);
    }
    j["logged"] = l;
    Ok(AxJson(j))
}

/// The records of a log table ([`strata::rank_log::export`]) as NDJSON,
/// read from one snapshot of a local database as they are sent. A failure
/// after the first record ends the answer with `{"error": ..., "status":
/// ..}`. A coordinator refuses it: each shard's server exports its own
/// rows, which hold every row of a context when the log and the events
/// are placed by it (`shard_by`).
async fn export_rank_log(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let x = strata::rank_log::Export::from_json(&b)?;
    let db = app.svc.local_db().cloned().ok_or_else(|| ApiError::bad("a coordinator does not export a rank log: export from each shard's server"))?;
    // Refused before the answer starts.
    strata::rank_log::check(&*app.svc.table(&x.table).await?)?;
    let (tx, rx) = tokio::sync::mpsc::channel::<Json>(1024);
    tokio::task::spawn_blocking(move || {
        let r = db.export_rank_log(&x, |r| tx.blocking_send(r).map_err(|_| strata::Error::Query("the export's client left".into())));
        if let Err(e) = r {
            let e = ApiError::from(e);
            let _ = tx.blocking_send(json!({"error": e.message, "status": e.status}));
        }
    });
    let lines = tokio_stream::wrappers::ReceiverStream::new(rx).map(|j| Ok::<_, std::convert::Infallible>(Bytes::from(format!("{j}\n"))));
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], axum::body::Body::from_stream(lines)).into_response())
}

/// The credit of each pair of interleaved rankings in a log table
/// ([`strata::rank_log::Credits`]) over an export of it (the body is
/// `/v1/rank_log/export`'s), from a local database as the export is.
async fn interleaving(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let x = strata::rank_log::Export::from_json(&b)?;
    let db = app.svc.local_db().cloned().ok_or_else(|| ApiError::bad("a coordinator does not export a rank log: ask each shard's server"))?;
    strata::rank_log::check(&*app.svc.table(&x.table).await?)?;
    let t0 = std::time::Instant::now();
    let pairs = tokio::task::spawn_blocking(move || db.interleaving_credit(&x)).await.map_err(ApiError::internal)??;
    Ok(AxJson(json!({"pairs": pairs.iter().map(|c| c.to_json()).collect::<Vec<_>>(), "took_ms": t0.elapsed().as_secs_f64() * 1e3})))
}

/// A query's answer; one a coordinator gave without some shards says
/// which (`"partial": {"missing_shards": [...]}`).
fn answer(a: &crate::service::Answer) -> Json {
    let mut j = js::result(&a.rs, a.took_ms);
    if !a.missing.is_empty() {
        j["partial"] = json!({"missing_shards": a.missing});
    }
    j
}

/// Any statement: the service checks that the request's credential may
/// run it ([`Service::sql`]), and audits a table's or rank profile's.
async fn sql(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Res {
    let params = app.svc.params(b.get("params")).await?;
    let w = who(w);
    if let Some(log) = b.get("log") {
        let req = QueryReq { sql: body_str(&b, "sql")?.to_string(), params, timeout_ms: body_u64(&b, "timeout_ms"), after: after(&b)?, ..Default::default() };
        return logged(&app, &w, req, log).await;
    }
    match app.svc.sql(&w, body_str(&b, "sql")?, &params, after(&b)?, body_u64(&b, "timeout_ms")).await? {
        SqlAnswer::Rows(a) => Ok(AxJson(answer(&a))),
        SqlAnswer::Written { w, took_ms } => {
            audit_ui_write(body_str(&b, "sql")?, w.rows as u64);
            Ok(AxJson(written(w, took_ms)))
        }
    }
}

/// A write the command center ran (`x-strata-client: command-center`),
/// noted for the audit log as a change: its statement (the first 2 KB),
/// its table and the rows it wrote.
fn audit_ui_write(statement: &str, rows: u64) {
    if !keys::caller().is_some_and(|c| c.command_center()) {
        return;
    }
    let mut end = statement.len().min(2048);
    while !statement.is_char_boundary(end) {
        end -= 1;
    }
    let table = table_written(statement);
    keys::note_change(keys::Change {
        object: table.as_ref().map(|t| format!("table {t}")),
        after: Some(json!({"statement": &statement[..end], "table": table, "rows": rows})),
        ..Default::default()
    });
    keys::note(format!("wrote {rows} rows{}: {}", table.as_ref().map_or(String::new(), |t| format!(" of {t}")), statement.chars().take(200).collect::<String>()));
}

/// The table a statement writes: what follows INSERT INTO, UPDATE, DELETE
/// FROM, or a table's CREATE, DROP or ALTER.
fn table_written(sql: &str) -> Option<String> {
    let words: Vec<&str> = sql.split_whitespace().take(6).collect();
    let up: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();
    let at = match up.first().map(String::as_str)? {
        "INSERT" | "REPLACE" | "UPSERT" => up.iter().position(|w| w == "INTO")? + 1,
        "UPDATE" => 1,
        "DELETE" => up.iter().position(|w| w == "FROM")? + 1,
        "CREATE" | "DROP" | "ALTER" | "TRUNCATE" => {
            let t = up.iter().position(|w| w == "TABLE")? + 1;
            t + up[t..].iter().take_while(|w| matches!(w.as_str(), "IF" | "NOT" | "EXISTS")).count()
        }
        _ => return None,
    };
    let name: String = words.get(at)?.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect();
    (!name.is_empty()).then_some(name)
}

/// A write's answer: the rows written, and the transaction's sequence
/// number in a database that numbers its writes, or through a
/// coordinator each shard's ([`Written`]).
fn written(w: Written, took_ms: f64) -> Json {
    let mut j = json!({"written": w.rows, "took_ms": took_ms});
    if w.skipped > 0 {
        j["skipped"] = json!(w.skipped);
    }
    if let Some(s) = w.seq_no {
        j["seq_no"] = json!(s);
    }
    if !w.seq_nos.is_empty() {
        j["seq_nos"] = json!(w.seq_nos);
    }
    j
}

/// Runs a plan ([`strata::query::ir`]) as `/v1/sql` runs a statement.
async fn run_plan(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Res {
    let plan = strata::query::ir::Plan::from_json(b.get("plan").ok_or_else(|| ApiError::bad("the body needs a \"plan\""))?)?;
    match app.svc.run_plan(&who(w), plan, after(&b)?, body_u64(&b, "timeout_ms")).await? {
        SqlAnswer::Rows(a) => Ok(AxJson(answer(&a))),
        SqlAnswer::Written { w, took_ms } => Ok(AxJson(written(w, took_ms))),
    }
}

/// The body's `ops`, applied once per idempotency key (the body's
/// `idempotency_key` or an `Idempotency-Key` header).
async fn write(State(app): State<App>, headers: HeaderMap, AxJson(mut b): AxJson<Json>) -> Res {
    let ops = match b.get_mut("ops").map(Json::take) {
        Some(Json::Array(ops)) => ops,
        _ => return Err(ApiError::bad("the body needs an array \"ops\"")),
    };
    let key = match (b.get("idempotency_key"), headers.get("idempotency-key")) {
        (Some(Json::String(k)), _) => Some(k.clone()),
        (Some(_), _) => return Err(ApiError::bad("idempotency_key is a string")),
        (None, Some(h)) => Some(h.to_str().map_err(|_| ApiError::bad("the Idempotency-Key header is not text"))?.to_string()),
        (None, None) => None,
    };
    if key.as_ref().is_some_and(|k| k.is_empty() || k.len() > 256) {
        return Err(ApiError::bad("an idempotency key has 1 to 256 characters"));
    }
    let t0 = std::time::Instant::now();
    let shown = keys::caller().is_some_and(|c| c.command_center()).then(|| Json::Array(ops.clone()).to_string());
    let w = app.svc.write_ops(ops, key).await?;
    if let Some(ops) = shown {
        audit_ui_write(&ops, w.rows as u64);
    }
    Ok(AxJson(written(w, t0.elapsed().as_secs_f64() * 1e3)))
}

/// The writes a read by key must see, from its query string
/// (`min_seq_no=n`, or `min_seq_nos=a,b,c`).
fn after_of_query(q: &std::collections::HashMap<String, String>) -> Result<After, ApiError> {
    let mut b = json!({});
    if let Some(v) = q.get("min_seq_no") {
        b["min_seq_no"] = json!(v.parse::<u64>().map_err(|_| ApiError::bad("min_seq_no is a sequence number: a whole number from 0"))?);
    }
    if let Some(v) = q.get("min_seq_nos") {
        let ns: Vec<u64> = v.split(',').map(|x| x.trim().parse::<u64>()).collect::<Result<_, _>>().map_err(|_| ApiError::bad("min_seq_nos is sequence numbers joined by commas, one per shard"))?;
        b["min_seq_nos"] = json!(ns);
    }
    after(&b)
}

/// A row read by key as JSON: an object of its columns.
fn key_row(columns: &[String], row: &[strata::value::Value]) -> Json {
    Json::Object(columns.iter().zip(row).map(|(c, v)| (c.clone(), js::to_json(v))).collect())
}

/// The key in a path as the table's key: an INT key's digits a number.
fn path_key(schema: &strata::schema::Schema, key: &str) -> Result<Json, ApiError> {
    let pk = schema.primary_key.ok_or_else(|| ApiError::bad(format!("{} has no primary key: rows are read and written by key only in a table with one", schema.table)))?;
    Ok(match schema.fields[pk].ty {
        strata::schema::FieldType::Int => json!(key.parse::<i64>().map_err(|_| ApiError::bad(format!("{key:?} is not a key of {}: its key {} is an INT", schema.table, schema.fields[pk].name)))?),
        _ => json!(key),
    })
}

/// `GET /v1/kv/:table/:key`: one row by its key, without SQL.
async fn kv_one(State(app): State<App>, axum::extract::Path((table, key)): axum::extract::Path<(String, String)>, Query(q): Query<std::collections::HashMap<String, String>>) -> Result<Response, ApiError> {
    let t0 = Instant::now();
    let timeout_ms = q.get("timeout_ms").and_then(|v| v.parse().ok()).unwrap_or(0);
    let got = app.svc.get_rows(Some(&table), vec![strata::value::Value::Str(key.clone())], &after_of_query(&q)?, timeout_ms, true).await?;
    let took_ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(match got.rows.into_iter().next().flatten() {
        Some((row, seq_no)) => AxJson(json!({"table": got.table, "key": key, "found": true, "row": key_row(&got.columns, &row), "seq_no": seq_no, "took_ms": took_ms})).into_response(),
        None => (StatusCode::NOT_FOUND, AxJson(json!({"table": got.table, "key": key, "found": false, "took_ms": took_ms}))).into_response(),
    })
}

/// `POST /v1/kv/get`: rows by their keys (MGET), without SQL.
async fn kv_get(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let t0 = Instant::now();
    let keys = match b.get("keys") {
        Some(Json::Array(k)) => k.iter().map(js::param).collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ApiError::bad("the body needs an array \"keys\"")),
    };
    let table = match b.get("table") {
        None | Some(Json::Null) => None,
        Some(Json::String(t)) => Some(t.as_str()),
        Some(_) => return Err(ApiError::bad("table is a table's name")),
    };
    let got = app.svc.get_rows(table, keys, &after(&b)?, body_u64(&b, "timeout_ms"), true).await?;
    let seq_nos: Vec<Json> = got.rows.iter().map(|r| r.as_ref().map_or(Json::Null, |(_, s)| json!(s))).collect();
    let rows: Vec<Json> = got.rows.iter().map(|r| r.as_ref().map_or(Json::Null, |(row, _)| key_row(&got.columns, row))).collect();
    Ok(AxJson(json!({"table": got.table, "columns": got.columns, "rows": rows, "seq_nos": seq_nos, "took_ms": t0.elapsed().as_secs_f64() * 1e3})))
}

/// The idempotency key of a write: the `Idempotency-Key` header.
fn idempotency(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let key = headers.get("idempotency-key").map(|h| h.to_str().map(str::to_string).map_err(|_| ApiError::bad("the Idempotency-Key header is not text"))).transpose()?;
    if key.as_ref().is_some_and(|k| k.is_empty() || k.len() > 256) {
        return Err(ApiError::bad("an idempotency key has 1 to 256 characters"));
    }
    Ok(key)
}

/// `if_seq_no` from a query string, for a write by key.
fn if_seq_no(op: &mut Json, q: &std::collections::HashMap<String, String>) -> Result<(), ApiError> {
    if let Some(v) = q.get("if_seq_no") {
        op["if_seq_no"] = json!(v.parse::<u64>().map_err(|_| ApiError::bad("if_seq_no is a sequence number: a whole number from 0"))?);
    }
    Ok(())
}

/// `PUT /v1/kv/:table/:key`: writes the row with that key (an upsert),
/// without SQL.
async fn kv_put(
    State(app): State<App>,
    axum::extract::Path((table, key)): axum::extract::Path<(String, String)>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    AxJson(mut doc): AxJson<Json>,
) -> Res {
    let t0 = Instant::now();
    let schema = app.svc.table(&table).await?;
    let k = path_key(&schema, &key)?;
    let pk = schema.fields[schema.primary_key.expect("path_key checked it")].name.clone();
    let Json::Object(m) = &mut doc else { return Err(ApiError::bad("the body is the row's fields, as an object")) };
    match m.iter().find(|(f, _)| f.eq_ignore_ascii_case(&pk)) {
        Some((_, v)) if *v != k => return Err(ApiError::bad(format!("the body's {pk} is {v}, and the path's key {k}"))),
        Some(_) => {}
        None => {
            m.insert(pk, k);
        }
    }
    let mut op = json!({"op": "upsert", "table": schema.table, "doc": doc});
    if_seq_no(&mut op, &q)?;
    let w = app.svc.write_ops(vec![op], idempotency(&headers)?).await?;
    Ok(AxJson(written(w, t0.elapsed().as_secs_f64() * 1e3)))
}

/// `DELETE /v1/kv/:table/:key`: deletes the row with that key.
async fn kv_delete(
    State(app): State<App>,
    axum::extract::Path((table, key)): axum::extract::Path<(String, String)>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> Res {
    let t0 = Instant::now();
    let schema = app.svc.table(&table).await?;
    let mut op = json!({"op": "delete", "table": schema.table, "key": path_key(&schema, &key)?});
    if_seq_no(&mut op, &q)?;
    if let Some(r) = q.get("routing") {
        op["routing"] = json!(r);
    }
    let w = app.svc.write_ops(vec![op], idempotency(&headers)?).await?;
    let mut j = written(w, t0.elapsed().as_secs_f64() * 1e3);
    j["deleted"] = json!(j["written"].as_u64().unwrap_or(0) > 0);
    Ok(AxJson(j))
}

/// Rows in the body, in the format `?format=` names or the content type
/// implies. The content type must not be one a page on another site may
/// send without asking first (text/plain, a form, or none).
async fn import(State(app): State<App>, w: Option<axum::Extension<Principal>>, Query(q): Query<std::collections::HashMap<String, String>>, headers: HeaderMap, body: Bytes) -> Res {
    if auth::simple_content_type(header_str(&headers, header::CONTENT_TYPE)) {
        return Err(ApiError {
            status: 415,
            message: "send the rows with their content type (text/csv, text/tab-separated-values, application/x-ndjson, application/json, application/vnd.sqlite3, application/vnd.apache.parquet or application/octet-stream), not text/plain or a form".into(),
        });
    }
    let ct = header_str(&headers, header::CONTENT_TYPE).unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let sqlite = match q.get("format") {
        Some(f) => f.eq_ignore_ascii_case("sqlite"),
        None => matches!(ct.as_str(), "application/vnd.sqlite3" | "application/x-sqlite3") || body.starts_with(strata::sqlite::MAGIC),
    };
    if sqlite {
        return import_sqlite(&app, &who(w), &q, body).await;
    }
    let parquet = match q.get("format") {
        Some(f) => f.eq_ignore_ascii_case("parquet"),
        None => matches!(ct.as_str(), "application/vnd.apache.parquet" | "application/x-parquet") || body.starts_with(b"PAR1"),
    };
    if parquet {
        return import_parquet(&app, &who(w), &q, body).await;
    }
    let format = match q.get("format") {
        Some(f) => Some(Format::parse(f).ok_or_else(|| ApiError::bad(format!("unknown format {f:?}: jsonl, json, csv, tsv, sqlite or parquet")))?),
        None => {
            let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
            let ct = ct.split(';').next().unwrap_or("").trim();
            match ct {
                "text/csv" => Some(Format::Csv),
                "text/tab-separated-values" => Some(Format::Tsv),
                "application/x-ndjson" | "application/jsonl" | "application/x-jsonlines" => Some(Format::Jsonl),
                "application/json" => Some(Format::Json),
                _ => None,
            }
        }
    };
    let format = format.ok_or_else(|| ApiError::bad("name the rows' format: ?format=jsonl, json, csv, tsv, sqlite or parquet"))?;
    let t0 = std::time::Instant::now();
    let (n, skipped) = app.svc.import(q.get("table").map(String::as_str), body, format).await?;
    Ok(AxJson(json!({"written": n, "skipped_fields": skipped, "took_ms": t0.elapsed().as_secs_f64() * 1e3})))
}

/// A query parameter that is true or false (`1`, `0`, `yes`, `no`), or
/// `default` when it is missing.
fn flag(q: &std::collections::HashMap<String, String>, k: &str, default: bool) -> Result<bool, ApiError> {
    match q.get(k).map(|v| v.to_ascii_lowercase()) {
        None => Ok(default),
        Some(v) => match v.as_str() {
            "" | "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(ApiError::bad(format!("{k} is true or false, not {v:?}"))),
        },
    }
}

/// `?tables=a,b`: the tables named, or none for every table.
fn tables_param(q: &std::collections::HashMap<String, String>) -> Vec<String> {
    q.get("tables").map(|t| t.split(',').map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// A SQLite file's tables ([`Service::import_sqlite`]): `?tables=a,b`
/// imports only those, `?blobs_base64=true` keeps BLOB columns as base64
/// text, and `?plan=true` answers what the import would do and writes
/// nothing.
async fn import_sqlite(app: &App, who: &Principal, q: &std::collections::HashMap<String, String>, body: Bytes) -> Res {
    if q.contains_key("table") {
        return Err(ApiError::bad("table is for rows: a SQLite file's tables go to the tables of their names (?tables= picks them)"));
    }
    let opts = strata::sqlite::ImportOptions { tables: tables_param(q), schemas: Vec::new(), blobs_base64: flag(q, "blobs_base64", false)? };
    let plan = flag(q, "plan", false)?;
    let t0 = std::time::Instant::now();
    let mut j = app.svc.import_sqlite(who, body, opts, plan, 0).await?;
    if !plan {
        let written: u64 = j["tables"].as_array().map_or(0, |t| t.iter().filter_map(|t| t["rows"].as_u64()).sum());
        j["written"] = json!(written);
        j["took_ms"] = json!(t0.elapsed().as_secs_f64() * 1e3);
    }
    Ok(AxJson(j))
}

/// A Parquet file's rows, into one table ([`Service::import_parquet`]):
/// `?table=` names it (else the table an export kept in the file),
/// `?key=` the key of a table the import creates (`""` for none),
/// `?blobs_base64=true` keeps binary columns as base64 text, and
/// `?plan=true` answers what the import would do and writes nothing.
#[cfg(feature = "parquet")]
async fn import_parquet(app: &App, who: &Principal, q: &std::collections::HashMap<String, String>, body: Bytes) -> Res {
    if q.contains_key("tables") {
        return Err(ApiError::bad("tables is for SQLite files: a Parquet file's rows go to one table (?table=)"));
    }
    let opts = strata::parquet::ImportOptions { table: q.get("table").cloned(), schema: None, key: q.get("key").cloned(), blobs_base64: flag(q, "blobs_base64", false)? };
    let plan = flag(q, "plan", false)?;
    let t0 = std::time::Instant::now();
    let mut j = app.svc.import_parquet(who, body, opts, plan, 0).await?;
    if !plan {
        j["written"] = j["rows"].clone();
        j["took_ms"] = json!(t0.elapsed().as_secs_f64() * 1e3);
    }
    Ok(AxJson(j))
}

#[cfg(not(feature = "parquet"))]
async fn import_parquet(_: &App, _: &Principal, _: &std::collections::HashMap<String, String>, _: Bytes) -> Res {
    Err(ApiError::bad("this server was built without Parquet (feature parquet)"))
}

/// The database as a new SQLite file ([`Service::export_sqlite`]), sent
/// as it is read from disk: `?tables=a,b` for only those tables,
/// `?fts=false`, `?indexes=false` and `?schemas=false` to leave out the
/// FTS5 indexes, the B-tree indexes and the kept Strata schemas. With
/// `?format=parquet`, one table (`?tables=`, which a database of one
/// table may leave out) as a Parquet file ([`Service::export_parquet`]),
/// `?schemas=false` leaving out its kept Strata schema.
async fn export(State(app): State<App>, Query(q): Query<std::collections::HashMap<String, String>>) -> Result<Response, ApiError> {
    let format = q.get("format").map(|f| f.to_ascii_lowercase());
    match format.as_deref() {
        None | Some("sqlite") => {}
        Some("parquet") => return export_parquet(&app, &q).await,
        Some(f) => return Err(ApiError::bad(format!("unknown format {f:?}: format=sqlite or format=parquet"))),
    }
    let opts = strata::sqlite::ExportOptions { tables: tables_param(&q), fts: flag(&q, "fts", true)?, indexes: flag(&q, "indexes", true)?, meta: flag(&q, "schemas", true)? };
    let (dir, path, done) = app.svc.export_sqlite(opts).await?;
    let name = done.tables.first().map_or("strata".into(), |t| t.table.clone());
    let tables: Vec<String> = done.tables.iter().map(|t| t.table.clone()).collect();
    send_file(dir, &path, "application/vnd.sqlite3", &format!("{name}.sqlite"), &tables).await
}

#[cfg(feature = "parquet")]
async fn export_parquet(app: &App, q: &std::collections::HashMap<String, String>) -> Result<Response, ApiError> {
    let mut tables = tables_param(q);
    if tables.len() > 1 {
        return Err(ApiError::bad("a Parquet file holds one table: name one (?tables=name), or export to a directory with strata-server export"));
    }
    let (dir, path, done) = app.svc.export_parquet(tables.pop(), flag(q, "schemas", true)?).await?;
    let tables: Vec<String> = done.tables.iter().map(|t| t.table.clone()).collect();
    let name = tables.first().cloned().unwrap_or_else(|| "strata".into());
    send_file(dir, &path, "application/vnd.apache.parquet", &format!("{name}.parquet"), &tables).await
}

#[cfg(not(feature = "parquet"))]
async fn export_parquet(_: &App, _: &std::collections::HashMap<String, String>) -> Result<Response, ApiError> {
    Err(ApiError::bad("this server was built without Parquet (feature parquet)"))
}

/// The file `path` as the answer, read from disk as it is sent; `dir`,
/// which holds it, goes when the last chunk is sent.
async fn send_file(dir: tempfile::TempDir, path: &std::path::Path, content_type: &str, filename: &str, tables: &[String]) -> Result<Response, ApiError> {
    let file = tokio::fs::File::open(path).await.map_err(ApiError::internal)?;
    let len = file.metadata().await.map_err(ApiError::internal)?.len();
    // The directory goes when the last chunk is sent.
    let chunks = futures::stream::unfold((file, Some(dir)), |(mut file, dir)| async move {
        use tokio::io::AsyncReadExt;
        dir.as_ref()?;
        let mut buf = vec![0u8; 1 << 16];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(Bytes::from(buf)), (file, dir)))
            }
            Err(e) => Some((Err(e), (file, None))),
        }
    });
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
            (header::HeaderName::from_static("x-strata-tables"), tables.join(",")),
        ],
        axum::body::Body::from_stream(chunks),
    )
        .into_response())
}

/// Write operations as NDJSON: one a line, or one event's several as
/// `{"ops": [...], "source": {...}}` (the source going to each operation
/// that names none), which a batch never splits. They are written in
/// batches as they arrive ([`Service::stream_write`]), and each batch is
/// answered by a line once it commits: `{"batch": n, "ops": .., "written":
/// .., "skipped": .., "lines": ..}`, `lines` counting the lines committed
/// so far. The answer ends with `{"done": true, ...}`, or with `{"error":
/// ..., "status": .., "lines": ..}` at the first failure, nothing after
/// the lines it counts applied.
async fn stream(State(app): State<App>, headers: HeaderMap, body: axum::body::Body) -> Result<Response, ApiError> {
    if auth::simple_content_type(header_str(&headers, header::CONTENT_TYPE)) {
        return Err(ApiError { status: 415, message: "send the operations as application/x-ndjson, not text/plain or a form".into() });
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Json>();
    tokio::spawn(ndjson(app.svc.clone(), body, tx).instrument(tracing::Span::current()));
    let lines = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(|j| Ok::<_, std::convert::Infallible>(Bytes::from(format!("{j}\n"))));
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], axum::body::Body::from_stream(lines)).into_response())
}

/// Lines a stream reads ahead of its writes ([`read_lines`]).
const STREAM_LINES: usize = 4096;

/// Writes a stream's lines ([`stream`]), answering on `tx`. The body is
/// read in a task of its own, ahead of the writes, so each batch takes the
/// lines that arrived while the one before it committed: one line at a
/// time while the client trickles, up to [`STREAM_OPS`] operations when
/// it sends faster than one commit takes. The client may send
/// acknowledged batches' successors before the answers arrive: answers
/// wait in memory, so that a client that reads them only once it has
/// sent everything does not stall the stream.
async fn ndjson(svc: Arc<Service>, body: axum::body::Body, tx: tokio::sync::mpsc::UnboundedSender<Json>) {
    let (ahead, mut lines) = tokio::sync::mpsc::channel(STREAM_LINES);
    tokio::spawn(read_lines(body, svc.max_body, ahead).in_current_span());
    let mut s = Stream::default();
    let mut batch: Vec<Json> = Vec::new();
    // Lines read, and lines committed.
    let (mut read, mut done) = (0u64, 0u64);
    while let Some(first) = lines.recv().await {
        let mut next = Some(first);
        while let Some(line) = next.take() {
            let ops = match line {
                Ok(ops) => s.admit(&ops).map(|_| ops).map_err(|e| ApiError { message: format!("line {}: {}", read + 1, e.message), ..e }),
                Err(e) => Err(e),
            };
            let ops = match ops {
                Ok(ops) => ops,
                Err(e) => {
                    // The lines before it are good.
                    if flush(&svc, &mut s, &mut batch, read, &mut done, &tx).await {
                        let _ = tx.send(json!({"error": e.message, "status": e.status, "lines": done}));
                    }
                    return;
                }
            };
            read += 1;
            batch.extend(ops);
            if batch.len() < STREAM_OPS {
                next = lines.try_recv().ok();
            }
        }
        if !flush(&svc, &mut s, &mut batch, read, &mut done, &tx).await {
            return;
        }
    }
    if flush(&svc, &mut s, &mut batch, read, &mut done, &tx).await {
        let _ = tx.send(json!({"done": true, "batches": s.batches, "lines": done, "written": s.written, "skipped": s.skipped}));
    }
}

/// Reads a stream's body into its lines' operations ([`line_ops`]) for
/// [`ndjson`] to write. Once [`STREAM_LINES`] lines wait it stops reading,
/// so that flow control slows a producer faster than the writes. A line
/// that is not operations is the last it sends.
async fn read_lines(body: axum::body::Body, max: usize, lines: tokio::sync::mpsc::Sender<Result<Vec<Json>, ApiError>>) {
    let at_line = |n: u64| move |e: ApiError| ApiError { message: format!("line {n}: {}", e.message), ..e };
    let mut chunks = body.into_data_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut n = 0u64;
    while let Some(next) = chunks.next().await {
        let bytes = match next {
            Ok(b) => b,
            Err(e) => {
                let _ = lines.send(Err(ApiError::bad(format!("reading the body: {e}")))).await;
                return;
            }
        };
        buf.extend_from_slice(&bytes);
        let mut at = 0;
        while let Some(nl) = buf[at..].iter().position(|&b| b == b'\n') {
            n += 1;
            let ops = line_ops(&buf[at..at + nl]).map_err(at_line(n));
            at += nl + 1;
            let bad = ops.is_err();
            if lines.send(ops).await.is_err() || bad {
                return;
            }
        }
        buf.drain(..at);
        if buf.len() > max {
            let _ = lines.send(Err(ApiError { status: 413, message: format!("line {}: longer than the server takes ({max} bytes)", n + 1) })).await;
            return;
        }
    }
    // A last line without its newline.
    if !buf.is_empty() {
        let _ = lines.send(line_ops(&buf).map_err(at_line(n + 1))).await;
    }
}

/// Writes the stream's batch, if any, and answers it; false when it
/// failed, which the answer says.
async fn flush(svc: &Arc<Service>, s: &mut Stream, batch: &mut Vec<Json>, read: u64, done: &mut u64, tx: &tokio::sync::mpsc::UnboundedSender<Json>) -> bool {
    if batch.is_empty() {
        *done = read;
        return true;
    }
    let ops = std::mem::take(batch);
    let n = ops.len();
    match svc.stream_write(s, ops).await {
        Ok(w) => {
            *done = read;
            let mut j = json!({"batch": s.batches, "ops": n, "written": w.rows, "skipped": w.skipped, "lines": read});
            if let Some(q) = w.seq_no {
                j["seq_no"] = json!(q);
            }
            if !w.seq_nos.is_empty() {
                j["seq_nos"] = json!(w.seq_nos);
            }
            let offsets = s.offsets();
            if !offsets.is_empty() {
                j["offsets"] = Json::Object(offsets.into_iter().map(|(n, o)| (n, json!(o))).collect());
            }
            let _ = tx.send(j);
            true
        }
        Err(e) => {
            let _ = tx.send(json!({"error": e.message, "status": e.status, "lines": *done}));
            false
        }
    }
}

/// A line of a stream: an operation, `{"ops": [...]}`, or blank.
fn line_ops(line: &[u8]) -> Result<Vec<Json>, ApiError> {
    let text = std::str::from_utf8(line).map_err(|_| ApiError::bad("not UTF-8 text"))?.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut j: Json = serde_json::from_str(text).map_err(|e| ApiError::bad(format!("not JSON: {e}")))?;
    match j.get_mut("ops").map(Json::take) {
        Some(Json::Array(mut ops)) => {
            if let Some(src) = j.get("source").filter(|s| !s.is_null()) {
                for op in ops.iter_mut().filter(|op| op.is_object() && !has_source(op)) {
                    op["source"] = src.clone();
                }
            }
            Ok(ops)
        }
        Some(_) => Err(ApiError::bad("\"ops\" is an array of operations")),
        None if j.is_object() => Ok(vec![j]),
        None => Err(ApiError::bad("a line is an operation (a JSON object), or one event's operations as {\"ops\": [...]}")),
    }
}

/// Every stream source written from, with the offset of the last event
/// each wrote ([`Service::sources`]).
async fn sources(State(app): State<App>) -> Res {
    let sources: Vec<Json> = app.svc.sources().await?.into_iter().map(source_json).collect();
    Ok(AxJson(json!({"sources": sources})))
}

/// One source's offset, null for one not written from (start from the
/// beginning).
async fn source(State(app): State<App>, axum::extract::Path(name): axum::extract::Path<String>) -> Res {
    let found = app.svc.sources().await?.into_iter().find(|s| s.name == name);
    Ok(AxJson(found.map_or_else(|| json!({"name": name, "offset": null, "updated": null}), source_json)))
}

fn source_json(s: strata::db::Source) -> Json {
    json!({"name": s.name, "offset": s.offset, "updated": s.updated.map(strata::timestamp::format)})
}

/// A schema validated, in both JSON forms and as the `CREATE TABLE`
/// statement that makes it.
async fn schema_check(AxJson(b): AxJson<Json>) -> Res {
    let s = strata::schema::Schema::from_json_value(&b)?;
    Ok(AxJson(json!({"schema": s, "short": s.to_short_json(), "sql": strata::query::ddl::create_sql(&s)})))
}

/// The vector of a text, or of an image or sound file (`"image"` or
/// `"audio"`, base64), as `{"embed": ...}` gives a query's parameter: for
/// a plan, whose vectors are values.
/// The embedding models a schema can name, local and hosted, and whether
/// each would open on this server ([`strata::embed::catalog`]): fetched,
/// its key set, its feature built. Nothing is loaded or sent.
async fn models(State(app): State<App>) -> Res {
    let dir = app.svc.models_dir();
    let d = dir.clone();
    let list = tokio::task::spawn_blocking(move || strata::embed::catalog(&d)).await.map_err(ApiError::internal)?;
    let models: Vec<Json> = list.iter().map(|m| json!({"name": m.name, "dim": m.dim, "hosted": m.hosted, "what": m.what, "ready": m.ready.is_ok(), "error": m.ready.as_ref().err()})).collect();
    Ok(AxJson(json!({"dir": dir.display().to_string(), "models": models})))
}

async fn embed(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let file = ["image", "audio"].into_iter().find_map(|k| b.get(k).map(|v| (k, v)));
    let what = match file {
        Some((k, v)) => json!({ k: v.as_str().ok_or_else(|| ApiError::bad(format!("{k} is a file in base64")))? }),
        None => json!(body_str(&b, "text")?),
    };
    let name = |k: &str| match b.get(k) {
        None | Some(Json::Null) => Ok(None),
        Some(f) => f.as_str().map(Some).ok_or_else(|| ApiError::bad(format!("{k} is a name"))),
    };
    let (table, field, _) = app.svc.model(name("table")?, name("field")?)?;
    let p = app.svc.params(Some(&json!({ "v": { "embed": what, "field": field, "table": table } }))).await?;
    match p.0.get("v") {
        Some(strata::value::Value::Vector(v)) => Ok(AxJson(json!({ "table": table, "field": field, "vector": &v[..] }))),
        // A multi-vector field's model: a vector a token of the text.
        Some(strata::value::Value::List(vs)) => {
            let vectors: Vec<&[f32]> = vs.iter().filter_map(|v| if let strata::value::Value::Vector(v) = v { Some(&v[..]) } else { None }).collect();
            Ok(AxJson(json!({ "table": table, "field": field, "vectors": vectors })))
        }
        _ => Err(ApiError::internal("the text has no vector")),
    }
}

/// The file a MEDIA value names, by its SHA-256, for showing it: only a
/// file some row names, read from where the row says and checked against
/// the hash ([`Service::media_file`]). Its bytes never change, so clients
/// may keep it.
async fn media_file(State(app): State<App>, axum::extract::Path(sha256): axum::extract::Path<String>) -> Result<Response, ApiError> {
    let (mime, bytes) = app.svc.media_file(&sha256).await?;
    Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string())], bytes).into_response())
}

async fn explain(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let sql = body_str(&b, "sql")?;
    let params = app.svc.params(b.get("params")).await?;
    let catalog = app.svc.catalog();
    let tables: Vec<&strata::schema::Schema> = catalog.iter().map(|t| &**t).collect();
    // A compound query is operators over single-table queries; what each
    // of those ran is in "plan".
    let profiles = |t: &str, n: &str| app.svc.rank_profile(t, n);
    // The plan names the files of its ranking models.
    let plan = app.svc.open_ranking_models(std::sync::Arc::new(strata::query::compound::plan_read(sql, &tables, &params, &profiles)?), false)?;
    let logical = match plan.root.uses_operators() {
        true => "a compound query: operators over single-table queries (\"tree\"), whose runs are in \"plan\"".to_string(),
        false => {
            let schema = tables.iter().find(|t| t.table.eq_ignore_ascii_case(plan.table())).ok_or_else(|| ApiError::bad(format!("query error: no such table: {}", plan.table())))?;
            // A query by a rank profile binds as written out.
            format!("{:#?}", strata::query::bind(&strata::query::profile::written_out(sql, &profiles)?, schema, &params)?)
        }
    };
    let (ir, tree) = (plan.to_json()?, json!(plan.explain(&tables)));
    // `hits`: the top rows with the values they were ranked by, for a
    // query that ranks them (another has no values to show).
    let hits = match body_u64(&b, "hits") {
        0 => None,
        _ if !strata::query::sql::ranks(sql) => None,
        n => {
            let sql = strata::query::sql::with_features(sql)?;
            let h = app.svc.query(QueryReq { sql, params: params.clone(), timeout_ms: body_u64(&b, "timeout_ms"), ..Default::default() }).await?;
            let j = js::result(&h.rs, h.took_ms);
            let names = j["columns"].as_array().cloned().unwrap_or_default();
            let rows = j["rows"].as_array().cloned().unwrap_or_default();
            Some(rows.iter().take(n as usize).map(|r| names.iter().zip(r.as_array().into_iter().flatten()).map(|(c, v)| (c.as_str().unwrap_or("").to_string(), v.clone())).collect::<serde_json::Map<_, _>>()).collect::<Vec<_>>())
        }
    };
    let a = app.svc.query(QueryReq { sql: sql.into(), params, timeout_ms: body_u64(&b, "timeout_ms"), profile: b.get("profile").and_then(|v| v.as_bool()) == Some(true), ..Default::default() }).await?;
    let mut out = json!({
        "kind": a.kind,
        "logical": logical,
        "ir": ir,
        "tree": tree,
        "plan": a.rs.stats.plan,
        "filter_docs": a.rs.stats.filter_docs,
        "scored": a.rs.stats.scored,
        "steps": a.rs.stats.steps,
        "rows": a.rs.rows.len(),
        "took_ms": a.took_ms,
    });
    if let Some(h) = hits {
        out["hits"] = json!(h);
    }
    Ok(AxJson(out))
}

async fn synonyms(State(app): State<App>) -> AxJson<Json> {
    let s = app.svc.synonyms();
    AxJson(json!({
        "text": s.as_ref().map_or("", |s| s.text()),
        "rules": s.as_ref().map_or(0, |s| s.rules()),
        "file": app.svc.synonyms_file().map(|p| p.display().to_string()),
    }))
}

async fn put_synonyms(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let rules = app.svc.put_synonyms(body_str(&b, "text")?)?;
    Ok(AxJson(json!({"rules": rules, "file": app.svc.synonyms_file().map(|p| p.display().to_string())})))
}

/// `{"field": "sid", "prefixes": ["12", "12.7"], "where": "tag = :t",
/// "params": {...}, "table": ..., "min_seq_no": ..., "timeout_ms": ...}`:
/// the codes that can follow each prefix among the rows `where` matches
/// (all rows without it), with how many rows have each
/// ([`strata::db::Db::sid_next`]). `table` defaults to the first.
async fn sid_next(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let t0 = Instant::now();
    let table = match b.get("table") {
        None | Some(Json::Null) => app.svc.schema.table.clone(),
        Some(_) => body_str(&b, "table")?.to_string(),
    };
    let field = body_str(&b, "field")?;
    let prefixes: Vec<String> = match b.get("prefixes") {
        Some(Json::Array(ps)) => ps.iter().map(|p| p.as_str().map(String::from).ok_or_else(|| ApiError::bad("prefixes are strings, such as \"12.7\", or \"\" for the first numbers"))).collect::<Result<_, _>>()?,
        _ => return Err(ApiError::bad("the body needs \"prefixes\", a list of codes such as [\"12\", \"12.7\"] (\"\" for the first numbers)")),
    };
    let filter = match b.get("where") {
        None | Some(Json::Null) => String::new(),
        Some(_) => format!(" WHERE {}", body_str(&b, "where")?),
    };
    let sql = format!("SELECT COUNT(*) FROM {}{filter}", strata::query::sql::ident(&table));
    let params = app.svc.params(b.get("params")).await?;
    let next = app.svc.sid_next(&sql, None, &params, field, &prefixes, &after(&b)?, body_u64(&b, "timeout_ms")).await?;
    let next: Vec<Json> = prefixes
        .iter()
        .zip(next)
        .map(|(p, cs)| json!({"prefix": p, "next": cs.into_iter().map(|(code, count)| json!({"code": code, "count": count})).collect::<Vec<_>>()}))
        .collect();
    Ok(AxJson(json!({"next": next, "took_ms": t0.elapsed().as_secs_f64() * 1e3})))
}

async fn term_stats(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let params = app.svc.params(b.get("params")).await?;
    let (stats, snapshot) = app.svc.term_stats(body_str(&b, "sql")?, None, &params, &after(&b)?, body_u64(&b, "timeout_ms")).await?;
    Ok(AxJson(json!({"stats": stats, "snapshot": snapshot})))
}

async fn snapshot(State(app): State<App>) -> Res {
    let (id, txn) = app.svc.lease()?;
    Ok(AxJson(json!({"snapshot": id, "txn": txn, "ttl_s": app.svc.lease_ttl.as_secs()})))
}

/// `{"name": ..., "layout": "file" | "dir", "link_from": ...}`: writes a
/// checkpoint into the server's `--backup-dir`.
async fn admin_checkpoint(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let layout = match b.get("layout").and_then(|v| v.as_str()).unwrap_or("file") {
        "file" => Layout::SingleFile,
        "dir" | "directory" => Layout::Directory,
        other => return Err(ApiError::bad(format!("layout is \"file\" or \"dir\", not {other:?}"))),
    };
    let stats = app.svc.checkpoint(body_str(&b, "name")?, layout, b.get("link_from").and_then(|v| v.as_str())).await?;
    Ok(AxJson(json!({"checkpoint": stats})))
}

async fn admin_check(State(app): State<App>) -> Res {
    app.svc.check().await?;
    Ok(AxJson(json!({"ok": true})))
}

async fn admin_vacuum(State(app): State<App>) -> Res {
    Ok(AxJson(json!({"bytes_freed": app.svc.vacuum().await?})))
}

async fn admin_verify(State(app): State<App>, body: Option<AxJson<Json>>) -> Res {
    let timeout_ms = body.as_ref().and_then(|b| b.get("timeout_ms")).and_then(|v| v.as_u64()).unwrap_or(0);
    let timeout = if timeout_ms > 0 { Duration::from_millis(timeout_ms) } else { VERIFY_WAIT };
    Ok(AxJson(app.svc.verify_replicas(timeout).await?))
}

/// Makes this replica the primary of a new term: `{"replicas": [gRPC
/// URLs], "write_acks": "all", "force": false}`
/// ([`crate::service::Service::promote`]).
async fn admin_promote(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Res {
    let urls = || ApiError::bad("replicas is a list of the gRPC URLs of the copies the new primary ships to");
    let replicas = match b.get("replicas") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(a)) => a.iter().map(|v| v.as_str().map(String::from).ok_or_else(urls)).collect::<Result<_, _>>()?,
        Some(_) => return Err(urls()),
    };
    let acks = match b.get("write_acks") {
        None | Some(Json::Null) => Acks::All,
        Some(v) => v.as_str().and_then(Acks::parse).ok_or_else(|| ApiError::bad("write_acks is all, majority or primary"))?,
    };
    let force = match b.get("force") {
        None | Some(Json::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| ApiError::bad("force is true or false"))?,
    };
    Ok(AxJson(app.svc.promote(&who(w), replicas, acks, force).await?))
}

/// The gRPC URLs a request names under `key`.
fn urls(b: &Json, key: &str) -> Result<Vec<String>, ApiError> {
    let bad = || ApiError::bad(format!("{key} is a list of gRPC URLs (http://host:port)"));
    match b.get(key) {
        Some(Json::Array(a)) => a.iter().map(|v| v.as_str().filter(|s| !s.is_empty()).map(String::from).ok_or_else(bad)).collect(),
        _ => Err(bad()),
    }
}

fn body_bool(b: &Json, key: &str) -> Result<bool, ApiError> {
    match b.get(key) {
        None | Some(Json::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| ApiError::bad(format!("{key} is true or false"))),
    }
}

/// `POST /v1/admin/shards {"servers": [...], "write_acks"?}`: a new
/// shard, holding no slot until slots move to it.
async fn add_shard(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let servers = urls(&b, "servers")?;
    let acks = match b.get("write_acks") {
        None | Some(Json::Null) => Acks::All,
        Some(v) => v.as_str().and_then(Acks::parse).ok_or_else(|| ApiError::bad("write_acks is all, majority or primary"))?,
    };
    let j = app.svc.coordinator()?.add_shard(servers.clone(), acks).await?;
    let detail = format!("added shard {} served by {}", j["shard"], servers.join(", "));
    Ok(audited(j, detail))
}

/// `DELETE /v1/admin/shards/:i`: removes a shard that holds no slot.
async fn remove_shard(State(app): State<App>, axum::extract::Path(i): axum::extract::Path<usize>) -> Result<Response, ApiError> {
    let j = app.svc.coordinator()?.remove_shard(i).await?;
    keys::note_change(keys::Change { object: Some(format!("shard {i}")), ..Default::default() });
    Ok(audited(j, format!("removed shard {i}")))
}

/// `POST /v1/admin/shards/:i/replicas {"url", "force"?}`: one more
/// server for shard `i`, a replica of its primary.
async fn add_replica(State(app): State<App>, axum::extract::Path(i): axum::extract::Path<usize>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let url = body_str(&b, "url")?.to_string();
    let j = app.svc.coordinator()?.add_replica(i, &url, body_bool(&b, "force")?).await?;
    keys::note_change(keys::Change { object: Some(format!("shard {i}")), after: Some(json!({"added": url})), ..Default::default() });
    Ok(audited(j, format!("added {url} to shard {i}")))
}

/// `POST /v1/admin/shards/:i/promote {"url", "force"?}`: makes that
/// replica shard `i`'s primary, by a majority of the shard's votes.
async fn promote_in_shard(State(app): State<App>, axum::extract::Path(i): axum::extract::Path<usize>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let url = body_str(&b, "url")?.to_string();
    let j = app.svc.coordinator()?.promote(i, &url, body_bool(&b, "force")?).await?;
    keys::note_change(keys::Change { object: Some(format!("shard {i}")), after: Some(json!({"primary": url})), ..Default::default() });
    Ok(audited(j, format!("promoted {url} in shard {i}")))
}

/// `DELETE /v1/admin/shards/:i/replicas?url=`: one server fewer for shard
/// `i`.
async fn remove_replica(State(app): State<App>, axum::extract::Path(i): axum::extract::Path<usize>, Query(q): Params) -> Result<Response, ApiError> {
    let url = q.get("url").filter(|u| !u.is_empty()).ok_or_else(|| ApiError::bad("url is the gRPC URL of the server to remove (?url=http://host:port)"))?;
    let j = app.svc.coordinator()?.remove_replica(i, url).await?;
    keys::note_change(keys::Change { object: Some(format!("shard {i}")), before: Some(json!({"removed": url})), ..Default::default() });
    Ok(audited(j, format!("removed {url} from shard {i}")))
}

/// The most slots each move of a rebalance or drain takes: the body's
/// `"chunk"`, or [`crate::coord::MOVE_SLOTS`].
fn chunk(b: &Option<AxJson<Json>>) -> Result<usize, ApiError> {
    match b.as_ref().and_then(|b| b.get("chunk")) {
        None | Some(Json::Null) => Ok(crate::coord::MOVE_SLOTS),
        Some(v) => v.as_u64().filter(|&c| (1..=crate::slots::SLOTS as u64).contains(&c)).map(|c| c as usize).ok_or_else(|| ApiError::bad(format!("chunk is the most slots one move takes: 1 to {}", crate::slots::SLOTS))),
    }
}

/// `POST /v1/admin/rebalance {"chunk"?}`: evens out the slots over the
/// shards, in the background (`GET /v1/admin/moves` follows it).
async fn rebalance(State(app): State<App>, w: Option<axum::Extension<Principal>>, body: Option<AxJson<Json>>) -> Result<Response, ApiError> {
    let j = app.svc.coordinator()?.rebalance(chunk(&body)?)?;
    let title = format!("Slots even out over the shards: {} moves", j["job"]["of"]);
    let j = moves_started(&app, w, j, title, None);
    let detail = format!("rebalancing: {} moves", j["job"]["of"]);
    Ok(audited(j, detail))
}

/// `POST /v1/admin/shards/:i/drain {"chunk"?}`: moves every slot of shard
/// `i` to the others, in the background, so that it can be removed.
async fn drain_shard(State(app): State<App>, w: Option<axum::Extension<Principal>>, axum::extract::Path(i): axum::extract::Path<usize>, body: Option<AxJson<Json>>) -> Result<Response, ApiError> {
    let j = app.svc.coordinator()?.drain(i, chunk(&body)?)?;
    keys::note_change(keys::Change { object: Some(format!("shard {i}")), ..Default::default() });
    let title = format!("Shard {i} drains: {} moves", j["job"]["of"]);
    let j = moves_started(&app, w, j, title, Some(i));
    let detail = format!("draining shard {i}: {} moves", j["job"]["of"]);
    Ok(audited(j, detail))
}

/// A job of moves just started: who started it, and its event.
fn moves_started(app: &App, w: Option<axum::Extension<Principal>>, j: Json, title: String, shard: Option<usize>) -> Json {
    let by = keys::audit_name(&who(w));
    let Ok(c) = app.svc.coordinator() else { return j };
    c.moves_by(by.clone());
    let where_ = shard.map_or(Json::Null, |s| json!({"shard": s}));
    app.svc.events.add(crate::events::Event { kind: "moves_started".into(), severity: "info".into(), title, detail: json!({"kind": j["job"]["kind"], "of": j["job"]["of"]}), where_, who: Some(by) });
    c.moves()
}

/// `POST /v1/admin/plan {"shards": [{"shard", "copies": [urls]}],
/// "add_shards": n, "remove_shards": [i], "moves": [{"slots": [a, b],
/// "to": i}]}` on a coordinator of its own machines: the steps that
/// would make the cluster so, the rows they copy and how long they take,
/// what clients feel meanwhile, what to check first, and the copies they
/// remove. Nothing changes. A plan holds only what was asked: a shard not
/// named keeps its copies.
async fn admin_plan(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let c = app.svc.coordinator()?;
    let snap = c.snapshot().await;
    Ok(AxJson(crate::coord::plan::plan(&snap, &b, c.moves()["running"] == true)?))
}

/// `GET /v1/admin/moves`: the job of moves running or last run here, and
/// the move under way in the cluster map.
async fn moves(State(app): State<App>) -> Res {
    Ok(AxJson(app.svc.coordinator()?.moves()))
}

/// `POST /v1/admin/moves {"slots": [[start, end], ...], "to": shard}`:
/// moves slots of one shard to another, in the background.
async fn move_slots(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let bad = || ApiError::bad("slots is a list of slots or of [start, end) ranges of them, all held by one shard");
    let mut slots = Vec::new();
    for v in b.get("slots").and_then(Json::as_array).ok_or_else(bad)? {
        match (v.as_u64(), v.as_array().map(|r| r.iter().map(Json::as_u64).collect::<Vec<_>>())) {
            (Some(s), _) => slots.push(s as usize),
            (_, Some(r)) if matches!(r[..], [Some(a), Some(b)] if a < b && b <= crate::slots::SLOTS as u64) => slots.extend(r[0].unwrap_or(0) as usize..r[1].unwrap_or(0) as usize),
            _ => return Err(bad()),
        }
    }
    let to = b.get("to").and_then(Json::as_u64).ok_or_else(|| ApiError::bad("to is the shard the slots move to"))? as usize;
    let j = app.svc.coordinator()?.start_move(slots.clone(), to)?;
    let j = moves_started(&app, w, j, format!("{} slots move to shard {to}", slots.len()), Some(to));
    Ok(audited(j, format!("moving {} slots to shard {to}", slots.len())))
}

/// `POST /v1/admin/moves/stop`: the job running here stops once its move
/// under way ends.
async fn stop_moves(State(app): State<App>) -> Result<Response, ApiError> {
    Ok(audited(app.svc.coordinator()?.stop_moves(), "stopping the job of moves".into()))
}

/// `POST /v1/admin/moves/finish`: ends the move a stopped coordinator
/// left in the cluster map (completed if it had flipped, given up if not).
async fn finish_move(State(app): State<App>) -> Result<Response, ApiError> {
    let j = app.svc.coordinator()?.finish_move().await?;
    let detail = format!("finished the move left in the cluster map: {}", j["finished"]);
    Ok(audited(j, detail))
}

/// How long `POST /v1/admin/verify` waits for the replicas' digests by
/// default.
const VERIFY_WAIT: Duration = Duration::from_secs(60);

async fn agent_run(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let req = agent::AgentReq {
        question: body_str(&b, "question")?.to_string(),
        model: b.get("model").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
        effort: b.get("effort").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
        api_key: b.get("api_key").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
        max_steps: b.get("max_steps").and_then(|v| v.as_u64()).map(|n| n.clamp(1, 20) as usize).unwrap_or(8),
    };
    app.svc.metrics.agent_run();
    Ok(AxJson(agent::run(&app.svc, req).await?))
}

/// `POST /v1/judge`: a Claude model's grade of each row for a query
/// ([`crate::judge`]).
async fn judge(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let opt = |k: &str| b.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
    let keys = b.get("keys").and_then(|v| v.as_array()).cloned().ok_or_else(|| ApiError::bad("the body needs \"keys\", the rows' keys"))?;
    let fields = match b.get("fields") {
        None | Some(Json::Null) => Vec::new(),
        Some(v) => v.as_array().map(|a| a.iter().filter_map(|f| f.as_str().map(String::from)).collect()).ok_or_else(|| ApiError::bad("fields is a list of field names"))?,
    };
    let req = judge::JudgeReq { table: opt("table"), query: body_str(&b, "query")?.to_string(), keys, fields, model: opt("model"), api_key: opt("api_key") };
    Ok(AxJson(judge::run(&app.svc, req).await?))
}

type Params = Query<std::collections::HashMap<String, String>>;

fn param_u64(q: &std::collections::HashMap<String, String>, k: &str, default: u64) -> Result<u64, ApiError> {
    q.get(k).map_or(Ok(default), |v| v.parse().map_err(|_| ApiError::bad(format!("{k} is a whole number, not {v:?}"))))
}

async fn live_queries(State(app): State<App>) -> AxJson<Json> {
    AxJson(json!({"queries": app.svc.live.list(), "running": app.svc.running()}))
}

async fn cancel_query(State(app): State<App>, axum::extract::Path(id): axum::extract::Path<u64>) -> Result<Response, ApiError> {
    let q = app.svc.live.list().into_iter().find(|q| q["id"] == id).unwrap_or(Json::Null);
    match app.svc.live.cancel(id) {
        true => {
            tracing::info!(query = id, "query cancelled by an operator");
            // What was cancelled: its statement, and whose it was.
            let sql: String = q["sql"].as_str().unwrap_or("").chars().take(2048).collect();
            let key = q["key"].as_str().or(q["key_name"].as_str()).map(String::from);
            let mut was = json!({"id": id, "statement": sql});
            for k in ["key", "key_id", "client", "app", "addr", "kind", "started_ms"] {
                if !q[k].is_null() {
                    was[k] = q[k].clone();
                }
            }
            keys::note_change(keys::Change { object: Some(format!("query {id}")), before: Some(was), ..Default::default() });
            let of = key.map_or(String::new(), |k| format!(" of key {k}"));
            Ok(audited(json!({"cancelled": true, "id": id}), format!("cancelled query {id}{of}: {}", sql.chars().take(200).collect::<String>())))
        }
        false => Err(ApiError { status: 404, message: format!("query {id} is not running") }),
    }
}

/// Ended queries as a list shows them: without their steps and plan
/// lines (`GET /v1/queries/finished/:id` has them), unless `full=true`.
fn finished_list(qs: Vec<Arc<telemetry::Finished>>, q: &std::collections::HashMap<String, String>) -> Json {
    match q.get("full").map(String::as_str) {
        Some("true" | "1") => json!(qs),
        _ => Json::Array(qs.iter().map(|f| f.summary()).collect()),
    }
}

async fn recent_queries(State(app): State<App>, Query(q): Params) -> Res {
    let (since, limit) = (param_u64(&q, "since", 0)?, param_u64(&q, "limit", 100)? as usize);
    Ok(AxJson(json!({"queries": finished_list(app.svc.query_log.recent(since, limit.min(telemetry::RECENT)), &q), "now_ms": telemetry::now_ms()})))
}

async fn slow_queries(State(app): State<App>, Query(q): Params) -> Res {
    let limit = param_u64(&q, "limit", 100)? as usize;
    Ok(AxJson(json!({"queries": finished_list(app.svc.query_log.slow(limit.min(telemetry::SLOW)), &q), "slow_ms": app.svc.slow_after().map(|d| d.as_secs_f64() * 1e3)})))
}

/// One ended query with its whole profile: its trace, its steps and what
/// ran ([`telemetry::Finished`]).
async fn finished_query(State(app): State<App>, axum::extract::Path(id): axum::extract::Path<u64>) -> Res {
    match app.svc.query_log.finished(id) {
        Some(f) => Ok(AxJson(json!(f))),
        None => Err(ApiError { status: 404, message: format!("query {id} is no longer kept: the server keeps the latest {} and the slowest {}", telemetry::RECENT, telemetry::SLOW) }),
    }
}

/// What profiling does now: the slow threshold (`slow_ms`, `null` for
/// none; as started with `--slow-ms` unless changed) and whether every
/// query's CPU time is measured (`cpu_all`).
fn profile_settings_json(app: &App) -> Json {
    json!({
        "slow_ms": app.svc.slow_after().map(|d| d.as_secs_f64() * 1e3),
        "started_slow_ms": app.svc.slow.map(|d| d.as_secs_f64() * 1e3),
        "cpu_all": app.svc.profiling.cpu_all.load(std::sync::atomic::Ordering::Relaxed),
    })
}

async fn profile_settings(State(app): State<App>) -> AxJson<Json> {
    AxJson(profile_settings_json(&app))
}

/// `{"slow_ms": 50}` keeps every query that takes 50 ms or longer with
/// its profile among the slow ones (`null`: none, `0`: every query);
/// `{"cpu_all": true}` measures every query's CPU time, step by step.
/// Either may come alone. Until the server restarts.
async fn put_profile_settings(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let mut did = Vec::new();
    if let Some(v) = b.get("slow_ms") {
        let slow = match v {
            Json::Null => None,
            v => match v.as_f64() {
                Some(ms) if ms >= 0.0 && ms.is_finite() => Some(Duration::from_secs_f64(ms / 1e3)),
                _ => return Err(ApiError::bad("slow_ms is a number of milliseconds, 0 or more, or null for no slow queries")),
            },
        };
        app.svc.profiling.set_slow(slow);
        did.push(format!("slow_ms {}", slow.map_or("off".to_string(), |d| format!("{}", d.as_secs_f64() * 1e3))));
    }
    if let Some(v) = b.get("cpu_all") {
        let on = v.as_bool().ok_or_else(|| ApiError::bad("cpu_all is true or false"))?;
        app.svc.profiling.cpu_all.store(on, std::sync::atomic::Ordering::Relaxed);
        did.push(format!("cpu_all {on}"));
    }
    if did.is_empty() {
        return Err(ApiError::bad("the body sets slow_ms (milliseconds, or null) or cpu_all (true or false)"));
    }
    tracing::info!(settings = %did.join(", "), "profiling settings changed");
    Ok(audited(profile_settings_json(&app), format!("profiling: {}", did.join(", "))))
}

/// A profile ([`profiling`]) of this server, or with `?node=` on a
/// coordinator, of that server of its cluster (one of `GET /v1/cluster`'s
/// replica URLs, over gRPC: [`crate::coord::Coordinator::profile`]).
/// `?format=folded` (`&value=in_use`) answers folded lines to download.
async fn profile(app: &App, kind: profiling::Kind, q: &std::collections::HashMap<String, String>, body: Json) -> Result<Response, ApiError> {
    let asked = profiling::Asked { kind, body, folded: q.get("format").map(String::as_str) == Some("folded"), in_use: q.get("value").map(String::as_str) == Some("in_use") };
    let answer = match q.get("node").map(|n| n.trim()).filter(|n| !n.is_empty()) {
        Some(node) => app.svc.profile_node(node, asked).await?,
        None => profiling::take(&app.svc, asked).await?,
    };
    Ok(match answer {
        profiling::Answer::Json(j) => AxJson(j).into_response(),
        profiling::Answer::Folded(file, text) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file}\""))], text).into_response(),
    })
}

/// A CPU profile ([`crate::profiler`]): of the whole server for `seconds`
/// (default 5), or of `sql` run again and again by `clients` callers. It
/// answers the profile with how busy the server was (`cores`), the samples
/// delivered a CPU second (`rate`) and, for a query, its runs and their
/// times.
async fn cpu_profile(State(app): State<App>, Query(q): Params, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    profile(&app, profiling::Kind::Cpu, &q, b).await
}

/// The heap's counts, the process's resident memory, and its threads.
async fn memory(State(app): State<App>, Query(q): Params) -> Result<Response, ApiError> {
    profile(&app, profiling::Kind::Memory, &q, Json::Null).await
}

/// An allocation profile ([`crate::heap::take`]), of the whole server or of a
/// query in a loop as [`cpu_profile`].
async fn heap_profile(State(app): State<App>, Query(q): Params, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    profile(&app, profiling::Kind::Heap, &q, b).await
}

async fn threads(State(app): State<App>, Query(q): Params) -> Result<Response, ApiError> {
    profile(&app, profiling::Kind::Threads, &q, Json::Null).await
}

async fn request_queries(State(app): State<App>, axum::extract::Path(id): axum::extract::Path<String>) -> AxJson<Json> {
    AxJson(json!({"queries": app.svc.query_log.by_request(&id)}))
}

// ----- workload (alerts agent): statements over a window

/// The window `from`/`to` (ms) a request names, by default the last 15
/// minutes ([`telemetry::window_of`]).
fn window_param(q: &std::collections::HashMap<String, String>) -> Result<(u64, u64), ApiError> {
    let at = |k: &str| param_u64(q, k, 0).map(|v| (v > 0).then_some(v));
    Ok(telemetry::window_of(at("from")?, at("to")?))
}

/// The statements of a window, by time spent, each with the window before
/// (`prev`), its keys and whether it is new ([`telemetry::Statements`]).
async fn query_stats(State(app): State<App>, Query(q): Params) -> Res {
    let (from, to) = window_param(&q)?;
    Ok(AxJson(app.svc.query_log.statements.window(from, to, param_u64(&q, "limit", 200)? as usize)))
}

/// One statement (by its id or fingerprint) over a window: a point a
/// minute, its keys, shards, slowest calls and errors.
async fn query_stat(State(app): State<App>, axum::extract::Path(which): axum::extract::Path<String>, Query(q): Params) -> Res {
    let (from, to) = window_param(&q)?;
    app.svc.query_log.statements.detail(&which, from, to).map(AxJson).ok_or_else(|| ApiError { status: 404, message: format!("no statement {which:?} in the last day") })
}

/// Starts this server's statement counts again (audited; a coordinator's
/// shards keep theirs).
async fn reset_query_stats(State(app): State<App>) -> AxJson<Json> {
    app.svc.query_log.statements.reset();
    keys::note("reset this server's statement statistics");
    AxJson(json!({"reset": true}))
}

// ----- end workload

/// `GET /v1/timeseries?since=` (memory's points, every 2 s over the last
/// hour) or `?from=&to=`: memory's points when it holds `from`, else a
/// point a minute from the log store's `load` table (an hour apart past a
/// week), with `source` saying which.
async fn timeseries(State(app): State<App>, Query(q): Params) -> Res {
    let now = telemetry::now_ms();
    let interval = app.svc.series.interval.as_millis() as u64;
    if !q.contains_key("from") {
        let since = param_u64(&q, "since", 0)?;
        return Ok(AxJson(json!({"interval_ms": interval, "points": app.svc.series.since(since), "now_ms": now, "source": "memory"})));
    }
    let from = param_u64(&q, "from", 0)?;
    let to = param_u64(&q, "to", now)?;
    if to < from {
        return Err(ApiError::bad("to comes before from"));
    }
    let memory = |error: Option<&str>| {
        let points: Vec<Arc<Json>> = app.svc.series.since(from).into_iter().filter(|p| p["t"].as_u64().is_some_and(|t| t <= to)).collect();
        let mut j = json!({"interval_ms": interval, "points": points, "now_ms": now, "from_ms": from, "to_ms": to, "source": "memory"});
        if let Some(e) = error {
            j["error"] = json!(e);
        }
        j
    };
    let oldest = app.svc.series.since(0).first().and_then(|p| p["t"].as_u64());
    if oldest.is_some_and(|o| from + interval >= o) {
        return Ok(AxJson(memory(None)));
    }
    let Some(store) = &app.svc.log_store else {
        return Ok(AxJson(memory(Some("this server keeps no log store (--log-store): only what memory holds, the last hour, is known"))));
    };
    let (rows, interval) = load_rows(store, from, to, &["qps", "error_ratio", "errors_s", "client_errors_s", "timeouts_s", "rejected_s", "p50_ms", "p99_ms", "writes_s", "cpu", "cores", "mem_bytes", "disk_used_ratio"]).await?;
    Ok(AxJson(json!({"interval_ms": interval, "points": rows, "now_ms": now, "from_ms": from, "to_ms": to, "source": "load"})))
}

/// The SQL of the `load` rows from `from` to `to` (ms) with `cols`: a row
/// a minute up to a week, else a row an hour (means; the most of
/// `p99_ms`, `cores`, `mem_bytes` and `disk_used_ratio`); and the
/// interval.
fn load_sql(from: u64, to: u64, cols: &[&str]) -> (String, u64) {
    if to.saturating_sub(from) / 60_000 <= 10_000 {
        (format!("SELECT ts, {} FROM load WHERE ts >= :from AND ts <= :to ORDER BY ts LIMIT 10000", cols.join(", ")), 60_000)
    } else {
        let agg: Vec<String> = cols.iter().map(|c| if matches!(*c, "p99_ms" | "cores" | "mem_bytes" | "disk_used_ratio") { format!("MAX({c})") } else { format!("AVG({c})") }).collect();
        (format!("SELECT hour, {} FROM load WHERE ts >= :from AND ts <= :to GROUP BY hour LIMIT 10000", agg.join(", ")), 3_600_000)
    }
}

fn load_params(from: u64, to: u64) -> strata::query::Params {
    let us = |ms: u64| strata::value::Value::Timestamp(ms.min(i64::MAX as u64 / 1000) as i64 * 1000);
    strata::query::Params::new().set("from", us(from)).set("to", us(to))
}

/// A result of [`load_sql`] as points `{"t", col...}`, oldest first: `t`
/// the end of each minute (or hour).
fn load_points(rs: &strata::query::exec::ResultSet, cols: &[&str], interval: u64) -> Vec<Json> {
    use strata::value::Value;
    let mut points: Vec<Json> = rs
        .rows
        .iter()
        .map(|r| {
            let mut p = serde_json::Map::new();
            let t = match r.first() {
                Some(Value::Timestamp(t)) => t.div_euclid(1000) as u64,
                Some(Value::Int(h)) => (*h as u64 + 1) * interval,
                _ => 0,
            };
            p.insert("t".into(), json!(t));
            for (c, v) in cols.iter().zip(r.iter().skip(1)) {
                match v {
                    Value::Float(x) if x.is_finite() => {
                        p.insert((*c).into(), json!(x));
                    }
                    Value::Int(x) => {
                        p.insert((*c).into(), json!(x));
                    }
                    _ => {}
                }
            }
            Json::Object(p)
        })
        .collect();
    points.sort_by_key(|p| p["t"].as_u64());
    points
}

/// This server's `load` rows from `from` to `to` ([`load_sql`]).
async fn load_rows(store: &crate::logstore::LogStore, from: u64, to: u64, cols: &[&str]) -> Result<(Vec<Json>, u64), ApiError> {
    let (sql, interval) = load_sql(from, to, cols);
    let (rs, _) = store.query(&sql, &load_params(from, to), false, Duration::from_secs(10)).await?;
    Ok((load_points(&rs, cols, interval), interval))
}

/// `GET /v1/load?from=&to=&copies=all` on a coordinator: each copy's load
/// a minute (cpu, disk, p99, qps) from its own log store's `load`, 16
/// copies asked at once, all in 10 s; the copies that did not answer are
/// named.
async fn copies_load(State(app): State<App>, Query(q): Params) -> Res {
    let crate::service::Backend::Cluster(c) = &app.svc.backend else {
        return Err(ApiError::bad("/v1/load is a coordinator's: it asks each copy. A shard server's own load is /v1/timeseries?from="));
    };
    if q.get("copies").is_some_and(|v| v != "all") {
        return Err(ApiError::bad("copies is all, the one choice so far"));
    }
    let now = telemetry::now_ms();
    let to = param_u64(&q, "to", now)?;
    let from = param_u64(&q, "from", to.saturating_sub(3_600_000))?;
    if to < from {
        return Err(ApiError::bad("to comes before from"));
    }
    const COLS: [&str; 6] = ["qps", "cpu", "cores", "busy", "p99_ms", "disk_used_ratio"];
    let (sql, interval) = load_sql(from, to, &COLS);
    let params = load_params(from, to);
    let snap = c.snapshot().await;
    let mut names = std::collections::HashMap::new();
    for s in snap["shards"].as_array().into_iter().flatten() {
        for r in s["replicas"].as_array().into_iter().flatten() {
            if let Some(u) = r["url"].as_str() {
                names.insert(u.to_string(), (r["short"].clone(), r["name"].clone()));
            }
        }
    }
    let gate = Arc::new(tokio::sync::Semaphore::new(16));
    let deadline = Instant::now() + Duration::from_secs(10);
    let asks = c.replica_clients().into_iter().map(|(shard, url, mut client)| {
        let (gate, sql, params) = (gate.clone(), sql.clone(), params.clone());
        async move {
            let answer = async {
                let _held = tokio::time::timeout_at(deadline.into(), gate.acquire_owned()).await.map_err(|_| ApiError { status: 504, message: "not asked within 10 s".into() })?.map_err(ApiError::internal)?;
                let req = crate::pb::QueryRequest { sql, params: crate::grpc::params_to_pb(&params), shard: false, timeout_ms: deadline.saturating_duration_since(Instant::now()).as_millis() as u32, ..Default::default() };
                match tokio::time::timeout_at(deadline.into(), client.logs(tonic::Request::new(req))).await {
                    Err(_) => Err(ApiError { status: 504, message: "no answer within 10 s".into() }),
                    Ok(Err(s)) if s.code() == tonic::Code::Unimplemented => Err(ApiError { status: 501, message: "an older server, without a log store".into() }),
                    Ok(Err(s)) => Err(ApiError::from(s)),
                    Ok(Ok(a)) => crate::grpc::result_from_pb(a.into_inner()),
                }
            }
            .await;
            (shard, url, answer)
        }
    });
    let answers = futures::future::join_all(asks).await;
    let (mut copies, mut missing) = (Vec::new(), Vec::new());
    for (shard, url, answer) in answers {
        let (short, name) = names.get(&url).cloned().unwrap_or((Json::Null, Json::Null));
        match answer {
            Ok(rs) => copies.push(json!({"shard": shard, "url": url, "short": short, "name": name, "points": load_points(&rs, &COLS, interval)})),
            Err(e) => missing.push(json!({"shard": shard, "url": url, "short": short, "error": e.message})),
        }
    }
    Ok(AxJson(json!({"from_ms": from, "to_ms": to, "interval_ms": interval, "asked": copies.len() + missing.len(), "answered": copies.len(), "copies": copies, "missing": missing})))
}

async fn logs(State(app): State<App>, Query(q): Params) -> Res {
    let (after, limit) = (param_u64(&q, "after", 0)?, param_u64(&q, "limit", 500)?.min(obs::RING_LINES as u64) as usize);
    let ring = obs::ring();
    let lines = ring.lines(after, q.get("level").map(String::as_str), q.get("request").map(String::as_str), q.get("q").map(String::as_str).filter(|t| !t.is_empty()), limit);
    let level = app.svc.log_filter_json();
    Ok(AxJson(json!({"lines": lines, "last": ring.last(), "filter": obs::log_filter(), "filter_expires_ms": level["expires_ms"], "filter_set_by": level["set_by"]})))
}

/// `GET /v1/logs/filter`: the log filter, until when and by whom it was set.
async fn log_filter_now(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.log_filter_json())
}

/// `PUT /v1/logs/filter?node= {"filter", "expires_s"?}`: this server's
/// log filter (or on a coordinator, a copy's, by its URL or short name)
/// for `expires_s` (30 min by default; 0: until set again), then back.
async fn log_filter(State(app): State<App>, w: Option<axum::Extension<Principal>>, Query(q): Params, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let filter = body_str(&b, "filter")?;
    let expires_s = match b.get("expires_s") {
        None | Some(Json::Null) => None,
        Some(v) => Some(v.as_u64().filter(|s| *s <= 7 * 86_400).ok_or_else(|| ApiError::bad("expires_s is a number of seconds up to a week (0: until set again)"))?),
    };
    let by = keys::audit_name(&who(w));
    let until = expires_s.unwrap_or(crate::service::LOG_FILTER_S);
    keys::note_change(keys::Change { object: Some("log filter".into()), before: Some(json!(obs::log_filter())), after: Some(json!({"filter": filter, "expires_s": until})), ..Default::default() });
    if let Some(node) = q.get("node").filter(|n| !n.is_empty()) {
        let crate::service::Backend::Cluster(c) = &app.svc.backend else {
            return Err(ApiError::bad("node names a copy of a coordinator's cluster; this server sets its own log filter without it"));
        };
        let url = copy_of(&*c.snapshot().await, node)?.url;
        let a = crate::profiling::Asked { kind: crate::profiling::Kind::LogFilter, body: json!({"filter": filter, "expires_s": until, "by": by}), folded: false, in_use: false };
        let j = match c.profile(&url, a).await? {
            crate::profiling::Answer::Json(j) => j,
            crate::profiling::Answer::Folded(..) => Json::Null,
        };
        return Ok(audited(j, format!("log filter {filter} on {node} for {} min", until.div_ceil(60))));
    }
    let j = app.svc.set_log_level(filter, Some(until), &by)?;
    Ok(audited(j, format!("log filter {filter} for {} min", until.div_ceil(60))))
}

/// A query of the log store: this server's, or on a coordinator every
/// server's merged ([`crate::logstore::cluster_query`]) unless `"scope":
/// "local"`.
async fn log_query(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let sql = body_str(&b, "sql")?;
    let params = app.svc.params(b.get("params")).await?;
    let timeout = app.svc.timeout_of(body_u64(&b, "timeout_ms"));
    let local = match b.get("scope").and_then(|v| v.as_str()) {
        None | Some("cluster") => false,
        Some("local") => true,
        Some(other) => return Err(ApiError::bad(format!("scope is \"cluster\" or \"local\", not {other:?}"))),
    };
    let store = app.svc.log_store.as_ref();
    match &app.svc.backend {
        crate::service::Backend::Cluster(c) if !local => Ok(AxJson(crate::logstore::cluster_query(store, c, sql, &params, timeout).await?)),
        _ => {
            let store = store.ok_or_else(crate::logstore::none_here)?;
            let (rs, took_ms) = store.query(sql, &params, false, timeout).await?;
            let mut j = js::result(&rs, took_ms);
            j["nodes"] = json!([{"node": store.options().node, "rows": rs.rows.len(), "ms": took_ms}]);
            Ok(AxJson(j))
        }
    }
}

/// The log store: this server's (`store`, null without one), its tables
/// (`schemas`, in the short JSON form), and on a coordinator every
/// server's (`nodes`), as each answered `Health`.
async fn log_store(State(app): State<App>) -> AxJson<Json> {
    let schemas: Vec<Json> = crate::logstore::schemas().iter().map(|s| json!({"table": s.table, "short": s.to_short_json(), "sql": strata::query::ddl::create_sql(s)})).collect();
    let mut j = json!({"store": app.svc.log_store.as_ref().map(|s| s.status()), "schemas": schemas});
    if let crate::service::Backend::Cluster(c) = &app.svc.backend {
        let cluster = c.cluster().await;
        let nodes: Vec<Json> = cluster["shards"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|s| {
                let shard = s["shard"].clone();
                s["replicas"].as_array().cloned().unwrap_or_default().into_iter().map(move |r| json!({"shard": shard, "url": r["url"], "status": r["status"], "error": r["error"], "store": r["detail"]["log_store"]}))
            })
            .collect();
        j["nodes"] = json!(nodes);
    }
    AxJson(j)
}

async fn system(State(app): State<App>) -> AxJson<Json> {
    let mut j = app.svc.system();
    j["auth"] = json!({"token": app.token.is_some(), "cluster_token": app.cluster.is_some()});
    j["logs"] = json!({"filter": obs::log_filter()});
    AxJson(j)
}

async fn cluster(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.cluster().await)
}

/// `GET /v1/kube`: the `StrataCluster` that runs this coordinator on
/// Kubernetes (its spec, and the operator's status), or `managed: false`.
async fn kube(State(app): State<App>) -> Res {
    let mut j = match &app.svc.kube {
        None => json!({"managed": false}),
        Some(l) => l.get().await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?,
    };
    // core: the images the copies run, and how far a rollout is.
    if let crate::service::Backend::Cluster(c) = &app.svc.backend {
        j["versions"] = json!(copy_versions(&*c.snapshot().await));
    }
    if let Some(p) = rollout_progress(&app, &j["status"]) {
        j["progress"] = p;
    }
    Ok(AxJson(j))
}

/// The images the cluster's copies run, one entry a digest (or image,
/// or build when no image is known), the most run first.
fn copy_versions(snap: &Json) -> Vec<Json> {
    let mut by: Vec<Json> = Vec::new();
    for r in snap["shards"].as_array().into_iter().flatten().flat_map(|s| s["replicas"].as_array().into_iter().flatten()) {
        if r["status"] == "down" {
            continue;
        }
        let v = &r["version"];
        let key = v["digest"].as_str().or(v["image"].as_str()).or(v["version"].as_str()).unwrap_or("unknown").to_string();
        let short = r["short"].clone();
        match by.iter_mut().find(|e| e["key"] == key.as_str()) {
            Some(e) => {
                e["count"] = json!(e["count"].as_u64().unwrap_or(0) + 1);
                e["copies"].as_array_mut().expect("a list").push(short);
            }
            None => by.push(json!({
                "key": key, "digest": v["digest"], "short": v["digest"].as_str().map(crate::operator::upgrade::short), "image": v["image"],
                "version": v["version"], "revision": v["revision"], "count": 1, "copies": [short],
            })),
        }
    }
    by.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
    for e in &mut by {
        if let Some(o) = e.as_object_mut() {
            o.remove("key");
        }
    }
    by
}

/// How far the rollout of `status.rollout` is, when one runs: copies
/// done of the total, seconds a copy so far, the time left at that pace,
/// since when the coordinator saw it run, and the step it waits on.
fn rollout_progress(app: &App, status: &Json) -> Option<Json> {
    let r = &status["rollout"];
    let total = r["copies"].as_u64()?;
    let stale = r["stale"].as_array().map_or(0, |v| v.len() as u64);
    let version = r["version"].as_str().unwrap_or_default().to_string();
    let now = telemetry::now_ms();
    let mut seen = app.svc.rollout_seen.lock().unwrap_or_else(|e| e.into_inner());
    let running = stale > 0 || r["step"].is_object();
    if !running {
        // A rollout seen to its end: its pace is the next one's estimate.
        if let Some((v, started, at_start)) = seen.take() {
            if v == version && at_start > 0 {
                *app.svc.rollout_pace_s.lock().unwrap_or_else(|e| e.into_inner()) = Some(now.saturating_sub(started) as f64 / 1e3 / at_start as f64);
            }
        }
        return None;
    }
    let (started, at_start) = match seen.as_ref() {
        Some((v, s, n)) if *v == version => (*s, *n),
        _ => {
            let since = r["step"]["sinceMs"].as_u64().filter(|t| *t > 0 && *t <= now).unwrap_or(now);
            *seen = Some((version, since, stale.max(1)));
            (since, stale.max(1))
        }
    };
    let done_here = at_start.saturating_sub(stale);
    let elapsed_s = now.saturating_sub(started) as f64 / 1e3;
    let per_copy_s = (done_here > 0).then(|| elapsed_s / done_here as f64).or(*app.svc.rollout_pace_s.lock().unwrap_or_else(|e| e.into_inner()));
    let phase = if r["held"].is_object() {
        "held".to_string()
    } else if let Some(s) = r["step"]["name"].as_str() {
        s.to_string()
    } else if r["verify"].is_object() {
        "verify".into()
    } else {
        "restarting".into()
    };
    Some(json!({"done": total.saturating_sub(stale), "total": total, "per_copy_s": per_copy_s.map(|s| (s * 10.0).round() / 10.0), "eta_s": per_copy_s.map(|s| (s * stale as f64).round()), "started_ms": started, "phase": phase}))
}

/// `GET /v1/admin/kube/versions?limit=30`: the images of the cluster's
/// repository (the StrataCluster's, else `--image-repository`) a person
/// may pick, newest first, each marked current, newer and tracked.
/// Without Kubernetes too, for the picker to be shown read-only.
async fn kube_versions(State(app): State<App>, Query(q): Params) -> Res {
    use crate::operator::upgrade::{self as up, Ref};
    let limit = param_u64(&q, "limit", 30)?.clamp(1, 500) as usize;
    let mut kube_error = None;
    let spec = match &app.svc.kube {
        Some(l) => match l.get().await {
            Ok(now) => Some(now["spec"].clone()),
            Err(e) => {
                kube_error = Some(format!("the Kubernetes API: {e}"));
                None
            }
        },
        None => None,
    };
    let image = spec.as_ref().and_then(|s| s["image"].as_str().map(String::from)).or_else(|| app.svc.place.image.clone());
    let repository = image.as_deref().and_then(|i| Ref::parse(i).ok()).map(|r| r.repository()).filter(|_| spec.is_some()).unwrap_or_else(|| app.svc.image_repository.clone());
    let upgrade: up::Upgrade = spec.as_ref().and_then(|s| serde_json::from_value(s["upgrade"].clone()).ok()).unwrap_or_default();
    let tracked = upgrade.tracked(image.as_deref().unwrap_or(&format!("{repository}:latest")));
    let track = Ref::parse(&tracked).ok().and_then(|r| r.tag).unwrap_or_else(|| "latest".into());
    // What runs now: the spec's digest, else the one most copies run, else
    // this server's own.
    let mut digest = image.as_deref().and_then(up::digest_of);
    if digest.is_none() {
        if let crate::service::Backend::Cluster(c) = &app.svc.backend {
            digest = copy_versions(&*c.snapshot().await).first().and_then(|v| v["digest"].as_str().map(String::from));
        }
    }
    let digest = digest.or_else(|| app.svc.place.digest.clone());
    let current = json!({"image": image, "digest": digest});
    let mut j = match up::Versions::get().list(&up::Registry::new(), &repository).await {
        Ok((offered, error)) => up::versions_json(&repository, &track, &current, &offered, error, limit),
        Err(e) => json!({"repository": repository, "track": track, "current": current, "images": [], "error": format!("the registry: {e}")}),
    };
    j["managed"] = json!(app.svc.kube.is_some());
    if let Some(e) = kube_error {
        j["kube_error"] = json!(e);
    }
    Ok(AxJson(j))
}

/// The spec patch a `POST /v1/admin/kube` or `/v1/admin/kube/plan` body
/// asks for of the `StrataCluster` `now`: `{"spec": {...}}` (any of the
/// spec's fields), `{"rollback": version}` (the spec of that version in
/// its `status.history`), or the sizes alone (`{"shards"?, "replicas"?,
/// "coordinators"?}`).
fn kube_patch(b: &Json, now: &Json) -> Result<Json, ApiError> {
    if let Some(v) = b.get("rollback") {
        let v = v.as_u64().ok_or_else(|| ApiError::bad("rollback is the number of a version in the cluster's history"))?;
        return crate::operator::kube::Link::rollback(now, v).map_err(ApiError::bad);
    }
    if let Some(spec) = b.get("spec") {
        return match spec {
            Json::Object(_) => Ok(spec.clone()),
            _ => Err(ApiError::bad("spec is an object: the StrataCluster's spec fields to change")),
        };
    }
    let n = |k: &str| match b.get(k) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v.as_u64().map(|n| Some(n as usize)).ok_or_else(|| ApiError::bad(format!("{k} is a number"))),
    };
    crate::operator::kube::Link::sizes(n("shards")?, n("replicas")?, n("coordinators")?).map_err(ApiError::bad)
}

/// The patch body `b` asks for, the spec it makes, and the plan of the
/// change ([`crate::operator::plan`]).
async fn kube_planned(l: &crate::operator::kube::Link, b: &Json) -> Result<(Json, crate::operator::Spec, Json), ApiError> {
    let now = l.get().await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?;
    let patch = kube_patch(b, &now)?;
    let (old, new) = crate::operator::patched(&now["spec"], &patch).map_err(ApiError::bad)?;
    let mut plan = crate::operator::plan(&old, &new, old.shards, old.replicas);
    if let Some(v) = b.get("rollback") {
        plan["rollback"] = v.clone();
        plan["patch"] = patch.clone();
        if let Some(w) = plan["warnings"].as_array_mut() {
            w.push(json!("a rollback: no gate holds it, so that it goes through while the version it leaves is unwell"));
        }
    }
    Ok((patch, new, plan))
}

fn kube_link(app: &App) -> Result<&crate::operator::kube::Link, ApiError> {
    app.svc.kube.as_ref().ok_or_else(|| ApiError::bad("this coordinator is not run by the Kubernetes operator: change the cluster with /v1/admin/shards"))
}

/// `POST /v1/admin/kube/plan {"spec": {...}}` (or `{"rollback": version}`):
/// what changing the `StrataCluster`'s spec so would change, the
/// operator's steps for it and what to know first; nothing is changed.
async fn kube_plan(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    let l = kube_link(&app)?;
    let (_, _, plan) = kube_planned(l, &b).await?;
    Ok(AxJson(plan))
}

/// `POST /v1/admin/kube {"spec": {...}}` (or the sizes alone, or
/// `{"rollback": version}`): changes the `StrataCluster`'s spec, checked
/// as the operator would, and notes who did; the operator then makes the
/// change. A rollback is held by no gate.
async fn kube_change(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let l = kube_link(&app)?;
    let (patch, new, plan) = kube_planned(l, &b).await?;
    if let Some(why) = plan["refused"].as_str() {
        return Err(ApiError::bad(why.to_string()));
    }
    use crate::operator::kube::{CHANGED_BY, ROLLBACK};
    let mut notes = vec![(CHANGED_BY, format!("{} {}", who(w).name, new.version()))];
    let back = b.get("rollback").and_then(Json::as_u64);
    if back.is_some() {
        notes.push((ROLLBACK, new.version()));
    }
    let j = l.change(&patch, &notes).await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?;
    let said: Vec<String> = plan["changes"].as_array().into_iter().flatten().map(|c| format!("{} {} -> {}", c["field"].as_str().unwrap_or("?"), c["from"], c["to"])).collect();
    let said = if said.is_empty() { "nothing".into() } else { said.join(", ") };
    Ok(audited(j, match back {
        Some(v) => format!("rolled the StrataCluster {}/{} back to version {v}: {said}", l.namespace, l.name),
        None => format!("changed the StrataCluster {}/{}: {said}", l.namespace, l.name),
    }))
}

/// `POST /v1/admin/kube/upgrade {"image"?, "plan"?, "force"?}`: rolls the
/// cluster out to `image`, or to the newest image of the tag the upgrader
/// follows (`status.upgrade.latest`, pinned by its digest), as a change
/// of `spec.image` would: the image is tried on every node first, each
/// server restarts in turn while the others serve, and the version is
/// verified, then rolled back if it fails (`spec.rollout.onFailure`).
/// With `plan`, nothing is changed: the plan is answered. An image the
/// operator rolled back once is refused unless `force`.
async fn kube_upgrade(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let l = kube_link(&app)?;
    let now = l.get().await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?;
    let up = &now["status"]["upgrade"];
    let image = match b.get("image") {
        Some(Json::String(i)) if !i.trim().is_empty() => i.trim().to_string(),
        Some(Json::Null) | None => up["latest"]["image"].as_str().map(str::to_string).ok_or_else(|| {
            ApiError::bad(match up["error"].as_str() {
                Some(e) => format!("the registry could not be asked what the tag names ({e}): name the image"),
                None => "no newer image is known: name the image, or set spec.upgrade.mode to suggest so that the operator asks the registry what its tag names".into(),
            })
        })?,
        Some(_) => return Err(ApiError::bad("image is an image reference: ghcr.io/chloetech-ai/strata-server:TAG or …@sha256:DIGEST")),
    };
    crate::operator::upgrade::Ref::parse(&image).map_err(|e| ApiError::bad(format!("image {image}: {e}")))?;
    if now["spec"]["image"].as_str() == Some(image.as_str()) {
        return Err(ApiError::bad(format!("the cluster's spec names {image} already: nothing to roll out")));
    }
    let digest = crate::operator::upgrade::digest_of(&image);
    let refused = up["rejected"].as_array().into_iter().flatten().find(|r| digest.is_some() && r["digest"].as_str() == digest.as_deref());
    if let (Some(r), false) = (refused, b["force"].as_bool().unwrap_or(false)) {
        return Err(ApiError::bad(format!("{image} was rolled back once: {}; pass force to try it again", r["why"].as_str().unwrap_or("it failed a check"))));
    }
    let ask = json!({"spec": {"image": image}});
    let (patch, new, mut plan) = kube_planned(l, &ask).await?;
    let from = now["spec"]["image"].as_str().unwrap_or_default().to_string();
    let latest = &up["latest"];
    if latest["image"].as_str() == Some(image.as_str()) {
        plan["image"] = latest.clone();
    }
    plan["from"] = json!(from);
    upgrade_estimates(&app, &now, &image, &mut plan).await;
    if b["plan"].as_bool().unwrap_or(false) {
        return Ok(AxJson(plan).into_response());
    }
    if let Some(why) = plan["refused"].as_str() {
        return Err(ApiError::bad(why.to_string()));
    }
    use crate::operator::kube::CHANGED_BY;
    let j = l.change(&patch, &[(CHANGED_BY, format!("{} {}", who(w).name, new.version()))]).await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?;
    Ok(audited(j, format!("upgraded the StrataCluster {}/{} from {from} to {image}", l.namespace, l.name)))
}

/// What an upgrade to `image` restarts and takes: the copies whose image
/// differs (`restarts`, named in `restart_copies`), and estimates of the
/// rollout and of a rollback, in seconds: the last rollout's pace a copy
/// when the coordinator saw one, else 30 s a copy to restart and catch
/// up, with the spec's soak, the pre-flight and the verification.
async fn upgrade_estimates(app: &App, now: &Json, image: &str, plan: &mut Json) {
    let digest = crate::operator::upgrade::digest_of(image);
    let mut differ = Vec::new();
    if let crate::service::Backend::Cluster(c) = &app.svc.backend {
        let snap = c.snapshot().await;
        for r in snap["shards"].as_array().into_iter().flatten().flat_map(|s| s["replicas"].as_array().into_iter().flatten()) {
            let v = &r["version"];
            let same = match &digest {
                Some(d) => v["digest"].as_str() == Some(d.as_str()),
                None => v["image"].as_str() == Some(image),
            };
            if !same {
                differ.push(r["short"].clone());
            }
        }
    }
    let rollout: crate::operator::RolloutSpec = serde_json::from_value(now["spec"]["rollout"].clone()).unwrap_or_default();
    let coordinators = now["spec"]["coordinators"].as_u64().unwrap_or(1) as f64;
    let pace = *app.svc.rollout_pace_s.lock().unwrap_or_else(|e| e.into_inner());
    let per_copy = pace.unwrap_or(30.0 + rollout.soak_seconds as f64);
    let n = differ.len() as f64;
    let (preflight, coordinator_s) = (60.0, 20.0);
    plan["restarts"] = json!(differ.len());
    plan["restart_copies"] = json!(differ);
    plan["estimate_s"] = json!((preflight + n * per_copy + coordinators * coordinator_s + rollout.verify_seconds as f64).round());
    plan["rollback_estimate_s"] = json!((n * per_copy + coordinators * coordinator_s).round());
    plan["estimate_from"] = json!(match pace {
        Some(p) => format!("the last rollout's pace, {p:.0} s a copy"),
        None => format!("about {per_copy:.0} s a copy to restart and catch up (with spec.rollout.soakSeconds), 60 s of pre-flight, 20 s a coordinator and spec.rollout.verifySeconds"),
    });
}

/// `POST /v1/admin/kube/resume`: lifts the hold a gate put on the
/// operator's change; the gates count again from now.
async fn kube_resume(State(app): State<App>) -> Result<Response, ApiError> {
    let l = kube_link(&app)?;
    let held = l.get().await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?["status"]["rollout"]["held"]["text"].as_str().unwrap_or_default().to_string();
    let j = l.resume().await.map_err(ApiError::bad)?;
    Ok(audited(j, format!("resumed the change of the StrataCluster {}/{}, held as {held}", l.namespace, l.name)))
}

/// `POST /v1/admin/kube/autoscale/dismiss`: the autoscaler's suggestion
/// is not made; its rules wait their cooldown before they suggest again.
async fn autoscale_dismiss(State(app): State<App>) -> Result<Response, ApiError> {
    let l = kube_link(&app)?;
    let now = l.get().await.map_err(|e| ApiError { status: 502, message: format!("the Kubernetes API: {e}") })?;
    let text = now["status"]["autoscale"]["suggestion"]["text"].as_str().unwrap_or_default().to_string();
    let j = l.dismiss().await.map_err(ApiError::bad)?;
    Ok(audited(j, format!("dismissed the autoscaler's suggestion for the StrataCluster {}/{}: {text}", l.namespace, l.name)))
}

/// The steps of load the servers kept since `since_ms` (each log store's
/// `load`, a row a server a minute), ten minutes each: the busiest
/// shard's copies, how busy and how many, and the fullest volume. And
/// the servers that answered.
async fn load_steps(app: &App, since_ms: u64) -> Result<(Vec<crate::operator::autoscale::Step>, Json), ApiError> {
    let sql = "SELECT step, shard, SUM(busy), COUNT(*), MIN(minute), MAX(minute), MAX(disk_used_ratio) FROM load \
               WHERE ts > :since AND role IN ('primary', 'replica', 'standalone') GROUP BY step, shard LIMIT 10000";
    let params = strata::query::Params::new().set("since", strata::value::Value::Timestamp(since_ms as i64 * 1000));
    let timeout = app.svc.timeout_of(0);
    let store = app.svc.log_store.as_ref();
    let j = match &app.svc.backend {
        crate::service::Backend::Cluster(c) => crate::logstore::cluster_query(store, c, sql, &params, timeout).await?,
        _ => {
            let store = store.ok_or_else(crate::logstore::none_here)?;
            let (rs, ms) = store.query(sql, &params, false, timeout).await?;
            let mut j = js::result(&rs, ms);
            j["nodes"] = json!([{"node": store.options().node, "rows": rs.rows.len(), "ms": ms}]);
            j
        }
    };
    // By step: (busy, copies) of each shard, and the fullest volume.
    let mut by: std::collections::BTreeMap<u64, (f64, f64, Option<f64>)> = Default::default();
    for r in j["rows"].as_array().into_iter().flatten() {
        let (Some(step), Some(sum), Some(n)) = (r[0].as_u64(), r[2].as_f64(), r[3].as_f64()) else { continue };
        let minutes = (r[5].as_f64().unwrap_or(0.0) - r[4].as_f64().unwrap_or(0.0) + 1.0).max(1.0);
        let (busy, copies) = (sum / n.max(1.0), n / minutes);
        let e = by.entry(step).or_insert((f64::MIN, 0.0, None));
        if busy > e.0 {
            (e.0, e.1) = (busy, copies);
        }
        if let Some(d) = r[6].as_f64() {
            e.2 = Some(e.2.map_or(d, |x: f64| x.max(d)));
        }
    }
    let steps = by.into_iter().map(|(step, (busy, copies, full))| crate::operator::autoscale::Step { at_ms: step * 600_000, busy, copies, full }).collect();
    Ok((steps, j["nodes"].clone()))
}

/// `POST /v1/admin/autoscale/replay {"autoscale": rules, "days"?}`: the
/// rules ([`crate::operator::autoscale`]) on the load the servers kept of
/// the last `days` (7 at most, the log stores' retention): how busy the
/// copies would have been, how many there would have been, and each
/// change, beside the cluster at its size. Nothing is changed. The sizes
/// it starts from are the `StrataCluster`'s, or the cluster's as it is,
/// or `sizes` (`{replicas, shards, volume}`).
async fn autoscale_replay(State(app): State<App>, AxJson(b): AxJson<Json>) -> Res {
    use crate::operator::autoscale::{replay, Autoscale, Sizes};
    let rules: Autoscale = serde_json::from_value(b.get("autoscale").cloned().unwrap_or(json!({}))).map_err(|e| ApiError::bad(format!("autoscale: {e}")))?;
    rules.check().map_err(ApiError::bad)?;
    let days = b.get("days").and_then(Json::as_f64).unwrap_or(7.0);
    if !(days > 0.0 && days <= 7.0) {
        return Err(ApiError::bad(format!("days is more than 0 and at most 7 (the log stores keep a week), not {days}")));
    }
    let spec = match &app.svc.kube {
        Some(l) => l.get().await.ok().and_then(|c| serde_json::from_value::<crate::operator::Spec>(c["spec"].clone()).ok()),
        None => None,
    };
    let cluster = app.svc.cluster().await;
    let shards = cluster["shards"].as_array();
    let given = &b["sizes"];
    let n = |k: &str| given[k].as_u64().map(|v| v as usize).filter(|v| *v > 0);
    let start = Sizes {
        replicas: n("replicas").or(spec.as_ref().map(|s| s.replicas)).or_else(|| shards?.iter().filter_map(|s| s["replicas"].as_array().map(Vec::len)).max()).unwrap_or(1),
        shards: n("shards").or(spec.as_ref().map(|s| s.shards)).or_else(|| shards.map(Vec::len)).unwrap_or(1).max(1),
        volume: given["volume"].as_str().map(String::from).or(spec.as_ref().map(|s| s.storage.size.clone())).unwrap_or_else(|| "10Gi".into()),
    };
    let now = crate::telemetry::now_ms();
    // A step a shard a row: at most 10,000 rows.
    let days = days.min(10_000.0 / (144.0 * start.shards as f64));
    let since = now.saturating_sub((days * 86_400_000.0) as u64);
    let (steps, nodes) = load_steps(&app, since).await?;
    let mut j = replay(&rules, &steps, &start);
    j["rules"] = json!(rules.said());
    j["sizes"] = json!({"replicas": start.replicas, "shards": start.shards, "volume": start.volume});
    j["sinceMs"] = json!(since);
    j["days"] = json!(days);
    j["nodes"] = nodes;
    j["managed"] = json!(spec.is_some());
    Ok(AxJson(j))
}

/// An AWS failure: 400 without credentials or for a region that is no
/// region's name, else 502 (AWS's answer).
fn aws_error(e: String) -> ApiError {
    ApiError { status: if e.starts_with("no AWS credentials") || e.contains("is not a region") { 400 } else { 502 }, message: e }
}

/// The cloud view's AWS state: the credentials as far as they may be shown (the
/// key id masked; never the secret nor the session token).
async fn aws_status(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.aws.status())
}

/// `{"access_key_id", "secret_access_key", "session_token"?, "region"}`:
/// checked with AWS (STS), then kept on this server. Answers who they are.
async fn aws_save(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let c: crate::aws::Creds = serde_json::from_value(b).map_err(|e| ApiError::bad(format!("the body is {{access_key_id, secret_access_key, session_token?, region}}: {e}")))?;
    let key = crate::aws::mask(&c.access_key_id);
    let who = who(w);
    // Refused or malformed credentials are the caller's; failing to write them, ours.
    let i = app.svc.aws.set(c, &who.name).await.map_err(|e| ApiError { status: if e.starts_with("write ") { 500 } else { 400 }, message: e })?;
    let mut j = app.svc.aws.status();
    j["identity"] = json!(i);
    Ok(audited(j, format!("saved the AWS credentials {key} (account {}, {})", i.account, i.arn)))
}

async fn aws_forget(State(app): State<App>) -> Result<Response, ApiError> {
    let was = app.svc.aws.forget().map_err(ApiError::internal)?;
    let detail = match &was {
        Some(k) => format!("forgot the AWS credentials {k}"),
        None => "forgot the AWS credentials (none were saved)".into(),
    };
    Ok(audited(json!({"forgot": was, "status": app.svc.aws.status()}), detail))
}

async fn aws_regions(State(app): State<App>) -> Result<AxJson<Json>, ApiError> {
    Ok(AxJson(json!({"regions": app.svc.aws.regions().await.map_err(aws_error)?})))
}

/// `?region=` (default: the credentials' region), `&fresh=1` to fetch
/// again: the region's machines with their prices, and the account's
/// quotas there.
async fn aws_machines(State(app): State<App>, Query(q): Query<std::collections::HashMap<String, String>>) -> Result<AxJson<Json>, ApiError> {
    let region = match q.get("region").filter(|r| !r.is_empty()) {
        Some(r) => r.clone(),
        None => app.svc.aws.creds().map(|(c, _)| c.region).ok_or_else(|| aws_error("no AWS credentials: save an access key in the Cloud view (PUT /v1/admin/aws/credentials)".into()))?,
    };
    let fresh = q.get("fresh").is_some_and(|v| v != "0" && v != "false");
    let cat = app.svc.aws.catalog(&region, fresh).await.map_err(aws_error)?;
    let mut j = json!(&*cat);
    let recommended: Vec<&str> = crate::aws::catalog::recommended(&cat.machines).iter().map(|m| m.name.as_str()).collect();
    j["recommended"] = json!(recommended);
    Ok(AxJson(j))
}

async fn aws_planned(app: &App, b: Json) -> Result<(crate::aws::stack::Plan, Json), ApiError> {
    let p: crate::aws::stack::Plan = serde_json::from_value(b).map_err(|e| ApiError::bad(format!("a plan: {e}")))?;
    let cat = app.svc.aws.catalog(&p.region, false).await.map_err(aws_error)?;
    let who = app.svc.aws.principal().await.map_err(aws_error)?;
    let j = crate::aws::stack::plan(&p, &cat, &who).map_err(ApiError::bad)?;
    Ok((p, j))
}

/// A plan of a cluster on EKS ([`crate::aws::stack`]): where its copies
/// go, its cost, its warnings, its CloudFormation template and its
/// StrataCluster. Creates nothing.
async fn aws_plan(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<AxJson<Json>, ApiError> {
    Ok(AxJson(aws_planned(&app, b).await?.1))
}

/// The plan, and CloudFormation's check of its template (free; creates
/// nothing).
async fn aws_validate(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<AxJson<Json>, ApiError> {
    let (p, mut j) = aws_planned(&app, b).await?;
    j["validated"] = match app.svc.aws.validate(&p.region, &j["template"]).await {
        Ok(v) => json!({"ok": true, "answer": v}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    Ok(AxJson(j))
}

/// The clusters the cloud view made ([`crate::cloud::clusters`]), the
/// newest first, each with where it is, what it has cost so far and how
/// to reach it.
async fn cloud_list(State(app): State<App>) -> AxJson<Json> {
    let list: Vec<Json> = app.svc.clouds.list().iter().map(crate::cloud::clusters::Made::view).collect();
    AxJson(json!({"clusters": list, "file": app.svc.clouds.file().map(|f| f.display().to_string())}))
}

async fn cloud_one(State(app): State<App>, axum::extract::Path(id): axum::extract::Path<String>) -> Result<AxJson<Json>, ApiError> {
    let m = app.svc.clouds.get(&id).ok_or_else(|| ApiError { status: 404, message: format!("no cluster {id}") })?;
    Ok(AxJson(m.view()))
}

/// `{"provider", "plan", "confirm"}`: makes the plan's cluster, which bills
/// the account until it is deleted. `confirm` is the cluster's name, typed
/// again. Refused when the account's limit would not start the nodes, or
/// when the cloud already has a cluster of that name. 202: the cluster is
/// on its way; `GET` it to follow.
async fn cloud_create(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    use crate::cloud::clusters::{Made, Phase};
    let provider = b["provider"].as_str().unwrap_or_default().to_string();
    let plan = b["plan"].clone();
    let refuse = |m: String| ApiError { status: 409, message: m };
    let (j, nodes) = match provider.as_str() {
        "aws" => {
            let (p, j) = aws_planned(&app, plan).await?;
            let (needs, may) = (j["quota"]["needs"].as_f64().unwrap_or(0.0), j["quota"]["vcpus"].as_f64());
            if may.is_some_and(|q| needs > q) {
                return Err(refuse(format!("the nodes need {needs} vCPUs of {} instances and the account may run {} in {}: ask AWS for more first (Service Quotas)", j["quota"]["kind"].as_str().unwrap_or("standard"), may.unwrap_or(0.0), p.region)));
            }
            if let Some(s) = app.svc.aws.stack(&p.region, j["stack"].as_str().unwrap_or_default()).await.map_err(aws_error)? {
                if s.status != "DELETE_COMPLETE" {
                    return Err(refuse(format!("{} already has a stack named {} ({}): pick another name", p.region, j["stack"].as_str().unwrap_or_default(), s.status)));
                }
            }
            (j, p.nodes)
        }
        "digitalocean" => {
            let p: crate::digitalocean::plan::Plan = serde_json::from_value(plan).map_err(|e| ApiError::bad(format!("a plan: {e}")))?;
            let cat = app.svc.digitalocean.catalog(&p.region, true).await.map_err(do_error)?;
            let j = crate::digitalocean::plan::plan(&p, &cat).map_err(ApiError::bad)?;
            let q = &j["quota"];
            if let (Some(limit), Some(running)) = (q["droplets"].as_u64(), q["running"].as_u64()) {
                if running + u64::from(p.nodes) > limit {
                    return Err(refuse(format!("the nodes need {} droplets and the account runs {running} of its {limit}: ask DigitalOcean for more first", p.nodes)));
                }
            }
            if app.svc.digitalocean.cluster_named(j["cluster_name"].as_str().unwrap_or_default()).await.map_err(do_error)?.is_some() {
                return Err(refuse(format!("the account already has a cluster named {}: pick another name", j["cluster_name"].as_str().unwrap_or_default())));
            }
            (j, p.nodes)
        }
        _ => return Err(ApiError::bad("provider is aws or digitalocean")),
    };
    let cluster = j["cluster_name"].as_str().unwrap_or_default().to_string();
    if b["confirm"].as_str() != Some(cluster.as_str()) {
        return Err(ApiError::bad(format!("confirm is the cluster's name, {cluster}, typed again: making it bills the account until it is deleted")));
    }
    let who = who(w);
    let now = crate::operator::now_ms();
    let m = Made {
        id: format!("{provider}-{cluster}-{now}"),
        provider: provider.clone(),
        name: j["cluster"]["name"].as_str().map(str::to_string).unwrap_or_else(|| cluster.trim_start_matches("strata-").to_string()),
        cluster: cluster.clone(),
        region: j["region"].as_str().unwrap_or_default().to_string(),
        machine: j["machine"]["type"].as_str().or(j["machine"]["name"].as_str()).unwrap_or_default().to_string(),
        nodes,
        cost_hour: j["cost"]["hour"].as_f64().unwrap_or(0.0),
        by: who.name.clone(),
        created_ms: now,
        phase: Phase::Creating,
        cloud_id: None,
        request: if provider == "aws" { j["template"].clone() } else { j["request"].clone() },
        image: j["cluster"]["image"].as_str().unwrap_or(crate::cloud::IMAGE).to_string(),
        objects: j["objects"].as_array().cloned().unwrap_or_default(),
        steps: Vec::new(),
        error: None,
        strata: Json::Null,
        progress: None,
        ready_ms: None,
        ended_ms: None,
        generation: 0,
        tries: 0,
        checked_ms: 0,
        deleting_since_ms: None,
        volumes_gone: false,
        delete_asked: false,
    };
    let m = app.svc.clouds.add(m).map_err(refuse)?;
    app.svc.clouds.start(app.svc.aws.clone(), app.svc.digitalocean.clone());
    let what = if provider == "aws" { "AWS" } else { "DigitalOcean" };
    let detail = format!("asked {what} for the cluster {cluster} in {}: {} × {}, ${:.3} an hour", m.region, m.nodes, m.machine, m.cost_hour);
    let mut r = audited(m.view(), detail);
    *r.status_mut() = axum::http::StatusCode::ACCEPTED;
    Ok(r)
}

/// `{"confirm"}`: deletes a cluster made here: the StrataCluster and its
/// volumes, then the cloud's cluster. `confirm` is its name.
async fn cloud_delete(State(app): State<App>, w: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let m = app.svc.clouds.get(&id).ok_or_else(|| ApiError { status: 404, message: format!("no cluster {id}") })?;
    if b["confirm"].as_str() != Some(m.cluster.as_str()) {
        return Err(ApiError::bad(format!("confirm is the cluster's name, {}, typed again: deleting it deletes its data", m.cluster)));
    }
    let who = who(w);
    let m = app.svc.clouds.delete(&id, &who.name).map_err(|e| ApiError { status: 409, message: e })?;
    app.svc.clouds.start(app.svc.aws.clone(), app.svc.digitalocean.clone());
    let detail = format!("deleting the cluster {} on {} in {}", m.cluster, if m.provider == "aws" { "AWS" } else { "DigitalOcean" }, m.region);
    let mut r = audited(m.view(), detail);
    *r.status_mut() = axum::http::StatusCode::ACCEPTED;
    Ok(r)
}

/// Forgets a cluster that is deleted, or that failed before the cloud
/// made anything; 409 while it may still bill.
async fn cloud_forget(State(app): State<App>, axum::extract::Path(id): axum::extract::Path<String>) -> Result<Response, ApiError> {
    let m = app.svc.clouds.forget(&id).map_err(|e| ApiError { status: if e.starts_with("no cluster") { 404 } else { 409 }, message: e })?;
    Ok(audited(json!({"forgot": m.view()}), format!("forgot the cluster {} ({:?})", m.cluster, m.phase)))
}

/// A DigitalOcean failure: 400 without a token or for a region that is no
/// region's name, else 502 (DigitalOcean's answer).
fn do_error(e: String) -> ApiError {
    ApiError { status: if e.starts_with("no DigitalOcean token") || e.contains("is not a region") || e.starts_with("DOKS does not run") { 400 } else { 502 }, message: e }
}

/// The cloud view's DigitalOcean state: the token as far as it may be
/// shown (masked).
async fn do_status(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.digitalocean.status())
}

/// `{"token"}`: checked with DigitalOcean (`GET /v2/account`), then kept on
/// this server. Answers whose it is.
async fn do_save(State(app): State<App>, w: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let token = b["token"].as_str().ok_or_else(|| ApiError::bad("the body is {\"token\": \"dop_v1_...\"}"))?.to_string();
    let masked = crate::aws::mask(token.trim());
    let who = who(w);
    let a = app.svc.digitalocean.set(token, &who.name).await.map_err(|e| ApiError { status: if e.starts_with("write ") { 500 } else { 400 }, message: e })?;
    let mut j = app.svc.digitalocean.status();
    j["account"] = json!(a);
    Ok(audited(j, format!("saved the DigitalOcean token {masked} (team {}, account {})", a.team, a.uuid)))
}

async fn do_forget(State(app): State<App>) -> Result<Response, ApiError> {
    let was = app.svc.digitalocean.forget().map_err(ApiError::internal)?;
    let detail = match &was {
        Some(k) => format!("forgot the DigitalOcean token {k}"),
        None => "forgot the DigitalOcean token (none was saved)".into(),
    };
    Ok(audited(json!({"forgot": was, "status": app.svc.digitalocean.status()}), detail))
}

async fn do_regions(State(app): State<App>) -> Result<AxJson<Json>, ApiError> {
    Ok(AxJson(json!({"regions": app.svc.digitalocean.regions().await.map_err(do_error)?})))
}

/// `?region=` (default nyc3), `&fresh=1` to fetch again: the sizes a DOKS
/// node may be there with their prices, DOKS's versions and the account's
/// droplets.
async fn do_machines(State(app): State<App>, Query(q): Query<std::collections::HashMap<String, String>>) -> Result<AxJson<Json>, ApiError> {
    let region = q.get("region").filter(|r| !r.is_empty()).cloned().unwrap_or_else(|| "nyc3".into());
    let fresh = q.get("fresh").is_some_and(|v| v != "0" && v != "false");
    let cat = app.svc.digitalocean.catalog(&region, fresh).await.map_err(do_error)?;
    let mut j = json!(&*cat);
    let recommended: Vec<&str> = crate::digitalocean::recommended(&cat.machines).iter().map(|m| m.name.as_str()).collect();
    j["recommended"] = json!(recommended);
    Ok(AxJson(j))
}

/// A plan of a cluster on DOKS ([`crate::digitalocean::plan`]). Creates
/// nothing.
async fn do_plan(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<AxJson<Json>, ApiError> {
    let p: crate::digitalocean::plan::Plan = serde_json::from_value(b).map_err(|e| ApiError::bad(format!("a plan: {e}")))?;
    let cat = app.svc.digitalocean.catalog(&p.region, false).await.map_err(do_error)?;
    Ok(AxJson(crate::digitalocean::plan::plan(&p, &cat).map_err(ApiError::bad)?))
}

/// The plan, and DigitalOcean's answers now: the account, the region,
/// the size, the version and the droplet limit, fetched again. Creates
/// nothing.
async fn do_validate(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<AxJson<Json>, ApiError> {
    let p: crate::digitalocean::plan::Plan = serde_json::from_value(b).map_err(|e| ApiError::bad(format!("a plan: {e}")))?;
    let d = &app.svc.digitalocean;
    let cat = d.catalog(&p.region, true).await.map_err(do_error)?;
    let mut j = crate::digitalocean::plan::plan(&p, &cat).map_err(ApiError::bad)?;
    let token = d.token().map(|(t, _)| t).ok_or_else(|| do_error("no DigitalOcean token".into()))?;
    j["validated"] = match d.account(&token).await {
        Ok(a) => {
            let checks = crate::digitalocean::plan::checks(&p, &cat, &a.status);
            json!({"ok": checks.iter().all(|c| c["ok"] == true), "checks": checks})
        }
        Err(e) => json!({"ok": false, "error": e}),
    };
    Ok(AxJson(j))
}

async fn routes(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.metrics.routes())
}

async fn whoami(State(app): State<App>, w: Option<axum::Extension<Principal>>) -> AxJson<Json> {
    let w = who(w);
    let mut j = w.json();
    j["auth"] = json!({"token": app.token.is_some(), "cluster_token": app.cluster.is_some(), "keys": !app.svc.keys.is_empty(), "open": !app.guarded() && app.svc.keys.is_empty()});
    j["cluster"] = app.svc.identity.json();
    j["caller"] = json!(keys::caller());
    AxJson(j)
}

async fn list_keys(State(app): State<App>) -> AxJson<Json> {
    // The keys, then the revoked ones (with revoked_ms, revoked_by).
    let mut keys = app.svc.keys.list();
    let revoked = app.svc.keys.revoked();
    let n = revoked.len();
    keys.extend(revoked);
    AxJson(json!({"keys": keys, "revoked": n, "file": app.svc.keys.file().map(|p| p.display().to_string()), "peers": app.svc.keys.peers()}))
}

/// `POST /v1/admin/keys/:id/rotate {"grace_minutes": 60}`: a new key of
/// the same name, scope and owner (its secret in this answer only); the
/// old one expires after the grace period.
async fn rotate_key(State(app): State<App>, w: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>, b: Option<AxJson<Json>>) -> Result<Response, ApiError> {
    let b = b.map_or(Json::Null, |AxJson(b)| b);
    let grace = match b.get("grace_minutes") {
        None | Some(Json::Null) => 60.0,
        Some(v) => v.as_f64().filter(|m| (0.0..=10_080.0).contains(m)).ok_or_else(|| ApiError::bad("grace_minutes is a number of minutes from 0 to 10080 (a week)"))?,
    };
    let (key, secret, old) = app.svc.keys.rotate(&id, (grace * 60_000.0) as u64, app.guarded())?;
    let by = keys::audit_name(&who(w));
    tracing::info!(key = %key.id, old = %old.id, name = %key.name, by = %by, "API key rotated");
    keys::note_change(keys::Change { object: Some(format!("key {}", key.name)), before: Some(json!({"id": old.id})), after: Some(json!({"id": key.id, "old_expires_ms": old.expires_ms})), ..Default::default() });
    let detail = format!("rotated key {} ({} in place of {}, which expires in {grace} min)", key.name, key.id, old.id);
    Ok(audited(json!({"key": key.json(), "secret": secret, "old": old.json()}), detail))
}

/// `{"name": ..., "scope": "read" | "write" | "admin", "expires_days": n}`:
/// a new key; its secret is in this answer only.
async fn create_key(State(app): State<App>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let name = body_str(&b, "name")?;
    let scope = Scope::parse(b.get("scope").and_then(|v| v.as_str()).unwrap_or("read")).ok_or_else(|| ApiError::bad("scope is read, write or admin"))?;
    let expires = match b.get("expires_days") {
        None | Some(Json::Null) => None,
        Some(v) => Some(telemetry::now_ms() + v.as_f64().filter(|d| *d > 0.0).ok_or_else(|| ApiError::bad("expires_days is a number of days above 0"))?.mul_add(86_400_000.0, 0.0) as u64),
    };
    let open = !app.guarded() && app.svc.keys.is_empty();
    // A shard, or a copy in replication, whose callers send no credential.
    let in_cluster = app.svc.slots().is_some() || app.svc.role() != crate::pb::Role::Standalone;
    let text = |k: &str| b.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(|v| v.chars().take(200).collect::<String>());
    let (key, secret) = app.svc.keys.create_owned(name, scope, expires, text("owner"), text("note"), app.guarded(), in_cluster)?;
    keys::note_change(keys::Change { object: Some(format!("key {}", key.name)), after: Some(json!({"id": key.id, "scope": scope.name(), "owner": key.owner, "expires_ms": key.expires_ms})), ..Default::default() });
    tracing::info!(key = %key.id, name = %key.name, scope = scope.name(), "API key created");
    let detail = format!("created key {} ({}, {})", key.name, key.id, scope.name());
    Ok(audited(json!({"key": key.json(), "secret": secret, "closed": open}), detail))
}

async fn revoke_key(State(app): State<App>, w: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>) -> Result<Response, ApiError> {
    let name = app.svc.keys.list().into_iter().find(|k| k["id"] == id.as_str()).and_then(|k| k["name"].as_str().map(String::from));
    match app.svc.keys.revoke_by(&id, app.guarded(), Some(keys::audit_name(&who(w))))? {
        true => {
            keys::note_change(keys::Change { object: name.map(|n| format!("key {n}")), before: Some(json!({"id": id})), ..Default::default() });
            tracing::info!(key = %id, "API key revoked");
            Ok(audited(json!({"revoked": id}), format!("revoked key {id}")))
        }
        false => Err(ApiError { status: 404, message: format!("no key {id}") }),
    }
}

/// `GET /v1/admin/audit?kind=change&from=&to=&who=&q=&limit=`: audited
/// requests, the newest first; from the log store for what memory no
/// longer holds.
async fn audit_log(State(app): State<App>, Query(q): Params) -> Res {
    let limit = param_u64(&q, "limit", 200)?.clamp(1, keys::AUDIT as u64) as usize;
    let to = param_u64(&q, "to", u64::MAX)?;
    let from = param_u64(&q, "from", 0)?;
    let kinds: Vec<&str> = q.get("kind").map(|k| k.split(',').map(str::trim).filter(|k| !k.is_empty()).collect()).unwrap_or_default();
    if let Some(k) = kinds.iter().find(|k| ![keys::CHANGE, keys::REFUSAL, keys::NOTE_KIND].contains(k)) {
        return Err(ApiError::bad(format!("kind is change, refusal or note, not {k:?}")));
    }
    let by = q.get("who").map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty());
    let text = q.get("q").map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty());
    let fits = |e: &Json| {
        let s = |k: &str| e[k].as_str().unwrap_or("").to_lowercase();
        let ts = e["ts_ms"].as_u64().unwrap_or(0);
        ts >= from
            && ts <= to
            && (kinds.is_empty() || kinds.contains(&e["kind"].as_str().unwrap_or("")))
            && by.as_ref().is_none_or(|w| s("who").contains(w.as_str()) || s("key") == *w)
            && text.as_ref().is_none_or(|t| ["detail", "route", "object", "reason", "who", "client", "addr"].iter().any(|k| s(k).contains(t.as_str())))
    };
    let all = app.svc.audit.latest(keys::AUDIT);
    // From when memory holds every entry: since the server started, or
    // its oldest once it holds as many as it keeps.
    let up_ms = app.svc.started.elapsed().map_or(0, |d| d.as_millis() as u64);
    let held = match all.last() {
        Some(e) if all.len() >= keys::AUDIT => e.ts_ms,
        _ => telemetry::now_ms().saturating_sub(up_ms),
    };
    let mut entries: Vec<Json> = all.iter().map(|e| {
        let mut j = json!(**e);
        j["kind"] = json!(e.kind());
        j
    }).filter(|j| fits(j)).take(limit).collect();
    let mut source = vec!["memory"];
    let mut error = None;
    if let (Some(store), true) = (&app.svc.log_store, entries.len() < limit && from < held) {
        let and = (kinds.len() == 1).then(|| format!("kind = '{}'", kinds[0]));
        let want = ((limit - entries.len()) * 4).min(keys::AUDIT);
        match store.kept_rows(crate::logstore::AUDIT, from, held.saturating_sub(1).min(to), and, want).await {
            Ok(rows) => {
                let more = rows.iter().filter_map(|r| r["entry"].as_str().and_then(|t| serde_json::from_str::<Json>(t).ok())).filter(|j| fits(j));
                entries.extend(more.take(limit - entries.len()));
                source.push("log_store");
            }
            Err(e) => error = Some(format!("the log store did not answer: {}", e.message)),
        }
    }
    let mut j = json!({"entries": entries, "file": app.svc.audit.path().map(|p| p.display().to_string()), "source": source, "memory_from_ms": held});
    if let Some(e) = error {
        j["error"] = json!(e);
    }
    Ok(AxJson(j))
}

// ----- alerts (alerts agent): rules, states, acknowledgements, silences,
// notes, routes and backtests ([`crate::alerts`])

/// The rules with their states, the latest changes and the routes.
async fn alerts(State(app): State<App>) -> AxJson<Json> {
    AxJson(app.svc.alerts.json())
}

/// The name a change is made by: the request's key.
fn by(who: &Principal) -> String {
    keys::audit_name(who)
}

/// One rule's state with its events: those in memory, and those of the
/// last 7 days in the log store's `alerts` table.
async fn alert(State(app): State<App>, axum::extract::Path(id): axum::extract::Path<String>) -> Res {
    let mut j = app.svc.alerts.one(&id)?;
    if let Some(store) = app.svc.log_store.clone() {
        let since = telemetry::now_ms().saturating_sub(crate::alerts::KEEP_DAYS * 86_400_000);
        match store.alert_events(&id, since, 500).await {
            Ok(rows) => {
                let mut events: Vec<Json> = j["events"].as_array().cloned().unwrap_or_default();
                let seen: std::collections::HashSet<(u64, String)> = events.iter().map(|e| (e["ts_ms"].as_u64().unwrap_or(0), e["state"].as_str().unwrap_or("").to_string())).collect();
                events.extend(rows.into_iter().filter(|e| !seen.contains(&(e["ts_ms"].as_u64().unwrap_or(0), e["state"].as_str().unwrap_or("").to_string()))));
                events.sort_by(|a, b| b["ts_ms"].as_u64().cmp(&a["ts_ms"].as_u64()));
                j["events"] = json!(events);
                j["events_source"] = json!("log_store");
            }
            Err(e) => j["events_error"] = json!(e.message),
        }
    }
    Ok(AxJson(j))
}

async fn put_alert_rules(State(app): State<App>, p: Option<axum::Extension<Principal>>, AxJson(b): AxJson<Json>) -> Result<Response, ApiError> {
    let rules: Vec<crate::alerts::Rule> = serde_json::from_value(b.get("rules").cloned().ok_or_else(|| ApiError::bad("the body needs \"rules\""))?).map_err(|e| ApiError::bad(format!("bad rules: {e}")))?;
    let n = rules.len();
    let (events, before, after) = app.svc.alerts.set_rules(rules, &by(&who(p)))?;
    let (before, after) = crate::alerts::changed(&before, &after);
    let what = crate::alerts::change_text(&before, &after);
    keys::note_change(keys::Change { object: Some("alert rules".into()), before: Some(before), after: Some(after), ..Default::default() });
    crate::alerts::deliver(&app.svc, events);
    Ok(audited(app.svc.alerts.json(), format!("{n} alert rules: {what}")))
}

/// Notes for the audit entry what a request changed of alert `id`: its
/// fields `fields` before and after.
fn alert_change(id: &str, before: &Json, after: &Json, fields: &[&str]) {
    let pick = |j: &Json| Json::Object(fields.iter().map(|k| (k.to_string(), j.get(*k).cloned().unwrap_or(Json::Null))).collect());
    keys::note_change(keys::Change { object: Some(format!("alert {id}")), before: Some(pick(before)), after: Some(pick(after)), ..Default::default() });
}

async fn ack_alert(State(app): State<App>, p: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>, b: Option<AxJson<Json>>) -> Res {
    let note = b.and_then(|AxJson(b)| b.get("note").and_then(Json::as_str).map(String::from));
    let before = app.svc.alerts.one(&id)?;
    let e = app.svc.alerts.ack(&id, &by(&who(p)), note)?;
    keys::note(format!("acknowledged alert {id}"));
    crate::alerts::deliver(&app.svc, vec![e]);
    let after = app.svc.alerts.one(&id)?;
    alert_change(&id, &before, &after, &["state", "ack"]);
    Ok(AxJson(after))
}

async fn silence_alert(State(app): State<App>, p: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>, AxJson(b): AxJson<Json>) -> Res {
    let minutes = b.get("minutes").and_then(Json::as_u64).ok_or_else(|| ApiError::bad("the body needs \"minutes\""))?;
    let reason = b.get("reason").and_then(Json::as_str).unwrap_or_default().to_string();
    let before = app.svc.alerts.one(&id)?;
    let e = app.svc.alerts.silence(&id, &by(&who(p)), minutes, &reason)?;
    keys::note(format!("silenced alert {id} for {minutes} min: {reason}"));
    crate::alerts::deliver(&app.svc, vec![e]);
    let after = app.svc.alerts.one(&id)?;
    alert_change(&id, &before, &after, &["silenced"]);
    Ok(AxJson(after))
}

async fn unsilence_alert(State(app): State<App>, p: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>) -> Res {
    let before = app.svc.alerts.one(&id)?;
    let e = app.svc.alerts.unsilence(&id, &by(&who(p)))?;
    keys::note(format!("lifted the silence of alert {id}"));
    crate::alerts::deliver(&app.svc, vec![e]);
    let after = app.svc.alerts.one(&id)?;
    alert_change(&id, &before, &after, &["silenced"]);
    Ok(AxJson(after))
}

async fn note_alert(State(app): State<App>, p: Option<axum::Extension<Principal>>, axum::extract::Path(id): axum::extract::Path<String>, AxJson(b): AxJson<Json>) -> Res {
    let text = b.get("text").and_then(Json::as_str).ok_or_else(|| ApiError::bad("the body needs \"text\""))?;
    let e = app.svc.alerts.note(&id, &by(&who(p)), text)?;
    keys::note(format!("noted on alert {id}: {}", crate::obs::truncate(text, 200)));
    keys::note_change(keys::Change { object: Some(format!("alert {id}")), after: Some(json!({"note": text})), ..Default::default() });
    crate::alerts::deliver(&app.svc, vec![e]);
    app.svc.alerts.one(&id).map(AxJson)
}

// ----- end alerts
