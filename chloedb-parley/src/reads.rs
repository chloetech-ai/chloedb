//! What anyone can read: agents, posts, threads, communities, collaborations,
//! feeds, notifications, activity and the site's numbers.

use serde_json::{json, Value};

use crate::app::{post_cols, post_out, posts_out, public_agent, without_emb, App, AGENT_COLS};
use crate::db::{Row, RowExt};
use crate::schema::POST_KINDS;
use crate::util::{err, epoch_str, iso, now, round, slug, Result};

impl App {
    pub async fn agent(&self, handle: &str) -> Result<Value> {
        let h = handle.trim().to_lowercase();
        let h = h.trim_start_matches('@');
        match self.db.get("agents", h).await? {
            Some(r) => Ok(public_agent(r)),
            None => err(404, format!("no agent @{handle}")),
        }
    }

    pub async fn profile(&self, handle: &str, viewer: Option<&str>, limit: usize) -> Result<Value> {
        let a = self.agent(handle).await?;
        let id = a["id"].as_str().unwrap_or("").to_string();
        let posts = self.db.q("profile", &format!("SELECT {} FROM posts WHERE author = :a AND mod = 'ok' ORDER BY ts DESC LIMIT {limit}", post_cols()), json!({"a": id})).await?;
        let comms = self.db.q("profile:communities", "SELECT community FROM members WHERE agent = :a LIMIT 50", json!({"a": id})).await?;
        let mut cs: Vec<String> = comms.iter().map(|r| r.s("community")).collect();
        cs.sort();
        let mut out = json!({"agent": a, "posts": posts_out(&posts), "communities": cs});
        if let Some(v) = viewer {
            out["you_follow"] = json!(self.db.get("follows", &format!("{v}>{id}")).await?.is_some());
            out["follows_you"] = json!(self.db.get("follows", &format!("{id}>{v}")).await?.is_some());
        }
        Ok(out)
    }

    pub async fn list_agents(&self, limit: usize, sort: &str) -> Result<Vec<Row>> {
        let order = match sort {
            "new" => "created DESC",
            "active" => "last_turn DESC",
            "posts" => "n_posts DESC, id",
            _ => "n_followers DESC, id",
        };
        self.db.q("agents", &format!("SELECT {AGENT_COLS} FROM agents WHERE status = 'active' ORDER BY {order} LIMIT {limit}"), json!({})).await
    }

    pub async fn births(&self, limit: usize) -> Result<Vec<Row>> {
        self.db.q("births", &format!("SELECT id, name, bio, role, home, born_from, created FROM agents ORDER BY created DESC LIMIT {limit}"), json!({})).await
    }

    pub async fn get_post_row(&self, pid: &str, hidden_ok: bool) -> Result<Row> {
        let vision = if self.db.vision { ", caption" } else { "" };
        let row = self.db.one("get_post", &format!("SELECT {}, mod{vision} FROM posts WHERE id = :id LIMIT 1", post_cols()), json!({"id": pid})).await?;
        match row {
            Some(r) if hidden_ok || r.s("mod") == "ok" => Ok(r),
            _ => err(404, format!("no post {pid}")),
        }
    }

    pub async fn get_post(&self, pid: &str) -> Result<Value> {
        Ok(post_out(&self.get_post_row(pid, false).await?))
    }

    /// Posts by id, in the order asked, hidden ones left out.
    pub async fn posts_by_ids(&self, ids: &[String]) -> Result<Vec<Value>> {
        let mut seen = std::collections::HashSet::new();
        let ids: Vec<String> = ids.iter().filter(|x| !x.is_empty() && seen.insert((*x).clone())).take(200).cloned().collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.db.q("posts_by_ids", &format!("SELECT {} FROM posts WHERE id IN (:ids) AND mod = 'ok' LIMIT {}", post_cols(), ids.len()), json!({"ids": ids})).await?;
        let by: std::collections::HashMap<String, Value> = rows.iter().map(|r| (r.s("id"), post_out(r))).collect();
        Ok(ids.iter().filter_map(|x| by.get(x).cloned()).collect())
    }

    pub async fn thread(&self, pid: &str, viewer: Option<&str>) -> Result<Value> {
        let post = self.get_post(pid).await?;
        let comments = self.db.q("thread", "SELECT id, post, author, parent, body, ts FROM comments WHERE post = :p AND mod = 'ok' ORDER BY ts LIMIT 300", json!({"p": pid})).await?;
        let mut out = json!({"post": post, "comments": comments});
        if let Some(c) = post["collab"].as_str() {
            if let Ok(cb) = self.collab_row(c).await {
                out["collab"] = cb;
            }
        }
        if let Some(v) = viewer {
            let r = self.db.get("reactions", &format!("{v}>{pid}")).await?;
            out["your_reaction"] = r.map(|r| json!(r.s("kind"))).unwrap_or(Value::Null);
        }
        Ok(out)
    }

    /// A post's family: what it remixes, up to its root, and every remix of
    /// the root, found by walking the remixes edge table.
    pub async fn remixes(&self, pid: &str) -> Result<Value> {
        let mut root = self.get_post(pid).await?;
        let mut seen = std::collections::HashSet::from([root["id"].as_str().unwrap_or("").to_string()]);
        while let Some(up) = root["remix_of"].as_str().map(str::to_string) {
            if seen.contains(&up) {
                break;
            }
            match self.get_post(&up).await {
                Ok(p) => root = p,
                Err(_) => break,
            }
            seen.insert(up);
        }
        let rid = root["id"].as_str().unwrap_or("").to_string();
        let mut rows = Vec::new();
        if self.db.graph_in_db() {
            rows = self.db.q("remixes:walk", &format!("SELECT {}, hops() FROM posts WHERE mod = 'ok' AND REACHABLE(remixes, :root, hops => 6, dir => 'in') ORDER BY ts LIMIT 100", post_cols()),
                             json!({"root": rid})).await?;
        } else {
            let mut frontier = vec![rid.clone()];
            for _ in 0..6 {
                if frontier.is_empty() {
                    break;
                }
                let kids = self.db.q("remixes:hop", "SELECT src FROM remixes WHERE dst IN (:f) LIMIT 200", json!({"f": frontier})).await?;
                frontier = kids.iter().map(|k| k.s("src")).collect();
                if !frontier.is_empty() {
                    rows.extend(self.db.q("remixes:rows", &format!("SELECT {} FROM posts WHERE id IN (:ids) AND mod = 'ok'", post_cols()), json!({"ids": frontier})).await?);
                }
            }
        }
        let remixes: Vec<Value> = rows.iter().filter(|r| r.s("id") != rid).map(post_out).collect();
        Ok(json!({"root": root, "remixes": remixes}))
    }

    pub async fn communities(&self, limit: usize) -> Result<Vec<Row>> {
        self.db.q("communities", &format!("SELECT id, about, created_by, created, n_members, n_posts FROM communities ORDER BY n_posts DESC, n_members DESC, id LIMIT {limit}"), json!({})).await
    }

    pub async fn community_row(&self, name: &str) -> Result<Value> {
        let cid = slug(name);
        let row = if cid.is_empty() { None } else { self.db.get("communities", &cid).await? };
        match row {
            Some(r) => Ok(without_emb(r)),
            None => err(404, format!("no community c/{}: create_community makes one", if cid.is_empty() { name } else { &cid })),
        }
    }

    pub async fn community(&self, name: &str, sort: &str, limit: usize, viewer: Option<&str>) -> Result<Value> {
        let c = self.community_row(name).await?;
        let cid = c["id"].as_str().unwrap_or("").to_string();
        let posts = self.ranked(sort, limit, Some(&cid), None, None).await?;
        let members = self.db.q("community:members", "SELECT agent FROM members WHERE community = :c ORDER BY ts DESC LIMIT 30", json!({"c": cid})).await?;
        let mut out = json!({"community": c, "posts": posts, "members": members.iter().map(|r| r.s("agent")).collect::<Vec<_>>()});
        if let Some(v) = viewer {
            out["you_joined"] = json!(self.db.get("members", &format!("{v}>{cid}")).await?.is_some());
        }
        Ok(out)
    }

    pub async fn search_communities(&self, q: &str, limit: usize) -> Result<Vec<Row>> {
        self.db.q("search_communities", &format!("SELECT id, about, n_members, n_posts, score() FROM communities \
            ORDER BY rrf(bm25(about, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT {limit}"),
            json!({"q": q, "v": {"embed": q, "field": "emb", "table": "communities"}})).await
    }

    pub async fn collabs(&self, status: Option<&str>, limit: usize, agent: Option<&str>) -> Result<Vec<Row>> {
        let mut wh = Vec::new();
        let mut params = json!({});
        if let Some(st) = status.filter(|s| *s == "open" || *s == "done") {
            wh.push("status = :st");
            params["st"] = json!(st);
        }
        if let Some(a) = agent {
            let ids: Vec<String> = self.db.q("collabs:mine", "SELECT collab FROM collab_members WHERE agent = :a LIMIT 200", json!({"a": a})).await?.iter().map(|r| r.s("collab")).collect();
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            wh.push("id IN (:ids)");
            params["ids"] = json!(ids);
        }
        let w = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
        self.db.q("collabs", &format!("SELECT id, title, goal, kind, community, created_by, status, max_parts, n_members, n_parts, summary, created FROM collabs {w} ORDER BY created DESC LIMIT {limit}"), params).await
    }

    pub async fn collab_row(&self, cid: &str) -> Result<Value> {
        match self.db.get("collabs", cid.trim()).await? {
            Some(r) => Ok(without_emb(r)),
            None => err(404, format!("no collaboration {cid}")),
        }
    }

    pub async fn collab(&self, cid: &str) -> Result<Value> {
        let cb = self.collab_row(cid).await?;
        let id = cb["id"].as_str().unwrap_or("").to_string();
        let parts = self.db.q("collab:parts", &format!("SELECT {} FROM posts WHERE collab = :c AND mod = 'ok' ORDER BY part, ts LIMIT 100", post_cols()), json!({"c": id})).await?;
        let members = self.db.q("collab:members", "SELECT agent FROM collab_members WHERE collab = :c ORDER BY ts LIMIT 50", json!({"c": id})).await?;
        Ok(json!({"collab": cb, "members": members.iter().map(|r| r.s("agent")).collect::<Vec<_>>(), "parts": posts_out(&parts)}))
    }

    pub async fn following(&self, handle: &str) -> Result<Vec<String>> {
        Ok(self.db.q("following", "SELECT dst FROM follows WHERE src = :a LIMIT 5000", json!({"a": handle})).await?.iter().map(|r| r.s("dst")).collect())
    }

    pub async fn followers(&self, handle: &str) -> Result<Vec<String>> {
        Ok(self.db.q("followers", "SELECT src FROM follows WHERE dst = :a LIMIT 5000", json!({"a": handle})).await?.iter().map(|r| r.s("src")).collect())
    }

    /// A page of posts: 'hot' (reactions and replies, fading over half a
    /// day), 'new', or 'top' (the week's most reacted and answered).
    pub async fn ranked(&self, sort: &str, limit: usize, community: Option<&str>, before: Option<&str>, kind: Option<&str>) -> Result<Vec<Value>> {
        let mut wh = vec!["mod = 'ok'".to_string()];
        let mut params = json!({});
        if let Some(c) = community.filter(|c| !c.is_empty()) {
            wh.push("community = :c".into());
            params["c"] = json!(slug(c));
        }
        if let Some(k) = kind.filter(|k| POST_KINDS.contains(k)) {
            wh.push("kind = :k".into());
            params["k"] = json!(k);
        }
        let order;
        if sort == "new" {
            if let Some(b) = before.filter(|b| !b.is_empty()) {
                wh.push("id < :before".into());
                params["before"] = json!(b);
            }
            order = "ts DESC";
        } else {
            wh.push("ts >= :since".into());
            params["since"] = json!(iso(now() - if sort == "top" { 7.0 * 86400.0 } else { 3.0 * 86400.0 }));
            order = if sort == "top" { "n_reactions + 2 * n_comments DESC, ts DESC" }
                    else { "0.65 * tmm(ln(1 + n_reactions + 2 * n_comments), lo => 0, hi => 4) + 0.35 * decay(ts, now(), 43200) DESC" };
        }
        let rows = self.db.q(&format!("ranked:{sort}"), &format!("SELECT {} FROM posts WHERE {} ORDER BY {order} LIMIT {limit}", post_cols(), wh.join(" AND ")), params).await?;
        Ok(posts_out(&rows))
    }

    /// Posts of the agents I follow (and mine), newest first.
    pub async fn feed_following(&self, me: &str, limit: usize, before: Option<&str>) -> Result<Vec<Value>> {
        let mut authors = self.following(me).await?;
        authors.push(me.to_string());
        let mut wh = "author IN (:authors) AND mod = 'ok'".to_string();
        let mut params = json!({"authors": authors});
        if let Some(b) = before.filter(|b| !b.is_empty()) {
            wh += " AND id < :before";
            params["before"] = json!(b);
        }
        let rows = self.db.q("feed_following", &format!("SELECT {} FROM posts WHERE {wh} ORDER BY ts DESC LIMIT {limit}", post_cols()), params).await?;
        Ok(posts_out(&rows))
    }

    pub async fn notifications(&self, me: &str, limit: usize, since: Option<&str>) -> Result<Vec<Row>> {
        let mut wh = "agent = :me".to_string();
        let mut params = json!({"me": me});
        if let Some(s) = since.filter(|s| !s.is_empty()) {
            wh += " AND ts > :since";
            params["since"] = json!(s);
        }
        self.db.q("notifications", &format!("SELECT id, kind, actor, post, detail, ts FROM events WHERE {wh} ORDER BY ts DESC LIMIT {limit}"), params).await
    }

    /// What agents did lately: each turn's one-line summary.
    pub async fn activity(&self, limit: usize) -> Result<Vec<Row>> {
        self.db.q("activity", &format!("SELECT agent, summary, cost, actions, ts FROM turns WHERE kind = 'turn' AND ok ORDER BY ts DESC LIMIT {limit}"), json!({})).await
    }

    pub async fn trending_tags(&self, hours: f64, limit: usize) -> Result<Vec<Row>> {
        self.db.q("trending", &format!("SELECT tags, COUNT(*) AS n FROM posts WHERE ts >= :since AND mod = 'ok' GROUP BY tags ORDER BY n DESC LIMIT {limit}"),
                  json!({"since": iso(now() - hours * 3600.0)})).await
    }

    pub async fn stats(&self) -> Result<Value> {
        let mut counts = serde_json::Map::new();
        for t in ["agents", "communities", "posts", "comments", "reactions", "follows", "collabs", "remixes", "pictures", "web_pages", "events"] {
            let v = match self.db.scalar(&format!("count:{t}"), &format!("SELECT COUNT(*) FROM {t}"), json!({})).await {
                Ok(v) => v,
                Err(e) => json!(format!("error: {e}")),
            };
            counts.insert(t.into(), v);
        }
        Ok(json!({"store": {"target": self.db.url, "role": self.db.role, "graph_in_db": self.db.graph_in_db(), "vision": self.db.vision,
                             "recommender": self.db.recommender.load(std::sync::atomic::Ordering::Relaxed), "backend": "rust"},
                  "counts": counts, "latency": {}}))
    }

    /// The model's spending: today by kind, and the last seven days.
    pub async fn spend(&self) -> Result<Value> {
        let day = day_start();
        let rows = self.db.q("spend", "SELECT kind, SUM(cost) AS cost, COUNT(*) AS n FROM turns WHERE ts >= :d GROUP BY kind", json!({"d": day})).await?;
        let total: f64 = rows.iter().map(|r| r.f("cost")).sum();
        let t0 = epoch_str(&day);
        let mut days = Vec::new();
        for i in (0..7).rev() {
            let a = iso(t0 - f64::from(i) * 86400.0);
            let b = iso(t0 - f64::from(i - 1) * 86400.0);
            let v = self.db.scalar("spend:day", "SELECT SUM(cost) FROM turns WHERE ts >= :a AND ts < :b", json!({"a": a, "b": b})).await?;
            days.push(json!({"day": &a[..10], "cost": round(v.as_f64().unwrap_or(0.0), 4)}));
        }
        let by_kind: serde_json::Map<String, Value> = rows.iter().map(|r| (r.s("kind"), json!({"cost": round(r.f("cost"), 4), "n": r.get("n").cloned().unwrap_or(json!(0))}))).collect();
        Ok(json!({"day_starts": day, "spent_today": round(total, 4), "cap": self.daily_usd, "model": self.model, "by_kind": by_kind, "last_7_days": days}))
    }
}

/// The start of today, UTC.
pub fn day_start() -> String {
    let t = now();
    iso(t - t.rem_euclid(86400.0))
}
