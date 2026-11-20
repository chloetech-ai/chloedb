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

/// A parameter as the page shows it: a vector as its length, a picture as
/// its size, so that an answer does not carry them back.
fn shown(v: &Value) -> Value {
    match v {
        Value::Array(a) if a.len() > 16 && a.iter().all(Value::is_number) => json!(format!("[a vector of {} numbers]", a.len())),
        Value::String(s) if s.len() > 300 => json!(format!("<{} bytes>", s.len())),
        Value::Array(a) => Value::Array(a.iter().map(shown).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), shown(x))).collect()),
        x => x.clone(),
    }
}

/// The queries one request ran: their SQL, parameters, rows and time.
#[derive(Clone, Default)]
pub struct Trace(Arc<Mutex<Vec<Value>>>);

impl Trace {
    pub fn new() -> Trace {
        Trace::default()
    }

    pub fn push(
        &self,
        what: &str,
        sql: &str,
        params: &Value,
        rows: usize,
        ms: f64,
        note: Option<&str>,
    ) {
        let mut q =
            json!({"what": what, "sql": sql, "rows": rows, "ms": (ms * 100.0).round() / 100.0});
        if params.as_object().is_some_and(|o| !o.is_empty()) {
            q["params"] = shown(params);
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
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("an HTTP client");
        Strata {
            url: url.trim_end_matches('/').to_string(),
            key: key.filter(|k| !k.is_empty()),
            http,
        }
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        self.call_within(method, path, body, None).await
    }

    /// A statement that may take minutes (`CREATE RECOMMENDER` over
    /// millions of events): the client waits up to `secs`.
    pub async fn sql_long(&self, sql: &str, secs: u64) -> Result<Value> {
        self.call_within(
            reqwest::Method::POST,
            "/v1/sql",
            Some(&json!({ "sql": sql })),
            Some(Duration::from_secs(secs)),
        )
        .await
    }

    async fn call_within(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let mut req = self
            .http
            .request(method, format!("{}{}", self.url, path))
            .header("x-chloedb-client", "chloedb-earth");
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let r = req.send().await.map_err(|e| Error {
            status: 0,
            message: format!("{}: {}", self.url, e),
        })?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(j);
        }
        let message = j["error"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| text.chars().take(400).collect());
        Err(Error {
            status: status.as_u16(),
            message,
        })
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
        self.query_within(sql, params, None).await
    }

    /// As [`Strata::query`], given `ms` milliseconds rather than the
    /// server's default deadline.
    pub async fn query_within(&self, sql: &str, params: Value, ms: Option<u64>) -> Result<Vec<Row>> {
        let mut body = json!({ "sql": sql, "params": params, "objects": true });
        if let Some(ms) = ms {
            body["timeout_ms"] = json!(ms);
        }
        let j = self.post("/v1/query", &body).await?;
        Ok(rows_of(j))
    }

    /// Every row of a SELECT, however many: a scroll, `page` rows a page
    /// (a query alone answers at most 10,000).
    pub async fn scroll(&self, sql: &str, params: Value, page: usize) -> Result<Vec<Row>> {
        self.scroll_map(sql, params, page, Some).await
    }

    /// As [`Strata::scroll`], each row turned into a `T` as its page
    /// arrives, so that only one page of JSON is held at a time (the
    /// places are 100,000 rows: kept as rows, they took several hundred
    /// megabytes at their peak).
    pub async fn scroll_map<T>(&self, sql: &str, params: Value, page: usize, mut f: impl FnMut(Row) -> Option<T>) -> Result<Vec<T>> {
        let mut j = self.post("/v1/query", &json!({ "sql": sql, "params": params, "objects": true, "page_size": page, "keep_alive_ms": 60000 })).await?;
        let mut all = Vec::new();
        loop {
            let cols: Vec<String> = j["columns"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
            let cursor = j["cursor"].as_str().map(String::from);
            let rows = match j.get_mut("rows").map(Value::take) {
                Some(Value::Array(a)) => a,
                _ => Vec::new(),
            };
            let n = rows.len();
            for r in rows {
                let row = match r {
                    Value::Object(m) => m,
                    // A scroll's next pages are arrays, in the columns' order.
                    Value::Array(a) => cols.iter().cloned().zip(a).collect(),
                    _ => continue,
                };
                all.extend(f(row));
            }
            drop(j);
            match cursor {
                Some(c) if n > 0 => j = self.post("/v1/query", &json!({ "cursor": c })).await?,
                _ => break,
            }
        }
        Ok(all)
    }

    /// As [`Strata::query`], kept in `trace` under the name `what`.
    pub async fn traced(
        &self,
        trace: &Trace,
        what: &str,
        sql: &str,
        params: Value,
    ) -> Result<Vec<Row>> {
        self.traced_within(trace, what, sql, params, None).await
    }

    /// As [`Strata::traced`], given `ms` milliseconds.
    pub async fn traced_within(
        &self,
        trace: &Trace,
        what: &str,
        sql: &str,
        params: Value,
        ms: Option<u64>,
    ) -> Result<Vec<Row>> {
        let t0 = Instant::now();
        let r = self.query_within(sql, params.clone(), ms).await;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match &r {
            Ok(rows) => trace.push(what, sql, &params, rows.len(), ms, None),
            Err(e) => trace.push(
                what,
                sql,
                &params,
                0,
                ms,
                Some(&format!("refused: {}", e.message)),
            ),
        }
        r
    }

    pub async fn health(&self) -> Result<Value> {
        self.get("/v1/health").await
    }

    /// The names of the server's tables.
    pub async fn tables(&self) -> Result<Vec<String>> {
        let j = self.get("/v1/tables").await?;
        Ok(j["tables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["table"].as_str().map(str::to_string))
            .collect())
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
                    Err(e)
                        if (e.status == 0 || e.status == 429 || e.status >= 500)
                            && t0.elapsed() < Duration::from_secs(60) =>
                    {
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
        let mut req = self
            .http
            .post(format!("{}/v1/import?format=jsonl&table={table}", self.url))
            .header("x-chloedb-client", "chloedb-earth")
            .header("content-type", "application/x-ndjson")
            .timeout(Duration::from_secs(1800))
            .body(body);
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        let r = req.send().await.map_err(|e| Error {
            status: 0,
            message: format!("{}: {}", self.url, e),
        })?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(j);
        }
        Err(Error {
            status: status.as_u16(),
            message: j["error"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| text.chars().take(400).collect()),
        })
    }

    pub async fn upsert(&self, table: &str, rows: Vec<Value>) -> Result<usize> {
        self.write(
            rows.into_iter()
                .map(|doc| json!({"op": "upsert", "table": table, "doc": doc}))
                .collect(),
        )
        .await
    }
}

fn rows_of(j: Value) -> Vec<Row> {
    match j {
        Value::Object(mut o) => match o.remove("rows") {
            Some(Value::Array(a)) => a
                .into_iter()
                .filter_map(|r| {
                    if let Value::Object(m) = r {
                        Some(m)
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        },
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
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .collect(),
            Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}
