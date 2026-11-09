//! A client of chloedb-server's HTTP API: SQL, reads past the result window
//! (scrolls), writes in one transaction under an idempotency key (sent again
//! on 429 and on failures), the tables, and SQLite files passed on to its
//! importer.

use std::time::Duration;

use reqwest::StatusCode;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Error {
    pub status: u16,
    pub message: String,
    pub body: Option<Value>,
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

#[derive(Clone)]
pub struct Strata {
    pub url: String,
    key: Option<String>,
    http: reqwest::Client,
}

/// Rows a scroll reads a page at a time; the server's window is 10,000.
pub const PAGE: usize = 10_000;

impl Strata {
    pub fn new(url: &str, key: Option<String>) -> Strata {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(300)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client");
        Strata { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), http }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<reqwest::Body>, ctype: &str) -> Result<(Value, Option<Duration>)> {
        let mut req = self.http.request(method, format!("{}{}", self.url, path)).header("x-strata-client", "chloedb-sheets");
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        if let Some(b) = body {
            req = req.header(reqwest::header::CONTENT_TYPE, ctype).body(b);
        }
        let r = req.send().await.map_err(|e| Error { status: 0, message: format!("{}: {}", self.url, e), body: None })?;
        let status = r.status();
        let retry_after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<f64>().ok()).map(|s| Duration::from_secs_f64(s.clamp(0.05, 30.0)));
        let text = r.text().await.unwrap_or_default();
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok((j, None));
        }
        let message = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(300).collect());
        let mut body = if j.is_object() { j } else { json!({}) };
        if status == StatusCode::TOO_MANY_REQUESTS {
            body["retry_after_s"] = json!(retry_after.map(|d| d.as_secs_f64()));
        }
        Err(Error { status: status.as_u16(), message, body: Some(body) })
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        Ok(self.call(reqwest::Method::GET, path, None, "").await?.0)
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let b = serde_json::to_vec(body).expect("json");
        Ok(self.call(reqwest::Method::POST, path, Some(b.into()), "application/json").await?.0)
    }

    /// A body of bytes, of a type (a SQLite file to `/v1/import`).
    pub async fn post_bytes(&self, path: &str, bytes: Vec<u8>, ctype: &str) -> Result<Value> {
        Ok(self.call(reqwest::Method::POST, path, Some(bytes.into()), ctype).await?.0)
    }

    /// Any statement: `CREATE TABLE`, `ALTER TABLE`, a write, a `SELECT`.
    pub async fn sql(&self, sql: &str) -> Result<Value> {
        self.post("/v1/sql", &json!({ "sql": sql })).await
    }

    /// A `SELECT`, its rows as lists in the order of `columns`.
    pub async fn query(&self, sql: &str, params: Value) -> Result<Value> {
        self.post("/v1/query", &json!({ "sql": sql, "params": params })).await
    }

    /// Every row of a `SELECT`, read a page at a time past the window, as
    /// lists; `f` gets each page's rows and the columns. Stops after `max`
    /// rows (the rest are left unread, and the scroll closed).
    pub async fn scroll(&self, sql: &str, params: Value, max: usize, mut f: impl FnMut(&[String], Vec<Value>)) -> Result<usize> {
        let mut j = self.post("/v1/query", &json!({ "sql": sql, "params": params, "page_size": PAGE.min(max.max(1)) })).await?;
        let columns: Vec<String> = j["columns"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
        let mut n = 0;
        loop {
            let rows = match j["rows"].take() {
                Value::Array(a) => a,
                _ => Vec::new(),
            };
            n += rows.len();
            f(&columns, rows);
            let Some(cursor) = j["cursor"].as_str().map(str::to_string) else { break };
            if n >= max {
                let _ = self.call(reqwest::Method::DELETE, &format!("/v1/query/cursors/{cursor}"), None, "").await;
                break;
            }
            j = self.post("/v1/query", &json!({ "cursor": cursor })).await?;
        }
        Ok(n)
    }

    pub async fn health(&self) -> Result<Value> {
        self.get("/v1/health").await
    }

    /// Every table's listing (`/v1/tables`).
    pub async fn tables(&self) -> Result<Vec<Value>> {
        let j = self.get("/v1/tables").await?;
        Ok(j["tables"].as_array().cloned().unwrap_or_default())
    }

    /// Sends `ops` to `/v1/write` in one transaction under `key`: a 429 waits
    /// as the server says, a partial write (a coordinator whose shards took
    /// part of it) goes again with its key, a failure of the network or the
    /// server goes again after 0.25, 0.5, 1 ... s, each for up to `patience`.
    /// The rows written, as the server counts them.
    pub async fn write(&self, ops: Vec<Value>, key: &str, patience: Duration) -> Result<usize> {
        let n = ops.len();
        let mut body = json!({ "ops": ops, "idempotency_key": key });
        let started = std::time::Instant::now();
        let mut backoff = Duration::from_millis(250);
        loop {
            match self.post("/v1/write", &body).await {
                Ok(j) => return Ok(j["written"].as_u64().map(|n| n as usize).unwrap_or(n)),
                Err(e) if e.status == 429 => {
                    let wait = e.body.as_ref().and_then(|b| b["retry_after_s"].as_f64()).map(Duration::from_secs_f64).unwrap_or(Duration::from_secs(1));
                    if started.elapsed() > patience {
                        return Err(e);
                    }
                    tokio::time::sleep(wait).await;
                }
                Err(e) if e.body.as_ref().is_some_and(|b| b["partial"] == true) => {
                    if let Some(k) = e.body.as_ref().and_then(|b| b["idempotency_key"].as_str()) {
                        body["idempotency_key"] = json!(k);
                    }
                    if started.elapsed() > patience {
                        return Err(e);
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(8));
                }
                Err(e) if e.status == 0 || e.status >= 500 || e.status == 409 || e.status == 408 => {
                    if started.elapsed() > patience {
                        return Err(e);
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(8));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// A string as a SQL literal.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A name as a quoted SQL identifier.
pub fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
