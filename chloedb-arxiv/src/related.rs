//! A reading list to a related-work section: the papers a reader picked
//! (up to 30) become BibTeX, written from their rows with no model, and a
//! related-work section in LaTeX that cites each of them by its key,
//! written by the model from their abstracts (`llm()` on the server). A
//! citation of a key the list does not hold is taken out, the papers the
//! text leaves uncited are named, and the section is kept (`ax_notes`), so
//! the same list asked again calls no model.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;

use crate::search::{Ctx, COLS};
use crate::strata::{Row, RowExt};

/// Papers a section may cite.
pub const MOST: usize = 30;

pub const WRITE: &str = "You write the related-work section of a research paper from the numbered sources a researcher picked, each given with its citation key, title, authors, year and abstract. \
Group the sources into two to five themes. Write one paragraph per theme, starting with \\paragraph{<theme in a few words>.} and then prose. \
Cite with \\cite{key} using only the keys given, several at once as \\cite{key1,key2}; cite every source at least once. \
Say what each line of work does and how the works differ from each other; give a number only when an abstract states it; never claim more than the abstracts say. \
If the researcher describes their own paper, close each paragraph with one sentence on how their work relates or differs, without inventing results for it. \
Output only the LaTeX of the paragraphs: no section heading, no bullets, no preamble, no closing remarks. Escape %, & and _ outside math, and keep math in $...$.";

/// ASCII letters and digits of a name or word, accents taken off.
fn fold(s: &str) -> String {
    s.nfkd().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

const SMALL: &[&str] = &["a", "an", "the", "on", "of", "for", "to", "in", "towards", "toward", "with", "and", "via", "from", "is", "are", "do", "does", "can", "what", "how", "when", "why", "by", "at"];

/// "Ashish Vaswani" → ("Vaswani", "Ashish"); "Aidan N. Gomez" → ("Gomez", "Aidan N.").
fn split_name(name: &str) -> (String, String) {
    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() < 2 {
        return (name.trim().to_string(), String::new());
    }
    // "Jr." and the like stay with the surname; so do particles ("van der Berg").
    let mut last = parts.len() - 1;
    if parts.len() > 2 && ["Jr.", "Jr", "Sr.", "II", "III", "IV"].contains(&parts[last]) {
        last -= 1;
    }
    let mut start = last;
    while start > 1 && ["van", "von", "der", "de", "den", "da", "di", "del", "la", "le", "du", "dos"].contains(&parts[start - 1]) {
        start -= 1;
    }
    (parts[start..].join(" "), parts[..start].join(" "))
}

/// A citation key: the first author's surname, the year and the first word
/// of the title that says something ("vaswani2017attention").
pub fn key_of(authors: &str, year: i64, title: &str) -> String {
    let first = authors.split(", ").next().unwrap_or("");
    let (last, _) = split_name(first);
    let sur = fold(last.split_whitespace().last().unwrap_or(&last));
    let word = title.split(|c: char| !c.is_alphanumeric()).map(fold).find(|w| !w.is_empty() && !SMALL.contains(&w.as_str())).unwrap_or_default();
    format!("{}{}{}", if sur.is_empty() { "anon".into() } else { sur }, year, word)
}

fn bib_escape(s: &str) -> String {
    // Titles from arXiv are LaTeX already ($...$, \"o); only bare & % # want escaping.
    let mut out = String::with_capacity(s.len());
    let mut prev = ' ';
    for ch in s.chars() {
        if matches!(ch, '&' | '%' | '#') && prev != '\\' {
            out.push('\\');
        }
        out.push(ch);
        prev = ch;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One BibTeX entry of a paper: `@inproceedings` when a venue is known,
/// else `@misc` with its arXiv identifiers.
pub fn entry(r: &Row, key: &str) -> String {
    let authors: Vec<String> = r
        .s("authors")
        .split(", ")
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| {
            let (last, first) = split_name(a);
            if first.is_empty() { last } else { format!("{last}, {first}") }
        })
        .collect();
    let id = r.s("id");
    let venue = r.s("venue");
    let year = if !venue.is_empty() && r.i("venue_year") > 0 { r.i("venue_year") } else { r.i("year") };
    let mut f: Vec<(&str, String)> = vec![("title", format!("{{{}}}", bib_escape(&r.s("title")))), ("author", authors.join(" and ")), ("year", year.to_string())];
    let kind = if venue.is_empty() { "misc" } else { "inproceedings" };
    if !venue.is_empty() {
        f.push(("booktitle", venue.clone()));
    }
    f.push(("eprint", id.clone()));
    f.push(("archivePrefix", "arXiv".into()));
    if !r.s("pcat").is_empty() {
        f.push(("primaryClass", r.s("pcat")));
    }
    if !r.s("doi").is_empty() {
        f.push(("doi", r.s("doi")));
    }
    f.push(("url", format!("https://arxiv.org/abs/{id}")));
    let body: Vec<String> = f.iter().map(|(k, v)| format!("  {k} = {{{v}}}")).collect();
    format!("@{kind}{{{key},\n{}\n}}\n", body.join(",\n"))
}

/// Keys for papers in order, a letter added where two would be the same.
pub fn keys(rows: &[Row]) -> Vec<String> {
    let base: Vec<String> = rows.iter().map(|r| key_of(&r.s("authors"), r.i("year"), &r.s("title"))).collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for k in &base {
        *count.entry(k).or_default() += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    base.iter()
        .map(|k| {
            if count[k.as_str()] == 1 {
                return k.clone();
            }
            let n = seen.entry(k).or_default();
            *n += 1;
            format!("{k}{}", (b'a' + (*n as u8 - 1)) as char)
        })
        .collect()
}

/// The text with every citation of an unknown key taken out, and the
/// keys it cites, in order of first citation.
pub fn check_cites(text: &str, known: &HashSet<String>) -> (String, Vec<String>) {
    let re = regex::Regex::new(r"\\cite[pt]?\{([^}]*)\}").expect("pattern");
    let mut order: Vec<String> = Vec::new();
    let out = re.replace_all(text, |c: &regex::Captures| {
        let keep: Vec<String> = c[1].split(',').map(|k| k.trim().to_string()).filter(|k| known.contains(k)).collect();
        for k in &keep {
            if !order.contains(k) {
                order.push(k.clone());
            }
        }
        if keep.is_empty() { String::new() } else { format!("\\cite{{{}}}", keep.join(",")) }
    });
    // A citation taken out can leave " ." or "  ".
    let out = out.replace(" .", ".").replace(" ,", ",");
    let out = regex::Regex::new(r"[ \t]{2,}").expect("pattern").replace_all(&out, " ").into_owned();
    (out.trim().to_string(), order)
}

/// A stable short hash of a list and a description (FNV-1a).
fn hash(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

/// The key a list's section is kept under.
pub fn note_key(ids: &[String], about: &str) -> String {
    let mut s: Vec<&str> = ids.iter().map(String::as_str).collect();
    s.sort_unstable();
    s.push(about.trim());
    format!("related:{}", hash(&s))
}

/// The arXiv ids written in a text (identifiers, abs and pdf links), in
/// the order written, each once.
pub fn ids_in(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"(?i)\b(\d{4}\.\d{4,5}|[a-z][a-z\-]+(?:\.[A-Z]{2})?/\d{7})(?:v\d+)?\b").expect("pattern");
    let mut out: Vec<String> = Vec::new();
    for c in re.captures_iter(text) {
        let id = c[1].to_string();
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// The papers of a list, in its order, those not found left out.
pub async fn papers(c: &Ctx<'_>, ids: &[String]) -> Vec<Row> {
    let rows = c.soft("the papers of the list", &format!("SELECT {COLS}, abstract FROM ax_papers WHERE id IN (:ids)"), json!({"ids": ids})).await;
    let mut by: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    ids.iter().filter_map(|id| by.remove(id)).collect()
}

/// BibTeX of a list, with no model.
pub fn bib(rows: &[Row]) -> (Vec<String>, String) {
    let k = keys(rows);
    let text: String = rows.iter().zip(&k).map(|(r, k)| entry(r, k)).collect::<Vec<_>>().join("\n");
    (k, text)
}

/// Whether a list's section is kept.
pub async fn is_kept(c: &Ctx<'_>, ids: &[String], about: &str) -> bool {
    !c.gsoft("a kept section?", "SELECT id FROM ax_notes WHERE id = :k", json!({"k": note_key(ids, about)})).await.is_empty()
}

/// The related-work section of a list, kept or written now, with its BibTeX.
pub async fn write(c: &Ctx<'_>, ids: &[String], about: &str, model: &str) -> Result<Value, String> {
    let rows = papers(c, ids).await;
    if rows.len() < 2 {
        return Err("name at least two papers on arXiv".into());
    }
    let (keys, bibtex) = bib(&rows);
    let list: Vec<Value> = rows
        .iter()
        .zip(&keys)
        .map(|(r, k)| json!({"id": r.s("id"), "key": k, "title": r.s("title"), "authors": crate::answer::first_author(r), "year": r.i("year"), "cites": r.i("cites"), "pcat": r.s("pcat")}))
        .collect();
    let nk = note_key(ids, about);
    let kept = c.gsoft("a kept section", "SELECT text, model, at FROM ax_notes WHERE id = :k", json!({"k": nk})).await;
    if let Some(r) = kept.first().filter(|r| !r.s("text").trim().is_empty()) {
        let v: Value = serde_json::from_str(&r.s("text")).unwrap_or(Value::Null);
        return Ok(json!({"papers": list, "bibtex": bibtex, "latex": v["latex"], "cited": v["cited"], "uncited": v["uncited"], "about": about, "model": r.s("model"), "at": r.s("at"), "kept": true}));
    }
    let t0 = std::time::Instant::now();
    let mut ctx = String::new();
    for (i, (r, k)) in rows.iter().zip(&keys).enumerate() {
        ctx.push_str(&format!("[{}] key {k}: “{}” ({}, {})\n{}\n\n", i + 1, r.s("title"), crate::answer::first_author(r), r.i("year"), r.s("abstract").trim()));
    }
    let question = if about.trim().is_empty() { "Write the related-work section from these sources.".to_string() } else { format!("Write the related-work section from these sources. The researcher's own paper: {}", about.trim()) };
    let text = crate::ask::llm(c, "the related-work section (llm() on the server)", model, WRITE, &question, &ctx, 4000).await?;
    let text = text.trim().trim_start_matches("```latex").trim_start_matches("```tex").trim_start_matches("```").trim_end_matches("```").trim().to_string();
    let known: HashSet<String> = keys.iter().cloned().collect();
    let (latex, cited) = check_cites(&text, &known);
    let uncited: Vec<String> = keys.iter().filter(|k| !cited.contains(k)).cloned().collect();
    let ms = (t0.elapsed().as_secs_f64() * 1000.0).round();
    let keep = json!({"latex": latex, "cited": cited, "uncited": uncited});
    let _ = c.g.upsert("ax_notes", vec![json!({"id": nk, "paper": "", "style": "related", "text": keep.to_string(), "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": ms})]).await;
    Ok(json!({"papers": list, "bibtex": bibtex, "latex": latex, "cited": cited, "uncited": uncited, "about": about, "model": model, "ms": ms, "kept": false}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: Value) -> Row {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn keys_are_surname_year_and_word() {
        assert_eq!(key_of("Ashish Vaswani, Noam Shazeer", 2017, "Attention Is All You Need"), "vaswani2017attention");
        assert_eq!(key_of("Heinrich Küttler", 2020, "The NetHack Learning Environment"), "kuttler2020nethack");
        assert_eq!(key_of("Ludwig van Beethoven", 1808, "On a Symphony"), "beethoven1808symphony");
        let a = row(json!({"authors": "Jacob Devlin", "year": 2018, "title": "BERT: Pre-training"}));
        let b = row(json!({"authors": "Jacob Devlin", "year": 2018, "title": "BERT for everything"}));
        let c = row(json!({"authors": "Ada Lovelace", "year": 1843, "title": "Notes"}));
        assert_eq!(keys(&[a, b, c]), vec!["devlin2018berta", "devlin2018bertb", "lovelace1843notes"]);
    }

    #[test]
    fn an_entry_names_its_authors_surname_first() {
        let r = row(json!({"id": "2005.11401", "title": "Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks & more", "authors": "Patrick Lewis, Aidan N. Gomez, Ludwig van Beethoven, Martin Luther King Jr.", "year": 2020, "venue": "NeurIPS", "venue_year": 2020, "pcat": "cs.CL", "doi": ""}));
        let e = entry(&r, "lewis2020retrieval");
        assert!(e.starts_with("@inproceedings{lewis2020retrieval,\n"), "{e}");
        assert!(e.contains("author = {Lewis, Patrick and Gomez, Aidan N. and van Beethoven, Ludwig and King Jr., Martin Luther}"), "{e}");
        assert!(e.contains("title = {{Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks \\& more}}"), "{e}");
        assert!(e.contains("booktitle = {NeurIPS}") && e.contains("eprint = {2005.11401}") && e.contains("url = {https://arxiv.org/abs/2005.11401}"), "{e}");
        let m = entry(&row(json!({"id": "1706.03762", "title": "Attention", "authors": "Ashish Vaswani", "year": 2017, "venue": "", "pcat": "cs.CL", "doi": ""})), "k");
        assert!(m.starts_with("@misc{k,") && m.contains("archivePrefix = {arXiv}"), "{m}");
    }

    #[test]
    fn citations_of_unknown_keys_are_taken_out() {
        let known: HashSet<String> = ["a2020x".to_string(), "b2021y".to_string()].into();
        let (t, order) = check_cites("Retrieval helps \\cite{b2021y, made2099up}. Others disagree \\cite{made2099up}. Both \\citep{a2020x,b2021y}.", &known);
        assert_eq!(t, "Retrieval helps \\cite{b2021y}. Others disagree. Both \\cite{a2020x,b2021y}.");
        assert_eq!(order, vec!["b2021y", "a2020x"]);
        assert_eq!(note_key(&["2".into(), "1".into()], " x "), note_key(&["1".into(), "2".into()], "x"));
        assert_eq!(ids_in("https://arxiv.org/abs/1706.03762v5\n2005.11401, arXiv:1706.03762 and hep-th/9711200 (pdf/2104.08663v2)"), vec!["1706.03762", "2005.11401", "hep-th/9711200", "2104.08663"]);
    }
}
