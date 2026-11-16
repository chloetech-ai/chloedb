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
        let mut q = json!({"what": what, "sql": sql, "rows": rows, "ms": (ms * 100.0).round() / 100.0});
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
    /// Tables of their own: `netops_x` is sent as `netops_<tag>_x`.
    tag: Option<String>,
}

impl Strata {
    pub fn new(url: &str, key: Option<String>) -> Strata {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client");
        Strata { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), http, tag: None }
    }

    /// The same server, with tables of its own: every `netops_` table the
    /// statements and writes sent through it name is `netops_<tag>_` (a
    /// second network beside the first, `--topology both`). The app's code
    /// keeps its names; what it sends, and the SQL its pages show, carry the
    /// real ones.
    pub fn with_tables(mut self, tag: &str) -> Strata {
        self.tag = Some(tag.to_string());
        self
    }

    /// The name a table has on the server.
    pub fn table(&self, name: &str) -> String {
        self.renamed(name).into_owned()
    }

    /// `sql` with each `netops_` table renamed (one already renamed is left).
    pub fn renamed<'a>(&self, sql: &'a str) -> std::borrow::Cow<'a, str> {
        let Some(tag) = &self.tag else { return std::borrow::Cow::Borrowed(sql) };
        if !sql.contains("netops_") {
            return std::borrow::Cow::Borrowed(sql);
        }
        let done = format!("{tag}_");
        let mut out = String::with_capacity(sql.len() + 32);
        let mut rest = sql;
        while let Some(at) = rest.find("netops_") {
            let (before, from) = rest.split_at(at + "netops_".len());
            out.push_str(before);
            // A name inside a longer word (`my_netops_x`) is not a table of the app's.
            let inside = at > 0 && rest[..at].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            if !inside && !from.starts_with(&done) {
                out.push_str(&done);
            }
            rest = from;
        }
        out.push_str(rest);
        std::borrow::Cow::Owned(out)
    }

    /// A request's body with its tables renamed.
    fn body_for(&self, path: &str, body: &Value) -> Option<Value> {
        self.tag.as_ref()?;
        let mut b = body.clone();
        match path {
            "/v1/query" | "/v1/sql" => {
                let sql = b["sql"].as_str()?.to_string();
                b["sql"] = json!(self.renamed(&sql));
            }
            "/v1/write" => {
                for op in b["ops"].as_array_mut()? {
                    if let Some(t) = op["table"].as_str().map(str::to_string) {
                        op["table"] = json!(self.renamed(&t));
                    }
                }
            }
            _ => return None,
        }
        Some(b)
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut req = self.http.request(method, format!("{}{}", self.url, path)).header("x-chloedb-client", "chloedb-netops");
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
        match self.body_for(path, body) {
            Some(b) => self.call(reqwest::Method::POST, path, Some(&b)).await,
            None => self.call(reqwest::Method::POST, path, Some(body)).await,
        }
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

    /// As [`Strata::query`], kept in `trace` under the name `what`.
    pub async fn traced(&self, trace: &Trace, what: &str, sql: &str, params: Value) -> Result<Vec<Row>> {
        let t0 = Instant::now();
        let r = self.query(sql, params.clone()).await;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let sql = self.renamed(sql);
        match &r {
            Ok(rows) => trace.push(what, &sql, &params, rows.len(), ms, None),
            Err(e) => trace.push(what, &sql, &params, 0, ms, Some(&format!("refused: {}", e.message))),
        }
        r
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

    pub async fn upsert(&self, table: &str, rows: Vec<Value>) -> Result<usize> {
        self.write(rows.into_iter().map(|doc| json!({"op": "upsert", "table": table, "doc": doc})).collect()).await
    }
}

fn rows_of(j: Value) -> Vec<Row> {
    match j {
        Value::Object(mut o) => match o.remove("rows") {
            Some(Value::Array(a)) => a.into_iter().filter_map(|r| if let Value::Object(m) = r { Some(m) } else { None }).collect(),
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
            Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect(),
            Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_network_has_tables_of_its_own() {
        let db = Strata::new("http://x", None).with_tables("disc");
        assert_eq!(db.renamed("SELECT a.id FROM netops_links a JOIN netops_devices d ON d.id = a.src"), "SELECT a.id FROM netops_disc_links a JOIN netops_disc_devices d ON d.id = a.src");
        assert_eq!(db.renamed("FROM netops_disc_links"), "FROM netops_disc_links", "renamed once");
        assert_eq!(db.renamed("netops_discovered"), "netops_disc_discovered");
        assert_eq!(db.renamed("SELECT 'my_netops_x'"), "SELECT 'my_netops_x'");
        assert_eq!(Strata::new("http://x", None).renamed("FROM netops_links"), "FROM netops_links");
        let b = db.body_for("/v1/write", &json!({"ops": [{"op": "upsert", "table": "netops_state", "doc": {"val": "netops_x"}}]})).unwrap();
        assert_eq!((b["ops"][0]["table"].as_str(), b["ops"][0]["doc"]["val"].as_str()), (Some("netops_disc_state"), Some("netops_x")), "rows are not rewritten");
    }
}
