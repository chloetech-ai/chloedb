//! The judge of a study: how well each row an agent was shown answers its
//! need, 0 (not), 1 (partly, related) or 2 (it answers it). A known need's
//! truth is its row; a topical need's rows are graded by Claude, from the
//! title and line the page showed and the text of the rows opened, every
//! row once for a need: the grades are kept (`exp_judged`), so the arms of
//! a study are graded alike and a study run again costs nothing more.
//! With no model, words shared with the need grade a row.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::live::{self, Brain};
use crate::personas::Need;
use crate::server::{lit, Result, Server};
use crate::source::{clip, Hit};

pub const TABLE: &str = "exp_judged";

pub struct Judge {
    brain: Brain,
    http: reqwest::Client,
    server: Server,
    /// Grades by need, then row.
    kept: Mutex<HashMap<String, HashMap<String, u8>>>,
    /// Needs whose kept grades were read from the table.
    read: Mutex<std::collections::HashSet<String>>,
}

const SYSTEM: &str = "You are the judge of a user study of a website's search. For a person's need and the results the site showed them, grade each result: 2 if it answers the need (it is what they came for, or one of the things they want), 1 if it is related and partly useful, 0 if it is not useful. Judge from the title and the line under it, and from its text when given. Be strict: sharing words with the need is not answering it.";

fn tool() -> Value {
    json!({"name": "grades", "description": "The grade of each result.", "input_schema": {"type": "object", "properties": {"grades": {"type": "array", "items": {"type": "object", "properties": {"n": {"type": "integer", "description": "The result's number"}, "grade": {"type": "integer", "enum": [0, 1, 2]}}, "required": ["n", "grade"]}}}, "required": ["grades"]}})
}

/// A need's key in the table: its text, hashed, so a need written again
/// in the same words keeps its grades.
pub fn need_key(n: &Need) -> String {
    let h = n.text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{h:016x}")
}

impl Judge {
    pub fn new(brain: Brain, server: Server) -> Judge {
        Judge { brain, http: reqwest::Client::new(), server, kept: Mutex::new(HashMap::new()), read: Mutex::new(Default::default()) }
    }

    pub fn model(&self) -> String {
        self.brain.name().to_string()
    }

    pub async fn create_table(server: &Server) -> Result<()> {
        server.sql(&format!("CREATE TABLE IF NOT EXISTS {TABLE} (id KEYWORD PRIMARY KEY, need KEYWORD, item KEYWORD, grade INT, model KEYWORD)"), json!({})).await.map(|_| ())
    }

    /// The grades of a need known so far.
    pub fn grades_of(&self, n: &Need) -> HashMap<String, u8> {
        if let (Some(t), "known") = (&n.target, n.kind.as_str()) {
            return [(t.clone(), 2u8)].into_iter().collect();
        }
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).get(&need_key(n)).cloned().unwrap_or_default()
    }

    /// Reads the kept grades of a need from the table, once, and gives
    /// every grade of it known so far.
    pub async fn load(&self, need: &Need) -> HashMap<String, u8> {
        if need.kind == "known" {
            return self.grades_of(need);
        }
        let key = need_key(need);
        let first = self.read.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone());
        if first {
            if let Ok(a) = self.server.query(&format!("SELECT item, grade FROM {TABLE} WHERE need = {} LIMIT 10000", lit(&key)), json!({}), None).await {
                let mut k = self.kept.lock().unwrap_or_else(|e| e.into_inner());
                let m = k.entry(key.clone()).or_default();
                for r in a["rows"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(g)) = (r[0].as_str(), r[1].as_u64()) {
                        m.insert(id.to_string(), g.min(2) as u8);
                    }
                }
            }
        }
        self.grades_of(need)
    }

    /// The grades of `rows` for `need` (with the text of those opened), and
    /// what grading the new ones cost.
    pub async fn grade(&self, need: &Need, rows: &[Hit], opened: &HashMap<String, String>) -> Result<(HashMap<String, u8>, f64)> {
        if need.kind == "known" {
            let t = need.target.clone().unwrap_or_default();
            return Ok((rows.iter().map(|r| (r.id.clone(), u8::from(r.id == t) * 2)).collect(), 0.0));
        }
        let key = need_key(need);
        self.load(need).await;
        let known = self.kept.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned().unwrap_or_default();
        let mut new: Vec<&Hit> = Vec::new();
        for r in rows {
            if !known.contains_key(&r.id) && !new.iter().any(|x| x.id == r.id) {
                new.push(r);
            }
        }
        let mut cost = 0.0;
        let mut got: HashMap<String, u8> = HashMap::new();
        for chunk in new.chunks(25) {
            let (g, c) = self.ask(need, chunk, opened).await?;
            cost += c;
            got.extend(g);
        }
        if !got.is_empty() {
            let model = self.model();
            let ops: Vec<Value> = got.iter().map(|(id, g)| json!({"op": "upsert", "table": TABLE, "doc": {"id": format!("{key}|{id}"), "need": key, "item": id, "grade": g, "model": model}})).collect();
            let _ = self.server.write(ops).await;
            self.kept.lock().unwrap_or_else(|e| e.into_inner()).entry(key.clone()).or_default().extend(got);
        }
        let all = self.kept.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned().unwrap_or_default();
        Ok((rows.iter().map(|r| (r.id.clone(), all.get(&r.id).copied().unwrap_or(0))).collect(), cost))
    }

    async fn ask(&self, need: &Need, rows: &[&Hit], opened: &HashMap<String, String>) -> Result<(HashMap<String, u8>, f64)> {
        match &self.brain {
            Brain::Scripted => {
                let want = live::words(&need.text);
                Ok((rows.iter().map(|r| {
                    let have = live::words(&format!("{} {}", r.title, r.snippet));
                    let n = want.iter().filter(|w| have.contains(w)).count();
                    (r.id.clone(), if n >= 3 { 2 } else { u8::from(n >= 1) })
                }).collect(), 0.0))
            }
            Brain::Claude { key, model, base } => {
                let mut msg = format!("The person's need:\n\n\"{}\"\n\nThe results:\n", need.text);
                for (i, r) in rows.iter().enumerate() {
                    msg.push_str(&format!("\n{}. {}\n   {}\n", i + 1, r.title, clip(&r.snippet, 300)));
                    if let Some(t) = opened.get(&r.id) {
                        msg.push_str(&format!("   Its text: {}\n", clip(&t.replace('\n', " "), 700)));
                    }
                }
                let (input, cost) = live::tool_answer(&self.http, base, key, model, SYSTEM, &tool(), &msg, 8000).await?;
                let mut out: HashMap<String, u8> = rows.iter().map(|r| (r.id.clone(), 0)).collect();
                for g in input["grades"].as_array().into_iter().flatten() {
                    let n = g["n"].as_u64().unwrap_or(0) as usize;
                    if let Some(r) = n.checked_sub(1).and_then(|i| rows.get(i)) {
                        out.insert(r.id.clone(), g["grade"].as_u64().unwrap_or(0).min(2) as u8);
                    }
                }
                Ok((out, cost))
            }
        }
    }
}
