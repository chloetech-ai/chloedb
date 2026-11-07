//! A client of strata-server's HTTP API: SQL, writes (batched, sent again
//! on 429 and on failures, under an idempotency key), the cluster's shape,
//! and the admin calls that grow it.

use std::time::Duration;

use reqwest::StatusCode;
use serde_json::{json, Value};

#[derive(Debug)]
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

/// What a batch of writes came to.
#[derive(Debug, Default)]
pub struct Written {
    pub rows: usize,
    /// Rows the server refused, and why (each sent alone to find them).
    pub refused: Vec<(String, String)>,
    /// 429 answers waited out.
    pub busy: u32,
    /// Sends again after a failure.
    pub retries: u32,
}

impl Strata {
    pub fn new(url: &str, key: Option<String>) -> Strata {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client");
        Strata { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), http }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<(Value, Option<Duration>)> {
        let mut req = self.http.request(method, format!("{}{}", self.url, path)).header("x-strata-client", "strata-crawl");
        if let Some(k) = &self.key {
            req = req.bearer_auth(k);
        }
        if let Some(b) = body {
            req = req.json(b);
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
        Ok(self.call(reqwest::Method::GET, path, None).await?.0)
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        Ok(self.call(reqwest::Method::POST, path, Some(body)).await?.0)
    }

    pub async fn sql(&self, sql: &str) -> Result<Value> {
        self.post("/v1/sql", &json!({ "sql": sql })).await
    }

    pub async fn query(&self, sql: &str, params: Value) -> Result<Value> {
        self.post("/v1/query", &json!({ "sql": sql, "params": params, "objects": true })).await
    }

    /// Whether the server answers, and what it is (`role`).
    pub async fn health(&self) -> Result<Value> {
        self.get("/v1/health").await
    }

    /// The table's fields as `/v1/tables` gives them (`short.fields`), if
    /// the table is there.
    pub async fn table(&self, table: &str) -> Result<Option<Vec<Value>>> {
        let j = self.get("/v1/tables").await?;
        Ok(j["tables"].as_array().into_iter().flatten().find(|t| t["table"] == table).map(|t| t["short"]["fields"].as_array().cloned().unwrap_or_default()))
    }

    /// Upserts rows into `table`, in one transaction under `key` (an
    /// idempotency key, so a batch sent again after a lost answer applies
    /// once). A 429 waits as the server says and goes again; a failure of
    /// the network or the server goes again after 0.5, 1, 2, 4 ... s for
    /// up to `patience`; a 4xx sends the rows in halves to find the ones
    /// refused, which are dropped.
    pub async fn upsert(&self, table: &str, rows: Vec<serde_json::Map<String, Value>>, key: &str, patience: Duration) -> Result<Written> {
        let mut out = Written::default();
        self.upsert_part(table, rows, key.to_string(), patience, &mut out).await?;
        Ok(out)
    }

    fn upsert_part<'a>(
        &'a self,
        table: &'a str,
        rows: Vec<serde_json::Map<String, Value>>,
        key: String,
        patience: Duration,
        out: &'a mut Written,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            if rows.is_empty() {
                return Ok(());
            }
            let ops: Vec<Value> = rows.iter().map(|r| json!({"op": "upsert", "table": table, "doc": r})).collect();
            match self.write(ops, &key, patience, out).await {
                Ok(n) => {
                    out.rows += n.unwrap_or(rows.len());
                    Ok(())
                }
                Err(e) if e.status == 0 || e.status == 401 || e.status == 403 || e.status == 404 || e.status == 408 || e.status == 409 || e.status == 429 || e.status >= 500 || e.body.as_ref().is_some_and(|b| b["partial"] == true) => Err(e),
                Err(e) => {
                    // Refused: find the rows that are, in halves.
                    if rows.len() == 1 {
                        let url = rows[0].get("url").and_then(|u| u.as_str()).unwrap_or("?").to_string();
                        out.refused.push((url, e.message));
                        return Ok(());
                    }
                    let mut rows = rows;
                    let right = rows.split_off(rows.len() / 2);
                    self.upsert_part(table, rows, format!("{key}-a"), patience, out).await?;
                    self.upsert_part(table, right, format!("{key}-b"), patience, out).await
                }
            }
        })
    }

    /// Sends `ops` to `/v1/write` in one transaction under `key`, as
    /// [`Strata::upsert`] does: a 429 waits as the server says, a partial
    /// write goes again with its key, a failure of the network or the
    /// server goes again after 0.5, 1, 2, 4 ... s, each for up to
    /// `patience`. The rows written, as the server counts them.
    pub async fn write(&self, ops: Vec<Value>, key: &str, patience: Duration, out: &mut Written) -> Result<Option<usize>> {
        let mut body = json!({ "ops": ops, "idempotency_key": key });
        let started = std::time::Instant::now();
        let mut backoff = Duration::from_millis(500);
        loop {
            match self.call(reqwest::Method::POST, "/v1/write", Some(&body)).await {
                Ok((j, _)) => return Ok(j["written"].as_u64().map(|n| n as usize)),
                Err(e) if e.status == 429 => {
                    out.busy += 1;
                    let wait = e.body.as_ref().and_then(|b| b["retry_after_s"].as_f64()).map(Duration::from_secs_f64).unwrap_or(Duration::from_secs(1));
                    if started.elapsed() > patience {
                        return Err(e);
                    }
                    tokio::time::sleep(wait).await;
                }
                // A cluster that took part of it: the same key again applies the rest.
                Err(e) if e.body.as_ref().is_some_and(|b| b["partial"] == true) => {
                    out.retries += 1;
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
                    out.retries += 1;
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

    /// The cluster as a coordinator sees it (`/v1/cluster`), or `None`
    /// from a server that is not one.
    pub async fn cluster(&self) -> Result<Option<Value>> {
        let j = self.get("/v1/cluster").await?;
        Ok((j["role"] == "coordinator").then_some(j))
    }
}

/// Rows of `table` on each shard of a cluster (`/v1/cluster`), in shard
/// order, with each shard's slots: the most any of its copies reports.
pub fn shard_rows(cluster: &Value, table: &str) -> Vec<ShardRows> {
    cluster["shards"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let reps = s["replicas"].as_array().cloned().unwrap_or_default();
            let rows = reps.iter().filter_map(|r| r["detail"]["health"]["tables"][table].as_u64()).max().unwrap_or(0);
            let all = reps.iter().filter_map(|r| r["detail"]["health"]["rows"].as_u64().or(r["docs"].as_u64())).max().unwrap_or(0);
            ShardRows {
                shard: s["shard"].as_u64().unwrap_or(0) as usize,
                rows,
                all_rows: all,
                slots: s["slots_held"].as_u64().unwrap_or(0),
                servers: reps.iter().filter_map(|r| r["url"].as_str().map(str::to_string)).collect(),
                up: reps.iter().any(|r| r["status"] == "ok"),
            }
        })
        .collect()
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ShardRows {
    pub shard: usize,
    /// Rows of the crawl's table.
    pub rows: u64,
    /// Rows of every table.
    pub all_rows: u64,
    pub slots: u64,
    pub servers: Vec<String>,
    pub up: bool,
}
