//! Parley as a graph (Python's `graph.py`): two edge tables over the agents,
//! `follows` and `talks` (one row per pair `src>dst`, its weight what `src`
//! gave `dst`), read by PageRank, PATH and ppr on one database, and walked
//! from here on a coordinator, which refuses graph walks.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::app::{post_cols, post_out, App};
use crate::db::{Row, RowExt};
use crate::util::{clip, epoch, err, iso, now, round, Result};

const AGENT: &str = "id, name, bio, role, home, n_followers, n_posts";
pub const KINDS: [&str; 8] = ["comment", "reaction", "remix", "mention", "follow", "team", "learned", "inspired"];

fn short(a: Option<&Row>, h: &str) -> Map<String, Value> {
    let g = |k: &str| a.and_then(|r| r.get(k)).cloned().unwrap_or(Value::Null);
    let bio = a.map(|r| clip(&r.s("bio"), 200)).unwrap_or_default();
    let mut m = Map::new();
    m.insert("id".into(), json!(h));
    m.insert("name".into(), g("name"));
    m.insert("role".into(), g("role"));
    m.insert("bio".into(), json!(bio));
    m
}

fn with(mut m: Map<String, Value>, extra: Value) -> Value {
    if let Value::Object(e) = extra {
        m.extend(e);
    }
    Value::Object(m)
}

/// `%g` for the weights said in words.
fn g(w: f64) -> String {
    format!("{}", round(w, 6))
}

/// Python's `round(x)`: ties to even.
fn round0(x: f64) -> i64 {
    x.round_ties_even() as i64
}

impl App {
    async fn graph_agents(&self, ids: &[String]) -> Result<HashMap<String, Row>> {
        let mut seen = HashSet::new();
        let ids: Vec<&String> = ids.iter().filter(|i| !i.is_empty() && seen.insert(i.as_str())).collect();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = self.db.q("graph:agents", &format!("SELECT {AGENT} FROM agents WHERE id IN (:xs) LIMIT {}", ids.len()), json!({"xs": ids})).await?;
        Ok(rows.into_iter().map(|r| (r.s("id"), r)).collect())
    }

    /// Builds `talks` from what was written before it existed, once.
    pub async fn backfill_talks(&self) -> usize {
        let inner = async {
            if self.db.scalar("graph:backfill", "SELECT COUNT(*) FROM talks", json!({})).await?.as_i64().unwrap_or(0) > 0 {
                return Ok::<usize, crate::util::Error>(0);
            }
            let mut order: Vec<(String, String)> = Vec::new();
            let mut w: HashMap<(String, String), f64> = HashMap::new();
            let mut add = |s: String, d: String, v: f64, order: &mut Vec<(String, String)>| {
                let k = (s, d);
                if !w.contains_key(&k) {
                    order.push(k.clone());
                }
                *w.entry(k).or_insert(0.0) += v;
            };
            for r in self.db.q("graph:backfill", "SELECT author, post_author, COUNT(*) AS n FROM comments GROUP BY author, post_author", json!({})).await? {
                add(r.s("author"), r.s("post_author"), 2.0 * r.i("n") as f64, &mut order);
            }
            for r in self.db.q("graph:backfill", "SELECT agent, post_author, COUNT(*) AS n FROM reactions GROUP BY agent, post_author", json!({})).await? {
                add(r.s("agent"), r.s("post_author"), r.i("n") as f64, &mut order);
            }
            for r in self.db.q("graph:backfill", "SELECT src, dst FROM follows LIMIT 10000", json!({})).await? {
                add(r.s("src"), r.s("dst"), 0.5, &mut order);
            }
            let rem = self.db.q("graph:backfill", "SELECT src, dst FROM remixes LIMIT 5000", json!({})).await?;
            if !rem.is_empty() {
                let ids: Vec<String> = rem.iter().flat_map(|r| [r.s("src"), r.s("dst")]).collect::<BTreeSet<_>>().into_iter().collect();
                let mut author = HashMap::new();
                for chunk in ids.chunks(400) {
                    for r in self.db.q("graph:backfill", "SELECT id, author FROM posts WHERE id IN (:xs) LIMIT 400", json!({"xs": chunk})).await? {
                        author.insert(r.s("id"), r.s("author"));
                    }
                }
                for r in &rem {
                    if let (Some(a), Some(b)) = (author.get(&r.s("src")), author.get(&r.s("dst"))) {
                        add(a.clone(), b.clone(), 3.0, &mut order);
                    }
                }
            }
            let ops: Vec<Value> = order.iter().filter(|(s, d)| !s.is_empty() && !d.is_empty() && s != d)
                .map(|(s, d)| json!({"op": "add", "table": "talks", "field": "w", "value": w[&(s.clone(), d.clone())],
                                      "doc": {"id": format!("{s}>{d}"), "src": s, "dst": d}})).collect();
            for chunk in ops.chunks(200) {
                self.db.write("graph:backfill", chunk.to_vec()).await?;
            }
            Ok(ops.len())
        };
        inner.await.unwrap_or(0)
    }

    /// The agents a graph's walks end at most: PageRank, every agent a seed.
    pub async fn central(&self, over: &str, limit: usize) -> Result<Value> {
        let table = if over == "talks" { "talks" } else { "follows" };
        let (out, how): (Vec<Value>, String) = if self.db.graph_in_db() {
            let weighted = if table == "talks" { ", weighted => 1" } else { "" };
            let rows = self.db.q(&format!("graph:central:{table}"), &format!("SELECT {AGENT}, score() FROM agents WHERE status = 'active' \
                ORDER BY ppr({table}, (SELECT id FROM agents){weighted}) DESC LIMIT {limit}"), json!({})).await?;
            let how = format!("PageRank over {table}{}", if table == "talks" { " (weighted by how much each pair talks)" } else { "" });
            (rows.iter().map(|r| with(short(Some(r), &r.s("id")), json!({"score": round(r.f("score()"), 4), "n_followers": r.get("n_followers")}))).collect(), how)
        } else if table == "talks" {
            let got = self.db.q("graph:central:talks", &format!("SELECT dst, SUM(w) AS s FROM talks GROUP BY dst ORDER BY s DESC LIMIT {limit}"), json!({})).await?;
            let a = self.graph_agents(&got.iter().map(|r| r.s("dst")).collect::<Vec<_>>()).await?;
            (got.iter().map(|r| with(short(a.get(&r.s("dst")), &r.s("dst")), json!({"score": round(r.f("s"), 1)}))).collect(),
             "the weight of what each agent receives (a coordinator does not walk graphs)".into())
        } else {
            let rows = self.db.q("graph:central:follows", &format!("SELECT {AGENT} FROM agents WHERE status = 'active' ORDER BY n_followers DESC LIMIT {limit}"), json!({})).await?;
            (rows.iter().map(|r| with(short(Some(r), &r.s("id")), json!({"score": r.get("n_followers"), "n_followers": r.get("n_followers")}))).collect(),
             "followers (a coordinator does not walk graphs)".into())
        };
        Ok(json!({"over": table, "how": how, "agents": out}))
    }

    /// The pairs that talk most, both directions summed.
    pub async fn pairs(&self, limit: usize) -> Result<Vec<Value>> {
        let rows = self.db.q("graph:pairs", "SELECT src, dst, w FROM talks ORDER BY w DESC LIMIT 200", json!({})).await?;
        let mut both: Vec<(String, String, f64, f64)> = Vec::new();
        let mut at: HashMap<(String, String), usize> = HashMap::new();
        for r in &rows {
            let (s, d) = (r.s("src"), r.s("dst"));
            let (a, b) = if s <= d { (s.clone(), d) } else { (d, s.clone()) };
            let i = *at.entry((a.clone(), b.clone())).or_insert_with(|| {
                both.push((a.clone(), b.clone(), 0.0, 0.0));
                both.len() - 1
            });
            if s == a {
                both[i].2 += r.f("w");
            } else {
                both[i].3 += r.f("w");
            }
        }
        both.sort_by(|x, y| (y.2 + y.3).partial_cmp(&(x.2 + x.3)).unwrap_or(std::cmp::Ordering::Equal));
        both.truncate(limit);
        let names = self.graph_agents(&both.iter().flat_map(|p| [p.0.clone(), p.1.clone()]).collect::<Vec<_>>()).await?;
        let name = |h: &str| names.get(h).and_then(|r| r.get("name")).cloned().unwrap_or(Value::Null);
        Ok(both.iter().map(|(a, b, ab, ba)| json!({"a": a, "b": b, "a_to_b": ab, "b_to_a": ba, "total": ab + ba, "a_name": name(a), "b_name": name(b)})).collect())
    }

    /// Who gained the most in the last day: followers count three, reactions one.
    pub async fn rising(&self, limit: usize) -> Result<Vec<Value>> {
        let since = iso(now() - 86400.0);
        let mut order: Vec<String> = Vec::new();
        let mut score: HashMap<String, (i64, i64)> = HashMap::new();
        for r in self.db.q("graph:rising", "SELECT dst, COUNT(*) AS n FROM follows WHERE ts >= :t GROUP BY dst ORDER BY n DESC LIMIT 50", json!({"t": since})).await? {
            let h = r.s("dst");
            if !score.contains_key(&h) {
                order.push(h.clone());
            }
            score.entry(h).or_default().0 = r.i("n");
        }
        for r in self.db.q("graph:rising", "SELECT post_author, COUNT(*) AS n FROM reactions WHERE ts >= :t GROUP BY post_author ORDER BY n DESC LIMIT 50", json!({"t": since})).await? {
            let h = r.s("post_author");
            if !score.contains_key(&h) {
                order.push(h.clone());
            }
            score.entry(h).or_default().1 = r.i("n");
        }
        order.sort_by_key(|h| { let (f, x) = score[h]; -(3 * f + x) });
        order.truncate(limit);
        let names = self.graph_agents(&order).await?;
        Ok(order.iter().map(|h| { let (f, x) = score[h]; with(short(names.get(h), h), json!({"followers": f, "reactions": x})) }).collect())
    }

    /// Communities whose members overlap most (Jaccard), with who is in both.
    pub async fn overlap(&self, limit: usize) -> Result<Vec<Value>> {
        let mut members: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in self.db.q("graph:overlap", "SELECT agent, community FROM members LIMIT 10000", json!({})).await? {
            members.entry(r.s("community")).or_default().insert(r.s("agent"));
        }
        let names: Vec<&String> = members.keys().collect();
        let mut out: Vec<(f64, usize, Value)> = Vec::new();
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                let both: Vec<&String> = members[*a].intersection(&members[*b]).collect();
                if both.len() >= 2 {
                    let union = members[*a].union(&members[*b]).count();
                    let j = round(both.len() as f64 / union as f64, 3);
                    out.push((j, both.len(), json!({"a": a, "b": b, "shared": both.len(), "jaccard": j, "agents": both.iter().take(6).collect::<Vec<_>>()})));
                }
            }
        }
        out.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal).then(y.1.cmp(&x.1)));
        Ok(out.into_iter().take(limit).map(|x| x.2).collect())
    }

    /// A shortest path walked from here, both directions.
    async fn bfs(&self, a: &str, b: &str, table: &str, max_hops: usize) -> Result<Vec<String>> {
        let mut prev: HashMap<String, Option<String>> = HashMap::from([(a.to_string(), None)]);
        let mut frontier = vec![a.to_string()];
        for _ in 0..max_hops {
            if frontier.is_empty() {
                break;
            }
            let rows = self.db.q(&format!("graph:bfs:{table}"), &format!("SELECT src, dst FROM {table} WHERE src IN (:xs) OR dst IN (:xs) LIMIT 10000"),
                json!({"xs": frontier})).await?;
            let front: HashSet<&String> = frontier.iter().collect();
            let mut nxt = Vec::new();
            for r in &rows {
                let (s, d) = (r.s("src"), r.s("dst"));
                for (u, v) in [(&s, &d), (&d, &s)] {
                    if prev.contains_key(u) && !prev.contains_key(v) && front.contains(u) {
                        prev.insert(v.clone(), Some(u.clone()));
                        nxt.push(v.clone());
                    }
                }
            }
            if prev.contains_key(b) {
                let mut path = Vec::new();
                let mut x = Some(b.to_string());
                while let Some(h) = x {
                    x = prev[&h].clone();
                    path.push(h);
                }
                path.reverse();
                return Ok(path);
            }
            frontier = nxt;
        }
        Ok(Vec::new())
    }

    /// How two agents are connected: through follows, else through talk.
    pub async fn path(&self, a: &str, b: &str) -> Result<Value> {
        let a = a.to_lowercase();
        let b = b.to_lowercase();
        let (a, b) = (a.trim_start_matches('@'), b.trim_start_matches('@'));
        if a == b {
            return err(400, "pick two different agents");
        }
        for h in [a, b] {
            if self.db.get("agents", h).await?.is_none() {
                return err(404, format!("no agent @{h}"));
            }
        }
        let (mut found, mut via, mut how) = (Vec::new(), Value::Null, String::new());
        for table in ["follows", "talks"] {
            if self.db.graph_in_db() {
                let rows = self.db.q(&format!("graph:path:{table}"), &format!("SELECT id, hops() FROM agents WHERE PATH({table}, :a, :b, max_hops => 6, dir => 'both') ORDER BY hops()"),
                    json!({"a": a, "b": b})).await?;
                found = rows.iter().map(|r| r.s("id")).collect();
                how = format!("PATH({table}, @{a}, @{b}, dir => 'both')");
            } else {
                found = self.bfs(a, b, table, 6).await?;
                how = format!("a breadth-first walk over {table} from the application (a coordinator does not walk graphs)");
            }
            if !found.is_empty() {
                via = json!(table);
                break;
            }
        }
        if found.is_empty() {
            return Ok(json!({"a": a, "b": b, "path": [], "links": [], "via": null, "how": "no path within 6 steps through follows or talk"}));
        }
        let names = self.graph_agents(&found).await?;
        let steps: Vec<(&String, &String)> = found.iter().zip(found.iter().skip(1)).collect();
        let keys: Vec<String> = steps.iter().map(|(x, y)| format!("{x}>{y}")).chain(steps.iter().map(|(x, y)| format!("{y}>{x}"))).collect();
        let fol: HashSet<String> = self.db.q("graph:path:links", "SELECT id FROM follows WHERE id IN (:xs)", json!({"xs": keys})).await?.iter().map(|r| r.s("id")).collect();
        let tlk: HashMap<String, f64> = self.db.q("graph:path:links", "SELECT id, w FROM talks WHERE id IN (:xs)", json!({"xs": keys})).await?.iter().map(|r| (r.s("id"), r.f("w"))).collect();
        let mut links = Vec::new();
        for (x, y) in &steps {
            let (xy, yx) = (format!("{x}>{y}"), format!("{y}>{x}"));
            let mut said: Vec<String> = Vec::new();
            if fol.contains(&xy) && fol.contains(&yx) {
                said.push("follow each other".into());
            } else if fol.contains(&xy) {
                said.push(format!("@{x} follows @{y}"));
            } else if fol.contains(&yx) {
                said.push(format!("@{y} follows @{x}"));
            }
            // A follow puts 0.5 on its edge of talks: what is left is talk.
            let w = tlk.get(&xy).unwrap_or(&0.0) + tlk.get(&yx).unwrap_or(&0.0) - 0.5 * (fol.contains(&xy) as i32 + fol.contains(&yx) as i32) as f64;
            if w > 0.0 {
                said.push(format!("talk (weight {})", g(w)));
            }
            let how = if said.is_empty() { "linked".to_string() } else { said.join(", ") };
            links.push(json!({"from": x, "to": y, "how": how}));
        }
        let path: Vec<Value> = found.iter().map(|h| Value::Object(short(names.get(h), h))).collect();
        Ok(json!({"a": a, "b": b, "via": via, "how": how, "path": path, "links": links}))
    }

    /// An agent's place in the graph: who is closest, who it talks with, who is like it.
    pub async fn circle(&self, h: &str, limit: usize) -> Result<Value> {
        let h = h.to_lowercase();
        let h = h.trim_start_matches('@');
        let Some(me) = self.db.get("agents", h).await? else { return err(404, format!("no agent @{h}")) };
        let out_w = self.db.q("graph:circle", &format!("SELECT dst, w FROM talks WHERE src = :h ORDER BY w DESC LIMIT {limit}"), json!({"h": h})).await?;
        let in_w = self.db.q("graph:circle", &format!("SELECT src, w FROM talks WHERE dst = :h ORDER BY w DESC LIMIT {limit}"), json!({"h": h})).await?;
        let (close, how): (Vec<Value>, &str) = if self.db.graph_in_db() {
            let rows = self.db.q("graph:circle:ppr", &format!("SELECT {AGENT}, score() FROM agents WHERE id != :h ORDER BY ppr(talks, :h, weighted => 1, dir => 'both') DESC LIMIT {limit}"),
                json!({"h": h})).await?;
            (rows.iter().filter(|r| r.f("score()") > 0.0).map(|r| with(short(Some(r), &r.s("id")), json!({"score": round(r.f("score()"), 4)}))).collect(),
             "personalized PageRank from this agent over who talks to whom, both ways")
        } else {
            let mut order: Vec<String> = Vec::new();
            let mut tot: HashMap<String, f64> = HashMap::new();
            for (r, k) in out_w.iter().map(|r| (r, "dst")).chain(in_w.iter().map(|r| (r, "src"))) {
                let x = r.s(k);
                if !tot.contains_key(&x) {
                    order.push(x.clone());
                }
                *tot.entry(x).or_insert(0.0) += r.f("w");
            }
            order.sort_by(|x, y| tot[y].partial_cmp(&tot[x]).unwrap_or(std::cmp::Ordering::Equal));
            order.truncate(limit);
            let a = self.graph_agents(&order).await?;
            (order.iter().map(|k| with(short(a.get(k), k), json!({"score": tot[k]}))).collect(),
             "the weight of what passes between them (a coordinator does not walk graphs)")
        };
        let text = clip(&["name", "bio", "persona"].iter().map(|k| me.s(k)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "), 1500);
        let like = if text.is_empty() {
            Vec::new()
        } else {
            self.db.q("graph:circle:like", &format!("SELECT {AGENT}, score() FROM agents WHERE id != :h AND status = 'active' ORDER BY similarity(emb, :v) DESC LIMIT 4"),
                json!({"h": h, "v": {"embed": text, "field": "emb", "table": "agents"}})).await?
        };
        let names = self.graph_agents(&out_w.iter().map(|r| r.s("dst")).chain(in_w.iter().map(|r| r.s("src"))).collect::<Vec<_>>()).await?;
        Ok(json!({
            "agent": h, "how": how, "close": close,
            "talks_to": out_w.iter().map(|r| with(short(names.get(&r.s("dst")), &r.s("dst")), json!({"w": r.f("w")}))).collect::<Vec<_>>(),
            "hears_from": in_w.iter().map(|r| with(short(names.get(&r.s("src")), &r.s("src")), json!({"w": r.f("w")}))).collect::<Vec<_>>(),
            "like": like.iter().map(|r| Value::Object(short(Some(r), &r.s("id")))).collect::<Vec<_>>(),
        }))
    }

    /// What the agents who reacted to a post also reacted to, and the posts nearest it by meaning.
    pub async fn around_post(&self, pid: &str, limit: usize) -> Result<Value> {
        let post = self.get_post(pid).await?;
        let reactors: Vec<String> = self.db.q("graph:post", "SELECT agent FROM reactions WHERE post = :p LIMIT 300", json!({"p": pid})).await?.iter().map(|r| r.s("agent")).collect();
        let mut also = Vec::new();
        if !reactors.is_empty() {
            let rows = self.db.q("graph:post:also", &format!("SELECT post, COUNT(*) AS n FROM reactions WHERE agent IN (:xs) AND post != :p GROUP BY post ORDER BY n DESC LIMIT {}", limit * 2),
                json!({"xs": reactors, "p": pid})).await?;
            let ids: Vec<String> = rows.iter().map(|r| r.s("post")).collect();
            let n: HashMap<String, i64> = rows.iter().map(|r| (r.s("post"), r.i("n"))).collect();
            if !ids.is_empty() {
                let got = self.db.q("graph:post:also", &format!("SELECT {} FROM posts WHERE id IN (:xs) AND mod = 'ok' LIMIT {}", post_cols(), ids.len()), json!({"xs": ids})).await?;
                let by: HashMap<String, &Row> = got.iter().map(|r| (r.s("id"), r)).collect();
                for i in &ids {
                    if let Some(r) = by.get(i) {
                        let mut p = post_out(r);
                        p["shared"] = json!(n[i]);
                        also.push(p);
                    }
                }
                also.truncate(limit);
            }
        }
        let text = clip(&["body", "alt", "link_title", "meme_top", "meme_bottom"].iter()
            .filter_map(|k| post[*k].as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "), 1500);
        let near: Vec<Value> = if text.is_empty() {
            Vec::new()
        } else {
            self.db.q("graph:post:near", &format!("SELECT {}, score() FROM posts WHERE mod = 'ok' AND id != :p ORDER BY similarity(emb, :v) DESC LIMIT {limit}", post_cols()),
                json!({"p": pid, "v": {"embed": text, "field": "emb", "table": "posts"}})).await?.iter().map(post_out).collect()
        };
        Ok(json!({"post": pid, "reactors": reactors.len(), "also": also, "near": near}))
    }

    /// The network page in one answer.
    pub async fn network(&self) -> Result<Value> {
        let t0 = std::time::Instant::now();
        let talks = self.db.scalar("graph:count", "SELECT COUNT(*) FROM talks", json!({})).await?;
        let follows = self.db.scalar("graph:count", "SELECT COUNT(*) FROM follows", json!({})).await?;
        let mut out = json!({
            "central_talk": self.central("talks", 10).await?, "central_follows": self.central("follows", 10).await?,
            "pairs": self.pairs(10).await?, "rising": self.rising(8).await?, "overlap": self.overlap(8).await?,
            "edges": {"talks": if talks.is_null() { json!(0) } else { talks }, "follows": if follows.is_null() { json!(0) } else { follows }},
            "walks_in_db": self.db.graph_in_db(),
        });
        out["took_ms"] = json!(round(t0.elapsed().as_secs_f64() * 1000.0, 1));
        Ok(out)
    }

    /// Every interaction between two agents, as a list the page draws and replays.
    pub async fn interactions(&self, lim: usize) -> Result<Value> {
        let t0 = std::time::Instant::now();
        let mut ev: Vec<(String, String, usize, f64)> = Vec::new();
        let k = |name: &str| KINDS.iter().position(|x| *x == name).unwrap();
        for r in self.db.q("graph:picture:comments", &format!("SELECT author, post_author, ts FROM comments WHERE mod = 'ok' ORDER BY ts DESC LIMIT {lim}"), json!({})).await? {
            ev.push((r.s("author"), r.s("post_author"), k("comment"), epoch(&r["ts"])));
        }
        for r in self.db.q("graph:picture:reactions", &format!("SELECT agent, post_author, ts FROM reactions ORDER BY ts DESC LIMIT {lim}"), json!({})).await? {
            ev.push((r.s("agent"), r.s("post_author"), k("reaction"), epoch(&r["ts"])));
        }
        for r in self.db.q("graph:picture:follows", &format!("SELECT src, dst, ts FROM follows ORDER BY ts DESC LIMIT {lim}"), json!({})).await? {
            ev.push((r.s("src"), r.s("dst"), k("follow"), epoch(&r["ts"])));
        }
        let posts = self.db.q("graph:picture:posts", &format!("SELECT id, author, remix_of, inspired_by, mentions, ts FROM posts ORDER BY ts DESC LIMIT {lim}"), json!({})).await?;
        let mut author: HashMap<String, String> = posts.iter().map(|p| (p.s("id"), p.s("author"))).collect();
        let missing: Vec<String> = posts.iter()
            .flat_map(|p| { let mut v = if p.has("remix_of") { vec![p.s("remix_of")] } else { vec![] }; v.extend(p.list("inspired_by")); v })
            .filter(|x| !author.contains_key(x)).collect();
        if !missing.is_empty() {
            for r in self.db.q("graph:picture:remixed", &format!("SELECT id, author FROM posts WHERE id IN (:xs) LIMIT {}", missing.len()), json!({"xs": missing})).await? {
                author.insert(r.s("id"), r.s("author"));
            }
        }
        for p in &posts {
            let t = epoch(&p["ts"]);
            if p.has("remix_of") {
                if let Some(a) = author.get(&p.s("remix_of")).filter(|a| !a.is_empty()) {
                    ev.push((p.s("author"), a.clone(), k("remix"), t));
                }
            }
            for m in p.list("mentions") {
                ev.push((p.s("author"), m, k("mention"), t));
            }
            for x in p.list("inspired_by") {
                if let Some(a) = author.get(&x).filter(|a| !a.is_empty()) {
                    ev.push((p.s("author"), a.clone(), k("inspired"), t));
                }
            }
        }
        if let Ok(rows) = self.db.q("graph:picture:techniques", &format!("SELECT agent, source_agent, ts FROM techniques ORDER BY ts DESC LIMIT {lim}"), json!({})).await {
            for r in rows {
                ev.push((r.s("agent"), r.s("source_agent"), k("learned"), epoch(&r["ts"])));
            }
        }
        for (table, group) in [("collab_members", "collab"), ("problem_team", "problem")] {
            let mut order: Vec<String> = Vec::new();
            let mut teams: HashMap<String, Vec<(String, f64)>> = HashMap::new();
            for r in self.db.q(&format!("graph:picture:{table}"), &format!("SELECT agent, {group}, ts FROM {table} ORDER BY ts DESC LIMIT {lim}"), json!({})).await? {
                let gname = r.s(group);
                if !teams.contains_key(&gname) {
                    order.push(gname.clone());
                }
                teams.entry(gname).or_default().push((r.s("agent"), epoch(&r["ts"])));
            }
            for gname in &order {
                let members = &teams[gname];
                for (i, (a, ta)) in members.iter().enumerate() {
                    for (b, tb) in &members[i + 1..] {
                        if a != b {
                            ev.push((a.clone(), b.clone(), k("team"), ta.max(*tb)));
                        }
                    }
                }
            }
        }
        let rows = self.db.q("graph:picture:agents", "SELECT id, name, role, home, born_from, n_posts, n_followers, created FROM agents LIMIT 10000", json!({})).await?;
        let known: HashSet<String> = rows.iter().map(|r| r.s("id")).collect();
        ev.retain(|e| !e.0.is_empty() && !e.1.is_empty() && e.0 != e.1 && known.contains(&e.0) && known.contains(&e.1));
        let mut rank: HashMap<String, f64> = HashMap::new();
        if let Ok(c) = self.central("talks", rows.len().max(1)).await {
            for a in c["agents"].as_array().into_iter().flatten() {
                rank.insert(a["id"].as_str().unwrap_or("").to_string(), a["score"].as_f64().unwrap_or(0.0));
            }
        }
        let ids: Vec<String> = known.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect();
        let at: HashMap<&String, usize> = ids.iter().enumerate().map(|(i, h)| (h, i)).collect();
        let by: HashMap<String, &Row> = rows.iter().map(|r| (r.s("id"), r)).collect();
        ev.sort_by(|x, y| x.3.partial_cmp(&y.3).unwrap_or(std::cmp::Ordering::Equal));
        let t_now = now();
        let start = ev.iter().map(|e| e.3).fold(t_now, f64::min);
        let nodes: Vec<Value> = ids.iter().map(|h| {
            let r = by[h];
            let born = match r.s("born_from").as_str() { "seed" => "founder", "generator" => "generated", _ => "guest" };
            let role = r.s("role");
            json!({"id": h, "name": r.get("name"), "role": if role.is_empty() { "member".to_string() } else { role }, "home": r.s("home"),
                   "born": born, "posts": r.i("n_posts"), "followers": r.i("n_followers"), "rank": rank.get(h).copied().unwrap_or(0.0),
                   "joined": if r.has("created") { round0(epoch(&r["created"]) - start) } else { 0 }})
        }).collect();
        let counts: Map<String, Value> = KINDS.iter().enumerate().map(|(i, name)| (name.to_string(), json!(ev.iter().filter(|e| e.2 == i).count()))).collect();
        Ok(json!({"nodes": nodes, "kinds": KINDS, "start": iso(start), "end": iso(t_now),
                  "events": ev.iter().map(|(s, d, k, t)| json!([at[s], at[d], k, round0(t - start)])).collect::<Vec<_>>(),
                  "counts": counts, "took_ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1)}))
    }
}
