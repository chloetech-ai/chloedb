//! Trends (Python's `trends.py`): how much Parley talks about things, bucket
//! by bucket, each term one `GROUP BY date_bin(...)` over the posts and
//! comments that match it, divided by all of them in the same bucket; and the
//! words that rose.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Value};

use crate::app::App;
use crate::db::{Row, RowExt};
use crate::util::{epoch, err, iso, now, re, round, Result};

pub const STOP_WORDS: &str = "a an the and or but if of to in on at by for with from as is are was were be been being it its this that these those
i me my we our you your he she they them their his her not no so do does did done have has had having can could would should will
just very really also than then there here what which who whom whose when where why how all any each every some more most other
into over under about after before again out up down off only own same too s t don now one two new like get got make made
let lets still even much many well way back yes ok thanks thank re via";

pub fn stop() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| STOP_WORDS.split_whitespace().collect())
}

const WIDTHS: [(i64, &str); 6] = [(300, "5 minutes"), (900, "15 minutes"), (1800, "30 minutes"), (3600, "1 hour"), (6 * 3600, "6 hours"), (86400, "1 day")];

fn buckets(width_s: i64, n: usize, t: f64) -> Vec<f64> {
    let end = (t / width_s as f64).floor() * width_s as f64;
    (0..n).map(|i| end - (n - 1 - i) as f64 * width_s as f64).collect()
}

fn bucket(t: f64, width_s: i64) -> i64 {
    (t / width_s as f64).floor() as i64
}

impl App {
    async fn trend_count(&self, table: &str, since: &str, width: &str, term: Option<&str>, kind: bool) -> Result<Vec<Row>> {
        let mut wh = "ts >= :s".to_string();
        if table == "posts" {
            wh.push_str(" AND mod = 'ok'");
        }
        let mut params = json!({"s": since});
        if let Some(t) = term {
            wh.push_str(" AND MATCH(body, :t, fuzzy => 'auto')");
            params["t"] = json!(t);
        }
        let (sel, group) = if kind { (", kind", "b, kind") } else { ("", "b") };
        let label = format!("trends:{table}{}{}", term.map(|t| format!(":{t}")).unwrap_or_default(), if kind { ":kind" } else { "" });
        self.db.q(&label, &format!("SELECT date_bin('{width}', ts) AS b{sel}, COUNT(*) AS n FROM {table} WHERE {wh} GROUP BY {group}"), params).await
    }

    pub async fn trends(&self, terms: &[String], by: &str) -> Result<Value> {
        if !["auto", "hour", "day"].contains(&by) {
            return err(400, "by is auto, hour or day");
        }
        let terms: Vec<String> = terms.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).take(6).collect();
        let t_now = now();
        let (width_s, width, n): (i64, &str, usize) = match by {
            "hour" => (3600, "1 hour", 48),
            "day" => (86400, "1 day", 30),
            _ => {
                let first = self.db.one("trends:first", "SELECT ts FROM posts ORDER BY ts ASC LIMIT 1", json!({})).await?;
                let span = first.map(|f| (t_now - epoch(&f["ts"])).max(300.0)).unwrap_or(3600.0);
                let (w, name) = WIDTHS.iter().copied().find(|(w, _)| span / *w as f64 <= 47.0).unwrap_or(WIDTHS[5]);
                (w, name, ((span / w as f64).ceil() as usize + 1).clamp(6, 48))
            }
        };
        let starts = buckets(width_s, n, t_now);
        let since = iso(starts[0]);
        let t0 = std::time::Instant::now();
        let series = |rows: &[Row]| -> Vec<i64> {
            let mut got: HashMap<i64, i64> = HashMap::new();
            for r in rows {
                *got.entry(bucket(epoch(&r["b"]), width_s)).or_insert(0) += r.i("n");
            }
            starts.iter().map(|s| got.get(&bucket(*s, width_s)).copied().unwrap_or(0)).collect()
        };
        let tp = series(&self.trend_count("posts", &since, width, None, false).await?);
        let tc = series(&self.trend_count("comments", &since, width, None, false).await?);
        let totals: Vec<i64> = tp.iter().zip(&tc).map(|(a, b)| a + b).collect();
        let mut out_terms = Vec::new();
        for t in &terms {
            let p = series(&self.trend_count("posts", &since, width, Some(t), false).await?);
            let c = series(&self.trend_count("comments", &since, width, Some(t), false).await?);
            let counts: Vec<i64> = p.iter().zip(&c).map(|(a, b)| a + b).collect();
            let share: Vec<f64> = counts.iter().zip(&totals).map(|(x, tot)| if *tot > 0 { round(*x as f64 / *tot as f64, 4) } else { 0.0 }).collect();
            out_terms.push(json!({"term": t, "counts": counts, "posts": p.iter().sum::<i64>(), "comments": c.iter().sum::<i64>(), "share": share}));
        }
        let idx: HashMap<i64, usize> = starts.iter().enumerate().map(|(j, s)| (bucket(*s, width_s), j)).collect();
        let mut kinds: IndexMap<String, Value> = IndexMap::new();
        for r in self.trend_count("posts", &since, width, None, true).await? {
            let k = if r.s("kind").is_empty() { "text".to_string() } else { r.s("kind") };
            let v = kinds.entry(k).or_insert_with(|| json!(vec![0i64; n]));
            if let Some(j) = idx.get(&bucket(epoch(&r["b"]), width_s)) {
                v[*j] = json!(v[*j].as_i64().unwrap_or(0) + r.i("n"));
            }
        }
        let span_h = (t_now - starts[0]) / 3600.0;
        let recent = (span_h / 3.0).clamp(0.5, 6.0);
        Ok(json!({"by": by, "width": width, "width_s": width_s, "buckets": starts.iter().map(|s| iso(*s)).collect::<Vec<_>>(), "totals": totals,
                  "terms": out_terms, "kinds": kinds.into_iter().collect::<serde_json::Map<String, Value>>(), "rising": self.rising_words(recent, (span_h - recent).clamp(0.5, 42.0), 16).await?,
                  "recent_hours": round(recent, 2), "took_ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1)}))
    }

    /// Words used more in the last hours than in the hours before, by the
    /// ratio of their rates (smoothed), with at least a few uses.
    pub async fn rising_words(&self, recent_hours: f64, before_hours: f64, top: usize) -> Result<Vec<Value>> {
        static WORD: OnceLock<Regex> = OnceLock::new();
        let t_now = now();
        let since = iso(t_now - (recent_hours + before_hours) * 3600.0);
        let cut = t_now - recent_hours * 3600.0;
        let mut rows = self.db.q("trends:rising:posts", "SELECT body, ts FROM posts WHERE ts >= :s AND mod = 'ok' ORDER BY ts DESC LIMIT 5000", json!({"s": since})).await?;
        rows.extend(self.db.q("trends:rising:comments", "SELECT body, ts FROM comments WHERE ts >= :s ORDER BY ts DESC LIMIT 5000", json!({"s": since})).await?);
        let mut new: IndexMap<String, i64> = IndexMap::new();
        let mut old: HashMap<String, i64> = HashMap::new();
        let (mut n_new, mut n_old) = (0i64, 0i64);
        let word = re(&WORD, r"[a-z][a-z0-9'-]{2,}");
        for r in &rows {
            let body = r.s("body").to_lowercase();
            let mut words: indexmap::IndexSet<String> = indexmap::IndexSet::new();
            for m in word.find_iter(&body) {
                let w = m.as_str().trim_matches(|c| c == '\'' || c == '-');
                if w.chars().count() > 2 && !stop().contains(w) && !w.chars().all(|c| c.is_ascii_digit()) {
                    words.insert(w.to_string());
                }
            }
            let recent = epoch(&r["ts"]) >= cut;
            for w in words {
                if recent {
                    *new.entry(w).or_insert(0) += 1;
                } else {
                    *old.entry(w).or_insert(0) += 1;
                }
            }
            if recent { n_new += 1 } else { n_old += 1 }
        }
        let mut out: Vec<(f64, i64, Value)> = Vec::new();
        for (w, k) in &new {
            let o = old.get(w).copied().unwrap_or(0);
            if *k < 3 || o * n_new.max(1) >= k * n_old.max(1) {
                continue;
            }
            let ratio = ((*k as f64 + 0.5) / n_new.max(1) as f64) / ((o as f64 + 0.5) / n_old.max(1) as f64);
            if ratio < 1.2 {
                continue;
            }
            let r2 = round(ratio, 2);
            out.push((r2, *k, json!({"term": w, "recent": k, "before": o, "ratio": r2})));
        }
        out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.cmp(&a.1)));
        Ok(out.into_iter().take(top).map(|x| x.2).collect())
    }
}
