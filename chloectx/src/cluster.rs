//! A client of chloedb-server's HTTP API, for a store kept in a table of a
//! ChloeDB server or cluster ([`crate::store::Store::open_remote`]) and for
//! `chloectx app` ([`crate::app`]): SQL with parameters, writes, and the
//! server's own calls (alerts, events, the on-call agent).
//!
//! Values cross as chloedb-server's JSON ([`chloedb-server`'s `json.rs`]):
//! an array of numbers is a vector, any other array a list.

use std::time::Duration;

use chloedb::query::exec::ResultSet;
use chloedb::query::Params;
use chloedb::Value;
use serde_json::{json, Value as Json};

use crate::{err, Result};

/// Rows one query answers at most: the server's result window.
pub const WINDOW: usize = 10_000;

/// A chloedb-server (a coordinator, or a server on its own).
pub struct Cluster {
    pub url: String,
    key: Option<String>,
    agent: ureq::Agent,
}

/// An answer that is not a success: its HTTP status (0 when the server did
/// not answer) and message.
#[derive(Debug, Clone)]
pub struct Fail {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            0 => write!(f, "{}", self.message),
            s => write!(f, "HTTP {s}: {}", self.message),
        }
    }
}

impl From<Fail> for crate::Error {
    fn from(f: Fail) -> crate::Error {
        crate::Error(f.to_string())
    }
}

impl Cluster {
    pub fn new(url: &str, key: Option<String>) -> Cluster {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).timeout_connect(Some(Duration::from_secs(10))).http_status_as_error(false).build().into();
        Cluster { url: url.trim_end_matches('/').to_string(), key: key.filter(|k| !k.is_empty()), agent }
    }

    fn answer(&self, r: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> std::result::Result<Json, Fail> {
        let mut r = r.map_err(|e| Fail { status: 0, message: format!("{}: {e}", self.url) })?;
        let status = r.status().as_u16();
        let text = r.body_mut().with_config().limit(256 << 20).read_to_string().map_err(|e| Fail { status, message: format!("{}: {e}", self.url) })?;
        let j: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
        if (200..300).contains(&status) {
            return Ok(j);
        }
        let message = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(300).collect());
        Err(Fail { status, message })
    }

    /// `GET path` (with its query string).
    pub fn get(&self, path: &str) -> std::result::Result<Json, Fail> {
        let mut req = self.agent.get(format!("{}{path}", self.url)).header("x-chloedb-client", "chloectx");
        if let Some(k) = &self.key {
            req = req.header("authorization", format!("Bearer {k}"));
        }
        self.answer(req.call())
    }

    /// `POST path` with a JSON body.
    pub fn post(&self, path: &str, body: &Json) -> std::result::Result<Json, Fail> {
        let mut req = self.agent.post(format!("{}{path}", self.url)).header("x-chloedb-client", "chloectx").header("content-type", "application/json");
        if let Some(k) = &self.key {
            req = req.header("authorization", format!("Bearer {k}"));
        }
        self.answer(req.send(body.to_string()))
    }

    /// Runs a query (`POST /v1/query`) and reads its rows back as values.
    pub fn query(&self, sql: &str, params: &Params) -> Result<ResultSet> {
        let j = self.post("/v1/query", &json!({ "sql": sql, "params": params_json(params) }))?;
        Ok(result_set(&j))
    }

    /// A statement (`POST /v1/sql`): DDL, or a write.
    pub fn sql(&self, sql: &str) -> Result<Json> {
        Ok(self.post("/v1/sql", &json!({ "sql": sql }))?)
    }

    /// Writes `ops` (`POST /v1/write`) in one transaction.
    pub fn write(&self, ops: Vec<Json>) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        // Sent again after a refusal for load (429), up to a minute.
        let body = json!({ "ops": ops });
        let mut wait = Duration::from_millis(100);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            match self.post("/v1/write", &body) {
                Ok(_) => return Ok(()),
                Err(f) if f.status == 429 && std::time::Instant::now() < deadline => {
                    std::thread::sleep(wait);
                    wait = (wait * 2).min(Duration::from_secs(5));
                }
                Err(f) => return Err(f.into()),
            }
        }
    }

    /// The names of the tables the server holds.
    pub fn tables(&self) -> Result<Vec<String>> {
        let j = self.get("/v1/tables")?;
        Ok(j["tables"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().or(t["table"].as_str()).map(str::to_string)).collect())
    }
}

/// A value as chloedb-server's JSON.
pub fn value_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Int(x) => json!(x),
        Value::Float(x) if x.is_finite() => json!(x),
        Value::Float(_) => Json::Null,
        Value::Str(s) => json!(s),
        Value::Vector(v) => Json::Array(v.iter().map(|&x| json!(x as f64)).collect()),
        Value::List(xs) => Json::Array(xs.iter().map(value_json).collect()),
        Value::Bool(b) => json!(b),
        Value::Timestamp(t) => json!(chloedb::timestamp::format(*t)),
    }
}

fn params_json(p: &Params) -> Json {
    Json::Object(p.0.iter().map(|(k, v)| (k.clone(), value_json(v))).collect())
}

/// A JSON value of an answer as a value: an array of numbers is a vector
/// (whole numbers too, as a vector column answers them), others a list.
pub fn json_value(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(x) => Value::Int(x),
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(xs) if !xs.is_empty() && xs.iter().all(|x| x.is_number()) => Value::Vector(xs.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>().into()),
        Json::Array(xs) => Value::List(xs.iter().map(json_value).collect()),
        Json::Object(_) => Value::Str(j.to_string()),
    }
}

/// `/v1/query`'s answer as a result set (rows, keys and scores).
pub fn result_set(j: &Json) -> ResultSet {
    let strs = |k: &str| j[k].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect::<Vec<_>>();
    ResultSet {
        columns: strs("columns"),
        rows: j["rows"].as_array().into_iter().flatten().map(|r| r.as_array().into_iter().flatten().map(json_value).collect()).collect(),
        keys: j["keys"].as_array().into_iter().flatten().map(json_value).collect(),
        scores: j["scores"].as_array().into_iter().flatten().map(|s| s.as_f64().unwrap_or(0.0)).collect(),
        parts: None,
        stats: Default::default(),
    }
}

/// Whether `name` may name a space (and so, after `ctx_`, a table):
/// lowercase letters, digits and `_`, starting with a letter, at most 48
/// characters.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty() && b.len() <= 48 && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// A SQL string literal of `s`.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// An error unless `name` is a valid space name ([`valid_name`]).
pub fn check_name(name: &str) -> Result<()> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(err(format!("a space's name is lowercase letters, digits and _, starting with a letter (at most 48): {name:?}")))
    }
}
