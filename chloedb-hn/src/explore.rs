//! Exploring: a log of the page's searches that values rankings it never
//! served. When the site is started with `--explore-share ε`, each search
//! the page makes (the first page of stories, by the best match, with
//! words) is logged: the stories in the order the site ranked them, the
//! order shown, and the stories people opened from it. With chance ε the
//! top `--explore-top` stories are shown in a uniform random order instead
//! (the ε-greedy policy of ChloeDB's rank log, `{"policy": "epsilon"}`), so
//! every story of the top has a chance at every place, and the Experiments
//! app can weight what people opened by that chance to value another
//! ranking (`GET /v1/experiment/log`, behind the experiments token).
//!
//! The draw is seeded by the request's id. Nothing is logged for a search
//! made through the API (the Experiments app's agents included), only for
//! the page's, which asks with `log=1`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::strata::{Row, Strata};

pub const LOG: &str = "hn_explore_log";
pub const CLICKS: &str = "hn_explore_clicks";
/// Requests kept in memory, for their clicks to be checked against.
const RECENT: usize = 20_000;

pub struct Explore {
    /// ε: the share of logged searches whose top is shuffled.
    pub share: f64,
    pub top: usize,
    /// Recent requests: when, and the stories shown, in order.
    recent: Mutex<HashMap<String, (i64, Vec<i64>)>>,
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// A request's seed: its draws (exploring's, interleaving's) start here.
pub fn seed(request: &str) -> u64 {
    fnv(request)
}

/// A uniform number in [0, 1) from a state, which it moves.
pub fn uniform(z: &mut u64) -> f64 {
    *z = mix(*z);
    (*z >> 11) as f64 / (1u64 << 53) as f64
}

impl Explore {
    pub fn new(share: f64, top: usize) -> Explore {
        Explore { share: share.clamp(0.0, 1.0), top: top.clamp(2, 20), recent: Mutex::new(HashMap::new()) }
    }

    pub fn policy(&self) -> Value {
        json!({"policy": "epsilon", "epsilon": self.share, "top": self.top})
    }

    /// A new request's id.
    pub fn request(&self) -> String {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let n = self.recent.lock().unwrap_or_else(|e| e.into_inner()).len() as u64;
        format!("{:016x}", mix(t ^ mix(n) ^ std::process::id() as u64))
    }

    /// The order the request shows the first `n` ranked rows in, as their
    /// indexes, and whether it explored: with chance ε the top `top` in a
    /// uniform order (Fisher–Yates), the rest kept.
    pub fn draw(&self, request: &str, n: usize) -> (Vec<usize>, bool) {
        let mut order: Vec<usize> = (0..n).collect();
        let mut z = fnv(request);
        if uniform(&mut z) >= self.share {
            return (order, false);
        }
        let top = self.top.min(n);
        for i in (1..top).rev() {
            let j = (uniform(&mut z) * (i + 1) as f64) as usize;
            order.swap(i, j.min(i));
        }
        (order, true)
    }

    /// Logs a search: its stories as ranked (the first `top` and those
    /// shown), reordered as drawn. The stories come back in the order to
    /// show, and the log's row to write.
    pub fn serve(&self, request: &str, q: &str, mut stories: Vec<Row>, now: i64) -> (Vec<Row>, Value) {
        let n = stories.len().min(self.top);
        let ranked: Vec<i64> = stories.iter().take(n).map(|r| r.get("id").and_then(Value::as_i64).unwrap_or(0)).collect();
        let (order, explored) = self.draw(request, n);
        let head: Vec<Row> = order.iter().map(|&i| stories[i].clone()).collect();
        stories.splice(0..n, head);
        let shown: Vec<i64> = order.iter().map(|&i| ranked[i]).collect();
        self.remember(request, shown.clone(), now);
        let row = json!({
            "id": request, "ts": now, "q": q, "policy": self.policy().to_string(), "explored": explored as i64,
            "ranked": serde_json::to_string(&ranked).unwrap_or_default(), "shown": serde_json::to_string(&shown).unwrap_or_default(),
        });
        (stories, row)
    }

    /// Keeps a request served another way (interleaved), for its clicks.
    pub fn remember(&self, request: &str, shown: Vec<i64>, now: i64) {
        let mut r = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        if r.len() >= RECENT {
            let cut = now - 3 * 3600;
            r.retain(|_, v| v.0 >= cut);
            if r.len() >= RECENT {
                r.clear();
            }
        }
        r.insert(request.to_string(), (now, shown));
    }

    /// A click's row, if the request is a recent one and showed the story.
    pub fn click(&self, request: &str, story: i64, now: i64) -> Option<Value> {
        let r = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        let (_, shown) = r.get(request)?;
        let pos = shown.iter().position(|&s| s == story)?;
        Some(json!({"id": format!("{request}|{story}"), "request": request, "story": story, "pos": pos as i64, "ts": now}))
    }
}

pub async fn create_tables(db: &Strata) -> crate::strata::Result<()> {
    db.sql(&format!("CREATE TABLE IF NOT EXISTS {LOG} (id KEYWORD PRIMARY KEY, ts INT, q TEXT OPTIONS(indexed = false), policy KEYWORD OPTIONS(indexed = false), explored INT, ranked TEXT OPTIONS(indexed = false), shown TEXT OPTIONS(indexed = false))")).await?;
    db.sql(&format!("CREATE TABLE IF NOT EXISTS {CLICKS} (id KEYWORD PRIMARY KEY, request KEYWORD, story INT, pos INT, ts INT)")).await?;
    Ok(())
}

/// The log since `since` (seconds), newest first, with each request's
/// clicks: what `GET /v1/experiment/log` answers.
pub async fn read(db: &Strata, since: i64, limit: usize) -> crate::strata::Result<Value> {
    let reqs = db.query(&format!("SELECT id, ts, q, policy, explored, ranked, shown FROM {LOG} WHERE ts >= :s ORDER BY ts DESC LIMIT {limit}"), json!({"s": since})).await?;
    let clicks = db.query(&format!("SELECT request, story, ts FROM {CLICKS} WHERE ts >= :s ORDER BY ts DESC LIMIT 10000"), json!({"s": since})).await?;
    let mut by: HashMap<String, Vec<i64>> = HashMap::new();
    for c in &clicks {
        let r = c.get("request").and_then(Value::as_str).unwrap_or("").to_string();
        let s = c.get("story").and_then(Value::as_i64).unwrap_or(0);
        let v = by.entry(r).or_default();
        if !v.contains(&s) {
            v.push(s);
        }
    }
    let parse = |r: &Row, k: &str| -> Value { r.get(k).and_then(Value::as_str).and_then(|s| serde_json::from_str(s).ok()).unwrap_or(json!([])) };
    let out: Vec<Value> = reqs
        .iter()
        .map(|r| {
            let id = r.get("id").and_then(Value::as_str).unwrap_or("");
            json!({
                "request": id, "ts": r.get("ts"), "q": r.get("q"), "explored": r.get("explored").and_then(Value::as_i64) == Some(1),
                "policy": parse(r, "policy"), "ranked": parse(r, "ranked"), "shown": parse(r, "shown"), "clicks": by.get(id).cloned().unwrap_or_default(),
            })
        })
        .collect();
    Ok(json!({"requests": out, "clicks": clicks.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explores_about_its_share_and_keeps_the_rest() {
        let e = Explore::new(0.2, 5);
        let mut explored = 0;
        let mut at = vec![vec![0usize; 8]; 8];
        for i in 0..20_000 {
            let (o, x) = e.draw(&format!("r{i}"), 8);
            explored += x as usize;
            assert_eq!(&o[5..], &[5, 6, 7], "below the top, the ranking");
            for (k, &r) in o.iter().enumerate() {
                at[r][k] += 1;
            }
            if !x {
                assert_eq!(o, (0..8).collect::<Vec<_>>());
            }
        }
        let share = explored as f64 / 20_000.0;
        assert!((share - 0.2).abs() < 0.015, "{share}");
        // Row 0 at place 0: (1 − ε) + ε / top; at place 3: ε / top.
        let p00 = at[0][0] as f64 / 20_000.0;
        let p03 = at[0][3] as f64 / 20_000.0;
        assert!((p00 - 0.84).abs() < 0.015 && (p03 - 0.04).abs() < 0.01, "{p00} {p03}");
        // The same request draws the same order.
        assert_eq!(e.draw("again", 8), e.draw("again", 8));
    }

    #[test]
    fn serves_in_the_drawn_order_and_checks_clicks() {
        let e = Explore::new(1.0, 4);
        let rows: Vec<Row> = (1..=6).map(|i| json!({"id": i, "title": format!("t{i}")}).as_object().unwrap().clone()).collect();
        let (shown, log) = e.serve("req", "rust", rows, 100);
        let ids: Vec<i64> = shown.iter().map(|r| r["id"].as_i64().unwrap()).collect();
        assert_eq!(&ids[4..], &[5, 6]);
        let mut top = ids[..4].to_vec();
        top.sort();
        assert_eq!(top, vec![1, 2, 3, 4]);
        assert_eq!(log["ranked"], "[1,2,3,4]");
        assert_eq!(log["explored"], 1);
        let c = e.click("req", ids[2], 101).unwrap();
        assert_eq!(c["pos"], 2);
        assert!(e.click("req", 6, 101).is_none(), "a story past the logged top");
        assert!(e.click("nope", 1, 101).is_none());
    }
}
