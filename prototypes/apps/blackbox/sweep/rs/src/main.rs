// See Cargo.toml. Usage:
//   bbsweep-rs <provider-formats.jsonl> < docs.jsonl > spans.jsonl
// Each input line is {"id": "...", "text": "..."}; each output line is
//   {"id", "chloectx": [[start, end, "kind"], ...], "noseyparker": [[start, end, rule_index], ...]}
// with byte offsets into the text. With `--self <source>` it instead runs every rule of that
// source on its own examples and prints, per example, the secret its rule captures.
use std::io::{BufRead, Write};

use regex::Regex;
use serde_json::{json, Value};

// ChloeCtx's redactor, compiled from the checkout as it is. It refers to crate::record::Record
// in one helper this tool does not call.
#[path = "../../../../../../chloectx/src/redact.rs"]
#[allow(dead_code)]
mod redact;
mod record {
    pub struct Record {
        pub text: String,
        pub ctx: String,
    }
}

/// The spans of `text` that `redact::redact` replaced, with each marker's kind. The redactor
/// returns text, not spans: the pieces between its markers are found again in the original, in
/// order, and what lies between two pieces is what a marker replaced.
fn chloectx_spans(text: &str, marker: &Regex) -> Vec<(usize, usize, String)> {
    let red = redact::redact(text);
    if red == text {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let mut pos = 0usize; // in text
    let mut last = 0usize; // in red
    let mut pending: Option<String> = None;
    let mut pieces: Vec<(&str, Option<String>)> = Vec::new();
    for m in marker.captures_iter(&red) {
        let whole = m.get(0).unwrap();
        pieces.push((&red[last..whole.start()], Some(m[1].to_string())));
        last = whole.end();
    }
    pieces.push((&red[last..], None));
    for (piece, kind_after) in pieces {
        // Where this literal piece is in the original, at or after `pos`.
        let at = if piece.is_empty() { Some(pos) } else { text[pos..].find(piece).map(|i| pos + i) };
        let Some(at) = at else { break };
        if let Some(kind) = pending.take() {
            if at > pos {
                spans.push((pos, at, kind));
            }
        }
        pos = at + piece.len();
        pending = kind_after;
    }
    if let Some(kind) = pending {
        if pos < text.len() {
            spans.push((pos, text.len(), kind));
        }
    }
    // An empty piece between two markers gives the first marker nothing: the second takes the
    // rest. That is enough to say what was removed.
    spans
}

struct Rule {
    idx: usize,
    re: Regex,
}

/// The spans a rule's match removes: every capture group that took part (a rule such as
/// np.generic.3 captures a username and a password), else the whole match.
fn secret_spans(c: &regex::Captures) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 1..c.len() {
        if let Some(g) = c.get(i) {
            if !g.as_str().is_empty() {
                out.push((g.start(), g.end()));
            }
        }
    }
    if out.is_empty() {
        let m = c.get(0).unwrap();
        out.push((m.start(), m.end()));
    }
    out
}

/// Nosey Parker's rules carry PCRE comments, `(?# ... )`, which Vectorscan reads and the regex
/// crate does not: they are removed, and nothing else of a rule is changed.
fn strip_comments(p: &str) -> String {
    Regex::new(r"\(\?#[^)]*\)").unwrap().replace_all(p, "").into_owned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rules_path = args.get(1).expect("usage: bbsweep-rs <provider-formats.jsonl> [--self]");
    let self_test = args.iter().any(|a| a == "--self");
    let file = std::fs::read_to_string(rules_path).expect("rules file");
    let mut rules = Vec::new();
    let mut all: Vec<Value> = Vec::new();
    let mut refused = Vec::new();
    for (idx, line) in file.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line).expect("rule json");
        if v["source"] == "noseyparker" {
            match regex::RegexBuilder::new(&strip_comments(v["regex"].as_str().unwrap_or(""))).size_limit(64 << 20).build() {
                Ok(re) => rules.push(Rule { idx, re }),
                Err(e) => refused.push(json!({"id": v["id"], "error": e.to_string().lines().last().unwrap_or("").to_string()})),
            }
        }
        all.push(v);
    }
    let out = std::io::stdout();
    let mut out = std::io::BufWriter::new(out.lock());
    if self_test {
        // Each Nosey Parker example with the secret its own rule captures.
        for r in &rules {
            let v = &all[r.idx];
            for (n, ex) in v["examples"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().enumerate() {
                let ex = ex.as_str().unwrap_or("");
                // The longest group is taken as the example's secret.
                let secret = r.re.captures(ex).map(|c| {
                    let (s, e) = secret_spans(&c).into_iter().max_by_key(|(s, e)| e - s).unwrap();
                    ex[s..e].to_string()
                });
                writeln!(out, "{}", json!({"rule": v["id"], "n": n, "example": ex, "secret": secret})).unwrap();
            }
        }
        writeln!(out, "{}", json!({"compiled": rules.len(), "refused": refused})).unwrap();
        return;
    }
    let marker = Regex::new(r"\[REDACTED:([a-z0-9\-]+)\]").unwrap();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.expect("stdin");
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).expect("doc json");
        let text = v["text"].as_str().unwrap_or("");
        let cx: Vec<Value> = chloectx_spans(text, &marker).into_iter().map(|(s, e, k)| json!([s, e, k])).collect();
        let mut np: Vec<Value> = Vec::new();
        for r in &rules {
            for c in r.re.captures_iter(text) {
                for (s, e) in secret_spans(&c) {
                    np.push(json!([s, e, all[r.idx]["id"]]));
                }
            }
        }
        writeln!(out, "{}", json!({"id": v["id"], "chloectx": cx, "noseyparker": np})).unwrap();
    }
}
