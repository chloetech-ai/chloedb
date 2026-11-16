//! An OpenSearch-compatible HTTP API (`--opensearch-listen`): indexes,
//! documents, `_bulk`, `_search` with the Query DSL and aggregations,
//! `_count`, `_mget`, `_msearch`, scrolls, `_delete_by_query`, aliases
//! and mappings, answered in OpenSearch 2's dialect, so that applications
//! written for OpenSearch (or Elasticsearch 7's open API) search Strata
//! unchanged.
//!
//! Each index is a table ([`mapping`]); each request becomes Strata SQL
//! and write operations that [`Service`] runs, on one server or through a
//! coordinator over shards alike. A request needs the scope its kind
//! does: reads the read scope, documents the write scope, and indexes,
//! mappings and aliases the admin scope. A client sends its token or key
//! as `Authorization: Bearer <key>`, or as the password of Basic
//! authentication (`https://user:<key>@host:9200`), as OpenSearch
//! clients do.

pub mod aggs;
pub mod dsl;
pub mod highlight;
pub mod mapping;
pub mod script;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine;
use serde_json::{json, Map, Value as Json};
use chloedb::query::Params;
use chloedb::value::Value;

use crate::auth::{Access, Secret};
use crate::keys::{Principal, Scope};
use crate::service::{After, QueryReq, Service, SqlAnswer, Written};
use crate::ApiError;
use dsl::{Compiled, Ctx, Score, Target};
use mapping::{ident, lit, Index, Kind, HASH, ID, META, VERSION};

/// The version the root answers by default: OpenSearch 2's API.
pub const VERSION_NUMBER: &str = "2.11.0";

/// How long an index's mapping is kept in memory before it is read again
/// (a change made here is seen at once; one made through another
/// coordinator within this).
const CACHE_FOR: Duration = Duration::from_secs(2);

/// The most hits a search pages to (`index.max_result_window`).
const MAX_WINDOW: usize = 10_000;

/// An error as OpenSearch answers one: its HTTP status, its type and
/// reason.
#[derive(Clone, Debug)]
pub struct OsError {
    pub status: u16,
    pub kind: String,
    pub reason: String,
    pub index: Option<String>,
}

impl OsError {
    pub fn new(status: u16, kind: &str, reason: impl Into<String>) -> OsError {
        OsError { status, kind: kind.into(), reason: reason.into(), index: None }
    }

    pub fn parsing(reason: impl Into<String>) -> OsError {
        OsError::new(400, "parsing_exception", reason)
    }

    pub fn mapper(reason: impl Into<String>) -> OsError {
        OsError::new(400, "mapper_parsing_exception", reason)
    }

    pub fn internal(reason: impl Into<String>) -> OsError {
        OsError::new(500, "exception", reason)
    }

    pub fn kind(mut self, kind: &str) -> OsError {
        self.kind = kind.into();
        self
    }

    pub fn index(mut self, index: &str) -> OsError {
        self.index = Some(index.into());
        self
    }

    fn not_found(index: &str) -> OsError {
        OsError::new(404, "index_not_found_exception", format!("no such index [{index}]")).index(index)
    }

    /// The error's body (`{"type", "reason", ...}`).
    pub fn cause(&self) -> Json {
        let mut c = json!({"type": self.kind, "reason": self.reason});
        if let Some(i) = &self.index {
            c["index"] = json!(i);
            if self.kind == "index_not_found_exception" {
                c["resource.type"] = json!("index_or_alias");
                c["resource.id"] = json!(i);
            }
        }
        c
    }

    pub fn json(&self) -> Json {
        let c = self.cause();
        let mut e = c.clone();
        e["root_cause"] = json!([c]);
        json!({"error": e, "status": self.status})
    }
}

impl From<ApiError> for OsError {
    fn from(e: ApiError) -> OsError {
        let kind = match e.status {
            400 if e.message.contains("query error") || e.message.contains("SQL") => "search_phase_execution_exception",
            400 => "illegal_argument_exception",
            401 | 403 => "security_exception",
            404 => "resource_not_found_exception",
            409 => "version_conflict_engine_exception",
            429 => "rejected_execution_exception",
            503 => "cluster_block_exception",
            _ => "exception",
        };
        OsError::new(e.status, kind, e.message)
    }
}

impl From<chloedb::Error> for OsError {
    fn from(e: chloedb::Error) -> OsError {
        OsError::from(ApiError::from(e))
    }
}

/// An answer: its status and body (none for a HEAD).
struct Reply {
    status: u16,
    body: Json,
}

fn ok(body: Json) -> Result<Reply, OsError> {
    Ok(Reply { status: 200, body })
}

/// A query's rows as JSON.
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Json>>,
    pub scores: Vec<f64>,
}

/// The API's state: the service, the credentials it checks, and the
/// indexes it has read.
pub struct Os {
    pub svc: Arc<Service>,
    token: Option<Secret>,
    cluster: Option<Secret>,
    hosts: Option<Arc<[String]>>,
    version: String,
    cache: std::sync::Mutex<HashMap<String, (Instant, Option<Arc<Index>>)>>,
    /// Changes of mappings from this process, one at a time, so that two
    /// requests that add fields to an index do not each keep their own.
    ddl: tokio::sync::Mutex<()>,
}

/// The API's router, for its own listener. `version` is the version the
/// root reports (`2.11.0` by default; `7.10.2` for clients that ask for
/// Elasticsearch 7's).
pub fn router(svc: Arc<Service>, access: &Access, version: &str) -> Router {
    let limit = axum::extract::DefaultBodyLimit::max(svc.max_body);
    let os = Arc::new(Os { svc, token: access.token.clone(), cluster: access.cluster.clone(), hosts: access.hosts.clone().map(Into::into), version: version.to_string(), cache: Default::default(), ddl: Default::default() });
    Router::new().fallback(dispatch).layer(limit).with_state(os)
}

/// The query string's parameters, decoded.
fn query_params(uri: &Uri) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for kv in uri.query().unwrap_or_default().split('&').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        out.insert(decode(k), decode(v));
    }
    out
}

/// Percent-decoding, `+` as a space in query strings.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                Ok(x) => {
                    out.push(x);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_path(s: &str) -> String {
    decode(&s.replace('+', "%2B"))
}

/// The credential of a request: a Bearer token, or Basic
/// authentication's password, as `Bearer <password>`.
fn credential(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    if v.starts_with("Bearer ") {
        return Some(v.to_string());
    }
    let b = v.strip_prefix("Basic ").or_else(|| v.strip_prefix("basic "))?;
    let raw = base64::engine::general_purpose::STANDARD.decode(b.trim()).ok()?;
    let text = String::from_utf8(raw).ok()?;
    let (_, pass) = text.split_once(':')?;
    Some(format!("Bearer {pass}"))
}

async fn dispatch(State(os): State<Arc<Os>>, peer: Option<ConnectInfo<std::net::SocketAddr>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let t0 = Instant::now();
    let q = query_params(&uri);
    let segs: Vec<String> = uri.path().split('/').filter(|s| !s.is_empty()).map(decode_path).collect();
    let label = route_label(&segs);
    let head = method == Method::HEAD;
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let caller = crate::http::caller_of(&headers, peer.map(|p| p.0), &os.svc.trusted_proxies);
    let auth = credential(&headers);
    let mut who = None;
    let (r, note, change) = if let Some(why) = crate::auth::browser_check(os.hosts.as_deref(), host, origin, method.is_safe()) {
        (Err(OsError::new(403, "security_exception", why)), None, crate::keys::Change::default())
    } else {
        who = os.svc.keys.resolve(os.token.as_ref(), os.cluster.as_ref(), auth.as_deref());
        match who.clone() {
            None => (Err(OsError::new(401, "security_exception", "missing or wrong credentials: send a key or the token as a Bearer token, or as the password of Basic authentication")), None, crate::keys::Change::default()),
            // A key limited to tables calls the main listener's routes,
            // which check its tables ([`crate::scoped`]); this one's do not.
            Some(who) if who.limited() => (Err(OsError::new(403, "security_exception", format!("the key {:?} is limited to tables: it calls the HTTP API, not the OpenSearch one", who.name))), None, crate::keys::Change::default()),
            Some(who) => {
                let tc = Arc::new(crate::telemetry::Caller::of(&who, Some(caller.clone())));
                match unzip(&headers, body, os.svc.max_body) {
                    Err(e) => (Err(e), None, crate::keys::Change::default()),
                    Ok(body) => {
                        let fut = os.route(&method, &segs, &q, &headers, body, who);
                        let fut = crate::telemetry::calling(tc, crate::keys::calling(caller.clone(), crate::keys::noting_change(fut)));
                        crate::service::DEADLINE.scope(Default::default(), fut).await
                    }
                }
            }
        }
    };
    let status = match &r {
        Ok(r) => r.status,
        Err(e) => e.status,
    };
    // As the main listener's: refusals, an admin's changes and what the
    // service noted go to the audit log; reads answered do not.
    let refusal = status == 401 || status == 403;
    if refusal || note.is_some() || admin_route(&method, &segs) {
        let known = match &who {
            None => os.svc.keys.identify(auth.as_deref()),
            Some(_) => None,
        };
        let detail = match (&known, note) {
            (Some((_, name, why)), None) => Some(format!("refused: the key {name} is {why}")),
            (_, n) => n,
        };
        let failed = match &r {
            Err(e) if status >= 400 && !refusal => Some(json!({"error": e.reason})),
            _ => None,
        };
        os.svc.audit_keep(
            crate::keys::Entry {
                agent: None,
                on_behalf_of: None,
                change_request: None,
                ts_ms: crate::telemetry::now_ms(),
                who: who.as_ref().map_or_else(|| known.as_ref().map_or("unknown".into(), |k| k.1.clone()), crate::keys::audit_name),
                key: who.as_ref().and_then(|w| w.key.clone()).or_else(|| known.as_ref().map(|k| k.0.clone())),
                scope: who.as_ref().map(|w| w.scope.name()),
                method: method.to_string(),
                route: format!("opensearch {label}"),
                status,
                ms: t0.elapsed().as_secs_f64() * 1e3,
                request_id: None,
                detail,
                kind: change.kind.unwrap_or(if refusal { crate::keys::REFUSAL } else { crate::keys::CHANGE }).into(),
                addr: caller.addr.clone(),
                client: caller.named(),
                reason: caller.reason.clone(),
                object: change.object.or_else(|| segs.first().filter(|s| !s.starts_with('_')).map(|ix| format!("index {ix}"))),
                before: change.before,
                after: change.after.or(failed),
                actor: change.actor.unwrap_or("person").into(),
            },
            true,
        );
    }
    finish(&os, &label, t0, head, &q, r)
}

/// Whether a request changes indexes, mappings, settings or aliases: what
/// [`Os::route`] asks the admin scope for.
fn admin_route(method: &Method, segs: &[String]) -> bool {
    let s: Vec<&str> = segs.iter().map(String::as_str).collect();
    use Method as M;
    matches!(
        (method, s.as_slice()),
        (&M::POST, ["_aliases"])
            | (&M::PUT | &M::POST | &M::DELETE, [_, "_alias" | "_aliases", _])
            | (&M::PUT | &M::POST, [_, "_mapping", ..])
            | (&M::PUT, [_, "_settings"])
            | (&M::PUT | &M::DELETE, [_])
    )
}

/// The answer, with `filter_path` applied and counted in the metrics.
fn finish(os: &Os, label: &str, t0: Instant, head: bool, q: &HashMap<String, String>, r: Result<Reply, OsError>) -> Response {
    let (status, mut body) = match r {
        Ok(r) => (r.status, r.body),
        Err(e) => (e.status, e.json()),
    };
    os.svc.metrics.http(label, status, t0.elapsed().as_secs_f64());
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if head {
        return code.into_response();
    }
    if let Some(f) = q.get("filter_path") {
        body = filter_path(&body, f);
    }
    let text = if q.contains_key("pretty") { serde_json::to_string_pretty(&body).unwrap_or_default() } else { body.to_string() };
    let ct = if body.is_string() && label == "/_cat" { "text/plain; charset=UTF-8" } else { "application/json; charset=UTF-8" };
    let text = match (&body, label) {
        (Json::String(s), "/_cat") => s.clone(),
        _ => text,
    };
    (code, [(header::CONTENT_TYPE, ct)], text).into_response()
}

/// A request body, gunzipped if it says it is gzip: to `max` bytes at
/// most, as a body sent as it is (a small body of zeros would otherwise
/// inflate past the memory the process has).
fn unzip(headers: &HeaderMap, body: Bytes, max: usize) -> Result<Bytes, OsError> {
    let gz = headers.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
    if !gz {
        return Ok(body);
    }
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&body[..]).take(max as u64 + 1).read_to_end(&mut out).map_err(|e| OsError::new(400, "parse_exception", format!("the body is not gzip: {e}")))?;
    if out.len() > max {
        return Err(OsError::new(413, "content_too_long_exception", format!("the body inflates past {max} bytes, the most a request may send")));
    }
    Ok(Bytes::from(out))
}

/// A route's name for the metrics: its fixed segments, an index as `{index}`.
fn route_label(segs: &[String]) -> String {
    if segs.first().is_some_and(|s| s == "_cat") {
        return "/_cat".into();
    }
    let mut out = String::new();
    for (i, s) in segs.iter().enumerate() {
        out.push('/');
        if s.starts_with('_') {
            out.push_str(s);
        } else if i == 0 {
            out.push_str("{index}");
        } else {
            out.push_str("{id}");
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

fn body_json(body: &Bytes) -> Result<Json, OsError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| OsError::new(400, "parse_exception", format!("the body is not JSON: {e}")))
}

/// `filter_path`: the answer's parts that the comma-separated paths name
/// (`hits.hits._id`, `*` for any key, `**` for any run of keys), arrays
/// passed through; paths starting with `-` leave parts out.
fn filter_path(j: &Json, spec: &str) -> Json {
    let (inc, exc): (Vec<&str>, Vec<&str>) = spec.split(',').map(str::trim).filter(|s| !s.is_empty()).partition(|s| !s.starts_with('-'));
    let inc: Vec<Vec<&str>> = inc.iter().map(|p| p.split('.').collect()).collect();
    let exc: Vec<Vec<&str>> = exc.iter().map(|p| p[1..].split('.').collect()).collect();
    fn keep(j: &Json, pats: &[Vec<&str>]) -> Option<Json> {
        if pats.iter().any(|p| p.is_empty() || (p.len() == 1 && p[0] == "**")) {
            return Some(j.clone());
        }
        match j {
            Json::Object(m) => {
                let mut out = Map::new();
                for (k, v) in m {
                    let mut next = Vec::new();
                    for p in pats {
                        if p[0] == "**" {
                            next.push(p.clone());
                            if p.len() > 1 && dsl::glob(p[1], k) {
                                next.push(p[2..].to_vec());
                            }
                        } else if dsl::glob(p[0], k) {
                            next.push(p[1..].to_vec());
                        }
                    }
                    if next.is_empty() {
                        continue;
                    }
                    if let Some(x) = keep(v, &next) {
                        out.insert(k.clone(), x);
                    }
                }
                (!out.is_empty()).then_some(Json::Object(out))
            }
            Json::Array(xs) => {
                let kept: Vec<Json> = xs.iter().filter_map(|x| keep(x, pats)).collect();
                (!kept.is_empty()).then_some(Json::Array(kept))
            }
            _ => None,
        }
    }
    fn drop(j: &mut Json, pats: &[Vec<&str>]) {
        match j {
            Json::Object(m) => {
                let keys: Vec<String> = m.keys().cloned().collect();
                for k in keys {
                    let mut next = Vec::new();
                    let mut gone = false;
                    for p in pats {
                        if dsl::glob(p[0], &k) {
                            if p.len() == 1 {
                                gone = true;
                            } else {
                                next.push(p[1..].to_vec());
                            }
                        }
                    }
                    if gone {
                        m.remove(&k);
                    } else if !next.is_empty() {
                        if let Some(v) = m.get_mut(&k) {
                            drop(v, &next);
                        }
                    }
                }
            }
            Json::Array(xs) => xs.iter_mut().for_each(|x| drop(x, pats)),
            _ => {}
        }
    }
    let mut out = if inc.is_empty() { j.clone() } else { keep(j, &inc).unwrap_or_else(|| json!({})) };
    if !exc.is_empty() {
        drop(&mut out, &exc);
    }
    out
}

/// What `_source` a search or a read returns: none, all, or the paths
/// its patterns include and do not exclude.
#[derive(Clone, Debug)]
struct SourceFilter {
    on: bool,
    includes: Vec<String>,
    excludes: Vec<String>,
}

impl SourceFilter {
    fn all() -> SourceFilter {
        SourceFilter { on: true, includes: Vec::new(), excludes: Vec::new() }
    }

    fn of(body: Option<&Json>, q: &HashMap<String, String>) -> SourceFilter {
        let mut f = SourceFilter::all();
        let list = |v: &Json| -> Vec<String> {
            match v {
                Json::String(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                Json::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                _ => Vec::new(),
            }
        };
        match body {
            Some(Json::Bool(b)) => f.on = *b,
            Some(v @ (Json::String(_) | Json::Array(_))) => f.includes = list(v),
            Some(Json::Object(o)) => {
                f.includes = o.get("includes").or_else(|| o.get("include")).map(list).unwrap_or_default();
                f.excludes = o.get("excludes").or_else(|| o.get("exclude")).map(list).unwrap_or_default();
            }
            _ => {}
        }
        if let Some(s) = q.get("_source") {
            match s.as_str() {
                "false" => f.on = false,
                "true" => f.on = true,
                s => f.includes = s.split(',').map(String::from).collect(),
            }
        }
        if let Some(s) = q.get("_source_includes").or_else(|| q.get("_source_include")) {
            f.includes = s.split(',').map(String::from).collect();
        }
        if let Some(s) = q.get("_source_excludes").or_else(|| q.get("_source_exclude")) {
            f.excludes = s.split(',').map(String::from).collect();
        }
        f
    }

    fn apply(&self, doc: &Json) -> Json {
        if self.includes.is_empty() && self.excludes.is_empty() {
            return doc.clone();
        }
        fn walk(m: &Map<String, Json>, prefix: &str, f: &SourceFilter) -> Map<String, Json> {
            let mut out = Map::new();
            for (k, v) in m {
                let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                if f.excludes.iter().any(|e| dsl::glob(e, &path)) {
                    continue;
                }
                let inc = f.includes.is_empty() || f.includes.iter().any(|i| dsl::glob(i, &path) || dsl::glob(&format!("{i}.*"), &path) || path.starts_with(&format!("{i}.")));
                let below = f.includes.iter().any(|i| i.starts_with(&format!("{path}.")) || i.starts_with('*'));
                match v {
                    Json::Object(sub) if !inc && below => {
                        let s = walk(sub, &path, f);
                        if !s.is_empty() {
                            out.insert(k.clone(), Json::Object(s));
                        }
                    }
                    Json::Object(sub) if inc => {
                        let f2 = SourceFilter { on: true, includes: Vec::new(), excludes: f.excludes.clone() };
                        out.insert(k.clone(), Json::Object(walk(sub, &path, &f2)));
                    }
                    v if inc => {
                        out.insert(k.clone(), v.clone());
                    }
                    _ => {}
                }
            }
            out
        }
        match doc {
            Json::Object(m) => Json::Object(walk(m, "", self)),
            d => d.clone(),
        }
    }
}

/// The shards a reply says answered: always one, complete.
fn shards() -> Json {
    json!({"total": 1, "successful": 1, "skipped": 0, "failed": 0})
}

/// A random document id, 20 characters as OpenSearch makes them.
fn new_id() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 15]>())
}

/// Merges `patch` into `doc`: objects key by key, anything else replaced.
fn merge(doc: &mut Json, patch: &Json) {
    match (doc, patch) {
        (Json::Object(a), Json::Object(b)) => {
            for (k, v) in b {
                match a.get_mut(k) {
                    Some(x) if x.is_object() && v.is_object() => merge(x, v),
                    _ => {
                        a.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (d, p) => *d = p.clone(),
    }
}

// ------------------------------------------------------------------ bulk items

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Index,
    Create,
    Update,
    Delete,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Index => "index",
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

#[derive(Debug)]
struct Item {
    action: Action,
    index: Option<String>,
    id: Option<String>,
    body: Option<Json>,
    if_seq_no: Option<i64>,
    version: Option<i64>,
    external: bool,
}

/// What became of an item.
#[derive(Debug)]
struct Outcome {
    action: Action,
    index: String,
    id: String,
    status: u16,
    result: &'static str,
    version: i64,
    error: Option<OsError>,
}

impl Outcome {
    fn failed(action: Action, index: &str, id: &str, e: OsError) -> Outcome {
        Outcome { action, index: index.into(), id: id.into(), status: e.status, result: "", version: 0, error: Some(e) }
    }

    fn json(&self) -> Json {
        let mut j = json!({"_index": self.index, "_id": self.id, "status": self.status});
        match &self.error {
            Some(e) => {
                let mut c = e.cause();
                if c.get("index").is_none() {
                    c["index"] = json!(self.index);
                }
                j["error"] = c;
            }
            None => {
                j["_version"] = json!(self.version);
                j["result"] = json!(self.result);
                j["_shards"] = json!({"total": 1, "successful": 1, "failed": 0});
                j["_seq_no"] = json!(self.version - 1);
                j["_primary_term"] = json!(1);
            }
        }
        j
    }
}

/// A `_bulk` body's items.
fn parse_bulk(body: &Bytes) -> Result<Vec<Item>, OsError> {
    let mut items = Vec::new();
    let text = std::str::from_utf8(body).map_err(|_| OsError::new(400, "illegal_argument_exception", "the bulk body is not UTF-8"))?;
    let mut lines = text.split('\n').map(str::trim).filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        let meta: Json = serde_json::from_str(line).map_err(|e| OsError::new(400, "illegal_argument_exception", format!("Malformed action/metadata line: {e}")))?;
        let m = meta.as_object().filter(|m| m.len() == 1).ok_or_else(|| OsError::new(400, "illegal_argument_exception", format!("Malformed action/metadata line [{line}], expected one action")))?;
        let (a, spec) = m.iter().next().expect("one");
        let action = match a.as_str() {
            "index" => Action::Index,
            "create" => Action::Create,
            "update" => Action::Update,
            "delete" => Action::Delete,
            other => return Err(OsError::new(400, "illegal_argument_exception", format!("Malformed action/metadata line, expected one of [create, delete, index, update] but found [{other}]"))),
        };
        let s = |k: &str| spec.get(k).map(dsl::id_text);
        let body = if action == Action::Delete {
            None
        } else {
            let l = lines.next().ok_or_else(|| OsError::new(400, "illegal_argument_exception", "The bulk request must be terminated by a newline [\\n]"))?;
            Some(serde_json::from_str::<Json>(l).map_err(|e| OsError::new(400, "mapper_parsing_exception", format!("failed to parse: {e}")))?)
        };
        items.push(Item {
            action,
            index: s("_index"),
            id: s("_id"),
            body,
            if_seq_no: spec.get("if_seq_no").and_then(Json::as_i64),
            version: spec.get("version").and_then(Json::as_i64),
            external: spec.get("version_type").and_then(Json::as_str).is_some_and(|t| t.starts_with("external")),
        });
    }
    Ok(items)
}

// ------------------------------------------------------------------ the API

impl Os {
    async fn route(&self, method: &Method, segs: &[String], q: &HashMap<String, String>, headers: &HeaderMap, body: Bytes, who: Principal) -> Result<Reply, OsError> {
        let _ = headers;
        let s: Vec<&str> = segs.iter().map(String::as_str).collect();
        let need = |scope: Scope, what: &str| who.may(scope, what).map_err(OsError::from);
        use Method as M;
        // Whether the server is up is anyone's to ask; every other route
        // reads documents or what indexes hold, and needs a read key at
        // least (a feed key reads the change feed alone).
        let up = matches!((method, s.as_slice()), (&M::GET | &M::HEAD, []) | (&M::GET, ["_cluster", "health", ..]));
        if !up {
            need(Scope::Read, "the OpenSearch API")?;
        }
        let m = method.clone();
        match (m, s.as_slice()) {
            (M::GET | M::HEAD, []) => ok(self.info().await),
            (M::GET, ["_cluster", "health", ..]) => ok(self.health().await),
            (M::GET, ["_cluster", "settings"]) => ok(json!({"persistent": {}, "transient": {}})),
            (M::GET, ["_cluster", "state", ..]) => ok(json!({"cluster_name": self.cluster_name(), "cluster_uuid": "chloedb"})),
            (M::GET, ["_nodes", ..]) => ok(self.nodes()),
            (M::GET, ["_cat", what, rest @ ..]) => self.cat(what, rest.first().copied(), q).await,
            (M::POST | M::PUT, ["_bulk"]) => {
                need(Scope::Write, "_bulk")?;
                self.bulk(&who, None, &body, q).await
            }
            (M::POST | M::PUT, [ix, "_bulk"]) => {
                need(Scope::Write, "_bulk")?;
                self.bulk(&who, Some(ix), &body, q).await
            }
            (M::GET | M::POST, ["_search"]) => self.search_req("*", &body, q).await,
            (M::GET | M::POST, ["_search", "scroll", rest @ ..]) => self.scroll(rest.first().copied(), &body, q).await,
            (M::DELETE, ["_search", "scroll", ..]) => ok(json!({"succeeded": true, "num_freed": 0})),
            (M::POST | M::DELETE, ["_pit" | "_search", "point_in_time", ..]) => Err(OsError::new(400, "illegal_argument_exception", "ChloeDB answers point-in-time searches with search_after on a sort, or a scroll")),
            (M::GET | M::POST, [ix, "_search"]) => self.search_req(ix, &body, q).await,
            (M::GET | M::POST, ["_msearch"]) => self.msearch(None, &body).await,
            (M::GET | M::POST, [ix, "_msearch"]) => self.msearch(Some(ix), &body).await,
            (M::GET | M::POST, ["_count"]) => self.count("*", &body, q).await,
            (M::GET | M::POST, [ix, "_count"]) => self.count(ix, &body, q).await,
            (M::GET | M::POST, ["_mget"]) => self.mget(None, &body, q).await,
            (M::GET | M::POST, [ix, "_mget"]) => self.mget(Some(ix), &body, q).await,
            (M::POST | M::GET, ["_refresh" | "_flush" | "_forcemerge"]) | (M::POST | M::GET, [_, "_refresh" | "_flush" | "_forcemerge"]) => ok(json!({"_shards": shards()})),
            (M::GET, ["_aliases" | "_alias"]) => self.get_aliases(None, None).await,
            (M::GET | M::HEAD, ["_alias", name]) => self.get_aliases(None, Some(name)).await,
            (M::POST, ["_aliases"]) => {
                need(Scope::Admin, "changing aliases")?;
                self.post_aliases(&body).await
            }
            (M::GET, [ix, "_alias" | "_aliases"]) => self.get_aliases(Some(ix), None).await,
            (M::GET | M::HEAD, [ix, "_alias" | "_aliases", name]) => self.get_aliases(Some(ix), Some(name)).await,
            (M::PUT | M::POST, [ix, "_alias" | "_aliases", name]) => {
                need(Scope::Admin, "changing aliases")?;
                self.post_aliases(&serde_json::to_vec(&json!({"actions": [{"add": {"index": ix, "alias": name}}]})).unwrap_or_default().into()).await
            }
            (M::DELETE, [ix, "_alias" | "_aliases", name]) => {
                need(Scope::Admin, "changing aliases")?;
                self.post_aliases(&serde_json::to_vec(&json!({"actions": [{"remove": {"index": ix, "alias": name}}]})).unwrap_or_default().into()).await
            }
            (M::GET, ["_mapping"]) => self.get_mapping("*").await,
            (M::GET, [ix, "_mapping", ..]) => self.get_mapping(ix).await,
            (M::PUT | M::POST, [ix, "_mapping", ..]) => {
                need(Scope::Admin, "changing a mapping")?;
                self.put_mapping(ix, &body).await
            }
            (M::GET, ["_settings"]) => self.get_settings("*").await,
            (M::GET, [ix, "_settings", ..]) => self.get_settings(ix).await,
            (M::PUT, [ix, "_settings"]) => {
                need(Scope::Admin, "changing settings")?;
                self.put_settings(ix, &body).await
            }
            (M::GET, [ix, "_stats", ..]) => self.stats(ix).await,
            (M::POST, [ix, "_delete_by_query"]) => {
                need(Scope::Write, "_delete_by_query")?;
                self.delete_by_query(&who, ix, &body, q).await
            }
            (M::POST, [ix, "_update_by_query"]) => {
                need(Scope::Write, "_update_by_query")?;
                self.update_by_query(ix, &body, q).await
            }
            (M::PUT | M::POST, [ix, "_doc" | "_create", id]) => {
                need(Scope::Write, "writing a document")?;
                let create = s[1] == "_create" || q.get("op_type").is_some_and(|o| o == "create");
                self.index_doc(&who, ix, Some(id), create, &body, q).await
            }
            (M::POST, [ix, "_doc"]) => {
                need(Scope::Write, "writing a document")?;
                self.index_doc(&who, ix, None, false, &body, q).await
            }
            (M::POST, [ix, "_update", id]) => {
                need(Scope::Write, "updating a document")?;
                self.update_doc(&who, ix, id, &body, q).await
            }
            (M::GET | M::HEAD, [ix, "_doc", id]) => self.get_doc(ix, id, q, false).await,
            (M::GET | M::HEAD, [ix, "_source", id]) => self.get_doc(ix, id, q, true).await,
            (M::DELETE, [ix, "_doc", id]) => {
                need(Scope::Write, "deleting a document")?;
                self.delete_doc(&who, ix, id, q).await
            }
            (M::PUT, [ix]) => {
                need(Scope::Admin, "creating an index")?;
                self.create_index(&who, ix, &body).await
            }
            (M::DELETE, [ix]) => {
                need(Scope::Admin, "deleting an index")?;
                self.delete_index(&who, ix).await
            }
            (M::GET, [ix]) => self.get_index(ix).await,
            (M::HEAD, [ix]) => {
                let found = !self.resolve(ix).await?.is_empty();
                Ok(Reply { status: if found { 200 } else { 404 }, body: Json::Null })
            }
            // Elasticsearch 6's typed paths, with the one type 7 kept.
            (m, [ix, "_doc", id, rest @ ..]) if !rest.is_empty() => Err(OsError::new(405, "illegal_argument_exception", format!("no handler for {m} /{ix}/_doc/{id}/{}", rest.join("/")))),
            (m, _) => Err(OsError::new(405, "illegal_argument_exception", format!("Incorrect HTTP method for uri [/{}] and method [{m}], or ChloeDB's OpenSearch API does not answer it", segs.join("/")))),
        }
    }

    fn cluster_name(&self) -> String {
        chloedb::env::var("CHLOEDB_CLUSTER_NAME").ok().filter(|n| !n.is_empty()).unwrap_or_else(|| "chloedb".into())
    }

    async fn info(&self) -> Json {
        let elastic7 = self.version.starts_with('7');
        let mut j = json!({
            "name": crate::logstore::hostname(),
            "cluster_name": self.cluster_name(),
            "cluster_uuid": "chloedb",
            "version": {
                "number": self.version,
                "build_type": "chloedb",
                "build_flavor": "default",
                "build_hash": env!("CARGO_PKG_VERSION"),
                "build_date": "2026-11-08T00:00:00Z",
                "build_snapshot": false,
                "lucene_version": "9.7.0",
                "minimum_wire_compatibility_version": "7.10.0",
                "minimum_index_compatibility_version": "7.0.0",
                "chloedb_version": env!("CARGO_PKG_VERSION"),
            },
            "tagline": if elastic7 { "You Know, for Search" } else { "The OpenSearch Project: https://opensearch.org/" },
        });
        if !elastic7 {
            j["version"]["distribution"] = json!("opensearch");
        }
        j
    }

    async fn health(&self) -> Json {
        let n = self.all().await.map(|v| v.len()).unwrap_or(0);
        let status = if self.svc.failing().is_some() { "red" } else { "green" };
        json!({
            "cluster_name": self.cluster_name(), "status": status, "timed_out": false,
            "number_of_nodes": 1, "number_of_data_nodes": 1, "discovered_master": true, "discovered_cluster_manager": true,
            "active_primary_shards": n, "active_shards": n, "relocating_shards": 0, "initializing_shards": 0,
            "unassigned_shards": 0, "delayed_unassigned_shards": 0, "number_of_pending_tasks": 0,
            "number_of_in_flight_fetch": 0, "task_max_waiting_in_queue_millis": 0, "active_shards_percent_as_number": 100.0,
        })
    }

    fn nodes(&self) -> Json {
        let name = crate::logstore::hostname();
        json!({"_nodes": {"total": 1, "successful": 1, "failed": 0}, "cluster_name": self.cluster_name(), "nodes": {"chloedb": {"name": name, "version": self.version, "roles": ["data", "ingest", "cluster_manager"]}}})
    }

    // -------------------------------------------------------------- running SQL

    /// Runs a SELECT.
    pub async fn rows(&self, sql: &str, params: &[(String, Value)]) -> Result<Rows, OsError> {
        let mut p = Params::new();
        for (k, v) in params {
            p = p.set(k, v.clone());
        }
        let a = self.svc.query(QueryReq { sql: sql.to_string(), params: p, reads: true, ..Default::default() }).await.map_err(|e| {
            let mut e = OsError::from(e);
            if e.status == 400 {
                e.reason = format!("{} (in {sql})", e.reason);
            }
            e
        })?;
        Ok(Rows {
            columns: a.rs.columns.clone(),
            rows: a.rs.rows.iter().map(|r| r.iter().map(crate::json::to_json).collect()).collect(),
            scores: a.rs.scores.clone(),
        })
    }

    /// Runs a statement as `who` (whose scope it must be in).
    async fn statement(&self, who: &Principal, sql: &str) -> Result<SqlAnswer, OsError> {
        Ok(self.svc.sql(who, sql, &Params::new(), After::Any, 0).await?)
    }

    async fn write(&self, ops: Vec<Json>) -> Result<Written, OsError> {
        Ok(self.svc.write_request(ops, None).await?)
    }

    // -------------------------------------------------------------- indexes

    async fn meta_exists(&self) -> bool {
        self.svc.table(META).await.is_ok()
    }

    /// Every index, by name.
    async fn all(&self) -> Result<Vec<Arc<Index>>, OsError> {
        if !self.meta_exists().await {
            return Ok(Vec::new());
        }
        let r = self.rows(&format!("SELECT name, aliases, meta FROM {META} LIMIT 10000"), &[]).await?;
        let mut out = Vec::new();
        for row in r.rows {
            out.push(Arc::new(index_of_row(&row)?));
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The index named `name` (not an alias), read again after
    /// [`CACHE_FOR`].
    async fn index(&self, name: &str) -> Result<Option<Arc<Index>>, OsError> {
        if let Some((at, ix)) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(name) {
            if at.elapsed() < CACHE_FOR {
                return Ok(ix.clone());
            }
        }
        if !self.meta_exists().await {
            return Ok(None);
        }
        let got = self.svc.get_rows(Some(META), vec![Value::Str(name.to_string())], Some(vec!["name".into(), "aliases".into(), "meta".into()]), &After::Any, 0, false).await?;
        let ix = match got.rows.into_iter().next().flatten() {
            Some((row, _)) => Some(Arc::new(index_of_row(&row.iter().map(crate::json::to_json).collect::<Vec<_>>())?)),
            None => None,
        };
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), (Instant::now(), ix.clone()));
        Ok(ix)
    }

    fn forget(&self, name: &str) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
    }

    /// The indexes an expression names: names, aliases and patterns
    /// (`logs-*`, `_all`, `*`), separated by commas; `-x` leaves one out.
    async fn resolve(&self, expr: &str) -> Result<Vec<Arc<Index>>, OsError> {
        let parts: Vec<&str> = expr.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
        let simple = parts.len() == 1 && !parts[0].contains('*') && parts[0] != "_all" && !parts[0].starts_with('-');
        if simple {
            if let Some(ix) = self.index(parts[0]).await? {
                return Ok(vec![ix]);
            }
            // An alias.
            if !self.meta_exists().await {
                return Ok(Vec::new());
            }
            let r = self.rows(&format!("SELECT name, aliases, meta FROM {META} WHERE aliases = {} LIMIT 10000", lit(parts[0])), &[]).await?;
            return r.rows.iter().map(|row| index_of_row(row).map(Arc::new)).collect();
        }
        let all = self.all().await?;
        let mut out: Vec<Arc<Index>> = Vec::new();
        for p in &parts {
            if let Some(not) = p.strip_prefix('-') {
                out.retain(|ix| !dsl::glob(not, &ix.name));
                continue;
            }
            let pat = if *p == "_all" { "*" } else { p };
            for ix in &all {
                let hit = dsl::glob(pat, &ix.name) || ix.aliases.iter().any(|a| dsl::glob(pat, a));
                // Patterns leave out hidden indexes (`.name`) unless they
                // name them.
                let hidden = ix.name.starts_with('.') && !pat.starts_with('.');
                if hit && !hidden && !out.iter().any(|o| o.name == ix.name) {
                    out.push(ix.clone());
                }
            }
        }
        Ok(out)
    }

    /// The one index an expression names, for a search or a write.
    async fn one(&self, expr: &str) -> Result<Arc<Index>, OsError> {
        let mut ixs = self.resolve(expr).await?;
        match ixs.len() {
            0 => Err(OsError::not_found(expr)),
            1 => Ok(ixs.pop().expect("one")),
            n => Err(OsError::new(400, "illegal_argument_exception", format!("[{expr}] names {n} indexes; ChloeDB's OpenSearch API searches and writes one index (or an alias of one) at a time"))),
        }
    }

    async fn ensure_meta(&self) -> Result<(), OsError> {
        if !self.meta_exists().await {
            self.statement(&Principal::open(), &mapping::create_meta_sql()).await?;
        }
        Ok(())
    }

    /// `ix` with the number fields no document holds an array of
    /// ([`Index::single_numbers`]), for its aggregations; as it is when
    /// the table cannot say.
    async fn with_single_numbers(&self, ix: &Index) -> Index {
        let mut out = ix.clone();
        let numbers: std::collections::BTreeSet<String> = ix.fields.iter().filter(|(n, f)| matches!(f.kind, Kind::Long | Kind::Double) && f.column(n).is_some()).map(|(n, _)| n.clone()).collect();
        if numbers.is_empty() {
            return out;
        }
        let m = ident(mapping::MULTI);
        if let Ok(r) = self.rows(&format!("SELECT {m}, COUNT(*) FROM {} WHERE {m} IS NOT NULL GROUP BY 1", ix.table), &[]).await {
            let many: std::collections::BTreeSet<String> = r.rows.iter().filter_map(|row| row.first().and_then(Json::as_str).map(String::from)).collect();
            out.single_numbers = Some(Arc::new(numbers.difference(&many).cloned().collect()));
        }
        out
    }

    async fn save(&self, ix: &Index) -> Result<(), OsError> {
        self.write(vec![json!({"op": "upsert", "table": META, "doc": ix.meta_row()})]).await?;
        self.forget(&ix.name);
        Ok(())
    }

    async fn create_index(&self, who: &Principal, name: &str, body: &Bytes) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let ix = self.make_index(who, name, &b).await?;
        ok(json!({"acknowledged": true, "shards_acknowledged": true, "index": ix.name}))
    }

    /// Makes an index from a create request's body (`settings`,
    /// `mappings`, `aliases`).
    async fn make_index(&self, who: &Principal, name: &str, b: &Json) -> Result<Arc<Index>, OsError> {
        mapping::check_name(name)?;
        let _g = self.ddl.lock().await;
        if self.index(name).await?.is_some() || !self.resolve(name).await?.is_empty() {
            return Err(OsError::new(400, "resource_already_exists_exception", format!("index [{name}/strata] already exists")).index(name));
        }
        let mut mappings = b.get("mappings").cloned().unwrap_or_else(|| json!({}));
        // A typed mapping (`{"_doc": {"properties": ...}}`) of 6.x clients.
        if let Some(m) = mappings.as_object() {
            if m.len() == 1 && !m.contains_key("properties") && !m.contains_key("dynamic") {
                if let Some((_, inner)) = m.iter().next().filter(|(_, v)| v.get("properties").is_some()) {
                    mappings = inner.clone();
                }
            }
        }
        let settings = b.get("settings").cloned().unwrap_or_else(|| json!({}));
        let aliases: Vec<String> = b.get("aliases").and_then(Json::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let uuid = new_id()[..22.min(20)].to_string();
        let ix = Index::new(name, &mapping::table_of(name), &uuid, crate::telemetry::now_ms() as i64, mappings, settings, aliases)?;
        who.may(Scope::Admin, "creating an index").map_err(OsError::from)?;
        self.ensure_meta().await?;
        self.statement(who, &mapping::create_sql(&ix)).await.map_err(|e| e.index(name))?;
        self.save(&ix).await?;
        Ok(Arc::new(ix))
    }

    async fn delete_index(&self, who: &Principal, expr: &str) -> Result<Reply, OsError> {
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() {
            return Err(OsError::not_found(expr));
        }
        let _g = self.ddl.lock().await;
        for ix in ixs {
            self.statement(who, &format!("DROP TABLE IF EXISTS {}", ix.table)).await?;
            self.write(vec![json!({"op": "delete", "table": META, "key": ix.name})]).await?;
            self.forget(&ix.name);
        }
        ok(json!({"acknowledged": true}))
    }

    async fn get_index(&self, expr: &str) -> Result<Reply, OsError> {
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() {
            return Err(OsError::not_found(expr));
        }
        ok(Json::Object(ixs.iter().map(|ix| (ix.name.clone(), ix.json())).collect()))
    }

    async fn get_mapping(&self, expr: &str) -> Result<Reply, OsError> {
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() && expr != "*" && expr != "_all" {
            return Err(OsError::not_found(expr));
        }
        ok(Json::Object(ixs.iter().map(|ix| (ix.name.clone(), json!({"mappings": ix.mappings}))).collect()))
    }

    async fn get_settings(&self, expr: &str) -> Result<Reply, OsError> {
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() && expr != "*" && expr != "_all" {
            return Err(OsError::not_found(expr));
        }
        ok(Json::Object(ixs.iter().map(|ix| (ix.name.clone(), json!({"settings": ix.json()["settings"]}))).collect()))
    }

    async fn put_settings(&self, expr: &str, body: &Bytes) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() {
            return Err(OsError::not_found(expr));
        }
        let _g = self.ddl.lock().await;
        for ix in ixs {
            let mut s = ix.settings.clone();
            if !s.is_object() {
                s = json!({});
            }
            let patch = b.get("settings").cloned().unwrap_or(b.clone());
            if patch.pointer("/index/analysis").is_some() || patch.get("analysis").is_some() {
                return Err(OsError::new(400, "illegal_argument_exception", "Can't update non dynamic settings [[index.analysis]] for open indices"));
            }
            merge(&mut s, &patch);
            let next = Index::new(&ix.name, &ix.table, &ix.uuid, ix.created_ms, ix.mappings.clone(), s, ix.aliases.clone())?;
            self.save(&next).await?;
        }
        ok(json!({"acknowledged": true}))
    }

    async fn put_mapping(&self, expr: &str, body: &Bytes) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() {
            return Err(OsError::not_found(expr));
        }
        for ix in ixs {
            let mut m = ix.mappings.clone();
            if !m.is_object() {
                m = json!({});
            }
            let props = b.get("properties").cloned().unwrap_or_else(|| json!({}));
            // A field's type stays as it was.
            let added = Index::new(&ix.name, &ix.table, &ix.uuid, ix.created_ms, json!({"properties": props}), ix.settings.clone(), Vec::new())?;
            for (name, f) in &added.fields {
                if let Some(old) = ix.fields.get(name) {
                    if old.kind != f.kind {
                        return Err(OsError::new(400, "illegal_argument_exception", format!("mapper [{name}] cannot be changed from type [{}] to [{}]", old.ty, f.ty)));
                    }
                }
            }
            merge(&mut m, &json!({"properties": props}));
            if let Some(d) = b.get("dynamic") {
                m["dynamic"] = d.clone();
            }
            self.remap(&ix, m).await?;
        }
        ok(json!({"acknowledged": true}))
    }

    /// Gives an index a mapping with more fields: the columns they need,
    /// then the mapping kept. A change of the cluster's own, which a
    /// write that adds fields (dynamic mapping) makes too.
    async fn remap(&self, ix: &Index, mappings: Json) -> Result<Arc<Index>, OsError> {
        let _g = self.ddl.lock().await;
        // The mapping as kept now, with the change on it.
        self.forget(&ix.name);
        let now = self.index(&ix.name).await?.ok_or_else(|| OsError::not_found(&ix.name))?;
        let mut m = now.mappings.clone();
        if !m.is_object() {
            m = json!({});
        }
        merge(&mut m, &mappings);
        let next = Index::new(&now.name, &now.table, &now.uuid, now.created_ms, m, now.settings.clone(), now.aliases.clone())?;
        let had: Vec<String> = now.columns().into_iter().map(|(c, _)| c).collect();
        let adds: Vec<String> = next.columns().into_iter().filter(|(c, _)| !had.contains(c)).map(|(c, k)| format!("ADD COLUMN IF NOT EXISTS {}", mapping::column_sql(&c, &k))).collect();
        if !adds.is_empty() {
            self.statement(&Principal::open(), &format!("ALTER TABLE {} {}", next.table, adds.join(", "))).await?;
        }
        self.save(&next).await?;
        Ok(Arc::new(next))
    }

    async fn post_aliases(&self, body: &Bytes) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let actions = b.get("actions").and_then(Json::as_array).ok_or_else(|| OsError::new(400, "action_request_validation_exception", "Validation Failed: 1: no actions specified;"))?;
        // index name -> aliases, as the actions leave them.
        let mut changed: BTreeMap<String, Arc<Index>> = BTreeMap::new();
        let mut drops = Vec::new();
        for a in actions {
            let (kind, spec) = a.as_object().and_then(|m| m.iter().next()).ok_or_else(|| OsError::parsing("an alias action is {\"add\": {...}}"))?;
            let list = |k: &str, ks: &str| -> Vec<String> {
                match spec.get(ks).or_else(|| spec.get(k)) {
                    Some(Json::String(s)) => vec![s.clone()],
                    Some(Json::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                    _ => Vec::new(),
                }
            };
            let (indexes, aliases) = (list("index", "indices"), list("alias", "aliases"));
            let mut targets = Vec::new();
            for i in &indexes {
                for ix in self.resolve(i).await? {
                    targets.push(ix);
                }
            }
            if targets.is_empty() {
                return Err(OsError::not_found(&indexes.join(",")));
            }
            for t in targets {
                let cur = changed.get(&t.name).cloned().unwrap_or(t);
                let mut al = cur.aliases.clone();
                match kind.as_str() {
                    "add" => {
                        for a in &aliases {
                            if !al.contains(a) {
                                al.push(a.clone());
                            }
                        }
                    }
                    "remove" => al.retain(|a| !aliases.iter().any(|p| dsl::glob(p, a))),
                    "remove_index" => {
                        drops.push(cur.clone());
                        continue;
                    }
                    other => return Err(OsError::parsing(format!("unknown alias action [{other}]"))),
                }
                let next = Index::new(&cur.name, &cur.table, &cur.uuid, cur.created_ms, cur.mappings.clone(), cur.settings.clone(), al)?;
                changed.insert(cur.name.clone(), Arc::new(next));
            }
        }
        let _g = self.ddl.lock().await;
        for ix in changed.values() {
            if !drops.iter().any(|d| d.name == ix.name) {
                self.save(ix).await?;
            }
        }
        for d in drops {
            self.statement(&Principal::open(), &format!("DROP TABLE IF EXISTS {}", d.table)).await?;
            self.write(vec![json!({"op": "delete", "table": META, "key": d.name})]).await?;
            self.forget(&d.name);
        }
        ok(json!({"acknowledged": true}))
    }

    async fn get_aliases(&self, ix: Option<&str>, name: Option<&str>) -> Result<Reply, OsError> {
        let ixs = match ix {
            Some(e) => self.resolve(e).await?,
            None => self.all().await?,
        };
        let mut out = Map::new();
        for i in &ixs {
            let al: Map<String, Json> = i.aliases.iter().filter(|a| name.is_none_or(|n| n.split(',').any(|p| dsl::glob(p, a)))).map(|a| (a.clone(), json!({}))).collect();
            if name.is_none() || !al.is_empty() {
                out.insert(i.name.clone(), json!({"aliases": al}));
            }
        }
        if name.is_some() && out.is_empty() {
            return Ok(Reply { status: 404, body: json!({"error": format!("alias [{}] missing", name.unwrap_or_default()), "status": 404}) });
        }
        ok(Json::Object(out))
    }

    async fn count_of(&self, ix: &Index, pred: &Option<String>, params: &[(String, Value)]) -> Result<i64, OsError> {
        let sql = format!("SELECT COUNT(*) FROM {}{}", ix.table, pred.as_ref().map(|p| format!(" WHERE {p}")).unwrap_or_default());
        Ok(self.rows(&sql, params).await?.rows.first().and_then(|r| r.first()).and_then(Json::as_i64).unwrap_or(0))
    }

    async fn stats(&self, expr: &str) -> Result<Reply, OsError> {
        let ixs = self.resolve(expr).await?;
        let mut indices = Map::new();
        let mut total = 0;
        for ix in &ixs {
            let n = self.count_of(ix, &None, &[]).await?;
            total += n;
            let s = json!({"docs": {"count": n, "deleted": 0}});
            indices.insert(ix.name.clone(), json!({"uuid": ix.uuid, "primaries": s, "total": s}));
        }
        let all = json!({"docs": {"count": total, "deleted": 0}});
        ok(json!({"_shards": shards(), "_all": {"primaries": all, "total": all}, "indices": indices}))
    }

    async fn cat(&self, what: &str, expr: Option<&str>, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let row = |cells: &[(&str, String)]| cells.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<Vec<(String, String)>>();
        let epoch = || (crate::telemetry::now_ms() / 1000).to_string();
        // Each row's columns in OpenSearch's order.
        let rows: Vec<Vec<(String, String)>> = match what {
            "indices" => {
                let ixs = match expr {
                    Some(e) => self.resolve(e).await?,
                    None => self.all().await?,
                };
                let mut out = Vec::new();
                for ix in &ixs {
                    let n = self.count_of(ix, &None, &[]).await?;
                    out.push(row(&[("health", "green".into()), ("status", "open".into()), ("index", ix.name.clone()), ("uuid", ix.uuid.clone()), ("pri", "1".into()), ("rep", "0".into()), ("docs.count", n.to_string()), ("docs.deleted", "0".into()), ("store.size", "-".into()), ("pri.store.size", "-".into())]));
                }
                out
            }
            "aliases" => {
                let mut out = Vec::new();
                for ix in self.all().await? {
                    for a in &ix.aliases {
                        out.push(row(&[("alias", a.clone()), ("index", ix.name.clone()), ("filter", "-".into()), ("routing.index", "-".into()), ("routing.search", "-".into()), ("is_write_index", "-".into())]));
                    }
                }
                out
            }
            "health" => {
                let h = self.health().await;
                let s = |k: &str| h[k].as_str().map(String::from).unwrap_or_else(|| h[k].to_string());
                vec![row(&[("epoch", epoch()), ("timestamp", "00:00:00".into()), ("cluster", s("cluster_name")), ("status", s("status")), ("node.total", "1".into()), ("node.data", "1".into()), ("discovered_cluster_manager", "true".into()), ("shards", s("active_shards")), ("pri", s("active_primary_shards")), ("relo", "0".into()), ("init", "0".into()), ("unassign", "0".into()), ("pending_tasks", "0".into()), ("max_task_wait_time", "-".into()), ("active_shards_percent", "100.0%".into())])]
            }
            "count" => {
                let ixs = match expr {
                    Some(e) => self.resolve(e).await?,
                    None => self.all().await?,
                };
                let mut n = 0;
                for ix in &ixs {
                    n += self.count_of(ix, &None, &[]).await?;
                }
                vec![row(&[("epoch", epoch()), ("timestamp", "00:00:00".into()), ("count", n.to_string())])]
            }
            other => return Err(OsError::new(400, "illegal_argument_exception", format!("ChloeDB's _cat answers indices, aliases, health and count, not [{other}]"))),
        };
        // `h`: the columns asked for, in its order.
        let all_cols: Vec<String> = match rows.first() {
            Some(r) => r.iter().map(|(k, _)| k.clone()).collect(),
            None => Vec::new(),
        };
        let cols: Vec<String> = match q.get("h") {
            Some(h) => h.split(',').map(str::trim).filter(|c| all_cols.iter().any(|x| x == c)).map(String::from).collect(),
            None => all_cols,
        };
        let mut table: Vec<Vec<String>> = rows.iter().map(|r| cols.iter().map(|c| r.iter().find(|(k, _)| k == c).map(|(_, v)| v.clone()).unwrap_or_default()).collect()).collect();
        // `s`: sorted by these columns (`index`, `docs.count:desc`).
        if let Some(sorts) = q.get("s") {
            let keys: Vec<(usize, bool)> = sorts.split(',').filter_map(|k| {
                let (name, desc) = match k.split_once(':') {
                    Some((n, d)) => (n, d.eq_ignore_ascii_case("desc")),
                    None => (k, false),
                };
                cols.iter().position(|c| c == name).map(|i| (i, desc))
            }).collect();
            table.sort_by(|a, b| {
                for &(i, desc) in &keys {
                    let o = match (a[i].parse::<f64>(), b[i].parse::<f64>()) {
                        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                        _ => a[i].cmp(&b[i]),
                    };
                    let o = if desc { o.reverse() } else { o };
                    if o.is_ne() {
                        return o;
                    }
                }
                std::cmp::Ordering::Equal
            });
        }
        if q.get("format").is_some_and(|f| f == "json") {
            return ok(Json::Array(table.into_iter().map(|r| Json::Object(cols.iter().cloned().zip(r.into_iter().map(Json::String)).collect())).collect()));
        }
        // A table of text, as `_cat` writes one.
        let mut widths: Vec<usize> = cols.iter().map(String::len).collect();
        for r in &table {
            for (i, v) in r.iter().enumerate() {
                widths[i] = widths[i].max(v.len());
            }
        }
        let mut text = String::new();
        let line = |vals: &[String], text: &mut String| {
            let l: Vec<String> = vals.iter().enumerate().map(|(i, v)| format!("{v:<w$}", w = widths[i])).collect();
            text.push_str(l.join(" ").trim_end());
            text.push('\n');
        };
        if q.contains_key("v") {
            line(&cols, &mut text);
        }
        for r in &table {
            line(r, &mut text);
        }
        ok(Json::String(text))
    }

    // -------------------------------------------------------------- documents

    /// Applies bulk items: each index's documents checked against its
    /// mapping (fields it lacks added, as dynamic mapping does), the rows
    /// they replace read, and every write sent in one request.
    async fn apply(&self, who: &Principal, default_index: Option<&str>, items: Vec<Item>) -> Vec<Outcome> {
        let n = items.len();
        let mut out: Vec<Option<Outcome>> = (0..n).map(|_| None).collect();
        // Each item's index.
        let mut ixs: BTreeMap<String, Arc<Index>> = BTreeMap::new();
        let mut of_item: Vec<Option<String>> = vec![None; n];
        for (i, it) in items.iter().enumerate() {
            let id = it.id.clone().unwrap_or_default();
            let Some(name) = it.index.clone().or_else(|| default_index.map(String::from)) else {
                out[i] = Some(Outcome::failed(it.action, "", &id, OsError::new(400, "action_request_validation_exception", "Validation Failed: 1: index is missing;")));
                continue;
            };
            if let Some(ix) = ixs.values().find(|x| x.name == name || x.aliases.contains(&name)) {
                of_item[i] = Some(ix.name.clone());
                continue;
            }
            let found = match self.resolve(&name).await {
                Ok(mut v) if v.len() == 1 => Ok(v.pop()),
                Ok(v) if v.is_empty() => Ok(None),
                Ok(_) => Err(OsError::new(400, "illegal_argument_exception", format!("no write index is defined for alias [{name}]: ChloeDB writes to an alias of one index"))),
                Err(e) => Err(e),
            };
            let ix = match found {
                Ok(Some(ix)) => Ok(ix),
                Ok(None) if it.action == Action::Delete => Err(OsError::not_found(&name)),
                // Made at the first write, as OpenSearch makes it.
                Ok(None) => self.make_index(who, &name, &json!({})).await,
                Err(e) => Err(e),
            };
            match ix {
                Ok(ix) => {
                    of_item[i] = Some(ix.name.clone());
                    ixs.insert(ix.name.clone(), ix);
                }
                Err(e) => out[i] = Some(Outcome::failed(it.action, &name, &id, e)),
            }
        }
        // Documents, with dotted keys made objects; fields new to their
        // index's mapping.
        let mut docs: Vec<Option<Map<String, Json>>> = vec![None; n];
        let mut patches: Vec<Option<Json>> = vec![None; n];
        for (i, it) in items.iter().enumerate() {
            if out[i].is_some() {
                continue;
            }
            let id = it.id.clone().unwrap_or_default();
            let fail = |e: OsError| Some(Outcome::failed(it.action, of_item[i].as_deref().unwrap_or_default(), &id, e));
            match (it.action, &it.body) {
                (Action::Index | Action::Create, Some(Json::Object(m))) => match mapping::expand_dots(m.clone()) {
                    Ok(d) => docs[i] = Some(d),
                    Err(e) => out[i] = fail(e),
                },
                (Action::Update, Some(b)) => {
                    if b.get("script").is_some() {
                        out[i] = fail(OsError::new(400, "illegal_argument_exception", "ChloeDB does not run scripts: send a partial document (doc) instead"));
                        continue;
                    }
                    let expand = |v: Option<&Json>| -> Result<Option<Json>, OsError> {
                        match v {
                            Some(Json::Object(m)) => Ok(Some(Json::Object(mapping::expand_dots(m.clone())?))),
                            None | Some(Json::Null) => Ok(None),
                            Some(_) => Err(OsError::new(400, "x_content_parse_exception", "[UpdateRequest] doc is an object")),
                        }
                    };
                    match (expand(b.get("doc")), expand(b.get("upsert"))) {
                        (Ok(d), Ok(u)) => {
                            patches[i] = d.clone();
                            docs[i] = u.or(d).and_then(|v| v.as_object().cloned());
                        }
                        (Err(e), _) | (_, Err(e)) => out[i] = fail(e),
                    }
                }
                (Action::Delete, _) => {}
                _ => out[i] = fail(OsError::mapper("failed to parse: the document is not an object")),
            }
            if out[i].is_none() && it.action != Action::Delete && it.id.as_deref() == Some("") {
                out[i] = fail(OsError::new(400, "action_request_validation_exception", "Validation Failed: 1: an id must not be empty;"));
            }
        }
        for (name, ix) in ixs.clone() {
            let mut mappings: Option<Json> = None;
            let mut cur = (*ix).clone();
            for i in 0..n {
                if out[i].is_some() || of_item[i].as_deref() != Some(name.as_str()) {
                    continue;
                }
                for d in [docs[i].as_ref(), patches[i].as_ref().and_then(Json::as_object)].into_iter().flatten() {
                    if let Some(m) = mapping::new_fields(&cur, d) {
                        match cur.dynamic() {
                            "strict" => {
                                out[i] = Some(Outcome::failed(items[i].action, &name, items[i].id.as_deref().unwrap_or_default(), OsError::new(400, "strict_dynamic_mapping_exception", "mapping set to strict, dynamic introduction of a field within the document is not allowed")));
                            }
                            "false" => {}
                            _ => match Index::new(&cur.name, &cur.table, &cur.uuid, cur.created_ms, m.clone(), cur.settings.clone(), cur.aliases.clone()) {
                                Ok(next) => {
                                    cur = next;
                                    mappings = Some(m);
                                }
                                Err(e) => out[i] = Some(Outcome::failed(items[i].action, &name, items[i].id.as_deref().unwrap_or_default(), e)),
                            },
                        }
                    }
                }
            }
            if let Some(m) = mappings {
                match self.remap(&ix, m).await {
                    Ok(next) => {
                        ixs.insert(name.clone(), next);
                    }
                    Err(e) => {
                        for i in 0..n {
                            if out[i].is_none() && of_item[i].as_deref() == Some(name.as_str()) {
                                out[i] = Some(Outcome::failed(items[i].action, &name, items[i].id.as_deref().unwrap_or_default(), e.clone()));
                            }
                        }
                    }
                }
            }
        }
        // The rows the items replace: each index's, read by key once.
        let mut current: HashMap<(String, String), Option<(i64, Json)>> = HashMap::new();
        for (name, ix) in &ixs {
            let ids: Vec<String> = (0..n).filter(|&i| out[i].is_none() && of_item[i].as_deref() == Some(name.as_str())).filter_map(|i| items[i].id.clone()).collect();
            if ids.is_empty() {
                continue;
            }
            let mut uniq = ids.clone();
            uniq.sort();
            uniq.dedup();
            let cols = ix.read_columns();
            match self.svc.get_rows(Some(&ix.table), uniq.iter().map(|k| Value::Str(k.clone())).collect(), Some(cols.clone()), &After::Any, 0, false).await {
                Ok(got) => {
                    for (k, row) in uniq.iter().zip(got.rows) {
                        let v = row.map(|(r, _)| {
                            let vals: Vec<Json> = r.iter().map(crate::json::to_json).collect();
                            let (_, ver, doc) = mapping::doc_of(ix, &got.columns, &vals);
                            (ver, doc)
                        });
                        current.insert((name.clone(), k.clone()), v);
                    }
                }
                Err(e) => {
                    let e = OsError::from(e);
                    for i in 0..n {
                        if out[i].is_none() && of_item[i].as_deref() == Some(name.as_str()) {
                            out[i] = Some(Outcome::failed(items[i].action, name, items[i].id.as_deref().unwrap_or_default(), e.clone()));
                        }
                    }
                }
            }
        }
        // Each item in turn, seeing the ones before it.
        let mut ops = Vec::new();
        let mut pending = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if out[i].is_some() {
                continue;
            }
            let name = of_item[i].clone().unwrap_or_default();
            let ix = ixs[&name].clone();
            let id = it.id.clone().unwrap_or_else(new_id);
            let key = (name.clone(), id.clone());
            let now = current.get(&key).cloned().flatten();
            let fail = |e: OsError| Some(Outcome::failed(it.action, &name, &id, e));
            let version = now.as_ref().map_or(0, |(v, _)| *v);
            if let Some(s) = it.if_seq_no {
                if now.is_none() || s != version - 1 {
                    out[i] = fail(OsError::new(409, "version_conflict_engine_exception", format!("[{id}]: version conflict, required seqNo [{s}], primary term [1]. current document has seqNo [{}] and primary term [1]", version - 1)));
                    continue;
                }
            }
            let next_version = match (it.external, it.version) {
                (true, Some(v)) => {
                    if now.is_some() && v <= version {
                        out[i] = fail(OsError::new(409, "version_conflict_engine_exception", format!("[{id}]: version conflict, current version [{version}] is higher or equal to the one provided [{v}]")));
                        continue;
                    }
                    v
                }
                _ => version + 1,
            };
            match it.action {
                Action::Delete => {
                    if now.is_none() {
                        out[i] = Some(Outcome { action: it.action, index: name.clone(), id: id.clone(), status: 404, result: "not_found", version: 1, error: None });
                        continue;
                    }
                    ops.push(json!({"op": "delete", "table": ix.table, "key": id}));
                    current.insert(key, None);
                    out[i] = Some(Outcome { action: it.action, index: name.clone(), id: id.clone(), status: 200, result: "deleted", version: next_version, error: None });
                }
                Action::Index | Action::Create | Action::Update => {
                    let doc = match (it.action, &now) {
                        (Action::Create, Some(_)) => {
                            out[i] = fail(OsError::new(409, "version_conflict_engine_exception", format!("[{id}]: version conflict, document already exists (current version [{version}])")));
                            continue;
                        }
                        (Action::Update, Some((_, cur))) => {
                            let mut d = cur.clone();
                            if let Some(p) = &patches[i] {
                                merge(&mut d, p);
                            }
                            let noop = it.body.as_ref().and_then(|b| b.get("detect_noop")).and_then(Json::as_bool) != Some(false);
                            if noop && &d == cur {
                                out[i] = Some(Outcome { action: it.action, index: name.clone(), id: id.clone(), status: 200, result: "noop", version, error: None });
                                continue;
                            }
                            d.as_object().cloned().unwrap_or_default()
                        }
                        (Action::Update, None) => {
                            let as_upsert = it.body.as_ref().and_then(|b| b.get("doc_as_upsert")).and_then(Json::as_bool) == Some(true);
                            let upsert = it.body.as_ref().and_then(|b| b.get("upsert")).is_some();
                            if !(as_upsert || upsert) {
                                out[i] = fail(OsError::new(404, "document_missing_exception", format!("[{id}]: document missing")));
                                continue;
                            }
                            docs[i].clone().unwrap_or_default()
                        }
                        _ => docs[i].clone().unwrap_or_default(),
                    };
                    match mapping::row_of(&ix, &id, next_version, &doc) {
                        Ok(row) => {
                            ops.push(json!({"op": "upsert", "table": ix.table, "doc": row}));
                            current.insert(key, Some((next_version, Json::Object(doc))));
                            let (status, result) = if now.is_some() { (200, "updated") } else { (201, "created") };
                            out[i] = Some(Outcome { action: it.action, index: name.clone(), id: id.clone(), status, result, version: next_version, error: None });
                            pending.push(i);
                        }
                        Err(e) => out[i] = fail(e),
                    }
                    continue;
                }
            }
            pending.push(i);
        }
        if !ops.is_empty() {
            if let Err(e) = self.write(ops).await {
                for i in pending {
                    let o = out[i].take().expect("an outcome");
                    out[i] = Some(Outcome::failed(o.action, &o.index, &o.id, e.clone()));
                }
            }
        }
        out.into_iter().map(|o| o.expect("every item has an outcome")).collect()
    }

    async fn bulk(&self, who: &Principal, ix: Option<&str>, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let t0 = Instant::now();
        let items = parse_bulk(body)?;
        let outcomes = self.apply(who, ix, items).await;
        let errors = outcomes.iter().any(|o| o.error.is_some());
        let items: Vec<Json> = outcomes.iter().map(|o| json!({o.action.name(): o.json()})).collect();
        let mut j = json!({"took": t0.elapsed().as_millis() as u64, "errors": errors, "items": items});
        if q.get("refresh").is_some_and(|r| r.is_empty() || r == "true") {
            j["forced_refresh"] = json!(true);
        }
        ok(j)
    }

    async fn index_doc(&self, who: &Principal, ix: &str, id: Option<&str>, create: bool, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let doc = body_json(body)?;
        let item = Item {
            action: if create { Action::Create } else { Action::Index },
            index: Some(ix.to_string()),
            id: id.map(String::from),
            body: Some(doc),
            if_seq_no: q.get("if_seq_no").and_then(|v| v.parse().ok()),
            version: q.get("version").and_then(|v| v.parse().ok()),
            external: q.get("version_type").is_some_and(|t| t.starts_with("external")),
        };
        self.one_item(who, item, q).await
    }

    async fn one_item(&self, who: &Principal, item: Item, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let o = self.apply(who, None, vec![item]).await.pop().expect("one outcome");
        if let Some(e) = o.error {
            return Err(e.index(&o.index));
        }
        let mut j = o.json();
        let status = j["status"].as_u64().unwrap_or(200) as u16;
        if let Some(m) = j.as_object_mut() {
            m.remove("status");
        }
        if q.get("refresh").is_some_and(|r| r.is_empty() || r == "true") {
            j["forced_refresh"] = json!(true);
        }
        Ok(Reply { status, body: j })
    }

    async fn update_doc(&self, who: &Principal, ix: &str, id: &str, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let item = Item { action: Action::Update, index: Some(ix.into()), id: Some(id.into()), body: Some(b), if_seq_no: q.get("if_seq_no").and_then(|v| v.parse().ok()), version: None, external: false };
        self.one_item(who, item, q).await
    }

    async fn delete_doc(&self, who: &Principal, ix: &str, id: &str, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let item = Item { action: Action::Delete, index: Some(ix.into()), id: Some(id.into()), body: None, if_seq_no: q.get("if_seq_no").and_then(|v| v.parse().ok()), version: None, external: false };
        self.one_item(who, item, q).await
    }

    /// Documents by id, as `_doc` and `_mget` answer them.
    async fn docs(&self, ix: &Index, ids: &[String], src: &SourceFilter) -> Result<Vec<Json>, OsError> {
        let cols = if src.on { ix.read_columns() } else { vec![ID.into(), VERSION.into()] };
        let got = self.svc.get_rows(Some(&ix.table), ids.iter().map(|k| Value::Str(k.clone())).collect(), Some(cols), &After::Any, 0, false).await?;
        Ok(ids
            .iter()
            .zip(got.rows)
            .map(|(id, row)| match row {
                None => json!({"_index": ix.name, "_id": id, "found": false}),
                Some((r, _)) => {
                    let vals: Vec<Json> = r.iter().map(crate::json::to_json).collect();
                    let (_, v, doc) = mapping::doc_of(ix, &got.columns, &vals);
                    let mut j = json!({"_index": ix.name, "_id": id, "_version": v, "_seq_no": v - 1, "_primary_term": 1, "found": true});
                    if src.on {
                        j["_source"] = src.apply(&doc);
                    }
                    j
                }
            })
            .collect())
    }

    async fn get_doc(&self, ix: &str, id: &str, q: &HashMap<String, String>, source_only: bool) -> Result<Reply, OsError> {
        let index = self.one(ix).await?;
        let src = SourceFilter::of(None, q);
        let d = self.docs(&index, &[id.to_string()], &src).await?.pop().unwrap_or(Json::Null);
        let found = d["found"] == json!(true);
        if source_only {
            if !found {
                return Err(OsError::new(404, "resource_not_found_exception", format!("Document not found [{}]/[{id}]", index.name)).index(&index.name));
            }
            return ok(d["_source"].clone());
        }
        Ok(Reply { status: if found { 200 } else { 404 }, body: d })
    }

    async fn mget(&self, ix: Option<&str>, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let src = SourceFilter::of(None, q);
        let mut asks: Vec<(String, String, SourceFilter)> = Vec::new();
        if let Some(ids) = b.get("ids").and_then(Json::as_array) {
            let i = ix.ok_or_else(|| OsError::new(400, "action_request_validation_exception", "Validation Failed: 1: index is missing for doc 0;"))?;
            for id in ids {
                asks.push((i.to_string(), dsl::id_text(id), src.clone()));
            }
        }
        if let Some(docs) = b.get("docs").and_then(Json::as_array) {
            for (k, d) in docs.iter().enumerate() {
                let i = d.get("_index").and_then(Json::as_str).or(ix).ok_or_else(|| OsError::new(400, "action_request_validation_exception", format!("Validation Failed: 1: index is missing for doc {k};")))?;
                let id = d.get("_id").map(dsl::id_text).ok_or_else(|| OsError::new(400, "action_request_validation_exception", format!("Validation Failed: 1: id is missing for doc {k};")))?;
                let s = if d.get("_source").is_some() { SourceFilter::of(d.get("_source"), &HashMap::new()) } else { src.clone() };
                asks.push((i.to_string(), id, s));
            }
        }
        let mut out = Vec::new();
        for (i, id, s) in asks {
            match self.one(&i).await {
                Ok(index) => out.extend(self.docs(&index, &[id], &s).await?),
                Err(e) => out.push(json!({"_index": i, "_id": id, "error": e.cause()})),
            }
        }
        ok(json!({"docs": out}))
    }

    // -------------------------------------------------------------- search

    async fn search_req(&self, expr: &str, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let (mut j, _) = self.search(expr, &b, q).await?;
        if q.contains_key("scroll") {
            j["_scroll_id"] = json!(scroll_id(expr, &b, q, &j));
        }
        if let Some(m) = j.as_object_mut() {
            m.remove(AFTER);
        }
        ok(j)
    }

    async fn scroll(&self, id: Option<&str>, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let sid = id.map(String::from).or_else(|| b.get("scroll_id").and_then(Json::as_str).map(String::from)).or_else(|| q.get("scroll_id").cloned()).ok_or_else(|| OsError::new(400, "action_request_validation_exception", "Validation Failed: 1: scrollId is missing;"))?;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sid.trim()).map_err(|_| OsError::new(404, "search_context_missing_exception", "No search context found: the scroll id is not one this server gave"))?;
        let s: Json = serde_json::from_slice(&raw).map_err(|_| OsError::new(404, "search_context_missing_exception", "No search context found"))?;
        let expr = s["i"].as_str().unwrap_or("*").to_string();
        let sb = s["b"].clone();
        let sq: HashMap<String, String> = s["q"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
        let (mut j, _) = self.search(&expr, &sb, &sq).await?;
        j["_scroll_id"] = json!(scroll_id(&expr, &sb, &sq, &j));
        if let Some(m) = j.as_object_mut() {
            m.remove(AFTER);
        }
        ok(j)
    }

    async fn msearch(&self, ix: Option<&str>, body: &Bytes) -> Result<Reply, OsError> {
        let t0 = Instant::now();
        let text = std::str::from_utf8(body).map_err(|_| OsError::new(400, "illegal_argument_exception", "the body is not UTF-8"))?;
        let lines: Vec<&str> = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
        let mut asks = Vec::new();
        for pair in lines.chunks(2) {
            let header: Json = serde_json::from_str(pair[0]).map_err(|e| OsError::new(400, "illegal_argument_exception", format!("msearch header: {e}")))?;
            let b: Json = match pair.get(1) {
                Some(l) => serde_json::from_str(l).map_err(|e| OsError::new(400, "illegal_argument_exception", format!("msearch body: {e}")))?,
                None => json!({}),
            };
            let expr = match header.get("index") {
                Some(Json::String(s)) => s.clone(),
                Some(Json::Array(a)) => a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(","),
                _ => ix.unwrap_or("*").to_string(),
            };
            asks.push((expr, b));
        }
        let none = HashMap::new();
        let futs = asks.iter().map(|(e, b)| self.search(e, b, &none));
        let res = futures::future::join_all(futs).await;
        let responses: Vec<Json> = res
            .into_iter()
            .map(|r| match r {
                Ok((mut j, _)) => {
                    j["status"] = json!(200);
                    j
                }
                Err(e) => e.json(),
            })
            .collect();
        ok(json!({"took": t0.elapsed().as_millis() as u64, "responses": responses}))
    }

    async fn count(&self, expr: &str, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let b = body_json(body)?;
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() && !(expr == "*" || expr == "_all") {
            return Err(OsError::not_found(expr));
        }
        let mut n = 0;
        for ix in &ixs {
            let mut cx = Ctx::new(ix);
            let c = self.query_of(&mut cx, &b, q)?;
            n += self.count_of(ix, &c.pred, &cx.params).await?;
        }
        ok(json!({"count": n, "_shards": shards()}))
    }

    /// The query of a search body, or of `q=` (a query string).
    fn query_of(&self, cx: &mut Ctx, b: &Json, q: &HashMap<String, String>) -> Result<Compiled, OsError> {
        if let Some(text) = q.get("q") {
            let mut qs = json!({"query": text});
            if let Some(df) = q.get("df") {
                qs["default_field"] = json!(df);
            }
            if q.get("default_operator").is_some_and(|o| o.eq_ignore_ascii_case("and")) {
                qs["default_operator"] = json!("and");
            }
            return dsl::compile(&json!({"query_string": qs}), cx);
        }
        match b.get("query") {
            Some(query) => dsl::compile(query, cx),
            None => Ok(Compiled { pred: None, score: Score::Const(1.0) }),
        }
    }

    /// A search: its answer, and the index it read (the first, of
    /// several). A search of several indexes searches each for the hits up
    /// to its page's end and merges them in its order; `collapse` keeps
    /// the first hit of each value of its field, reading more until the
    /// page is full or every hit is read.
    async fn search(&self, expr: &str, b: &Json, q: &HashMap<String, String>) -> Result<(Json, Arc<Index>), OsError> {
        let t0 = Instant::now();
        let ixs = self.resolve(expr).await?;
        let collapse = match b.get("collapse") {
            None => None,
            Some(c) => {
                if c.get("inner_hits").is_some() {
                    return Err(OsError::parsing("ChloeDB's collapse takes no inner_hits"));
                }
                Some(c.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[collapse] requires field"))?.to_string())
            }
        };
        if ixs.len() <= 1 && collapse.is_none() {
            return self.search_one(expr, b, q).await;
        }
        let scrolling = q.contains_key("scroll") || b.get("scroll").is_some() || b.get(SCROLL_AFTER).is_some() || b.get("_scroll_from").is_some();
        if scrolling {
            return Err(OsError::new(400, "illegal_argument_exception", "ChloeDB scrolls one index at a time, without collapse"));
        }
        if ixs.len() > 1 && (b.get("aggs").is_some() || b.get("aggregations").is_some()) {
            return Err(OsError::new(400, "illegal_argument_exception", format!("[{expr}] names {} indexes; ChloeDB aggregates one index (or an alias of one) at a time", ixs.len())));
        }
        if b.get("search_after").is_some() && collapse.is_some() {
            return Err(OsError::parsing("ChloeDB's collapse takes no search_after"));
        }
        let page = |k: &str, d: usize| -> Result<usize, OsError> {
            let v = q.get(k).map(|s| json!(s)).or_else(|| b.get(k).cloned());
            match v {
                None | Some(Json::Null) => Ok(d),
                Some(v) => match v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())) {
                    Some(n) if n >= 0 => Ok(n as usize),
                    _ => Err(OsError::new(400, "illegal_argument_exception", format!("[{k}] parameter cannot be negative"))),
                },
            }
        };
        let (from, size) = (page("from", 0)?, page("size", 10)?);
        if from + size > MAX_WINDOW {
            return Err(OsError::new(400, "illegal_argument_exception", format!("Result window is too large, from + size must be less than or equal to: [{MAX_WINDOW}] but was [{}]. See the scroll api for a more efficient way to request large data sets.", from + size)));
        }
        let mut inner = b.clone();
        if let Some(m) = inner.as_object_mut() {
            m.remove("collapse");
            m.remove("from");
        }
        let mut q2 = q.clone();
        q2.remove("from");
        q2.remove("size");
        if let Some(f) = &collapse {
            let mut dv = match inner.get("docvalue_fields") {
                Some(Json::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            dv.push(json!(f));
            inner["docvalue_fields"] = Json::Array(dv);
        }
        let dirs = sort_dirs(b, q);
        let want = from + size;
        let mut fetch = if collapse.is_some() { (want * 2).max(want + 10).min(MAX_WINDOW) } else { want };
        loop {
            inner["size"] = json!(fetch);
            let answers = futures::future::join_all(ixs.iter().map(|ix| self.search_one(&ix.name, &inner, &q2))).await;
            let answers: Vec<Json> = answers.into_iter().map(|r| r.map(|(j, _)| j)).collect::<Result<_, _>>()?;
            let mut all: Vec<(usize, usize, Json)> = Vec::new();
            let mut more = false;
            for (i, a) in answers.iter().enumerate() {
                let hits = a["hits"]["hits"].as_array().cloned().unwrap_or_default();
                more |= hits.len() >= fetch;
                all.extend(hits.into_iter().enumerate().map(|(k, h)| (i, k, h)));
            }
            all.sort_by(|x, y| {
                let ord = if dirs.is_empty() {
                    let s = |h: &Json| h["_score"].as_f64().unwrap_or(f64::MIN);
                    s(&y.2).partial_cmp(&s(&x.2)).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    let (a, c) = (x.2["sort"].as_array(), y.2["sort"].as_array());
                    let mut o = std::cmp::Ordering::Equal;
                    for (k, desc) in dirs.iter().enumerate() {
                        let (u, v) = (a.and_then(|a| a.get(k)).unwrap_or(&Json::Null), c.and_then(|c| c.get(k)).unwrap_or(&Json::Null));
                        o = cmp_sort(u, v, *desc);
                        if o.is_ne() {
                            break;
                        }
                    }
                    o
                };
                ord.then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1))
            });
            let mut hits: Vec<Json> = all.into_iter().map(|(_, _, h)| h).collect();
            if let Some(f) = &collapse {
                let mut seen = std::collections::HashSet::new();
                hits.retain(|h| seen.insert(h["fields"][f].get(0).cloned().unwrap_or(Json::Null).to_string()));
            }
            if collapse.is_some() && hits.len() < want && more && fetch < MAX_WINDOW {
                fetch = (fetch * 4).min(MAX_WINDOW);
                continue;
            }
            let page: Vec<Json> = hits.into_iter().skip(from).take(size).collect();
            let max_score = answers.iter().filter_map(|a| a["hits"]["max_score"].as_f64()).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
            let mut hits_j = json!({"max_score": max_score, "hits": page});
            if answers.iter().all(|a| a["hits"].get("total").is_some()) {
                let value: i64 = answers.iter().map(|a| a["hits"]["total"]["value"].as_i64().unwrap_or(0)).sum();
                let gte = answers.iter().any(|a| a["hits"]["total"]["relation"] == "gte");
                hits_j["total"] = json!({"value": value, "relation": if gte { "gte" } else { "eq" }});
            }
            let mut j = json!({"took": t0.elapsed().as_millis() as u64, "timed_out": false, "_shards": shards(), "hits": hits_j});
            if let Some(aggs) = answers.first().and_then(|a| a.get("aggregations")) {
                j["aggregations"] = aggs.clone();
            }
            return Ok((j, ixs[0].clone()));
        }
    }

    /// A search of one index.
    async fn search_one(&self, expr: &str, b: &Json, q: &HashMap<String, String>) -> Result<(Json, Arc<Index>), OsError> {
        let t0 = Instant::now();
        let ix = match self.resolve(expr).await? {
            v if v.is_empty() && (expr == "*" || expr == "_all") => return Ok((empty_search(), Arc::new(Index::new("", "", "", 0, json!({}), json!({}), Vec::new())?))),
            mut v if v.len() == 1 => v.pop().expect("one"),
            v if v.is_empty() => return Err(OsError::not_found(expr)),
            v => return Err(OsError::new(400, "illegal_argument_exception", format!("[{expr}] names {} indexes ({}); ChloeDB's OpenSearch API searches one index (or an alias of one) at a time", v.len(), v.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(", ")))),
        };
        let num = |k: &str, d: usize| -> Result<usize, OsError> {
            match q.get(k).map(|s| s.parse::<i64>()).or_else(|| b.get(k).map(|v| v.as_i64().ok_or(()).or_else(|_| v.as_str().unwrap_or("x").parse::<i64>().map_err(|_| ())).map_err(|_| "x".parse::<i64>().unwrap_err()))) {
                None => Ok(d),
                Some(Ok(n)) if n >= 0 => Ok(n as usize),
                Some(_) => Err(OsError::new(400, "illegal_argument_exception", format!("[{k}] parameter cannot be negative"))),
            }
        };
        let size = num("size", 10)?;
        let from = num("from", 0)?;
        let window = ix.settings.pointer("/index/max_result_window").or_else(|| ix.settings.get("max_result_window")).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(MAX_WINDOW as u64) as usize;
        let scrolling = q.contains_key("scroll") || b.get("scroll").is_some() || b.get("_scroll_from").is_some();
        if from + size > window && !scrolling {
            return Err(OsError::new(400, "illegal_argument_exception", format!("Result window is too large, from + size must be less than or equal to: [{window}] but was [{}]. See the scroll api for a more efficient way to request large data sets.", from + size)));
        }
        let mut cx = Ctx::new(&ix);
        cx.exact = from + size > 100;
        let c = self.query_of(&mut cx, b, q)?;
        let post = match b.get("post_filter") {
            Some(pf) => dsl::unscored(&mut cx, |cx| dsl::compile(pf, cx))?.pred,
            None => None,
        };
        let and2 = |a: &Option<String>, b: &Option<String>| -> Option<String> {
            match (a, b) {
                (Some(a), Some(b)) => Some(format!("({a}) AND ({b})")),
                (Some(x), None) | (None, Some(x)) => Some(x.clone()),
                (None, None) => None,
            }
        };
        let mut hits_pred = and2(&c.pred, &post);
        // The total counts the hits before search_after's.
        let count_pred = hits_pred.clone();
        // The order.
        let mut sorts: Vec<Json> = match b.get("sort") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(a)) => a.clone(),
            Some(s) => vec![s.clone()],
        };
        if let Some(s) = q.get("sort") {
            sorts = s.split(',').map(|p| match p.split_once(':') {
                Some((f, d)) => json!({f: {"order": d}}),
                None => json!(p),
            }).collect();
        }
        let mut order: Vec<String> = Vec::new();
        // What each sort key reads, for the hits' `sort` values.
        let mut sort_cols: Vec<(String, SortVal)> = Vec::new();
        let ranked = c.score.is_ranked();
        let mut by_score_first = false;
        for (k, s) in sorts.iter().enumerate() {
            let (field, spec) = match s {
                Json::String(f) => (f.clone(), json!({})),
                Json::Object(m) => {
                    let (f, v) = m.iter().next().ok_or_else(|| OsError::parsing("a sort key is a field"))?;
                    match v {
                        Json::String(d) => (f.clone(), json!({"order": d})),
                        v => (f.clone(), v.clone()),
                    }
                }
                _ => return Err(OsError::parsing("a sort key is a field or {\"field\": {\"order\": \"desc\"}}")),
            };
            let desc = spec.get("order").and_then(Json::as_str).map_or(field == "_score", |o| o.eq_ignore_ascii_case("desc"));
            let dir = if desc { "DESC" } else { "ASC" };
            match field.as_str() {
                "_score" => {
                    if let Score::Expr(e) = &c.score {
                        if k == 0 {
                            by_score_first = true;
                        }
                        order.push(format!("{e} {dir}"));
                        sort_cols.push(("score()".into(), SortVal::Score));
                    } else {
                        sort_cols.push((String::new(), SortVal::Const));
                    }
                }
                "_doc" => sort_cols.push((String::new(), SortVal::Doc)),
                "_geo_distance" | "_script" => return Err(OsError::parsing(format!("ChloeDB does not sort by [{field}]"))),
                f => {
                    let t = cx.target(f);
                    if matches!(t, Target::Vector(_)) {
                        return Err(OsError::parsing(format!("can't sort on the vector field [{f}]")));
                    }
                    let nulls = match spec.get("missing").and_then(Json::as_str) {
                        Some("_first") => " NULLS FIRST",
                        Some("_last") => " NULLS LAST",
                        _ => "",
                    };
                    order.push(format!("{} {dir}{nulls}", t.sql()));
                    sort_cols.push((t.sql(), if matches!(t, Target::Date(_)) { SortVal::Date } else { SortVal::Plain }));
                }
            }
        }
        if sorts.is_empty() && ranked {
            if let Score::Expr(e) = &c.score {
                order.push(format!("{e} DESC"));
                by_score_first = true;
            }
        }
        // A scroll in no order but the documents' own pages through them by
        // `_hash`, from the last page's last document on: past any window.
        let keyset = scrolling && !by_score_first && sort_cols.iter().all(|(_, k)| matches!(k, SortVal::Doc));
        let from = if keyset { 0 } else { from };
        if keyset {
            order = vec![format!("{} ASC", ident(HASH))];
        }
        // search_after: the rows after these sort values.
        if let Some(after) = b.get("search_after").and_then(Json::as_array) {
            if sorts.is_empty() || after.len() != sort_cols.len() {
                return Err(OsError::parsing("search_after takes one value for each sort key"));
            }
            let mut ors = Vec::new();
            for k in 0..sort_cols.len() {
                let mut ands = Vec::new();
                for (j, (col, kind)) in sort_cols.iter().enumerate().take(k + 1) {
                    if matches!(kind, SortVal::Score | SortVal::Const | SortVal::Doc) {
                        return Err(OsError::parsing("ChloeDB's search_after follows field sorts, not _score or _doc"));
                    }
                    let desc = order[j].contains(" DESC");
                    let v = match (kind, &after[j]) {
                        (SortVal::Date, Json::Number(n)) => lit(&chloedb::timestamp::format(n.as_i64().unwrap_or(0) * 1000)),
                        (_, Json::String(s)) => lit(s),
                        (_, Json::Number(n)) => n.to_string(),
                        (_, Json::Bool(b)) => if *b { "TRUE".into() } else { "FALSE".into() },
                        (_, v) => return Err(OsError::parsing(format!("search_after value [{v}]"))),
                    };
                    let op = if j < k { "=" } else if desc { "<" } else { ">" };
                    ands.push(format!("{col} {op} {v}"));
                }
                ors.push(format!("({})", ands.join(" AND ")));
            }
            hits_pred = and2(&hits_pred, &Some(ors.join(" OR ")));
        }
        if let (true, Some(a)) = (keyset, b.get(SCROLL_AFTER).and_then(Json::as_i64)) {
            hits_pred = and2(&hits_pred, &Some(format!("{} > {a}", ident(HASH))));
        }
        let src = SourceFilter::of(b.get("_source"), q);
        let highlight = b.get("highlight").cloned();
        let fields_asked: Vec<String> = ["fields", "docvalue_fields", "stored_fields"].iter().filter_map(|k| b.get(*k)).flat_map(|v| match v {
            Json::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from).or_else(|| x.get("field").and_then(Json::as_str).map(String::from))).collect::<Vec<_>>(),
            Json::String(s) => vec![s.clone()],
            _ => Vec::new(),
        }).filter(|f| f != "_none_").collect();
        let need_doc = src.on || highlight.is_some() || !fields_asked.is_empty();
        // The total.
        let track = match q.get("track_total_hits").map(|s| json!(s.parse::<i64>().map(Json::from).unwrap_or(json!(s == "true")))).or_else(|| b.get("track_total_hits").cloned()) {
            Some(Json::Bool(false)) => None,
            Some(Json::Bool(true)) => Some(i64::MAX),
            Some(Json::Number(n)) => Some(n.as_i64().unwrap_or(10_000)),
            _ => Some(10_000),
        };
        let knn = cx.knn_k;
        let mut limit = size;
        if let Some(k) = knn {
            limit = k.saturating_sub(from).min(size);
        }
        let params = cx.params.clone();
        // The hits' columns: the document's, the score, then each field
        // sort key's from `sort_base` on.
        let mut cols: Vec<String> = if need_doc { ix.read_columns().iter().map(|c| ident(c)).collect() } else { vec![ident(ID), ident(VERSION)] };
        if by_score_first {
            // score() reads a ranking of scores; one of conditions and
            // numbers alone (`count_true`, `doc['f'].value`) is its own
            // column.
            cols.push(match &c.score {
                Score::Expr(e) if !dsl::has_score(e) => format!("{e} AS {}", ident(SCORE_COL)),
                _ => "score()".into(),
            });
        }
        let sort_base = cols.len();
        let hits_sql = if limit > 0 {
            for (col, kind) in &sort_cols {
                if matches!(kind, SortVal::Plain | SortVal::Date) {
                    cols.push(col.clone());
                }
            }
            if keyset {
                cols.push(ident(HASH));
            }
            let mut sql = format!("SELECT {} FROM {}", cols.join(", "), ix.table);
            if let Some(p) = &hits_pred {
                sql.push_str(&format!(" WHERE {p}"));
            }
            if !order.is_empty() {
                sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
            }
            sql.push_str(&format!(" LIMIT {limit}"));
            if from > 0 {
                sql.push_str(&format!(" OFFSET {from}"));
            }
            Some(sql)
        } else {
            None
        };
        let hits_f = async {
            match &hits_sql {
                Some(s) => self.rows(s, &params).await.map(Some),
                None => Ok(None),
            }
        };
        let count_f = async {
            match track {
                Some(_) => self.count_of(&ix, &count_pred, &params).await.map(Some),
                None => Ok(None),
            }
        };
        let aggs_f = async {
            match b.get("aggs").or_else(|| b.get("aggregations")) {
                Some(a) => aggs::run(self, &self.with_single_numbers(&ix).await, c.pred.clone(), a).await.map(Some),
                None => Ok(None),
            }
        };
        let (hits, total, aggs) = futures::join!(hits_f, count_f, aggs_f);
        let (hits, total, aggs) = (hits?, total?, aggs?);
        // A query ranked by bm25 returns the rows its words are in: the
        // rows the condition keeps without any of them (a filter's, with
        // optional words) come after those, scored 0, as OpenSearch ranks
        // them below.
        let hits = match (hits, by_score_first.then(|| cx.unscored()).flatten()) {
            (Some(mut r), Some(none_of)) if r.rows.len() < limit => {
                let ranked = match from {
                    0 => r.rows.len(),
                    _ => self.count_of(&ix, &and2(&hits_pred, &Some(format!("NOT ({none_of})"))), &params).await? as usize,
                };
                let fill: Vec<String> = cols.iter().map(|c| if c == "score()" { "0.0".to_string() } else { c.clone() }).collect();
                let mut sql = format!("SELECT {} FROM {} WHERE {}", fill.join(", "), ix.table, and2(&hits_pred, &Some(none_of)).unwrap_or_default());
                if order.len() > 1 {
                    sql.push_str(&format!(" ORDER BY {}", order[1..].join(", ")));
                }
                sql.push_str(&format!(" LIMIT {} OFFSET {}", limit - r.rows.len(), from.saturating_sub(ranked)));
                r.rows.extend(self.rows(&sql, &params).await?.rows);
                Some(r)
            }
            (h, _) => h,
        };
        let mut out_hits = Vec::new();
        let mut max_score: Option<f64> = None;
        // Where a keyset scroll's next page starts.
        let last_hash = match (&hits, keyset) {
            (Some(r), true) => r.rows.last().and_then(|row| row.last()).and_then(Json::as_i64),
            _ => None,
        };
        if let Some(r) = hits {
            let words_all: Vec<String> = cx.words.values().flatten().cloned().collect();
            for (k, row) in r.rows.iter().enumerate() {
                let (id, version, doc) = mapping::doc_of(&ix, &r.columns, row);
                let score: Json = if by_score_first {
                    // A float, as Lucene's scores are.
                    let s = r.columns.iter().position(|c| c == "score()" || c == SCORE_COL).and_then(|i| row[i].as_f64()).or_else(|| r.scores.get(k).copied()).map(|s| mapping::shortest(s as f32));
                    if let Some(s) = s {
                        max_score = Some(max_score.map_or(s, |m: f64| m.max(s)));
                    }
                    json!(s)
                } else if sorts.is_empty() {
                    let s = match c.score {
                        Score::Const(x) => x,
                        _ => 1.0,
                    };
                    max_score = Some(s);
                    json!(s)
                } else {
                    Json::Null
                };
                let mut h = json!({"_index": ix.name, "_id": id, "_score": score});
                if b.get("version").and_then(Json::as_bool) == Some(true) || q.get("version").is_some_and(|v| v == "true") {
                    h["_version"] = json!(version);
                }
                if b.get("seq_no_primary_term").and_then(Json::as_bool) == Some(true) {
                    h["_seq_no"] = json!(version - 1);
                    h["_primary_term"] = json!(1);
                }
                if src.on {
                    h["_source"] = src.apply(&doc);
                }
                if !fields_asked.is_empty() {
                    let mut f = Map::new();
                    for name in &fields_asked {
                        let pats: Vec<String> = if name.contains('*') { ix.fields.keys().filter(|k| dsl::glob(name, k)).cloned().collect() } else { vec![name.clone()] };
                        for p in pats {
                            let source = ix.fields.get(&p).map_or(p.clone(), |x| x.source.clone());
                            let vals: Vec<Json> = mapping::values_at(&doc, &source).into_iter().cloned().collect();
                            if !vals.is_empty() {
                                f.insert(p, Json::Array(vals));
                            }
                        }
                    }
                    if !f.is_empty() {
                        h["fields"] = Json::Object(f);
                    }
                }
                if let Some(hl) = &highlight {
                    let got = highlights(&ix, &cx.words, &words_all, hl, &doc);
                    if !got.is_empty() {
                        h["highlight"] = Json::Object(got);
                    }
                }
                if !sorts.is_empty() {
                    let mut vals = Vec::new();
                    let mut at = sort_base;
                    for (_, kind) in &sort_cols {
                        let v = match kind {
                            SortVal::Score => score.clone(),
                            SortVal::Const => json!(1.0),
                            SortVal::Doc => json!(from + k),
                            SortVal::Plain | SortVal::Date => {
                                at += 1;
                                sort_value(row.get(at - 1).cloned().unwrap_or(Json::Null), matches!(kind, SortVal::Date))
                            }
                        };
                        vals.push(v);
                    }
                    h["sort"] = Json::Array(vals);
                }
                out_hits.push(h);
            }
        }
        let mut hits_j = json!({"max_score": max_score, "hits": out_hits});
        if let (Some(n), Some(cap)) = (total, track) {
            let n = knn.map_or(n, |k| n.min(k as i64));
            let rel = if n > cap { "gte" } else { "eq" };
            let v = n.min(cap);
            if q.get("rest_total_hits_as_int").is_some_and(|v| v == "true") {
                hits_j["total"] = json!(v);
            } else {
                hits_j["total"] = json!({"value": v, "relation": rel});
            }
        }
        let mut j = json!({"took": t0.elapsed().as_millis() as u64, "timed_out": false, "_shards": shards(), "hits": hits_j});
        if let Some(h) = last_hash {
            j[AFTER] = json!(h);
        }
        if let Some(a) = aggs {
            j["aggregations"] = a;
        }
        Ok((j, ix))
    }

    /// `top_hits` of an aggregation's bucket.
    pub async fn top_hits(&self, ix: &Index, pred: Option<String>, spec: &Json) -> Result<Json, OsError> {
        let size = spec.get("size").and_then(Json::as_u64).unwrap_or(3) as usize;
        let from = spec.get("from").and_then(Json::as_u64).unwrap_or(0) as usize;
        let cx = Ctx::new(ix);
        let mut order = Vec::new();
        // Each sort key's column, and whether it is a date.
        let mut keys: Vec<(String, bool)> = Vec::new();
        let sorts = match spec.get("sort") {
            Some(Json::Array(a)) => a.clone(),
            Some(s) => vec![s.clone()],
            None => Vec::new(),
        };
        for s in sorts {
            let (f, d) = match &s {
                Json::String(f) => (f.clone(), "ASC"),
                Json::Object(m) => match m.iter().next() {
                    Some((f, v)) => (f.clone(), if v.as_str().or_else(|| v.get("order").and_then(Json::as_str)).is_some_and(|o| o.eq_ignore_ascii_case("desc")) { "DESC" } else { "ASC" }),
                    None => continue,
                },
                _ => continue,
            };
            if f == "_score" || f == "_doc" {
                continue;
            }
            let t = cx.target(&f);
            order.push(format!("{} {d}", t.sql()));
            keys.push((t.sql(), matches!(t, Target::Date(_))));
        }
        let src = SourceFilter::of(spec.get("_source"), &HashMap::new());
        let mut cols: Vec<String> = ix.read_columns().iter().map(|c| ident(c)).collect();
        let base = cols.len();
        cols.extend(keys.iter().map(|(c, _)| c.clone()));
        let mut sql = format!("SELECT {} FROM {}", cols.join(", "), ix.table);
        if let Some(p) = &pred {
            sql.push_str(&format!(" WHERE {p}"));
        }
        if !order.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
        }
        sql.push_str(&format!(" LIMIT {size}"));
        if from > 0 {
            sql.push_str(&format!(" OFFSET {from}"));
        }
        let (r, n) = futures::join!(self.rows(&sql, &[]), self.count_of(ix, &pred, &[]));
        let (r, n) = (r?, n?);
        let hits: Vec<Json> = r
            .rows
            .iter()
            .map(|row| {
                let (id, _, doc) = mapping::doc_of(ix, &r.columns, row);
                let mut h = json!({"_index": ix.name, "_id": id, "_score": if order.is_empty() { json!(1.0) } else { Json::Null }});
                if src.on {
                    h["_source"] = src.apply(&doc);
                }
                if !keys.is_empty() {
                    h["sort"] = Json::Array(keys.iter().enumerate().map(|(k, (_, date))| sort_value(row.get(base + k).cloned().unwrap_or(Json::Null), *date)).collect());
                }
                h
            })
            .collect();
        Ok(json!({"hits": {"total": {"value": n, "relation": "eq"}, "max_score": if order.is_empty() { json!(1.0) } else { Json::Null }, "hits": hits}}))
    }

    async fn delete_by_query(&self, who: &Principal, expr: &str, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let t0 = Instant::now();
        let b = body_json(body)?;
        if b.get("query").is_none() && !q.contains_key("q") {
            return Err(OsError::new(400, "action_request_validation_exception", "Validation Failed: 1: query is missing;"));
        }
        let ixs = self.resolve(expr).await?;
        if ixs.is_empty() {
            return Err(OsError::not_found(expr));
        }
        let mut deleted = 0;
        for ix in ixs {
            let mut cx = Ctx::new(&ix);
            let c = self.query_of(&mut cx, &b, q)?;
            if !cx.params.is_empty() {
                return Err(OsError::parsing("ChloeDB's _delete_by_query does not take a knn query"));
            }
            let sql = format!("DELETE FROM {}{}", ix.table, c.pred.map(|p| format!(" WHERE {p}")).unwrap_or_default());
            if let SqlAnswer::Written { w, .. } = self.statement(who, &sql).await? {
                deleted += w.rows;
            }
        }
        ok(json!({"took": t0.elapsed().as_millis() as u64, "timed_out": false, "total": deleted, "deleted": deleted, "batches": 1, "version_conflicts": 0, "noops": 0, "retries": {"bulk": 0, "search": 0}, "throttled_millis": 0, "requests_per_second": -1.0, "throttled_until_millis": 0, "failures": []}))
    }

    async fn update_by_query(&self, expr: &str, body: &Bytes, q: &HashMap<String, String>) -> Result<Reply, OsError> {
        let t0 = Instant::now();
        let b = body_json(body)?;
        if b.get("script").is_some() {
            return Err(OsError::new(400, "illegal_argument_exception", "ChloeDB does not run scripts: update documents by _bulk with partial documents"));
        }
        // Without a script, an update by query writes each document as it
        // is, which here changes nothing.
        let ix = self.one(expr).await?;
        let mut cx = Ctx::new(&ix);
        let c = self.query_of(&mut cx, &b, q)?;
        let n = self.count_of(&ix, &c.pred, &cx.params).await?;
        ok(json!({"took": t0.elapsed().as_millis() as u64, "timed_out": false, "total": n, "updated": 0, "noops": n, "deleted": 0, "batches": 1, "version_conflicts": 0, "retries": {"bulk": 0, "search": 0}, "failures": []}))
    }
}

/// A hit's sort value as OpenSearch gives it: a date in milliseconds, an
/// array's first value.
fn sort_value(v: Json, date: bool) -> Json {
    match (date, v) {
        (true, Json::String(s)) => chloedb::timestamp::parse(&s).map_or(Json::Null, |t| json!(t.div_euclid(1000))),
        (_, Json::Array(a)) => a.first().cloned().map_or(Json::Null, |x| sort_value(x, date)),
        (_, v) => v,
    }
}

/// What a hit's sort value is.
enum SortVal {
    Score,
    Const,
    Doc,
    Plain,
    Date,
}

/// The direction of each key a search sorts by (true: descending), as
/// its hits' `sort` values come.
fn sort_dirs(b: &Json, q: &HashMap<String, String>) -> Vec<bool> {
    if let Some(s) = q.get("sort") {
        return s.split(',').map(|p| match p.split_once(':') {
            Some((_, d)) => d.eq_ignore_ascii_case("desc"),
            None => p == "_score",
        }).collect();
    }
    let sorts = match b.get("sort") {
        None | Some(Json::Null) => return Vec::new(),
        Some(Json::Array(a)) => a.clone(),
        Some(s) => vec![s.clone()],
    };
    sorts
        .iter()
        .map(|s| match s {
            Json::String(f) => f == "_score",
            Json::Object(m) => match m.iter().next() {
                Some((f, Json::String(d))) => d.eq_ignore_ascii_case("desc") || (f == "_score" && !d.eq_ignore_ascii_case("asc")),
                Some((f, v)) => v.get("order").and_then(Json::as_str).map_or(f == "_score", |o| o.eq_ignore_ascii_case("desc")),
                None => false,
            },
            _ => false,
        })
        .collect()
}

/// Two hits' values of one sort key in its order: missing values last.
fn cmp_sort(a: &Json, b: &Json, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let o = match (a, b) {
        (Json::Null, Json::Null) => return Equal,
        (Json::Null, _) => return Greater,
        (_, Json::Null) => return Less,
        (Json::Number(x), Json::Number(y)) => x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(Equal),
        (Json::String(x), Json::String(y)) => x.cmp(y),
        (Json::Bool(x), Json::Bool(y)) => x.cmp(y),
        _ => Equal,
    };
    if desc {
        o.reverse()
    } else {
        o
    }
}

fn empty_search() -> Json {
    json!({"took": 0, "timed_out": false, "_shards": {"total": 0, "successful": 0, "skipped": 0, "failed": 0}, "hits": {"total": {"value": 0, "relation": "eq"}, "max_score": null, "hits": []}})
}

/// A search's answer's `_hash` of its last hit, in a scroll that pages by
/// it (taken out of the answer).
const AFTER: &str = "_strata_after";

/// The `_hash` a scroll's next page starts after, in its search's body.
const SCROLL_AFTER: &str = "_scroll_after";
/// The column of a score that is no ranking of scores.
const SCORE_COL: &str = "_strata_score";

/// The id of a scroll: the search, and where its next page starts.
fn scroll_id(expr: &str, b: &Json, q: &HashMap<String, String>, answer: &Json) -> String {
    let size = b.get("size").and_then(Json::as_u64).or_else(|| q.get("size").and_then(|s| s.parse().ok())).unwrap_or(10);
    let from = b.get("from").and_then(Json::as_u64).or_else(|| q.get("from").and_then(|s| s.parse().ok())).unwrap_or(0);
    let got = answer["hits"]["hits"].as_array().map_or(0, Vec::len) as u64;
    let mut nb = b.clone();
    if !nb.is_object() {
        nb = json!({});
    }
    match answer.get(AFTER) {
        Some(h) => nb[SCROLL_AFTER] = h.clone(),
        None if got == 0 => nb[SCROLL_AFTER] = json!(i64::MAX),
        None => nb["from"] = json!(from + got),
    }
    nb["size"] = json!(size);
    nb["_scroll_from"] = json!(true);
    let mut nq: Map<String, Json> = q.iter().filter(|(k, _)| !matches!(k.as_str(), "scroll" | "from" | "size")).map(|(k, v)| (k.clone(), json!(v))).collect();
    nq.remove("scroll_id");
    let s = json!({"i": expr, "b": nb, "q": nq});
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.to_string())
}

/// An index of a [`META`] row (`name`, `aliases`, `meta`).
fn index_of_row(row: &[Json]) -> Result<Index, OsError> {
    let name = row.first().and_then(Json::as_str).unwrap_or_default();
    let aliases = match row.get(1) {
        Some(Json::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        Some(Json::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };
    Index::of_meta(name, aliases, row.get(2).and_then(Json::as_str).unwrap_or("{}"))
}

/// A hit's highlights, as a search's `highlight` asks for them.
fn highlights(ix: &Index, words: &BTreeMap<String, Vec<String>>, all: &[String], hl: &Json, doc: &Json) -> Map<String, Json> {
    let tags = |k: &str, d: &str| -> String {
        hl.get(k).and_then(|v| v.as_array().and_then(|a| a.first()).or(Some(v))).and_then(Json::as_str).unwrap_or(d).to_string()
    };
    let (pre, post) = (tags("pre_tags", "<em>"), tags("post_tags", "</em>"));
    let mut out = Map::new();
    let fields: Vec<(String, Json)> = match hl.get("fields") {
        Some(Json::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(Json::Array(a)) => a.iter().filter_map(Json::as_object).flat_map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone()))).collect(),
        _ => Vec::new(),
    };
    let require = hl.get("require_field_match").and_then(Json::as_bool).unwrap_or(true);
    for (pat, spec) in fields {
        let names: Vec<String> = if pat.contains('*') { ix.fields.keys().filter(|k| dsl::glob(&pat, k)).cloned().collect() } else { vec![pat.clone()] };
        for name in names {
            let Some(f) = ix.fields.get(&name) else { continue };
            let Kind::Text { analyzer, cut, .. } = &f.kind else { continue };
            let a = if *analyzer == "english" { chloedb::text::analyzer::Analyzer::English } else { chloedb::text::analyzer::Analyzer::Simple };
            let ws: Vec<String> = if require { words.get(&name).cloned().unwrap_or_default() } else { all.to_vec() };
            if ws.is_empty() {
                continue;
            }
            let terms = highlight::terms(&ws, a);
            let get = |k: &str, d: usize| spec.get(k).or_else(|| hl.get(k)).and_then(Json::as_u64).map_or(d, |x| x as usize);
            let ask = highlight::Ask {
                pre: &pre,
                post: &post,
                size: get("fragment_size", 100),
                count: get("number_of_fragments", 5),
                html: spec.get("encoder").or_else(|| hl.get("encoder")).and_then(Json::as_str) == Some("html"),
            };
            let mut frags = Vec::new();
            for v in mapping::values_at(doc, &f.source) {
                if let Some(text) = v.as_str() {
                    frags.extend(highlight::fragments(text, &terms, a, cut, &ask));
                }
            }
            if ask.count > 0 {
                frags.truncate(ask.count);
            }
            if !frags.is_empty() {
                out.insert(name.clone(), json!(frags));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_filtered() {
        let j = json!({"took": 1, "hits": {"total": {"value": 2}, "hits": [{"_id": "a", "_source": {"x": 1}}, {"_id": "b", "_source": {"x": 2}}]}});
        assert_eq!(filter_path(&j, "hits.hits._id"), json!({"hits": {"hits": [{"_id": "a"}, {"_id": "b"}]}}));
        assert_eq!(filter_path(&j, "-hits.hits._source,-took"), json!({"hits": {"total": {"value": 2}, "hits": [{"_id": "a"}, {"_id": "b"}]}}));
        assert_eq!(filter_path(&j, "**._id"), json!({"hits": {"hits": [{"_id": "a"}, {"_id": "b"}]}}));
    }

    #[test]
    fn sources_filtered() {
        let d = json!({"a": 1, "b": {"c": 2, "d": 3}, "e": "x"});
        let f = SourceFilter { on: true, includes: vec!["b.c".into(), "e".into()], excludes: vec![] };
        assert_eq!(f.apply(&d), json!({"b": {"c": 2}, "e": "x"}));
        let f = SourceFilter { on: true, includes: vec![], excludes: vec!["b.*".into()] };
        assert_eq!(f.apply(&d), json!({"a": 1, "b": {}, "e": "x"}));
        let f = SourceFilter { on: true, includes: vec!["b".into()], excludes: vec!["b.d".into()] };
        assert_eq!(f.apply(&d), json!({"b": {"c": 2}}));
    }

    #[test]
    fn bulk_bodies() {
        let body = Bytes::from("{\"index\":{\"_index\":\"i\",\"_id\":1}}\n{\"a\":1}\n{\"delete\":{\"_index\":\"i\",\"_id\":\"2\"}}\n{\"update\":{\"_id\":\"3\"}}\n{\"doc\":{\"b\":2}}\n");
        let items = parse_bulk(&body).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id.as_deref(), Some("1"));
        assert_eq!(items[1].action, Action::Delete);
        assert_eq!(items[2].body, Some(json!({"doc": {"b": 2}})));
        assert!(parse_bulk(&Bytes::from("{\"index\":{}}\n")).is_err());
    }

    #[test]
    fn credentials() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Basic dXNlcjpzM2NyZXQ=".parse().unwrap());
        assert_eq!(credential(&h).as_deref(), Some("Bearer s3cret"));
        h.insert(header::AUTHORIZATION, "Bearer k".parse().unwrap());
        assert_eq!(credential(&h).as_deref(), Some("Bearer k"));
    }

    #[test]
    fn decoding() {
        assert_eq!(decode("a%2Cb+c"), "a,b c");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode_path("a+b"), "a+b");
    }
}
