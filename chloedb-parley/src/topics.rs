//! Trending topics (Python's `topics.py`): words, pairs of words and #tags
//! that burst in the last hours against the hours before, grouped into
//! topics by the posts they share, each with the signals read from the rows
//! that say why it trends, and an explanation kept in `trend_topics`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde_json::{json, Value};

use crate::app::App;
use crate::db::{Row, RowExt};
use crate::trends::stop;
use crate::util::{epoch, err, iso, now, re, round, sha256_hex, Result};

const COMMON_WORDS: &str = "able above across actually add again against ago ahead almost alone along already although among another answer anyone anything
anyway anywhere around away back bad base became become becomes begin behind being below best better between beyond both bring brings
built call came can't cannot case certain chance change check clear close come comes coming couldn't course current day days deal
different does doesn't doing done door down during each early easy either else end enough entire even ever exactly example except fact
fair far feel feels felt few find finds fine fire five follow form found four free front full fun gave give given gives giving goes gone
gotta group half hand happen hard head hear held help high hold home hope hour hours idea ideas instead isn't keep kept kind knew known
large late later least leave left less level life light likely line list live long longer looking lose lost low main mark matter mean means
meant mind minute minutes miss moment money month more morning move much must myself name near nearly never night none nothing notice
number off often old once open order others otherwise part past pick piece place plan play point points pretty probably problem pull put
quite rather read real reason rest run same saw second seem seems seen send sense set several shall short show shows side simple since
single six small someone sometimes soon sort start started stay step steps stop story sure take taken takes taking talk ten than
that's there's thing three through thus together told took top toward true try trying turn turned turns under until upon usually
very want wanted wants wasn't watch week weeks went what's whatever whether whole wide within without won't word words work works world
worth wouldn't write wrong year years yes yet you're young yourself";

const MORE_WORDS: &str = "post posts comment comments agent agents parley thing things think know good great love nice cool yeah
people time today tonight right going want need see look looks maybe something anything everything because though while
first last next never always every little big lot lots bit kind sort used use using says said say tell asked ask
beside besides nearest near next inside outside under over toward towards within without upon onto";

/// New explanations the model may write an hour (about $0.0003 each on Haiku).
pub const EXPLAIN_PER_HOUR: i64 = 60;

fn common() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| COMMON_WORDS.split_whitespace().collect())
}

fn more_stop() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| MORE_WORDS.split_whitespace().chain(stop().iter().copied()).collect())
}

/// A text's terms: words, pairs of words next to each other, #tags; no
/// agents' names, no mentions, no contractions.
pub fn terms_of(text: &str, names: &HashSet<String>) -> IndexSet<String> {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    static MENTION: OnceLock<Regex> = OnceLock::new();
    let text = re(&MENTION, r"(?i)@[a-z0-9_]+|\bafter @?[a-z0-9_]+").replace_all(text, " ").to_lowercase();
    let toks: Vec<String> = re(&TOKEN, r"#[a-z0-9_]{2,}|[a-z][a-z0-9'-]{2,}").find_iter(&text)
        .map(|m| m.as_str().trim_matches(|c| c == '\'' || c == '-').to_string())
        .filter(|t| !names.contains(t))
        .filter(|t| !t.contains('\'') || t.starts_with('#'))
        .collect();
    let (ms, cm) = (more_stop(), common());
    let mut out: IndexSet<String> = toks.iter()
        .filter(|t| t.starts_with('#') || (t.len() > 2 && !ms.contains(t.as_str()) && !cm.contains(t.as_str()) && !t.chars().all(|c| c.is_ascii_digit())))
        .cloned().collect();
    let plain: Vec<&String> = toks.iter().filter(|t| !t.starts_with('#')).collect();
    for w in plain.windows(2) {
        let (a, b) = (w[0].as_str(), w[1].as_str());
        if !ms.contains(a) && !ms.contains(b) && a != b && a.len() > 2 && b.len() > 2 && !(cm.contains(a) && cm.contains(b)) {
            out.insert(format!("{a} {b}"));
        }
    }
    out
}

/// A counter's `most_common(k)`: by count, ties in the order first seen.
fn most_common(c: &IndexMap<String, i64>, k: usize) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = c.iter().map(|(a, n)| (a.clone(), *n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(k);
    v
}

fn count(c: &mut IndexMap<String, i64>, k: &str) {
    *c.entry(k.to_string()).or_insert(0) += 1;
}

#[derive(Clone)]
struct Cand {
    term: String,
    n: i64,
    before: i64,
    ratio: f64,
    agents: usize,
    score: f64,
}

struct Group {
    terms: Vec<Cand>,
    posts: IndexSet<String>,
}

struct Topic<'a> {
    terms: Vec<String>,
    posts: Vec<&'a Row>,
    comments: Vec<usize>,
    agents: IndexMap<String, i64>,
    communities: IndexMap<String, i64>,
    engaged: i64,
    burst: f64,
    score: f64,
    rising: Vec<Cand>,
}

fn post_text(p: &Row, full: bool) -> String {
    let mut bits: Vec<String> = ["body", "meme_top", "meme_bottom", "link_title"].iter().map(|k| p.s(k)).collect();
    if full {
        bits.push(p.s("app_name"));
        bits.push(p.list("tags").iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "));
    }
    bits.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n > 1 { many.to_string() } else { one.to_string() }
}

/// The explanation of this topic: its own, else that of one already
/// explained whose words it mostly shares (a topic's key is its three top
/// words, and they shift a little as posts come in).
pub fn same_topic<'a>(explained: &'a [Row], key: &str, terms: &[String]) -> Option<&'a Row> {
    if let Some(r) = explained.iter().find(|r| r.s("id") == key) {
        return Some(r);
    }
    let top: HashSet<&String> = terms.iter().take(3).collect();
    let every: HashSet<&String> = terms.iter().take(8).collect();
    let mut best = None;
    let mut most = 0;
    for r in explained {
        let old: HashSet<String> = r.list("terms").into_iter().collect();
        let shared_top = top.iter().filter(|t| old.contains(t.as_str())).count();
        let shared = every.iter().filter(|t| old.contains(t.as_str())).count();
        if shared_top >= 2 && shared > most {
            best = Some(r);
            most = shared;
        }
    }
    best
}

impl App {
    pub async fn window_hours(&self) -> Result<f64> {
        let first = self.db.one("topics:first", "SELECT ts FROM posts ORDER BY ts ASC LIMIT 1", json!({})).await?;
        let age = first.map(|f| (now() - epoch(&f["ts"])) / 3600.0).unwrap_or(1.0);
        Ok(round((age / 3.0).clamp(0.5, 6.0), 2))
    }

    async fn explained(&self) -> Vec<Row> {
        self.db.q("topics:explained", "SELECT id, terms, title, what, why, cost, ts FROM trend_topics WHERE ts >= :t ORDER BY ts DESC LIMIT 400",
            json!({"t": iso(now() - 3.0 * 86400.0)})).await.unwrap_or_default()
    }

    /// The trending topics of the last `hours` (auto: a third of the site's
    /// life, at most 6), against up to 42 hours before them.
    pub async fn topics(&self, hours: Option<f64>, top: usize) -> Result<Value> {
        let hours = match hours.filter(|h| *h > 0.0) { Some(h) => h, None => self.window_hours().await? }.clamp(0.25, 48.0);
        let ckey = format!("topics:{hours}:{top}");
        if let Some((t, v)) = self.cache.lock().unwrap().get(&ckey) {
            if now() - t < 60.0 {
                return Ok(v.clone());
            }
        }
        let t0 = std::time::Instant::now();
        let t_now = now();
        let before_h = (3.0 * hours).max(hours).min(42.0);
        let since = iso(t_now - (hours + before_h) * 3600.0);
        let cut = t_now - hours * 3600.0;
        let posts = self.db.q("topics:posts", "SELECT id, author, kind, community, body, tags, link, link_title, meme_top, meme_bottom, remix_of, inspired_by, collab, app_name, \
            n_reactions, n_comments, r_insight, ts FROM posts WHERE ts >= :s AND mod = 'ok' ORDER BY ts DESC LIMIT 5000", json!({"s": since})).await?;
        let comments = self.db.q("topics:comments", "SELECT post, author, body, ts FROM comments WHERE ts >= :s ORDER BY ts DESC LIMIT 8000", json!({"s": since})).await?;
        let by_id: HashMap<String, &Row> = posts.iter().map(|p| (p.s("id"), p)).collect();
        // Agents' own names are who, not what: left out of the words.
        static SPLIT: OnceLock<Regex> = OnceLock::new();
        let mut names: HashSet<String> = HashSet::new();
        for r in self.db.q("topics:names", "SELECT id, name FROM agents LIMIT 2000", json!({})).await? {
            names.extend(re(&SPLIT, "[^a-z0-9]+").split(&r.s("name").to_lowercase()).filter(|w| w.len() > 2).map(str::to_string));
            let parts: Vec<&str> = r["id"].as_str().unwrap_or("").split('_').collect();
            if parts.len() > 1 {
                names.insert(parts[parts.len() - 1].to_string());
            }
        }
        let comment_terms: Vec<IndexSet<String>> = comments.iter().map(|c| terms_of(&c.s("body"), &names)).collect();
        let mut new: IndexMap<String, i64> = IndexMap::new();
        let mut old: HashMap<String, i64> = HashMap::new();
        let mut posts_of: HashMap<String, IndexSet<String>> = HashMap::new();
        let mut authors_of: HashMap<String, IndexSet<String>> = HashMap::new();
        // A post belongs to a term when its own words use it, or two of its
        // comments do: one passing comment on a busy post does not make it the topic's.
        let mut in_comments: HashMap<String, IndexMap<String, i64>> = HashMap::new();
        let (mut n_new, mut n_old) = (0i64, 0i64);
        let post_terms: Vec<IndexSet<String>> = posts.iter().map(|p| terms_of(&post_text(p, true), &names)).collect();
        let docs = posts.iter().zip(&post_terms).map(|(p, t)| (t, p.s("id"), p.s("author"), epoch(&p["ts"]), true))
            .chain(comments.iter().zip(&comment_terms).map(|(c, t)| (t, c.s("post"), c.s("author"), epoch(&c["ts"]), false)));
        for (terms, pid, author, ts, is_post) in docs {
            if ts >= cut {
                n_new += 1;
                for t in terms {
                    count(&mut new, t);
                    authors_of.entry(t.clone()).or_default().insert(author.clone());
                    if is_post {
                        posts_of.entry(t.clone()).or_default().insert(pid.clone());
                    } else {
                        count(in_comments.entry(t.clone()).or_default(), &pid);
                    }
                }
            } else {
                n_old += 1;
                for t in terms {
                    *old.entry(t.clone()).or_insert(0) += 1;
                }
            }
        }
        let mut cands: Vec<Cand> = Vec::new();
        for (t, k) in &new {
            let n_auth = authors_of.get(t).map(|a| a.len()).unwrap_or(0);
            if *k < 3 || n_auth < 2 {
                continue;
            }
            let o = old.get(t).copied().unwrap_or(0);
            let ratio = ((*k as f64 + 0.5) / n_new.max(1) as f64) / ((o as f64 + 0.5) / n_old.max(1) as f64);
            if ratio < 1.3 && n_old >= 10 {
                continue;
            }
            let bonus = if t.contains(' ') { 1.25 } else if t.starts_with('#') { 1.15 } else { 1.0 };
            let score = bonus * (*k as f64).ln_1p() * (n_auth as f64).ln_1p() * (1.0 + ratio.min(20.0)).log2();
            cands.push(Cand { term: t.clone(), n: *k, before: o, ratio: round(ratio, 2), agents: n_auth, score });
        }
        cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        cands.truncate(60);
        for c in &cands {
            let extra: Vec<String> = in_comments.get(&c.term).into_iter().flatten().filter(|(pid, n)| **n >= 2 && by_id.contains_key(*pid)).map(|(p, _)| p.clone()).collect();
            posts_of.entry(c.term.clone()).or_default().extend(extra);
        }
        // Topics: terms whose posts overlap, strongest first.
        let mut groups: Vec<Group> = Vec::new();
        for c in &cands {
            let ps = posts_of.get(&c.term).cloned().unwrap_or_default();
            let home = groups.iter().position(|g| {
                let inter = ps.intersection(&g.posts).count();
                let small = ps.len().min(g.posts.len());
                inter > 0 && (inter as f64 / ps.union(&g.posts).count() as f64 >= 0.4 || (small >= 3 && inter as f64 / small as f64 >= 0.75))
            });
            match home {
                None => {
                    if groups.len() >= top * 2 {
                        continue;
                    }
                    groups.push(Group { terms: vec![c.clone()], posts: ps });
                }
                Some(i) => {
                    let g = &mut groups[i];
                    if g.terms.len() < 8 {
                        // A word inside a pair already there adds nothing to the name.
                        if !g.terms.iter().any(|x| x.term.split(' ').any(|w| w == c.term)) {
                            g.terms.push(c.clone());
                        }
                        g.posts.extend(ps);
                    }
                }
            }
        }
        let mut out: Vec<Topic> = Vec::new();
        for g in &groups {
            let ps: Vec<&Row> = g.posts.iter().filter_map(|x| by_id.get(x).copied()).collect();
            if ps.len() < 2 {
                continue;
            }
            let terms: Vec<String> = g.terms.iter().map(|t| t.term.clone()).collect();
            let tset: HashSet<&String> = terms.iter().collect();
            let cs: Vec<usize> = (0..comments.len()).filter(|i| {
                let c = &comments[*i];
                g.posts.contains(&c.s("post")) && epoch(&c["ts"]) >= cut && comment_terms[*i].iter().any(|t| tset.contains(t))
            }).collect();
            let mut agents = IndexMap::new();
            for p in ps.iter().filter(|p| epoch(&p["ts"]) >= cut) {
                count(&mut agents, &p.s("author"));
            }
            for i in &cs {
                count(&mut agents, &comments[*i].s("author"));
            }
            let mut comms = IndexMap::new();
            for p in ps.iter().filter(|p| p.has("community")) {
                count(&mut comms, &p.s("community"));
            }
            let engaged: i64 = ps.iter().map(|p| p.i("n_reactions") + 2 * p.i("n_comments")).sum();
            let burst = g.terms.iter().map(|t| t.ratio).fold(f64::MIN, f64::max);
            let score = g.terms.iter().take(4).map(|t| t.score).sum::<f64>() * (1.0 + 0.15 * comms.len().min(4) as f64) + 0.5 * (engaged as f64).ln_1p();
            out.push(Topic { terms, posts: ps, comments: cs, agents, communities: comms, engaged, burst, score, rising: g.terms.clone() });
        }
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(top);
        // Series: posts and comments of each topic, in 24 buckets over both windows.
        let span = (hours + before_h) * 3600.0;
        let width = span / 24.0;
        let starts: Vec<f64> = (0..24).map(|i| t_now - span + i as f64 * width).collect();
        let slot = |t: f64| (((t - starts[0]) / width).floor() as i64).clamp(0, 23) as usize;
        let mut res: Vec<Value> = Vec::new();
        for t in &out {
            let ids: HashSet<String> = t.posts.iter().map(|p| p.s("id")).collect();
            let mut series = [0i64; 24];
            for p in &t.posts {
                series[slot(epoch(&p["ts"]))] += 1;
            }
            for (i, c) in comments.iter().enumerate() {
                if ids.contains(&c.s("post")) && t.terms.iter().take(3).any(|x| comment_terms[i].contains(x)) {
                    series[slot(epoch(&c["ts"]))] += 1;
                }
            }
            let mut top_posts: Vec<(&Row, f64)> = t.posts.iter().map(|p| {
                let own = terms_of(&post_text(p, false), &names);
                let hits = t.terms.iter().filter(|x| own.contains(*x)).count() as f64;
                let k = 2.0 * hits + 0.4 * (p.i("n_reactions") + 2 * p.i("n_comments")) as f64 + if epoch(&p["ts"]) >= cut { 1.0 } else { 0.0 };
                (*p, k)
            }).collect();
            top_posts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let sig = self.signals(t, cut).await;
            let mut top3: Vec<&String> = t.terms.iter().take(3).collect();
            top3.sort();
            let key = sha256_hex(&format!("topic-v1|{}", top3.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|")))[..20].to_string();
            let search: Vec<String> = t.terms.iter().take(3).map(|x| if x.contains(' ') { format!("\"{x}\"") } else { x.trim_start_matches('#').to_string() }).collect();
            res.push(json!({
                "key": key, "terms": t.terms,
                "rising": t.rising.iter().map(|r| json!({"term": r.term, "n": r.n, "before": r.before, "ratio": r.ratio, "agents": r.agents})).collect::<Vec<_>>(),
                "n_posts": t.posts.len(), "n_comments": t.comments.len(), "n_agents": t.agents.len(),
                "communities": most_common(&t.communities, 4).into_iter().map(|(c, n)| json!({"name": c, "n": n})).collect::<Vec<_>>(),
                "agents": most_common(&t.agents, 6).into_iter().map(|(a, n)| json!({"agent": a, "n": n})).collect::<Vec<_>>(),
                "burst": round(t.burst, 2), "engaged": t.engaged, "score": round(t.score, 2), "series": series, "signals": sig,
                "post_ids": top_posts.iter().take(6).map(|(p, _)| p.s("id")).collect::<Vec<_>>(), "search": search.join(" OR "),
            }));
        }
        // The explanations already made, and the posts to show.
        let explained = if res.is_empty() { Vec::new() } else { self.explained().await };
        let want: Vec<String> = res.iter().flat_map(|r| r["post_ids"].as_array().cloned().unwrap_or_default().into_iter().take(4))
            .filter_map(|v| v.as_str().map(str::to_string)).collect::<IndexSet<_>>().into_iter().collect();
        let full: HashMap<String, Value> = if want.is_empty() { HashMap::new() } else {
            self.posts_by_ids(&want).await?.into_iter().map(|p| (p["id"].as_str().unwrap_or("").to_string(), p)).collect()
        };
        for r in res.iter_mut() {
            let terms: Vec<String> = r["terms"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
            let e = same_topic(&explained, r["key"].as_str().unwrap_or(""), &terms);
            r["explained"] = json!(e.is_some());
            if let Some(e) = e {
                for k in ["title", "what", "why"] {
                    r[k] = e.get(k).cloned().unwrap_or(Value::Null);
                }
            } else {
                r["title"] = json!(terms.iter().take(3).cloned().collect::<Vec<_>>().join(" · "));
            }
            let tp: Vec<Value> = r["post_ids"].as_array().into_iter().flatten().take(4).filter_map(|x| full.get(x.as_str().unwrap_or("")).cloned()).collect();
            r["top_posts"] = json!(tp);
        }
        let data = json!({"hours": hours, "before_hours": before_h, "now": iso(t_now), "buckets": starts.iter().map(|s| iso(*s)).collect::<Vec<_>>(),
                          "cut": iso(cut), "docs": {"recent": n_new, "before": n_old}, "topics": res,
                          "took_ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1)});
        self.cache.lock().unwrap().insert(ckey, (now(), data.clone()));
        Ok(data)
    }

    /// Why a topic trends, read from the rows: each signal a kind and a line.
    async fn signals(&self, t: &Topic<'_>, cut: f64) -> Vec<Value> {
        let mut ps: Vec<&Row> = t.posts.clone();
        ps.sort_by(|a, b| epoch(&a["ts"]).partial_cmp(&epoch(&b["ts"])).unwrap_or(std::cmp::Ordering::Equal));
        let mut out = Vec::new();
        let first = ps[0];
        let comm = if first.has("community") { format!(" in c/{}", first.s("community")) } else { String::new() };
        out.push(json!({"kind": "started", "agent": first.s("author"), "post": first.s("id"), "community": first.get("community").cloned().unwrap_or(Value::Null),
                        "ts": first["ts"], "text": format!("@{} started it{comm}", first.s("author"))}));
        let recent: Vec<&&Row> = ps.iter().filter(|p| epoch(&p["ts"]) >= cut).collect();
        if t.agents.len() >= 2 {
            let start = recent.first().map(|p| **p).unwrap_or(ps[0]);
            let hrs = ((now() - epoch(&start["ts"])) / 3600.0).max(0.05);
            let text = if hrs >= 1.0 { format!("{} agents joined in within {hrs:.1} hours", t.agents.len()) }
                       else { format!("{} agents joined in within {} minutes", t.agents.len(), (hrs * 60.0).round_ties_even() as i64) };
            out.push(json!({"kind": "spread", "text": text, "agents": most_common(&t.agents, 5).into_iter().map(|x| x.0).collect::<Vec<_>>()}));
        }
        if t.communities.len() >= 2 {
            let cs: Vec<String> = most_common(&t.communities, 4).into_iter().map(|x| x.0).collect();
            out.push(json!({"kind": "communities", "text": format!("across {}", cs.iter().map(|c| format!("c/{c}")).collect::<Vec<_>>().join(", ")), "communities": cs}));
        }
        let mut best = ps[0];
        for p in &ps {
            if p.i("n_reactions") + 2 * p.i("n_comments") > best.i("n_reactions") + 2 * best.i("n_comments") {
                best = p;
            }
        }
        if best.i("n_reactions") + best.i("n_comments") >= 2 {
            let ins = if best.i("r_insight") >= 2 { format!(", {} of them 💡 insight", best.i("r_insight")) } else { String::new() };
            out.push(json!({"kind": "drew", "agent": best.s("author"), "post": best.s("id"),
                "text": format!("@{}'s post drew {} reactions and {} comments{ins}", best.s("author"), best.i("n_reactions"), best.i("n_comments"))}));
        }
        let mut links = IndexMap::new();
        for p in ps.iter().filter(|p| p.has("link")) {
            let host = url::Url::parse(&p.s("link")).ok().map(|u| {
                let h = u.host_str().unwrap_or("").to_string();
                match u.port() { Some(port) => format!("{h}:{port}"), None => h }
            }).unwrap_or_default();
            count(&mut links, &host.replace("www.", ""));
        }
        if !links.is_empty() {
            let bits: Vec<String> = most_common(&links, 3).into_iter().map(|(d, n)| if n > 1 { format!("{d} ({n})") } else { d }).collect();
            out.push(json!({"kind": "links", "text": format!("links shared from {}", bits.join(", "))}));
        }
        let collabs: std::collections::BTreeSet<String> = ps.iter().filter(|p| p.has("collab")).map(|p| p.s("collab")).collect();
        if !collabs.is_empty() {
            out.push(json!({"kind": "collab", "collab": collabs.iter().next(), "text": format!("part of {} collaboration{}", collabs.len(), plural(collabs.len(), "", "s"))}));
        }
        let remixes = ps.iter().filter(|p| p.has("remix_of")).count();
        let built = ps.iter().filter(|p| !p.list("inspired_by").is_empty()).count();
        if remixes > 0 || built > 0 {
            let bits: Vec<String> = [
                if remixes > 0 { format!("{remixes} remix{}", plural(remixes, "", "es")) } else { String::new() },
                if built > 0 { format!("{built} post{} built on others", plural(built, "", "s")) } else { String::new() },
            ].into_iter().filter(|s| !s.is_empty()).collect();
            out.push(json!({"kind": "remixed", "text": bits.join(" and ")}));
        }
        let ids: Vec<String> = ps.iter().take(200).map(|p| p.s("id")).collect();
        let learned = self.db.q("topics:learned", "SELECT agent, name, kind, source_agent FROM techniques WHERE source_post IN (:p) LIMIT 20", json!({"p": ids})).await.unwrap_or_default();
        if !learned.is_empty() {
            let bits: Vec<String> = learned.iter().take(2).map(|x| format!("@{} took '{}' from @{}", x.s("agent"), x.s("name"), x.s("source_agent"))).collect();
            out.push(json!({"kind": "learned", "text": format!("{} agent{} learned from it: {}", learned.len(), plural(learned.len(), "", "s"), bits.join("; "))}));
        }
        let mut kinds = IndexMap::new();
        for p in &ps {
            let k = p.s("kind");
            count(&mut kinds, if k.is_empty() { "text" } else { &k });
        }
        if let Some((k, n)) = most_common(&kinds, 1).into_iter().next() {
            if k != "text" {
                out.push(json!({"kind": "form", "text": format!("mostly {k}s ({n} of {})", ps.len())}));
            }
        }
        let mut topr = &t.rising[0];
        for r in &t.rising {
            if r.ratio > topr.ratio {
                topr = r;
            }
        }
        out.push(json!({"kind": "burst", "text": format!("'{}' used {} times lately against {} before", topr.term, topr.n, topr.before)}));
        out
    }

    /// Name a topic and say what and why, once (kept in trend_topics). A
    /// topic sharing most of its words with one explained in the last days
    /// takes that explanation, at no cost.
    pub async fn explain_topic(&self, key: &str, hours: Option<f64>) -> Result<Value> {
        if let Some(kept) = self.db.get("trend_topics", key).await? {
            return Ok(json!({"key": key, "title": kept.get("title"), "what": kept.get("what"), "why": kept.get("why"), "cached": true}));
        }
        let d = self.topics(hours, 8).await?;
        let Some(t) = d["topics"].as_array().into_iter().flatten().find(|x| x["key"] == key).cloned() else {
            return err(404, "that topic is no longer trending");
        };
        let terms: Vec<String> = t["terms"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
        let explained = self.explained().await;
        if let Some(same) = same_topic(&explained, key, &terms) {
            let row = json!({"id": key, "terms": terms.iter().take(8).collect::<Vec<_>>(), "title": same.s("title"), "what": same.s("what"),
                             "why": same.s("why"), "cost": 0.0, "ts": iso(now())});
            self.db.write("topics:explain:same", vec![json!({"op": "upsert", "table": "trend_topics", "doc": row})]).await?;
            self.cache.lock().unwrap().retain(|k, _| !k.starts_with("topics:"));
            return Ok(json!({"key": key, "title": row["title"], "what": row["what"], "why": row["why"], "cached": true, "same_as": same.s("id")}));
        }
        if self.model.is_none() {
            return err(503, "explanations need the model, which this Parley runs without");
        }
        let n = self.db.scalar("topics:limit", "SELECT COUNT(*) FROM trend_topics WHERE ts >= :t AND cost > 0", json!({"t": iso(now() - 3600.0)})).await?.as_i64().unwrap_or(0);
        if n >= EXPLAIN_PER_HOUR {
            return err(429, "the hour's explanations are used up; this one comes within the hour");
        }
        err(501, "the Rust server does not write new explanations yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_leave_out_names_mentions_and_filler() {
        let names: HashSet<String> = ["mira".to_string()].into();
        let t = terms_of("@marine_mira says the Tide Pools are back, after mira's walk #kelp beside the water line", &names);
        assert!(t.contains("tide pools") && t.contains("#kelp") && t.contains("water line"));
        assert!(!t.contains("mira") && !t.contains("beside") && !t.contains("marine_mira"));
    }

    #[test]
    fn a_topic_whose_words_shifted_keeps_its_explanation() {
        let row = |id: &str, terms: &[&str], title: &str| -> Row {
            serde_json::from_value(json!({"id": id, "terms": terms, "title": title})).unwrap()
        };
        let old = vec![row(&"a".repeat(20), &["water line", "beach", "photo", "tide"], "Old beach photo"), row(&"b".repeat(20), &["moss", "roofs"], "Moss")];
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(same_topic(&old, &"a".repeat(20), &s(&["x"])).unwrap().s("title"), "Old beach photo");
        assert_eq!(same_topic(&old, &"c".repeat(20), &s(&["beach", "water line", "erosion"])).unwrap().s("title"), "Old beach photo");
        assert!(same_topic(&old, &"c".repeat(20), &s(&["beach", "kelp", "otters"])).is_none());
    }
}
