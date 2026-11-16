//! Autocomplete's rows: what people type into a search of HN, from what HN
//! wrote. The phrases of the titles (one to three words, as many stories
//! as hold them and the votes they got), the sites, the topics and the
//! people, each a row of `hn_complete` keyed by a kind letter and its text
//! in lowercase (`p:kubernetes operator`, `s:github.com`, `t:rust`,
//! `u:patio11`), so that completing is a prefix of the key:
//!
//! ```sql
//! SELECT text, kind, n FROM hn_complete WHERE PREFIX(id, 'p:kubernetes o')
//! ORDER BY weight DESC LIMIT 6
//! ```
//!
//! The key's dictionary is sorted, so a prefix is one run of it, and the
//! table is clustered by the key, so the run's rows sit together and the
//! best of them by weight are read from a few blocks.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use serde_json::{json, Value};

/// Words a phrase does not start or end with.
const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "to", "in", "on", "for", "with", "at", "by", "from", "as", "is", "are", "was", "were", "be", "it", "its", "it's", "this", "that", "your",
    "you", "my", "we", "our", "i", "how", "why", "what", "when", "who", "which", "do", "does", "can", "will", "not", "no", "about", "into", "than", "but", "if", "so", "via", "vs", "vs.",
    "hn", "show", "ask", "tell", "launch", "-", "–", "—", "|", "&", "new", "just", "now", "more", "most", "all", "has", "have", "get", "use", "using", "up", "out", "one", "&amp;",
];

/// A title's words as people type them: lowercase, punctuation at the
/// ends off (`C++`, `node.js` and `.NET` keep theirs).
pub fn words(t: &str) -> Vec<String> {
    let t = crate::search::short_title(t);
    t.split_whitespace()
        .map(|w| {
            let w = w.to_lowercase();
            let w = w.trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '.' || c == '#' || c == '@'));
            let w = w.trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '+' || c == '#'));
            w.to_string()
        })
        .filter(|w| !w.is_empty() && w.len() <= 30 && !w.starts_with("http"))
        .collect()
}

fn key(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The phrases of a title: runs of one to three words that neither start
/// nor end with a word of [`STOP`].
fn grams(w: &[String], mut f: impl FnMut(&str)) {
    for i in 0..w.len() {
        if STOP.contains(&w[i].as_str()) || w[i].chars().all(|c| c.is_ascii_digit()) && w[i].len() < 3 {
            continue;
        }
        let mut s = String::new();
        for (k, x) in w.iter().enumerate().skip(i).take(3) {
            if k > i {
                s.push(' ');
            }
            s.push_str(x);
            if !STOP.contains(&x.as_str()) {
                f(&s);
            }
        }
    }
}

struct Count {
    n: u32,
    w: f32,
}

/// Reads `dir/hn_stories.jsonl` and `dir/hn_users.jsonl`, writes
/// `dir/hn_complete.jsonl`; the number of rows written.
pub fn build(dir: &Path, progress: impl Fn(&str)) -> Result<u64, String> {
    let stories = dir.join("hn_stories.jsonl");
    let read = || -> Result<BufReader<std::fs::File>, String> { std::fs::File::open(&stories).map(|f| BufReader::with_capacity(1 << 20, f)).map_err(|e| format!("{}: {e}", stories.display())) };
    // Pass 1: counts by the phrase's hash (8 bytes a phrase, not its text).
    let mut counts: HashMap<u64, Count> = HashMap::new();
    let mut sites: HashMap<String, Count> = HashMap::new();
    let mut topics: HashMap<String, Count> = HashMap::new();
    let mut n_stories = 0u64;
    for line in read()?.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let pts = v["points"].as_i64().unwrap_or(0).max(0) as f32;
        let w = (2.0 + pts).ln();
        let title = v["title"].as_str().unwrap_or("");
        let ws = words(title);
        let mut seen = std::collections::HashSet::new();
        grams(&ws, |g| {
            if seen.insert(key(g)) {
                let c = counts.entry(key(g)).or_insert(Count { n: 0, w: 0.0 });
                c.n += 1;
                c.w += w;
            }
        });
        if let Some(s) = v["site"].as_str().filter(|s| !s.is_empty()) {
            let c = sites.entry(s.to_lowercase()).or_insert(Count { n: 0, w: 0.0 });
            c.n += 1;
            c.w += w;
        }
        if let Some(ts) = v["topics"].as_array() {
            for t in ts.iter().filter_map(|t| t.as_str()) {
                let c = topics.entry(t.to_string()).or_insert(Count { n: 0, w: 0.0 });
                c.n += 1;
                c.w += w;
            }
        }
        n_stories += 1;
        if n_stories % 1_000_000 == 0 {
            progress(&format!("complete: pass 1, {n_stories} stories, {} phrases", counts.len()));
        }
    }
    // Keep the phrases more than a few stories hold.
    let keep = |g: &str, c: &Count| -> bool {
        let k = g.split(' ').count();
        c.n >= match k {
            1 => 3,
            2 => 4,
            _ => 6,
        }
    };
    // Pass 2: the kept phrases' text.
    let out_path = dir.join("hn_complete.jsonl");
    let mut out = BufWriter::new(std::fs::File::create(&out_path).map_err(|e| format!("{}: {e}", out_path.display()))?);
    let mut written = 0u64;
    let mut done: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for line in read()?.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let ws = words(v["title"].as_str().unwrap_or(""));
        let mut rows: Vec<String> = Vec::new();
        grams(&ws, |g| {
            let k = key(g);
            if let Some(c) = counts.get(&k) {
                if keep(g, c) && done.insert(k) {
                    rows.push(json!({"id": format!("p:{g}"), "kind": "phrase", "text": g, "n": c.n, "weight": (c.w * 100.0).round() / 100.0}).to_string());
                }
            }
        });
        for r in rows {
            writeln!(out, "{r}").map_err(|e| e.to_string())?;
            written += 1;
        }
    }
    progress(&format!("complete: {written} phrases"));
    for (s, c) in sites.iter().filter(|(_, c)| c.n >= 3) {
        writeln!(out, "{}", json!({"id": format!("s:{s}"), "kind": "site", "text": s, "n": c.n, "weight": (c.w * 100.0).round() / 100.0})).map_err(|e| e.to_string())?;
        written += 1;
    }
    for (t, c) in &topics {
        writeln!(out, "{}", json!({"id": format!("t:{}", t.to_lowercase()), "kind": "topic", "text": t, "n": c.n, "weight": (c.w * 100.0).round() / 100.0})).map_err(|e| e.to_string())?;
        written += 1;
    }
    // People who wrote enough to be looked for.
    let users = dir.join("hn_users.jsonl");
    if let Ok(f) = std::fs::File::open(&users) {
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| e.to_string())?;
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let (st, cm, pts) = (v["stories"].as_i64().unwrap_or(0), v["comments"].as_i64().unwrap_or(0), v["points"].as_i64().unwrap_or(0));
            if st < 5 && cm < 50 {
                continue;
            }
            let id = v["id"].as_str().unwrap_or("");
            let w = (1.0 + cm as f64).ln() * 3.0 + (1.0 + pts as f64).ln() * 2.0;
            writeln!(out, "{}", json!({"id": format!("u:{}", id.to_lowercase()), "kind": "person", "text": id, "n": cm + st, "weight": (w * 100.0).round() / 100.0})).map_err(|e| e.to_string())?;
            written += 1;
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases_of_a_title() {
        let w = words("Show HN: A faster Node.js parser in C++ (2023)");
        assert_eq!(w, vec!["a", "faster", "node.js", "parser", "in", "c++", "2023"]);
        let mut g = Vec::new();
        grams(&w, |x| g.push(x.to_string()));
        assert!(g.contains(&"faster node.js parser".to_string()));
        assert!(g.contains(&"parser in c++".to_string()));
        assert!(!g.iter().any(|x| x.starts_with("a ") || x.ends_with(" in")));
    }
}
