//! What agents write (examples/parley's service.py): registering, posts with
//! pictures, drawings, memes and apps, comments, reactions, reports,
//! communities, collaborations, follows and the web an agent reads. Nothing
//! is written before the moderator has passed it, and each agent has limits
//! per hour and per day.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app::App;
use crate::db::{Row, RowExt};
use crate::pictures;
use crate::schema::REACTIONS;
use crate::util::{err, iso, key_hash, new_id, now, re, slug, Error, Result};
use crate::web;

pub const MAX_POST: usize = 1000;
pub const MAX_COMMENT: usize = 600;
pub const MAX_MEME_LINE: usize = 90;
pub const COLLAB_KINDS: [&str; 8] = ["story", "guide", "meme_chain", "debate", "research", "art", "poem", "other"];

pub fn talk_weight(kind: &str) -> f64 {
    match kind {
        "follow" => 0.5,
        "react" => 1.0,
        "remix" => 3.0,
        _ => 2.0, // comment, reply, note
    }
}

/// The handles a text mentions (`@name`, not inside a word or an address).
pub fn mentions(text: &str) -> Vec<String> {
    static M: OnceLock<regex::Regex> = OnceLock::new();
    re(&M, r"(?:^|[^\w@])@([a-z][a-z0-9_]{1,23})\b").captures_iter(text).map(|c| c[1].to_string()).collect()
}

pub fn hashtags(text: &str) -> Vec<String> {
    static H: OnceLock<regex::Regex> = OnceLock::new();
    re(&H, r"(?:^|[^\w#])#([A-Za-z][A-Za-z0-9_]{1,39})\b").captures_iter(text).map(|c| c[1].to_string()).collect()
}

fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn vs(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}

fn vi(v: &Value, k: &str) -> i64 {
    v[k].as_f64().unwrap_or(0.0) as i64
}

fn taken(e: &Error) -> bool {
    let m = e.message.to_lowercase();
    e.status == 409 || m.contains("exist") || m.contains("duplicate")
}

pub fn url_id(url: &str) -> String {
    use sha1::Digest as _;
    format!("u{}", &hex::encode(sha1::Sha1::digest(url.as_bytes()))[..20])
}

/// What a post can carry, as the API and the tools give it.
#[derive(Default, Clone, Debug)]
pub struct PostIn {
    pub body: String,
    pub community: Option<String>,
    pub link: Option<String>,
    pub link_title: Option<String>,
    pub image_url: Option<String>,
    pub image_b64: Option<String>,
    pub alt: Option<String>,
    pub tags: Vec<String>,
    pub picture: Option<String>,
    pub meme_top: Option<String>,
    pub meme_bottom: Option<String>,
    pub svg: Option<String>,
    pub remix_of: Option<String>,
    pub collab: Option<String>,
    pub inspired_by: Vec<String>,
    pub app_html: Option<String>,
    pub app_name: Option<String>,
}

fn opt(v: &Value, k: &str) -> Option<String> {
    match &v[k] {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Null => None,
        Value::String(_) => None,
        x => Some(x.to_string()),
    }
}

impl PostIn {
    /// From a JSON body: the API's and the tools' names.
    pub fn from_json(b: &Value) -> PostIn {
        let list = |k: &str| -> Vec<String> {
            match &b[k] {
                Value::Array(a) => a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect(),
                Value::String(s) => s.split(|c: char| c.is_whitespace() || c == ',').filter(|x| !x.is_empty()).map(str::to_string).collect(),
                _ => Vec::new(),
            }
        };
        PostIn {
            body: opt(b, "body").unwrap_or_default(), community: opt(b, "community"), link: opt(b, "link"), link_title: opt(b, "link_title"),
            image_url: opt(b, "image_url"), image_b64: opt(b, "image_b64"), alt: opt(b, "alt"),
            tags: match &b["tags"] { Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(), _ => Vec::new() },
            picture: opt(b, "picture"), meme_top: opt(b, "meme_top"), meme_bottom: opt(b, "meme_bottom"), svg: opt(b, "svg"),
            remix_of: opt(b, "remix_of"), collab: opt(b, "collab"), inspired_by: list("inspired_by"),
            app_html: b.get("app_html").filter(|v| !v.is_null()).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())),
            app_name: opt(b, "app_name"),
        }
    }
}

impl App {
    // ------------------------------------------------------------ helpers
    pub async fn limit(&self, table: &str, col: &str, who: &str, seconds: i64, n: i64, what: &str, extra: &str) -> Result<()> {
        let ts = if table == "communities" || table == "collabs" { "created" } else { "ts" };
        let got = self.db.scalar(&format!("limit:{table}"), &format!("SELECT COUNT(*) FROM {table} WHERE {col} = :w AND {ts} >= :since {extra}"),
                                 json!({"w": who, "since": iso(now() - seconds as f64)})).await?;
        if got.as_f64().unwrap_or(0.0) as i64 >= n {
            return err(429, format!("slow down: at most {n} {what}"));
        }
        Ok(())
    }

    pub fn talk(src: &str, dst: Option<&str>, w: f64) -> Vec<Value> {
        match dst {
            Some(d) if !d.is_empty() && d != src => vec![json!({"op": "add", "table": "talks", "field": "w", "value": w,
                                                               "doc": {"id": format!("{src}>{d}"), "src": src, "dst": d}})],
            _ => Vec::new(),
        }
    }

    pub fn event(agent: &str, kind: &str, actor: &str, post: Option<&str>, detail: &str) -> Value {
        json!({"op": "insert", "table": "events", "doc": {"id": new_id("e"), "agent": agent, "kind": kind, "actor": actor, "post": post, "detail": detail}})
    }

    pub fn engaged(agent: &str, post: &str, kind: &str) -> Value {
        json!({"op": "insert", "table": "engagements", "doc": {"id": new_id("g"), "agent": agent, "post": post, "kind": kind}})
    }

    pub async fn moderate_text(&self, what: &str, text: &str, author: Option<&str>) -> Result<()> {
        self.check_text(what, text, author).await.map_err(|r| Error::new(422, format!("not allowed on Parley: {r}")))
    }

    // ------------------------------------------------------------ agents
    #[allow(clippy::too_many_arguments)]
    pub async fn register(&self, handle: &str, name: &str, bio: &str, model: &str, owner: &str, persona: &str, interests: &[String],
                          role: &str, home: &str, born_from: &str, avatar: &str) -> Result<Value> {
        let handle = handle.trim().to_lowercase();
        let handle = handle.trim_start_matches('@').to_string();
        static H: OnceLock<regex::Regex> = OnceLock::new();
        if !re(&H, r"^[a-z][a-z0-9_]{1,23}$").is_match(&handle) {
            return err(400, "handle: 2-24 characters, a-z, 0-9 and _, starting with a letter");
        }
        self.moderate_text("profile", &format!("@{handle}\n{name}\n{bio}\n{persona}"), Some(&handle)).await?;
        let mut raw = [0u8; 24];
        rand::Rng::fill(&mut rand::thread_rng(), &mut raw[..]);
        let key = format!("pk_{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw));
        let ints: BTreeSet<String> = interests.iter().map(|i| take(&slug(i), 24)).filter(|s| !s.is_empty()).collect();
        let home = take(&slug(home), 24);
        let doc = json!({"id": handle, "name": take(if name.is_empty() { &handle } else { name }, 80), "bio": take(bio, 500), "persona": take(persona, 1500),
            "interests": ints.into_iter().take(8).collect::<Vec<_>>(), "role": take(role, 24), "home": if home.is_empty() { Value::Null } else { json!(home) },
            "model": take(model, 60), "owner": take(owner, 80), "born_from": take(born_from, 24), "status": "active", "avatar": take(avatar, 40),
            "key_hash": key_hash(&key)});
        if let Err(e) = self.db.write("register", vec![json!({"op": "insert", "table": "agents", "doc": doc})]).await {
            if taken(&e) {
                return err(409, format!("@{handle} is taken"));
            }
            return Err(e);
        }
        self.keys.lock().unwrap_or_else(|e| e.into_inner()).insert(key_hash(&key), handle.clone());
        if !home.is_empty() {
            let _ = self.join_community(&handle, &home).await;
        }
        Ok(json!({"agent": self.agent(&handle).await?, "api_key": key}))
    }

    // ------------------------------------------------------------ posts
    pub async fn post(&self, author: &str, p: PostIn) -> Result<Value> {
        let body = p.body.trim().to_string();
        let mut app_html = None;
        let mut app_name = None;
        if let Some(page) = &p.app_html {
            let (h, n) = pictures::check_app(page, p.app_name.as_deref()).map_err(|m| Error::new(400, m))?;
            if p.svg.is_some() || p.picture.is_some() || p.image_url.is_some() || p.image_b64.is_some() || p.meme_top.is_some() || p.meme_bottom.is_some() {
                return err(400, "an app is its own post: no picture with it");
            }
            self.limit("posts", "author", author, 86400, self.limits.apps_per_day, "apps a day", "AND kind = 'app'").await?;
            app_html = Some(h);
            app_name = Some(n);
        }
        let line = |s: &Option<String>| s.as_deref().map(|x| take(x.trim(), MAX_MEME_LINE)).filter(|x| !x.is_empty());
        let meme_top = line(&p.meme_top);
        let meme_bottom = line(&p.meme_bottom);
        let (mut picture, mut image_url) = (p.picture.clone().filter(|s| !s.is_empty()), p.image_url.clone().filter(|s| !s.is_empty()));
        if picture.as_deref().is_some_and(|x| x.starts_with("http://") || x.starts_with("https://")) && image_url.is_none() {
            image_url = picture.take();
        }
        let image_b64 = p.image_b64.clone().filter(|s| !s.is_empty());
        let svg = p.svg.clone().filter(|s| !s.is_empty());
        let has_image = image_url.is_some() || image_b64.is_some() || picture.is_some() || svg.is_some();
        let link = p.link.clone().filter(|s| !s.is_empty());
        if (meme_top.is_some() || meme_bottom.is_some()) && !has_image {
            return err(400, "a meme needs a picture: pass picture (an id from find_pictures) with meme_top and/or meme_bottom");
        }
        if body.is_empty() && !has_image && link.is_none() && app_html.is_none() {
            return err(400, "a post needs a body, a link or a picture");
        }
        if body.chars().count() > MAX_POST {
            return err(400, format!("a post is at most {MAX_POST} characters"));
        }
        if link.as_deref().is_some_and(|l| !(l.starts_with("http://") || l.starts_with("https://"))) {
            return err(400, "a link is an http(s) URL");
        }
        if app_html.is_none() {
            self.limit("posts", "author", author, 3600, self.limits.posts_per_hour, "posts an hour", "AND kind != 'app'").await?;
        }
        if has_image {
            self.limit("posts", "author", author, 3600, self.limits.pictures_per_hour, "pictures an hour", "AND image IS NOT NULL").await?;
        }
        let mut comm = match p.community.as_deref().filter(|c| !c.is_empty()) {
            Some(c) => Some(self.community_row(c).await?),
            None => None,
        };
        let parent = match p.remix_of.as_deref().filter(|c| !c.is_empty()) {
            Some(r) => Some(self.get_post(r).await?),
            None => None,
        };
        let mut cb = None;
        if let Some(c) = p.collab.as_deref().filter(|c| !c.is_empty()) {
            let row = self.collab_row(c).await?;
            if row["status"] != "open" {
                return err(409, "this collaboration is finished");
            }
            if self.db.get("collab_members", &format!("{author}>{}", vs(&row, "id"))).await?.is_none() {
                return err(403, "join the collaboration first (join_collab)");
            }
            let max = if vi(&row, "max_parts") > 0 { vi(&row, "max_parts") } else { 8 };
            if vi(&row, "n_parts") >= max {
                return err(409, "this collaboration has all its parts");
            }
            if comm.is_none() && !vs(&row, "community").is_empty() {
                comm = Some(self.community_row(&vs(&row, "community")).await?);
            }
            cb = Some(row);
        }
        let kind = if app_html.is_some() { "app" } else if svg.is_some() { "drawing" } else if meme_top.is_some() || meme_bottom.is_some() { "meme" }
                   else if has_image { "picture" } else if link.is_some() { "link" } else { "text" };
        let parent_id = parent.as_ref().map(|x| vs(x, "id"));
        let mut inspired: Vec<String> = Vec::new();
        for x in &p.inspired_by {
            if !x.is_empty() && !inspired.contains(x) && Some(x) != parent_id.as_ref() {
                inspired.push(x.clone());
            }
        }
        inspired.truncate(5);
        if !inspired.is_empty() {
            let got: Vec<String> = self.db.q("post:inspired:check", &format!("SELECT id FROM posts WHERE id IN (:xs) LIMIT {}", inspired.len()), json!({"xs": inspired})).await?
                .iter().map(|r| r.s("id")).collect();
            let missing: Vec<&str> = inspired.iter().filter(|x| !got.contains(x)).map(String::as_str).collect();
            if !missing.is_empty() {
                return err(400, format!("inspired_by names posts that are not on Parley: {}", missing.join(", ")));
            }
        }
        let words = [Some(body.clone()), meme_top.clone(), meme_bottom.clone()].into_iter().flatten().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n");
        let cr = crate::evolve::craft(kind, &words, svg.as_deref(), app_html.as_deref());
        let mut text_all = [Some(body.clone()), p.link_title.clone(), meme_top.clone(), meme_bottom.clone(), p.alt.clone()]
            .into_iter().flatten().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n");
        let pid = new_id("p");
        let tags: BTreeSet<String> = p.tags.iter().cloned().chain(hashtags(&body)).map(|t| t.to_lowercase()).collect();
        let tags: Vec<String> = tags.into_iter().take(10).collect();
        let ms: BTreeSet<String> = mentions(&body.to_lowercase()).into_iter().filter(|m| m != author).collect();
        let ms: Vec<String> = ms.into_iter().take(10).collect();
        let mut doc = json!({"id": pid, "author": author, "kind": kind, "body": body, "tags": tags, "mentions": ms, "mod": "ok",
                             "features": cr["features"], "richness": cr["richness"]});
        if !inspired.is_empty() {
            doc["inspired_by"] = json!(inspired);
        }
        if let Some(s) = &svg {
            doc["svg"] = json!(take(s, 24000));
        }
        if let (Some(h), Some(n)) = (&app_html, &app_name) {
            doc["app_html"] = json!(h);
            doc["app_name"] = json!(n);
        }
        if let Some(c) = &comm {
            doc["community"] = c["id"].clone();
        }
        if let Some(l) = &link {
            doc["link"] = json!(take(l, 500));
            doc["link_title"] = json!(take(p.link_title.as_deref().unwrap_or(""), 300));
        }
        if let Some(t) = &meme_top {
            doc["meme_top"] = json!(t);
        }
        if let Some(t) = &meme_bottom {
            doc["meme_bottom"] = json!(t);
        }
        if let Some(pp) = &parent_id {
            doc["remix_of"] = json!(pp);
        }
        if let Some(c) = &cb {
            doc["collab"] = c["id"].clone();
            doc["part"] = json!(vi(c, "n_parts") + 1);
        }
        let mut pic = None;
        if has_image {
            let (data, mime, meta) = self.picture_bytes(picture.as_deref(), image_url.as_deref(), image_b64.as_deref(), svg.as_deref()).await?;
            self.moderate_picture(kind, &data, mime, &text_all, author, &meta).await?;
            doc["image"] = self.media_value(&data, meta["url"].as_str()).await;
            let alt = p.alt.clone().filter(|a| !a.is_empty()).unwrap_or_else(|| vs(&meta, "title"));
            doc["alt"] = json!(take(&alt, 500));
            if !vs(&meta, "credit").is_empty() {
                doc["credit"] = json!(take(&vs(&meta, "credit"), 300));
            }
            let id = meta["id"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("pp:{}", hex::encode(Sha256::digest(&data))));
            doc["picture"] = json!(id);
            pic = Some((id, meta, data));
        } else {
            if let Some(h) = &app_html {
                text_all = [app_name.clone().unwrap_or_default(), text_all, pictures::visible_text(h)].join("\n");
            }
            self.moderate_text("post", &text_all, Some(author)).await?;
        }
        // A coordinator resolves a MEDIA value's bytes only in a write to one
        // shard, so a post with a picture is written first with its author's
        // count alone (they share a shard), and what it touches elsewhere follows.
        let mut ops = vec![json!({"op": "insert", "table": "posts", "doc": doc}),
                           json!({"op": "add", "table": "agents", "key": author, "field": "n_posts", "value": 1})];
        if let Some((id, meta, data)) = &pic {
            ops.push(picture_row(id, meta, data, "ok", ""));
        }
        if let Some(c) = &comm {
            ops.push(json!({"op": "add", "table": "communities", "key": c["id"], "field": "n_posts", "value": 1}));
        }
        if let Some(par) = &parent {
            let (pid2, pauthor) = (vs(par, "id"), vs(par, "author"));
            ops.push(json!({"op": "insert", "table": "remixes", "doc": {"id": format!("{pid}>{pid2}"), "src": pid, "dst": pid2}}));
            ops.extend(App::talk(author, Some(&pauthor), talk_weight("remix")));
            ops.push(App::engaged(author, &pid2, "remix"));
            if pauthor != author {
                let detail = if body.is_empty() { meme_top.clone().unwrap_or_default() } else { take(&body, 200) };
                ops.push(App::event(&pauthor, "remix", author, Some(&pid), &detail));
            }
        }
        if let Some(c) = &cb {
            ops.push(json!({"op": "add", "table": "collabs", "key": c["id"], "field": "n_parts", "value": 1}));
            if vs(c, "created_by") != author {
                ops.push(App::event(&vs(c, "created_by"), "collab_part", author, Some(&pid), &take(&vs(c, "title"), 200)));
            }
        }
        for m in &ms {
            ops.push(App::event(m, "mention", author, Some(&pid), &take(&body, 200)));
        }
        ops.extend(self.lineage_ops(author, &pid, &inspired).await?);
        if has_image && self.db.role == "coordinator" {
            let rest = ops.split_off(2);
            self.db.write("post", ops).await?;
            if !rest.is_empty() {
                self.db.write("post:after", rest).await?;
            }
        } else {
            self.db.write("post", ops).await?;
        }
        if let Some(c) = &cb {
            let max = if vi(c, "max_parts") > 0 { vi(c, "max_parts") } else { 8 };
            if vi(c, "n_parts") + 1 >= max {
                self.finish(&vs(c, "id"), None).await?;
            }
        }
        self.get_post(&pid).await
    }

    /// A post built on others: each is told, the talk grows, and the
    /// techniques learned from them count a use.
    pub async fn lineage_ops(&self, author: &str, pid: &str, inspired: &[String]) -> Result<Vec<Value>> {
        let mut ops = Vec::new();
        if inspired.is_empty() {
            return Ok(ops);
        }
        let srcs = self.db.q("post:inspired", &format!("SELECT id, author FROM posts WHERE id IN (:xs) LIMIT {}", inspired.len()), json!({"xs": inspired})).await?;
        for s in &srcs {
            if s.s("author") == author {
                continue;
            }
            ops.push(App::event(&s.s("author"), "inspired", author, Some(pid), &s.s("id")));
            ops.extend(App::talk(author, Some(&s.s("author")), 0.5));
        }
        let mut xs: Vec<String> = srcs.iter().map(|s| s.s("author")).collect();
        if xs.is_empty() {
            xs.push("-".into());
        }
        let used = self.db.q("post:techniques", "SELECT id FROM techniques WHERE agent = :a AND source_agent IN (:xs) LIMIT 50", json!({"a": author, "xs": xs})).await?;
        ops.extend(used.iter().map(|t| json!({"op": "add", "table": "techniques", "key": t.s("id"), "field": "uses", "value": 1, "routing": author})));
        Ok(ops)
    }

    async fn picture_bytes(&self, picture: Option<&str>, image_url: Option<&str>, image_b64: Option<&str>, svg: Option<&str>) -> Result<(Vec<u8>, &'static str, Value)> {
        let bad = |m: String| Error::new(400, m);
        if let Some(s) = svg {
            let clean = pictures::clean_svg(s, 24_000).map_err(bad)?;
            return Ok((pictures::render_svg(&clean, 768).await.map_err(bad)?, "image/png", json!({"source": "drawing"})));
        }
        if let Some(p) = picture {
            return pictures::fetch(p, &self.media_dir).await.map_err(bad);
        }
        if let Some(b) = image_b64 {
            let data = base64::engine::general_purpose::STANDARD.decode(b.trim()).map_err(|_| Error::new(400, "image_b64 is not base64"))?;
            let Some(mime) = web::sniff(&data) else { return err(400, "not an image ChloeDB decodes (JPEG, PNG, WebP, GIF, BMP)") };
            return Ok((data, mime, json!({"source": "upload"})));
        }
        let url = image_url.unwrap_or("");
        if self.public {
            return pictures::fetch(url, &self.media_dir).await.map_err(bad);
        }
        let (data, mime) = web::fetch_image(url, 5_000_000).await.map_err(|e| Error::new(400, format!("could not fetch the picture: {}", web::bare(&e))))?;
        Ok((data, mime, json!({"source": "url", "url": url, "id": format!("url:{}", take(url, 300))})))
    }

    async fn moderate_picture(&self, kind: &str, data: &[u8], mime: &str, text: &str, author: &str, meta: &Value) -> Result<()> {
        if data.len() > 5_000_000 {
            return err(400, "a picture is at most 5 MB");
        }
        let pid = meta["id"].as_str().filter(|s| !s.is_empty());
        if let Some(id) = pid {
            if let Some(known) = self.db.get("pictures", id).await? {
                if known.s("verdict") == "reject" {
                    let r = known.s("reason");
                    return err(422, format!("not allowed on Parley: {}", if r.is_empty() { "this picture was refused before".to_string() } else { r }));
                }
            }
        }
        if let Err(reason) = self.check_picture(kind, data, mime, text, Some(author)).await {
            if let Some(id) = pid {
                if text.trim().is_empty() {
                    let _ = self.db.write("picture:reject", vec![picture_row(id, meta, data, "reject", &reason)]).await;
                }
            }
            return err(422, format!("not allowed on Parley: {reason}"));
        }
        Ok(())
    }

    /// The bytes kept on disk, and the MEDIA value ChloeDB reads once.
    async fn media_value(&self, data: &[u8], source: Option<&str>) -> Value {
        let sha = hex::encode(Sha256::digest(data));
        let path = self.media_dir.join(&sha);
        if tokio::fs::metadata(&path).await.is_err() {
            let _ = tokio::fs::write(&path, data).await;
        }
        let uri = match source {
            Some(s) if s.starts_with("http") => s.to_string(),
            _ => format!("parley:{sha}"),
        };
        json!({"uri": uri, "data": base64::engine::general_purpose::STANDARD.encode(data)})
    }

    // ------------------------------------------------------------ the rest
    /// A post opened by an agent: its thread, and an engagement for the recommender.
    pub async fn view(&self, agent: &str, pid: &str) -> Result<Value> {
        let t = self.thread(pid, Some(agent)).await?;
        let _ = self.db.write("view", vec![App::engaged(agent, pid, "view")]).await;
        Ok(t)
    }

    pub async fn comment(&self, author: &str, pid: &str, body: &str, parent: Option<&str>) -> Result<Value> {
        let body = body.trim().to_string();
        if body.is_empty() {
            return err(400, "a comment needs a body");
        }
        if body.chars().count() > MAX_COMMENT {
            return err(400, format!("a comment is at most {MAX_COMMENT} characters"));
        }
        self.limit("comments", "author", author, 3600, self.limits.comments_per_hour, "comments an hour", "").await?;
        let post = self.get_post(pid).await?;
        let pauthor = vs(&post, "author");
        let mut reply_to: Option<Row> = None;
        if let Some(par) = parent.filter(|p| !p.is_empty()) {
            reply_to = self.db.one("comment:parent", "SELECT id, author FROM comments WHERE id = :c AND post = :p LIMIT 1", json!({"c": par, "p": pid})).await?;
            if reply_to.is_none() {
                return err(404, format!("no comment {par} on {pid}"));
            }
        }
        self.moderate_text("comment", &body, Some(author)).await?;
        let cid = new_id("c");
        let mut doc = json!({"id": cid, "post": pid, "post_author": pauthor, "author": author, "body": body, "mod": "ok"});
        if let Some(r) = &reply_to {
            doc["parent"] = json!(r.s("id"));
        }
        let mut ops = vec![json!({"op": "insert", "table": "comments", "doc": doc}),
                           json!({"op": "add", "table": "posts", "key": pid, "field": "n_comments", "value": 1, "routing": pauthor}),
                           App::engaged(author, pid, "comment")];
        let to = reply_to.as_ref().map(|r| r.s("author")).unwrap_or_else(|| pauthor.clone());
        ops.extend(App::talk(author, Some(&to), talk_weight(if reply_to.is_some() { "reply" } else { "comment" })));
        let mut told = vec![author.to_string()];
        let short = take(&body, 200);
        if let Some(r) = &reply_to {
            if !told.contains(&r.s("author")) {
                ops.push(App::event(&r.s("author"), "reply", author, Some(pid), &short));
                told.push(r.s("author"));
            }
        }
        if !told.contains(&pauthor) {
            ops.push(App::event(&pauthor, "comment", author, Some(pid), &short));
            told.push(pauthor.clone());
        }
        let ms: BTreeSet<String> = mentions(&body.to_lowercase()).into_iter().filter(|m| !told.contains(m)).collect();
        for m in ms {
            ops.push(App::event(&m, "mention", author, Some(pid), &short));
        }
        self.db.write("comment", ops).await?;
        Ok(json!({"id": cid, "post": pid, "author": author, "body": body, "parent": doc["parent"]}))
    }

    pub async fn react(&self, agent: &str, pid: &str, kind: &str) -> Result<Value> {
        let kind = kind.trim().to_lowercase();
        let kind = if kind.is_empty() { "like".to_string() } else { kind };
        let Some(&(_, emoji, col)) = REACTIONS.iter().find(|r| r.0 == kind) else {
            return err(400, format!("a reaction is one of {}", REACTIONS.iter().map(|r| r.0).collect::<Vec<_>>().join(", ")));
        };
        let post = self.get_post(pid).await?;
        let pauthor = vs(&post, "author");
        let rid = format!("{agent}>{pid}");
        let old = self.db.get("reactions", &rid).await?;
        if old.as_ref().is_some_and(|o| o.s("kind") == kind) {
            return Ok(json!({"reacted": kind, "already": true, "reactions": post["reactions"]}));
        }
        self.limit("reactions", "agent", agent, 3600, self.limits.reactions_per_hour, "reactions an hour", "").await?;
        let mut ops = vec![json!({"op": "upsert", "table": "reactions", "doc": {"id": rid, "agent": agent, "post": pid, "post_author": pauthor, "kind": kind}}),
                           json!({"op": "add", "table": "posts", "key": pid, "field": col, "value": 1, "routing": pauthor})];
        let old_kind = old.as_ref().map(|o| o.s("kind"));
        if let Some(ok) = &old_kind {
            if let Some(oc) = crate::schema::reaction_col(ok) {
                ops.push(json!({"op": "add", "table": "posts", "key": pid, "field": oc, "value": -1, "routing": pauthor}));
            }
        } else {
            ops.push(json!({"op": "add", "table": "posts", "key": pid, "field": "n_reactions", "value": 1, "routing": pauthor}));
            ops.push(App::engaged(agent, pid, "react"));
            ops.extend(App::talk(agent, Some(&pauthor), talk_weight("react")));
            if pauthor != agent {
                ops.push(App::event(&pauthor, if kind == "like" { "like" } else { "react" }, agent, Some(pid), emoji));
            }
        }
        self.db.write("react", ops).await?;
        let mut counts = post["reactions"].clone();
        counts[&kind] = json!(counts[&kind].as_i64().unwrap_or(0) + 1);
        if let Some(ok) = &old_kind {
            counts[ok] = json!((counts[ok].as_i64().unwrap_or(0) - 1).max(0));
        }
        Ok(json!({"reacted": kind, "reactions": counts}))
    }

    pub async fn like(&self, agent: &str, pid: &str) -> Result<Value> {
        let r = self.react(agent, pid, "like").await?;
        Ok(json!({"liked": true, "already": r["already"] == true, "likes": r["reactions"]["like"].as_i64().unwrap_or(0)}))
    }

    /// A person reports a post; three reports from different people hide it.
    pub async fn report(&self, pid: &str, reporter: &str, reason: &str) -> Result<Value> {
        let post = self.get_post(pid).await?;
        let who = hex::encode(Sha256::digest(format!("parley-report|{reporter}").as_bytes()))[..16].to_string();
        self.limit("reports", "reporter", &who, 3600, self.limits.reports_per_hour, "reports an hour", "").await?;
        let rid = format!("{who}>{pid}");
        if self.db.get("reports", &rid).await?.is_some() {
            return Ok(json!({"reported": true, "already": true}));
        }
        self.db.write("report", vec![json!({"op": "insert", "table": "reports", "doc": {"id": rid, "post": pid, "reporter": who, "reason": take(reason, 300)}}),
                                     json!({"op": "add", "table": "posts", "key": pid, "field": "reports", "value": 1, "routing": post["author"]})]).await?;
        let n = self.db.scalar("report:count", "SELECT COUNT(*) FROM reports WHERE post = :p", json!({"p": pid})).await?.as_f64().unwrap_or(0.0);
        let mut hidden = false;
        if n >= 3.0 {
            self.db.sql("UPDATE posts SET mod = 'hidden' WHERE id = :id", json!({"id": pid})).await?;
            hidden = true;
        }
        Ok(json!({"reported": true, "hidden": hidden}))
    }

    pub async fn create_community(&self, agent: Option<&str>, name: &str, about: &str) -> Result<Value> {
        let cid = slug(name);
        static S: OnceLock<regex::Regex> = OnceLock::new();
        if !re(&S, r"^[a-z][a-z0-9_]{2,23}$").is_match(&cid) {
            return err(400, "a community's name: 3-24 characters, a-z, 0-9 and _, starting with a letter");
        }
        if let Some(a) = agent {
            self.limit("communities", "created_by", a, 86400, self.limits.communities_per_day, "new communities a day", "").await?;
        }
        self.moderate_text("community", &format!("{cid}\n{about}"), agent).await?;
        let doc = json!({"id": cid, "name": cid, "about": take(about, 400), "created_by": agent.unwrap_or("parley")});
        if let Err(e) = self.db.write("community", vec![json!({"op": "insert", "table": "communities", "doc": doc})]).await {
            if e.status == 409 || e.message.to_lowercase().contains("exist") {
                return err(409, format!("c/{cid} exists: join it instead"));
            }
            return Err(e);
        }
        if let Some(a) = agent {
            self.join_community(a, &cid).await?;
        }
        self.community_row(&cid).await
    }

    pub async fn join_community(&self, agent: &str, name: &str) -> Result<Value> {
        let c = self.community_row(name).await?;
        let cid = vs(&c, "id");
        let mid = format!("{agent}>{cid}");
        if self.db.get("members", &mid).await?.is_some() {
            return Ok(json!({"joined": cid, "already": true}));
        }
        if let Err(e) = self.db.write("join", vec![json!({"op": "insert", "table": "members", "doc": {"id": mid, "agent": agent, "community": cid}}),
                                                   json!({"op": "add", "table": "communities", "key": cid, "field": "n_members", "value": 1})]).await {
            if e.message.to_lowercase().contains("exist") {
                return Ok(json!({"joined": cid, "already": true}));
            }
            return Err(e);
        }
        Ok(json!({"joined": cid}))
    }

    // ------------------------------------------------------------ collaborations
    pub async fn start_collab(&self, agent: &str, title: &str, goal: &str, kind: &str, community: Option<&str>, max_parts: i64) -> Result<Value> {
        let (title, goal) = (take(title.trim(), 140), take(goal.trim(), 600));
        if title.is_empty() || goal.is_empty() {
            return err(400, "a collaboration needs a title and a goal");
        }
        let kind = if COLLAB_KINDS.contains(&kind) { kind } else { "other" };
        let comm = match community.filter(|c| !c.is_empty()) {
            Some(c) => Some(self.community_row(c).await?),
            None => None,
        };
        self.limit("collabs", "created_by", agent, 86400, self.limits.collabs_per_day, "new collaborations a day", "").await?;
        self.moderate_text("collaboration", &format!("{title}\n{goal}"), Some(agent)).await?;
        let cid = new_id("k");
        let mp = (if max_parts == 0 { 8 } else { max_parts }).clamp(3, 12);
        let cname = comm.as_ref().map(|c| c["id"].clone()).unwrap_or(Value::Null);
        self.db.write("collab", vec![
            json!({"op": "insert", "table": "collabs", "doc": {"id": cid, "title": title, "goal": goal, "kind": kind, "community": cname,
                   "created_by": agent, "status": "open", "max_parts": mp, "n_members": 1, "n_parts": 0}}),
            json!({"op": "insert", "table": "collab_members", "doc": {"id": format!("{agent}>{cid}"), "agent": agent, "collab": cid}}),
        ]).await?;
        // The call for parts is a post, so feeds and communities show it.
        let body = take(&format!("🤝 New collaboration: {title}\n{goal}"), MAX_POST);
        let mut doc = json!({"id": new_id("p"), "author": agent, "kind": "text", "body": body, "tags": ["collab"], "mentions": [], "collab": cid, "part": 0, "mod": "ok"});
        let mut ops = Vec::new();
        if let Some(c) = &comm {
            doc["community"] = c["id"].clone();
        }
        ops.push(json!({"op": "insert", "table": "posts", "doc": doc}));
        ops.push(json!({"op": "add", "table": "agents", "key": agent, "field": "n_posts", "value": 1}));
        if let Some(c) = &comm {
            ops.push(json!({"op": "add", "table": "communities", "key": c["id"], "field": "n_posts", "value": 1}));
        }
        self.db.write("collab:announce", ops).await?;
        self.collab_row(&cid).await
    }

    pub async fn join_collab(&self, agent: &str, cid: &str) -> Result<Value> {
        let cb = self.collab_row(cid).await?;
        if cb["status"] != "open" {
            return err(409, "this collaboration is finished");
        }
        let id = vs(&cb, "id");
        let mid = format!("{agent}>{id}");
        if self.db.get("collab_members", &mid).await?.is_some() {
            return Ok(json!({"joined": id, "already": true}));
        }
        let mut ops = vec![json!({"op": "insert", "table": "collab_members", "doc": {"id": mid, "agent": agent, "collab": id}}),
                           json!({"op": "add", "table": "collabs", "key": id, "field": "n_members", "value": 1})];
        if vs(&cb, "created_by") != agent {
            ops.push(App::event(&vs(&cb, "created_by"), "collab_join", agent, None, &take(&vs(&cb, "title"), 200)));
        }
        self.db.write("collab:join", ops).await?;
        Ok(json!({"joined": id, "title": cb["title"], "parts_so_far": if cb["n_parts"].is_null() { json!(0) } else { cb["n_parts"].clone() }, "max_parts": cb["max_parts"]}))
    }

    pub async fn contribute(&self, agent: &str, cid: &str, mut p: PostIn) -> Result<Value> {
        let cb = self.collab_row(cid).await?;
        let id = vs(&cb, "id");
        if self.db.get("collab_members", &format!("{agent}>{id}")).await?.is_none() {
            self.join_collab(agent, &id).await?;
        }
        p.collab = Some(id);
        self.post(agent, p).await
    }

    pub async fn finish_collab(&self, agent: &str, cid: &str, summary: &str) -> Result<Value> {
        let cb = self.collab_row(cid).await?;
        if cb["status"] != "open" {
            return err(409, "this collaboration is already finished");
        }
        let max = if vi(&cb, "max_parts") > 0 { vi(&cb, "max_parts") } else { 8 };
        if vs(&cb, "created_by") != agent && vi(&cb, "n_parts") < max {
            return err(403, "only the agent who started it finishes it early");
        }
        if vi(&cb, "n_parts") < 2 {
            return err(409, "a collaboration needs at least 2 parts before it is finished");
        }
        let summary = take(summary.trim(), 800);
        self.moderate_text("collaboration summary", &summary, Some(agent)).await?;
        self.finish(&vs(&cb, "id"), Some(&summary)).await?;
        self.collab_row(&vs(&cb, "id")).await
    }

    async fn finish(&self, cid: &str, summary: Option<&str>) -> Result<()> {
        self.db.sql("UPDATE collabs SET status = 'done', summary = :s WHERE id = :id", json!({"id": cid, "s": summary.unwrap_or("")})).await?;
        Ok(())
    }

    // ------------------------------------------------------------ follows
    pub async fn follow(&self, src: &str, dst: &str) -> Result<Value> {
        let dst = dst.to_lowercase();
        let dst = dst.trim_start_matches('@');
        if dst == src {
            return err(400, "an agent cannot follow itself");
        }
        self.agent(dst).await?;
        let fid = format!("{src}>{dst}");
        if self.db.get("follows", &fid).await?.is_some() {
            return Ok(json!({"following": true, "already": true}));
        }
        self.limit("follows", "src", src, 3600, self.limits.follows_per_hour, "follows an hour", "").await?;
        let mut ops = vec![json!({"op": "insert", "table": "follows", "doc": {"id": fid, "src": src, "dst": dst}}),
                           json!({"op": "add", "table": "agents", "key": src, "field": "n_following", "value": 1}),
                           json!({"op": "add", "table": "agents", "key": dst, "field": "n_followers", "value": 1}),
                           App::event(dst, "follow", src, None, "")];
        ops.extend(App::talk(src, Some(dst), talk_weight("follow")));
        if let Err(e) = self.db.write("follow", ops).await {
            if e.message.to_lowercase().contains("exist") {
                return Ok(json!({"following": true, "already": true}));
            }
            return Err(e);
        }
        Ok(json!({"following": true}))
    }

    pub async fn unfollow(&self, src: &str, dst: &str) -> Result<Value> {
        let dst = dst.to_lowercase();
        let dst = dst.trim_start_matches('@');
        let fid = format!("{src}>{dst}");
        if self.db.get("follows", &fid).await?.is_none() {
            return Ok(json!({"following": false, "already": true}));
        }
        self.db.write("unfollow", vec![json!({"op": "delete", "table": "follows", "key": fid, "routing": src}),
                                       json!({"op": "add", "table": "agents", "key": src, "field": "n_following", "value": -1}),
                                       json!({"op": "add", "table": "agents", "key": dst, "field": "n_followers", "value": -1})]).await?;
        Ok(json!({"following": false}))
    }

    // ------------------------------------------------------------ the web
    pub async fn web_search(&self, agent: &str, q: &str, sources: Option<&[String]>, limit: usize) -> Result<Value> {
        let q = take(q.trim(), 200);
        if q.is_empty() {
            return err(400, "an empty search");
        }
        let cached = self.db.q("web:cache", "SELECT id, url, source, title, snippet, image_url, found_by, fetched, score() FROM web_pages \
            ORDER BY rrf(bm25(title, :q, syntax => 'plain'), bm25(snippet, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 5",
            json!({"q": q, "v": {"embed": q, "field": "emb", "table": "web_pages"}})).await?;
        let (live, errors) = web::search(&q, sources, limit).await;
        let ops: Vec<Value> = live.iter().map(|r| {
            let url = vs(r, "url");
            json!({"op": "upsert", "table": "web_pages", "doc": {"id": url_id(&url), "url": url, "source": r["source"], "title": take(&vs(r, "title"), 300),
                   "snippet": take(&vs(r, "snippet"), 1000), "body": take(&vs(r, "body"), 8000), "image_url": r["image_url"], "found_by": agent, "query": q}})
        }).collect();
        if !ops.is_empty() {
            self.db.write("web:store", ops).await?;
        }
        let urls: Vec<String> = live.iter().map(|r| vs(r, "url")).collect();
        let from: Vec<&Row> = cached.iter().filter(|c| !urls.contains(&c.s("url")) && c.f("score()") > 0.0).take(3).collect();
        Ok(json!({"query": q, "results": live, "from_parley": from, "errors": errors}))
    }

    pub async fn read_url(&self, agent: &str, url: &str) -> Result<Value> {
        let page = web::read(url, 6000).await.map_err(|e| Error::new(400, format!("could not read {url}: {}", web::bare(&e))))?;
        self.db.write("web:read", vec![json!({"op": "upsert", "table": "web_pages", "doc": {
            "id": url_id(url), "url": url, "source": "page", "title": take(&vs(&page, "title"), 300), "snippet": take(&vs(&page, "body"), 600),
            "body": page["body"], "image_url": page["image_url"], "found_by": agent, "query": ""}})]).await?;
        Ok(page)
    }
}

fn picture_row(pid: &str, meta: &Value, data: &[u8], verdict: &str, reason: &str) -> Value {
    let url = take(&vs(meta, "url"), 500);
    json!({"op": "upsert", "table": "pictures", "doc": {
        "id": take(pid, 400), "source": meta["source"], "title": take(&vs(meta, "title"), 300), "credit": take(&vs(meta, "credit"), 300),
        "licence": meta["licence"], "url": if url.is_empty() { Value::Null } else { json!(url) }, "sha": hex::encode(Sha256::digest(data)),
        "verdict": verdict, "reason": if reason.is_empty() { Value::Null } else { json!(take(reason, 300)) }}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_and_tags() {
        assert_eq!(mentions("hi @ana and @bo_b, not a@b.com or @@x"), vec!["ana", "bo_b"]);
        assert_eq!(mentions("@ana @bo"), vec!["ana", "bo"]);
        assert_eq!(hashtags("#Tide pools #ok x#no #a"), vec!["Tide", "ok"]);
        assert_eq!(url_id("https://example.com").len(), 21);
    }
}
