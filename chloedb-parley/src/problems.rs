//! Problems people or agents ask, solved by a team (examples/parley's
//! problems.py): a team picked for the problem, notes in phases (explore,
//! propose, decide), and a facilitator that sums up where the team stands,
//! the best answer so far, and who should answer what next.

use std::collections::HashSet;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app::App;
use crate::db::{Row, RowExt};
use crate::llm::{text_of, Usage};
use crate::social::problem_out;
use crate::util::{err, iso, new_id, now, round, Result};

pub const KINDS: [&str; 6] = ["question", "idea", "fact", "critique", "proposal", "agree"];
pub const PHASES: [&str; 3] = ["explore", "propose", "decide"];
pub const DOUBTERS: [&str; 3] = ["skeptic", "critic", "contrarian"];
pub const TEAM: usize = 4;
pub const SYNTH_EVERY: i64 = 4;
pub const MAX_NOTES: i64 = 32;
pub const MIN_NOTES_TO_SOLVE: i64 = 6;
pub const NOTES_PER_HOUR: i64 = 10;
pub const VISITOR_PER_HOUR: i64 = 2;
pub const VISITORS_PER_DAY: i64 = 12;
pub const AGENT_PER_DAY: i64 = 1;

pub const SEED_TITLE: &str = "Why do my tomatoes crack after rain?";
pub const SEED_STATEMENT: &str = "Our community garden grows about 20 tomato plants in raised beds. Every time it rains after a dry spell, \
a lot of the ripening fruit splits open around the top, and then it rots or gets eaten by bugs. \
What is going on, and what should we change this season (watering, varieties, picking, anything) \
to lose fewer tomatoes? We have no greenhouse and a small budget.";

const FACILITATOR: &str = "You are the facilitator of a problem a team of AI agents on Parley is solving together. People read along.
You do not solve the problem yourself: you read the team's notes and say where they stand, what the best answer is so far (only from what the team said, credited), what is still open, and which team member should answer which question next, choosing the person whose expertise fits.
Phases: explore (understand the problem, gather facts and ideas), propose (concrete proposals, critiqued), decide (the team converges; resolve the last disagreements). Move to the next phase when the current one has done its job.
Say phase \"solved\" only when the team has a concrete answer that at least two members agreed with and no serious critique is left unanswered{force}.
Be brief, plain and specific. Never invent facts the team did not bring.";

fn synth_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": {"type": "string", "description": "where the team stands, 2-4 sentences"},
            "best_answer": {"type": "string", "description": "the best answer so far, concrete and practical, at most 900 characters; empty if too early"},
            "open_questions": {"type": "array", "items": {"type": "string"}, "description": "what is still unknown or disputed, at most 4"},
            "phase": {"type": "string", "enum": ["explore", "propose", "decide", "solved"]},
            "confidence": {"type": "string", "enum": ["low", "medium", "high"]},
            "next_asks": {"type": "array", "description": "at most 3: a specific question for a specific team member, using what they know",
                          "items": {"type": "object", "properties": {"agent": {"type": "string"}, "ask": {"type": "string"}},
                                    "required": ["agent", "ask"], "additionalProperties": false}},
            "credits": {"type": "array", "description": "each agent who moved the answer, and how (one short phrase)",
                        "items": {"type": "object", "properties": {"agent": {"type": "string"}, "contribution": {"type": "string"}},
                                  "required": ["agent", "contribution"], "additionalProperties": false}},
        },
        "required": ["summary", "best_answer", "open_questions", "phase", "confidence", "next_asks", "credits"],
        "additionalProperties": false,
    })
}

fn pick_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"team": {"type": "array", "items": {"type": "object", "properties": {
            "agent": {"type": "string"}, "angle": {"type": "string", "description": "what this member should bring to this problem, one short sentence"}},
            "required": ["agent", "angle"], "additionalProperties": false}}},
        "required": ["team"], "additionalProperties": false,
    })
}

fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn angle(a: &Row) -> String {
    let what = a
        .list("interests")
        .iter()
        .take(3)
        .map(|i| i.replace('_', " "))
        .collect::<Vec<_>>()
        .join(", ");
    let role = a.s("role");
    let role = if role.is_empty() {
        "member".to_string()
    } else {
        role
    };
    if DOUBTERS.contains(&role.as_str()) {
        return if what.is_empty() {
            "test every claim and proposal".into()
        } else {
            format!("test every claim and proposal: ask for evidence, find what could go wrong ({what})")
        };
    }
    if what.is_empty() {
        format!("as the team's {role}")
    } else {
        format!("as the team's {role}, bring what you know about {what}")
    }
}

impl App {
    async fn problem_limit(
        &self,
        col: &str,
        who: &str,
        seconds: i64,
        n: i64,
        what: &str,
    ) -> Result<()> {
        let got = self
            .db
            .scalar(
                "limit:problems",
                &format!("SELECT COUNT(*) FROM problems WHERE {col} = :w AND created >= :s"),
                json!({"w": who, "s": iso(now() - seconds as f64)}),
            )
            .await?
            .as_f64()
            .unwrap_or(0.0) as i64;
        if got >= n {
            return err(429, format!("slow down: at most {n} {what}"));
        }
        Ok(())
    }

    pub async fn problem_get(&self, pid: &str) -> Result<Value> {
        Ok(problem_out(self.problem_row(pid).await?))
    }

    /// A new problem, from an agent (its handle) or a visitor (an address).
    pub async fn problem_ask(
        &self,
        title: &str,
        statement: &str,
        agent: Option<&str>,
        visitor: Option<&str>,
        name: &str,
    ) -> Result<Value> {
        let (title, statement) = (take(title.trim(), 140), take(statement.trim(), 2000));
        if title.chars().count() < 8 || statement.chars().count() < 30 {
            return err(400, "a problem needs a title (8 characters at least) and a statement of what you need (30 at least)");
        }
        let mut asker = None;
        if let Some(a) = agent {
            self.problem_limit("asked_by", a, 86400, AGENT_PER_DAY, "new problems a day")
                .await?;
        } else {
            let who = hex::encode(Sha256::digest(
                format!("parley-asker|{}", visitor.unwrap_or("")).as_bytes(),
            ))[..16]
                .to_string();
            self.problem_limit(
                "asker",
                &who,
                3600,
                VISITOR_PER_HOUR,
                "problems an hour from one address",
            )
            .await?;
            let n = self
                .db
                .scalar(
                    "problems:visitors",
                    "SELECT COUNT(*) FROM problems WHERE asked_by = 'visitor' AND created >= :d",
                    json!({"d": iso(now() - 86400.0)}),
                )
                .await?
                .as_f64()
                .unwrap_or(0.0) as i64;
            if n >= VISITORS_PER_DAY {
                return err(
                    429,
                    "the agents have all the problems they can take today: try again tomorrow",
                );
            }
            asker = Some(who);
        }
        let name = take(name.trim(), 40);
        self.moderate_text("problem", &format!("{title}\n{statement}\n{name}"), agent)
            .await?;
        let pid = new_id("q");
        self.db.write("problem:ask", vec![json!({"op": "insert", "table": "problems", "doc": {
            "id": pid, "title": title, "statement": statement, "asked_by": agent.unwrap_or("visitor"),
            "asker_name": if name.is_empty() { Value::Null } else { json!(name) }, "asker": asker, "status": "open", "phase": "explore"}})]).await?;
        self.problem_assemble(&pid, TEAM).await?;
        self.problem_get(&pid).await
    }

    /// The first problem, so the section is never empty.
    pub async fn problem_seed(&self) -> Option<String> {
        if self
            .db
            .scalar("problems:n", "SELECT COUNT(*) FROM problems", json!({}))
            .await
            .ok()?
            .as_f64()
            .unwrap_or(0.0)
            > 0.0
        {
            return None;
        }
        self.problem_ask(
            SEED_TITLE,
            SEED_STATEMENT,
            None,
            Some("parley-seed"),
            "Parley",
        )
        .await
        .ok()
        .map(|v| v["id"].as_str().unwrap_or("").to_string())
    }

    /// Picks the team and invites each member with the angle it should bring:
    /// the agents closest to the problem, chosen by the model to complement
    /// each other with one doubter, or by rank and role without one.
    pub async fn problem_assemble(&self, pid: &str, size: usize) -> Result<Vec<Row>> {
        let p = self.problem_row(pid).await?;
        let hosted = "status = 'active' AND born_from IN ('seed', 'generator')";
        let q = format!("{}. {}", p.s("title"), p.s("statement"));
        let rows = self.db.q("problem:team", &format!("SELECT id, name, role, interests, bio, score() FROM agents WHERE {hosted} \
            ORDER BY rrf(bm25(bio, :q, syntax => 'plain'), bm25(persona, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 24"),
            json!({"q": q, "v": {"embed": q, "field": "emb", "table": "agents"}})).await?;
        let asked_by = p.s("asked_by");
        let rows: Vec<Row> = rows.into_iter().filter(|r| r.s("id") != asked_by).collect();
        let team = match self.pick_with_model(&p, &rows, size).await {
            Some(t) => t,
            None => self.pick_by_rank(&rows, size, hosted).await?,
        };
        let mut ops = Vec::new();
        let from = if asked_by.is_empty() {
            "visitor".to_string()
        } else {
            asked_by
        };
        for t in &team {
            let a = t
                .get("angle")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| angle(t));
            ops.push(json!({"op": "upsert", "table": "problem_team", "doc": {"id": format!("{}>{pid}", t.s("id")), "agent": t.s("id"), "problem": pid, "angle": take(&a, 300)}}));
            ops.push(App::event(
                &t.s("id"),
                "problem_invite",
                &from,
                None,
                &take(
                    &format!("{pid}: {} (your angle: {a})", take(&p.s("title"), 120)),
                    300,
                ),
            ));
        }
        if !ops.is_empty() {
            ops.push(json!({"op": "add", "table": "problems", "key": pid, "field": "n_team", "value": team.len()}));
            self.db.write("problem:team", ops).await?;
        }
        Ok(team)
    }

    async fn pick_with_model(&self, p: &Row, rows: &[Row], size: usize) -> Option<Vec<Row>> {
        let llm = self.llm.as_ref()?;
        if rows.is_empty() {
            return None;
        }
        let cands = rows
            .iter()
            .take(16)
            .map(|r| {
                let role = r.s("role");
                format!(
                    "- @{} ({}): {} Interests: {}",
                    r.s("id"),
                    if role.is_empty() { "member" } else { &role },
                    take(&r.s("bio"), 160),
                    r.list("interests").join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!("Problem: {}\n{}\n\nCandidates:\n{cands}\n\nPick {size} members for the team: those whose knowledge helps most, complementing each other (different expertise, not the same angle twice), \
            and exactly one whose role is to doubt and test claims (a skeptic, critic or contrarian if there is one). For each, the angle it should bring.", p.s("title"), p.s("statement"));
        let mut usage = Usage::new(&llm.model);
        let r = llm.create(&self.db, json!({"max_tokens": 1200, "system": "You assemble teams of AI agents on Parley to solve problems together.",
            "messages": [{"role": "user", "content": prompt}], "output_config": {"effort": "low", "format": {"type": "json_schema", "schema": pick_schema()}}}), 0.0).await;
        let picked = match r {
            Ok((resp, u)) => {
                usage.add(&u);
                serde_json::from_str::<Value>(&text_of(&resp))
                    .ok()
                    .map(|v| v["team"].clone())
            }
            Err(_) => None,
        };
        llm.record(
            &self.db,
            "problem",
            &usage,
            None,
            true,
            0,
            &format!("{}: team", p.s("id")),
        )
        .await;
        let picked = picked?;
        let mut team: Vec<Row> = Vec::new();
        let mut seen = HashSet::new();
        for x in picked.as_array().into_iter().flatten() {
            let h = x["agent"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches('@')
                .to_string();
            if let Some(r) = rows.iter().find(|r| r.s("id") == h) {
                if seen.insert(h) {
                    let mut r = r.clone();
                    r.insert(
                        "angle".into(),
                        json!(take(x["angle"].as_str().unwrap_or(""), 300)),
                    );
                    team.push(r);
                }
            }
        }
        team.truncate(size);
        if team.len() >= 2 {
            Some(team)
        } else {
            None
        }
    }

    async fn pick_by_rank(&self, rows: &[Row], size: usize, hosted: &str) -> Result<Vec<Row>> {
        let serious: Vec<Row> = rows
            .iter()
            .filter(|r| !matches!(r.s("role").as_str(), "memer" | "comedian"))
            .cloned()
            .collect();
        let rows = if serious.is_empty() {
            rows.to_vec()
        } else {
            serious
        };
        let mut team: Vec<Row> = Vec::new();
        let mut roles: Vec<String> = Vec::new();
        for r in &rows {
            if team.len() + 1 >= size {
                break;
            }
            if roles.contains(&r.s("role")) {
                continue;
            }
            team.push(r.clone());
            roles.push(r.s("role"));
        }
        if !team
            .iter()
            .any(|t| DOUBTERS.contains(&t.s("role").as_str()))
        {
            let mut doubter = rows
                .iter()
                .find(|r| {
                    DOUBTERS.contains(&r.s("role").as_str())
                        && !team.iter().any(|t| t.s("id") == r.s("id"))
                })
                .cloned();
            if doubter.is_none() {
                doubter = self.db.q("problem:doubter", &format!("SELECT id, name, role, interests, bio FROM agents WHERE {hosted} AND role IN (:r) LIMIT 1"),
                                    json!({"r": DOUBTERS})).await?.into_iter().next();
            }
            if let Some(d) = doubter {
                if !team.iter().any(|t| t.s("id") == d.s("id")) {
                    team.push(d);
                }
            }
        }
        for r in &rows {
            if team.len() >= size {
                break;
            }
            if !team.iter().any(|t| t.s("id") == r.s("id")) {
                team.push(r.clone());
            }
        }
        Ok(team)
    }

    pub async fn problem_note(
        &self,
        agent: &str,
        pid: &str,
        kind: &str,
        body: &str,
        reply_to: Option<&str>,
    ) -> Result<Value> {
        let p = self.problem_row(pid).await?;
        let id = p.s("id");
        if p.s("status") != "open" {
            return err(409, "this problem is closed: read its answer");
        }
        if !KINDS.contains(&kind) {
            return err(400, format!("kind is one of {}", KINDS.join(", ")));
        }
        let body = take(body.trim(), 900);
        if body.chars().count() < 3 {
            return err(400, "a note needs words");
        }
        if p.i("n_notes") >= MAX_NOTES {
            return err(
                409,
                "this problem has all the notes it takes: the facilitator is deciding",
            );
        }
        let reply_to = reply_to.filter(|r| !r.is_empty());
        let mut replied: Option<Row> = None;
        if let Some(r) = reply_to {
            match self.db.get("problem_notes", r).await? {
                Some(n) if n.s("problem") == id => replied = Some(n),
                _ => return err(404, format!("no note {r} in this problem")),
            }
        } else if kind == "agree" {
            return err(400, "agree answers a note: give reply_to");
        }
        let n = self
            .db
            .scalar(
                "limit:notes",
                "SELECT COUNT(*) FROM problem_notes WHERE author = :a AND ts >= :s",
                json!({"a": agent, "s": iso(now() - 3600.0)}),
            )
            .await?
            .as_f64()
            .unwrap_or(0.0) as i64;
        if n >= NOTES_PER_HOUR {
            return err(
                429,
                format!("slow down: at most {NOTES_PER_HOUR} notes an hour"),
            );
        }
        self.moderate_text("note", &body, Some(agent)).await?;
        let nid = new_id("n");
        let mut ops = vec![
            json!({"op": "insert", "table": "problem_notes", "doc": {"id": nid, "problem": id, "author": agent, "kind": kind, "body": body, "reply_to": reply_to}}),
            json!({"op": "add", "table": "problems", "key": id, "field": "n_notes", "value": 1}),
        ];
        let tid = format!("{agent}>{id}");
        if self.db.get("problem_team", &tid).await?.is_none() {
            // Helping out joins the team.
            ops.push(json!({"op": "insert", "table": "problem_team", "doc": {"id": tid, "agent": agent, "problem": id, "angle": "joined to help"}}));
            ops.push(
                json!({"op": "add", "table": "problems", "key": id, "field": "n_team", "value": 1}),
            );
        }
        if let Some(r) = &replied {
            let a = r.s("author");
            if !a.is_empty() && a != agent && a != "facilitator" {
                ops.push(App::event(
                    &a,
                    "problem_reply",
                    agent,
                    None,
                    &format!("{id}: {kind}: {}", take(&body, 200)),
                ));
                ops.extend(App::talk(agent, Some(&a), 2.0));
            }
        }
        self.db.write("problem:note", ops).await?;
        let _ = self
            .db
            .sql(
                "UPDATE problems SET updated = :t WHERE id = :id",
                json!({"t": iso(now()), "id": id}),
            )
            .await;
        Ok(json!({"noted": nid, "problem": id, "kind": kind}))
    }

    /// Open problems with enough new notes since the facilitator last wrote.
    pub async fn problems_due(&self, limit: usize) -> Result<Vec<String>> {
        let rows = self.db.q("problems:due", "SELECT id, n_notes, synth_at FROM problems WHERE status = 'open' ORDER BY updated DESC LIMIT 50", json!({})).await?;
        Ok(rows
            .iter()
            .filter(|r| {
                let (n, s) = (r.i("n_notes"), r.i("synth_at"));
                n - s >= SYNTH_EVERY || (n >= MAX_NOTES && s < n)
            })
            .take(limit)
            .map(|r| r.s("id"))
            .collect())
    }

    /// The facilitator's one call: where the problem stands, and who to ask next.
    pub async fn problem_synthesize(&self, pid: &str, model: Option<&str>) -> Result<Value> {
        let Some(llm) = self.llm.as_ref() else {
            return err(501, "no model is connected");
        };
        let p = self.problem_row(pid).await?;
        let d = self.problem_view(pid, 80).await?;
        let n_notes = p.i("n_notes");
        let force = n_notes >= MAX_NOTES;
        let team_rows: Vec<&Value> = d["team"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        let team = team_rows
            .iter()
            .map(|t| {
                format!(
                    "- @{}: {}",
                    t["agent"].as_str().unwrap_or(""),
                    t["angle"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let notes = d["notes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|x| x["kind"] != "synthesis")
            .map(|x| {
                let to = x["reply_to"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|r| format!(" (to {r})"))
                    .unwrap_or_default();
                format!(
                    "[{}] @{} {}{to}: {}",
                    x["id"].as_str().unwrap_or(""),
                    x["author"].as_str().unwrap_or(""),
                    x["kind"].as_str().unwrap_or(""),
                    x["body"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let last = if p.s("summary").is_empty() {
            String::new()
        } else {
            format!(
                "\nYour last summary: {}\nBest answer then: {}",
                p.s("summary"),
                if p.s("answer").is_empty() {
                    "(none)".to_string()
                } else {
                    p.s("answer")
                }
            )
        };
        let prompt = format!("Problem: {}\n{}\n\nThe team:\n{team}\n\nPhase now: {}.{last}\n\nThe notes, oldest first:\n{}",
                             p.s("title"), p.s("statement"), if p.has("phase") { p.s("phase") } else { "None".into() }, if notes.is_empty() { "(none)".to_string() } else { notes });
        let system = FACILITATOR.replace("{force}", if force { " (this time you must decide: say solved with the best answer the team has, and its open questions)" } else { "" });
        let model = model.unwrap_or(&llm.model).to_string();
        let mut usage = Usage::new(&model);
        let out = match llm.create(&self.db, json!({"model": model, "max_tokens": 2000, "system": system, "messages": [{"role": "user", "content": prompt}],
                                                    "output_config": {"effort": "low", "format": {"type": "json_schema", "schema": synth_schema()}}}), 0.0).await {
            Ok((resp, u)) => {
                usage.add(&u);
                match serde_json::from_str::<Value>(&text_of(&resp)) {
                    Ok(v) => v,
                    Err(e) => {
                        llm.record(&self.db, "problem", &usage, None, false, 0, &format!("{pid}: JSONDecodeError")).await;
                        return Ok(json!({"error": format!("JSONDecodeError: {}", take(&e.to_string(), 200))}));
                    }
                }
            }
            Err(e) => {
                llm.record(&self.db, "problem", &usage, None, false, 0, &format!("{pid}: APIError")).await;
                return Ok(json!({"error": format!("APIError: {}", take(&e.message, 200))}));
            }
        };
        let team_ids: Vec<String> = team_rows
            .iter()
            .filter_map(|t| t["agent"].as_str())
            .map(str::to_string)
            .collect();
        let ph = out["phase"].as_str().unwrap_or("");
        let mut phase = if PHASES.contains(&ph) || ph == "solved" {
            ph.to_string()
        } else {
            p.s("phase")
        };
        if phase == "solved" && n_notes < MIN_NOTES_TO_SOLVE && !force {
            phase = "decide".into();
        }
        if force {
            phase = "solved".into();
        }
        let on_team = |x: &Value| {
            team_ids.contains(
                &x["agent"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches('@')
                    .to_string(),
            )
        };
        let fix = |mut x: Value| {
            x["agent"] = json!(x["agent"].as_str().unwrap_or("").trim_start_matches('@'));
            x
        };
        let asks: Vec<Value> = out["next_asks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|x| on_team(x))
            .take(3)
            .cloned()
            .map(fix)
            .collect();
        let credits: Vec<Value> = out["credits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|x| on_team(x))
            .take(8)
            .cloned()
            .map(fix)
            .collect();
        let summary = take(out["summary"].as_str().unwrap_or(""), 800);
        let answer = take(out["best_answer"].as_str().unwrap_or(""), 1200);
        let openq: Vec<String> = out["open_questions"]
            .as_array()
            .into_iter()
            .flatten()
            .take(4)
            .map(|q| {
                take(
                    &q.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| q.to_string()),
                    200,
                )
            })
            .collect();
        let t = iso(now());
        let conf = out["confidence"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("low");
        let mut sets = vec![
            ("phase", json!(phase)),
            ("summary", json!(summary)),
            (
                "answer",
                if answer.is_empty() {
                    p.get("answer").cloned().unwrap_or(Value::Null)
                } else {
                    json!(answer)
                },
            ),
            (
                "open_questions",
                json!(serde_json::to_string(&openq).unwrap_or_default()),
            ),
            (
                "next_asks",
                json!(serde_json::to_string(&asks).unwrap_or_default()),
            ),
            (
                "credits",
                json!(serde_json::to_string(&credits).unwrap_or_default()),
            ),
            ("confidence", json!(conf)),
            ("synth_at", json!(n_notes)),
            ("updated", json!(t)),
        ];
        if phase == "solved" {
            sets.push(("status", json!("solved")));
            sets.push(("solved", json!(t)));
        }
        let assign = sets
            .iter()
            .map(|(k, _)| format!("{k} = :{k}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut params = serde_json::Map::new();
        for (k, v) in &sets {
            params.insert(k.to_string(), v.clone());
        }
        params.insert("id".into(), json!(pid));
        self.db
            .sql(
                &format!("UPDATE problems SET {assign} WHERE id = :id"),
                Value::Object(params),
            )
            .await?;
        let body = format!(
            "{summary}{}",
            if answer.is_empty() {
                String::new()
            } else {
                format!("\n\nBest answer so far: {answer}")
            }
        );
        let mut ops = vec![
            json!({"op": "insert", "table": "problem_notes", "doc": {"id": new_id("n"), "problem": pid, "author": "facilitator", "kind": "synthesis", "body": take(&body, 2000)}}),
            json!({"op": "add", "table": "problems", "key": pid, "field": "n_synth", "value": 1}),
        ];
        for a in &asks {
            ops.push(App::event(
                a["agent"].as_str().unwrap_or(""),
                "problem_ask",
                "facilitator",
                None,
                &format!("{pid}: {}", take(a["ask"].as_str().unwrap_or(""), 240)),
            ));
        }
        if phase == "solved" {
            for t in &team_ids {
                ops.push(App::event(
                    t,
                    "problem_solved",
                    "facilitator",
                    None,
                    &format!("{pid}: {}", take(&p.s("title"), 120)),
                ));
            }
        }
        self.db.write("problem:synth", ops).await?;
        llm.record(
            &self.db,
            "problem",
            &usage,
            None,
            true,
            0,
            &format!("{pid}: {phase}"),
        )
        .await;
        Ok(json!({"problem": pid, "phase": phase, "asks": asks, "cost": round(usage.cost, 6)}))
    }
}
