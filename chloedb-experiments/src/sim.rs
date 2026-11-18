//! The simulator: users of a site ask its queries, a logging policy draws
//! each list from the site's ranking, and a click model says what each
//! user does with it. Two modes make the same impressions:
//!
//! - **served**: each list is a logged, exploring query to a chloedb-server
//!   (`"log": {"explore": ...}`), so the server draws it and writes it to
//!   its rank log, and the clicks go back as events; the estimators then
//!   read what the server logged, through its export.
//! - **fast**: the lists are drawn here, by the server's policies ported
//!   ([`crate::policy`]), from the ranking's rows fetched once per query,
//!   for millions of impressions a minute.
//!
//! Impression i of a run is the same in both: its user and query come from
//! a stream seeded by the run and i, its list from the request's own stream
//! (the server's seed), and its clicks from another. So a fast run checks
//! itself against a served one, impression by impression.
//!
//! Each impression's context (user, query) is drawn independently of what
//! came before, as off-policy evaluation assumes; a user's sessions are
//! valued exactly from what each query's list does ([`Truth::session`]).

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::click::{self, Expected, Model, Outcome, Slot};
use crate::policy::{Policy, Rng};
use crate::server::{Result, Server};
use crate::site::Site;
use crate::users::{Alias, Population, User};

/// The time the first impression of a served run is logged at
/// (2026-11-01T00:00:00Z, in microseconds); impression i is 10 s later
/// per i, so one user's impressions never share an event's window.
pub const BASE_TS: i64 = 1_793_491_200_000_000;
pub const STEP_US: i64 = 10_000_000;
/// The window after an impression in which its user's events label it.
pub const WINDOW_S: i64 = 5;
/// A session's longest run of queries.
pub const SESSION_CAP: i32 = 20;

/// What a ranking ranked first for one query: keys and scores.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ranked {
    pub keys: Vec<String>,
    pub scores: Vec<Option<f64>>,
}

/// One list shown to one user, and what they did.
#[derive(Clone, Debug, PartialEq)]
pub struct Impression {
    pub i: u64,
    pub user: u32,
    pub query: u32,
    /// Per position, the row shown, as its index in the query's ranked rows.
    pub shown: Vec<u16>,
    /// Per position, the logging policy's chance of that row there.
    pub propensity: Vec<f64>,
    /// Per position, its chance of the rows down to it, in this order.
    pub prefix: Vec<f64>,
    pub click: Vec<bool>,
    pub satisfied: Vec<bool>,
    /// Seconds per click (0 where none).
    pub dwell: Vec<f64>,
    /// The query's time at the server (served runs).
    pub took_ms: f64,
}

impl Impression {
    pub fn reward(&self, satisfied: bool, k: usize) -> f64 {
        (if satisfied { self.satisfied[k] } else { self.click[k] }) as u8 as f64
    }
}

/// A user and a query, for impression `i` of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub i: u64,
    pub user: u32,
    pub query: u32,
    pub request: String,
}

pub struct World {
    pub site: Site,
    pub users: Population,
    pub model: Model,
    pub policy: Policy,
    /// Rows shown.
    pub limit: usize,
    /// The site's ranking: an ORDER BY expression over its table, of
    /// parameter `:t`, the query's text.
    pub rank: String,
    /// Per query, the ranking's first `limit.max(top)` rows.
    pub ranked: Vec<Ranked>,
    keyed: Vec<HashMap<String, u16>>,
    /// Per query, a softmax's position chances (see [`Policy::draw_with`]).
    kept: Vec<Option<Vec<Vec<f64>>>>,
    pick_query: Alias,
    /// Per query, taste group and ranked row: the row's chances.
    slots: Vec<Vec<Vec<Slot>>>,
}

/// The taste group of a user: age band and gender.
fn group(u: &User) -> usize {
    u.age as usize * 3 + u.gender as usize
}
const GROUPS: usize = 18;

fn fnv(s: &str) -> u32 {
    let h = s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    (h ^ (h >> 32)) as u32
}

impl World {
    /// The rows a query fetches to draw from.
    pub fn depth(policy: &Policy, limit: usize) -> usize {
        policy.top().max(limit)
    }

    /// The logged query a served impression asks.
    pub fn logged_sql(&self) -> String {
        format!("SELECT id FROM {} ORDER BY {} DESC LIMIT {}", self.site.table, self.rank, self.limit)
    }

    /// Fetches each query's ranked rows from the server.
    pub async fn rank_all(server: &Server, site: &Site, rank: &str, depth: usize) -> Result<Vec<Ranked>> {
        let sql = format!("SELECT id, score() FROM {} ORDER BY {rank} DESC LIMIT {depth}", site.table);
        let mut out = Vec::with_capacity(site.queries.len());
        for q in &site.queries {
            let a = server.query(&sql, json!({"t": q.text}), None).await?;
            let rows = a["rows"].as_array().cloned().unwrap_or_default();
            out.push(Ranked { keys: rows.iter().map(|r| key_text(&r[0])).collect(), scores: rows.iter().map(|r| r[1].as_f64()).collect() });
        }
        Ok(out)
    }

    pub fn new(site: Site, users: Population, model: Model, policy: Policy, limit: usize, rank: String, ranked: Vec<Ranked>) -> World {
        let keyed = ranked.iter().map(|r| r.keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u16)).collect()).collect();
        let kept = ranked
            .iter()
            .map(|r| match &policy {
                Policy::Softmax { tau, by, .. } => {
                    let n = policy.top().min(r.scores.len());
                    (n >= 2).then(|| Policy::weights(*tau, *by, &r.scores, n).ok().map(|w| Policy::marginals_of(&w, n))).flatten()
                }
                _ => None,
            })
            .collect();
        let pick_query = Alias::new(&site.queries.iter().map(|q| q.weight).collect::<Vec<_>>());
        let mut w = World { site, users, model, policy, limit, rank, ranked, keyed, kept, pick_query, slots: Vec::new() };
        w.slots = (0..w.ranked.len()).map(|q| (0..GROUPS).map(|g| w.ranked[q].keys.iter().map(|k| w.slot_for(g, q, k)).collect()).collect()).collect();
        w
    }

    fn slot_for(&self, g: usize, q: usize, key: &str) -> Slot {
        let base = self.site.slot(q, key);
        let taste = self.users.taste(g as u32, q as u32, fnv(key));
        Slot { attract: (base.attract * taste).min(0.95), satisfy: base.satisfy }
    }

    /// Row `key`'s chances for user `u` asking query `q`.
    pub fn slot(&self, u: &User, q: usize, key: &str) -> Slot {
        match self.keyed[q].get(key) {
            Some(&i) => self.slots[q][group(u)][i as usize],
            None => self.slot_for(group(u), q, key),
        }
    }

    /// The ranked row `i` of query `q`, its index if among them.
    pub fn index(&self, q: usize, key: &str) -> Option<u16> {
        self.keyed[q].get(key).copied()
    }

    pub fn user(&self, id: u32) -> &User {
        &self.users.users[id as usize]
    }

    pub fn context(&self, run: &str, i: u64) -> Context {
        let mut rng = Rng::of(&format!("{run}/{i}"));
        let user = self.users.draw(&mut rng).id;
        let query = self.pick_query.draw(&mut rng) as u32;
        Context { i, user, query, request: format!("{run}-{i}") }
    }

    /// What the user does with the rows `shown` (ranked indexes).
    pub fn react(&self, c: &Context, shown: &[u16]) -> Outcome {
        let u = self.user(c.user);
        let g = group(u);
        let slots: Vec<Slot> = shown.iter().map(|&i| self.slots[c.query as usize][g][i as usize]).collect();
        self.model.sample(&slots, &u.behaviour, &mut Rng::of(&format!("{}/clicks", c.request)))
    }

    /// Impression `i` of run `run`, drawn here.
    pub fn fast(&self, run: &str, i: u64) -> Impression {
        let c = self.context(run, i);
        let q = c.query as usize;
        let r = &self.ranked[q];
        let d = self.policy.draw_with(&r.scores, &mut Rng::of(&c.request), self.kept[q].as_deref()).expect("a policy the world was made with draws");
        let n = self.limit.min(d.order.len());
        let shown: Vec<u16> = d.order[..n].iter().map(|&i| i as u16).collect();
        let o = self.react(&c, &shown);
        Impression { i, user: c.user, query: c.query, shown, propensity: d.propensity[..n].to_vec(), prefix: d.prefix[..n].to_vec(), click: o.click, satisfied: o.satisfied, dwell: o.dwell, took_ms: 0.0 }
    }

    /// Impression `i` of run `run`, drawn by the server: a logged, exploring
    /// query to log table `log`; the clicks it gets are returned as write
    /// operations on table `events`.
    pub async fn served(&self, server: &Server, run: &str, log: &str, events: &str, i: u64) -> Result<(Impression, Vec<Value>)> {
        let c = self.context(run, i);
        let q = c.query as usize;
        let ts = BASE_TS + i as i64 * STEP_US;
        let mut l = json!({"request": c.request, "context": format!("u{}", c.user), "ts": ts, "table": log, "variant": run});
        if self.policy != Policy::None {
            l["explore"] = self.policy.to_json();
        }
        let a = server.query(&self.logged_sql(), json!({"t": self.site.queries[q].text}), Some(l)).await?;
        let keys: Vec<String> = a["keys"].as_array().ok_or_else(|| format!("{}: no keys in {a}", c.request))?.iter().map(key_text).collect();
        let shown: Vec<u16> = keys.iter().map(|k| self.index(q, k).ok_or_else(|| format!("{}: row {k} is not among the query's ranked rows (did the table change?)", c.request))).collect::<Result<_>>()?;
        let propensity: Vec<f64> = match a["propensities"].as_array() {
            Some(p) => p.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect(),
            None => vec![1.0; shown.len()],
        };
        let order: Vec<usize> = shown.iter().map(|&i| i as usize).collect();
        let prefix = self.policy.prefix_chances(&self.ranked[q].scores, &order)?;
        let o = self.react(&c, &shown);
        let ops = (0..shown.len())
            .filter(|&k| o.click[k])
            .map(|k| json!({"op": "insert", "table": events, "doc": {"id": format!("{}#{k}", c.request), "usr": format!("u{}", c.user), "item": keys[k], "action": if o.satisfied[k] { "satisfied" } else { "click" }, "ts": ts + 1_000_000, "dwell_ms": o.dwell[k] * 1000.0}}))
            .collect();
        let took_ms = a["took_ms"].as_f64().unwrap_or(0.0);
        Ok((Impression { i, user: c.user, query: c.query, shown, propensity, prefix, click: o.click, satisfied: o.satisfied, dwell: o.dwell, took_ms }, ops))
    }

    /// The impressions a served run's log holds, as the server exports
    /// them: its rows with their propensities, labelled by the events in
    /// the window after each (a click 1, a satisfied click 2).
    pub async fn read_back(&self, server: &Server, run: &str, log: &str, events: &str) -> Result<Vec<Impression>> {
        let body = json!({"table": log, "events": {"table": events, "user": "usr", "item": "item", "time": "ts", "action": "action"}, "window": WINDOW_S, "grades": {"click": 1, "satisfied": 2}});
        // Per impression, its shown rows as the log has them: position,
        // key, propensity, prefix propensity, label, and the context of
        // the first (kept small: a run's log is millions of rows).
        type Row = (u64, String, f64, f64, f64, Option<String>);
        let mut by_i: HashMap<u64, Vec<Row>> = HashMap::new();
        let prefix = format!("{run}-");
        let mut bad = None;
        server
            .export(&body, |r| {
                let req = r["request"].as_str().unwrap_or_default();
                match req.strip_prefix(&prefix).and_then(|x| x.parse::<u64>().ok()) {
                    Some(i) if r["shown"] == json!(true) => by_i.entry(i).or_default().push((
                        r["position"].as_u64().unwrap_or(0),
                        key_text(&r["row_key"]),
                        r["propensity"].as_f64().unwrap_or(f64::NAN),
                        r["prefix_propensity"].as_f64().unwrap_or(f64::NAN),
                        r["label"].as_f64().unwrap_or(0.0),
                        r["context"].as_str().map(str::to_string),
                    )),
                    Some(_) => {}
                    None => bad = Some(req.to_string()),
                }
            })
            .await?;
        if let Some(b) = bad {
            return Err(format!("log {log} holds request {b:?}, not of run {run}"));
        }
        let mut out: Vec<Impression> = Vec::with_capacity(by_i.len());
        for (i, mut rows) in by_i {
            rows.sort_by_key(|r| r.0);
            let c = self.context(run, i);
            let q = c.query as usize;
            let mut imp = Impression { i, user: c.user, query: c.query, shown: Vec::new(), propensity: Vec::new(), prefix: Vec::new(), click: Vec::new(), satisfied: Vec::new(), dwell: Vec::new(), took_ms: 0.0 };
            if rows.first().and_then(|r| r.5.as_deref()) != Some(&format!("u{}", c.user)) {
                return Err(format!("{run}-{i}: logged for {:?}, drawn for u{}", rows.first().map(|r| r.5.clone()), c.user));
            }
            for (_, key, p, pre, label, _) in &rows {
                imp.shown.push(self.index(q, key).ok_or_else(|| format!("{run}-{i}: logged row {key} is not ranked"))?);
                imp.propensity.push(*p);
                imp.prefix.push(*pre);
                imp.click.push(*label >= 1.0);
                imp.satisfied.push(*label >= 2.0);
                imp.dwell.push(0.0);
            }
            out.push(imp);
        }
        out.sort_by_key(|x| x.i);
        Ok(out)
    }
}

/// A key as text, as the rank log keeps it.
pub fn key_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The exact value of a ranking, per impression, over the population and
/// the queries as the simulator draws them.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Truth {
    /// Clicks per impression.
    pub clicks: f64,
    /// Satisfied clicks per impression.
    pub satisfied: f64,
    /// The chance of a satisfied click (the query's success).
    pub success: f64,
    /// The chance of no click.
    pub abandonment: f64,
    /// Seconds of dwell per impression.
    pub dwell: f64,
    /// Queries per session, a session going on after each query with the
    /// user's chance for how that query went (at most [`SESSION_CAP`]).
    pub session: f64,
    /// Per position, clicks and satisfied clicks.
    pub click_at: Vec<f64>,
    pub satisfied_at: Vec<f64>,
}

impl Truth {
    pub fn metric(&self, name: &str) -> f64 {
        match name {
            "clicks" => self.clicks,
            "satisfied" => self.satisfied,
            "success" => self.success,
            "abandonment" => self.abandonment,
            "dwell" => self.dwell,
            "session" => self.session,
            _ => f64::NAN,
        }
    }
}

impl World {
    /// The exact value of showing, for each query, the rows `lists[q]` (keys,
    /// in order; the first `limit` are shown), over the users `keep` keeps.
    pub fn truth(&self, lists: &[Vec<String>], keep: &(dyn Fn(&User) -> bool + Sync)) -> Truth {
        let shares = self.users.shares();
        let qshares = self.site.shares();
        let users: Vec<&User> = self.users.users.iter().filter(|u| keep(u)).collect();
        let total: f64 = users.iter().map(|u| shares[u.id as usize]).sum();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
        let chunk = users.len().div_ceil(threads).max(1);
        let parts: Vec<Truth> = std::thread::scope(|s| {
            let hs: Vec<_> = users
                .chunks(chunk)
                .map(|us| {
                    let (shares, qshares) = (&shares, &qshares);
                    s.spawn(move || {
                        let mut t = Truth { click_at: vec![0.0; self.limit], satisfied_at: vec![0.0; self.limit], ..Default::default() };
                        for u in us {
                            let wu = shares[u.id as usize] / total;
                            let mut go_on = 0.0;
                            for (q, list) in lists.iter().enumerate() {
                                let slots: Vec<Slot> = list.iter().take(self.limit).map(|k| self.slot(u, q, k)).collect();
                                let e: Expected = self.model.expected(&slots, &u.behaviour);
                                let w = wu * qshares[q];
                                t.clicks += w * e.click.iter().sum::<f64>();
                                t.satisfied += w * e.satisfied.iter().sum::<f64>();
                                t.success += w * e.any_satisfied;
                                t.abandonment += w * (1.0 - e.any_click);
                                t.dwell += w * e.dwell;
                                for k in 0..e.click.len() {
                                    t.click_at[k] += w * e.click[k];
                                    t.satisfied_at[k] += w * e.satisfied[k];
                                }
                                go_on += qshares[q] * (e.any_satisfied * u.go_on.0 + (1.0 - e.any_satisfied) * u.go_on.1);
                            }
                            t.session += wu * (1.0 - go_on.powi(SESSION_CAP)) / (1.0 - go_on);
                        }
                        t
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("a truth thread")).collect()
        });
        let mut t = Truth { click_at: vec![0.0; self.limit], satisfied_at: vec![0.0; self.limit], ..Default::default() };
        for p in parts {
            t.clicks += p.clicks;
            t.satisfied += p.satisfied;
            t.success += p.success;
            t.abandonment += p.abandonment;
            t.dwell += p.dwell;
            t.session += p.session;
            for k in 0..t.click_at.len().min(p.click_at.len()) {
                t.click_at[k] += p.click_at[k];
                t.satisfied_at[k] += p.satisfied_at[k];
            }
        }
        t
    }

    /// Under PBM, the exact clicks on the ranking's first row when it is
    /// shown at position k, per k, over the population and queries: what
    /// a swap policy's log measures position bias by.
    pub fn first_row_clicks(&self) -> Vec<f64> {
        let shares = self.users.shares();
        let qshares = self.site.shares();
        let mut out = vec![0.0; self.limit];
        for u in &self.users.users {
            for (q, r) in self.ranked.iter().enumerate() {
                let Some(k0) = r.keys.first() else { continue };
                let a = self.slot(u, q, k0).attract;
                for (k, o) in out.iter_mut().enumerate() {
                    *o += shares[u.id as usize] * qshares[q] * click::theta(u.behaviour.eta, k) * a;
                }
            }
        }
        out
    }
}

/// The lists of a ranking that reorders each query's ranked rows by
/// `order(q, &ranked rows) -> indexes`, as keys.
pub fn reordered(w: &World, order: impl Fn(usize, &Ranked) -> Vec<usize>) -> Vec<Vec<String>> {
    w.ranked.iter().enumerate().map(|(q, r)| order(q, r).into_iter().map(|i| r.keys[i].clone()).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::Query;
    use crate::users::Shape;

    /// A world without a server: 30 queries over made-up rows.
    pub fn toy(model: Model, policy: Policy) -> World {
        let nq = 30;
        let queries: Vec<Query> = (0..nq).map(|q| Query { id: format!("q{q}"), text: format!("query {q}"), weight: 1.0 / (q + 1) as f64 }).collect();
        let grades: Vec<HashMap<String, u8>> = (0..nq).map(|q| (0..12).filter(|d| (d * 7 + q) % 5 < 2).map(|d| (format!("d{q}-{d}"), ((d + q) % 3) as u8)).filter(|(_, g)| *g > 0).collect()).collect();
        let site = Site { name: "toy".into(), table: "docs".into(), queries, grades, max_grade: 2 };
        let depth = World::depth(&policy, 5);
        let ranked = (0..nq).map(|q| Ranked { keys: (0..depth.max(12)).map(|d| format!("d{q}-{d}")).collect(), scores: (0..depth.max(12)).map(|d| Some(12.0 - d as f64 * 0.8)).collect() }).collect();
        World::new(site, Population::new(400, 11, Shape::default()), model, policy, 5, "bm25(body, :t)".into(), ranked)
    }

    #[test]
    fn an_impression_is_its_run_and_index() {
        let w = toy(Model::Dbn, Policy::Softmax { tau: 1.0, top: 10, by: crate::policy::By::Rank });
        let (a, b) = (w.fast("r", 17), w.fast("r", 17));
        assert_eq!(a, b);
        assert_ne!(w.fast("r", 17), w.fast("s", 17));
        // The propensities are the policy's, for the request's draw.
        let c = w.context("r", 17);
        let d = w.policy.draw(&w.ranked[c.query as usize].scores, &mut Rng::of(&c.request)).unwrap();
        assert_eq!(a.propensity, d.propensity[..5].to_vec());
        assert_eq!(a.prefix, d.prefix[..5].to_vec());
        let order: Vec<usize> = a.shown.iter().map(|&i| i as usize).collect();
        assert_eq!(w.policy.prefix_chances(&w.ranked[c.query as usize].scores, &order).unwrap(), a.prefix);
    }

    /// The truth is what the simulator's impressions average to.
    #[test]
    fn the_truth_is_what_users_do() {
        for model in [Model::Pbm, Model::Dbn, Model::Ubm] {
            let w = toy(model, Policy::None);
            let lists = reordered(&w, |_, r| (0..r.keys.len()).collect());
            let t = w.truth(&lists, &|_| true);
            let n = 200_000;
            let (mut clicks, mut sat) = (0.0, 0.0);
            for i in 0..n {
                let imp = w.fast("t", i);
                clicks += imp.click.iter().filter(|&&c| c).count() as f64;
                sat += imp.satisfied.iter().filter(|&&c| c).count() as f64;
            }
            let (c, s) = (clicks / n as f64, sat / n as f64);
            // Clicks per impression have a standard deviation below 1.5.
            assert!((c - t.clicks).abs() < 4.0 * 1.5 / (n as f64).sqrt(), "{}: {c} vs {}", model.name(), t.clicks);
            assert!((s - t.satisfied).abs() < 4.0 * 1.5 / (n as f64).sqrt(), "{}: {s} vs {}", model.name(), t.satisfied);
            assert!(t.session > 1.0 && t.session < SESSION_CAP as f64);
        }
    }
}
