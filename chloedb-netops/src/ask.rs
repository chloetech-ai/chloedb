//! A question about the estate, in a sentence, answered by one query.
//!
//! "All nodes changed in the last week where the change was about a
//! firmware upgrade, done by m.rossi, with latency above 40 ms, behind
//! ag1-fra" names five kinds of condition: on an object's own columns
//! (kind, state, vendor, place, its latest readings), on the words of what
//! happened to it (`MATCH` over two months of changes and faults), on who
//! did it and when, and on where it sits in the graph (`REACHABLE` over
//! the edge table). ChloeDB answers all five in one statement: the events
//! are a subquery whose values the objects are looked up by, the graph
//! walk is a predicate beside the comparisons, and the planner picks the
//! order.
//!
//! The sentence becomes a [`Plan`], and only a plan becomes SQL
//! ([`compile`]): every value in it is checked against what the estate
//! has, or passed as a parameter. Two readers make plans. A model, asked
//! through the server's own `llm()` over the row the question was saved
//! as, when the server has one (`--rewrite-model`); and a reader of fixed
//! vocabulary ([`read`]), which is what answers when it has not. The
//! answer says which read it and shows the plan and the SQL.

use std::sync::atomic::{AtomicI64, Ordering};

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Map, Value};

use crate::api::{bad, Answer, Q, Shared};
use crate::estate::{EDGES, EVENTS, NODES, OPERATORS};
use crate::sim::Engine;
use crate::strata::{Row, RowExt};
use crate::telemetry::iso;

pub fn routes() -> Router<Shared> {
    Router::new().route("/v1/map/ask", get(ask)).route("/v1/map/asks", get(asks))
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Reading {
    /// `latency_ms` or `loss`.
    pub field: &'static str,
    /// `>` or `<`.
    pub op: &'static str,
    /// The number, or `None` for "more than twice its usual".
    pub value: Option<f64>,
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Happened {
    /// `change`, `fault`, or empty for either.
    pub event: String,
    pub about: String,
    pub author: String,
    pub cause: String,
    pub hours: f64,
    /// Objects this did not happen to.
    pub negate: bool,
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Walk {
    /// `out` (what depends on it), `in` (what it depends on) or `both`.
    pub dir: String,
    pub from: String,
    pub hops: i64,
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Plan {
    pub kinds: Vec<String>,
    /// `down`, `degraded`, `impacted`, `ok`, or `not ok`.
    pub states: Vec<String>,
    pub vendor: String,
    pub version: String,
    pub customer: String,
    pub region: String,
    pub metro: String,
    pub hub: String,
    pub readings: Vec<Reading>,
    pub happened: Option<Happened>,
    pub walk: Option<Walk>,
    /// `latency`, `loss`, or empty.
    pub order: String,
}

/// What a plan may name.
pub struct Vocabulary {
    /// (id, names it goes by, all lower case).
    pub metros: Vec<(String, Vec<String>)>,
    pub regions: Vec<(String, Vec<String>)>,
    pub vendors: Vec<String>,
}

pub const KINDS: &[(&str, &[&str])] = &[
    ("olt", &["olt", "olts", "cabinet", "cabinets"]),
    ("cell", &["cell site", "cell sites", "cell router", "cell routers", "cell-site"]),
    ("switch", &["switch", "switches", "access switch", "access switches"]),
    ("hub-router", &["hub router", "hub routers", "hub-router", "hub-routers", "aggregation router", "aggregation routers", "metro router", "metro routers"]),
    ("power", &["power plant", "power plants", "rectifier", "rectifiers"]),
    ("ont", &["ont", "onts"]),
    ("radio", &["radio", "radios", "sector", "sectors"]),
    ("cpe", &["cpe", "cpes", "customer router", "customer routers"]),
    ("core", &["core router", "core routers"]),
    ("border", &["border router", "border routers"]),
    ("agg", &["agg router", "agg routers"]),
    ("firewall", &["firewall", "firewalls"]),
    ("spine", &["spine", "spines"]),
    ("leaf", &["leaf", "leaves", "leafs"]),
];
const CAUSES: &[(&str, &[&str])] = &[
    ("fibre", &["fibre cut", "fiber cut", "fibre", "fiber"]),
    ("power", &["power", "mains", "battery"]),
    ("software", &["software", "firmware", "upgrade"]),
    ("hardware", &["hardware", "line card", "optic"]),
    ("config", &["config", "configuration", "misconfiguration"]),
    ("weather", &["weather", "rain"]),
    ("congestion", &["congestion", "congested"]),
];
const STOP: &[&str] = &["and", "with", "where", "by", "in", "within", "whose", "that", "done", "which", "on", "from", "behind", "near", "during", "over", "since", "was", "were", "is", "are", "the", "a", "an", "having", "has", "have"];

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '?' | '!' | '"' | '\u{201c}' | '\u{201d}'))
        .map(|w| w.trim_matches(|c: char| matches!(c, '.' | ':' | '\'')).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Where `phrase` (words) starts in `w`.
fn at(w: &[String], phrase: &str) -> Option<usize> {
    let p: Vec<&str> = phrase.split(' ').collect();
    (0..w.len().saturating_sub(p.len() - 1)).find(|&i| p.iter().enumerate().all(|(k, x)| w[i + k] == *x))
}

fn has(w: &[String], phrases: &[&str]) -> bool {
    phrases.iter().any(|p| at(w, p).is_some())
}

fn number(s: &str) -> Option<f64> {
    let t = s.trim_end_matches("ms").trim_end_matches('%');
    if t.is_empty() { None } else { t.parse().ok() }
}

/// The reader of fixed vocabulary: what it recognises it takes, and the
/// rest of the sentence it leaves.
pub fn read(text: &str, v: &Vocabulary, known: &dyn Fn(&str) -> bool) -> Plan {
    let w = words(text);
    let mut p = Plan::default();
    let mut used = vec![false; w.len()];
    let mark = |i: usize, n: usize, used: &mut Vec<bool>| (i..(i + n).min(used.len())).for_each(|k| used[k] = true);

    // When: "last 3 days", "past week", "today", "yesterday".
    let mut hours = None;
    for i in 0..w.len() {
        if matches!(w[i].as_str(), "last" | "past" | "previous") {
            let unit = |u: &str| match u.trim_end_matches('s') {
                "minute" | "min" => Some(1.0 / 60.0),
                "hour" | "hr" | "h" => Some(1.0),
                "day" | "d" => Some(24.0),
                "week" | "wk" => Some(168.0),
                "month" => Some(720.0),
                _ => None,
            };
            if let Some(u) = w.get(i + 1).and_then(|x| unit(x)) {
                hours = Some(u);
                mark(i, 2, &mut used);
            } else if let (Some(n), Some(u)) = (w.get(i + 1).and_then(|x| number(x).or(match x.as_str() { "two" => Some(2.0), "three" => Some(3.0), "four" => Some(4.0), "six" => Some(6.0), _ => None })), w.get(i + 2).and_then(|x| unit(x))) {
                hours = Some(n * u);
                mark(i, 3, &mut used);
            }
        } else if w[i] == "today" {
            hours = Some(24.0);
            mark(i, 1, &mut used);
        } else if w[i] == "yesterday" {
            hours = Some(48.0);
            mark(i, 1, &mut used);
        } else if w[i] == "this" && matches!(w.get(i + 1).map(String::as_str), Some("week")) {
            hours = Some(168.0);
            mark(i, 2, &mut used);
        }
    }

    // Readings: "latency greater than 40", "loss above 1%", "high latency".
    for (names, field) in [(&["latency", "delay", "rtt"][..], "latency_ms"), (&["loss"][..], "loss")] {
        let Some(i) = w.iter().position(|x| names.contains(&x.as_str())) else { continue };
        mark(i, 1, &mut used);
        let mut op = None;
        let mut value = None;
        for k in i + 1..(i + 8).min(w.len()) {
            match w[k].as_str() {
                "greater" | "more" | "above" | "over" | "higher" | "exceeds" | "exceeding" | ">" | ">=" | "least" => op = Some(">"),
                "less" | "below" | "under" | "lower" | "<" | "<=" | "most" => op = Some("<"),
                x => {
                    if let Some(n) = number(x.trim_start_matches(['>', '<', '='])) {
                        if x.starts_with('<') {
                            op = Some("<");
                        }
                        value = Some(n);
                        mark(i + 1, k - i, &mut used);
                        break;
                    }
                    if STOP.contains(&x) && !matches!(x, "is" | "was" | "are" | "were" | "the" | "a" | "over") {
                        break;
                    }
                }
            }
        }
        let high = i > 0 && matches!(w[i - 1].as_str(), "high" | "higher" | "elevated" | "bad" | "slow");
        if value.is_some() || high {
            if high {
                mark(i - 1, 1, &mut used);
            }
            p.readings.push(Reading { field, op: op.unwrap_or(">"), value });
        }
        if field == "latency_ms" {
            p.order = "latency".into();
        } else if p.order.is_empty() {
            p.order = "loss".into();
        }
    }
    if has(&w, &["slowest", "slow"]) && p.order.is_empty() {
        p.order = "latency".into();
    }

    // Who: "by operator m.rossi", "done by Rossi".
    let mut author = String::new();
    for (i, x) in w.iter().enumerate() {
        if let Some(op) = OPERATORS.iter().find(|o| **o == x.as_str() || o.split('.').nth(1) == Some(x.as_str())) {
            author = op.to_string();
            mark(i, 1, &mut used);
            if i > 0 && matches!(w[i - 1].as_str(), "operator" | "engineer" | "user") {
                mark(i - 1, 1, &mut used);
            }
        }
    }
    if author.is_empty() {
        if let Some(i) = at(&w, "by operator").or_else(|| at(&w, "done by")).or_else(|| at(&w, "made by")).or_else(|| at(&w, "changed by")) {
            if let Some(x) = w.get(i + 2).filter(|x| !STOP.contains(&x.as_str())) {
                author = x.clone();
                mark(i, 3, &mut used);
            }
        }
    }

    // What happened.
    let changed = w.iter().position(|x| matches!(x.as_str(), "changed" | "change" | "changes" | "modified" | "touched" | "upgraded" | "reconfigured" | "pushed" | "unchanged"));
    let failed = w.iter().enumerate().position(|(i, x)| matches!(x.as_str(), "failed" | "fault" | "faults" | "failure" | "failures" | "broke" | "outage" | "outages" | "alarm" | "alarms") || (x == "down" && i > 0 && matches!(w[i - 1].as_str(), "went" | "gone" | "going")));
    let mut event = String::new();
    if let Some(i) = changed {
        event = "change".into();
        mark(i, 1, &mut used);
    } else if let Some(i) = failed {
        event = "fault".into();
        mark(i, 1, &mut used);
    } else if !author.is_empty() {
        event = "change".into();
    }
    let negate = has(&w, &["not changed", "not been changed", "unchanged", "never changed", "without a change", "without change", "no change", "no changes", "not failed", "never failed"]);
    // About what: the words after "about", or between quotes.
    let mut about = String::new();
    if let (Some(a), Some(b)) = (text.find(['"', '\u{201c}']), text.rfind(['"', '\u{201d}'])) {
        if b > a {
            about = text[a..b].trim_matches(['"', '\u{201c}', '\u{201d}']).trim().to_lowercase();
        }
    }
    if about.is_empty() {
        for lead in ["about", "regarding", "concerning", "mentioning", "mentions", "mention", "related to", "to do with", "matching"] {
            if let Some(i) = at(&w, lead) {
                let from = i + lead.split(' ').count();
                let mut k = from;
                while k < w.len() && !used[k] && (!STOP.contains(&w[k].as_str()) || (k == from && matches!(w[k].as_str(), "a" | "an" | "the"))) && k - from < 6 {
                    k += 1;
                }
                about = w[from..k].iter().filter(|x| !matches!(x.as_str(), "a" | "an" | "the")).cloned().collect::<Vec<_>>().join(" ");
                mark(i, k - i, &mut used);
                break;
            }
        }
    }
    let mut cause = String::new();
    if event == "fault" && about.is_empty() {
        if let Some((c, _)) = CAUSES.iter().find(|(_, names)| has(&w, names)) {
            cause = c.to_string();
        }
    }
    if !event.is_empty() || !about.is_empty() || !author.is_empty() {
        p.happened = Some(Happened { event, about, author, cause, hours: hours.unwrap_or(168.0), negate });
    }

    // Where in the graph.
    let seed = |from: usize, used: &mut Vec<bool>| -> Option<String> {
        (from..(from + 3).min(w.len())).find(|&k| known(&w[k])).map(|k| {
            used[k] = true;
            w[k].clone()
        })
    };
    for (lead, dir, hops) in [("downstream of", "out", 6), ("behind", "out", 6), ("below", "out", 6), ("fed by", "out", 6), ("depend on", "out", 6), ("depends on", "out", 6), ("under", "out", 6), ("upstream of", "in", 6), ("feeding", "in", 6), ("above", "in", 6), ("hops of", "both", 2), ("hops from", "both", 2), ("hop of", "both", 1), ("near", "both", 2), ("around", "both", 2), ("next to", "both", 1)] {
        let Some(i) = at(&w, lead) else { continue };
        let n = lead.split(' ').count();
        let Some(from) = seed(i + n, &mut used) else { continue };
        let mut hops = hops;
        if lead.starts_with("hop") && i > 0 {
            if let Some(h) = number(&w[i - 1]) {
                hops = h as i64;
                mark(i - 1, 1, &mut used);
                if i > 1 && w[i - 2] == "within" {
                    mark(i - 2, 1, &mut used);
                }
            }
        }
        mark(i, n, &mut used);
        p.walk = Some(Walk { dir: dir.into(), from, hops: hops.clamp(1, 8) });
        break;
    }

    // Its own columns.
    for (kind, names) in KINDS {
        if let Some((i, n)) = names.iter().find_map(|nm| at(&w, nm).map(|i| (i, nm.split(' ').count()))) {
            if !(0..n).any(|k| used[i + k]) {
                p.kinds.push(kind.to_string());
                mark(i, n, &mut used);
            }
        }
    }
    for (i, x) in w.iter().enumerate() {
        if used[i] {
            continue;
        }
        let before = if i > 0 { w[i - 1].as_str() } else { "" };
        match x.as_str() {
            "down" | "degraded" | "impacted" if !matches!(before, "went" | "gone" | "going") => p.states.push(x.clone()),
            "broken" | "unhealthy" | "failing" => p.states.push("not ok".into()),
            "trouble" if before == "in" => p.states.push("not ok".into()),
            "ok" if before == "not" => p.states.push("not ok".into()),
            "healthy" | "ok" => p.states.push("ok".into()),
            _ => continue,
        }
        used[i] = true;
    }
    p.states.dedup();
    let named = |list: &[(String, Vec<String>)], used: &mut Vec<bool>| -> String {
        for (id, names) in list {
            for nm in names {
                if let Some(i) = at(&w, nm) {
                    let n = nm.split(' ').count();
                    if !(0..n).any(|k| used[i + k]) {
                        (i..i + n).for_each(|k| used[k] = true);
                        return id.clone();
                    }
                }
            }
        }
        String::new()
    };
    p.metro = named(&v.metros, &mut used);
    p.region = named(&v.regions, &mut used);
    for (i, x) in w.iter().enumerate() {
        if used[i] {
            continue;
        }
        if x.len() > 4 && x.starts_with('h') && x[1..3].chars().all(|c| c.is_ascii_digit()) && x.as_bytes()[3] == b'-' {
            p.hub = x.clone();
            used[i] = true;
        } else if let Some(vn) = v.vendors.iter().find(|vn| vn.to_lowercase() == *x || vn.to_lowercase().split(' ').next() == Some(x.as_str())) {
            p.vendor = vn.clone();
            used[i] = true;
        } else if matches!(x.as_str(), "running" | "version" | "release") {
            if let Some(ver) = w.get(i + 1).filter(|s| s.chars().any(|c| c.is_ascii_digit())) {
                p.version = ver.clone();
                used[i] = true;
                used[i + 1] = true;
            }
        } else if x == "customer" {
            if let Some(c) = w.get(i + 1).filter(|s| !STOP.contains(&s.as_str()) && !used[i + 1]) {
                p.customer = c.clone();
                used[i] = true;
                used[i + 1] = true;
            }
        }
    }
    p
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '/')).take(80).collect::<String>().trim().to_string()
}

/// A plan from what a model answered: the JSON object in it, each field
/// kept only if it is one a plan may hold. `None` when there is no object.
pub fn from_model(answer: &str, v: &Vocabulary, known: &dyn Fn(&str) -> bool) -> Option<Plan> {
    let (a, b) = (answer.find('{')?, answer.rfind('}')?);
    let j: Value = serde_json::from_str(answer.get(a..=b)?).ok()?;
    let s = |k: &str| j[k].as_str().map(|x| clean(&x.to_lowercase())).unwrap_or_default();
    let list = |k: &str| -> Vec<String> { j[k].as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(|x| clean(&x.to_lowercase())).collect() };
    let mut p = Plan { kinds: list("kinds").into_iter().filter(|k| KINDS.iter().any(|(id, _)| id == k)).collect(), states: list("states").into_iter().filter(|x| matches!(x.as_str(), "down" | "degraded" | "impacted" | "ok" | "not ok")).collect(), ..Plan::default() };
    p.vendor = v.vendors.iter().find(|x| x.to_lowercase() == s("vendor")).cloned().unwrap_or_default();
    p.version = s("version");
    p.customer = s("customer");
    let one_of = |list: &[(String, Vec<String>)], x: String| list.iter().find(|(id, names)| *id == x || names.contains(&x)).map(|(id, _)| id.clone()).unwrap_or_default();
    p.metro = one_of(&v.metros, s("metro"));
    p.region = one_of(&v.regions, s("region"));
    p.hub = Some(s("hub")).filter(|h| known(h)).unwrap_or_default();
    for (key, field) in [("latency_ms", "latency_ms"), ("loss", "loss")] {
        let r = &j[key];
        if r.is_object() {
            let op = if r["op"].as_str().is_some_and(|o| o.starts_with('<')) { "<" } else { ">" };
            p.readings.push(Reading { field, op, value: r["value"].as_f64() });
        }
    }
    let h = &j["event"];
    if h.is_object() {
        let hs = |k: &str| h[k].as_str().map(|x| clean(&x.to_lowercase())).unwrap_or_default();
        let event = Some(hs("type")).filter(|e| matches!(e.as_str(), "change" | "fault")).unwrap_or_default();
        let author = hs("author");
        let author = OPERATORS.iter().find(|o| **o == author || o.split('.').nth(1) == Some(author.as_str())).map(|o| o.to_string()).unwrap_or(author);
        let cause = Some(hs("cause")).filter(|c| CAUSES.iter().any(|(id, _)| id == c)).unwrap_or_default();
        p.happened = Some(Happened { event, about: hs("about"), author, cause, hours: h["hours"].as_f64().filter(|x| *x > 0.0).unwrap_or(168.0).min(24.0 * 90.0), negate: h["negate"].as_bool().unwrap_or(false) });
    }
    let g = &j["graph"];
    if let Some(from) = g["from"].as_str().map(|x| clean(&x.to_lowercase())).filter(|x| known(x)) {
        let dir = match g["dir"].as_str() {
            Some("in") => "in",
            Some("both") => "both",
            _ => "out",
        };
        p.walk = Some(Walk { dir: dir.into(), from, hops: g["hops"].as_i64().unwrap_or(if dir == "both" { 2 } else { 6 }).clamp(1, 8) });
    }
    p.order = Some(s("order")).filter(|o| matches!(o.as_str(), "latency" | "loss")).unwrap_or_default();
    Some(p)
}

/// What the model is told: the plan's shape and what it may name.
pub fn instructions(v: &Vocabulary) -> String {
    let names = |list: &[(String, Vec<String>)]| list.iter().map(|(id, n)| format!("{id} ({})", n.first().cloned().unwrap_or_default())).collect::<Vec<_>>().join(", ");
    format!(
        "You turn a network engineer's question about a carrier's network into a JSON search plan. Answer with one JSON object and nothing else. Leave out what the question does not say.\n\
        Fields:\n\
        \"kinds\": list of object kinds, from: {kinds}.\n\
        \"states\": list from: down, degraded, impacted, ok, \"not ok\" (how the object is now).\n\
        \"vendor\": one of: {vendors}. \"version\": a software version. \"customer\": part of a customer's name.\n\
        \"metro\": one of: {metros}. \"region\": one of: {regions}. \"hub\": a hub id such as h03-ams.\n\
        \"latency_ms\" and \"loss\": {{\"op\": \">\" or \"<\", \"value\": number}} on the object's latest reading (latency in ms, loss in percent); leave value out for \"high\" with no number.\n\
        \"event\": something that happened to the object: {{\"type\": \"change\" or \"fault\", \"about\": the two or three key words of what it was about (they are matched against the change's or fault's text), \"author\": who made the change, one of: {operators}, \"cause\": for a fault, one of: fibre, power, software, hardware, config, weather, congestion, \"hours\": how far back to look (a week is 168), \"negate\": true for objects it did NOT happen to}}.\n\
        \"graph\": where it sits: {{\"dir\": \"out\" for objects that depend on (are behind, downstream of) the named object, \"in\" for what the named object depends on, \"both\" for near it, \"from\": the object's id exactly as written, \"hops\": number}}.\n\
        \"order\": \"latency\" or \"loss\" when the question asks for the worst first.",
        kinds = KINDS.iter().map(|k| k.0).collect::<Vec<_>>().join(", "),
        vendors = v.vendors.join(", "),
        metros = names(&v.metros),
        regions = names(&v.regions),
        operators = OPERATORS.join(", "),
    )
}

pub struct Compiled {
    /// The conditions, to follow `WHERE`.
    pub conditions: String,
    pub order: String,
    pub params: Map<String, Value>,
    /// The conditions on what happened, for showing each match's event.
    pub happened: Option<String>,
    /// The plan in words, one per condition.
    pub read: Vec<Value>,
}

/// The plan as conditions of one query over the objects. `walked` is the
/// graph condition's rows when the app walked the graph itself (a
/// coordinator does not walk graphs yet): the condition is then a list of
/// keys rather than `REACHABLE`.
pub fn compile(p: &Plan, now: i64, walked: Option<&str>) -> Compiled {
    let mut c: Vec<String> = vec!["layer != 'container'".into()];
    let mut params = Map::new();
    let mut read: Vec<Value> = Vec::new();
    let mut said = |kind: &str, text: String, how: &str| read.push(json!({"kind": kind, "text": text, "how": how}));
    if !p.kinds.is_empty() {
        c.push(format!("kind IN ({})", p.kinds.iter().map(|k| format!("'{}'", clean(k))).collect::<Vec<_>>().join(", ")));
        said("column", format!("kind: {}", p.kinds.join(", ")), "a keyword column");
    }
    if !p.states.is_empty() {
        if p.states.iter().any(|s| s == "not ok") {
            c.push("state != 'ok'".into());
        } else {
            c.push(format!("state IN ({})", p.states.iter().map(|k| format!("'{}'", clean(k))).collect::<Vec<_>>().join(", ")));
        }
        said("column", format!("now: {}", p.states.join(", ")), "a keyword column");
    }
    for (col, val, label) in [("vendor", &p.vendor, "vendor"), ("os_version", &p.version, "running"), ("region", &p.region, "region"), ("metro", &p.metro, "metro"), ("hub", &p.hub, "hub")] {
        if !val.is_empty() {
            c.push(format!("{col} = :{col}"));
            params.insert(col.into(), json!(val));
            said("column", format!("{label}: {val}"), "a keyword column");
        }
    }
    if !p.customer.is_empty() {
        c.push("customer ILIKE :customer".into());
        params.insert("customer".into(), json!(format!("%{}%", clean(&p.customer))));
        said("substring", format!("customer contains \"{}\"", p.customer), "ILIKE from the substring index");
    }
    for r in &p.readings {
        let (name, unit) = if r.field == "loss" { ("loss", "%") } else { ("latency", " ms") };
        match r.value {
            Some(v) => {
                c.push(format!("{} {} :{}", r.field, r.op, r.field));
                params.insert(r.field.into(), json!(v));
                said("reading", format!("{name} {} {v}{unit}", r.op), "its latest reading, a number column");
            }
            None if r.field == "latency_ms" => {
                c.push("latency_ms > base_ms * 2".into());
                said("reading", "latency more than twice its usual".into(), "two columns of the same row compared");
            }
            None => {
                c.push("loss > 0.5".into());
                said("reading", "loss above 0.5%".into(), "its latest reading, a number column");
            }
        }
    }
    let mut happened = None;
    if let Some(h) = &p.happened {
        let mut hc: Vec<String> = Vec::new();
        if !h.event.is_empty() {
            hc.push(format!("event = '{}'", clean(&h.event)));
        } else {
            hc.push("event IN ('change', 'fault')".into());
        }
        hc.push("ts >= :since".into());
        params.insert("since".into(), json!(iso(now - (h.hours * 3600.0) as i64)));
        let mut text = format!("{} in the last {}", match h.event.as_str() { "change" => "changed", "fault" => "failed", _ => "something happened" }, span(h.hours));
        if !h.author.is_empty() {
            hc.push("author = :author".into());
            params.insert("author".into(), json!(h.author));
            text += &format!(", by {}", h.author);
        }
        if !h.cause.is_empty() {
            hc.push("cause = :cause".into());
            params.insert("cause".into(), json!(h.cause));
            text += &format!(", cause {}", h.cause);
        }
        if !h.about.is_empty() {
            hc.push("MATCH(text, :about)".into());
            params.insert("about".into(), json!(clean(&h.about)));
            text += &format!(", about \"{}\"", h.about);
        }
        let hc = hc.join(" AND ");
        c.push(format!("id {}IN (\n    SELECT node FROM {EVENTS}\n    WHERE {hc})", if h.negate { "NOT " } else { "" }));
        said("events", if h.negate { format!("not: {text}") } else { text }, "a subquery over the events table: time, author and MATCH on the words");
        happened = Some(hc);
    }
    if let Some(g) = &p.walk {
        let dir = match g.dir.as_str() {
            "in" => "in",
            "both" => "both",
            _ => "out",
        };
        match walked {
            Some(list) => c.push(format!("id IN ({list})")),
            None => c.push(format!("REACHABLE({EDGES}, '{}', hops => {}, dir => '{dir}', through => layer != 'core')", clean(&g.from), g.hops.clamp(1, 8))),
        }
        said("graph", match dir { "out" => format!("depends on {}", g.from), "in" => format!("{} depends on it", g.from), _ => format!("within {} hops of {}", g.hops, g.from) }, if walked.is_some() { "walked by the app: a coordinator does not walk graphs yet" } else { "REACHABLE over the edge table" });
    }
    let order = match p.order.as_str() {
        "loss" => "loss DESC",
        _ => "latency_ms DESC",
    };
    Compiled { conditions: c.join("\n  AND "), order: order.into(), params, happened, read }
}

fn span(hours: f64) -> String {
    if hours < 1.0 {
        format!("{} minutes", (hours * 60.0).round())
    } else if hours < 48.0 {
        format!("{} hours", hours.round())
    } else if hours < 24.0 * 14.0 {
        format!("{} days", (hours / 24.0).round())
    } else {
        format!("{} weeks", (hours / 168.0).round())
    }
}

fn plan_json(p: &Plan) -> Value {
    json!({"kinds": p.kinds, "states": p.states, "vendor": p.vendor, "version": p.version, "customer": p.customer, "region": p.region, "metro": p.metro, "hub": p.hub,
        "readings": p.readings.iter().map(|r| json!({"field": r.field, "op": r.op, "value": r.value})).collect::<Vec<_>>(),
        "event": p.happened.as_ref().map(|h| json!({"type": h.event, "about": h.about, "author": h.author, "cause": h.cause, "hours": h.hours, "negate": h.negate})),
        "graph": p.walk.as_ref().map(|g| json!({"dir": g.dir, "from": g.from, "hops": g.hops})), "order": p.order})
}

/// What the estate has that a plan may name: read once from the tables.
pub async fn vocabulary(e: &Engine) -> Vocabulary {
    let rows = e.db.query(&format!("SELECT id, kind, name FROM {NODES} WHERE tier <= 1 LIMIT 2000"), json!({})).await.unwrap_or_default();
    let of = |kind: &str| -> Vec<(String, Vec<String>)> {
        rows.iter().filter(|r| r.s("kind") == kind).map(|r| {
            let (id, name) = (r.s("id"), r.s("name").to_lowercase());
            let short = id.split_once('-').map(|x| x.1.to_string()).unwrap_or_default();
            let mut names = vec![name.clone(), name.replace('-', " ")];
            if kind == "metro" && short.len() >= 3 {
                names.push(short);
            }
            names.dedup();
            (id, names)
        }).collect()
    };
    let vendors = e.db.query(&format!("SELECT vendor, COUNT(*) AS n FROM {NODES} GROUP BY vendor LIMIT 100"), json!({})).await.unwrap_or_default();
    Vocabulary { metros: of("metro"), regions: of("region"), vendors: vendors.iter().map(|r| r.s("vendor")).filter(|s| !s.is_empty()).collect() }
}

/// When the model may be asked again after it failed (epoch seconds).
static MODEL_AFTER: AtomicI64 = AtomicI64::new(0);

#[derive(serde::Deserialize)]
struct AskQ {
    q: String,
    /// `rules` to skip the model.
    reader: Option<String>,
}

const FOUND: &str = "id, kind, layer, name, state, reason, x, y, latency_ms, base_ms, loss, vendor, os_version, customer, region, metro, hub, asite";

async fn ask(State(svc): State<Shared>, Query(a): Query<AskQ>) -> Answer {
    let q = Q::new(&svc);
    let e = q.e.clone();
    let asked = a.q.trim().to_string();
    if asked.len() < 3 || asked.len() > 600 {
        return Err(bad("ask in a sentence, such as: OLTs changed in the last week by m.rossi with latency above 20 ms"));
    }
    let t0 = std::time::Instant::now();
    let v = vocabulary(&e).await;
    // Ids the question names are looked up, so a plan walks from a row that exists.
    let candidates: Vec<String> = words(&asked).into_iter().filter(|w| w.contains('-') && w.chars().any(|c| c.is_ascii_digit()) && w.len() <= 60).map(|w| clean(&w)).collect();
    let mut ids: Vec<String> = Vec::new();
    if !candidates.is_empty() {
        let list = candidates.iter().map(|c| format!("'{c}'")).collect::<Vec<_>>().join(", ");
        ids = q.rows("The objects the question names", &format!("SELECT id FROM {NODES}\nWHERE id IN ({list}) LIMIT 20"), json!({})).await?.iter().map(|r| r.s("id")).collect();
    }
    let known = |x: &str| ids.iter().any(|i| i == x);
    let id = format!("ask-{}-{}", e.st().last_minute / 60, e.next_id());
    let now = crate::sim::now();
    e.db.upsert("netops_searches", vec![json!({"id": id, "ts": iso(now), "asked": asked, "reader": "", "plan": "", "sql_text": "", "found": 0, "ms": 0.0})]).await?;
    let mut reader = "rules";
    let mut note = String::new();
    let mut plan = None;
    let model = e.opts.rewrite_model.clone();
    if model.is_empty() {
        note = "no model is set (--rewrite-model)".into();
    } else if a.reader.as_deref() == Some("rules") {
        note = "the fixed reader was asked for".into();
    } else if MODEL_AFTER.load(Ordering::Relaxed) > now {
        note = "the server's model did not answer a moment ago: it is tried again in a few minutes".into();
    } else {
        let sql = format!("SELECT llm('{}', :instructions, asked, max_tokens => 500) AS plan FROM netops_searches\nWHERE id = :id", clean(&model));
        match e.db.traced(&q.trace, "The question rewritten as a plan, by the server's model (llm() over the saved question)", &sql, json!({"instructions": instructions(&v), "id": id})).await {
            Ok(rows) => match rows.first().map(|r| r.s("plan")).and_then(|ans| from_model(&ans, &v, &known)) {
                Some(p) => {
                    plan = Some(p);
                    reader = "model";
                }
                None => note = "the model's answer was not a plan".into(),
            },
            Err(err) => {
                MODEL_AFTER.store(now + 600, Ordering::Relaxed);
                note = format!("the server has no model to ask ({})", err.message.chars().take(140).collect::<String>());
            }
        }
    }
    let plan = plan.unwrap_or_else(|| read(&asked, &v, &known));
    if plan == Plan::default() {
        e.db.upsert("netops_searches", vec![json!({"id": id, "ts": iso(now), "asked": asked, "reader": reader, "plan": "{}", "sql_text": "", "found": 0, "ms": 0.0})]).await?;
        return q.done(json!({"asked": asked, "reader": reader, "note": note, "understood": false, "read": [], "plan": plan_json(&plan), "rows": [], "places": {}, "total": 0,
            "hint": "Nothing in the sentence was recognised. Name a kind (OLTs, cell sites, hub routers), a place (Frankfurt), a state (down, degraded), a reading (latency above 30 ms), something that happened (changed in the last week about firmware, by m.rossi) or a place in the graph (behind hr1-h03-ams)."}));
    }
    let at = e.st().last_minute.max(now - 120);
    let statement = |c: &Compiled| format!("SELECT {FOUND} FROM {NODES}\nWHERE {}\nORDER BY {} LIMIT 300", c.conditions, c.order);
    let mut c = compile(&plan, at, None);
    let mut params = Value::Object(c.params.clone());
    let mut sql = statement(&c);
    let display = sql.clone();
    let rows = match e.db.traced(&q.trace, "The search: every condition in one statement", &sql, params.clone()).await {
        Ok(rows) => rows,
        Err(err) => {
            // A server that does not walk graphs: the app walks, and the
            // rest of the statement stays as it was.
            let (Some(g), Some(est)) = (&plan.walk, &e.estate) else { return Err(err.into()) };
            let Some(from) = est.find(&g.from) else { return Err(err.into()) };
            let ids = est.walk_hops(from, g.hops as usize, 50_000, g.dir != "in", g.dir != "out");
            let list = if ids.is_empty() { "''".to_string() } else { ids.iter().map(|(i, _)| format!("'{}'", est.nodes[*i as usize].id)).collect::<Vec<_>>().join(", ") };
            c = compile(&plan, at, Some(&list));
            params = Value::Object(c.params.clone());
            sql = statement(&c);
            q.rows("The search, the graph walked by the app", &sql, params.clone()).await?
        }
    };
    let by_place = q.rows("Where the matches are, for the map at every zoom", &format!("SELECT region, metro, hub, asite, COUNT(*) AS n FROM {NODES}\nWHERE {}\nGROUP BY region, metro, hub, asite LIMIT 10000", c.conditions), params.clone()).await?;
    let mut places: Map<String, Value> = Map::new();
    let mut total = 0;
    for b in &by_place {
        total += b.i("n");
        for id in [b.s("region"), b.s("metro"), b.s("hub"), b.s("asite")].into_iter().filter(|s| !s.is_empty()) {
            let p = places.entry(id).or_insert(json!(0));
            *p = json!(p.as_i64().unwrap_or(0) + b.i("n"));
        }
    }
    // What happened to each of the first matches, to show beside it.
    let mut why: Map<String, Value> = Map::new();
    if let (Some(hc), false) = (&c.happened, rows.is_empty()) {
        if !plan.happened.as_ref().is_some_and(|h| h.negate) {
            let list = rows.iter().take(120).map(|r| format!("'{}'", r.s("id").replace('\'', ""))).collect::<Vec<_>>().join(", ");
            let events: Vec<Row> = q.soft("What happened to the first of them", &format!("SELECT node, ts, event, text, author, ticket, cause FROM {EVENTS}\nWHERE node IN ({list}) AND {hc}\nORDER BY ts DESC LIMIT 400"), params.clone()).await;
            for ev in events {
                why.entry(ev.s("node")).or_insert(Value::Object(ev));
            }
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let shown = display.clone() + &format!("\n-- {}", Value::Object(compile(&plan, at, None).params));
    e.db.upsert("netops_searches", vec![json!({"id": id, "ts": iso(now), "asked": asked, "reader": reader, "plan": plan_json(&plan).to_string(), "sql_text": shown, "found": total, "ms": ms})]).await?;
    q.done(json!({"asked": asked, "reader": reader, "note": note, "understood": true, "read": c.read, "plan": plan_json(&plan), "sql": display, "params": compile(&plan, at, None).params, "rows": rows, "why": why, "places": places, "total": total, "ms": ms}))
}

/// The searches asked lately.
async fn asks(State(svc): State<Shared>) -> Answer {
    let q = Q::new(&svc);
    let rows = q.rows("Searches asked lately", "SELECT id, ts, asked, reader, found, ms FROM netops_searches\nWHERE found > 0\nORDER BY ts DESC LIMIT 12", json!({})).await?;
    q.done(json!({"asks": rows}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocabulary {
        Vocabulary { metros: vec![("m-fra".into(), vec!["frankfurt".into(), "fra".into()]), ("m-ams".into(), vec!["amsterdam".into(), "ams".into()])], regions: vec![("r-eu-west".into(), vec!["eu-west".into(), "eu west".into()])], vendors: vec!["Nokia".into(), "Cisco".into()] }
    }

    #[test]
    fn a_sentence_is_read_into_a_plan() {
        let known = |x: &str| x == "hr1-h03-ams";
        let p = read("all nodes that have been changed in the last week and the change was about firmware upgrade and the latency is greater than 40 ms and it was done by operator m.rossi", &vocab(), &known);
        let h = p.happened.clone().expect("what happened");
        assert_eq!((h.event.as_str(), h.about.as_str(), h.author.as_str(), h.hours, h.negate), ("change", "firmware upgrade", "m.rossi", 168.0, false));
        assert_eq!(p.readings, vec![Reading { field: "latency_ms", op: ">", value: Some(40.0) }]);
        let c = compile(&p, 1_800_000_000, None);
        assert!(c.conditions.contains("MATCH(text, :about)") && c.conditions.contains("author = :author") && c.conditions.contains("latency_ms > :latency_ms") && c.conditions.contains("id IN ("), "{}", c.conditions);

        let p = read("degraded Nokia OLTs in Frankfurt behind hr1-h03-ams with loss above 1%", &vocab(), &known);
        assert_eq!((p.kinds.clone(), p.states.clone(), p.vendor.as_str(), p.metro.as_str()), (vec!["olt".to_string()], vec!["degraded".to_string()], "Nokia", "m-fra"));
        assert_eq!(p.walk, Some(Walk { dir: "out".into(), from: "hr1-h03-ams".into(), hops: 6 }));
        assert_eq!(p.readings, vec![Reading { field: "loss", op: ">", value: Some(1.0) }]);

        let p = read("cell sites that went down in the last 3 days because of power", &vocab(), &known);
        let h = p.happened.expect("a fault");
        assert_eq!((p.kinds, h.event.as_str(), h.cause.as_str(), h.hours, p.states.is_empty()), (vec!["cell".to_string()], "fault", "power", 72.0, true));

        let p = read("hub routers not changed in the last month", &vocab(), &known);
        assert!(p.happened.is_some_and(|h| h.negate && h.hours == 720.0));
        assert_eq!(read("what is the weather", &vocab(), &known), Plan::default());
    }

    #[test]
    fn a_models_answer_is_checked() {
        let known = |x: &str| x == "ag1-fra";
        let p = from_model("Here you go:\n{\"kinds\": [\"olt\", \"DROP TABLE\"], \"metro\": \"Frankfurt\", \"latency_ms\": {\"op\": \">\", \"value\": 25}, \"event\": {\"type\": \"change\", \"about\": \"vlan'; --\", \"author\": \"Rossi\", \"hours\": 48}, \"graph\": {\"dir\": \"out\", \"from\": \"nothing-9\"}}", &vocab(), &known).expect("a plan");
        assert_eq!((p.kinds, p.metro.as_str()), (vec!["olt".to_string()], "m-fra"));
        let h = p.happened.expect("an event");
        assert_eq!((h.about.as_str(), h.author.as_str(), h.hours), ("vlan --", "m.rossi", 48.0));
        assert!(p.walk.is_none(), "a walk from a row that does not exist is dropped");
        assert!(from_model("I cannot help with that", &vocab(), &known).is_none());
    }
}
