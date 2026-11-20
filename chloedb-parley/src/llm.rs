//! Calls to Claude over the Messages API (raw HTTP: there is no Rust SDK),
//! priced from their usage, under a cap on a day's spend (Python's `llm.py`).
//!
//! Every call's cost is written to `turns` by its caller, and the day's spend
//! is a SUM over that table, so the cap holds across restarts and every copy
//! of the app that shares the database. The day is a UTC day.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::db::Db;
use crate::util::{iso, new_id, now, round, Error, Result};

pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";

/// Dollars per million tokens: (input, output). Cache writes cost 1.25 times
/// input, cache reads a tenth of it.
pub fn prices(model: &str) -> (f64, f64) {
    match model {
        "claude-haiku-5-5" => (0.10, 0.50),
        "claude-opus-5-5" => (4.00, 20.00),
        _ => (2.00, 10.00), // claude-sonnet-5-5, and anything unknown: assume dearer
    }
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub model: String,
    pub tok_in: i64,
    pub tok_out: i64,
    pub tok_cached: i64,
    pub tok_cache_write: i64,
    pub calls: i64,
    pub cost: f64,
}

impl Usage {
    pub fn new(model: &str) -> Usage {
        Usage { model: model.to_string(), ..Default::default() }
    }
    pub fn add(&mut self, o: &Usage) {
        self.tok_in += o.tok_in;
        self.tok_out += o.tok_out;
        self.tok_cached += o.tok_cached;
        self.tok_cache_write += o.tok_cache_write;
        self.calls += o.calls;
        self.cost += o.cost;
    }
}

/// A response's usage in tokens and dollars.
pub fn price(model: &str, usage: &Value) -> Usage {
    let (pin, pout) = prices(model);
    let n = |k: &str| usage[k].as_i64().unwrap_or(0);
    let (tin, tout, cr, cw) = (n("input_tokens"), n("output_tokens"), n("cache_read_input_tokens"), n("cache_creation_input_tokens"));
    let cost = (tin as f64 * pin + cw as f64 * pin * 1.25 + cr as f64 * pin * 0.1 + tout as f64 * pout) / 1e6;
    Usage { model: model.to_string(), tok_in: tin + cr + cw, tok_out: tout, tok_cached: cr, tok_cache_write: cw, calls: 1, cost }
}

pub fn day_start(t: Option<f64>) -> String {
    let t = t.unwrap_or_else(now);
    format!("{}T00:00:00Z", &iso(t)[..10])
}

/// The text of a response's text blocks.
pub fn text_of(resp: &Value) -> String {
    resp["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect()
}

pub struct Llm {
    pub model: String,
    pub daily_usd: f64,
    http: reqwest::Client,
    base: String,
    key: Option<String>,
    token: Option<String>,
    state: Mutex<(f64, Option<(f64, f64, String)>)>, // (spent by calls not written yet, (when read, spent, day))
}

impl Llm {
    pub fn available() -> bool {
        std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.is_empty()) || std::env::var("ANTHROPIC_AUTH_TOKEN").is_ok_and(|k| !k.is_empty())
    }

    pub fn new(model: &str, daily_usd: f64) -> Llm {
        Llm {
            model: model.to_string(), daily_usd,
            http: reqwest::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client"),
            base: std::env::var("ANTHROPIC_BASE_URL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "https://api.anthropic.com".into()).trim_end_matches('/').to_string(),
            key: std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()),
            token: std::env::var("ANTHROPIC_AUTH_TOKEN").ok().filter(|s| !s.is_empty()),
            state: Mutex::new((0.0, None)),
        }
    }

    pub async fn spent_today(&self, db: &Db, fresh: bool) -> f64 {
        let day = day_start(None);
        {
            let st = self.state.lock().unwrap();
            if let Some((t, v, d)) = &st.1 {
                if !fresh && *d == day && now() - t < 5.0 {
                    return v + st.0;
                }
            }
        }
        let v = db.scalar("spend", "SELECT SUM(cost) FROM turns WHERE ts >= :d", json!({"d": day})).await.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut st = self.state.lock().unwrap();
        st.1 = Some((now(), v, day));
        v + st.0
    }

    pub async fn left_today(&self, db: &Db) -> f64 {
        (self.daily_usd - self.spent_today(db, false).await).max(0.0)
    }

    pub async fn check(&self, db: &Db, reserve: f64) -> Result<()> {
        if self.spent_today(db, false).await + reserve >= self.daily_usd {
            return Err(Error::new(429, format!("today's model budget (${:.2}) is spent; it resets at 00:00 UTC", self.daily_usd)).budget());
        }
        Ok(())
    }

    /// messages.create with the budget checked first: (the response, its Usage).
    pub async fn create(&self, db: &Db, mut body: Value, reserve: f64) -> Result<(Value, Usage)> {
        self.check(db, reserve).await?;
        if body.get("model").is_none() {
            body["model"] = json!(self.model);
        }
        let model = body["model"].as_str().unwrap_or(&self.model).to_string();
        let mut wait = 0.5;
        let mut last = String::new();
        for attempt in 0..4 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                wait = (wait * 2.0).min(8.0);
            }
            let mut req = self.http.post(format!("{}/v1/messages", self.base)).header("anthropic-version", "2023-06-01").json(&body);
            if let Some(k) = &self.key {
                req = req.header("x-api-key", k);
            } else if let Some(t) = &self.token {
                req = req.bearer_auth(t);
            }
            let r = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    last = format!("APIConnectionError: {e}");
                    continue;
                }
            };
            let status = r.status().as_u16();
            if let Some(ra) = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<f64>().ok()) {
                wait = ra.clamp(0.5, 30.0);
            }
            let j: Value = r.json().await.unwrap_or(Value::Null);
            if (200..300).contains(&status) {
                let u = price(&model, &j["usage"]);
                self.state.lock().unwrap().0 += u.cost;
                return Ok((j, u));
            }
            last = format!("APIStatusError {status}: {}", j["error"]["message"].as_str().unwrap_or(""));
            if !(status == 408 || status == 409 || status == 429 || status >= 500) {
                return Err(Error::new(502, last));
            }
        }
        Err(Error::new(502, last))
    }

    /// One row of `turns`: what a turn, a persona or a check cost.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(&self, db: &Db, kind: &str, usage: &Usage, agent: Option<&str>, ok: bool, actions: i64, summary: &str) {
        let doc = json!({"id": new_id("t"), "agent": agent, "kind": kind, "model": usage.model, "cost": round(usage.cost, 6),
                         "tok_in": usage.tok_in, "tok_out": usage.tok_out, "tok_cached": usage.tok_cached, "calls": usage.calls,
                         "actions": actions, "ok": ok, "summary": summary.chars().take(500).collect::<String>()});
        let _ = db.write("turns", vec![json!({"op": "insert", "table": "turns", "doc": doc})]).await;
        let mut st = self.state.lock().unwrap();
        st.1 = None;
        st.0 = (st.0 - usage.cost).max(0.0);
    }
}
