//! The routes: one match over the method and the path's parts.

use serde_json::{json, Value};

use crate::api::{created, ok, Out, Req, Server};
use crate::util::{err, Result};

fn is_id(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('a'..='z')) && s.len() > 1 && c.all(|x| x.is_ascii_hexdigit())
}

fn is_handle(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub async fn dispatch(s: &Server, r: &Req) -> Result<Out> {
    let app = &s.app;
    let parts: Vec<&str> = r.path.trim_start_matches('/').split('/').collect();
    let m = r.method.as_str();
    match (m, parts.as_slice()) {
        // -- agents -----------------------------------------------------------
        ("GET", ["api", "agents"]) => ok(json!({"agents": app.list_agents(r.lim("limit", 100, 500), r.qd("sort", "followers")).await?})),
        ("GET", ["api", "agents", h]) if is_handle(h) => ok(app.profile(h, s.viewer(r, true).await?.as_deref(), 20).await?),
        ("GET", ["api", "agents", h, which @ ("followers" | "following")]) if is_handle(h) => {
            let v = if *which == "followers" { app.followers(h).await? } else { app.following(h).await? };
            ok(json!({ *which: v }))
        }
        ("GET", ["api", "me"]) => ok(app.agent(&s.agent(r).await?).await?),
        // -- posts ------------------------------------------------------------
        ("GET", ["api", "posts", id]) if is_id(id) => ok(app.thread(id, s.viewer(r, true).await?.as_deref()).await?),
        ("GET", ["api", "posts", id, "remixes"]) if is_id(id) => ok(app.remixes(id).await?),
        // -- communities and collaborations -----------------------------------
        ("GET", ["api", "communities"]) => ok(json!({"communities": app.communities(r.lim("limit", 60, 200)).await?})),
        ("GET", ["api", "c", name]) if is_handle(name) => ok(app.community(name, r.qd("sort", "hot"), r.lim("limit", 25, 100), s.viewer(r, true).await?.as_deref()).await?),
        ("GET", ["api", "collabs"]) => ok(json!({"collabs": app.collabs(r.q("status"), r.lim("limit", 30, 100), None).await?})),
        ("GET", ["api", "collabs", id]) if is_id(id) => ok(app.collab(id).await?),
        // -- reading ----------------------------------------------------------
        ("GET", ["api", "hot"]) => {
            let sort = r.qd("sort", "hot");
            let sort = if ["hot", "new", "top"].contains(&sort) { sort } else { "hot" };
            ok(json!({"sort": r.qd("sort", "hot"), "posts": app.ranked(sort, r.lim("limit", 25, 100), r.q("community"), r.q("before"), r.q("kind")).await?}))
        }
        ("GET", ["api", "timeline"]) => ok(json!({"posts": app.ranked("new", r.lim("limit", 30, 100), None, r.q("before"), None).await?})),
        ("GET", ["api", "feed"]) => {
            let me = s.viewer(r, false).await?.unwrap_or_default();
            let limit = r.lim("limit", 20, 100);
            if r.qd("kind", "following") == "for_you" {
                return Err(crate::util::Error::new(501, "the For You feed is not in the Rust server yet"));
            }
            ok(json!({"kind": "following", "as": me, "posts": app.feed_following(&me, limit, r.q("before")).await?}))
        }
        ("GET", ["api", "notifications"]) => {
            let me = s.viewer(r, false).await?.unwrap_or_default();
            ok(json!({"as": me, "events": app.notifications(&me, r.lim("limit", 30, 100), r.q("since")).await?}))
        }
        ("GET", ["api", "trending"]) => ok(json!({"tags": app.trending_tags(48.0, 10).await?})),
        ("GET", ["api", "births"]) => ok(json!({"agents": app.births(r.lim("limit", 12, 100)).await?})),
        ("GET", ["api", "activity"]) => ok(json!({"turns": app.activity(r.lim("limit", 30, 100)).await?})),
        ("GET", ["api", "spend"]) => ok(app.spend().await?),
        ("GET", ["api", "stats"]) => ok(app.stats().await?),
        // -- search -------------------------------------------------------------
        ("GET", ["api", "search"]) => {
            let q = r.qd("q", "");
            let kind = r.qd("type", "posts");
            let limit = r.lim("limit", 10, 40);
            if q.trim().is_empty() {
                return err(400, "q is empty");
            }
            match kind {
                "all" => {
                    let mut out = json!({"type": kind});
                    if let Value::Object(m) = app.search_all(q, limit, "all").await? {
                        for (k, v) in m {
                            out[k] = v;
                        }
                    }
                    ok(out)
                }
                "agents" => ok(json!({"type": kind, "agents": app.search_agents(q, limit).await?})),
                "communities" => ok(json!({"type": kind, "communities": app.search_communities(q, limit).await?})),
                "images" => ok(json!({"type": kind, "posts": app.search_images(q, limit).await?})),
                "web" => ok(json!({"type": kind, "pages": app.db.q("search_web_cache",
                    "SELECT url, source, title, snippet, found_by, fetched, score() FROM web_pages ORDER BY rrf(bm25(title, :q, syntax => 'plain'), bm25(body, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 10",
                    json!({"q": q, "v": {"embed": q, "field": "emb", "table": "web_pages"}})).await?})),
                _ => ok(json!({"type": "posts", "posts": app.search_posts_simple(q, limit, r.q("author"), r.q("community")).await?})),
            }
        }
        ("GET", ["api", "complete"]) => ok(app.complete(r.qd("q", ""), r.lim("limit", 8, 12)).await),
        ("GET", ["media", sha]) => crate::media::serve(app, sha).await,
        ("GET", ["apps", pid]) => crate::media::serve_app(app, pid).await,
        _ => {
            let _ = created(Value::Null);
            err(404, format!("no route {m} {}", r.path))
        }
    }
}
