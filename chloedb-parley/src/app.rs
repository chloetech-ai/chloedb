//! Parley: the state every request shares, and what a post looks like out.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use crate::db::{Db, Row, RowExt};
use crate::schema::{reaction_cols, REACTIONS};

pub fn post_cols() -> String {
    format!("id, author, kind, community, body, tags, link, link_title, image, alt, credit, picture, meme_top, meme_bottom, \
             remix_of, inspired_by, features, richness, app_name, collab, part, {}, n_reactions, n_comments, ts", reaction_cols())
}

pub const AGENT_COLS: &str = "id, name, bio, persona, interests, role, home, model, owner, born_from, status, created, last_turn, n_turns, n_posts, n_followers, n_following";

/// What one agent may do, per rolling hour or day.
#[derive(Clone)]
pub struct Limits {
    pub posts_per_hour: i64,
    pub pictures_per_hour: i64,
    pub apps_per_day: i64,
    pub comments_per_hour: i64,
    pub reactions_per_hour: i64,
    pub follows_per_hour: i64,
    pub communities_per_day: i64,
    pub collabs_per_day: i64,
    pub reports_per_hour: i64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { posts_per_hour: 6, pictures_per_hour: 5, apps_per_day: 3, comments_per_hour: 12, reactions_per_hour: 40, follows_per_hour: 20,
                 communities_per_day: 1, collabs_per_day: 2, reports_per_hour: 20 }
    }
}

pub struct App {
    pub db: Db,
    pub media_dir: PathBuf,
    pub shards: Vec<String>,
    pub public: bool,
    pub limits: Limits,
    /// API keys' hashes to their agents, as they are seen.
    pub keys: Mutex<HashMap<String, String>>,
    /// The model's name and daily cap, when one is connected.
    pub model: Option<String>,
    pub daily_usd: Option<f64>,
    pub ctx_url: Option<String>,
    pub ctx_token: Option<String>,
    /// Answers kept a short while (the trending topics), by key: when and what.
    pub cache: Mutex<HashMap<String, (f64, Value)>>,
    /// ChloeCtx spaces known to exist.
    pub ctx_made: Mutex<std::collections::HashSet<String>>,
}

impl App {
    pub fn new(db: Db) -> App {
        App { db, media_dir: PathBuf::from("parley-media"), shards: Vec::new(), public: false, limits: Limits::default(),
              keys: Mutex::new(HashMap::new()), model: None, daily_usd: None, ctx_url: None, ctx_token: None, cache: Mutex::new(HashMap::new()),
              ctx_made: Mutex::new(std::collections::HashSet::new()) }
    }
}

/// A post as the API answers it: reactions as one object, the image as a
/// URL of this site, no internal columns.
pub fn post_out(r: &Row) -> Value {
    let mut out = Map::new();
    for (k, v) in r {
        if k == "score()" || k == "mod" || k == "hops()" || k.starts_with("r_") || k == "likes" {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    let reactions: Map<String, Value> = REACTIONS.iter().map(|(kind, _, col)| (kind.to_string(), json!(r.i(col)))).collect();
    out.insert("likes".into(), reactions["like"].clone());
    out.insert("reactions".into(), Value::Object(reactions));
    if let Some(s) = r.get("score()").and_then(Value::as_f64) {
        out.insert("score".into(), json!(crate::util::round(s, 4)));
    }
    if r.has("image") {
        let m = match &r["image"] {
            Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
            v => v.clone(),
        };
        if m.is_object() {
            let sha = m["sha256"].clone();
            let uri = m["uri"].as_str().unwrap_or("");
            out.insert("image".into(), json!({"sha256": sha, "url": format!("/media/{}", sha.as_str().unwrap_or("")), "width": m["width"],
                "height": m["height"], "mime": m["mime"], "source": if uri.starts_with("http") { json!(uri) } else { Value::Null }}));
        } else {
            out.insert("image".into(), Value::Null);
        }
    }
    for k in ["tags", "mentions"] {
        if let Some(Value::String(s)) = out.get(k) {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                out.insert(k.into(), v);
            }
        }
    }
    out.retain(|_, v| !v.is_null());
    Value::Object(out)
}

pub fn posts_out(rows: &[Row]) -> Vec<Value> {
    rows.iter().map(post_out).collect()
}

/// An agent as anyone may see it: no key, no vector.
pub fn public_agent(r: Row) -> Value {
    Value::Object(r.into_iter().filter(|(k, _)| k != "key_hash" && k != "emb").collect())
}

pub fn without_emb(r: Row) -> Value {
    Value::Object(r.into_iter().filter(|(k, _)| k != "emb").collect())
}
