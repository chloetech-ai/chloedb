//! A client of chloedb-server's HTTP API: SQL, queries with parameters and
//! writes in batches. Every query the app runs is timed and can be kept in
//! a [`Trace`], which is how each screen shows the SQL behind its numbers.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub struct Error {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "HTTP {}: {}", self.status, self.message)
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
pub type Row = Map<String, Value>;

/// The queries one request ran: their SQL, parameters, rows and time.
#[derive(Clone, Default)]
pub struct Trace(Arc<Mutex<Vec<Value>>>);

impl Trace {
    pub fn new() -> Trace {
        Trace::default()
    }

    pub fn push(&self, what: &str, sql: &str, params: &Value, rows: usize, ms: f64, note: Option<&str>) {
        self.push_timed(what, sql, params, rows, ms, None, note)
    }

    /// As [`Trace::push`], with the server's own time (`took_ms`): the
    /// rest of `ms` is the network and the wait before the server began.
    #[allow(clippy::too_many_arguments)]
    pub fn push_timed(&self, what: &str, sql: &str, params: &Value, rows: usize, ms: f64, server: Option<f64>, note: Option<&str>) {
        let mut q = json!({"what": what, "sql": sql, "rows": rows, "ms": (ms * 100.0).round() / 100.0});
        if let Some(s) = server {
            q["server_ms"] = json!((s * 100.0).round() / 100.0);
        }
        if params.as_object().is_some_and(|o| !o.is_empty()) {
            q["params"] = params.clone();
        }
        if let Some(n) = note {
            q["note"] = json!(n);
        }
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(q);
    }

    pub fn take(&self) -> Vec<Value> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn list(&self) -> Vec<Value> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[derive(Clone)]
pub struct Strata {
    pub url: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl Strata {
    pub fn new(url: &str, key: Option<String>) -> Strata {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client");
        Strata { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), http }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut req = self.http.request(method, format!("{}{}", self.url, path)).header("x-chloedb-client", "chloedb-sound");
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let r = req.send().await.map_err(|e| Error { status: 0, message: format!("{}: {}", self.url, e) })?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(j);
        }
        let message = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(400).collect());
        Err(Error { status: status.as_u16(), message })
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.call(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// A statement that is not a query (CREATE TABLE, UPDATE, DELETE).
    pub async fn sql(&self, sql: &str) -> Result<Value> {
        self.post("/v1/sql", &json!({ "sql": sql })).await
    }

    /// A SELECT's rows, each an object keyed by column.
    pub async fn query(&self, sql: &str, params: Value) -> Result<Vec<Row>> {
        let j = self.post("/v1/query", &json!({ "sql": sql, "params": params, "objects": true })).await?;
        Ok(rows_of(j))
    }

    /// As [`Strata::query`], refused by the server past `ms` milliseconds
    /// (its default deadline is 5 s).
    pub async fn query_within(&self, sql: &str, params: Value, ms: u64) -> Result<Vec<Row>> {
        let j = self.post("/v1/query", &json!({ "sql": sql, "params": params, "objects": true, "timeout_ms": ms })).await?;
        Ok(rows_of(j))
    }

    /// As [`Strata::query`], kept in `trace` under the name `what`.
    pub async fn traced(&self, trace: &Trace, what: &str, sql: &str, params: Value) -> Result<Vec<Row>> {
        self.traced_within(trace, what, sql, params, None).await
    }

    /// As [`Strata::traced`], with a deadline of its own.
    pub async fn traced_within(&self, trace: &Trace, what: &str, sql: &str, params: Value, within: Option<u64>) -> Result<Vec<Row>> {
        let t0 = Instant::now();
        let mut body = json!({ "sql": sql, "params": params.clone(), "objects": true });
        if let Some(ms) = within {
            body["timeout_ms"] = json!(ms);
        }
        let r = self.post("/v1/query", &body).await;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match r {
            Ok(j) => {
                let server = j["took_ms"].as_f64();
                let rows = rows_of(j);
                trace.push_timed(what, sql, &params, rows.len(), ms, server, None);
                Ok(rows)
            }
            Err(e) => {
                trace.push(what, sql, &params, 0, ms, Some(&format!("refused: {}", e.message)));
                Err(e)
            }
        }
    }

    /// Every row of a SELECT, in pages of `page` rows read from one version
    /// of the data (a scroll), each page handed to `each` as it comes.
    pub async fn scroll(&self, sql: &str, params: Value, page: usize, mut each: impl FnMut(Vec<Row>)) -> Result<usize> {
        // Pages of at most 10,000 rows (the server's --max-result-window by
        // default), as arrays in the order of the columns; a first page the
        // cluster did not answer in time is asked again.
        let first = json!({ "sql": sql, "params": params, "page_size": page.clamp(1, 10_000), "keep_alive_ms": 120_000, "timeout_ms": 120_000 });
        let mut tries = 0;
        let mut j = loop {
            match self.post("/v1/query", &first).await {
                Ok(j) => break j,
                Err(e) if tries < 3 && (e.status == 0 || e.status >= 500) => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_secs(5 * tries)).await;
                }
                Err(e) => return Err(e),
            }
        };
        let mut n = 0;
        loop {
            let cursor = j["cursor"].as_str().map(str::to_string);
            let rows = rows_of(j);
            n += rows.len();
            each(rows);
            let Some(c) = cursor else { return Ok(n) };
            j = self.post("/v1/query", &json!({ "cursor": c, "keep_alive_ms": 120_000, "timeout_ms": 120_000 })).await?;
        }
    }

    pub async fn health(&self) -> Result<Value> {
        self.get("/v1/health").await
    }

    /// The names of the server's tables.
    pub async fn tables(&self) -> Result<Vec<String>> {
        let j = self.get("/v1/tables").await?;
        Ok(j["tables"].as_array().into_iter().flatten().filter_map(|t| t["table"].as_str().map(str::to_string)).collect())
    }

    /// Writes `ops` in batches of 1,000, each a transaction of its own. A
    /// 429 or a failure of the network or the server is tried again for up
    /// to a minute.
    pub async fn write(&self, ops: Vec<Value>) -> Result<usize> {
        let mut n = 0;
        for chunk in ops.chunks(1000) {
            let body = json!({ "ops": chunk });
            let t0 = Instant::now();
            let mut backoff = Duration::from_millis(300);
            loop {
                match self.post("/v1/write", &body).await {
                    Ok(_) => break,
                    Err(e) if (e.status == 0 || e.status == 429 || e.status >= 500) && t0.elapsed() < Duration::from_secs(60) => {
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(5));
                    }
                    Err(e) => return Err(e),
                }
            }
            n += chunk.len();
        }
        Ok(n)
    }

    /// Rows as JSON lines into a table (`POST /v1/import`), each line a row.
    pub async fn import_lines(&self, table: &str, body: Vec<u8>) -> Result<Value> {
        let mut req = self.http.post(format!("{}/v1/import?format=jsonl&table={table}", self.url)).header("x-chloedb-client", "chloedb-sound").header("content-type", "application/x-ndjson").timeout(Duration::from_secs(1800)).body(body);
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        let r = req.send().await.map_err(|e| Error { status: 0, message: format!("{}: {}", self.url, e) })?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(j);
        }
        Err(Error { status: status.as_u16(), message: j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(400).collect()) })
    }

    pub async fn upsert(&self, table: &str, rows: Vec<Value>) -> Result<usize> {
        self.write(rows.into_iter().map(|doc| json!({"op": "upsert", "table": table, "doc": doc})).collect()).await
    }
}

fn rows_of(j: Value) -> Vec<Row> {
    match j {
        Value::Object(mut o) => {
            let columns: Vec<String> = match o.remove("columns") {
                Some(Value::Array(c)) => c.into_iter().map(|c| c.as_str().unwrap_or("").to_string()).collect(),
                _ => Vec::new(),
            };
            match o.remove("rows") {
                // Objects, or (a scroll's later pages) arrays in the order of the columns.
                Some(Value::Array(a)) => a
                    .into_iter()
                    .filter_map(|r| match r {
                        Value::Object(m) => Some(m),
                        Value::Array(vals) if !columns.is_empty() => Some(columns.iter().cloned().zip(vals).collect()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// A row's text, number or flag by column, with a default.
pub trait RowExt {
    fn s(&self, k: &str) -> String;
    fn f(&self, k: &str) -> f64;
    fn i(&self, k: &str) -> i64;
    fn list(&self, k: &str) -> Vec<String>;
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
            Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

