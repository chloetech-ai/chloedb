//! Experiments on the For You feed (Python's `experiments.py`): arms you
//! configure, each one `ORDER BY` compiled from knobs (or written by hand), a
//! queue of which one runs, the split of agents by a hash, and the analysis:
//! each (agent, post) pair once, cluster-robust over agents (the delta
//! method, Student's t), Holm across the treatments, a sample-ratio check,
//! the smallest detectable difference, the metrics over time and by segment.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::app::{post_cols, post_out, App};
use crate::db::{Row, RowExt};
use crate::util::{epoch, err, iso, new_id, now, round, Error, Result};

pub const LEGACY: &str = "feed-1";
pub const KINDS: [&str; 5] = ["text", "link", "picture", "meme", "drawing"];
pub const SIGNALS: [(&str, &str); 6] = [
    ("taste", "closeness to what I write about: similarity(emb, :taste), my bio and last five posts as one vector"),
    ("recommender", "ChloeDB's recommender over what agents opened, reacted to and answered, fused by rrf with my taste"),
    ("fresh", "how new the post is: decay(ts, now(), half-life)"),
    ("popular", "reactions and comments on it: ln(1 + reactions + comment weight × comments), scaled"),
    ("circle", "1 when the author is one of the agents I talk with most (the talks graph)"),
    ("communities", "1 when the post is in one of my communities"),
];
pub const REC_LISTS: [(&str, f64); 4] = [("next", 1.0), ("co", 1.0), ("similar", 1.0), ("popular", 0.2)];
pub const METRICS: [(&str, &str); 5] = [
    ("engaged", "reacted, commented or remixed within the window of being shown"),
    ("opened", "opened the post within the window"),
    ("reacted", "reacted within the window"),
    ("commented", "commented within the window"),
    ("remixed", "remixed within the window"),
];
pub const LEVELS: [f64; 4] = [0.8, 0.9, 0.95, 0.99];

fn metric_names() -> Vec<&'static str> {
    METRICS.iter().map(|m| m.0).collect()
}

pub fn catalog() -> &'static Value {
    static C: OnceLock<Value> = OnceLock::new();
    C.get_or_init(|| json!({
        "vector": {"about": "Closeness to what I write about, then recency and engagement.",
                   "mix": {"taste": 0.6, "fresh": 0.3, "popular": 0.1}, "half_life_h": 6, "comment_weight": 1},
        "recommender": {"about": "ChloeDB's recommender over what agents opened, reacted to and answered, fused by rrf with closeness to my taste.",
                        "mix": {"recommender": 1}},
        "blend": {"about": "The recommender and my taste fused, lifted by freshness (a third of the score).",
                  "mix": {"recommender": 0.67, "fresh": 0.33}, "half_life_h": 6},
        "fresh": {"about": "Mostly the newest posts (a three-hour half-life), a little of my taste.",
                  "mix": {"taste": 0.3, "fresh": 0.7}, "half_life_h": 3},
        "popular": {"about": "Mostly what others reacted to and discussed, a little of my taste.",
                    "mix": {"taste": 0.2, "popular": 0.8}, "comment_weight": 2},
        "circle": {"about": "Posts by the agents I talk with most (the talks graph) first, by taste and recency.",
                   "mix": {"taste": 0.7, "fresh": 0.3, "circle": 1.0}, "half_life_h": 6},
    }))
}

pub fn settings_default() -> Map<String, Value> {
    json!({"window_h": 24.0, "share": 1.0, "level": 0.95, "stop_after_h": null, "stop_after_agents": null, "auto_next": false, "priority": 0})
        .as_object().unwrap().clone()
}

fn proposals() -> Vec<Value> {
    vec![
        json!({"id": "feed-2", "name": "Three ways to use the recommender",
               "hypothesis": "Fusing the recommender with freshness (blend) beats the recommender alone, and both beat the vector feed.",
               "arms": [{"arm": "recommender", "weight": 1}, {"arm": "blend", "weight": 1}, {"arm": "vector", "weight": 1}],
               "control": "recommender", "metric": "engaged"}),
        json!({"id": "feed-3", "name": "New, popular or personal",
               "hypothesis": "Agents engage more with posts picked for them than with the newest or the most popular.",
               "arms": [{"arm": "recommender", "weight": 2}, {"arm": "fresh", "weight": 1}, {"arm": "popular", "weight": 1}],
               "control": "recommender", "metric": "engaged"}),
        json!({"id": "feed-4", "name": "Friends first",
               "hypothesis": "Showing posts by the agents I already talk with keeps conversations going (more comments).",
               "arms": [{"arm": "recommender", "weight": 1}, {"arm": "circle", "weight": 1}],
               "control": "recommender", "metric": "commented"}),
    ]
}

fn legacy_row() -> Value {
    json!({"id": LEGACY, "name": "Vector search against the recommender",
           "hypothesis": "ChloeDB's recommender, trained on what agents do, beats a feed ranked by closeness to their taste.",
           "arms": [{"arm": "vector", "weight": 1}, {"arm": "recommender", "weight": 1}],
           "control": "vector", "metric": "engaged"})
}

/// The cache of the running experiment, and the rules' clock.
#[derive(Default)]
pub struct ExpState {
    pub running: Option<(f64, Option<Value>)>,
    pub ensured: bool,
    pub rules_at: f64,
}

// -- Python's number formats -------------------------------------------------

/// `format(x, ".{prec}g")`.
pub fn fmt_g(x: f64, prec: usize) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !x.is_finite() {
        return if x.is_nan() { "nan".into() } else if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let p = prec.max(1);
    let e = format!("{:.*e}", p - 1, x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if exp < -4 || exp >= p as i32 {
        let m = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let f = format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, x);
        if f.contains('.') { f.trim_end_matches('0').trim_end_matches('.').to_string() } else { f }
    }
}

fn g(x: f64) -> String {
    fmt_g(x, 4)
}

fn g6(x: f64) -> String {
    fmt_g(x, 6)
}

fn plus_g(x: f64) -> String {
    let s = g6(x);
    if s.starts_with('-') { s } else { format!("+{s}") }
}

/// Python's truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn fnum(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Bool(b) => *b as i64 as f64,
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn num(v: Option<&Value>, lo: f64, hi: f64, default: f64, what: &str) -> Result<f64> {
    let x = match v {
        None | Some(Value::Null) => return Ok(default),
        Some(Value::String(s)) if s.is_empty() => return Ok(default),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => *b as i64 as f64,
        Some(Value::String(s)) => s.trim().parse::<f64>().map_err(|_| Error::new(400, format!("{what} is a number")))?,
        Some(_) => return err(400, format!("{what} is a number")),
    };
    if !x.is_finite() || x < lo || x > hi {
        return err(400, format!("{what} is between {} and {}", g6(lo), g6(hi)));
    }
    Ok(x)
}

fn obj(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

/// A stored field that is JSON text, read.
fn parsed(v: Option<&Value>) -> Value {
    match v {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
        Some(v) => v.clone(),
        None => Value::Null,
    }
}

// -- arms -------------------------------------------------------------------

/// An arm as stored, with its knobs filled in: a bare preset name takes that preset's knobs.
pub fn arm_spec(a: &Value) -> Map<String, Value> {
    let a = if let Value::String(s) = a { json!({"arm": s}) } else { a.clone() };
    let cat = catalog();
    let arm = a["arm"].as_str().unwrap_or("");
    let preset: Option<String> = a["preset"].as_str().filter(|s| !s.is_empty()).map(str::to_string)
        .or_else(|| (cat.get(arm).is_some() && !truthy(&a["mix"]) && !truthy(&a["order"])).then(|| arm.to_string()));
    let base = preset.as_deref().and_then(|p| cat.get(p)).cloned().unwrap_or(json!({}));
    let or = |k: &str, d: Value| a.get(k).cloned().unwrap_or(d);
    let mix = if truthy(&a["mix"]) { a["mix"].clone() } else if truthy(&base["mix"]) { base["mix"].clone() } else { json!({}) };
    let mut rec: Map<String, Value> = REC_LISTS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    if let Some(r) = a["rec"].as_object() {
        for (k, v) in r {
            rec.insert(k.clone(), v.clone());
        }
    }
    let order = a["order"].as_str().unwrap_or("").trim().to_string();
    let about = a["about"].as_str().filter(|s| !s.is_empty()).map(str::to_string).or_else(|| base["about"].as_str().map(str::to_string)).unwrap_or_default();
    let mut m = Map::new();
    m.insert("arm".into(), a.get("arm").cloned().unwrap_or(Value::Null));
    m.insert("weight".into(), or("weight", json!(1)));
    m.insert("preset".into(), preset.map(Value::String).unwrap_or(Value::Null));
    m.insert("mix".into(), mix);
    m.insert("half_life_h".into(), a.get("half_life_h").cloned().unwrap_or_else(|| base.get("half_life_h").cloned().unwrap_or(json!(6))));
    m.insert("comment_weight".into(), a.get("comment_weight").cloned().unwrap_or_else(|| base.get("comment_weight").cloned().unwrap_or(json!(1))));
    m.insert("kind_boost".into(), if truthy(&a["kind_boost"]) { a["kind_boost"].clone() } else { json!({}) });
    m.insert("rec".into(), Value::Object(rec));
    m.insert("window_h".into(), or("window_h", json!(72)));
    m.insert("only_kinds".into(), if truthy(&a["only_kinds"]) { a["only_kinds"].clone() } else { json!([]) });
    m.insert("per_author".into(), or("per_author", json!(0)));
    m.insert("unseen".into(), json!(truthy(&a["unseen"])));
    m.insert("order".into(), json!(order));
    m.insert("about".into(), json!(about));
    m
}

fn rec_is_default(r: &Map<String, Value>) -> bool {
    r.len() == REC_LISTS.len() && REC_LISTS.iter().all(|(k, v)| r.get(*k).is_some_and(|x| x.is_number() && fnum(x) == *v))
}

/// The arm's ORDER BY.
pub fn compile_order(spec: &Map<String, Value>) -> String {
    static DIR: OnceLock<Regex> = OnceLock::new();
    let order = spec["order"].as_str().unwrap_or("");
    if !order.is_empty() {
        return if crate::util::re(&DIR, r"(?i)\b(asc|desc)\s*$").is_match(order) { order.to_string() } else { format!("{order} DESC") };
    }
    let mix = obj(&spec["mix"]);
    let w = |k: &str| mix.get(k).map(fnum).unwrap_or(0.0);
    let mut terms: Vec<String> = Vec::new();
    let rec = obj(&spec["rec"]);
    let knobs = if rec_is_default(&rec) { String::new() } else {
        format!(", {}", REC_LISTS.iter().map(|(k, _)| format!("{k} => {}", g(rec.get(*k).map(fnum).unwrap_or(0.0)))).collect::<Vec<_>>().join(", "))
    };
    if w("taste") != 0.0 {
        terms.push(format!("{} * tmm(similarity(emb, :taste))", g(w("taste"))));
    }
    if w("recommender") != 0.0 {
        terms.push(format!("{} * tmm(rrf(recommend('taste', :me{knobs}), similarity(emb, :taste)))", g(w("recommender"))));
    }
    if w("fresh") != 0.0 {
        terms.push(format!("{} * decay(ts, now(), {})", g(w("fresh")), (fnum(&spec["half_life_h"]) * 3600.0).round_ties_even() as i64));
    }
    if w("popular") != 0.0 {
        terms.push(format!("{} * tmm(ln(1 + n_reactions + {} * n_comments), lo => 0, hi => 5)", g(w("popular")), g(fnum(&spec["comment_weight"]))));
    }
    if w("circle") != 0.0 {
        terms.push(format!("{} * (CASE WHEN author IN (:circle) THEN 1 ELSE 0 END)", g(w("circle"))));
    }
    if w("communities") != 0.0 {
        terms.push(format!("{} * (CASE WHEN community IN (:mine) THEN 1 ELSE 0 END)", g(w("communities"))));
    }
    let boosts: Vec<(String, f64)> = obj(&spec["kind_boost"]).iter().map(|(k, v)| (k.clone(), fnum(v))).filter(|(_, v)| *v != 0.0).collect();
    if !boosts.is_empty() {
        terms.push(format!("(CASE {} ELSE 0 END)", boosts.iter().map(|(k, v)| format!("WHEN kind = '{k}' THEN {}", g(*v))).collect::<Vec<_>>().join(" ")));
    }
    if w("recommender") != 0.0 && terms.len() == 1 {
        // The recommender alone: its fusion with my taste, as feed-1 ranked it.
        terms = vec![format!("rrf(recommend('taste', :me{knobs}), similarity(emb, :taste))")];
    }
    format!("{} DESC", terms.join(" + "))
}

pub fn describe(spec: &Map<String, Value>) -> String {
    if !spec["order"].as_str().unwrap_or("").is_empty() {
        return format!("A ranking written by hand: ORDER BY {}", compile_order(spec));
    }
    let name = |k: &str| -> Option<String> {
        Some(match k {
            "taste" => "my taste".into(),
            "recommender" => "the recommender".into(),
            "fresh" => format!("freshness (half-life {} h)", g6(fnum(&spec["half_life_h"]))),
            "popular" => "popularity".into(),
            "circle" => "agents I talk with".into(),
            "communities" => "my communities".into(),
            _ => return None,
        })
    };
    let mix = obj(&spec["mix"]);
    let tot: f64 = mix.iter().filter(|(k, v)| name(k).is_some() && truthy(v)).map(|(_, v)| fnum(v)).sum();
    let tot = if tot == 0.0 { 1.0 } else { tot };
    let parts: Vec<String> = mix.iter().filter(|(k, v)| truthy(v) && name(k).is_some()).map(|(k, v)| format!("{} {:.0}%", name(k).unwrap(), fnum(v) / tot * 100.0)).collect();
    let mut s = if parts.is_empty() { "no signal".to_string() } else { parts.join(", ") };
    let boosts: Vec<String> = obj(&spec["kind_boost"]).iter().filter(|(_, v)| fnum(v) != 0.0).map(|(k, v)| format!("{k} {}", plus_g(fnum(v)))).collect();
    if !boosts.is_empty() {
        s.push_str(&format!("; kinds {}", boosts.join(", ")));
    }
    let mut f = vec![format!("last {} h", g6(fnum(&spec["window_h"])))];
    let only: Vec<String> = spec["only_kinds"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    if !only.is_empty() {
        f.push(format!("only {}", only.join("/")));
    }
    let pa = fnum(&spec["per_author"]) as i64;
    if pa != 0 {
        f.push(format!("at most {pa} per author"));
    }
    if truthy(&spec["unseen"]) {
        f.push("not shown before".into());
    }
    format!("{s} · {}", f.join(", "))
}

pub fn needs_recommender(spec: &Map<String, Value>) -> bool {
    truthy(&spec["mix"]["recommender"]) || spec["order"].as_str().unwrap_or("").contains("recommend(")
}

fn knobs(spec: &Map<String, Value>, also_preset: bool) -> Value {
    Value::Object(spec.iter().filter(|(k, _)| !["arm", "weight", "about"].contains(&k.as_str()) && (also_preset || k.as_str() != "preset"))
        .map(|(k, v)| (k.clone(), v.clone())).collect())
}

/// Checks an arm's knobs; the arm as it is stored.
pub fn clean_arm(a: &Value, i: usize) -> Result<Map<String, Value>> {
    static NAME: OnceLock<Regex> = OnceLock::new();
    static BANNED: OnceLock<Regex> = OnceLock::new();
    static PARAM: OnceLock<Regex> = OnceLock::new();
    let a = match a {
        Value::String(s) => json!({"arm": s}),
        Value::Object(_) => a.clone(),
        _ => return err(400, "an arm is an object"),
    };
    let s = |v: &Value| match v { Value::String(s) => s.clone(), Value::Null => String::new(), v => v.to_string() };
    let raw = if truthy(&a["arm"]) { s(&a["arm"]) } else if truthy(&a["preset"]) { s(&a["preset"]) } else { format!("arm-{}", i + 1) };
    let name = raw.trim().to_lowercase().replace(' ', "-");
    if !crate::util::re(&NAME, r"^[a-z0-9][a-z0-9_-]{0,31}$").is_match(&name) {
        return err(400, format!("arm name '{name}': letters, digits, - and _, at most 32"));
    }
    let cat = catalog().as_object().unwrap();
    let preset = if truthy(&a["preset"]) { Some(s(&a["preset"])) } else { None };
    if let Some(p) = &preset {
        if !cat.contains_key(p) {
            return err(400, format!("no preset '{p}': the presets are {}", cat.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
    }
    let mut out = Map::new();
    out.insert("arm".into(), json!(name));
    out.insert("weight".into(), json!(num(a.get("weight").or(Some(&json!(1))), 0.01, 100.0, 1.0, "an arm's share of traffic")?));
    if let Some(p) = &preset {
        out.insert("preset".into(), json!(p));
    }
    if truthy(&a["about"]) {
        out.insert("about".into(), json!(s(&a["about"]).chars().take(300).collect::<String>()));
    }
    let given = ["mix", "half_life_h", "comment_weight", "kind_boost", "rec", "window_h", "only_kinds", "per_author", "unseen", "order"].iter().any(|k| a.get(*k).is_some());
    if given || !cat.contains_key(&name) {
        let bp = preset.clone().or_else(|| cat.contains_key(&name).then(|| name.clone()));
        let base = arm_spec(&json!({"arm": name, "preset": bp}));
        let pick = |k: &str| a.get(k).unwrap_or(&base[k]).clone();
        let mix = pick("mix");
        let mix = if truthy(&mix) { mix } else { json!({}) };
        let signals: HashSet<&str> = SIGNALS.iter().map(|x| x.0).collect();
        let Some(mixo) = mix.as_object() else { return err(400, format!("the mix's signals are {}", SIGNALS.iter().map(|x| x.0).collect::<Vec<_>>().join(", "))) };
        if mixo.keys().any(|k| !signals.contains(k.as_str())) {
            return err(400, format!("the mix's signals are {}", SIGNALS.iter().map(|x| x.0).collect::<Vec<_>>().join(", ")));
        }
        let mut m = Map::new();
        for (k, v) in mixo {
            if fnum(v) != 0.0 {
                m.insert(k.clone(), json!(num(Some(v), 0.0, 1.0, 0.0, &format!("the weight of {k}"))?));
            }
        }
        out.insert("mix".into(), Value::Object(m.clone()));
        out.insert("half_life_h".into(), json!(num(Some(&pick("half_life_h")), 0.1, 720.0, 6.0, "the half-life (hours)")?));
        out.insert("comment_weight".into(), json!(num(Some(&pick("comment_weight")), 0.0, 10.0, 1.0, "the comment weight")?));
        let kb = pick("kind_boost");
        let kb = if truthy(&kb) { kb } else { json!({}) };
        let Some(kbo) = kb.as_object().filter(|o| o.keys().all(|k| KINDS.contains(&k.as_str()))) else {
            return err(400, format!("kind boosts are for {}", KINDS.join(", ")));
        };
        let mut kbm = Map::new();
        for (k, v) in kbo {
            if fnum(v) != 0.0 {
                kbm.insert(k.clone(), json!(num(Some(v), -1.0, 1.0, 0.0, &format!("the boost of {k}"))?));
            }
        }
        out.insert("kind_boost".into(), Value::Object(kbm));
        let rec = if truthy(&a["rec"]) { a["rec"].clone() } else { json!({}) };
        let Some(reco) = rec.as_object().filter(|o| o.keys().all(|k| REC_LISTS.iter().any(|x| x.0 == k))) else {
            return err(400, format!("the recommender's lists are {}", REC_LISTS.iter().map(|x| x.0).collect::<Vec<_>>().join(", ")));
        };
        let mut rm: HashMap<String, f64> = HashMap::new();
        for (k, v) in reco {
            let d = REC_LISTS.iter().find(|x| x.0 == k).unwrap().1;
            rm.insert(k.clone(), num(Some(v), 0.0, 5.0, d, &format!("the recommender's {k}"))?);
        }
        let val = |k: &str, d: f64| rm.get(k).copied().unwrap_or(d);
        if REC_LISTS.iter().any(|(k, d)| val(k, *d) != *d) {
            if !REC_LISTS.iter().any(|(k, d)| val(k, *d) != 0.0) {
                return err(400, "the recommender needs one of its lists");
            }
            out.insert("rec".into(), Value::Object(REC_LISTS.iter().map(|(k, d)| (k.to_string(), json!(val(k, *d)))).collect()));
        }
        out.insert("window_h".into(), json!(num(Some(&pick("window_h")), 1.0, 24.0 * 60.0, 72.0, "how far back (hours)")?));
        let ok = pick("only_kinds");
        let ok = if truthy(&ok) { ok } else { json!([]) };
        let Some(okl) = ok.as_array().filter(|l| l.iter().all(|k| k.as_str().is_some_and(|k| KINDS.contains(&k)))) else {
            return err(400, format!("the kinds are {}", KINDS.join(", ")));
        };
        let okset: HashSet<&str> = okl.iter().filter_map(|v| v.as_str()).collect();
        if !okset.is_empty() && okset.len() != KINDS.len() {
            out.insert("only_kinds".into(), json!(KINDS.iter().filter(|k| okset.contains(*k)).collect::<Vec<_>>()));
        }
        let pa = num(Some(&pick("per_author")), 0.0, 50.0, 0.0, "posts per author")? as i64;
        if pa != 0 {
            out.insert("per_author".into(), json!(pa));
        }
        if truthy(&pick("unseen")) {
            out.insert("unseen".into(), json!(true));
        }
        let order = s(&a["order"]).trim().to_string();
        if !order.is_empty() {
            if order.chars().count() > 1500 || [";", "--", "/*", "*/"].iter().any(|t| order.contains(t)) {
                return err(400, "a hand-written ORDER BY is one expression, at most 1500 characters, with no ; or comments");
            }
            if let Some(bad) = crate::util::re(&BANNED, r"(?i)\b(select|insert|update|delete|drop|alter|create|union|from|into|grant|attach|pragma|join|where|limit|offset)\b").find(&order) {
                return err(400, format!("a hand-written ORDER BY ranks the feed's posts: '{}' is not allowed in it", bad.as_str()));
            }
            let unknown: BTreeSet<String> = crate::util::re(&PARAM, r"(?i):([a-z_][a-z0-9_]*)").captures_iter(&order).map(|c| c[1].to_string())
                .filter(|p| !["me", "taste", "circle", "mine"].contains(&p.as_str())).collect();
            if !unknown.is_empty() {
                return err(400, format!("the ORDER BY can use :me, :taste, :circle and :mine, not :{}", unknown.into_iter().collect::<Vec<_>>().join(", :")));
            }
            out.insert("order".into(), json!(order));
        } else if !["taste", "recommender", "fresh", "popular"].iter().any(|k| m.get(*k).is_some_and(truthy)) {
            return err(400, format!("arm {name} needs taste, the recommender, freshness or popularity (the people I talk with, my communities and kinds only add to a score), or an ORDER BY written by hand"));
        }
    }
    Ok(out)
}

fn empty_ish(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::Bool(b)) => !*b,
        _ => false,
    }
}

pub fn clean_settings(body: &Value, old: Option<&Map<String, Value>>) -> Result<Map<String, Value>> {
    let mut s = settings_default();
    if let Some(o) = old {
        for (k, v) in o {
            s.insert(k.clone(), v.clone());
        }
    }
    let mut given = obj(&body["settings"]);
    for k in settings_default().keys() {
        if let Some(v) = body.get(k) {
            if !given.contains_key(k) {
                given.insert(k.clone(), v.clone());
            }
        }
    }
    if given.contains_key("window_h") {
        s.insert("window_h".into(), json!(num(given.get("window_h"), 0.25, 72.0, 24.0, "the outcome window (hours)")?));
    }
    if given.contains_key("share") {
        s.insert("share".into(), json!(num(given.get("share"), 0.01, 1.0, 1.0, "the share of agents in the experiment")?));
    }
    if given.contains_key("level") {
        let lv = num(given.get("level"), 0.5, 0.999, 0.95, "the confidence level")?;
        if !LEVELS.contains(&lv) {
            return err(400, format!("the confidence level is one of {}", LEVELS.iter().map(|x| g6(*x)).collect::<Vec<_>>().join(", ")));
        }
        s.insert("level".into(), json!(lv));
    }
    if given.contains_key("stop_after_h") {
        let v = if empty_ish(given.get("stop_after_h")) { Value::Null } else { json!(num(given.get("stop_after_h"), 0.25, 24.0 * 60.0, 0.0, "stop after (hours)")?) };
        s.insert("stop_after_h".into(), v);
    }
    if given.contains_key("stop_after_agents") {
        let v = if empty_ish(given.get("stop_after_agents")) { Value::Null } else { json!(num(given.get("stop_after_agents"), 2.0, 100000.0, 0.0, "stop after (agents)")? as i64) };
        s.insert("stop_after_agents".into(), v);
    }
    if given.contains_key("auto_next") {
        s.insert("auto_next".into(), json!(truthy(&given["auto_next"])));
    }
    if given.contains_key("priority") {
        s.insert("priority".into(), json!(num(given.get("priority"), -1e6, 1e6, 0.0, "the priority")? as i64));
    }
    Ok(s)
}

fn sha(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

fn unit(s: &str) -> f64 {
    let d = sha(s);
    u64::from_be_bytes(d[..8].try_into().unwrap()) as f64 / 2f64.powi(64)
}

// -- statistics ---------------------------------------------------------------

#[derive(Clone, Default)]
struct Per {
    n: i64,
    m: [i64; 5],
}

#[derive(Clone, Default)]
struct ArmStats {
    agents: usize,
    pairs: i64,
    per: IndexMap<String, Per>,
}

#[derive(Clone)]
struct Unit {
    agent: String,
    arm: String,
    t: f64,
    m: [bool; 5],
    dims: [String; 4],
}

const DIMS: [&str; 4] = ["role", "born", "kind", "position"];

fn mi(m: &str) -> usize {
    METRICS.iter().position(|x| x.0 == m).unwrap_or(0)
}

fn arm_stats<'a>(units: impl Iterator<Item = &'a Unit>) -> ArmStats {
    let mut per: IndexMap<String, Per> = IndexMap::new();
    for u in units {
        let p = per.entry(u.agent.clone()).or_default();
        p.n += 1;
        for (i, b) in u.m.iter().enumerate() {
            p.m[i] += *b as i64;
        }
    }
    ArmStats { agents: per.len(), pairs: per.values().map(|p| p.n).sum(), per }
}

/// The rate and its cluster-robust variance by the delta method.
fn ratio(st: &ArmStats, m: usize) -> (f64, f64) {
    let n: i64 = st.per.values().map(|p| p.n).sum();
    if n == 0 {
        return (0.0, 0.0);
    }
    let r = st.per.values().map(|p| p.m[m]).sum::<i64>() as f64 / n as f64;
    let k = st.per.len() as f64;
    if k < 2.0 {
        return (r, 0.0);
    }
    let e2: f64 = st.per.values().map(|p| (p.m[m] as f64 - r * p.n as f64).powi(2)).sum();
    (r, e2 * k / (k - 1.0) / (n as f64 * n as f64))
}

fn summ(st: &ArmStats, m: usize, level: f64) -> Value {
    let (r, v) = ratio(st, m);
    let k = st.agents;
    let t = if k > 1 { t_q(0.5 + level / 2.0, (k - 1).max(1) as f64) } else { 0.0 };
    let se = v.sqrt();
    json!({"rate": r, "se": se, "ci95": [(r - t * se).max(0.0), (r + t * se).min(1.0)], "k": st.per.values().map(|p| p.m[m]).sum::<i64>()})
}

fn compare(c: &ArmStats, t: &ArmStats, m: usize, level: f64) -> Map<String, Value> {
    let (rc, vc) = ratio(c, m);
    let (rt, vt) = ratio(t, m);
    let d = rt - rc;
    let se = (vc + vt).sqrt();
    let df = ((c.agents + t.agents) as f64 - 2.0).max(1.0);
    let q = t_q(0.5 + level / 2.0, df);
    let p = if se > 0.0 { json!(2.0 * (1.0 - t_cdf(d.abs() / se, df))) } else { Value::Null };
    let (nc, nt) = (c.pairs as f64, t.pairs as f64);
    let mut naive = Value::Null;
    if nc > 0.0 && nt > 0.0 {
        let sn = (rc * (1.0 - rc) / nc + rt * (1.0 - rt) / nt).sqrt();
        if sn > 0.0 {
            naive = json!(2.0 * (1.0 - phi(d.abs() / sn)));
        }
    }
    let (rel, rel_se) = if rc != 0.0 { (Some(d / rc), Some((vt / rc.powi(2) + rt.powi(2) * vc / rc.powi(4)).sqrt())) } else { (None, None) };
    json!({"diff": d, "se": se, "ci95": [d - q * se, d + q * se], "p": p, "naive_p": naive, "df": df as i64,
           "lift": rel, "lift_ci95": match (rel, rel_se) { (Some(r), Some(s)) => json!([r - q * s, r + q * s]), _ => Value::Null }})
        .as_object().unwrap().clone()
}

fn holm(cs: &mut [&mut Map<String, Value>]) {
    let mut ps: Vec<(f64, usize)> = cs.iter().enumerate().filter_map(|(i, c)| c.get("p").and_then(Value::as_f64).map(|p| (p, i))).collect();
    ps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let m = ps.len();
    let mut running = 0.0f64;
    for (j, (p, i)) in ps.iter().enumerate() {
        running = running.max(((m - j) as f64 * p).min(1.0));
        cs[*i].insert("p_holm".into(), json!(running));
    }
}

fn phi(x: f64) -> f64 {
    0.5 * (1.0 + libm::erf(x / 2f64.sqrt()))
}

fn lgamma(x: f64) -> f64 {
    libm::lgamma(x)
}

fn betacf(a: f64, b: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let gg = |v: f64| if v.abs() > tiny { v } else { tiny };
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 / gg(1.0 - qab * x / qap);
    let mut h = d;
    for m in 1..300 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 / gg(1.0 + aa * d);
        c = gg(1.0 + aa / c);
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 / gg(1.0 + aa * d);
        c = gg(1.0 + aa / c);
        let de = d * c;
        h *= de;
        if (de - 1.0).abs() < 1e-13 {
            break;
        }
    }
    h
}

fn betainc(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let lb = lgamma(a + b) - lgamma(a) - lgamma(b) + a * x.ln() + b * (1.0 - x).ln();
    if x < (a + 1.0) / (a + b + 2.0) {
        lb.exp() * betacf(a, b, x) / a
    } else {
        1.0 - lb.exp() * betacf(b, a, 1.0 - x) / b
    }
}

pub fn t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let p = 0.5 * betainc(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - p } else { p }
}

pub fn t_q(q: f64, df: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 50.0);
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        if t_cdf(mid, df) < q {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

fn gammainc_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    if x < a + 1.0 {
        let (mut s, mut term, mut n) = (1.0 / a, 1.0 / a, a);
        for _ in 0..500 {
            n += 1.0;
            term *= x / n;
            s += term;
            if term.abs() < s.abs() * 1e-14 {
                break;
            }
        }
        return 1.0 - s * (-x + a * x.ln() - lgamma(a)).exp();
    }
    let (mut b, mut c, mut d) = (x + 1.0 - a, 1e300, 1.0 / (x + 1.0 - a));
    let mut h = d;
    for i in 1..500 {
        let i = i as f64;
        let an = -i * (i - a);
        b += 2.0;
        d = an * d + b;
        d = 1.0 / if d.abs() > 1e-300 { d } else { 1e-300 };
        c = b + an / c;
        c = if c.abs() > 1e-300 { c } else { 1e-300 };
        let de = d * c;
        h *= de;
        if (de - 1.0).abs() < 1e-14 {
            break;
        }
    }
    (-x + a * x.ln() - lgamma(a)).exp() * h
}

pub fn chi2_sf(x: f64, df: usize) -> f64 {
    if df > 0 { gammainc_q(df as f64 / 2.0, x / 2.0) } else { 1.0 }
}

fn segments(units: &[Unit], arms: &[String], control: &str, dim: usize, level: f64) -> Vec<Value> {
    let mut values: IndexMap<String, Vec<&Unit>> = IndexMap::new();
    for u in units {
        values.entry(u.dims[dim].clone()).or_default().push(u);
    }
    let mut vs: Vec<(String, Vec<&Unit>)> = values.into_iter().collect();
    vs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let names = metric_names();
    vs.into_iter().map(|(v, us)| {
        let stats: IndexMap<&String, ArmStats> = arms.iter().map(|a| (a, arm_stats(us.iter().copied().filter(|u| &u.arm == a)))).collect();
        let mut arms_o = Map::new();
        for a in arms {
            let st = &stats[a];
            let mut o = json!({"pairs": st.pairs, "agents": st.agents});
            for (i, m) in names.iter().enumerate() {
                o[*m] = summ(st, i, 0.95)["rate"].clone();
            }
            arms_o.insert(a.clone(), o);
        }
        let mut diffs = Map::new();
        for a in arms {
            if a != control && stats[a].pairs > 0 && stats[&control.to_string()].pairs > 0 {
                let mut d = Map::new();
                for (i, m) in names.iter().enumerate() {
                    let c = compare(&stats[&control.to_string()], &stats[a], i, level);
                    d.insert(m.to_string(), json!({"diff": c["diff"], "ci95": c["ci95"], "p": c["p"]}));
                }
                diffs.insert(a.clone(), Value::Object(d));
            }
        }
        json!({"value": v, "pairs": us.len(), "arms": arms_o, "diffs": diffs})
    }).collect()
}

#[allow(clippy::too_many_arguments)]
fn series(units: &[Unit], arms: &[String], control: &str, metric: usize, t0: f64, t1: f64, level: f64) -> Value {
    let span = (t1 - t0).max(1.0);
    let width = [300.0, 900.0, 1800.0, 3600.0, 6.0 * 3600.0, 86400.0].into_iter().find(|w| span / w <= 47.0).unwrap_or(86400.0);
    let nb = ((span / width).ceil() as usize).max(1);
    let mut cum: IndexMap<String, ArmStats> = arms.iter().map(|a| (a.clone(), ArmStats::default())).collect();
    let mut rows: Vec<&Unit> = units.iter().collect();
    rows.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    let mut j = 0;
    let mut out = Vec::new();
    for i in 0..nb {
        let end = t0 + (i + 1) as f64 * width;
        let mut here: HashMap<&String, [i64; 2]> = arms.iter().map(|a| (a, [0, 0])).collect();
        while j < rows.len() && rows[j].t < end {
            let u = rows[j];
            if let Some(st) = cum.get_mut(&u.arm) {
                let p = st.per.entry(u.agent.clone()).or_default();
                p.n += 1;
                for (k, b) in u.m.iter().enumerate() {
                    p.m[k] += *b as i64;
                }
            }
            if let Some(h) = here.get_mut(&u.arm) {
                h[0] += 1;
                h[1] += u.m[metric] as i64;
            }
            j += 1;
        }
        for st in cum.values_mut() {
            st.agents = st.per.len();
            st.pairs = st.per.values().map(|p| p.n).sum();
        }
        let mut pa = Map::new();
        for a in arms {
            let (r, _) = ratio(&cum[a], metric);
            let h = here[a];
            pa.insert(a.clone(), json!({"pairs": h[0], "rate": if h[0] > 0 { json!(h[1] as f64 / h[0] as f64) } else { Value::Null },
                                        "cum_rate": if cum[a].pairs > 0 { json!(r) } else { Value::Null }, "cum_pairs": cum[a].pairs}));
        }
        let mut diffs = Map::new();
        for a in arms {
            if a != control && cum[a].pairs > 0 && cum.get(control).is_some_and(|c| c.pairs > 0) {
                let c = compare(&cum[control], &cum[a], metric, level);
                diffs.insert(a.clone(), json!({"diff": c["diff"], "ci95": c["ci95"], "p": c["p"]}));
            }
        }
        out.push(json!({"t": iso(t0 + i as f64 * width), "arms": pa, "diffs": diffs}));
    }
    json!({"width_hours": width / 3600.0, "points": out})
}

fn verdict(arms: &[Value], comps: &[Value], metric: &str, control: &str, srm_ok: bool, level: f64) -> String {
    if !arms.iter().any(|a| a["pairs"].as_i64().unwrap_or(0) > 0) {
        return "No impressions yet: the agents have not been shown their feeds in this experiment.".into();
    }
    if !srm_ok {
        return "The agents are not split as the weights say (sample ratio check failed): look for a bug before reading the results.".into();
    }
    let mut wins = Vec::new();
    for c in comps {
        let x = &c["metrics"][metric];
        let p = x.get("p_holm").or(x.get("p")).and_then(Value::as_f64);
        if let Some(p) = p.filter(|p| *p < 1.0 - level) {
            let d = x["diff"].as_f64().unwrap_or(0.0);
            wins.push(format!("{} {} {control} by {:.1} points (p = {p:.3}, Holm)", c["arm"].as_str().unwrap_or(""), if d > 0.0 { "beats" } else { "loses to" }, d.abs() * 100.0));
        }
    }
    if !wins.is_empty() {
        return format!("{}.", wins.join("; "));
    }
    format!("No arm differs from {control} on {metric} yet in a way the data can tell from chance (Holm-corrected, agents as the unit).")
}

// -- the experiments, as rows ---------------------------------------------------

fn row_settings(out: &mut Map<String, Value>) {
    let cfg = parsed(out.remove("config").as_ref());
    let mut s = settings_default();
    if let Some(c) = cfg.as_object() {
        for (k, v) in c {
            s.insert(k.clone(), v.clone());
        }
    }
    out.insert("settings".into(), Value::Object(s));
}

/// A row as the API shows it: arms with their description, SQL and knobs.
fn row_out(r: Row) -> Value {
    let mut out = r;
    for k in ["arms", "result"] {
        if let Some(v) = out.get(k).cloned() {
            out.insert(k.into(), parsed(Some(&v)));
        }
    }
    row_settings(&mut out);
    let arms: Vec<Value> = out.get("arms").and_then(Value::as_array).cloned().unwrap_or_default().iter().map(|a| {
        let spec = arm_spec(a);
        let mut o = obj(a);
        let about = if truthy(&a["about"]) { a["about"].clone() } else { json!(describe(&spec)) };
        o.insert("about".into(), about);
        o.insert("sql".into(), json!(compile_order(&spec)));
        o.insert("knobs".into(), knobs(&spec, true));
        Value::Object(o)
    }).collect();
    out.insert("arms".into(), json!(arms));
    out.retain(|_, v| !v.is_null());
    Value::Object(out)
}

fn raw_out(r: Row) -> Map<String, Value> {
    let mut out = r;
    for k in ["arms", "result"] {
        if let Some(v) = out.get(k).cloned() {
            out.insert(k.into(), parsed(Some(&v)));
        }
    }
    row_settings(&mut out);
    out.retain(|_, v| !v.is_null());
    out
}

impl App {
    async fn exp_save(&self, row: &Map<String, Value>) -> Result<()> {
        let mut doc: Map<String, Value> = row.iter().filter(|(k, _)| k.as_str() != "settings").map(|(k, v)| (k.clone(), v.clone())).collect();
        let settings = row.get("settings").filter(|s| truthy(s)).cloned().unwrap_or_else(|| Value::Object(settings_default()));
        doc.insert("config".into(), json!(settings.to_string()));
        for k in ["arms", "result"] {
            if let Some(v) = doc.get(k).cloned() {
                if !v.is_string() {
                    doc.insert(k.into(), json!(v.to_string()));
                }
            }
        }
        self.db.write("experiments:save", vec![json!({"op": "upsert", "table": "experiments", "doc": doc})]).await?;
        self.exp.lock().unwrap().running = None;
        Ok(())
    }

    async fn exp_raw(&self, eid: &str) -> Result<Map<String, Value>> {
        match self.db.one("experiments:get", "SELECT * FROM experiments WHERE id = :id", json!({"id": eid})).await? {
            Some(r) => Ok(raw_out(r)),
            None => err(404, format!("no experiment {eid}")),
        }
    }

    pub async fn exp_get(&self, eid: &str) -> Result<Value> {
        match self.db.one("experiments:get", "SELECT * FROM experiments WHERE id = :id", json!({"id": eid})).await? {
            Some(r) => Ok(row_out(r)),
            None => err(404, format!("no experiment {eid}")),
        }
    }

    pub async fn exp_list(&self) -> Result<Vec<Value>> {
        self.exp_running(true).await?;
        let mut rows: Vec<Value> = self.db.q("experiments:list", "SELECT * FROM experiments ORDER BY created DESC LIMIT 200", json!({})).await?.into_iter().map(row_out).collect();
        let key = |r: &Value| -> (i32, f64, f64) {
            let st = r["status"].as_str().unwrap_or("");
            let o = match st { "running" => 0, "queued" => 1, "done" => 2, _ => 3 };
            let queued = st == "queued";
            let pr = if queued { -fnum(&r["settings"]["priority"]) } else { 0.0 };
            let created = if truthy(&r["created"]) { epoch(&r["created"]) } else { 0.0 };
            let t = if queued { created } else {
                let s = if truthy(&r["started"]) { &r["started"] } else { &r["created"] };
                -(if truthy(s) { epoch(s) } else { 0.0 })
            };
            (o, pr, t)
        };
        rows.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
        Ok(rows)
    }

    /// The running experiment (cached 20 seconds); with `rules`, stopped first if its settings say it is done.
    pub fn exp_running(&self, rules: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Value>>> + Send + '_>> {
        Box::pin(async move {
            let ensure = { let mut st = self.exp.lock().unwrap(); let e = !st.ensured; st.ensured = true; e };
            if ensure && self.exp_ensure().await.is_err() {
                self.exp.lock().unwrap().ensured = false;
            }
            if let Some((t, row)) = self.exp.lock().unwrap().running.clone() {
                if now() - t < 20.0 {
                    return Ok(row);
                }
            }
            let r = self.db.one("experiments:running", "SELECT * FROM experiments WHERE status = 'running' LIMIT 1", json!({})).await?;
            let row = r.map(row_out);
            self.exp.lock().unwrap().running = Some((now(), row.clone()));
            match row {
                Some(row) if rules => self.exp_rules(row).await,
                row => Ok(row),
            }
        })
    }

    async fn exp_rules(&self, row: Value) -> Result<Option<Value>> {
        let st = &row["settings"];
        if !(truthy(&st["stop_after_h"]) || truthy(&st["stop_after_agents"])) {
            return Ok(Some(row));
        }
        let s0 = if truthy(&row["started"]) { &row["started"] } else { &row["created"] };
        let started = if truthy(s0) { epoch(s0) } else { now() };
        let mut why = None;
        if truthy(&st["stop_after_h"]) && now() - started >= fnum(&st["stop_after_h"]) * 3600.0 {
            why = Some(format!("ran {} hours", g6(fnum(&st["stop_after_h"]))));
        } else if truthy(&st["stop_after_agents"]) && now() - self.exp.lock().unwrap().rules_at > 120.0 {
            self.exp.lock().unwrap().rules_at = now();
            let arms: Vec<String> = row["arms"].as_array().into_iter().flatten().filter_map(|a| logged_arm(Some(&row), a["arm"].as_str().unwrap_or(""))).collect();
            let n = self.db.q("experiments:agents", "SELECT agent, COUNT(*) AS n FROM impressions WHERE ts > :s AND arm IN (:arms) GROUP BY agent",
                json!({"s": iso(started - 1.0), "arms": arms})).await?.len();
            if n as f64 >= fnum(&st["stop_after_agents"]) {
                why = Some(format!("reached {n} agents"));
            }
        }
        let Some(why) = why else { return Ok(Some(row)) };
        let Ok(_guard) = self.exp_lock.try_lock() else { return Ok(Some(row)) };
        self.exp_stop_inner(row["id"].as_str().unwrap_or(""), &why).await?;
        self.exp_running(false).await
    }

    /// The first experiment and the proposals, as rows, once.
    pub async fn exp_ensure(&self) -> Result<Vec<String>> {
        let have: HashSet<String> = self.db.q("experiments:ids", "SELECT id FROM experiments LIMIT 500", json!({})).await?.iter().map(|r| r.s("id")).collect();
        let mut made = Vec::new();
        if !have.contains(LEGACY) {
            let first = self.db.one("experiments:first", "SELECT ts FROM impressions ORDER BY ts ASC LIMIT 1", json!({})).await?;
            let mut row = obj(&legacy_row());
            row.insert("status".into(), json!("running"));
            row.insert("started".into(), first.and_then(|f| f.get("ts").cloned()).filter(truthy).unwrap_or_else(|| json!(iso(now()))));
            row.insert("created_by".into(), json!("parley"));
            self.exp_save(&row).await?;
            made.push(LEGACY.to_string());
        }
        for p in proposals() {
            let id = p["id"].as_str().unwrap_or("").to_string();
            if !have.contains(&id) {
                let mut row = obj(&p);
                row.insert("status".into(), json!("queued"));
                row.insert("created_by".into(), json!("parley"));
                self.exp_save(&row).await?;
                made.push(id);
            }
        }
        Ok(made)
    }

    async fn exp_clean(&self, body: &Value, old: Option<&Map<String, Value>>) -> Result<Map<String, Value>> {
        let pick = |k: &str| body.get(k).cloned().or_else(|| old.and_then(|o| o.get(k).cloned())).unwrap_or(Value::Null);
        let name: String = match pick("name") { Value::String(s) => s, Value::Null => String::new(), v => v.to_string() }.trim().chars().take(120).collect();
        if name.is_empty() {
            return err(400, "an experiment needs a name");
        }
        let arms_in = pick("arms");
        let arms_in = if truthy(&arms_in) { arms_in } else { json!([]) };
        let Some(arms_in) = arms_in.as_array() else { return err(400, "arms is a list") };
        let mut clean = Vec::new();
        let mut seen = HashSet::new();
        let rec_on = self.db.recommender.load(std::sync::atomic::Ordering::Relaxed);
        for (i, a) in arms_in.iter().enumerate() {
            let c = clean_arm(a, i)?;
            let name = c["arm"].as_str().unwrap_or("").to_string();
            if seen.contains(&name) {
                return err(400, format!("arm {name} twice: give each arm its own name"));
            }
            if needs_recommender(&arm_spec(&Value::Object(c.clone()))) && !rec_on {
                return err(400, format!("arm {name} uses the recommender, which this Parley does not have"));
            }
            seen.insert(name);
            clean.push(c);
        }
        if !(2..=8).contains(&clean.len()) {
            return err(400, "an experiment has 2 to 8 arms");
        }
        let control = pick("control");
        let control = if truthy(&control) { control.as_str().map(str::to_string).unwrap_or_else(|| control.to_string()) } else { clean[0]["arm"].as_str().unwrap_or("").to_string() };
        if !seen.contains(&control) {
            return err(400, "the control is one of the arms");
        }
        let metric = pick("metric");
        let metric = if truthy(&metric) { metric.as_str().unwrap_or("").to_string() } else { "engaged".into() };
        if !METRICS.iter().any(|m| m.0 == metric) {
            return err(400, format!("the metric is one of {}", metric_names().join(", ")));
        }
        for c in &clean {
            if truthy(&c.get("order").cloned().unwrap_or(Value::Null)) {
                self.exp_try(c).await?;
            }
        }
        let hyp = pick("hypothesis");
        let hyp: String = if truthy(&hyp) { hyp.as_str().map(str::to_string).unwrap_or_else(|| hyp.to_string()) } else { String::new() };
        let old_settings = old.and_then(|o| o.get("settings")).and_then(Value::as_object);
        let mut out = Map::new();
        out.insert("name".into(), json!(name));
        out.insert("hypothesis".into(), json!(hyp.chars().take(600).collect::<String>()));
        out.insert("arms".into(), json!(clean));
        out.insert("control".into(), json!(control));
        out.insert("metric".into(), json!(metric));
        out.insert("settings".into(), Value::Object(clean_settings(body, old_settings)?));
        Ok(out)
    }

    /// Runs a hand-written ranking once, so a mistake shows now, not in the feed.
    async fn exp_try(&self, arm: &Map<String, Value>) -> Result<()> {
        let me = self.db.one("experiments:try", "SELECT id FROM agents LIMIT 1", json!({})).await?.map(|r| r.s("id")).unwrap_or_else(|| "nobody".into());
        self.rank(&me, &arm_spec(&Value::Object(arm.clone())), 1, "a test", "feed_for_you").await
            .map(|_| ()).map_err(|e| Error::new(400, format!("arm {}: the ORDER BY does not run: {}", arm["arm"].as_str().unwrap_or(""), e.message.chars().take(300).collect::<String>())))
    }

    pub async fn exp_create(&self, body: &Value, by: &str) -> Result<Value> {
        let mut row = self.exp_clean(body, None).await?;
        let id = new_id("x");
        row.insert("id".into(), json!(id));
        row.insert("status".into(), json!("queued"));
        row.insert("created_by".into(), json!(by.chars().take(40).collect::<String>()));
        self.exp_save(&row).await?;
        if truthy(&body["start"]) {
            return self.exp_start(&id).await;
        }
        self.exp_get(&id).await
    }

    pub async fn exp_update(&self, eid: &str, body: &Value) -> Result<Value> {
        {
            let _g = self.exp_lock.lock().await;
            let old = self.exp_raw(eid).await?;
            if old.get("status").and_then(Value::as_str) != Some("queued") {
                return err(409, "only a queued experiment can be changed: copy this one into a new one");
            }
            let mut row = old.clone();
            for (k, v) in self.exp_clean(body, Some(&old)).await? {
                row.insert(k, v);
            }
            self.exp_save(&row).await?;
        }
        if truthy(&body["start"]) {
            return self.exp_start(eid).await;
        }
        self.exp_get(eid).await
    }

    pub async fn exp_delete(&self, eid: &str) -> Result<Value> {
        let _g = self.exp_lock.lock().await;
        let old = self.exp_raw(eid).await?;
        if old.get("status").and_then(Value::as_str) != Some("queued") {
            return err(409, "only a queued experiment can be deleted; a finished one keeps its result");
        }
        self.db.write("experiments:delete", vec![json!({"op": "delete", "table": "experiments", "key": eid})]).await?;
        Ok(json!({"deleted": eid}))
    }

    /// Puts a queued experiment first in the queue.
    pub async fn exp_to_front(&self, eid: &str) -> Result<Value> {
        let _g = self.exp_lock.lock().await;
        let mut old = self.exp_raw(eid).await?;
        if old.get("status").and_then(Value::as_str) != Some("queued") {
            return err(409, "that experiment is not in the queue");
        }
        let top = self.exp_list().await?.iter().filter(|r| r["status"] == "queued").map(|r| fnum(&r["settings"]["priority"]) as i64).max().unwrap_or(0);
        if let Some(Value::Object(s)) = old.get_mut("settings") {
            s.insert("priority".into(), json!(top + 1));
        }
        self.exp_save(&old).await?;
        self.exp_get(eid).await
    }

    pub async fn exp_start(&self, eid: &str) -> Result<Value> {
        let _g = self.exp_lock.lock().await;
        self.exp_start_inner(eid).await
    }

    async fn exp_start_inner(&self, eid: &str) -> Result<Value> {
        let mut row = self.exp_raw(eid).await?;
        match row.get("status").and_then(Value::as_str) {
            Some("running") => return self.exp_get(eid).await,
            Some("done") => return err(409, "that experiment is done: copy it into a new one"),
            _ => {}
        }
        let t = iso(now());
        if let Some(cur) = self.exp_running(false).await? {
            let raw = self.exp_raw(cur["id"].as_str().unwrap_or("")).await?;
            self.exp_finish(raw, &t, "another experiment started").await?;
        }
        row.insert("status".into(), json!("running"));
        row.insert("started".into(), json!(t));
        self.exp_save(&row).await?;
        self.exp_get(eid).await
    }

    pub async fn exp_stop(&self, eid: &str) -> Result<Value> {
        let _g = self.exp_lock.lock().await;
        self.exp_stop_inner(eid, "stopped by hand").await
    }

    async fn exp_stop_inner(&self, eid: &str, reason: &str) -> Result<Value> {
        let row = self.exp_raw(eid).await?;
        if row.get("status").and_then(Value::as_str) != Some("running") {
            return err(409, "that experiment is not running");
        }
        let auto = truthy(&row["settings"]["auto_next"]);
        self.exp_finish(row, &iso(now()), reason).await?;
        if auto {
            let q: Vec<Value> = self.exp_list().await?.into_iter().filter(|r| r["status"] == "queued").collect();
            if let Some(n) = q.first().and_then(|r| r["id"].as_str()) {
                self.exp_start_inner(n).await?;
            }
        }
        self.exp_get(eid).await
    }

    async fn exp_finish(&self, row: Map<String, Value>, t: &str, reason: &str) -> Result<()> {
        let mut row = row;
        row.insert("status".into(), json!("done"));
        row.insert("ended".into(), json!(t));
        let result = match self.exp_analyze(Value::Object(row.clone())).await {
            Ok(a) => {
                let metric = a["metric"].as_str().unwrap_or("engaged").to_string();
                json!({"verdict": a["verdict"], "reason": reason, "arms": a["arms"].as_array().into_iter().flatten()
                    .map(|x| json!({"arm": x["arm"], "agents": x["agents"], "pairs": x["pairs"], "rate": x["metrics"][&metric]["rate"]})).collect::<Vec<_>>()})
            }
            Err(e) => json!({"verdict": format!("not analysed: {}", e.message), "reason": reason}),
        };
        row.insert("result".into(), result);
        self.exp_save(&row).await
    }

    // -- the feed -----------------------------------------------------------

    /// (the running experiment or none, my arm, its spec). An agent outside
    /// the experiment's share gets the control's ranking, unlogged.
    pub async fn assign(&self, agent: &str) -> Result<(Option<Value>, String, Map<String, Value>)> {
        let rec_on = self.db.recommender.load(std::sync::atomic::Ordering::Relaxed);
        let Some(exp) = self.exp_running(true).await? else {
            let name = if rec_on { "recommender" } else { "vector" };
            return Ok((None, name.into(), arm_spec(&json!(name))));
        };
        let arms: Vec<Value> = exp["arms"].as_array().cloned().unwrap_or_default();
        let specs: HashMap<String, Map<String, Value>> = arms.iter().map(|a| (a["arm"].as_str().unwrap_or("").to_string(), arm_spec(a))).collect();
        let spec = |a: &str| specs.get(a).cloned().unwrap_or_else(|| arm_spec(&json!(a)));
        let id = exp["id"].as_str().unwrap_or("").to_string();
        if id == LEGACY {
            if !rec_on {
                return Ok((Some(exp), "vector".into(), spec("vector")));
            }
            let arm = if sha(&format!("{LEGACY}|{agent}"))[0] % 2 == 1 { "recommender" } else { "vector" };
            return Ok((Some(exp), arm.into(), spec(arm)));
        }
        let share = { let s = fnum(&exp["settings"]["share"]); if s == 0.0 { 1.0 } else { s } };
        let control = exp["control"].as_str().unwrap_or("").to_string();
        if share < 1.0 && unit(&format!("{id}|share|{agent}")) >= share {
            return Ok((None, control.clone(), spec(&control)));
        }
        let total: f64 = arms.iter().map(|a| fnum(&a["weight"])).sum();
        let mut u = unit(&format!("{id}|{agent}")) * total;
        for a in &arms {
            u -= fnum(&a["weight"]);
            if u < 0.0 {
                let n = a["arm"].as_str().unwrap_or("");
                return Ok((Some(exp.clone()), n.into(), spec(n)));
            }
        }
        let last = arms.last().and_then(|a| a["arm"].as_str()).unwrap_or("vector").to_string();
        Ok((Some(exp), last.clone(), spec(&last)))
    }

    pub async fn circle_of(&self, me: &str, n: usize) -> Vec<String> {
        self.db.q("feed:circle", &format!("SELECT dst FROM talks WHERE src = :me ORDER BY w DESC LIMIT {n}"), json!({"me": me})).await
            .map(|rows| rows.iter().map(|r| r.s("dst")).collect()).unwrap_or_default()
    }

    pub async fn communities_of(&self, me: &str) -> Vec<String> {
        self.db.q("feed:communities", "SELECT community FROM members WHERE agent = :a LIMIT 50", json!({"a": me})).await
            .map(|rows| rows.iter().map(|r| r.s("community")).collect()).unwrap_or_default()
    }

    /// The posts an arm shows me, best first.
    pub async fn rank(&self, me: &str, spec: &Map<String, Value>, limit: usize, taste: &str, label: &str) -> Result<Vec<Row>> {
        let order = compile_order(spec);
        let since = iso(now() - fnum(&spec["window_h"]) * 3600.0);
        let mut params = json!({"me": me, "since": since, "taste": {"embed": taste, "field": "emb", "table": "posts"}});
        let mut wh = "author != :me AND ts >= :since AND mod = 'ok'".to_string();
        if order.contains(":circle") {
            let c = self.circle_of(me, 15).await;
            params["circle"] = if c.is_empty() { json!(["-"]) } else { json!(c) };
        }
        if order.contains(":mine") {
            let c = self.communities_of(me).await;
            params["mine"] = if c.is_empty() { json!(["-"]) } else { json!(c) };
        }
        if truthy(&spec["only_kinds"]) {
            wh.push_str(" AND kind IN (:kinds)");
            params["kinds"] = spec["only_kinds"].clone();
        }
        let mut seen: HashSet<String> = HashSet::new();
        if truthy(&spec["unseen"]) {
            seen = self.db.q(&format!("{label}:seen"), "SELECT post FROM impressions WHERE agent = :me AND ts >= :since LIMIT 2000", json!({"me": me, "since": params["since"]})).await?
                .iter().map(|r| r.s("post")).collect();
        }
        let cap = fnum(&spec["per_author"]) as i64;
        let fetch = limit * if cap != 0 || !seen.is_empty() { 3 } else { 1 } + seen.len().min(200);
        let rows = self.db.q(label, &format!("SELECT {}, score() FROM posts WHERE {wh} ORDER BY {order} LIMIT {}", post_cols(), fetch.min(1000)), params).await?;
        let mut out = Vec::new();
        let mut per: HashMap<String, i64> = HashMap::new();
        for r in rows {
            if seen.contains(&r.s("id")) {
                continue;
            }
            let a = r.s("author");
            if cap != 0 && *per.get(&a).unwrap_or(&0) >= cap {
                continue;
            }
            *per.entry(a).or_insert(0) += 1;
            out.push(r);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn taste_of(&self, me: &str) -> Result<String> {
        let a = self.agent(me).await?;
        let mine = self.db.q("taste", "SELECT body FROM posts WHERE author = :a ORDER BY ts DESC LIMIT 5", json!({"a": me})).await?;
        let mut bits = vec![a["bio"].as_str().unwrap_or("").to_string()];
        bits.extend(mine.iter().map(|p| p.s("body")));
        let t: String = bits.join(" ").chars().take(2000).collect();
        Ok(if t.is_empty() { a["name"].as_str().unwrap_or("").to_string() } else { t })
    }

    /// Posts from anyone, ranked for me by my arm of the running experiment;
    /// with `log`, the posts shown are the experiment's impressions.
    pub async fn feed_for_you(&self, me: &str, limit: usize, log: bool) -> Result<Vec<Value>> {
        let taste = self.taste_of(me).await?;
        let (mut exp, mut arm, spec) = self.assign(me).await?;
        let rows = match self.rank(me, &spec, limit, &taste, &format!("feed_for_you:{arm}")).await {
            Ok(rows) => rows,
            Err(e) => {
                if exp.is_some() && !truthy(&spec["order"]) && !truthy(&spec["mix"]["recommender"]) {
                    return Err(e);
                }
                // No recommender (a coordinator, or not trained yet), or a
                // ranking that fails: the vector feed, not logged.
                exp = None;
                arm = "vector".into();
                self.rank(me, &arm_spec(&json!("vector")), limit, &taste, "feed_for_you:fallback").await?
            }
        };
        let mut posts: Vec<Value> = rows.iter().map(post_out).collect();
        let logged = logged_arm(exp.as_ref(), &arm);
        if log && !posts.is_empty() {
            if let Some(l) = &logged {
                let ops = posts.iter().enumerate().map(|(i, p)| json!({"op": "insert", "table": "impressions",
                    "doc": {"id": new_id("i"), "agent": me, "post": p["id"], "arm": l, "pos": i}})).collect();
                let _ = self.db.write("impressions", ops).await;
            }
        }
        for p in posts.iter_mut() {
            p["arm"] = json!(arm);
            if let Some(e) = &exp {
                p["experiment"] = e["id"].clone();
            }
        }
        Ok(posts)
    }

    /// What an arm would show an agent, and its SQL, without logging.
    pub async fn exp_preview(&self, body: &Value) -> Result<Value> {
        let arm = clean_arm(&if truthy(&body["arm"]) { body["arm"].clone() } else { json!({}) }, 0)?;
        let spec = arm_spec(&Value::Object(arm.clone()));
        if needs_recommender(&spec) && !self.db.recommender.load(std::sync::atomic::Ordering::Relaxed) {
            return err(400, "this arm uses the recommender, which this Parley does not have");
        }
        let given = match &body["agent"] { Value::String(s) => s.clone(), Value::Null => String::new(), v => v.to_string() };
        let mut me = given.to_lowercase().trim_start_matches('@').to_string();
        if me.is_empty() {
            me = self.db.q("experiments:preview:agent", "SELECT id FROM agents ORDER BY n_posts DESC LIMIT 1", json!({})).await?.first().map(|r| r.s("id")).unwrap_or_else(|| "nobody".into());
        }
        let taste = self.taste_of(&me).await?;
        let t0 = std::time::Instant::now();
        let limit = num(body.get("limit").or(Some(&json!(8))), 1.0, 20.0, 8.0, "limit")? as usize;
        let rows = self.rank(&me, &spec, limit, &taste, "experiments:preview").await
            .map_err(|e| Error::new(400, format!("the ranking does not run: {}", e.message.chars().take(300).collect::<String>())))?;
        Ok(json!({"agent": me, "arm": arm, "order": compile_order(&spec), "about": describe(&spec),
                  "posts": rows.iter().map(|r| {
                      let b = if r.s("body").is_empty() { r.s("meme_top") } else { r.s("body") };
                      json!({"id": r.get("id"), "author": r.get("author"), "kind": if r.s("kind").is_empty() { "text".to_string() } else { r.s("kind") },
                             "community": r.get("community"), "body": b.chars().take(160).collect::<String>(), "score": r.get("score"), "ts": r.get("ts")})
                  }).collect::<Vec<_>>(),
                  "took_ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1)}))
    }

    /// The experiments page: the list, the presets and what an arm can say.
    pub async fn exp_page(&self, admin_open: bool) -> Result<Value> {
        let presets: Map<String, Value> = catalog().as_object().unwrap().iter().map(|(k, v)| {
            let spec = arm_spec(&json!(k));
            let needs = if truthy(&v["mix"]["recommender"]) { json!("recommender") } else { Value::Null };
            (k.clone(), json!({"about": v["about"], "needs": needs, "knobs": knobs(&spec, false), "sql": compile_order(&spec)}))
        }).collect();
        Ok(json!({"experiments": self.exp_list().await?, "catalog": presets,
                  "signals": SIGNALS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(), "kinds": KINDS,
                  "rec_lists": REC_LISTS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
                  "settings": settings_default(), "levels": LEVELS,
                  "metrics": METRICS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
                  "recommender_on": self.db.recommender.load(std::sync::atomic::Ordering::Relaxed), "admin": admin_open}))
    }

    // -- the analysis ---------------------------------------------------------

    /// Every row, 10,000 at a time by ts (a query's most).
    async fn paged(&self, sql: &str, params: Value, label: &str) -> Result<Vec<Row>> {
        let mut out = Vec::new();
        let mut after = params["s"].clone();
        while out.len() < 100_000 {
            let mut p = params.clone();
            p["s"] = after.clone();
            let rows = self.db.q(label, sql, p).await?;
            let n = rows.len();
            if let Some(last) = rows.last() {
                after = match &last["ts"] { Value::Number(_) => json!(iso(epoch(&last["ts"]))), v => v.clone() };
            }
            out.extend(rows);
            if n < 10000 {
                break;
            }
        }
        Ok(out)
    }

    pub async fn exp_analyze_id(&self, eid: &str) -> Result<Value> {
        let e = self.exp_get(eid).await?;
        self.exp_analyze(e).await
    }

    pub async fn exp_analyze(&self, exp: Value) -> Result<Value> {
        let t0 = std::time::Instant::now();
        let start = if truthy(&exp["started"]) { exp["started"].clone() } else { exp["created"].clone() };
        let s_epoch = if truthy(&start) { epoch(&start) } else { now() - 30.0 * 86400.0 };
        let e_epoch = if truthy(&exp["ended"]) { epoch(&exp["ended"]) } else { now() };
        let mut cfg = settings_default();
        if let Some(s) = exp["settings"].as_object() {
            for (k, v) in s {
                cfg.insert(k.clone(), v.clone());
            }
        }
        let win = { let w = fnum(&cfg["window_h"]); if w == 0.0 { 24.0 } else { w } } * 3600.0;
        let level = { let l = fnum(&cfg["level"]); if l == 0.0 { 0.95 } else { l } };
        let arm_rows: Vec<Value> = exp["arms"].as_array().cloned().unwrap_or_default();
        let names: Vec<String> = arm_rows.iter().map(|a| a["arm"].as_str().unwrap_or("").to_string()).collect();
        let logged: IndexMap<String, String> = names.iter().filter_map(|a| logged_arm(Some(&exp), a).map(|l| (l, a.clone()))).collect();
        let since = iso(s_epoch - 1.0);
        let imps: Vec<Row> = self.paged("SELECT agent, post, arm, pos, ts FROM impressions WHERE ts > :s AND arm IN (:arms) ORDER BY ts ASC LIMIT 10000",
            json!({"s": since, "arms": logged.keys().collect::<Vec<_>>()}), "exp:impressions").await?
            .into_iter().filter(|r| epoch(&r["ts"]) <= e_epoch).collect();
        let agents: Vec<String> = imps.iter().map(|r| r.s("agent")).collect::<BTreeSet<_>>().into_iter().collect();
        let acts = if agents.is_empty() { Vec::new() } else {
            self.paged("SELECT agent, post, kind, ts FROM engagements WHERE ts > :s AND agent IN (:agents) ORDER BY ts ASC LIMIT 10000",
                json!({"s": since, "agents": agents}), "exp:engagements").await?
        };
        let mut first_act: HashMap<(String, String, String), f64> = HashMap::new();
        for r in &acts {
            let k = (r.s("agent"), r.s("post"), r.s("kind"));
            let t = epoch(&r["ts"]);
            if first_act.get(&k).is_none_or(|x| t < *x) {
                first_act.insert(k, t);
            }
        }
        let who: HashMap<String, Row> = self.db.q("exp:agents", "SELECT id, role, born_from FROM agents LIMIT 10000", json!({})).await?.into_iter().map(|r| (r.s("id"), r)).collect();
        let posts: Vec<String> = imps.iter().map(|r| r.s("post")).collect::<BTreeSet<_>>().into_iter().collect();
        let mut kind_of: HashMap<String, String> = HashMap::new();
        for chunk in posts.chunks(1000) {
            for r in self.db.q("exp:posts", "SELECT id, kind FROM posts WHERE id IN (:ids) LIMIT 1000", json!({"ids": chunk})).await? {
                let k = r.s("kind");
                kind_of.insert(r.s("id"), if k.is_empty() { "text".into() } else { k });
            }
        }
        // One unit per (agent, post): its first impression and what followed.
        let mut units: Vec<Unit> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for r in &imps {
            let k = (r.s("agent"), r.s("post"));
            if !seen.insert(k.clone()) {
                continue;
            }
            let t = epoch(&r["ts"]);
            let Some(arm) = logged.get(&r.s("arm")) else { continue };
            let did = |kind: &str| first_act.get(&(k.0.clone(), k.1.clone(), kind.to_string())).is_some_and(|x| t <= *x && *x <= t + win);
            let (reacted, commented, remixed, opened) = (did("react"), did("comment"), did("remix"), did("view"));
            let a = who.get(&k.0);
            let role = a.map(|a| a.s("role")).filter(|s| !s.is_empty()).unwrap_or_else(|| "none".into());
            let born = match a.map(|a| a.s("born_from")).unwrap_or_default().as_str() { "seed" => "founder", "generator" => "generated", _ => "guest" };
            let pos = r.i("pos");
            let position = if pos < 3 { "1-3" } else if pos < 8 { "4-8" } else { "9+" };
            units.push(Unit { agent: k.0.clone(), arm: arm.clone(), t, m: [reacted || commented || remixed, opened, reacted, commented, remixed],
                              dims: [role, born.into(), kind_of.get(&k.1).cloned().unwrap_or_else(|| "text".into()), position.into()] });
        }
        let metric = exp["metric"].as_str().filter(|s| !s.is_empty()).unwrap_or("engaged").to_string();
        let m_i = mi(&metric);
        let control = exp["control"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| names.first().cloned().unwrap_or_default());
        let stats: HashMap<&String, ArmStats> = names.iter().map(|a| (a, arm_stats(units.iter().filter(|u| &u.arm == a)))).collect();
        let empty = ArmStats::default();
        let st_of = |a: &str| stats.iter().find(|(k, _)| k.as_str() == a).map(|(_, v)| v).unwrap_or(&empty);
        let mnames = metric_names();
        let arms_out: Vec<Value> = arm_rows.iter().map(|a| {
            let st = st_of(a["arm"].as_str().unwrap_or(""));
            let about = if truthy(&a["about"]) { a["about"].clone() } else { json!(describe(&arm_spec(a))) };
            json!({"arm": a["arm"], "weight": a["weight"], "about": about, "agents": st.agents, "pairs": st.pairs,
                   "per_agent": if st.agents > 0 { st.pairs as f64 / st.agents as f64 } else { 0.0 },
                   "metrics": mnames.iter().enumerate().map(|(i, m)| (m.to_string(), summ(st, i, level))).collect::<Map<_, _>>()})
        }).collect();
        // Each treatment against the control, every metric; Holm on the primary.
        let mut comps_m: Vec<(String, Vec<Map<String, Value>>)> = names.iter().filter(|a| **a != control)
            .map(|a| (a.clone(), (0..mnames.len()).map(|i| compare(st_of(&control), st_of(a), i, level)).collect())).collect();
        {
            let mut prim: Vec<&mut Map<String, Value>> = comps_m.iter_mut().map(|(_, ms)| &mut ms[m_i]).collect();
            holm(&mut prim);
        }
        let comps: Vec<Value> = comps_m.into_iter().map(|(a, ms)| json!({"arm": a, "against": control,
            "metrics": mnames.iter().zip(ms).map(|(m, v)| (m.to_string(), Value::Object(v))).collect::<Map<_, _>>()})).collect();
        // Sample ratio: are agents split as the weights say?
        let total_w: f64 = arm_rows.iter().map(|a| fnum(&a["weight"])).sum();
        let n_agents: usize = names.iter().map(|a| st_of(a).agents).sum();
        let exp_n = |a: &Value| n_agents as f64 * fnum(&a["weight"]) / total_w;
        let chi: f64 = if n_agents > 0 { arm_rows.iter().map(|a| (st_of(a["arm"].as_str().unwrap_or("")).agents as f64 - exp_n(a)).powi(2) / exp_n(a)).sum() } else { 0.0 };
        let srm_p = if n_agents > 0 { Some(chi2_sf(chi, names.len().saturating_sub(1))) } else { None };
        let srm_ok = srm_p.is_none_or(|p| p >= 0.01);
        let srm = json!({"agents": names.iter().map(|a| (a.clone(), json!(st_of(a).agents))).collect::<Map<_, _>>(),
                         "expected": arm_rows.iter().map(|a| (a["arm"].as_str().unwrap_or("").to_string(), json!(round(exp_n(a), 1)))).collect::<Map<_, _>>(),
                         "chi2": chi, "p": srm_p, "ok": srm_ok});
        // Power: the smallest difference the data can tell from chance now.
        let mut power = json!({});
        let mde: Vec<f64> = comps.iter().filter_map(|c| c["metrics"][&metric]["se"].as_f64()).filter(|se| *se != 0.0).map(|se| 2.8 * se).collect();
        if !mde.is_empty() {
            let m = mde.iter().copied().fold(f64::MIN, f64::max);
            let k = n_agents.max(1) as f64;
            power = json!({"mde": m, "agents_for_5_points": if m > 0.0 { json!((k * (m / 0.05).powi(2)).ceil() as i64) } else { Value::Null }});
        }
        let t_end = e_epoch.min(now());
        let series_v = series(&units, &names, &control, m_i, s_epoch, t_end, level);
        let segs: Map<String, Value> = DIMS.iter().enumerate().map(|(i, d)| (d.to_string(), json!(segments(&units, &names, &control, i, level)))).collect();
        let mut by_agent: IndexMap<String, Vec<&Unit>> = IndexMap::new();
        for u in &units {
            by_agent.entry(u.agent.clone()).or_default().push(u);
        }
        let mut per_agent: Vec<Value> = by_agent.iter().map(|(ag, us)| {
            let mut o = json!({"agent": ag, "arm": us[0].arm, "role": us[0].dims[0], "pairs": us.len()});
            for (i, m) in mnames.iter().enumerate() {
                o[*m] = json!(us.iter().filter(|u| u.m[i]).count() as f64 / us.len() as f64);
            }
            o
        }).collect();
        per_agent.sort_by(|a, b| a["arm"].as_str().cmp(&b["arm"].as_str())
            .then(b[&metric].as_f64().unwrap_or(0.0).partial_cmp(&a[&metric].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)));
        let mut e_out = obj(&exp);
        e_out.remove("result");
        let verdict_s = verdict(&arms_out, &comps, &metric, &control, srm_ok, level);
        let about = METRICS.iter().find(|m| m.0 == metric).map(|m| m.1).unwrap_or("");
        Ok(json!({"experiment": e_out, "metric": metric, "metric_about": about,
                  "metrics": METRICS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
                  "control": control, "arms": arms_out, "comparisons": comps, "srm": srm, "power": power, "series": series_v, "segments": segs,
                  "per_agent": per_agent, "verdict": verdict_s, "level": level, "settings": cfg,
                  "window": {"from": iso(s_epoch), "to": iso(t_end), "hours": round((t_end - s_epoch) / 3600.0, 1)},
                  "units": units.len(), "analysis_ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1)}))
    }
}

pub fn logged_arm(exp: Option<&Value>, arm: &str) -> Option<String> {
    let exp = exp?;
    let id = exp["id"].as_str().unwrap_or("");
    Some(if id == LEGACY { arm.to_string() } else { format!("{id}:{arm}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_number_formats() {
        for (x, p, want) in [(0.6, 4, "0.6"), (1.0, 4, "1"), (0.67, 4, "0.67"), (21600.0, 4, "2.16e+04"), (0.2, 4, "0.2"), (1e-6, 6, "1e-06"),
                             (-1e6, 6, "-1e+06"), (100000.0, 6, "100000"), (0.999, 6, "0.999"), (12.5, 6, "12.5"), (1440.0, 6, "1440"), (0.33333333, 4, "0.3333")] {
            assert_eq!(fmt_g(x, p), want, "{x} at {p}");
        }
        assert_eq!(plus_g(0.5), "+0.5");
        assert_eq!(plus_g(-0.25), "-0.25");
    }

    #[test]
    fn presets_compile_as_feed_1_ranked() {
        assert_eq!(compile_order(&arm_spec(&json!("vector"))),
                   "0.6 * tmm(similarity(emb, :taste)) + 0.3 * decay(ts, now(), 21600) + 0.1 * tmm(ln(1 + n_reactions + 1 * n_comments), lo => 0, hi => 5) DESC");
        assert_eq!(compile_order(&arm_spec(&json!("recommender"))), "rrf(recommend('taste', :me), similarity(emb, :taste)) DESC");
        assert_eq!(describe(&arm_spec(&json!("blend"))), "the recommender 67%, freshness (half-life 6 h) 33% · last 72 h");
    }

    #[test]
    fn arms_are_checked() {
        assert!(clean_arm(&json!({"arm": "x", "mix": {"taste": 2}}), 0).is_err());
        assert!(clean_arm(&json!({"arm": "x", "order": "score() DESC; DROP TABLE posts"}), 0).is_err());
        assert!(clean_arm(&json!({"arm": "x", "order": "n_reactions FROM posts"}), 0).is_err());
        let a = clean_arm(&json!({"arm": "Mine", "mix": {"taste": 0.5, "circle": 1}, "only_kinds": ["text", "link"], "per_author": 2}), 0).unwrap();
        assert_eq!(a["arm"], "mine");
        assert_eq!(a["only_kinds"], json!(["text", "link"]));
    }

    #[test]
    fn t_and_chi2_match_known_values() {
        assert!((t_q(0.975, 10.0) - 2.228138852).abs() < 1e-6);
        assert!((t_cdf(2.0, 5.0) - 0.9490302605).abs() < 1e-8);
        assert!((chi2_sf(3.841458821, 1) - 0.05).abs() < 1e-8);
    }
}
