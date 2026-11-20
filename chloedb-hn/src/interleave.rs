//! Interleaving on real visitors: a test of two rankings (the control and
//! a treatment, each a set of the Experiments app's knobs) on the page's
//! own searches. With chance `share`, a logged search (the first page of
//! stories by the best match, with words) shows the two arms' top 10
//! drafted into one list by team draft, each story marked with the arm
//! that picked it, and the stories opened from it are credited to that
//! arm. Each search compares the two rankings on the same person and the
//! same query, so far fewer searches tell them apart than an A/B would
//! need, which is what a site of a few hundred searches a day can afford.
//!
//! One test runs at a time. The Experiments app starts and stops it
//! (`/v1/experiment/interleave`, behind the experiments token) and reads
//! its log; the test is kept in a table, so a restart keeps running it.
//! The draft's coins and whether a search takes part are drawn from the
//! request's id, as exploring's are.

use std::sync::{Arc, RwLock};

use serde_json::{json, Value};

use crate::experiment::Knobs;
use crate::strata::{Row, Strata};

pub const TESTS: &str = "hn_interleave_tests";
pub const LOG: &str = "hn_interleave_log";
/// Rows of each arm drafted into the list.
pub const TOP: usize = 10;

#[derive(Debug)]
pub struct Test {
    pub id: String,
    pub name: String,
    /// The arms' knobs as given, and as the search reads them.
    pub a: Value,
    pub b: Value,
    pub ka: Knobs,
    pub kb: Knobs,
    /// The share of logged searches interleaved.
    pub share: f64,
    pub started: i64,
}

impl Test {
    pub fn view(&self) -> Value {
        json!({"id": self.id, "name": self.name, "a": self.a, "b": self.b, "share": self.share, "started": self.started, "top": TOP})
    }

    /// Whether the request takes part, and the draft's coins.
    pub fn take(&self, request: &str) -> Option<u32> {
        let mut z = crate::explore::seed(request) ^ 0x1f7e_51ea_5ed0_c0de;
        if crate::explore::uniform(&mut z) >= self.share {
            return None;
        }
        Some((crate::explore::uniform(&mut z) * (1u64 << 31) as f64) as u32)
    }
}

#[derive(Default)]
pub struct Interleave {
    active: RwLock<Option<Arc<Test>>>,
}

impl Interleave {
    pub fn current(&self) -> Option<Arc<Test>> {
        self.active.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, t: Option<Test>) {
        *self.active.write().unwrap_or_else(|e| e.into_inner()) = t.map(Arc::new);
    }
}

/// Team-draft interleaving of `a` and `b` to `limit` rows; the draft's
/// coin flips are the bits of `coins`, the j-th flip bit j (1: A picks).
/// Returns the rows and, per row, the team that picked it (true: B). The
/// same draft as the Experiments app's simulator (`chloedb-experiments`,
/// `src/interleave.rs`): keep the two the same.
pub fn team_draft(a: &[i64], b: &[i64], limit: usize, coins: u32) -> (Vec<i64>, Vec<bool>) {
    let mut rows: Vec<i64> = Vec::with_capacity(limit);
    let mut team = Vec::with_capacity(limit);
    let (mut na, mut nb, mut flips, mut ia, mut ib) = (0usize, 0usize, 0u32, 0usize, 0usize);
    while rows.len() < limit {
        while ia < a.len() && rows.contains(&a[ia]) {
            ia += 1;
        }
        while ib < b.len() && rows.contains(&b[ib]) {
            ib += 1;
        }
        let (has_a, has_b) = (ia < a.len(), ib < b.len());
        if !has_a && !has_b {
            break;
        }
        let a_turn = if na != nb {
            na < nb
        } else {
            let c = coins >> flips & 1 == 1;
            flips += 1;
            c
        };
        let a_picks = if !has_b { true } else if !has_a { false } else { a_turn };
        if a_picks {
            rows.push(a[ia]);
            team.push(false);
            na += 1;
        } else {
            rows.push(b[ib]);
            team.push(true);
            nb += 1;
        }
    }
    (rows, team)
}

/// The page's stories for an interleaved search: the drafted list, then
/// the page's own stories not in it, to the page's length. Returns the
/// stories, the ids drafted and their teams.
pub fn serve(a: Vec<Row>, b: Vec<Row>, page: Vec<Row>, per: usize, coins: u32) -> (Vec<Row>, Vec<i64>, Vec<bool>) {
    let id = |r: &Row| r.get("id").and_then(Value::as_i64).unwrap_or(0);
    let ia: Vec<i64> = a.iter().take(TOP).map(id).collect();
    let ib: Vec<i64> = b.iter().take(TOP).map(id).collect();
    let (ids, teams) = team_draft(&ia, &ib, TOP, coins);
    let mut out: Vec<Row> = ids.iter().filter_map(|k| a.iter().chain(b.iter()).find(|r| id(r) == *k).cloned()).collect();
    for r in page {
        if out.len() >= per.max(ids.len()) {
            break;
        }
        if !ids.contains(&id(&r)) {
            out.push(r);
        }
    }
    (out, ids, teams)
}

pub async fn create_tables(db: &Strata) -> crate::strata::Result<()> {
    db.sql(&format!("CREATE TABLE IF NOT EXISTS {TESTS} (id KEYWORD PRIMARY KEY, name TEXT OPTIONS(indexed = false), a TEXT OPTIONS(indexed = false), b TEXT OPTIONS(indexed = false), share DOUBLE, started INT, stopped INT)")).await?;
    db.sql(&format!("CREATE TABLE IF NOT EXISTS {LOG} (id KEYWORD PRIMARY KEY, test KEYWORD, ts INT, q TEXT OPTIONS(indexed = false), a TEXT OPTIONS(indexed = false), b TEXT OPTIONS(indexed = false), shown TEXT OPTIONS(indexed = false), teams KEYWORD OPTIONS(indexed = false), dev KEYWORD, a_ms DOUBLE, b_ms DOUBLE)")).await?;
    // Since: what a search was (drafted, a next page, another search, an
    // arm's failure), the browser's anonymous id, and each story's age and site.
    for col in ["kind KEYWORD", "err KEYWORD", "vid KEYWORD", "page INT", "meta TEXT OPTIONS(indexed = false)"] {
        db.sql(&format!("ALTER TABLE {LOG} ADD COLUMN IF NOT EXISTS {col}")).await?;
    }
    Ok(())
}

/// The browser's anonymous id as the page sent it, or none when it is not
/// one (16 hex digits).
pub fn visitor(v: &str) -> String {
    if v.len() == 16 && v.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        v.to_string()
    } else {
        String::new()
    }
}

/// Each story's age in days (at `now`) and its site, by id: what the
/// report reads for freshness and diversity.
pub fn meta<'a>(rows: impl Iterator<Item = &'a crate::strata::Row>, now: i64) -> Value {
    let mut m = serde_json::Map::new();
    for r in rows {
        let Some(id) = r.get("id").and_then(Value::as_i64) else { continue };
        let created = r.get("created").and_then(Value::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp());
        let age = created.map(|c| ((now - c).max(0) as f64 / 86_400.0 * 10.0).round() / 10.0);
        let site = r.get("site").and_then(Value::as_str).unwrap_or("");
        m.insert(id.to_string(), json!([age, site]));
    }
    Value::Object(m)
}

/// A test from its row or its request.
pub fn test_of(v: &Value, indexes: &[(String, String)]) -> Result<Test, String> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let knobs = |k: &str| -> Value {
        match v.get(k) {
            Some(Value::String(t)) => serde_json::from_str(t).unwrap_or(json!({})),
            Some(x) if x.is_object() => x.clone(),
            _ => json!({}),
        }
    };
    let id = s("id");
    if id.is_empty() || id.len() > 80 {
        return Err("a test has an id of at most 80 characters".into());
    }
    let share = v.get("share").and_then(Value::as_f64).unwrap_or(0.5);
    if !(share > 0.0 && share <= 1.0) {
        return Err("the share interleaved is more than 0 and at most 1".into());
    }
    let (a, b) = (knobs("a"), knobs("b"));
    if a == b {
        return Err("the two arms search alike: nothing to compare".into());
    }
    let ka = Knobs::from_json(&a, indexes).map_err(|e| format!("the control: {e}"))?;
    let kb = Knobs::from_json(&b, indexes).map_err(|e| format!("the treatment: {e}"))?;
    let name: String = s("name").chars().take(200).collect();
    Ok(Test { id, name, a, b, ka, kb, share, started: v.get("started").and_then(Value::as_i64).unwrap_or(0) })
}

/// The test running when the site last stopped, if any.
pub async fn load(db: &Strata, indexes: &[(String, String)]) -> Option<Test> {
    let rows = db.query(&format!("SELECT id, name, a, b, share, started FROM {TESTS} WHERE stopped = 0 ORDER BY started DESC LIMIT 1"), json!({})).await.ok()?;
    let r = rows.first()?;
    test_of(&Value::Object(r.clone()), indexes).ok()
}

pub fn row(t: &Test, stopped: i64) -> Value {
    json!({"id": t.id, "name": t.name, "a": t.a.to_string(), "b": t.b.to_string(), "share": t.share, "started": t.started, "stopped": stopped})
}

/// A test's searches since `since`, newest first, with the stories opened
/// from each.
pub async fn read(db: &Strata, test: &str, since: i64, limit: usize) -> crate::strata::Result<Value> {
    let reqs = db.query(&format!("SELECT id, ts, q, a, b, shown, teams, dev, a_ms, b_ms, kind, err, vid, page, meta FROM {LOG} WHERE test = :t AND ts >= :s ORDER BY ts DESC LIMIT {limit}"), json!({"t": test, "s": since})).await?;
    let ids: Vec<String> = reqs.iter().filter_map(|r| r.get("id").and_then(Value::as_str).map(str::to_string)).collect();
    let mut clicks: std::collections::HashMap<String, Vec<(i64, i64)>> = Default::default();
    for chunk in ids.chunks(500) {
        let list = chunk.iter().map(|k| format!("'{}'", k.replace('\'', "''"))).collect::<Vec<_>>().join(", ");
        let cs = db.query(&format!("SELECT request, story, ts FROM {} WHERE request IN ({list}) LIMIT 10000", crate::explore::CLICKS), json!({})).await?;
        for c in &cs {
            let r = c.get("request").and_then(Value::as_str).unwrap_or("").to_string();
            let s = c.get("story").and_then(Value::as_i64).unwrap_or(0);
            let t = c.get("ts").and_then(Value::as_i64).unwrap_or(0);
            let v = clicks.entry(r).or_default();
            match v.iter_mut().find(|x| x.0 == s) {
                // A story opened twice: its first time.
                Some(x) => x.1 = x.1.min(t),
                None => v.push((s, t)),
            }
        }
    }
    let parse = |r: &Row, k: &str| -> Value { r.get(k).and_then(Value::as_str).and_then(|s| serde_json::from_str(s).ok()).unwrap_or(json!([])) };
    let out: Vec<Value> = reqs
        .iter()
        .map(|r| {
            let id = r.get("id").and_then(Value::as_str).unwrap_or("");
            let cl = clicks.get(id).cloned().unwrap_or_default();
            let meta = r.get("meta").and_then(Value::as_str).and_then(|s| serde_json::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
            json!({
                "request": id, "ts": r.get("ts"), "q": r.get("q"), "a": parse(r, "a"), "b": parse(r, "b"), "shown": parse(r, "shown"),
                "teams": r.get("teams"), "dev": r.get("dev"), "a_ms": r.get("a_ms"), "b_ms": r.get("b_ms"),
                "kind": r.get("kind").cloned().unwrap_or_else(|| json!("drafted")), "err": r.get("err"), "vid": r.get("vid"), "page": r.get("page"), "meta": meta,
                "clicks": cl.iter().map(|c| c.0).collect::<Vec<_>>(), "click_ts": cl.iter().map(|c| c.1).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({"requests": out}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_draft_alternates_and_skips_rows_shown() {
        let (rows, team) = team_draft(&[1, 2, 3], &[2, 4, 5], 4, 0b1);
        assert_eq!(rows, vec![1, 2, 4, 3]);
        assert_eq!(team, vec![false, true, true, false]);
        let (rows, _) = team_draft(&[1, 2], &[1, 2], 4, 0);
        assert_eq!(rows, vec![1, 2]);
    }

    #[test]
    fn a_share_of_requests_take_part() {
        let t = Test { id: "t".into(), name: String::new(), a: json!({}), b: json!({"votes": 1}), ka: Knobs::default(), kb: Knobs::default(), share: 0.3, started: 0 };
        let n = (0..10_000).filter(|i| t.take(&format!("r{i}")).is_some()).count();
        assert!((2_700..3_300).contains(&n), "{n}");
        assert_eq!(t.take("same"), t.take("same"));
    }

    #[test]
    fn serves_the_draft_then_the_page() {
        let r = |i: i64| json!({"id": i}).as_object().unwrap().clone();
        let (out, ids, teams) = serve((1..=12).map(r).collect(), (5..=16).map(r).collect(), (20..=40).map(r).collect(), 15, 0);
        assert_eq!(ids.len(), TOP);
        assert_eq!(teams.len(), TOP);
        assert_eq!(out.len(), 15);
        let got: Vec<i64> = out.iter().map(|x| x["id"].as_i64().unwrap()).collect();
        assert_eq!(&got[..TOP], &ids[..]);
        assert_eq!(&got[TOP..], &[20, 21, 22, 23, 24]);
    }
}
