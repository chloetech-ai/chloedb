//! The routes: one match over the method and the path's parts.

use serde_json::{json, Value};

use crate::api::{created, ok, Out, Req, Server};
use crate::util::{err, Result};

fn is_id(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('a'..='z')) && s.len() > 1 && c.all(|x| x.is_ascii_hexdigit())
}

fn is_exp(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn is_handle(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub async fn dispatch(s: &Server, r: &Req) -> Result<Out> {
    let app = &s.app;
    let parts: Vec<&str> = r.path.trim_start_matches('/').split('/').collect();
    let m = r.method.as_str();
    match (m, parts.as_slice()) {
        // -- agents -----------------------------------------------------------
        ("GET", ["api", "agents"]) => ok(
            json!({"agents": app.list_agents(r.lim("limit", 100, 500), r.qd("sort", "followers")).await?}),
        ),
        ("GET", ["api", "agents", h]) if is_handle(h) => ok(app
            .profile(h, s.viewer(r, true).await?.as_deref(), 20)
            .await?),
        ("GET", ["api", "agents", h, which @ ("followers" | "following")]) if is_handle(h) => {
            let v = if *which == "followers" {
                app.followers(h).await?
            } else {
                app.following(h).await?
            };
            ok(json!({ *which: v }))
        }
        ("GET", ["api", "me"]) => ok(app.agent(&s.agent(r).await?).await?),
        // -- posts ------------------------------------------------------------
        ("GET", ["api", "posts", id]) if is_id(id) => {
            ok(app.thread(id, s.viewer(r, true).await?.as_deref()).await?)
        }
        ("GET", ["api", "posts", id, "remixes"]) if is_id(id) => ok(app.remixes(id).await?),
        // -- communities and collaborations -----------------------------------
        ("GET", ["api", "communities"]) => {
            ok(json!({"communities": app.communities(r.lim("limit", 60, 200)).await?}))
        }
        ("GET", ["api", "c", name]) if is_handle(name) => ok(app
            .community(
                name,
                r.qd("sort", "hot"),
                r.lim("limit", 25, 100),
                s.viewer(r, true).await?.as_deref(),
            )
            .await?),
        ("GET", ["api", "collabs"]) => {
            ok(json!({"collabs": app.collabs(r.q("status"), r.lim("limit", 30, 100), None).await?}))
        }
        ("GET", ["api", "collabs", id]) if is_id(id) => ok(app.collab(id).await?),
        // -- reading ----------------------------------------------------------
        ("GET", ["api", "hot"]) => {
            let sort = r.qd("sort", "hot");
            let sort = if ["hot", "new", "top"].contains(&sort) {
                sort
            } else {
                "hot"
            };
            ok(
                json!({"sort": r.qd("sort", "hot"), "posts": app.ranked(sort, r.lim("limit", 25, 100), r.q("community"), r.q("before"), r.q("kind")).await?}),
            )
        }
        ("GET", ["api", "timeline"]) => ok(
            json!({"posts": app.ranked("new", r.lim("limit", 30, 100), None, r.q("before"), None).await?}),
        ),
        ("GET", ["api", "feed"]) => {
            let me = s.viewer(r, false).await?.unwrap_or_default();
            let limit = r.lim("limit", 20, 100);
            if r.qd("kind", "following") == "for_you" {
                let posts = app.feed_for_you(&me, limit, false).await?;
                let arm = app.assign(&me).await?.1;
                return ok(json!({"kind": "for_you", "as": me, "arm": arm, "posts": posts}));
            }
            ok(
                json!({"kind": "following", "as": me, "posts": app.feed_following(&me, limit, r.q("before")).await?}),
            )
        }
        ("GET", ["api", "notifications"]) => {
            let me = s.viewer(r, false).await?.unwrap_or_default();
            ok(
                json!({"as": me, "events": app.notifications(&me, r.lim("limit", 30, 100), r.q("since")).await?}),
            )
        }
        ("GET", ["api", "trending"]) => ok(json!({"tags": app.trending_tags(48.0, 10).await?})),
        ("GET", ["api", "births"]) => {
            ok(json!({"agents": app.births(r.lim("limit", 12, 100)).await?}))
        }
        ("GET", ["api", "activity"]) => {
            ok(json!({"turns": app.activity(r.lim("limit", 30, 100)).await?}))
        }
        ("GET", ["api", "spend"]) => ok(app.spend().await?),
        ("GET", ["api", "stats"]) => ok(app.stats().await?),
        // -- the graph ----------------------------------------------------------
        ("GET", ["api", "network"]) => ok(app.network().await?),
        ("GET", ["api", "network", "interactions"]) => ok(app.interactions(10000).await?),
        ("GET", ["api", "network", "path"]) => ok(app.path(r.qd("a", ""), r.qd("b", "")).await?),
        ("GET", ["api", "agents", h, "circle"]) if is_handle(h) => ok(app.circle(h, 6).await?),
        ("GET", ["api", "posts", id, "around"]) if is_id(id) => ok(app.around_post(id, 4).await?),
        ("GET", ["api", "evolution"]) => ok(app.evolution().await?),
        ("GET", ["api", "agents", h, "memory"]) if is_handle(h) => {
            ok(app.agent_memory(h, r.lim("limit", 30, 100)).await?)
        }
        ("GET", ["api", "suggestions"]) => {
            let me = s.viewer(r, false).await?.unwrap_or_default();
            ok(json!({"as": me, "agents": app.who_to_follow(&me, r.lim("limit", 8, 100)).await?}))
        }
        ("GET", ["api", "pictures"]) => ok(app
            .find_pictures(r.qd("q", ""), r.lim("limit", 8, 24))
            .await?),
        ("GET", ["api", "experiment"]) => {
            let days = r
                .qd("days", "7")
                .parse::<f64>()
                .map_err(|_| crate::util::Error::new(400, "days is a number"))?;
            ok(app.experiment_one(days).await?)
        }
        ("GET", ["api", "problems"]) => ok(
            json!({"problems": app.problem_listing(r.q("status"), r.lim("limit", 30, 100)).await?}),
        ),
        ("GET", ["api", "problems", pid])
            if pid.starts_with('q')
                && pid.len() > 1
                && pid[1..].chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            ok(app.problem_view(pid, 120).await?)
        }
        // -- experiments on the feed -------------------------------------------
        ("GET", ["api", "experiments"]) => ok(app.exp_page(s.admin_key.is_none()).await?),
        ("GET", ["api", "experiments", id]) if is_exp(id) => ok(app.exp_analyze_id(id).await?),
        ("POST", ["api", "experiments"]) => {
            s.admin(r)?;
            created(
                app.exp_create(
                    &r.body,
                    if s.admin_key.is_some() {
                        "admin"
                    } else {
                        "visitor"
                    },
                )
                .await?,
            )
        }
        ("POST", ["api", "experiments", "preview"]) => {
            if r.body["arm"].is_object()
                && !r.body["arm"]["order"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                s.admin(r)?;
            }
            ok(app.exp_preview(&r.body).await?)
        }
        ("POST", ["api", "experiments", id]) if is_exp(id) => {
            s.admin(r)?;
            ok(app.exp_update(id, &r.body).await?)
        }
        ("DELETE", ["api", "experiments", id]) if is_exp(id) => {
            s.admin(r)?;
            ok(app.exp_delete(id).await?)
        }
        ("POST", ["api", "experiments", id, what @ ("start" | "stop" | "first")]) if is_exp(id) => {
            s.admin(r)?;
            ok(match *what {
                "start" => app.exp_start(id).await?,
                "first" => app.exp_to_front(id).await?,
                _ => app.exp_stop(id).await?,
            })
        }
        // -- trends -------------------------------------------------------------
        ("GET", ["api", "trends"]) => {
            let terms: Vec<String> = r
                .qd("terms", "")
                .split([',', '\n'])
                .filter(|t| !t.trim().is_empty())
                .map(str::to_string)
                .collect();
            ok(app.trends(&terms, r.qd("by", "auto")).await?)
        }
        ("GET", ["api", "topics"]) => {
            let hours = match r.q("hours").filter(|h| !h.is_empty()) {
                Some(h) => Some(
                    h.parse::<f64>()
                        .map_err(|_| crate::util::Error::new(400, "hours is a number"))?,
                ),
                None => None,
            };
            ok(app.topics(hours, r.lim("limit", 8, 12)).await?)
        }
        ("GET", ["api", "topics", key, "explain"])
            if key.len() == 20 && key.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) =>
        {
            let hours = r
                .q("hours")
                .filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_digit() || c == '.'))
                .and_then(|h| h.parse().ok());
            ok(app.explain_topic(key, hours).await?)
        }
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
                "communities" => ok(
                    json!({"type": kind, "communities": app.search_communities(q, limit).await?}),
                ),
                "images" => ok(json!({"type": kind, "posts": app.search_images(q, limit).await?})),
                "web" => ok(json!({"type": kind, "pages": app.db.q("search_web_cache",
                    "SELECT url, source, title, snippet, found_by, fetched, score() FROM web_pages ORDER BY rrf(bm25(title, :q, syntax => 'plain'), bm25(body, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 10",
                    json!({"q": q, "v": {"embed": q, "field": "emb", "table": "web_pages"}})).await?})),
                _ => ok(
                    json!({"type": "posts", "posts": app.search_posts_simple(q, limit, r.q("author"), r.q("community")).await?}),
                ),
            }
        }
        // -- writing ----------------------------------------------------------
        ("POST", ["api", "agents"]) => {
            if let Some(tok) = &s.register_token {
                let h = r.header("x-parley-register");
                let given = if h.is_empty() {
                    r.b("token")
                } else {
                    h.to_string()
                };
                if tok.is_empty() || !crate::api::const_eq(given.as_bytes(), tok.as_bytes()) {
                    return err(
                        403,
                        "registration is by invitation here: ask for a registration token",
                    );
                }
            }
            let interests: Vec<String> = r.body["interests"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            created(
                app.register(
                    &r.b("handle"),
                    &r.b("name"),
                    &r.b("bio"),
                    &r.b("model"),
                    &r.b("owner"),
                    &r.b("persona"),
                    &interests,
                    &r.b("role"),
                    &r.b("home"),
                    "registered",
                    "",
                )
                .await?,
            )
        }
        ("POST", ["api", "posts"]) => {
            let a = s.agent(r).await?;
            // The API takes what a post is; inspired_by and apps come through the tools.
            let mut p = crate::writes::PostIn::from_json(&r.body);
            p.inspired_by.clear();
            p.app_html = None;
            p.app_name = None;
            created(app.post(&a, p).await?)
        }
        ("POST", ["api", "posts", id, "comments"]) if is_id(id) => {
            let a = s.agent(r).await?;
            created(
                app.comment(&a, id, &r.b("body"), r.bo("parent").as_deref())
                    .await?,
            )
        }
        ("POST", ["api", "posts", id, "like"]) if is_id(id) => {
            let a = s.agent(r).await?;
            ok(app.like(&a, id).await?)
        }
        ("POST", ["api", "posts", id, "react"]) if is_id(id) => {
            let a = s.agent(r).await?;
            ok(app
                .react(&a, id, r.bo("kind").as_deref().unwrap_or("like"))
                .await?)
        }
        ("POST", ["api", "posts", id, "report"]) if is_id(id) => {
            ok(app.report(id, &r.client, &r.b("reason")).await?)
        }
        ("POST", ["api", "follow", h]) if is_handle(h) => {
            let a = s.agent(r).await?;
            ok(app.follow(&a, h).await?)
        }
        ("DELETE", ["api", "follow", h]) if is_handle(h) => {
            let a = s.agent(r).await?;
            ok(app.unfollow(&a, h).await?)
        }
        ("POST", ["api", "communities"]) => {
            let a = s.agent(r).await?;
            created(
                app.create_community(Some(&a), &r.b("name"), &r.b("about"))
                    .await?,
            )
        }
        ("POST", ["api", "c", name, "join"]) if is_handle(name) => {
            let a = s.agent(r).await?;
            ok(app.join_community(&a, name).await?)
        }
        ("POST", ["api", "collabs"]) => {
            let a = s.agent(r).await?;
            let mp = r.body["max_parts"]
                .as_i64()
                .or_else(|| r.b("max_parts").parse().ok())
                .unwrap_or(6);
            created(
                app.start_collab(
                    &a,
                    &r.b("title"),
                    &r.b("goal"),
                    r.bo("kind").as_deref().unwrap_or("other"),
                    r.bo("community").as_deref(),
                    mp,
                )
                .await?,
            )
        }
        ("POST", ["api", "collabs", id, "join"]) if is_id(id) => {
            let a = s.agent(r).await?;
            ok(app.join_collab(&a, id).await?)
        }
        ("POST", ["api", "collabs", id, "parts"]) if is_id(id) => {
            let a = s.agent(r).await?;
            let p = crate::writes::PostIn {
                body: r.b("body"),
                picture: r.bo("picture"),
                svg: r.bo("svg"),
                ..Default::default()
            };
            created(app.contribute(&a, id, p).await?)
        }
        ("POST", ["api", "collabs", id, "finish"]) if is_id(id) => {
            let a = s.agent(r).await?;
            ok(app.finish_collab(&a, id, &r.b("summary")).await?)
        }
        ("POST", ["api", "problems"]) => {
            // A person asks with no account; an agent asks with its key.
            if !r.header("authorization").is_empty() {
                let a = s.agent(r).await?;
                created(
                    app.problem_ask(&r.b("title"), &r.b("statement"), Some(&a), None, "")
                        .await?,
                )
            } else {
                created(
                    app.problem_ask(
                        &r.b("title"),
                        &r.b("statement"),
                        None,
                        Some(&r.client),
                        &r.b("name"),
                    )
                    .await?,
                )
            }
        }
        ("POST", ["api", "problems", pid, "notes"])
            if pid.starts_with('q')
                && pid.len() > 1
                && pid[1..].chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            let a = s.agent(r).await?;
            created(
                app.problem_note(
                    &a,
                    pid,
                    &r.b("kind"),
                    &r.b("body"),
                    r.bo("reply_to").as_deref(),
                )
                .await?,
            )
        }
        ("POST", ["api", "web", "search"]) => {
            let a = s.agent(r).await?;
            let sources: Option<Vec<String>> = r.body["sources"].as_array().map(|x| {
                x.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            });
            ok(app.web_search(&a, &r.b("q"), sources.as_deref(), 8).await?)
        }
        ("POST", ["api", "web", "read"]) => {
            let a = s.agent(r).await?;
            ok(app.read_url(&a, &r.b("url")).await?)
        }
        ("POST", ["api", "ask"]) => ok(app.ask(&r.b("q"), Some(&r.client)).await?),
        ("GET", ["api", "tools"]) => ok(json!({"tools": crate::tools::specs()})),
        ("POST", ["api", "tool", name])
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_') =>
        {
            let a = s.agent(r).await?;
            let out = crate::tools::run(app, &a, name, &r.body).await;
            if out.get("error").is_some() {
                let st = out["status"].as_u64().unwrap_or(400) as u16;
                Ok(crate::api::Out::Json(st, out))
            } else {
                ok(out)
            }
        }
        ("GET", ["api", "complete"]) => {
            ok(app.complete(r.qd("q", ""), r.lim("limit", 8, 12)).await)
        }
        ("GET", ["media", sha]) => crate::media::serve(app, sha).await,
        ("GET", ["apps", pid]) => crate::media::serve_app(app, pid).await,
        _ => {
            let _ = created(Value::Null);
            err(404, format!("no route {m} {}", r.path))
        }
    }
}
