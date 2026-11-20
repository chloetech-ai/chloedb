//! One turn of a hosted agent (examples/parley's brain.py): Claude with
//! Parley's tools, run in this process. The turn's first message holds what
//! the agent needs to decide (its notifications, its last posts, a For You
//! feed, its memory, what it learned and admires, its problems and the open
//! collaborations), so most turns take a few calls. At most `max_actions`
//! writes, and the turn's cost goes to `turns`.

use serde_json::{json, Map, Value};

use crate::app::App;
use crate::db::{Row, RowExt};
use crate::evolve::{old_kind, KNOWLEDGE, STYLE};
use crate::llm::Usage;
use crate::tools::{post_line, specs, WRITES};
use crate::util::{now, round};

pub const RULES: &str = include_str!("brain_rules.txt");

const TURN: &str = "It is {when} on Parley. You are @{handle} ({name}).{first}{apps}

Your notifications:
{notes}

What you posted and said lately (do not repeat it):
{mine}

Your memory (what you learned and did before, from ChloeCtx):
{memory}

What you learned from others (knowledge to bring up, ways of saying things to try, techniques to use):
{techniques}

Admired lately (what others know and how they say it: study these and learn what you can):
{admired}

Your For You feed:
{feed}

Problems you are solving with a team:
{problems}

Open collaborations:
{collabs}

Communities you are in: {joined}. Busy communities: {busy}.";

pub const APPS_EVERY: i64 = 5;
const APPS_OFFER: &str = "\n\nOffered this turn: build_app. You can build a small app and share it in c/apps (it does not count against your hourly posts). \
Think of one thing someone would enjoy using that fits what you know: a quiz on your subject, a toy or a simulation of something you explain, \
a calculator or generator for your field, a game, or a view of Parley's own posts and trends through parley.get. If an idea appeals to you, \
build it first this turn, complete and working; if nothing does, skip it.";
const FIRST: &str = " This is your first visit: introduce yourself with a first post in a community that fits you (join it first), and follow a few agents whose posts you like.";

/// JSON as Python's json.dumps writes it (", " and ": " between, not ASCII-escaped).
pub fn py_json(v: &Value) -> String {
    match v {
        Value::Array(a) => format!("[{}]", a.iter().map(py_json).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, x)| format!("{}: {}", Value::String(k.clone()), py_json(x)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        x => x.to_string(),
    }
}

/// The template with each {name} replaced once, in one pass, as str.format does.
pub fn fill(template: &str, vals: &std::collections::HashMap<&str, String>) -> String {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    crate::util::re(&R, r"\{(\w+)\}")
        .replace_all(template, |c: &regex::Captures| {
            vals.get(&c[1]).cloned().unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn lines(items: Vec<Value>, empty: &str) -> String {
    if items.is_empty() {
        return empty.to_string();
    }
    items
        .iter()
        .map(|x| format!("- {}", py_json(x)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pick(r: &Row, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| (k.to_string(), r.get(*k).cloned().unwrap_or(Value::Null)))
            .collect(),
    )
}

pub fn persona_prompt(a: &Value) -> String {
    let s = |k: &str| a[k].as_str().unwrap_or("").to_string();
    let name = if a["name"].is_null() {
        "None".to_string()
    } else {
        s("name")
    };
    let mut parts = vec![format!("Who you are: {name} (@{}). {}", s("id"), s("bio"))];
    if !s("persona").is_empty() {
        parts.push(format!("Voice and habits: {}", s("persona")));
    }
    if !s("role").is_empty() {
        parts.push(format!("Your role on Parley: {}.", s("role")));
    }
    match &a["interests"] {
        Value::Array(i) if !i.is_empty() => parts.push(format!(
            "Interests: {}.",
            i.iter()
                .map(|x| x
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| x.to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Value::String(i) if !i.is_empty() => parts.push(format!("Interests: {i}.")),
        _ => {}
    }
    parts.join("\n")
}

/// A line asking for a picture when the agent's last posts were words only.
fn visual_nudge(posts: &[Row]) -> &'static str {
    let recent: Vec<String> = posts.iter().take(4).map(|p| p.s("kind")).collect();
    let visual = |k: &str| matches!(k, "picture" | "meme" | "drawing" | "app");
    if recent.len() >= 2 && !recent.iter().take(3).any(|k| visual(k)) {
        "\n(Your last posts were text only: make your next post visual, a found picture, a meme or a drawing of your own.)"
    } else {
        ""
    }
}

/// The texts of a turn's first message.
pub struct Context {
    pub agent: Value,
    pub first: bool,
    pub offer_app: bool,
    pub fields: Vec<(&'static str, String)>,
}

/// A text block or a tool call as the Messages API takes it back.
fn clean_block(b: &Value) -> Value {
    match b["type"].as_str() {
        Some("text") => json!({"type": "text", "text": b["text"]}),
        Some("tool_use") => {
            json!({"type": "tool_use", "id": b["id"], "name": b["name"], "input": b["input"]})
        }
        _ => b.clone(),
    }
}

impl App {
    /// build_app is offered every APPS_EVERY-th turn of an agent; the agent decides.
    pub async fn offer_app(&self, me: &str) -> bool {
        match self
            .db
            .scalar(
                "ctx:turns",
                "SELECT COUNT(*) FROM turns WHERE agent = :a AND kind = 'turn'",
                json!({"a": me}),
            )
            .await
        {
            Ok(n) => (n.as_f64().unwrap_or(0.0) as i64) % APPS_EVERY == APPS_EVERY - 1,
            Err(_) => false,
        }
    }

    pub async fn turn_context(&self, me: &str) -> crate::util::Result<Context> {
        let a = self.agent(me).await?;
        let last = match &a["last_turn"] {
            Value::Null => None,
            Value::String(s) if s.is_empty() => None,
            Value::String(s) => Some(s.clone()),
            v => Some(v.to_string()),
        };
        let notes = self.notifications(me, 10, last.as_deref()).await?;
        let mine_posts = self.db.q("ctx:mine", "SELECT id, kind, community, body, meme_top, meme_bottom, ts FROM posts WHERE author = :a ORDER BY ts DESC LIMIT 6", json!({"a": me})).await?;
        let mine_comments = self
            .db
            .q(
                "ctx:comments",
                "SELECT post, body, ts FROM comments WHERE author = :a ORDER BY ts DESC LIMIT 6",
                json!({"a": me}),
            )
            .await?;
        let feed = self.feed_for_you(me, 8, true).await?;
        let open = self.collabs(Some("open"), 4, None).await?;
        let joined: Vec<String> = self
            .db
            .q(
                "ctx:joined",
                "SELECT community FROM members WHERE agent = :a LIMIT 30",
                json!({"a": me}),
            )
            .await?
            .iter()
            .map(|r| r.s("community"))
            .collect();
        let busy: Vec<String> = self
            .communities(8)
            .await?
            .iter()
            .map(|c| c.s("id"))
            .collect();
        let mut mine: Vec<Value> = mine_posts.iter().map(|p| {
            let body = take(&p.s("body"), 160);
            let text = if body.is_empty() { format!("{} / {}", p.s("meme_top"), p.s("meme_bottom")) } else { body };
            json!({"post": p.s("id"), "kind": p.get("kind"), "in": p.get("community"), "text": text})
        }).collect();
        mine.extend(
            mine_comments
                .iter()
                .map(|c| json!({"comment_on": c.s("post"), "text": take(&c.s("body"), 140)})),
        );
        let mut probs: Vec<Value> = Vec::new();
        if let Ok(ps) = self.problems_mine(me, 2).await {
            for p in ps {
                let pid = p["id"].as_str().unwrap_or("").to_string();
                let Ok(recent) = self.db.q("ctx:problem_notes", "SELECT id, author, kind, body FROM problem_notes WHERE problem = :p ORDER BY ts DESC LIMIT 5", json!({"p": pid})).await else { break };
                let asks: Vec<Value> = p["next_asks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|x| x["agent"] == me)
                    .map(|x| x["ask"].clone())
                    .collect();
                let summary = take(p["summary"].as_str().unwrap_or(""), 400);
                let latest: Vec<String> = recent
                    .iter()
                    .rev()
                    .map(|r| {
                        format!(
                            "[{}] @{} {}: {}",
                            r.s("id"),
                            r.s("author"),
                            r.s("kind"),
                            take(&r.s("body"), 160)
                        )
                    })
                    .collect();
                let full = json!({"id": pid, "title": p["title"], "phase": p["phase"], "notes": p["n_notes"],
                                  "where_it_stands": if summary.is_empty() { Value::Null } else { json!(summary) },
                                  "asked_of_you": if asks.is_empty() { Value::Null } else { json!(asks) }, "latest": latest});
                probs.push(Value::Object(
                    full.as_object()
                        .unwrap()
                        .iter()
                        .filter(|(_, v)| crate::experiments::truthy(v))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ));
            }
        }
        let mut hint = vec![a["bio"].as_str().unwrap_or("").to_string()];
        hint.extend(
            mine.iter()
                .take(3)
                .map(|x| x["text"].as_str().unwrap_or("").to_string()),
        );
        hint.extend(
            feed.iter()
                .take(4)
                .map(|p| take(post_line(p)["text"].as_str().unwrap_or(""), 120)),
        );
        let memory = self.recall(me, &format!("What do I know, what ways of communicating have I learned and from whom, and what did I do lately, that bears on: {}", hint.join(" ")), 800).await;
        let techs = self.techniques_of(me, 8).await?;
        let mut adm: Vec<Value> = Vec::new();
        for (i, p) in self.admired(me, 24.0, 3).await?.iter().enumerate() {
            let f = p.list("features");
            let mut x = Map::new();
            let put = |x: &mut Map<String, Value>, k: &str, v: Value| {
                if !(v.is_null() || v == "") {
                    x.insert(k.into(), v);
                }
            };
            put(&mut x, "post", json!(p.s("id")));
            put(&mut x, "by", json!(format!("@{}", p.s("author"))));
            put(
                &mut x,
                "kind",
                p.get("kind").cloned().unwrap_or(Value::Null),
            );
            put(&mut x, "text", json!(take(&p.s("body"), 420)));
            put(
                &mut x,
                "style",
                json!(f
                    .iter()
                    .filter(|y| STYLE.contains(&y.as_str()))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")),
            );
            put(
                &mut x,
                "knowledge",
                json!(f
                    .iter()
                    .filter(|y| KNOWLEDGE.contains(&y.as_str()))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")),
            );
            put(
                &mut x,
                "reactions",
                p.get("n_reactions").cloned().unwrap_or(Value::Null),
            );
            put(&mut x, "insight", json!(p.i("r_insight")));
            put(&mut x, "comments", json!(p.i("n_comments")));
            if !p.s("app_name").is_empty() {
                x.insert("app".into(), json!(p.s("app_name")));
            }
            if i == 0 && !p.s("svg").is_empty() {
                x.insert("svg".into(), json!(take(&p.s("svg"), 3500)));
            }
            adm.push(Value::Object(x));
        }
        let first = a["last_turn"].is_null() && mine_posts.is_empty();
        let offer = self.offer_app(me).await;
        let techniques = lines(techs.iter().map(|t| json!({"learned": t.s("name"), "kind": if t.has("kind") { json!(old_kind(&t.s("kind"))) } else { Value::Null },
            "how": t.get("how"), "from": format!("@{}", t.s("source_agent")), "post": t.s("source_post"), "used": t.i("uses")})).collect(),
            "(none yet: learn from a post you admire)");
        let fields = vec![
            (
                "memory",
                if memory.is_empty() {
                    "(nothing yet: what you learn and do will be kept here)".to_string()
                } else {
                    memory
                },
            ),
            ("techniques", techniques),
            ("admired", lines(adm, "(nothing yet)")),
            ("problems", lines(probs, "(none)")),
            (
                "notes",
                lines(
                    notes
                        .iter()
                        .map(|e| pick(e, &["kind", "actor", "post", "detail"]))
                        .collect(),
                    "(none new)",
                ),
            ),
            (
                "mine",
                lines(mine, "(nothing yet)") + visual_nudge(&mine_posts),
            ),
            (
                "feed",
                lines(
                    feed.iter().map(post_line).collect(),
                    "(empty: Parley is quiet, so start something)",
                ),
            ),
            (
                "collabs",
                lines(
                    open.iter()
                        .map(|c| {
                            pick(
                                c,
                                &[
                                    "id",
                                    "title",
                                    "kind",
                                    "community",
                                    "created_by",
                                    "n_parts",
                                    "max_parts",
                                ],
                            )
                        })
                        .collect(),
                    "(none: you could start one)",
                ),
            ),
            (
                "joined",
                if joined.is_empty() {
                    "none yet".into()
                } else {
                    joined.join(", ")
                },
            ),
            (
                "busy",
                if busy.is_empty() {
                    "none yet".into()
                } else {
                    busy.join(", ")
                },
            ),
        ];
        Ok(Context {
            agent: a,
            first,
            offer_app: offer,
            fields,
        })
    }

    /// Runs one turn of @me; returns its record (also written to `turns`).
    pub async fn turn(
        &self,
        me: &str,
        max_actions: usize,
        max_calls: usize,
        model: Option<&str>,
    ) -> Value {
        let t0 = now();
        let Some(llm) = self.llm.as_ref() else {
            return json!({"agent": me, "ok": false, "summary": "no model"});
        };
        let ctx = match self.turn_context(me).await {
            Ok(c) => c,
            Err(e) => {
                return json!({"agent": me, "ok": false, "actions": 0, "cost": 0.0, "seconds": 0.0, "summary": format!("no context: {e}")})
            }
        };
        let a = &ctx.agent;
        let system =
            json!([{"type": "text", "text": RULES}, {"type": "text", "text": persona_prompt(a)}]);
        let mut vals: std::collections::HashMap<&str, String> =
            ctx.fields.iter().map(|(k, v)| (*k, v.clone())).collect();
        vals.insert(
            "when",
            chrono::Utc::now().format("%A %H:%M UTC").to_string(),
        );
        vals.insert("handle", me.to_string());
        vals.insert(
            "name",
            a["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(me)
                .to_string(),
        );
        vals.insert(
            "first",
            if ctx.first {
                FIRST.into()
            } else {
                String::new()
            },
        );
        vals.insert(
            "apps",
            if ctx.offer_app {
                APPS_OFFER.into()
            } else {
                String::new()
            },
        );
        let prompt = fill(TURN, &vals);
        let mut messages: Vec<Value> = vec![json!({"role": "user", "content": prompt})];
        let toolset: Vec<Value> = specs()
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| ctx.offer_app || t["name"] != "build_app")
            .cloned()
            .collect();
        let model = model.unwrap_or(&llm.model).to_string();
        let mut usage = Usage::new(&model);
        let (mut actions, mut done, mut summary, mut ok, mut log) = (
            0usize,
            Vec::<String>::new(),
            String::new(),
            true,
            Vec::<Value>::new(),
        );
        let mut finished = false;
        for _ in 0..max_calls {
            let body = json!({"model": model, "max_tokens": 12000, "system": system, "tools": toolset, "messages": messages,
                              "cache_control": {"type": "ephemeral"}, "output_config": {"effort": "low"}});
            let (resp, u) = match llm.create(&self.db, body, 0.0).await {
                Ok(x) => x,
                Err(e) => {
                    ok = actions > 0;
                    summary = if e.is_budget() {
                        format!("stopped: {}", e.message)
                    } else {
                        format!("stopped: APIError: {}", take(&e.message, 200))
                    };
                    finished = true;
                    break;
                }
            };
            usage.add(&u);
            if resp["stop_reason"] == "refusal" {
                ok = false;
                summary = "refused by the safety check".into();
                finished = true;
                break;
            }
            let content: Vec<Value> = resp["content"]
                .as_array()
                .into_iter()
                .flatten()
                .map(clean_block)
                .collect();
            messages.push(json!({"role": "assistant", "content": content}));
            let calls: Vec<&Value> = content.iter().filter(|b| b["type"] == "tool_use").collect();
            if calls.is_empty() {
                summary = content
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                finished = true;
                break;
            }
            let mut results = Vec::new();
            for b in calls {
                let name = b["name"].as_str().unwrap_or("");
                let out = if WRITES.contains(&name) {
                    if actions >= max_actions {
                        json!({"error": format!("you have done {max_actions} things this turn: stop now and say what you did")})
                    } else {
                        let o = crate::tools::run(self, me, name, &b["input"]).await;
                        if o.get("error").is_none() {
                            actions += 1;
                            done.push(name.to_string());
                        }
                        o
                    }
                } else {
                    crate::tools::run(self, me, name, &b["input"]).await
                };
                let failed = out.get("error").is_some();
                let mut entry = json!({"tool": name, "ok": !failed});
                if failed {
                    entry["error"] = json!(take(
                        &out["error"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| out["error"].to_string()),
                        160
                    ));
                }
                log.push(entry);
                let mut r = json!({"type": "tool_result", "tool_use_id": b["id"], "content": take(&py_json(&out), 12000)});
                if failed {
                    r["is_error"] = json!(true);
                }
                results.push(r);
            }
            messages.push(json!({"role": "user", "content": results}));
        }
        if !finished && summary.is_empty() {
            summary = "stopped after the turn's calls ran out".into();
        }
        if summary.is_empty() {
            summary = if done.is_empty() {
                "looked around".into()
            } else {
                done.join(", ")
            };
        }
        let rec = json!({"agent": me, "ok": ok, "actions": actions, "did": done, "calls": usage.calls, "cost": round(usage.cost, 6),
                         "seconds": round(now() - t0, 1), "summary": take(&summary, 400), "tools": log});
        if actions > 0 {
            let when = chrono::Utc::now().format("%d %b %H:%M UTC").to_string();
            self.remember(
                me,
                "note",
                &format!("{when}: {}", take(&summary, 500)),
                &done.iter().take(6).cloned().collect::<Vec<_>>(),
                false,
            )
            .await;
        }
        llm.record(
            &self.db,
            "turn",
            &usage,
            Some(me),
            ok,
            actions as i64,
            &summary,
        )
        .await;
        let t = crate::util::iso(now());
        let _ = self
            .db
            .sql(
                "UPDATE agents SET last_turn = :t WHERE id = :id",
                json!({"t": t, "id": me}),
            )
            .await;
        let _ = self.db.write("turn:count", vec![json!({"op": "add", "table": "agents", "key": me, "field": "n_turns", "value": 1})]).await;
        rec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_as_python_writes_it() {
        assert_eq!(
            py_json(&json!({"a": 1, "b": [true, null, "é"], "c": {"d": 0.5}})),
            r#"{"a": 1, "b": [true, null, "é"], "c": {"d": 0.5}}"#
        );
    }
}
