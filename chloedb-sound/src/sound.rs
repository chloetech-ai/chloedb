//! What each page asks ChloeDB: searches by words and by how things sound,
//! a recording's page, the moments inside long recordings, a clip someone
//! hums, scenes, drifts and the map. Every query is SQL over the three
//! tables, and kept in the request's [`Trace`] for the page's SQL button.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::strata::{Result, Row, RowExt, Strata, Trace};
use crate::vecs::{self, Embedder, Label};

/// The columns of a sound as the pages show it.
pub const CARD: &str = "id, title, author, author_url, page, source, licence, licence_url, lic, kind, seconds, year, peaks, tags, labels, genres, moments, lat, lon";

pub struct Ctx<'a> {
    pub db: &'a Strata,
    pub emb: &'a Embedder,
    pub trace: Trace,
}

impl Ctx<'_> {
    pub async fn q(&self, what: &str, sql: &str, params: Value) -> Result<Vec<Row>> {
        self.db.traced(self.trace_ref(), what, sql, params).await
    }
    fn trace_ref(&self) -> &Trace {
        &self.trace
    }
}

/// A source's name as a credit line gives it.
pub fn source_name(s: &str) -> &str {
    match s {
        "fsd50k" => "Freesound (FSD50K)",
        "commons" => "Wikimedia Commons",
        "fma" => "Free Music Archive",
        o => o,
    }
}

/// The credit a reuse gives: title, author, source and licence, linked.
pub fn credit(r: &Row) -> String {
    let by = if r.s("author").is_empty() { String::new() } else { format!(" by {}", r.s("author")) };
    format!("\u{201c}{}\u{201d}{by}, {} ({}), {} ({})", r.s("title"), source_name(&r.s("source")), r.s("page"), r.s("licence"), r.s("licence_url"))
}

/// A sound for a page: its columns, its credit line, and the score it was found by.
pub fn card(r: &Row) -> Value {
    let mut v = Value::Object(r.clone());
    v["credit"] = json!(credit(r));
    v["source_name"] = json!(source_name(&r.s("source")));
    if let Some(o) = v.as_object_mut() {
        o.remove("emb");
    }
    v
}

#[derive(Default, Clone, Debug)]
pub struct Search {
    pub q: String,
    /// Words of what must not be in it ("no thunder").
    pub not: String,
    /// Sounds to come closer to, and to move away from.
    pub like: Vec<String>,
    pub unlike: Vec<String>,
    /// Directions: heavier, lighter, drier, wetter, closer, farther, busier, calmer.
    pub steer: Vec<String>,
    pub source: Vec<String>,
    pub kind: Vec<String>,
    pub lic: Vec<String>,
    pub min_s: Option<f64>,
    pub max_s: Option<f64>,
    /// `both` (the default), `sound` (CLAP alone) or `words` (BM25 alone).
    pub by: String,
    pub page: usize,
    pub per: usize,
}

/// The directions of the steering buttons: towards the first sentence and
/// away from the second.
pub const STEER: &[(&str, &str, &str)] = &[
    ("heavier", "a heavy, deep, powerful, low sound", "a light, thin, delicate, high sound"),
    ("lighter", "a light, thin, delicate, high sound", "a heavy, deep, powerful, low sound"),
    ("drier", "a dry sound recorded close in a small dead room, no echo", "a sound with long reverb and echo in a large hall"),
    ("wetter", "a sound with long reverb and echo in a large hall", "a dry sound recorded close in a small dead room, no echo"),
    ("closer", "a sound recorded very close to the microphone", "a sound far away in the distance"),
    ("farther", "a sound far away in the distance", "a sound recorded very close to the microphone"),
    ("busier", "a busy, dense sound with many things happening at once", "a single calm sound alone in silence"),
    ("calmer", "a single calm sound alone in silence", "a busy, dense sound with many things happening at once"),
];

/// The filters of a search as SQL, and their parameters.
fn filters(s: &Search, params: &mut Value) -> String {
    let mut w = Vec::new();
    for (col, vals) in [("source", &s.source), ("kind", &s.kind), ("lic", &s.lic)] {
        if !vals.is_empty() {
            params[format!("f_{col}")] = json!(vals);
            w.push(format!("{col} IN (:f_{col})"));
        }
    }
    if let Some(m) = s.min_s {
        params["min_s"] = json!(m);
        w.push("seconds >= :min_s".into());
    }
    if let Some(m) = s.max_s {
        params["max_s"] = json!(m);
        w.push("seconds <= :max_s".into());
    }
    if w.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", w.join(" AND "))
    }
}

async fn vectors_of(c: &Ctx<'_>, ids: &[String], what: &str) -> Result<HashMap<String, Vec<f32>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = c.q(what, "SELECT id, emb FROM au_sounds WHERE id IN (:ids)", json!({"ids": ids})).await?;
    Ok(rows.iter().map(|r| (r.s("id"), vecs::floats(r.get("emb").unwrap_or(&Value::Null)))).filter(|(_, v)| v.len() == crate::schema::DIM).collect())
}

/// The vector a search looks for: its words', moved towards the sounds it
/// likes and the directions it steers, away from its "not" words and the
/// sounds it does not like. None when it has neither words nor sounds.
pub async fn search_vector(c: &Ctx<'_>, s: &Search) -> Result<(Option<Vec<f32>>, Vec<Value>)> {
    let mut v = vec![0f32; crate::schema::DIM];
    let mut any = false;
    let mut understood = Vec::new();
    // Ranked by words alone, the words need no vector. A server that
    // cannot run CLAP (one built without the `audio` feature, or without
    // its model) still searches by words, and says so.
    let mut deaf = false;
    if !s.q.trim().is_empty() && s.by != "words" {
        match c.emb.text(&s.q).await {
            Ok(x) => {
                vecs::add(&mut v, &x, 1.0);
                any = true;
            }
            Err(e) if s.like.is_empty() => {
                tracing::warn!("no vector for the words, searching by words only: {}", e.message);
                deaf = true;
                understood.push(json!({"kind": "note", "text": "by words only: this server cannot listen by meaning right now"}));
            }
            Err(e) => return Err(e),
        }
    }
    if !s.q.trim().is_empty() {
        understood.push(json!({"kind": "words", "text": s.q.trim()}));
    }
    let ids: Vec<String> = s.like.iter().chain(&s.unlike).cloned().collect();
    let got = vectors_of(c, &ids, "the vectors of the sounds steered by").await?;
    for id in &s.like {
        if let Some(x) = got.get(id) {
            vecs::add(&mut v, x, if any { 0.6 } else { 1.0 });
            any = true;
            understood.push(json!({"kind": "like", "id": id}));
        }
    }
    if !any || deaf {
        return Ok((None, understood));
    }
    vecs::norm(&mut v);
    for id in &s.unlike {
        if let Some(x) = got.get(id) {
            vecs::add(&mut v, x, -0.35);
            understood.push(json!({"kind": "unlike", "id": id}));
        }
    }
    if !s.not.trim().is_empty() {
        let n = c.emb.text(&format!("the sound of {}", s.not.trim())).await?;
        vecs::add(&mut v, &n, -0.4);
        understood.push(json!({"kind": "not", "text": s.not.trim()}));
    }
    for d in &s.steer {
        if let Some((name, to, from)) = STEER.iter().find(|(n, _, _)| n == d) {
            let (a, b) = (c.emb.text(to).await?, c.emb.text(from).await?);
            vecs::add(&mut v, &a, 0.3);
            vecs::add(&mut v, &b, -0.3);
            understood.push(json!({"kind": "steer", "text": name}));
        }
    }
    vecs::norm(&mut v);
    Ok((Some(v), understood))
}

/// Sounds by words and by how they sound. With words only it ranks by both
/// (CLAP's similarity and BM25, fused by `tmm`); `by` picks one.
pub async fn search(c: &Ctx<'_>, s: &Search) -> Result<Value> {
    let (v, understood) = search_vector(c, s).await?;
    let mut params = json!({});
    let wh = filters(s, &mut params);
    let per = s.per.clamp(1, 60);
    let window = 200;
    let words = s.q.trim();
    let by = if v.is_none() || (s.by == "words" && !words.is_empty()) {
        if words.is_empty() { "browse" } else { "words" }
    } else if s.by == "sound" || words.is_empty() {
        "sound"
    } else {
        "both"
    };
    if let Some(v) = &v {
        params["v"] = vecs::param(v);
    }
    if !words.is_empty() {
        params["q"] = json!(words);
    }
    let order = match by {
        "words" => "bm25(title, :q) + 0.6 * coalesce(bm25(words, :q), 0)".to_string(),
        "sound" => "similarity(emb, :v)".to_string(),
        "both" => "0.75 * tmm(similarity(emb, :v)) + 0.25 * tmm(bm25(words, :q))".to_string(),
        _ => "moments".to_string(),
    };
    let sql = format!("SELECT {CARD}, score() AS score FROM au_sounds {wh} ORDER BY {order} DESC LIMIT {window}");
    let rows = c.q(&format!("sounds, ranked by {}", match by { "both" => "sound and words", "sound" => "how they sound", "words" => "words", _ => "length" }), &sql, params).await?;
    let mut facets: HashMap<&str, HashMap<String, usize>> = HashMap::new();
    for r in &rows {
        for col in ["source", "kind", "lic"] {
            *facets.entry(col).or_default().entry(r.s(col)).or_default() += 1;
        }
    }
    let facets: Value = facets.into_iter().map(|(k, m)| {
        let mut l: Vec<(String, usize)> = m.into_iter().filter(|(v, _)| !v.is_empty()).collect();
        l.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        (k.to_string(), json!(l.into_iter().map(|(v, n)| json!({"value": v, "n": n})).collect::<Vec<_>>()))
    }).collect::<serde_json::Map<_, _>>().into();
    let from = (s.page * per).min(rows.len());
    let to = (from + per).min(rows.len());
    Ok(json!({
        "by": by,
        "understood": understood,
        "results": rows[from..to].iter().map(card).collect::<Vec<_>>(),
        "total": rows.len(),
        "more": to < rows.len(),
        "facets": facets,
    }))
}

/// A sound's moments with their vectors, in order.
async fn moments_of(c: &Ctx<'_>, id: &str) -> Result<Vec<(f64, Vec<f32>)>> {
    let rows = c.q("its moments, every 5 seconds", "SELECT t, emb FROM au_moments WHERE sound = :id ORDER BY t LIMIT 2000", json!({"id": id})).await?;
    Ok(rows.iter().map(|r| (r.f("t"), vecs::floats(r.get("emb").unwrap_or(&Value::Null)))).filter(|(_, v)| v.len() == crate::schema::DIM).collect())
}

/// What is in a recording, moment by moment: each label CLAP hears with a
/// fair chance (its odds among all the labels), as layers over time.
pub fn layers(moments: &[(f64, Vec<f32>)], labels: &[Label], seconds: f64) -> Vec<Value> {
    if moments.is_empty() || labels.is_empty() {
        return Vec::new();
    }
    // Each moment's odds over the labels.
    let odds: Vec<Vec<f32>> = moments.iter().map(|(_, m)| vecs::softmax(&labels.iter().map(|l| vecs::dot(m, &l.v)).collect::<Vec<_>>(), 100.0)).collect();
    let mut total = vec![0f32; labels.len()];
    let mut best = vec![0f32; labels.len()];
    for o in &odds {
        for (i, p) in o.iter().enumerate() {
            total[i] += p;
            best[i] = best[i].max(*p);
        }
    }
    let mut order: Vec<usize> = (0..labels.len()).filter(|&i| best[i] >= 0.12).collect();
    order.sort_by(|&a, &b| total[b].partial_cmp(&total[a]).unwrap_or(std::cmp::Ordering::Equal));
    order.truncate(6);
    order
        .into_iter()
        .map(|i| {
            // Where it is heard: the moments where its odds pass a third of its best.
            let mut spans: Vec<[f64; 2]> = Vec::new();
            for (k, o) in odds.iter().enumerate() {
                if o[i] >= best[i] / 3.0 && o[i] >= 0.05 {
                    let t0 = moments[k].0;
                    let t1 = (t0 + 10.0).min(seconds.max(t0 + 1.0));
                    match spans.last_mut() {
                        Some(s) if s[1] >= t0 => s[1] = s[1].max(t1),
                        _ => spans.push([t0, t1]),
                    }
                }
            }
            json!({"id": labels[i].id, "name": labels[i].name, "group": labels[i].grp, "odds": (best[i] * 100.0).round() / 100.0, "share": (total[i] / odds.len() as f32 * 100.0).round() / 100.0, "spans": spans})
        })
        .collect()
}

/// A sound's page: it, its credit, what is in it, sounds like it and more
/// by its author.
pub async fn sound(c: &Ctx<'_>, id: &str) -> Result<Option<Value>> {
    let rows = c.q("the sound", &format!("SELECT {CARD}, x, y FROM au_sounds WHERE id = :id"), json!({"id": id})).await?;
    let Some(r) = rows.into_iter().next() else { return Ok(None) };
    let (moments, labels) = (moments_of(c, id).await?, c.emb.labels(Some(&c.trace)).await?);
    let like = c.q("sounds like it", &format!("SELECT {CARD}, score() AS score FROM au_sounds WHERE id <> :id ORDER BY similarity(emb, :v) DESC LIMIT 12"), json!({"id": id, "v": {"vector_of": id, "table": "au_sounds", "field": "emb"}})).await?;
    let more = if r.s("author").is_empty() { Vec::new() } else { c.q("more by its author", &format!("SELECT {CARD} FROM au_sounds WHERE author = :a AND id <> :id ORDER BY seconds DESC LIMIT 6"), json!({"a": r.s("author"), "id": id})).await? };
    Ok(Some(json!({
        "sound": card(&r),
        "layers": layers(&moments, &labels, r.f("seconds")),
        "moments": moments.len(),
        "like": like.iter().map(card).collect::<Vec<_>>(),
        "more": more.iter().map(card).collect::<Vec<_>>(),
    })))
}

/// The cards of sounds by id, in the order given.
async fn cards(c: &Ctx<'_>, ids: &[String], what: &str) -> Result<Vec<Value>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = c.q(what, &format!("SELECT {CARD} FROM au_sounds WHERE id IN (:ids)"), json!({"ids": ids})).await?;
    let by: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    Ok(ids.iter().filter_map(|i| by.get(i).map(card)).collect())
}

/// The moments inside recordings that sound like `v`: the best recording
/// by its best moment, each with its other good moments.
pub async fn moments_like(c: &Ctx<'_>, v: &[f32], min_seconds: f64, n: usize, music: bool) -> Result<Vec<Value>> {
    let p = json!({"v": vecs::param(v), "min": min_seconds});
    // Without music (the Moments page, about recordings), more are read and
    // the songs dropped once their cards say which they are.
    let read = if music { n } else { n * 3 };
    let best = c.q("the best moment of each recording", &format!("SELECT DISTINCT ON (sound) sound, t, score() AS score FROM au_moments WHERE seconds >= :min ORDER BY similarity(emb, :v) DESC LIMIT {read}"), p.clone()).await?;
    let ids: Vec<String> = best.iter().map(|r| r.s("sound")).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut p2 = p;
    p2["ids"] = json!(ids);
    let others = c.q("their other good moments", &format!("SELECT sound, t, score() AS score FROM au_moments WHERE sound IN (:ids) ORDER BY similarity(emb, :v) DESC LIMIT {}", n * 8), p2).await?;
    let mut times: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    for r in &others {
        times.entry(r.s("sound")).or_default().push((r.f("t"), r.f("score")));
    }
    let cs = cards(c, &ids, "the recordings").await?;
    Ok(best
        .iter()
        .zip(cs)
        .map(|(b, mut cd)| {
            let top = b.f("score");
            let mut ts: Vec<(f64, f64)> = times.remove(&b.s("sound")).unwrap_or_default().into_iter().filter(|(_, s)| *s >= top - 0.06).collect();
            ts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            // Moments 5 s apart overlap: keep the better of two neighbours.
            let mut kept: Vec<(f64, f64)> = Vec::new();
            for t in ts {
                if !kept.iter().any(|k| (k.0 - t.0).abs() < 9.9) {
                    kept.push(t);
                }
            }
            kept.truncate(5);
            cd["t"] = json!(b.f("t"));
            cd["score"] = json!(top);
            cd["at"] = json!(kept.iter().map(|(t, s)| json!({"t": t, "score": s})).collect::<Vec<_>>());
            cd
        })
        .filter(|cd| music || cd["kind"] != "music")
        .take(n)
        .collect())
}

/// A clip someone recorded or dropped: the sounds it sounds like, and the
/// recordings it may be cut from (its best match among every moment).
pub async fn listen(c: &Ctx<'_>, b64: &str) -> Result<Value> {
    let t0 = std::time::Instant::now();
    let v = c.emb.audio(b64).await?;
    c.trace.push("the clip, heard by CLAP in ChloeDB (POST /v1/embed)", "-- not SQL: the server's model of sound and text embeds the clip", &Value::Null, 1, t0.elapsed().as_secs_f64() * 1000.0, None);
    let like = c.q("sounds like the clip", &format!("SELECT {CARD}, score() AS score FROM au_sounds ORDER BY similarity(emb, :v) DESC LIMIT 24"), json!({"v": vecs::param(&v)})).await?;
    let from = moments_like(c, &v, 0.0, 6, true).await?;
    // A clip cut from a recording here matches one of its moments far
    // better than anything else does.
    let original = from.first().filter(|m| m["score"].as_f64().unwrap_or(0.0) >= 0.9).cloned();
    Ok(json!({"like": like.iter().map(card).collect::<Vec<_>>(), "moments": from, "original": original}))
}

/// A place described in words, as layers of sounds to play together: the
/// sounds nearest the words and their parts, a layer for each thing they
/// sound like most (their best label by sound), unlike one another.
pub async fn scene(c: &Ctx<'_>, text: &str) -> Result<Value> {
    // The place as a whole and its parts ("a rainy café" and "a harbour"),
    // each heard by CLAP; the sounds nearest any of them are the candidates.
    let mut asks = vec![(text.trim().to_string(), 1.0f32)];
    for part in scene_parts(text) {
        asks.push((part, 0.92));
    }
    let labels = c.emb.labels(Some(&c.trace)).await?;
    let leaves: Vec<&Label> = labels.iter().filter(|l| l.name != l.grp).collect();
    let mut best: HashMap<String, (f32, Row, Vec<f32>)> = HashMap::new();
    for (i, (q, w)) in asks.iter().enumerate() {
        let tv = c.emb.text(q).await?;
        let rows = c.q(&format!("sounds of \u{201c}{q}\u{201d}"), &format!("SELECT {CARD}, emb, score() AS score FROM au_sounds WHERE seconds >= 4 ORDER BY similarity(emb, :v) DESC LIMIT {}", if i == 0 { 40 } else { 20 }), json!({"v": vecs::param(&tv)})).await?;
        for r in rows {
            let v = vecs::floats(r.get("emb").unwrap_or(&Value::Null));
            let sc = r.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32 * w;
            let id = r.s("id");
            if v.len() == crate::schema::DIM && best.get(&id).map_or(true, |b| b.0 < sc) {
                best.insert(id, (sc, r, v));
            }
        }
    }
    let mut cands: Vec<(f32, Row, Vec<f32>)> = best.into_values().collect();
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    // A layer each for what the candidates sound like most (their best label
    // by sound), the best sound of each, unlike the layers before it.
    let mut layers = Vec::new();
    let mut used_labels: Vec<&Label> = Vec::new();
    let mut picked: Vec<Vec<f32>> = Vec::new();
    for (_, r, v) in &cands {
        if layers.len() >= 4 {
            break;
        }
        let Some(l) = leaves.iter().max_by(|a, b| vecs::dot(v, &a.v).partial_cmp(&vecs::dot(v, &b.v)).unwrap_or(std::cmp::Ordering::Equal)) else { break };
        // Not rain twice: a label like one already playing (rain, raindrop)
        // is the same layer.
        if used_labels.iter().any(|u| u.id == l.id || vecs::dot(&u.v, &l.v) > 0.8) || picked.iter().any(|p| vecs::dot(p, v) > 0.9) {
            continue;
        }
        used_labels.push(l);
        picked.push(v.clone());
        let gain = [1.0, 0.7, 0.55, 0.45][layers.len()];
        let mut r = r.clone();
        r.remove("emb");
        layers.push(json!({"label": l.name, "group": l.grp, "gain": gain, "sound": card(&r)}));
    }
    let credits: Vec<String> = layers.iter().filter_map(|l| l["sound"]["credit"].as_str().map(str::to_string)).collect();
    Ok(json!({"text": text, "layers": layers, "credits": credits}))
}

/// The parts of a described place: "a rainy café by a harbour" is "a rainy
/// café" and "a harbour".
pub fn scene_parts(text: &str) -> Vec<String> {
    let mut parts = vec![text.to_lowercase()];
    for sep in [",", ";", " and ", " with ", " by ", " near ", " beside ", " next to ", " in ", " on ", " at ", " under ", " over "] {
        parts = parts.iter().flat_map(|p| p.split(sep).map(str::to_string).collect::<Vec<_>>()).collect();
    }
    let parts: Vec<String> = parts.into_iter().map(|p| p.trim().to_string()).filter(|p| p.split_whitespace().any(|w| !["a", "an", "the", "of", "some"].contains(&w))).collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    parts.into_iter().take(4).collect()
}

/// A path of sounds from one to another: at each step the sound nearest
/// the point that far along the way between them.
pub async fn drift(c: &Ctx<'_>, from: &str, to: &str, steps: usize, kind: Option<&str>) -> Result<Value> {
    let got = vectors_of(c, &[from.to_string(), to.to_string()], "the two ends").await?;
    let (Some(a), Some(b)) = (got.get(from), got.get(to)) else {
        return Ok(json!({"path": [], "error": "no such sound"}));
    };
    let mut used: HashSet<String> = [from.to_string(), to.to_string()].into_iter().collect();
    let mut path = vec![from.to_string()];
    let steps = steps.clamp(2, 16);
    let wh = if kind.is_some() { "WHERE kind = :kind" } else { "" };
    for i in 1..steps {
        let v = vecs::slerp(a, b, i as f32 / steps as f32);
        let mut p = json!({"v": vecs::param(&v)});
        if let Some(k) = kind {
            p["kind"] = json!(k);
        }
        let rows = c.q(&format!("step {i} of {steps}"), &format!("SELECT id FROM au_sounds {wh} ORDER BY similarity(emb, :v) DESC LIMIT 12"), p).await?;
        if let Some(r) = rows.iter().find(|r| !used.contains(&r.s("id"))) {
            used.insert(r.s("id"));
            path.push(r.s("id"));
        }
    }
    path.push(to.to_string());
    Ok(json!({"path": cards(c, &path, "the sounds of the path").await?}))
}

/// Every sound's place on the map: by how it sounds (a layout of the
/// vectors made by tools/prepare.py) and, when the recording says, where
/// it was made.
pub async fn map(c: &Ctx<'_>, limit: usize) -> Result<Value> {
    // Every row, a page of 10,000 at a time (a query returns at most that).
    let sql = "SELECT id, title, kind, source, seconds, x, y, lat, lon FROM au_sounds";
    let t0 = std::time::Instant::now();
    let mut pts: Vec<Value> = Vec::new();
    c.db.scroll(sql, Value::Null, 10_000, |rows| {
        for r in rows {
            if pts.len() < limit {
                pts.push(json!([r.s("id"), r.s("title"), r.s("kind"), r.s("source"), r.f("x"), r.f("y"), r.get("lat").cloned().unwrap_or(Value::Null), r.get("lon").cloned().unwrap_or(Value::Null), r.f("seconds")]));
            }
        }
    })
    .await?;
    c.trace.push("every sound's place, read a page at a time", sql, &Value::Null, pts.len(), t0.elapsed().as_secs_f64() * 1000.0, None);
    Ok(json!({"points": pts}))
}

/// The front page: a few sounds of each kind, and the counts.
pub async fn home(c: &Ctx<'_>) -> Result<Value> {
    let mut shelves = Vec::new();
    for (kind, title) in [("place", "Places and long recordings"), ("nature", "Nature and weather"), ("animal", "Animals"), ("effect", "Sound effects"), ("music", "Music")] {
        let rows = c.q(&format!("a shelf: {title}"), &format!("SELECT {CARD} FROM au_sounds WHERE kind = :k AND seconds >= 3 ORDER BY moments DESC LIMIT 40"), json!({"k": kind})).await?;
        // A different few every day.
        let day = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) / 86_400) as usize;
        let n = rows.len();
        let pick: Vec<Value> = (0..n.min(6)).map(|i| card(&rows[(day * 7 + i * 13) % n])).collect();
        let mut seen = HashSet::new();
        let pick: Vec<Value> = pick.into_iter().filter(|p| seen.insert(p["id"].as_str().unwrap_or("").to_string())).collect();
        if !pick.is_empty() {
            shelves.push(json!({"kind": kind, "title": title, "sounds": pick}));
        }
    }
    Ok(json!({"shelves": shelves}))
}

/// The counts behind "How it works".
pub async fn stats(c: &Ctx<'_>) -> Result<Value> {
    let by = c.q("recordings by source", "SELECT source, COUNT(*) AS n, SUM(seconds) AS s FROM au_sounds GROUP BY source ORDER BY n DESC", Value::Null).await?;
    let lic = c.q("recordings by licence", "SELECT lic, COUNT(*) AS n FROM au_sounds GROUP BY lic ORDER BY n DESC", Value::Null).await?;
    let kinds = c.q("recordings by kind", "SELECT kind, COUNT(*) AS n FROM au_sounds GROUP BY kind ORDER BY n DESC", Value::Null).await?;
    let m = c.q("moments", "SELECT COUNT(*) AS n FROM au_moments", Value::Null).await?;
    let l = c.q("labels", "SELECT COUNT(*) AS n FROM au_labels", Value::Null).await?;
    let sounds: i64 = by.iter().map(|r| r.i("n")).sum();
    let seconds: f64 = by.iter().map(|r| r.f("s")).sum();
    Ok(json!({
        "sounds": sounds,
        "hours": (seconds / 360.0).round() / 10.0,
        "moments": m.first().map(|r| r.i("n")).unwrap_or(0),
        "labels": l.first().map(|r| r.i("n")).unwrap_or(0),
        "sources": by.iter().map(|r| json!({"source": r.s("source"), "name": source_name(&r.s("source")), "n": r.i("n"), "hours": (r.f("s") / 360.0).round() / 10.0})).collect::<Vec<_>>(),
        "licences": lic.iter().map(|r| json!({"lic": r.s("lic"), "n": r.i("n")})).collect::<Vec<_>>(),
        "kinds": kinds.iter().map(|r| json!({"kind": r.s("kind"), "n": r.i("n")})).collect::<Vec<_>>(),
    }))
}

/// The labels, for the chips that start a search.
pub async fn labels(emb: &Arc<Embedder>) -> Result<Vec<Value>> {
    Ok(emb.labels(None).await?.iter().map(|l| json!({"id": l.id, "name": l.name, "group": l.grp})).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_place_is_cut_into_its_parts() {
        assert_eq!(super::scene_parts("a rainy café by a harbour"), vec!["a rainy café", "a harbour"]);
        assert_eq!(super::scene_parts("Market, with gulls and a bell"), vec!["market", "gulls", "a bell"]);
        assert!(super::scene_parts("a forest at night").len() == 2);
        assert!(super::scene_parts("thunderstorm").is_empty());
    }

    use super::*;

    fn label(id: &str, v: Vec<f32>) -> Label {
        Label { id: id.into(), name: id.into(), grp: "g".into(), v }
    }

    #[test]
    fn layers_find_what_is_heard_and_where() {
        // Two labels on two axes; the first 20 s sound like the first,
        // the last 10 s like the second.
        let labels = vec![label("rain", vec![1.0, 0.0]), label("thunder", vec![0.0, 1.0])];
        let m = vec![(0.0, vec![1.0, 0.0]), (5.0, vec![1.0, 0.0]), (10.0, vec![1.0, 0.0]), (15.0, vec![0.0, 1.0]), (20.0, vec![0.0, 1.0])];
        let l = layers(&m, &labels, 30.0);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0]["id"], "rain");
        assert_eq!(l[0]["spans"], json!([[0.0, 20.0]]));
        assert_eq!(l[1]["spans"], json!([[15.0, 30.0]]));
    }

    #[test]
    fn credit_names_title_author_source_and_licence() {
        let r: Row = serde_json::from_value(json!({"title": "Rain on a tin roof", "author": "ana", "source": "fsd50k", "page": "https://freesound.org/s/1/", "licence": "CC BY 3.0", "licence_url": "https://creativecommons.org/licenses/by/3.0/"})).unwrap();
        assert_eq!(credit(&r), "\u{201c}Rain on a tin roof\u{201d} by ana, Freesound (FSD50K) (https://freesound.org/s/1/), CC BY 3.0 (https://creativecommons.org/licenses/by/3.0/)");
    }
}
