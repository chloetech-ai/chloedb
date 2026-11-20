//! Errors, ids, times and names.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::Rng;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// An error with the HTTP status it answers with (Python's ParleyError).
#[derive(Debug, Clone)]
pub struct Error {
    pub status: u16,
    pub message: String,
    pub answer: Option<Value>,
}

impl Error {
    pub fn new(status: u16, message: impl Into<String>) -> Error {
        Error { status, message: message.into(), answer: None }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(json!({"error": self.message, "queries": crate::db::trace()}))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn err<T>(status: u16, message: impl Into<String>) -> Result<T> {
    Err(Error::new(status, message))
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Sortable by time (microseconds, hex), unique by the random tail.
pub fn new_id(prefix: &str) -> String {
    let us = (now() * 1e6) as u64;
    let tail: [u8; 3] = rand::thread_rng().gen();
    format!("{prefix}{us:014x}{}", hex::encode(tail))
}

pub fn key_hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

pub fn sha256_hex(s: &str) -> String {
    key_hash(s)
}

/// A UTC time as ISO 8601, to the second.
pub fn iso(t: f64) -> String {
    chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

/// Seconds since the epoch of a row's time (text, or micro/seconds).
pub fn epoch(v: &Value) -> f64 {
    match v {
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(0.0);
            if x > 1e12 { x / 1e6 } else { x }
        }
        Value::String(s) => epoch_str(s),
        _ => 0.0,
    }
}

pub fn epoch_str(s: &str) -> f64 {
    let s = s.trim();
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(&s.replace(' ', "T")) {
        return d.timestamp() as f64 + f64::from(d.timestamp_subsec_micros()) / 1e6;
    }
    if let Ok(d) = chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S%.f") {
        return d.and_utc().timestamp() as f64 + f64::from(d.and_utc().timestamp_subsec_micros()) / 1e6;
    }
    if let Ok(d) = chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%d %H:%M:%S%.f") {
        return d.and_utc().timestamp() as f64;
    }
    0.0
}

fn rx(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("a regex"))
}

/// A community's or a tag's name: lower case, a-z, 0-9 and _.
pub fn slug(name: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let s = name.trim().to_lowercase();
    let s = s.strip_prefix("r/").or_else(|| s.strip_prefix("c/")).unwrap_or(&s);
    let s = s.trim_start_matches('#');
    rx(&R, "[^a-z0-9_]").replace_all(&s.replace('-', "_").replace(' ', "_"), "").into_owned()
}

pub fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    rx(cell, pat)
}

/// Round to `d` decimals.
pub fn round(x: f64, d: i32) -> f64 {
    let m = 10f64.powi(d);
    (x * m).round() / m
}

pub fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
