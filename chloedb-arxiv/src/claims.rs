//! For and against: a reader states a claim ("scaling laws hold for
//! retrieval") and gets the papers that support it and those that
//! contradict it, each with the sentence of its abstract that says so.
//! The papers nearest the claim are found by its words and its meaning
//! (fused by rank), and the model reads their abstracts against the claim
//! in one call through `llm()`. A side counts only with a sentence that is
//! in the abstract (a quote the model made up is dropped), and the answer
//! is kept (`ax_notes`), so a claim asked again calls no model.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::{Row, RowExt};

/// Papers read for a claim.
pub const READ: usize = 40;

pub const SIDES: &str = "A reader states a claim about research. You get the numbered abstracts of the arXiv papers nearest to it. \
For each paper decide what its abstract says about the claim: \"for\" if its results or analysis support the claim, \
\"against\" if they contradict it or show it fails (the opposite holds, the effect vanishes, a counterexample, a limit the claim denies), \
\"mixed\" if it finds the claim holds only in some settings and not others, and \"none\" if it does not test or speak to the claim (the same topic is not enough). \
Judge what the paper finds, not what it assumes or cites. For every side but none, quote the one sentence of the abstract that shows it, copied word for word. \
Answer with JSON only, one object per paper in order: [{\"n\": 1, \"side\": \"for\", \"quote\": \"the sentence, word for word\", \"why\": \"at most 20 words on how it bears on the claim\"}, {\"n\": 2, \"side\": \"none\"}]; a paper that does not speak to it gets only its number and side.";

/// The key a claim is kept under: its words, lowercased.
pub fn key(claim: &str, since: i64) -> String {
    let words: Vec<String> = claim.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect();
    if since > 0 { format!("claim:{since}:{}", words.join(" ")) } else { format!("claim:{}", words.join(" ")) }
}

fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// The sentences of an abstract.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        cur.push(if ch == '\n' { ' ' } else { ch });
        let next = chars.get(i + 1).copied();
        if matches!(ch, '.' | '?' | '!') && next.is_none_or(|n| n.is_whitespace()) {
            // Not after one capital letter or a common abbreviation ("e.g.", "et al.").
            let t = cur.trim_end_matches(['.', '?', '!']);
            let last = t.rsplit(|c: char| c.is_whitespace()).next().unwrap_or("");
            let abbrev = (last.chars().count() == 1 && last.chars().all(char::is_uppercase)) || ["e.g", "i.e", "al", "vs", "cf", "Fig", "Eq", "approx"].contains(&last);
            if !abbrev {
                let s = cur.split_whitespace().collect::<Vec<_>>().join(" ");
                if !s.is_empty() {
                    out.push(s);
                }
                cur.clear();
            }
        }
    }
    let s = cur.split_whitespace().collect::<Vec<_>>().join(" ");
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// The abstract's own words for a quote: the quote when it is in the
/// abstract (spacing, case and punctuation aside), else the abstract's
/// sentence that holds nine tenths of its words, else none.
pub fn checked_quote(abstract_: &str, quote: &str) -> Option<String> {
    let q = norm(quote);
    let qw: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    if qw.len() < 4 {
        return None;
    }
    let sents = sentences(abstract_);
    if norm(abstract_).contains(&q) {
        // The sentence it is in, when it is within one; else the quote as it was.
        return Some(sents.iter().find(|s| norm(s).contains(&q)).cloned().unwrap_or_else(|| quote.trim().to_string()));
    }
    let mut best: Option<(f64, &String)> = None;
    for s in &sents {
        let sw: std::collections::HashSet<String> = norm(s).split(' ').map(str::to_string).collect();
        let hit = qw.iter().filter(|w| sw.contains(**w)).count() as f64 / qw.len() as f64;
        if hit >= 0.9 && best.is_none_or(|(b, _)| hit > b) {
            best = Some((hit, s));
        }
    }
    best.map(|(_, s)| s.clone())
}

/// A string field of one verdict, read even when the JSON around it is
/// broken (a quote of LaTeX can hold a bare backslash or quotation mark).
fn field(piece: &str, name: &str, next: &[&str]) -> String {
    let Some(at) = piece.find(&format!("\"{name}\"")) else { return String::new() };
    let rest = &piece[at + name.len() + 2..];
    let Some(open) = rest.find('"') else { return String::new() };
    let body = &rest[open + 1..];
    // It ends at the quotation mark before the next field, or the object's end.
    let mut end = body.len();
    for n in next {
        if let Some(i) = body.find(&format!("\"{n}\"")) {
            end = end.min(i);
        }
    }
    let raw = body[..end].trim_end_matches(|c: char| c == '}' || c == ']' || c == ',' || c.is_whitespace());
    let raw = raw.strip_suffix('"').unwrap_or(raw);
    serde_json::from_str::<String>(&format!("\"{raw}\"")).unwrap_or_else(|_| raw.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// The model's answer cut into one piece per paper, at each `{"n": `.
fn pieces(text: &str) -> Vec<(usize, &str)> {
    let re = regex::Regex::new(r#"\{\s*"n"\s*:\s*(\d+)"#).expect("pattern");
    let starts: Vec<(usize, usize)> = re.captures_iter(text).filter_map(|c| Some((c.get(0)?.start(), c[1].parse().ok()?))).collect();
    starts.iter().enumerate().map(|(i, &(at, n))| (n, &text[at..starts.get(i + 1).map(|s| s.0).unwrap_or(text.len())])).collect()
}

/// One verdict of the model: (side, quote, why), by the paper's number.
pub fn verdicts(text: &str) -> HashMap<usize, (String, String, String)> {
    let mut out = HashMap::new();
    for (n, piece) in pieces(text) {
        let v = crate::watch::objects(piece).into_iter().next().filter(|v| v["n"].as_u64() == Some(n as u64)).unwrap_or_else(|| {
            json!({"n": n, "side": field(piece, "side", &["quote", "why"]), "quote": field(piece, "quote", &["why"]), "why": field(piece, "why", &[])})
        });
        let side = v["side"].as_str().unwrap_or("none").trim().to_lowercase();
        let side = match side.as_str() {
            "for" | "supports" | "support" => "for",
            "against" | "contradicts" | "contradict" => "against",
            "mixed" => "mixed",
            _ => "none",
        };
        out.insert(n as usize, (side.to_string(), v["quote"].as_str().unwrap_or("").trim().to_string(), v["why"].as_str().unwrap_or("").trim().to_string()));
    }
    out
}

/// Words that say what a claim asserts about its subject, not what the subject is.
const STANCE: &[&str] = &[
    "improve", "improves", "improved", "improving", "help", "helps", "hurt", "hurts", "hold", "holds", "work", "works", "beat", "beats", "outperform", "outperforms",
    "better", "worse", "than", "always", "never", "does", "do", "not", "no", "can", "cannot", "can't", "don't", "doesn't", "is", "are", "be", "really", "true", "false",
    "matter", "matters", "need", "needed", "necessary", "enough", "still", "more", "less", "make", "makes", "for", "in", "and", "or", "with", "by", "at", "it", "they",
];

/// A claim's subject: its words without the asking and the asserting.
pub fn topic_words(claim: &str) -> String {
    let words = crate::watch::search_words(claim);
    let kept: Vec<&str> = words.split_whitespace().filter(|w| !STANCE.contains(&w.to_lowercase().as_str())).collect();
    if kept.is_empty() { words } else { kept.join(" ") }
}

/// The papers nearest a claim, by its words and its meaning.
async fn nearest(c: &Ctx<'_>, claim: &str, since: i64) -> Vec<Row> {
    let words = topic_words(claim);
    let n = words.split_whitespace().count();
    let cond = if since > 0 { "year >= :since AND " } else { "" };
    let params = json!({
        "q": words, "m": words, "since": since,
        "v": {"embed": claim, "field": "emb", "table": "ax_papers"},
    });
    // Every word of its subject (up to six), and all but one; ranked by the abstract, where findings are.
    let mut all = params.clone();
    all["min"] = json!(n.clamp(1, 6).to_string());
    let mut most = params.clone();
    most["min"] = json!(n.saturating_sub(1).clamp(1, 5).to_string());
    let rank = "ORDER BY coalesce(bm25(abstract, :q, synonyms => 1), 0) + 0.5 * coalesce(bm25(title, :q, synonyms => 1), 0) DESC LIMIT 40";
    let (by_all, by_most, by_meaning) = tokio::join!(
        c.soft("papers with every word of its subject", &format!("SELECT id FROM ax_papers WHERE {cond}{} {rank}", crate::search::MATCHED), all),
        c.soft("papers with all but one", &format!("SELECT id FROM ax_papers WHERE {cond}{} {rank}", crate::search::MATCHED), most),
        c.soft("papers by its meaning", &format!("SELECT id FROM ax_papers {} ORDER BY similarity(emb, :v) DESC LIMIT 40", if since > 0 { "WHERE year >= :since" } else { "" }), params.clone()),
    );
    let ids = |rows: &[Row]| rows.iter().map(|r| r.s("id")).collect::<Vec<_>>();
    // Every word counts twice: a paper that names the whole subject is the likeliest to test it.
    let top: Vec<String> = crate::watch::fuse(&[ids(&by_all), ids(&by_all), ids(&by_meaning), ids(&by_most)]).into_iter().take(READ).collect();
    if top.is_empty() {
        return Vec::new();
    }
    let rows = c.soft("their abstracts", &format!("SELECT {COLS}, abstract FROM ax_papers WHERE id IN (:ids)"), json!({"ids": top})).await;
    let mut by_id: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    top.iter().filter_map(|id| by_id.remove(id)).filter(|r| r.s("abstract").trim().len() > 80).collect()
}

fn card(r: &Row, quote: &str, why: &str) -> Value {
    json!({
        "id": r.s("id"), "title": r.s("title"), "authors": crate::answer::first_author(r), "year": r.i("year"),
        "cites": r.i("cites"), "pcat": r.s("pcat"), "venue": r.s("venue"), "code": r.s("code"),
        "quote": quote, "why": why,
    })
}

/// Papers the model reads in one call; the calls of a claim run at once.
pub const PER_CALL: usize = 10;

/// The model's verdicts on the papers (numbered from 1 in `papers`'
/// order), read ten to a call, all at once; a call whose answer stops
/// short is asked again for the papers it left out, with more room.
async fn read(c: &Ctx<'_>, claim: &str, model: &str, papers: &[Row]) -> Result<HashMap<usize, (String, String, String)>, String> {
    let ask = |part: Vec<usize>, room: usize| async move {
        let mut ctx = String::new();
        for (j, &i) in part.iter().enumerate() {
            let r = &papers[i - 1];
            ctx.push_str(&format!("[{}] “{}” ({}, {})\n{}\n\n", j + 1, r.s("title"), r.s("pcat"), r.i("year"), r.s("abstract").trim()));
        }
        let text = crate::ask::llm(c, "which papers support or contradict it (llm() on the server)", model, SIDES, &format!("The claim: {claim}"), &ctx, room).await;
        // Back to the papers' own numbers.
        text.map(|t| verdicts(&t).into_iter().filter_map(|(j, v)| Some((*part.get(j.checked_sub(1)?)?, v))).collect::<HashMap<_, _>>())
    };
    let parts: Vec<Vec<usize>> = (1..=papers.len()).collect::<Vec<_>>().chunks(PER_CALL).map(<[usize]>::to_vec).collect();
    let got = futures_util::future::join_all(parts.iter().map(|p| ask(p.clone(), 1500 + 250 * p.len()))).await;
    let mut out = HashMap::new();
    let mut again = Vec::new();
    let mut failed = None;
    for (p, g) in parts.iter().zip(got) {
        match g {
            Ok(v) => {
                let left: Vec<usize> = p.iter().copied().filter(|i| !v.contains_key(i)).collect();
                if left.len() * 5 > p.len() {
                    again.push(left);
                }
                out.extend(v);
            }
            Err(e) => {
                failed = Some(e);
                again.push(p.clone());
            }
        }
    }
    if !again.is_empty() {
        let got = futures_util::future::join_all(again.into_iter().map(|p| ask(p, 8192))).await;
        for g in got {
            match g {
                Ok(v) => out.extend(v),
                Err(e) => failed = Some(e),
            }
        }
    }
    match failed {
        Some(e) if out.is_empty() => Err(e),
        _ => Ok(out),
    }
}

/// Whether a claim's answer is kept.
pub async fn is_kept(c: &Ctx<'_>, claim: &str, since: i64) -> bool {
    !c.gsoft("a kept answer?", "SELECT id FROM ax_notes WHERE id = :k", json!({"k": key(claim, since)})).await.is_empty()
}

/// The papers for and against a claim, kept or read now (one model call).
pub async fn sides(c: &Ctx<'_>, claim: &str, since: i64, model: &str) -> Result<Value, String> {
    let k = key(claim, since);
    let kept = c.gsoft("a kept answer", "SELECT text, model, at FROM ax_notes WHERE id = :k", json!({"k": k})).await;
    if let Some(r) = kept.first().filter(|r| !r.s("text").trim().is_empty()) {
        let mut v: Value = serde_json::from_str(&r.s("text")).map_err(|e| e.to_string())?;
        v["kept"] = json!(true);
        v["at"] = json!(r.s("at"));
        return Ok(v);
    }
    let papers = nearest(c, claim, since).await;
    if papers.is_empty() {
        return Ok(json!({"claim": claim, "since": since, "for": [], "against": [], "mixed": [], "read": 0, "none": 0, "dropped": 0, "kept": false}));
    }
    let t0 = std::time::Instant::now();
    let v = read(c, claim, model, &papers).await?;
    let (mut pro, mut con, mut mixed) = (Vec::new(), Vec::new(), Vec::new());
    let (mut none, mut dropped, mut unread) = (0, 0, 0);
    let mut others = Vec::new();
    for (i, r) in papers.iter().enumerate() {
        let Some((side, quote, why)) = v.get(&(i + 1)) else {
            unread += 1;
            continue;
        };
        if side == "none" {
            none += 1;
            others.push(json!({"id": r.s("id"), "title": r.s("title"), "year": r.i("year")}));
            continue;
        }
        let Some(q) = checked_quote(&r.s("abstract"), quote) else {
            dropped += 1;
            continue;
        };
        let c = card(r, &q, why);
        match side.as_str() {
            "for" => pro.push(c),
            "against" => con.push(c),
            _ => mixed.push(c),
        }
    }
    let ms = (t0.elapsed().as_secs_f64() * 1000.0).round();
    let out = json!({
        "claim": claim, "since": since, "for": pro, "against": con, "mixed": mixed,
        "read": papers.len() - unread, "none": none, "others": others, "dropped": dropped, "model": model, "ms": ms, "kept": false,
    });
    // A reading the model cut short is not kept, so the next ask reads them all.
    if unread * 5 <= papers.len() {
        let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": "", "style": "claim", "text": out.to_string(), "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": ms})]).await;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABS: &str = "We study scaling laws for dense retrieval. Retrieval quality improves as a power law in model size, e.g. from 10M to 1B parameters. \
However, the gains vanish once the corpus exceeds 100M documents.";

    #[test]
    fn a_quote_counts_only_when_the_abstract_says_it() {
        // Word for word, and with other spacing, case and punctuation.
        assert_eq!(checked_quote(ABS, "Retrieval quality improves as a power law in model size").unwrap(), "Retrieval quality improves as a power law in model size, e.g. from 10M to 1B parameters.");
        assert_eq!(checked_quote(ABS, "however the gains VANISH once the corpus exceeds 100M documents").unwrap(), "However, the gains vanish once the corpus exceeds 100M documents.");
        // Nearly word for word: the abstract's sentence.
        assert_eq!(checked_quote(ABS, "However, gains vanish once the corpus exceeds 100M documents.").unwrap(), "However, the gains vanish once the corpus exceeds 100M documents.");
        // Made up, or too short to show anything.
        assert!(checked_quote(ABS, "Scaling laws do not hold for sparse retrieval at all.").is_none());
        assert!(checked_quote(ABS, "scaling laws").is_none());
    }

    #[test]
    fn sentences_split_after_their_full_stop_only() {
        let s = sentences(ABS);
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[1].contains("e.g. from 10M"));
    }

    #[test]
    fn verdicts_name_the_sides() {
        let v = verdicts(r#"[{"n": 1, "side": "supports", "quote": "a b c d", "why": "x"}, {"n": 2, "side": "against", "quote": "", "why": ""}, {"n": 3, "side": "unclear"}]"#);
        assert_eq!(v[&1].0, "for");
        assert_eq!(v[&2].0, "against");
        assert_eq!(v[&3].0, "none");
        // A quote of LaTeX with a bare backslash and quotation mark loses only its own object's JSON.
        let v = verdicts(r#"[{"n": 1, "side": "for", "quote": "constants $C_{\mathrm{LS}}$ of Poincar\'e "type" hold", "why": "a bound"}, {"n": 2, "side": "none"}, {"n": 3, "side": "against", "quote": "it fails", "why": "no"}]"#);
        assert_eq!(v.len(), 3);
        assert_eq!(v[&1].0, "for");
        assert!(v[&1].1.contains("mathrm{LS}"), "{:?}", v[&1]);
        assert_eq!(v[&1].2, "a bound");
        assert_eq!(v[&2].0, "none");
        assert_eq!(v[&3].0, "against");
        assert_eq!(topic_words("Weight decay improves generalization in transformers"), "Weight decay generalization transformers");
        assert_eq!(topic_words("does dense retrieval beat BM25 out of domain?"), "dense retrieval BM25 out domain");
        assert_eq!(key("Scaling laws hold, for retrieval!", 0), "claim:scaling laws hold for retrieval");
        assert_eq!(key("Scaling laws hold for retrieval", 2022), "claim:2022:scaling laws hold for retrieval");
    }
}
