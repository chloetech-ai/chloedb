//! A client of chloedb-server's HTTP API (SQL, lookups by key, writes),
//! and the trace of what one request ran, which the page shows in its SQL
//! drawer: each statement with its label, parameters, rows and time.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::util::{Error, Result};

pub type Row = Map<String, Value>;

tokio::task_local! {
    /// The statements the current request ran (set by the API per request).
    pub static TRACE: Arc<Mutex<Vec<Value>>>;
}

/// A parameter as the drawer shows it: vectors and long text cut short.
fn shown(v: &Value) -> Value {
    match v {
        Value::Array(a) if a.len() > 8 && a.iter().take(8).all(Value::is_number) => json!(format!("[{} numbers]", a.len())),
        Value::Array(a) => {
            let mut out: Vec<Value> = a.iter().take(12).map(shown).collect();
            if a.len() > 12 {
                out.push(json!(format!("… {} more", a.len() - 12)));
            }
            Value::Array(out)
        }
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), shown(x))).collect()),
        Value::String(s) if s.chars().count() > 240 => json!(format!("{}…", s.chars().take(240).collect::<String>())),
        _ => v.clone(),
    }
}

pub fn record(label: &str, sql: &str, params: &Value, t0: Instant, rows: Option<usize>) {
    let _ = TRACE.try_with(|t| {
        let mut t = t.lock().unwrap_or_else(|e| e.into_inner());
        if t.len() < 200 {
            let ms = (t0.elapsed().as_secs_f64() * 100_000.0).round() / 100.0;
            let params = if params.is_null() { json!({}) } else { shown(params) };
            t.push(json!({"label": label, "sql": sql.trim(), "params": params, "ms": ms, "rows": rows}));
        }
    });
}

/// The trace so far (for an answer's `queries`).
pub fn trace() -> Vec<Value> {
    TRACE.try_with(|t| t.lock().unwrap_or_else(|e| e.into_inner()).clone()).unwrap_or_default()
}

#[derive(Clone)]
pub struct Db {
    pub url: String,
    key: Option<String>,
    pub http: reqwest::Client,
    /// "server" or "coordinator": a coordinator walks no graph.
    pub role: String,
    pub vision: bool,
    pub recommender: Arc<std::sync::atomic::AtomicBool>,
}

impl Db {
    pub fn new(url: &str, key: Option<String>) -> Db {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client");
        Db { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), http, role: "server".into(), vision: false,
             recommender: Arc::new(std::sync::atomic::AtomicBool::new(false)) }
    }

    pub fn graph_in_db(&self) -> bool {
        self.role != "coordinator"
    }

    pub async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut req = self.http.request(method, format!("{}{}", self.url, path)).header("x-chloedb-client", "chloedb-parley");
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let r = req.send().await.map_err(|e| Error::new(502, format!("ChloeDB: {e}")))?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(j);
        }
        let msg = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(400).collect());
        Err(Error { status: status.as_u16(), message: msg, answer: Some(j) })
    }

    pub async fn health(&self) -> Result<Value> {
        self.call(reqwest::Method::GET, "/v1/health", None).await
    }

    /// Asks the server what it is (a server or a coordinator).
    pub async fn connect(&mut self) -> Result<()> {
        let h = self.health().await?;
        self.role = if h["role"].as_str().unwrap_or("").eq_ignore_ascii_case("coordinator") { "coordinator".into() } else { "server".into() };
        Ok(())
    }

    pub async fn tables(&self) -> Result<Vec<String>> {
        let j = self.call(reqwest::Method::GET, "/v1/tables", None).await?;
        Ok(j["tables"].as_array().into_iter().flatten()
            .filter_map(|t| t["short"]["table"].as_str().or(t["table"].as_str()).or(t["name"].as_str()).map(str::to_string)).collect())
    }

    /// A SELECT's rows, each an object keyed by column; kept in the trace as `label`.
    pub async fn q(&self, label: &str, sql: &str, params: Value) -> Result<Vec<Row>> {
        let t0 = Instant::now();
        let r = self.call(reqwest::Method::POST, "/v1/query", Some(&json!({"sql": sql, "params": params, "objects": true}))).await;
        match r {
            Ok(j) => {
                let rows = rows_of(j);
                record(label, sql, &params, t0, Some(rows.len()));
                Ok(rows)
            }
            Err(e) => {
                record(label, sql, &params, t0, None);
                Err(e)
            }
        }
    }

    pub async fn one(&self, label: &str, sql: &str, params: Value) -> Result<Option<Row>> {
        Ok(self.q(label, sql, params).await?.into_iter().next())
    }

    pub async fn scalar(&self, label: &str, sql: &str, params: Value) -> Result<Value> {
        Ok(self.one(label, sql, params).await?.and_then(|r| r.into_iter().next().map(|(_, v)| v)).unwrap_or(Value::Null))
    }

    /// The row with this key, or None.
    pub async fn get(&self, table: &str, key: &str) -> Result<Option<Row>> {
        let t0 = Instant::now();
        let path = format!("/v1/kv/{}/{}", enc(table), enc(key));
        let r = self.call(reqwest::Method::GET, &path, None).await;
        let out = match r {
            Ok(mut j) => match j["row"].take() {
                Value::Object(m) => Some(m),
                _ => None,
            },
            Err(e) if e.status == 404 => None,
            Err(e) => return Err(e),
        };
        record(&format!("get:{table}"), &format!("SELECT * FROM {table} WHERE id = :key  -- a lookup by key"), &json!({"key": key}), t0,
               Some(usize::from(out.is_some())));
        Ok(out)
    }

    pub async fn get_many(&self, table: &str, keys: &[String]) -> Result<Vec<Option<Row>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let t0 = Instant::now();
        let j = self.call(reqwest::Method::POST, "/v1/kv/get", Some(&json!({"table": table, "keys": keys}))).await?;
        let rows: Vec<Option<Row>> = j["rows"].as_array().cloned().unwrap_or_default().into_iter().map(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        }).collect();
        record(&format!("get:{table}"), &format!("SELECT * FROM {table} WHERE id IN (:keys)  -- lookups by key, one call"), &json!({"keys": keys}), t0,
               Some(rows.iter().filter(|r| r.is_some()).count()));
        Ok(rows)
    }

    /// One atomic write of `ops`. Through a coordinator, a write some shards
    /// took is sent again once with its idempotency key.
    pub async fn write(&self, label: &str, ops: Vec<Value>) -> Result<Value> {
        let t0 = Instant::now();
        let body = json!({"ops": ops});
        let r = match self.call(reqwest::Method::POST, "/v1/write", Some(&body)).await {
            Err(e) if e.answer.as_ref().is_some_and(|a| a["partial"].as_bool() == Some(true)) => {
                let key = e.answer.as_ref().map(|a| a["idempotency_key"].clone()).unwrap_or(Value::Null);
                self.call(reqwest::Method::POST, "/v1/write", Some(&json!({"ops": body["ops"], "idempotency_key": key}))).await
            }
            r => r,
        }?;
        let mut kinds: Vec<(String, usize)> = Vec::new();
        for o in body["ops"].as_array().into_iter().flatten() {
            let k = format!("{} {}", o["op"].as_str().unwrap_or("put"), o["table"].as_str().unwrap_or("")).trim().to_uppercase();
            match kinds.iter_mut().find(|(x, _)| *x == k) {
                Some(e) => e.1 += 1,
                None => kinds.push((k, 1)),
            }
        }
        let sql = format!("-- one atomic write\n{}", kinds.iter().map(|(k, n)| format!("{k} ({n} {})", if *n == 1 { "row" } else { "rows" })).collect::<Vec<_>>().join("\n"));
        record(label, &sql, &Value::Null, t0, r["written"].as_u64().map(|n| n as usize));
        Ok(r)
    }

    /// A statement that is not a query (CREATE, UPDATE, DELETE).
    pub async fn sql(&self, stmt: &str, params: Value) -> Result<Value> {
        let t0 = Instant::now();
        let r = self.call(reqwest::Method::POST, "/v1/sql", Some(&json!({"sql": stmt, "params": params}))).await;
        record("sql", stmt, &params, t0, None);
        r
    }

    /// A server route that is not SQL (/v1/highlight, /v1/correct).
    pub async fn post(&self, label: &str, path: &str, body: Value) -> Result<Value> {
        let t0 = Instant::now();
        let r = self.call(reqwest::Method::POST, path, Some(&body)).await;
        record(label, &format!("POST {path}"), &body, t0, None);
        r
    }

    pub async fn embed(&self, text: &str, field: &str, table: &str) -> Result<Vec<f32>> {
        let j = self.call(reqwest::Method::POST, "/v1/embed", Some(&json!({"text": text, "field": field, "table": table}))).await?;
        Ok(j["vector"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(|x| x as f32).collect())
    }

    /// An image's bytes from the server's media store (a coordinator asks its shards).
    pub async fn media(&self, sha: &str, shards: &[String]) -> Option<Vec<u8>> {
        let bases: Vec<String> = if self.role == "coordinator" { shards.to_vec() } else { vec![self.url.clone()] };
        for b in bases {
            let mut req = self.http.get(format!("{}/v1/media/{sha}", b.trim_end_matches('/'))).timeout(Duration::from_secs(10));
            if let Some(k) = &self.key {
                req = req.bearer_auth(k);
            }
            if let Ok(r) = req.send().await {
                if r.status().is_success() {
                    if let Ok(b) = r.bytes().await {
                        return Some(b.to_vec());
                    }
                }
            }
        }
        None
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20")
}

pub fn rows_of(j: Value) -> Vec<Row> {
    match j {
        Value::Object(mut o) => {
            let columns: Vec<String> = match o.remove("columns") {
                Some(Value::Array(c)) => c.into_iter().map(|c| c.as_str().unwrap_or("").to_string()).collect(),
                _ => Vec::new(),
            };
            match o.remove("rows") {
                Some(Value::Array(a)) => a.into_iter().filter_map(|r| match r {
                    Value::Object(m) => Some(m),
                    Value::Array(vals) if !columns.is_empty() => Some(columns.iter().cloned().zip(vals).collect()),
                    _ => None,
                }).collect(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// A row's text, number or list by column, with a default.
pub trait RowExt {
    fn s(&self, k: &str) -> String;
    fn f(&self, k: &str) -> f64;
    fn i(&self, k: &str) -> i64;
    fn list(&self, k: &str) -> Vec<String>;
    fn has(&self, k: &str) -> bool;
}

impl RowExt for Row {
    fn s(&self, k: &str) -> String {
        match self.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }
    fn f(&self, k: &str) -> f64 {
        match self.get(k) {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::String(s)) => s.parse().unwrap_or(0.0),
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            _ => 0.0,
        }
    }
    fn i(&self, k: &str) -> i64 {
        self.f(k) as i64
    }
    fn list(&self, k: &str) -> Vec<String> {
        match self.get(k) {
            Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect(),
            Some(Value::String(s)) if s.starts_with('[') => serde_json::from_str::<Vec<String>>(s).unwrap_or_default(),
            Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }
    fn has(&self, k: &str) -> bool {
        !matches!(self.get(k), None | Some(Value::Null))
    }
}
