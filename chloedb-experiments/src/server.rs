//! A client of chloedb-server over HTTP: statements, queries (logged ones
//! included), writes, a scroll past the window and the rank log's export.

use std::time::Duration;

use serde_json::{json, Value};

#[derive(Clone)]
pub struct Server {
    pub url: String,
    key: Option<String>,
    http: reqwest::Client,
}

/// Rows a scroll reads a page at a time; the server's window is 10,000.
pub const PAGE: usize = 10_000;

pub type Result<T> = std::result::Result<T, String>;

impl Server {
    pub fn new(url: &str, key: Option<String>) -> Server {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(600)).connect_timeout(Duration::from_secs(10)).pool_max_idle_per_host(64).build().expect("an HTTP client");
        Server { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), http }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<String> {
        let mut req = self.http.request(method, format!("{}{}", self.url, path)).header("x-chloedb-client", "chloedb-experiments");
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let mut backoff = Duration::from_millis(200);
        for attempt in 0.. {
            let r = match req.try_clone().expect("a body in memory").send().await {
                Ok(r) => r,
                Err(e) if attempt < 6 && (e.is_connect() || e.is_timeout()) => {
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                    continue;
                }
                Err(e) => return Err(format!("{}{path}: {e}", self.url)),
            };
            let status = r.status();
            let text = r.text().await.map_err(|e| format!("{}{path}: {e}", self.url))?;
            if status.is_success() {
                return Ok(text);
            }
            if (status.as_u16() == 429 || status.as_u16() == 503) && attempt < 20 {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
            let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let message = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(400).collect());
            return Err(format!("{path} ({status}): {message}"));
        }
        unreachable!()
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let t = self.call(reqwest::Method::POST, path, Some(body)).await?;
        serde_json::from_str(&t).map_err(|e| format!("{path}: {e}"))
    }

    pub async fn health(&self) -> Result<Value> {
        let t = self.call(reqwest::Method::GET, "/v1/health", None).await?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    }

    /// Any statement.
    pub async fn sql(&self, sql: &str, params: Value) -> Result<Value> {
        self.post("/v1/sql", &json!({"sql": sql, "params": params})).await
    }

    /// A `SELECT`, logged when `log` is given (`/v1/query`'s `"log"`).
    pub async fn query(&self, sql: &str, params: Value, log: Option<Value>) -> Result<Value> {
        let mut body = json!({"sql": sql, "params": params});
        if let Some(l) = log {
            body["log"] = l;
        }
        self.post("/v1/query", &body).await
    }

    /// `ops` (each `{"op", "table", "doc"}`) in one transaction.
    pub async fn write(&self, ops: Vec<Value>) -> Result<u64> {
        let n = ops.len() as u64;
        let j = self.post("/v1/write", &json!({"ops": ops})).await?;
        Ok(j["written"].as_u64().unwrap_or(n))
    }

    /// Every row of a `SELECT`, a page at a time: `f` gets each page.
    pub async fn scroll(&self, sql: &str, params: Value, mut f: impl FnMut(&[String], Vec<Value>)) -> Result<usize> {
        let mut j = self.post("/v1/query", &json!({"sql": sql, "params": params, "page_size": PAGE})).await?;
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
            j = self.post("/v1/query", &json!({"cursor": cursor})).await?;
        }
        Ok(n)
    }

    /// The rank log's export (`/v1/rank_log/export`), a record at a time,
    /// read as it streams.
    pub async fn export(&self, body: &Value, mut f: impl FnMut(Value)) -> Result<usize> {
        let mut req = self.http.post(format!("{}/v1/rank_log/export", self.url)).header("x-chloedb-client", "chloedb-experiments").json(body);
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        let mut r = req.send().await.map_err(|e| format!("/v1/rank_log/export: {e}"))?;
        if !r.status().is_success() {
            let s = r.status();
            return Err(format!("/v1/rank_log/export ({s}): {}", r.text().await.unwrap_or_default()));
        }
        let mut n = 0;
        let mut line = |l: &[u8], n: &mut usize| -> Result<()> {
            if l.iter().all(|b| b.is_ascii_whitespace()) {
                return Ok(());
            }
            let j: Value = serde_json::from_slice(l).map_err(|e| format!("the export: {e}"))?;
            if j.get("error").is_some() && j.get("request").is_none() {
                return Err(format!("the export: {}", j["error"]));
            }
            *n += 1;
            f(j);
            Ok(())
        };
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|e| format!("the export: {e}"))? {
            buf.extend_from_slice(&chunk);
            let mut start = 0;
            while let Some(p) = buf[start..].iter().position(|&b| b == b'\n') {
                line(&buf[start..start + p], &mut n)?;
                start += p + 1;
            }
            buf.drain(..start);
        }
        line(&buf, &mut n)?;
        Ok(n)
    }
}

/// A string as a SQL literal.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
