//! Personas of a real site, written from its own rows: people who would
//! use it, each with needs in their own words. A need is either
//!
//! - **known**: the person remembers one row of the site (a story, a
//!   paper) and wants it again; they describe it as they remember it, not
//!   by its title, and the row is the exact truth: found or not; or
//! - **topical**: they want rows on a subject, an answer or opinions; any
//!   row may answer it, and a judge grades what they were shown
//!   ([`crate::judge`]).
//!
//! Claude writes them ([`write`]) from a sample of the site's rows; with
//! no model, a plain stand-in does ([`scripted`]), for tests.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::live::{self, Brain};
use crate::server::Result;
use crate::source::clip;

pub const MAX_PERSONAS: usize = 16;
pub const MAX_NEEDS: usize = 5;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Need {
    pub id: String,
    /// `known` or `topical`.
    pub kind: String,
    pub text: String,
    /// A known need's row, and its title.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub target_title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub about: String,
    pub needs: Vec<Need>,
}

/// Personas written for a site at once.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersonaSet {
    pub id: String,
    /// The source they were written from, and its title.
    pub source: String,
    pub title: String,
    pub created: i64,
    pub model: String,
    pub seed: u64,
    /// Rows of the site they were written from.
    pub rows: usize,
    pub cost: f64,
    pub personas: Vec<Persona>,
}

impl PersonaSet {
    pub fn needs(&self) -> usize {
        self.personas.iter().map(|p| p.needs.len()).sum()
    }

    /// One line of it, for lists.
    pub fn summary(&self) -> Value {
        let known = self.personas.iter().flat_map(|p| &p.needs).filter(|n| n.kind == "known").count();
        json!({"id": self.id, "source": self.source, "title": self.title, "created": self.created, "model": self.model, "personas": self.personas.len(), "needs": self.needs(), "known": known, "names": self.personas.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), "cost": self.cost})
    }
}

const SYSTEM: &str = "You write the people of a user study of a website's search: realistic, varied people who would use this site, and what each came to find. You write them from rows of the site itself, so that what they look for is really there.";

fn tool() -> Value {
    json!({
        "name": "personas",
        "description": "The people of the study and their needs.",
        "input_schema": {"type": "object", "properties": {"personas": {"type": "array", "items": {"type": "object", "properties": {
            "name": {"type": "string", "description": "A first name and a few words: who they are (\"Maya, a nurse in Leeds\")"},
            "about": {"type": "string", "description": "Two or three sentences: their work, how much they know of the subject, how they use the site and how they search (few words or sentences, patient or not)"},
            "needs": {"type": "array", "items": {"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["known", "topical"]},
                "need": {"type": "string", "description": "In the first person, one or two sentences, as they would say it"},
                "row": {"type": "integer", "description": "For a known need, the number of the row they remember; 0 for a topical need"}
            }, "required": ["kind", "need", "row"]}}
        }, "required": ["name", "about", "needs"]}}}, "required": ["personas"]}
    })
}

/// The first message: the site, its rows, and what to write.
pub fn prompt(describe: &Value, rows: &[Value], count: usize, needs: usize) -> String {
    let mut s = format!(
        "The site: {}\n\n{}\n\nSome of its {}:\n",
        describe["title"].as_str().unwrap_or("a site"),
        describe["about"].as_str().unwrap_or(""),
        describe["rows"].as_str().unwrap_or("rows")
    );
    for (i, r) in rows.iter().enumerate() {
        s.push_str(&format!("\n{}. {} — {}", i + 1, r["title"].as_str().unwrap_or(""), clip(r["snippet"].as_str().unwrap_or(""), 220)));
    }
    s.push_str(&format!(
        "\n\nWrite {count} people who would really use this site, as varied as its readers are: in work, age, country, how much they know, and why they come. Give each {needs} needs: things they came to the site to find.\n\n\
         About half the needs are known: the person remembers one row listed above and wants it again. Give its number in row, and write the need as they would remember it: what it was about, roughly when, what was said, a detail; vaguely, in their own words, never its title or a phrase copied from it, so that only a good search finds it. Do not pick the same row twice.\n\n\
         The others are topical: the person wants rows on a subject, an answer, opinions or the best of something, which the site holds; any row may answer it, and row is 0. Write these as people do, from what they want, not from the rows above.\n\n\
         Keep every need to what the site holds, in the person's voice."
    ));
    s
}

/// Personas written by Claude from `rows`, and what they cost.
pub async fn write(brain: &Brain, http: &reqwest::Client, set_id: &str, describe: &Value, rows: &[Value], count: usize, needs: usize, seed: u64) -> Result<(Vec<Persona>, f64)> {
    let count = count.clamp(1, MAX_PERSONAS);
    let needs = needs.clamp(1, MAX_NEEDS);
    let Brain::Claude { key, model, base } = brain else {
        return Ok((scripted(set_id, rows, count, needs, seed), 0.0));
    };
    // The models think first, so the room is for the thinking and the people.
    let (input, cost) = live::tool_answer(http, base, key, model, SYSTEM, &tool(), &prompt(describe, rows, count, needs), 32000).await?;
    Ok((read(set_id, &input, rows), cost))
}

/// The personas of the tool's input: known needs whose row is not one of
/// `rows`, or is picked again, become topical.
pub fn read(set_id: &str, input: &Value, rows: &[Value]) -> Vec<Persona> {
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (pi, p) in input["personas"].as_array().into_iter().flatten().take(MAX_PERSONAS).enumerate() {
        let pid = format!("{set_id}-p{}", pi + 1);
        let mut needs = Vec::new();
        for (ni, n) in p["needs"].as_array().into_iter().flatten().take(MAX_NEEDS).enumerate() {
            let text: String = n["need"].as_str().unwrap_or("").trim().chars().take(400).collect();
            if text.is_empty() {
                continue;
            }
            let row = n["row"].as_u64().unwrap_or(0) as usize;
            let target = (n["kind"] == "known").then(|| row.checked_sub(1).and_then(|i| rows.get(i))).flatten().filter(|_| used.insert(row));
            needs.push(Need {
                id: format!("{pid}-n{}", ni + 1),
                kind: if target.is_some() { "known" } else { "topical" }.into(),
                text,
                target: target.and_then(|r| r["id"].as_str().map(str::to_string).or_else(|| r["id"].as_i64().map(|i| i.to_string()))),
                target_title: target.and_then(|r| r["title"].as_str().map(str::to_string)),
            });
        }
        if needs.is_empty() {
            continue;
        }
        out.push(Persona { id: pid, name: p["name"].as_str().unwrap_or("Someone").chars().take(80).collect(), about: p["about"].as_str().unwrap_or("").chars().take(600).collect(), needs });
    }
    out
}

/// A plain stand-in, with no model: each person remembers a row by the
/// first words of its title, and wants rows about two of its words.
pub fn scripted(set_id: &str, rows: &[Value], count: usize, needs: usize, seed: u64) -> Vec<Persona> {
    const NAMES: [&str; 8] = ["Ana, a nurse", "Ben, a student", "Chen, an engineer", "Dara, a journalist", "Eli, a founder", "Femi, a teacher", "Gus, a retired pilot", "Hana, a researcher"];
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut r = seed as usize;
    for p in 0..count {
        let pid = format!("{set_id}-p{}", p + 1);
        let mut list = Vec::new();
        for n in 0..needs {
            r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let row = &rows[(r >> 33) % rows.len()];
            let title = row["title"].as_str().unwrap_or("");
            let words: Vec<&str> = title.split_whitespace().collect();
            let known = n % 2 == 0;
            list.push(Need {
                id: format!("{pid}-n{}", n + 1),
                kind: if known { "known" } else { "topical" }.into(),
                text: if known { format!("I remember one about {}", words.iter().take(4).cloned().collect::<Vec<_>>().join(" ")) } else { words.iter().rev().take(2).cloned().collect::<Vec<_>>().join(" ") },
                target: known.then(|| row["id"].as_str().map(str::to_string).unwrap_or_else(|| row["id"].to_string())),
                target_title: known.then(|| title.to_string()),
            });
        }
        out.push(Persona { id: pid, name: NAMES[(p + seed as usize) % NAMES.len()].into(), about: "Uses the site now and then, and searches in a few words.".into(), needs: list });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_needs_keep_their_row_once() {
        let rows = vec![json!({"id": "101", "title": "Announcing Rust 1.0"}), json!({"id": "102", "title": "Why I switched from Go to Rust"})];
        let input = json!({"personas": [
            {"name": "Maya, a nurse", "about": "x", "needs": [{"kind": "known", "need": "the launch of a language", "row": 1}, {"kind": "known", "need": "the same one", "row": 1}, {"kind": "known", "need": "no such row", "row": 9}, {"kind": "topical", "need": "is rust worth it", "row": 0}, {"kind": "topical", "need": "", "row": 0}]},
            {"name": "Nobody", "about": "", "needs": []}
        ]});
        let ps = read("s1", &input, &rows);
        assert_eq!(ps.len(), 1);
        let kinds: Vec<(&str, Option<&str>)> = ps[0].needs.iter().map(|n| (n.kind.as_str(), n.target.as_deref())).collect();
        assert_eq!(kinds, [("known", Some("101")), ("topical", None), ("topical", None), ("topical", None)]);
        assert_eq!(ps[0].needs[0].target_title.as_deref(), Some("Announcing Rust 1.0"));
        assert_eq!(ps[0].needs[0].id, "s1-p1-n1");
        let p = prompt(&json!({"title": "Hacker News", "about": "stories", "rows": "stories"}), &rows, 3, 2);
        assert!(p.contains("1. Announcing Rust 1.0") && p.contains("Write 3 people") && p.contains("Give each 2 needs"));
    }

    #[test]
    fn the_stand_in_writes_from_the_rows() {
        let rows = vec![json!({"id": "7", "title": "Postgres full text search is good enough"})];
        let ps = scripted("s", &rows, 2, 2, 1);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].needs[0].target.as_deref(), Some("7"));
        assert_eq!(ps[0].needs[1].kind, "topical");
    }
}
