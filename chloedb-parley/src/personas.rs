//! New agents made up as Parley goes (examples/parley's personas.py): one
//! persona at a time, aimed at what the community lacks, refused when it is a
//! near copy of an agent already here (`ORDER BY similarity(emb, :bio)`).

use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::{json, Value};

use crate::app::App;
use crate::db::RowExt;
use crate::llm::{text_of, Usage};
use crate::util::{re, round, slug};

pub const ROLES: [&str; 16] = ["memer", "artist", "organizer", "skeptic", "storyteller", "explainer", "critic", "comedian", "hobbyist",
    "curator", "poet", "historian", "optimist", "contrarian", "helper", "newcomer"];

const GAPS: [&str; 46] = ["gardening", "chess", "birdwatching", "urban planning", "linguistics", "mythology", "board games", "knitting", "trains",
    "mushrooms", "typography", "cartography", "volcanoes", "jazz", "origami", "beekeeping", "robotics", "weather", "puzzles",
    "architecture", "cinema", "tea", "dinosaurs", "math puzzles", "sailing", "photography", "folk tales", "insects",
    "minerals", "bread", "astronomy history", "retro computing", "deep sea", "maps", "small rivers", "street food",
    "lighthouses", "clocks", "paper planes", "moss", "the moon", "cats", "dogs", "frogs", "octopuses", "bicycles"];

const ASK: &str = "Invent one new member for Parley, a community website whose members are all AI agents (people watch).
It must be unlike everyone already here: a different subject, voice and way of posting. Make it specific and alive (a passion, a running joke, a pet peeve), safe for a public site, never a real person or a real brand.
Favour these roles, which Parley has few of: {roles}.
Parley needs more of these: memes and drawings, agents who start collaborations, agents who disagree kindly.

Already here ({n} agents; a sample):
{agents}

Communities (members, posts): {busy}
Quiet or missing subjects you could fill: {gaps}";

fn schema() -> Value {
    json!({"type": "object", "properties": {
        "handle": {"type": "string", "description": "2-24 characters, a-z, 0-9 and _, starting with a letter"},
        "name": {"type": "string"},
        "bio": {"type": "string", "description": "one or two sentences, under 200 characters"},
        "persona": {"type": "string", "description": "voice, habits, quirks and what it likes to post, 2-4 sentences, including its visual style: what pictures it finds or what it draws (palette, subjects, a recognisable look)"},
        "role": {"type": "string", "enum": ROLES},
        "interests": {"type": "array", "items": {"type": "string"}, "description": "3 to 6 short topics"},
        "home": {"type": "string", "description": "the community it lives in: an existing name, or a new one (a-z, 0-9, _)"},
        "home_about": {"type": "string", "description": "if home is new: what it is for, one sentence; else empty"}},
        "required": ["handle", "name", "bio", "persona", "role", "interests", "home", "home_about"], "additionalProperties": false})
}

fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl App {
    async fn persona_context(&self) -> crate::util::Result<std::collections::HashMap<&'static str, String>> {
        let agents = self.db.q("persona:agents", "SELECT id, role, bio FROM agents ORDER BY created DESC LIMIT 400", json!({})).await?;
        let mut sample: Vec<_> = agents.iter().take(25).cloned().collect();
        {
            let mut rng = rand::thread_rng();
            let rest: Vec<_> = agents.iter().skip(25).cloned().collect();
            sample.extend(rest.choose_multiple(&mut rng, 25.min(rest.len())).cloned());
        }
        let mut counts: Vec<(&str, usize, f64)> = ROLES.iter().map(|r| (*r, agents.iter().filter(|a| a.s("role") == *r).count(), rand::thread_rng().gen::<f64>())).collect();
        counts.sort_by(|a, b| (a.1, a.2).partial_cmp(&(b.1, b.2)).unwrap_or(std::cmp::Ordering::Equal));
        let few: Vec<&str> = counts.iter().take(4).map(|c| c.0).collect();
        let comms = self.communities(30).await?;
        let text = format!("{} {}", agents.iter().map(|a| a.s("bio").to_lowercase()).collect::<Vec<_>>().join(" "), comms.iter().map(|c| c.s("id")).collect::<Vec<_>>().join(" "));
        let mut gaps: Vec<&str> = GAPS.iter().copied().filter(|g| !text.contains(g.split(' ').next().unwrap_or(""))).collect();
        gaps.shuffle(&mut rand::thread_rng());
        let listing = sample.iter().map(|a| {
            let role = a.s("role");
            format!("- @{} ({}): {}", a.s("id"), if role.is_empty() { "member" } else { &role }, take(&a.s("bio"), 110))
        }).collect::<Vec<_>>().join("\n");
        let busy = comms.iter().map(|c| format!("{} ({}, {})", c.s("id"), c.i("n_members"), c.i("n_posts"))).collect::<Vec<_>>().join(", ");
        Ok([("n", agents.len().to_string()), ("agents", if listing.is_empty() { "(nobody yet)".into() } else { listing }),
            ("busy", if busy.is_empty() { "(none yet)".into() } else { busy }),
            ("gaps", if gaps.is_empty() { "anything new".into() } else { gaps.iter().take(10).copied().collect::<Vec<_>>().join(", ") }),
            ("roles", few.join(", "))].into_iter().collect())
    }

    /// The agent whose name, bio and voice read most like this text, and how close.
    pub async fn closest_agent(&self, text: &str) -> (Option<String>, f64) {
        match self.db.q("persona:closest", "SELECT id, score() FROM agents ORDER BY similarity(emb, :v) DESC LIMIT 1",
                        json!({"v": {"embed": text, "field": "emb", "table": "agents"}})).await {
            Ok(rows) if !rows.is_empty() => (Some(rows[0].s("id")), rows[0].f("score()")),
            _ => (None, 0.0),
        }
    }

    /// Makes up, checks and registers one new agent: {agent, closest, similarity} or {error}.
    pub async fn spawn_persona(&self, max_similarity: f64, tries: usize) -> Value {
        let Some(llm) = self.llm.as_ref() else { return json!({"error": "no model"}) };
        let mut usage = Usage::new(&llm.model);
        let mut last_error = String::new();
        for _ in 0..tries {
            let ctx = match self.persona_context().await {
                Ok(c) => c,
                Err(e) => {
                    last_error = e.message;
                    break;
                }
            };
            let body = json!({"max_tokens": 2000, "messages": [{"role": "user", "content": crate::brain::fill(ASK, &ctx)}],
                              "output_config": {"effort": "low", "format": {"type": "json_schema", "schema": schema()}}});
            let resp = match llm.create(&self.db, body, 0.0).await {
                Ok((r, u)) => {
                    usage.add(&u);
                    r
                }
                Err(e) => {
                    last_error = e.message;
                    break;
                }
            };
            let Ok(p) = serde_json::from_str::<Value>(&text_of(&resp)) else {
                last_error = "the persona was not JSON".into();
                continue;
            };
            let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
            static NOT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            static H: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            let mut handle = take(&re(&NOT, "[^a-z0-9_]").replace_all(&s("handle").to_lowercase(), ""), 24);
            if !re(&H, r"^[a-z][a-z0-9_]{1,23}$").is_match(&handle) {
                last_error = format!("bad handle {}", crate::tools::py_repr(&handle));
                continue;
            }
            if matches!(self.db.get("agents", &handle).await, Ok(Some(_))) {
                handle = format!("{}{}", take(&handle, 20), rand::thread_rng().gen_range(10..=99));
            }
            let (who, sim) = self.closest_agent(&format!("{} {} {}", s("name"), s("bio"), s("persona"))).await;
            if let Some(w) = &who {
                if sim >= max_similarity {
                    last_error = format!("too close to @{w} ({sim:.2})");
                    continue;
                }
            }
            let mut home = take(&slug(&s("home")), 24);
            if !home.is_empty() && matches!(self.db.get("communities", &home).await, Ok(None)) {
                let about = if s("home_about").is_empty() { s("bio") } else { s("home_about") };
                if self.create_community(None, &home, &take(&about, 300)).await.is_err() {
                    home.clear();
                }
            }
            let interests: Vec<String> = p["interests"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            match self.register(&handle, &take(&s("name"), 80), &take(&s("bio"), 300), &llm.model, "parley-genesis", &take(&s("persona"), 1200),
                                &interests, &s("role"), &home, "generator", "").await {
                Ok(out) => {
                    llm.record(&self.db, "persona", &usage, Some(&handle), true, 0,
                               &format!("born: @{handle}, {}, {}", s("role"), if home.is_empty() { "no home".to_string() } else { home.clone() })).await;
                    return json!({"agent": out["agent"], "closest": who, "similarity": round(sim, 3)});
                }
                Err(e) => last_error = e.message,
            }
        }
        if usage.calls > 0 {
            llm.record(&self.db, "persona", &usage, None, false, 0, &take(&format!("no persona: {last_error}"), 400)).await;
        }
        json!({"error": if last_error.is_empty() { "no persona".to_string() } else { last_error }})
    }
}
