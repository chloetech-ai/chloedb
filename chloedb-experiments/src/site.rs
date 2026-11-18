//! A site to experiment on: a table of rows a ranking serves, the queries
//! people ask it, and what each row is worth to each query (its grade),
//! which the simulator turns into a chance of a click and of a satisfied
//! one. A BEIR collection is a site with judged queries; any table is one
//! with queries and grades given (`--queries`, `--grades`), or with grades
//! learnt from a log (later phases).

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use serde_json::{json, Value};

use crate::click::Slot;
use crate::server::{Result, Server};

#[derive(Clone, Debug)]
pub struct Query {
    pub id: String,
    pub text: String,
    /// Its share of the site's traffic (relative).
    pub weight: f64,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub name: String,
    pub table: String,
    pub queries: Vec<Query>,
    /// Per query, each judged row's key and grade (0 to `max_grade`).
    pub grades: Vec<HashMap<String, u8>>,
    pub max_grade: u8,
}

/// A row's chances from its grade g of G: attractiveness 0.04 + 0.8 ×
/// (2^g − 1)/(2^G − 1), satisfaction 0.15 + 0.7 × the same (graded gain,
/// as in Chapelle and Zhang's DBN and the TREC tracks' ERR).
pub fn slot_of(grade: u8, max: u8) -> Slot {
    let g = ((1u32 << grade) - 1) as f64 / ((1u32 << max.max(1)) - 1) as f64;
    Slot { attract: 0.04 + 0.8 * g, satisfy: 0.15 + 0.7 * g }
}

fn lines(path: &Path) -> Result<Vec<String>> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::BufReader::new(f).lines().collect::<std::io::Result<_>>().map_err(|e| format!("{}: {e}", path.display()))
}

/// A BEIR collection's rows (`corpus.jsonl`: `_id`, `title`, `text`).
pub fn beir_rows(dir: &Path) -> Result<Vec<(String, String, String)>> {
    lines(&dir.join("corpus.jsonl"))?
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let j: Value = serde_json::from_str(l).map_err(|e| format!("corpus.jsonl: {e}"))?;
            Ok((j["_id"].as_str().unwrap_or_default().to_string(), j["title"].as_str().unwrap_or_default().to_string(), j["text"].as_str().unwrap_or_default().to_string()))
        })
        .collect()
}

impl Site {
    /// A BEIR collection's judged queries of `split` (`qrels/<split>.tsv`),
    /// over table `table`. Each query's traffic is Zipf's: the k-th of a
    /// seeded order gets weight 1/(k + 1)^0.9, a head and a long tail.
    pub fn beir(dir: &Path, table: &str, split: &str, seed: u64) -> Result<Site> {
        let mut texts = HashMap::new();
        for l in lines(&dir.join("queries.jsonl"))?.iter().filter(|l| !l.trim().is_empty()) {
            let j: Value = serde_json::from_str(l).map_err(|e| format!("queries.jsonl: {e}"))?;
            texts.insert(j["_id"].as_str().unwrap_or_default().to_string(), j["text"].as_str().unwrap_or_default().to_string());
        }
        let mut judged: Vec<(String, HashMap<String, u8>)> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        let mut max_grade = 1;
        for (i, l) in lines(&dir.join("qrels").join(format!("{split}.tsv")))?.iter().enumerate() {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 3 || (i == 0 && f[2].parse::<i64>().is_err()) {
                continue;
            }
            let g: i64 = f[2].trim().parse().map_err(|_| format!("qrels: grade {:?}", f[2]))?;
            if g <= 0 || !texts.contains_key(f[0]) {
                continue;
            }
            let g = g.min(4) as u8;
            max_grade = max_grade.max(g);
            let k = *at.entry(f[0].to_string()).or_insert_with(|| {
                judged.push((f[0].to_string(), HashMap::new()));
                judged.len() - 1
            });
            judged[k].1.insert(f[1].to_string(), g);
        }
        let mut order: Vec<usize> = (0..judged.len()).collect();
        let mut rng = crate::policy::Rng::seeded(seed ^ 0x51_7e);
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i + 1));
        }
        let mut weight = vec![0.0; judged.len()];
        for (rank, &q) in order.iter().enumerate() {
            weight[q] = 1.0 / ((rank + 1) as f64).powf(0.9);
        }
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "beir".into());
        let queries = judged.iter().enumerate().map(|(i, (id, _))| Query { id: id.clone(), text: texts[id].clone(), weight: weight[i] }).collect();
        Ok(Site { name, table: table.to_string(), queries, grades: judged.into_iter().map(|(_, g)| g).collect(), max_grade })
    }

    /// Keeps the queries `keep` says (by index), in order.
    pub fn keep(&mut self, keep: &[bool]) {
        let mut i = 0;
        self.queries.retain(|_| (keep[i], i += 1).0);
        let mut i = 0;
        self.grades.retain(|_| (keep[i], i += 1).0);
    }

    /// Row `key`'s chances for query `q`, before a cohort's taste.
    pub fn slot(&self, q: usize, key: &str) -> Slot {
        slot_of(self.grades[q].get(key).copied().unwrap_or(0), self.max_grade)
    }

    /// Each query's share of the traffic.
    pub fn shares(&self) -> Vec<f64> {
        let t: f64 = self.queries.iter().map(|q| q.weight).sum();
        self.queries.iter().map(|q| q.weight / t).collect()
    }
}

/// Makes table `table` (`id`, `title`, `body`) on the server and loads a
/// BEIR collection's rows into it, unless it holds them already.
pub async fn load_beir(server: &Server, dir: &Path, table: &str) -> Result<usize> {
    let rows = beir_rows(dir)?;
    server.sql(&format!("CREATE TABLE IF NOT EXISTS {table} (id KEYWORD PRIMARY KEY, title TEXT, body TEXT)"), json!({})).await?;
    let have = server.query(&format!("SELECT COUNT(*) FROM {table}"), json!({}), None).await?["rows"][0][0].as_u64().unwrap_or(0) as usize;
    if have == rows.len() {
        return Ok(0);
    }
    for chunk in rows.chunks(2000) {
        let ops: Vec<Value> = chunk.iter().map(|(id, title, body)| json!({"op": "upsert", "table": table, "doc": {"id": id, "title": title, "body": body}})).collect();
        server.write(ops).await?;
    }
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_become_chances() {
        let (a, b, c) = (slot_of(0, 2), slot_of(1, 2), slot_of(2, 2));
        assert!((a.attract - 0.04).abs() < 1e-12 && (c.attract - 0.84).abs() < 1e-12);
        assert!(a.attract < b.attract && b.attract < c.attract && a.satisfy < b.satisfy && b.satisfy < c.satisfy);
        assert_eq!(slot_of(1, 1), c);
    }
}
