//! Feedback on the command center: a screenshot of a view, marked up, with
//! a note, kept as a row of the table [`TABLE`] so that people triage it in
//! the command center's Feedback view and agents read it with SQL, these
//! routes or the MCP tools (`list_feedback`, `get_feedback`,
//! `update_feedback`).
//!
//! The browser makes the screenshot and draws the marks itself (no service
//! outside the server sees it) and sends the picture flattened, so that a
//! part blacked out is gone from the stored image. The server keeps it as a
//! `data:` URL in a column that is stored and not indexed, with a small
//! thumbnail for the list.
//!
//! | route | body | answer |
//! |---|---|---|
//! | `POST /v1/feedback` | `{note, kind?, title?, page?, route?, url?, shot?, thumb?, shot_w?, shot_h?, marks?, context?}` | `{id, ts}` (write scope) |
//! | `GET /v1/feedback?status=new,planned&kind=bug&q=words&limit=100` | | `{feedback: [...], counts, statuses, kinds, table}`: the newest first, without screenshots |
//! | `GET /v1/feedback/:id?shot=1` | | one row in full; with `shot`, the screenshot as a `data:` URL |
//! | `GET /v1/feedback/:id/shot`, `.../thumb` | | the screenshot or its thumbnail, as an image |
//! | `PATCH /v1/feedback/:id` | `{status?, priority?, assignee?, resolution?, kind?, title?, if_status?}` | the row (write scope); `if_status` refuses (409) when the row's status is another, so that two agents do not take one item (triage through one server or coordinator is one at a time) |
//! | `DELETE /v1/feedback/:id` | | `{deleted}` (admin scope) |
//!
//! A key limited to tables calls these when it is granted the table:
//! reading at `read`, submitting and triaging at `write`.

use std::sync::Arc;

/// Held while an item is triaged: see [`update`].
static TRIAGE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

use base64::Engine as _;
use chloedb::query::Params;
use chloedb::value::Value;
use serde_json::{json, Map, Value as Json};

use crate::keys::{Principal, Scope};
use crate::service::{After, QueryReq, Service};
use crate::ApiError;

/// The table the feedback is kept in.
pub const TABLE: &str = "cc_feedback";

/// What an item is about.
pub const KINDS: &[&str] = &["bug", "change", "idea", "question"];
/// Where an item is: `new` until someone looks at it, then `triaged`,
/// `planned`, `doing` (someone, perhaps an agent, works on it), and at the
/// end `done` or `wontfix`.
pub const STATUSES: &[&str] = &["new", "triaged", "planned", "doing", "done", "wontfix"];
/// The statuses of items still to do.
pub const OPEN: &[&str] = &["new", "triaged", "planned", "doing"];

/// Bytes of a screenshot, decoded.
pub const MAX_SHOT: usize = 6 << 20;
/// Bytes of a thumbnail, decoded.
pub const MAX_THUMB: usize = 256 << 10;
/// Characters of a note.
pub const MAX_NOTE: usize = 20_000;
/// Marks on a screenshot.
pub const MAX_MARKS: usize = 200;
/// Bytes of the marks and of the context, as JSON.
const MAX_JSON: usize = 64 << 10;

/// The columns a list answers: all but the pictures and the marks' JSON.
const LIST_COLUMNS: &str = "id, ts, author, kind, status, priority, title, note, page, route, marks_text, shot_bytes, shot_w, shot_h, assignee, resolution, updated, updated_by";
/// The columns of one item, without its pictures.
const ONE_COLUMNS: &str = "id, ts, author, author_key, kind, status, priority, title, note, page, route, url, cluster_name, server_version, marks_text, marks, context, shot_type, shot_bytes, shot_w, shot_h, assignee, resolution, updated, updated_by";

/// The table, made the first time feedback is sent.
pub fn create_sql() -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {TABLE} (\
         id KEYWORD PRIMARY KEY, ts TIMESTAMP NOT NULL, author KEYWORD, author_key KEYWORD, \
         kind KEYWORD NOT NULL, status KEYWORD NOT NULL, priority INT, \
         title TEXT, note TEXT, page KEYWORD, route KEYWORD, url KEYWORD OPTIONS(indexed = false), \
         cluster_name KEYWORD, server_version KEYWORD, \
         marks_text TEXT, marks KEYWORD OPTIONS(indexed = false), context JSON, \
         shot KEYWORD OPTIONS(indexed = false), thumb KEYWORD OPTIONS(indexed = false), \
         shot_type KEYWORD, shot_bytes INT, shot_w INT, shot_h INT, \
         assignee KEYWORD, resolution TEXT, updated TIMESTAMP, updated_by KEYWORD)"
    )
}

/// Who makes the table: the server itself, since a key that may write
/// feedback need not be one that may create tables.
fn maker() -> Principal {
    Principal { name: "feedback".into(), key: None, scope: Scope::Admin, root: false, tables: None, role: None, agent: false, on_behalf_of: None }
}

/// Refuses what a key limited to tables may not do to the feedback table.
pub fn may(who: &Principal, need: Scope, what: &str) -> Result<(), ApiError> {
    match who.limited() {
        true => who.may_table(need, TABLE, what),
        false => who.may(need, what),
    }
}

async fn exists(svc: &Arc<Service>) -> bool {
    svc.table(TABLE).await.is_ok()
}

async fn ensure(svc: &Arc<Service>) -> Result<(), ApiError> {
    if !exists(svc).await {
        svc.sql(&maker(), &create_sql(), &Params::new(), After::Any, 0).await?;
    }
    Ok(())
}

/// Rows of a SELECT, as objects.
async fn select(svc: &Arc<Service>, sql: String, params: Params) -> Result<Vec<Map<String, Json>>, ApiError> {
    let a = svc.query(QueryReq { sql, params, ..Default::default() }).await?;
    Ok(a.rs.rows.iter().map(|r| a.rs.columns.iter().zip(r).map(|(c, v)| (c.clone(), crate::json::to_json(v))).collect()).collect())
}

fn text(v: Option<&Json>, what: &str, max: usize) -> Result<Option<String>, ApiError> {
    match v {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => {
            let s = s.trim();
            if s.chars().count() > max {
                return Err(ApiError::bad(format!("{what} is at most {max} characters")));
            }
            Ok((!s.is_empty()).then(|| s.to_string()))
        }
        Some(_) => Err(ApiError::bad(format!("{what} is a string"))),
    }
}

/// The command center route an item was sent from: `#/` and a path, nothing
/// that a link could run (the triage view makes it a link).
pub fn route(v: Option<&Json>) -> Result<Option<String>, ApiError> {
    let r = text(v, "route", 2000)?;
    match &r {
        Some(r) if !r.starts_with("#/") || r.chars().any(|c| c.is_control() || c.is_whitespace()) => {
            Err(ApiError::bad("route is the command center's route: #/ and a path, as in #/cluster"))
        }
        _ => Ok(r),
    }
}

/// The address of the page, kept as text: an http(s) URL or nothing.
fn page_url(v: Option<&Json>) -> Result<Option<String>, ApiError> {
    let u = text(v, "url", 4000)?;
    match &u {
        Some(u) if !(u.starts_with("https://") || u.starts_with("http://")) || u.chars().any(|c| c.is_control()) => {
            Err(ApiError::bad("url is the page's http or https address"))
        }
        _ => Ok(u),
    }
}

fn one_of(v: Option<&Json>, what: &str, allowed: &[&str]) -> Result<Option<String>, ApiError> {
    match text(v, what, 40)? {
        None => Ok(None),
        Some(s) if allowed.contains(&s.as_str()) => Ok(Some(s)),
        Some(s) => Err(ApiError::bad(format!("{what} is one of {}, not {s:?}", allowed.join(", ")))),
    }
}

/// A picture as the browser sends it: `data:image/<png|jpeg|webp>;base64,...`.
/// Its type and its decoded bytes.
pub fn picture(data_url: &str, max: usize, what: &str) -> Result<(String, Vec<u8>), ApiError> {
    let rest = data_url.strip_prefix("data:").ok_or_else(|| ApiError::bad(format!("{what} is a data: URL of a PNG, JPEG or WebP image")))?;
    let (mime, b64) = rest.split_once(";base64,").ok_or_else(|| ApiError::bad(format!("{what} is a base64 data: URL")))?;
    if !["image/png", "image/jpeg", "image/webp"].contains(&mime) {
        return Err(ApiError::bad(format!("{what} is a PNG, JPEG or WebP image, not {mime:?}")));
    }
    if b64.len() / 4 * 3 > max + 3 {
        return Err(ApiError { code: None, status: 413, message: format!("{what} is at most {} MB", max >> 20) });
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| ApiError::bad(format!("{what}: not base64 ({e})")))?;
    let magic: &[u8] = match mime {
        "image/png" => b"\x89PNG",
        "image/jpeg" => b"\xff\xd8\xff",
        _ => b"RIFF",
    };
    if !bytes.starts_with(magic) {
        return Err(ApiError::bad(format!("{what} does not hold the {mime} its URL says")));
    }
    Ok((mime.to_string(), bytes))
}

/// A mark as kept: its number, tool, place on the screenshot (in the
/// page's CSS pixels), what it says and the element under it.
fn mark(m: &Json, i: usize) -> Result<Json, ApiError> {
    let o = m.as_object().ok_or_else(|| ApiError::bad("each mark is an object"))?;
    let tool = one_of(o.get("tool"), "a mark's tool", &["box", "arrow", "pen", "text", "redact"])?.ok_or_else(|| ApiError::bad("each mark names its tool"))?;
    let mut out = Map::new();
    out.insert("n".into(), json!(o.get("n").and_then(|n| n.as_u64()).unwrap_or(i as u64 + 1)));
    out.insert("tool".into(), json!(tool));
    for k in ["x", "y", "w", "h", "x2", "y2"] {
        if let Some(v) = o.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite()) {
            out.insert(k.into(), json!((v * 10.0).round() / 10.0));
        }
    }
    if let Some(Json::Array(p)) = o.get("points") {
        let pts: Vec<Json> = p.iter().take(2000).filter_map(|q| q.as_array().filter(|q| q.len() == 2 && q.iter().all(|x| x.is_number())).map(|q| json!(q))).collect();
        out.insert("points".into(), json!(pts));
    }
    if let Some(c) = text(o.get("color"), "a mark's color", 32)? {
        out.insert("color".into(), json!(c));
    }
    if let Some(t) = text(o.get("text"), "a mark's text", 2000)? {
        out.insert("text".into(), json!(t));
    }
    // What the browser saw under it (none under a part blacked out).
    if tool != "redact" {
        if let Some(Json::Object(t)) = o.get("target") {
            let mut tg = Map::new();
            for k in ["selector", "text", "card", "heading", "role", "label"] {
                if let Some(v) = text(t.get(k), "a mark's target", 500)? {
                    tg.insert(k.into(), json!(v));
                }
            }
            if !tg.is_empty() {
                out.insert("target".into(), Json::Object(tg));
            }
        }
    }
    Ok(Json::Object(out))
}

/// The marks as text, one a line, for people and agents to read and for
/// `MATCH` to find: `1. box "too small" on card "Shards" (button.btn: Add shard)`.
pub fn marks_text(marks: &[Json]) -> String {
    let mut lines = Vec::new();
    for m in marks {
        let tool = m["tool"].as_str().unwrap_or("");
        if tool == "redact" {
            continue;
        }
        let mut l = format!("{}. {tool}", m["n"]);
        if let Some(t) = m["text"].as_str() {
            l.push_str(&format!(" {t:?}"));
        }
        let t = &m["target"];
        if let Some(c) = t["card"].as_str().or(t["heading"].as_str()) {
            l.push_str(&format!(" on {c:?}"));
        }
        match (t["selector"].as_str(), t["text"].as_str().or(t["label"].as_str())) {
            (Some(s), Some(x)) => l.push_str(&format!(" ({s}: {x})")),
            (Some(s), None) => l.push_str(&format!(" ({s})")),
            (None, Some(x)) => l.push_str(&format!(" ({x})")),
            _ => {}
        }
        lines.push(l);
    }
    lines.join("\n")
}

fn new_id(now_us: i64) -> String {
    use rand::Rng;
    let day = chloedb::timestamp::format(now_us);
    let day: String = day.chars().take(10).filter(|c| c.is_ascii_digit()).collect();
    let tail: u32 = rand::thread_rng().gen_range(0..0x100_0000);
    format!("fb-{day}-{tail:06x}")
}

fn id_ok(id: &str) -> Result<(), ApiError> {
    match !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        true => Ok(()),
        false => Err(ApiError::bad(format!("no feedback {id:?}: an id is letters, digits, - and _"))),
    }
}

/// `POST /v1/feedback`.
pub async fn submit(svc: &Arc<Service>, who: &Principal, b: Json) -> Result<Json, ApiError> {
    may(who, Scope::Write, "sending feedback")?;
    let o = b.as_object().ok_or_else(|| ApiError::bad("the body is an object: {note, kind?, shot?, marks?, ...}"))?;
    let marks: Vec<Json> = match o.get("marks") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(a)) if a.len() <= MAX_MARKS => a.iter().enumerate().map(|(i, m)| mark(m, i)).collect::<Result<_, _>>()?,
        Some(Json::Array(_)) => return Err(ApiError::bad(format!("at most {MAX_MARKS} marks"))),
        Some(_) => return Err(ApiError::bad("marks is an array")),
    };
    let marks_json = Json::Array(marks.clone()).to_string();
    if marks_json.len() > MAX_JSON {
        return Err(ApiError::bad("the marks are too long: draw fewer, or shorter lines"));
    }
    let mt = marks_text(&marks);
    let note = text(o.get("note"), "note", MAX_NOTE)?;
    if note.is_none() && mt.is_empty() {
        return Err(ApiError::bad("feedback needs a note, or a mark that says something"));
    }
    let kind = one_of(o.get("kind"), "kind", KINDS)?.unwrap_or_else(|| "change".into());
    let title = text(o.get("title"), "title", 300)?.or_else(|| note.as_deref().map(first_line)).or_else(|| marks.iter().find_map(|m| m["text"].as_str().map(first_line)));
    let now = chloedb::timestamp::now();
    let id = new_id(now);
    let mut doc = json!({
        "id": id, "ts": chloedb::timestamp::format(now), "author": who.name, "author_key": who.key,
        "kind": kind, "status": "new", "title": title, "note": note,
        "page": text(o.get("page").or(o.get("view")), "page", 200)?, "route": route(o.get("route"))?, "url": page_url(o.get("url"))?,
        "cluster_name": svc.identity.json().get("name").cloned().unwrap_or(Json::Null), "server_version": env!("CARGO_PKG_VERSION"),
        "marks_text": (!mt.is_empty()).then_some(mt), "marks": (!marks.is_empty()).then_some(marks_json),
    });
    match o.get("context") {
        None | Some(Json::Null) => {}
        Some(c @ Json::Object(_)) if c.to_string().len() <= MAX_JSON => doc["context"] = c.clone(),
        Some(Json::Object(_)) => return Err(ApiError::bad("the context is too long")),
        Some(_) => return Err(ApiError::bad("context is an object")),
    }
    if let Some(s) = text(o.get("shot"), "shot", usize::MAX)? {
        let (mime, bytes) = picture(&s, MAX_SHOT, "the screenshot")?;
        doc["shot"] = json!(s);
        doc["shot_type"] = json!(mime);
        doc["shot_bytes"] = json!(bytes.len());
        for k in ["shot_w", "shot_h"] {
            if let Some(n) = o.get(k).and_then(|v| v.as_u64()).filter(|n| *n <= 20_000) {
                doc[k] = json!(n);
            }
        }
    }
    if let Some(t) = text(o.get("thumb"), "thumb", usize::MAX)? {
        picture(&t, MAX_THUMB, "the thumbnail")?;
        doc["thumb"] = json!(t);
    }
    ensure(svc).await?;
    let doc = Json::Object(doc.as_object().unwrap().iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect());
    svc.write_ops(vec![json!({"op": "insert", "table": TABLE, "doc": doc})], None).await?;
    Ok(json!({"id": id, "ts": chloedb::timestamp::format(now), "table": TABLE}))
}

fn first_line(s: &str) -> String {
    let l = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    match l.char_indices().nth(120) {
        Some((i, _)) => format!("{}…", &l[..i]),
        None => l.to_string(),
    }
}

/// `GET /v1/feedback`: `status` (a list, or `open` for those still to
/// do, or `all`; `open` by default), `kind`, `q` (words of the note, the
/// title or the marks), `limit` (100; at most 1,000).
pub async fn list(svc: &Arc<Service>, who: &Principal, q: &std::collections::HashMap<String, String>) -> Result<Json, ApiError> {
    may(who, Scope::Read, "reading feedback")?;
    let meta = json!({"statuses": STATUSES, "open": OPEN, "kinds": KINDS, "table": TABLE});
    if !exists(svc).await {
        let mut j = json!({"feedback": [], "counts": {"status": {}, "kind": {}}, "exists": false});
        j.as_object_mut().unwrap().extend(meta.as_object().unwrap().clone());
        return Ok(j);
    }
    let mut conds = Vec::new();
    let mut p = Params::new();
    let statuses: Vec<String> = match q.get("status").map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or("open") {
        "all" => Vec::new(),
        "open" => OPEN.iter().map(|s| s.to_string()).collect(),
        list => list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
    };
    for s in &statuses {
        if !STATUSES.contains(&s.as_str()) {
            return Err(ApiError::bad(format!("status is one of {}, open or all, not {s:?}", STATUSES.join(", "))));
        }
    }
    if !statuses.is_empty() {
        conds.push(format!("status IN ({})", statuses.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")));
    }
    if let Some(k) = q.get("kind").filter(|k| !k.is_empty()) {
        one_of(Some(&json!(k)), "kind", KINDS)?;
        conds.push("kind = :kind".into());
        p = p.set("kind", Value::Str(k.clone()));
    }
    if let Some(a) = q.get("assignee").filter(|k| !k.is_empty()) {
        conds.push("assignee = :assignee".into());
        p = p.set("assignee", Value::Str(a.clone()));
    }
    if let Some(words) = q.get("q").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        conds.push("(MATCH(note, :q) OR MATCH(title, :q) OR MATCH(marks_text, :q))".into());
        p = p.set("q", Value::Str(words.to_string()));
    }
    let limit = q.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(100).clamp(1, 1000);
    let thumbs = q.get("thumbs").is_some_and(|t| t == "1" || t == "true");
    let cols = if thumbs { format!("{LIST_COLUMNS}, thumb") } else { LIST_COLUMNS.to_string() };
    let wh = if conds.is_empty() { String::new() } else { format!(" WHERE {}", conds.join(" AND ")) };
    let rows = select(svc, format!("SELECT {cols} FROM {TABLE}{wh} ORDER BY ts DESC LIMIT {limit}"), p).await?;
    let mut counts = json!({"status": {}, "kind": {}});
    for (col, key) in [("status", "status"), ("kind", "kind")] {
        for r in select(svc, format!("SELECT {col}, COUNT(*) AS n FROM {TABLE} GROUP BY {col}"), Params::new()).await? {
            if let (Some(k), Some(n)) = (r.get(col).and_then(|v| v.as_str()), r.get("n")) {
                counts[key][k] = n.clone();
            }
        }
    }
    let mut j = json!({"feedback": rows, "counts": counts, "exists": true});
    j.as_object_mut().unwrap().extend(meta.as_object().unwrap().clone());
    Ok(j)
}

async fn row(svc: &Arc<Service>, id: &str, cols: &str) -> Result<Map<String, Json>, ApiError> {
    id_ok(id)?;
    if !exists(svc).await {
        return Err(ApiError { code: None, status: 404, message: format!("no feedback {id:?}: none was sent yet") });
    }
    let p = Params::new().set("id", Value::Str(id.into()));
    select(svc, format!("SELECT {cols} FROM {TABLE} WHERE id = :id"), p).await?.into_iter().next().ok_or_else(|| ApiError { code: None, status: 404, message: format!("no feedback {id:?}") })
}

/// `GET /v1/feedback/:id`: the row, its marks and context as JSON, and the
/// paths of its pictures; with `shot`, the screenshot too.
pub async fn get(svc: &Arc<Service>, who: &Principal, id: &str, shot: bool) -> Result<Json, ApiError> {
    may(who, Scope::Read, "reading feedback")?;
    let cols = if shot { format!("{ONE_COLUMNS}, shot") } else { ONE_COLUMNS.to_string() };
    let mut r = row(svc, id, &cols).await?;
    // The marks are kept as text, the context as a JSON field, which a
    // row answers as text too: both as JSON here.
    for k in ["marks", "context"] {
        if let Some(Json::String(m)) = r.get(k) {
            let parsed = serde_json::from_str(m).unwrap_or(Json::Null);
            r.insert(k.into(), parsed);
        }
    }
    let has = r.get("shot_bytes").is_some_and(|b| !b.is_null());
    r.insert("shot_url".into(), if has { json!(format!("/v1/feedback/{id}/shot")) } else { Json::Null });
    Ok(Json::Object(r))
}

/// `GET /v1/feedback/:id/shot` (or `thumb`): the picture's type and bytes.
pub async fn image(svc: &Arc<Service>, who: &Principal, id: &str, thumb: bool) -> Result<(String, Vec<u8>), ApiError> {
    may(who, Scope::Read, "reading feedback")?;
    let col = if thumb { "thumb" } else { "shot" };
    let r = row(svc, id, col).await?;
    match r.get(col).and_then(|v| v.as_str()) {
        Some(s) => picture(s, usize::MAX >> 1, col),
        None => Err(ApiError { code: None, status: 404, message: format!("feedback {id} has no {}", if thumb { "thumbnail" } else { "screenshot" }) }),
    }
}

/// `PATCH /v1/feedback/:id`: what triage sets.
pub async fn update(svc: &Arc<Service>, who: &Principal, id: &str, b: Json) -> Result<Json, ApiError> {
    may(who, Scope::Write, "triaging feedback")?;
    let o = b.as_object().ok_or_else(|| ApiError::bad("the body is an object: {status?, priority?, assignee?, resolution?, kind?, title?}"))?;
    for k in o.keys() {
        if !["status", "priority", "assignee", "resolution", "kind", "title", "if_status"].contains(&k.as_str()) {
            return Err(ApiError::bad(format!("{k:?} is not set here: status, priority, assignee, resolution, kind and title are")));
        }
    }
    let if_status = one_of(o.get("if_status"), "if_status", STATUSES)?;
    let mut sets: Vec<(&str, Json)> = Vec::new();
    if o.contains_key("status") {
        sets.push(("status", json!(one_of(o.get("status"), "status", STATUSES)?.ok_or_else(|| ApiError::bad("status cannot be emptied"))?)));
    }
    if o.contains_key("kind") {
        sets.push(("kind", json!(one_of(o.get("kind"), "kind", KINDS)?.ok_or_else(|| ApiError::bad("kind cannot be emptied"))?)));
    }
    if let Some(p) = o.get("priority") {
        match p {
            Json::Null => sets.push(("priority", Json::Null)),
            p if p.as_u64().is_some_and(|n| n <= 3) => sets.push(("priority", p.clone())),
            _ => return Err(ApiError::bad("priority is 0 (now), 1, 2 or 3 (some day), or null")),
        }
    }
    for (k, max) in [("assignee", 200), ("resolution", 5000), ("title", 300)] {
        if o.contains_key(k) {
            sets.push((k, json!(text(o.get(k), k, max)?)));
        }
    }
    if sets.is_empty() {
        return Err(ApiError::bad("nothing to set: status, priority, assignee, resolution, kind or title"));
    }
    sets.push(("updated", json!(chloedb::timestamp::format(chloedb::timestamp::now()))));
    sets.push(("updated_by", json!(who.name)));
    let ops = sets.into_iter().map(|(f, v)| json!({"op": "set", "table": TABLE, "key": id, "field": f, "value": v})).collect();
    {
        // Triage on this server is one at a time, so the status that
        // `if_status` compares is the one the write replaces: of two agents
        // taking one item, the second gets 409. It holds for the writes that
        // go through this server (one server, or one coordinator), not across
        // coordinators.
        let _one = TRIAGE.lock().await;
        let now = row(svc, id, "status").await?;
        if let Some(want) = &if_status {
            let is = now.get("status").and_then(|s| s.as_str()).unwrap_or("");
            if is != want {
                return Err(ApiError { code: None, status: 409, message: format!("feedback {id} is {is}, not {want}: someone changed it first") });
            }
        }
        svc.write_ops(ops, None).await?;
    }
    get(svc, who, id, false).await
}

/// `DELETE /v1/feedback/:id`.
pub async fn delete(svc: &Arc<Service>, who: &Principal, id: &str) -> Result<Json, ApiError> {
    may(who, Scope::Admin, "deleting feedback")?;
    row(svc, id, "id").await?;
    svc.write_ops(vec![json!({"op": "delete", "table": TABLE, "key": id})], None).await?;
    Ok(json!({"deleted": id}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_checked_by_their_bytes() {
        let png = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nrest"));
        assert_eq!(picture(&png, 1 << 20, "shot").unwrap().0, "image/png");
        let lie = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n"));
        assert!(picture(&lie, 1 << 20, "shot").is_err());
        assert!(picture("data:image/svg+xml;base64,PHN2Zz4=", 1 << 20, "shot").is_err());
        assert!(picture("https://x/a.png", 1 << 20, "shot").is_err());
        let big = format!("data:image/png;base64,{}", "A".repeat(2 << 20));
        assert_eq!(picture(&big, 1 << 20, "shot").unwrap_err().status, 413);
    }

    #[test]
    fn marks_read_as_lines_and_redactions_say_nothing() {
        let ms = vec![
            mark(&json!({"tool": "box", "x": 10, "y": 20, "w": 30, "h": 40, "text": "too small", "target": {"selector": "button.btn", "text": "Add shard", "card": "Shards"}}), 0).unwrap(),
            mark(&json!({"tool": "redact", "x": 1, "y": 1, "w": 5, "h": 5, "target": {"text": "sk_live_secret"}}), 1).unwrap(),
            mark(&json!({"tool": "arrow", "x": 0, "y": 0, "x2": 5, "y2": 5}), 2).unwrap(),
        ];
        assert!(ms[1].get("target").is_none(), "{}", ms[1]);
        assert_eq!(marks_text(&ms), "1. box \"too small\" on \"Shards\" (button.btn: Add shard)\n3. arrow");
        assert!(mark(&json!({"tool": "laser"}), 0).is_err());
    }

    #[test]
    fn ids_are_short_and_safe() {
        let id = new_id(chloedb::timestamp::now());
        assert!(id.starts_with("fb-") && id.len() == 18, "{id}");
        assert!(id_ok(&id).is_ok());
        assert!(id_ok("a' OR '1'='1").is_err());
    }
}
