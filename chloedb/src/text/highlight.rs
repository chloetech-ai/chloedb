//! Highlights: where in a text a search's words are, as the field's
//! analyzer reads both, so that `running` marks "runs", a synonym marks
//! its other words (`k8s` marks "Kubernetes"), a typo marks the word it
//! meant (`kubernets`), `bro*` marks "brown", and a quoted phrase marks
//! its words where they follow each other. A query word with punctuation
//! inside (`C++`, `std::move`, `.NET`, `k8s.io/api`), which the analyzer
//! would split, is marked where its characters are, as `LIKE` with a
//! substring index finds it.
//!
//! [`highlight`] tags every match in a whole text; [`snippet`] cuts the
//! stretch of about `size` characters that holds the most matches, tagged,
//! with an ellipsis where it was cut.

use std::collections::HashSet;

use super::analyzer::{normalize_word, Analyzer};
use super::fuzzy;
use super::synonyms::SynonymMap;

/// How the matches are written.
#[derive(Clone, Debug)]
pub struct Tags {
    pub pre: String,
    pub post: String,
    /// Escape the text as HTML (with the default tags, `<mark>`).
    pub html: bool,
}

impl Default for Tags {
    fn default() -> Tags {
        Tags { pre: "<mark>".into(), post: "</mark>".into(), html: true }
    }
}

/// What a query asks to mark, read once for many texts.
#[derive(Clone, Debug, Default)]
pub struct Marks {
    /// Sequences of terms: a word is one term, a phrase or a synonym of
    /// several words more.
    seqs: Vec<Vec<String>>,
    /// Prefixes of terms (`bro*`), normalized, not stemmed.
    prefixes: Vec<String>,
    /// Words that may be typos: the normalized word and the edits allowed.
    fuzzy: Vec<(Vec<char>, u8, String)>,
    /// Strings marked where their characters are, lowercased.
    literals: Vec<String>,
    analyzer: Analyzer,
}

/// The edits a word of `n` characters may have and still be the word
/// meant, for marking: none up to four, one up to seven, two past.
fn edits_for(n: usize) -> u8 {
    match n {
        0..=4 => 0,
        5..=7 => 1,
        _ => 2,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// The words of `text`: byte ranges of runs of letters and digits, with an
/// apostrophe inside a word kept (`don't`).
fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let inner_apostrophe = (c == '\'' || c == '’') && start.is_some() && it.peek().is_some_and(|&(_, n)| is_word_char(n));
        if is_word_char(c) || inner_apostrophe {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push((s, i));
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

impl Marks {
    /// The marks of `query`, written as `MATCH` takes it: words, quoted
    /// phrases, `word*`, `word~`; a word after `-` is left out, and
    /// operators (`AND`, `OR`, `NOT`) are not words. `synonyms`, the
    /// field's list for its analyzer, adds what each word also finds.
    pub fn new(query: &str, analyzer: Analyzer, synonyms: Option<&SynonymMap>) -> Marks {
        let mut m = Marks { analyzer, ..Marks::default() };
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        // Tokens: quoted phrases whole, else runs without spaces.
        let mut toks: Vec<(String, bool)> = Vec::new();
        let mut rest = query.trim();
        while !rest.is_empty() {
            let neg = rest.starts_with('-');
            let r = rest.trim_start_matches('-');
            if let Some(body) = r.strip_prefix('"') {
                let end = body.find('"').unwrap_or(body.len());
                if !neg {
                    toks.push((body[..end].to_string(), true));
                }
                rest = body.get(end + 1..).unwrap_or("").trim_start();
                continue;
            }
            let end = r.find(char::is_whitespace).unwrap_or(r.len());
            if !neg {
                toks.push((r[..end].to_string(), false));
            }
            rest = r[end..].trim_start();
        }
        let mut add = |m: &mut Marks, seq: Vec<String>| {
            if !seq.is_empty() && seen.insert(seq.clone()) {
                m.seqs.push(seq);
            }
        };
        for (tok, phrase) in toks {
            if phrase {
                let terms = analyzer.analyze(&tok);
                // A phrase of punctuation and code is marked as written.
                if terms.is_empty() || tok.chars().any(|c| !is_word_char(c) && !c.is_whitespace() && c != '\'' && c != '-') && terms.len() <= 1 {
                    let lit = tok.trim().to_lowercase();
                    if lit.chars().count() >= 2 {
                        m.literals.push(lit);
                    }
                }
                add(&mut m, terms);
                continue;
            }
            if matches!(tok.as_str(), "AND" | "OR" | "NOT" | "&&" | "||") {
                continue;
            }
            let t = tok.trim_start_matches(['(', '+']).trim_end_matches([')', ',', ';', '?', '!']);
            if let Some(p) = t.strip_suffix('*').filter(|p| !p.is_empty() && p.chars().all(is_word_char)) {
                let token = normalize_word(p);
                m.prefixes.push(analyzer.prefix_of(&token));
                // The stems the prefix stands for, as MATCH finds them:
                // "run" in "run tracker" for `running*`.
                m.seqs.extend(super::query::prefix_stems(analyzer, &token).into_iter().map(|t| vec![t]));
                continue;
            }
            let (t, typo) = match t.strip_suffix('~') {
                Some(w) => (w, true),
                None => (t.trim_end_matches(['.', ':']), false),
            };
            // Punctuation inside a word: the analyzer would split it, so it
            // is marked where its characters are (`c++`, `std::move`).
            if t.chars().any(|c| !is_word_char(c) && c != '\'' && c != '’' && c != '-') && t.chars().count() >= 2 {
                m.literals.push(t.to_lowercase());
                continue;
            }
            let terms = analyzer.analyze(t);
            if terms.len() > 1 {
                // A hyphenated or joined word: its parts in a row.
                add(&mut m, terms);
                continue;
            }
            let Some(term) = terms.into_iter().next() else { continue };
            if let Some(s) = synonyms {
                for other in s.get(std::slice::from_ref(&term)) {
                    add(&mut m, other.clone());
                }
            }
            let word = normalize_word(t);
            let k = if typo { edits_for(word.chars().count()).max(1) } else { edits_for(word.chars().count()) };
            if k > 0 {
                m.fuzzy.push((word.chars().collect(), k, term.clone()));
            }
            add(&mut m, vec![term]);
        }
        // Synonyms of phrases of two words or more.
        if let Some(s) = synonyms.filter(|s| s.max_len() > 1) {
            for seq in m.seqs.clone() {
                if seq.len() > 1 {
                    for other in s.get(&seq) {
                        add(&mut m, other.clone());
                    }
                }
            }
        }
        m
    }

    pub fn is_empty(&self) -> bool {
        self.seqs.is_empty() && self.prefixes.is_empty() && self.literals.is_empty()
    }

    /// The byte ranges of `text` to mark, in order, none overlapping.
    pub fn spans(&self, text: &str) -> Vec<(usize, usize)> {
        let ws = words(text);
        // Each word's term (None for a stopword), and its normalized form.
        let toks: Vec<(usize, usize, Option<String>, String)> = ws
            .iter()
            .map(|&(s, e)| {
                let n = normalize_word(&text[s..e]);
                (s, e, self.analyzer.term_of(&n).map(|t| t.into_owned()), n)
            })
            .collect();
        // The positions of terms, stopwords left out, as an index counts them.
        let terms: Vec<usize> = (0..toks.len()).filter(|&i| toks[i].2.is_some()).collect();
        let mut hits: Vec<(usize, usize)> = Vec::new();
        let term_at = |k: usize| toks[terms[k]].2.as_deref().unwrap_or("");
        let mut exact: HashSet<&str> = HashSet::new();
        for k in 0..terms.len() {
            for seq in &self.seqs {
                if k + seq.len() <= terms.len() && seq.iter().enumerate().all(|(j, t)| term_at(k + j) == t) {
                    hits.push((toks[terms[k]].0, toks[terms[k + seq.len() - 1]].1));
                    if seq.len() == 1 {
                        exact.insert(seq[0].as_str());
                    }
                }
            }
            let (s, e, _, n) = &toks[terms[k]];
            if self.prefixes.iter().any(|p| n.starts_with(p.as_str())) {
                hits.push((*s, *e));
            }
        }
        // A word near a query word that is not in the text as written is
        // the word it meant: what `fuzzy => 'auto'` found.
        for (q, k, term) in &self.fuzzy {
            if exact.contains(term.as_str()) {
                continue;
            }
            for &(s, e, ref t, ref n) in &toks {
                if t.is_some() && n.chars().next() == q.first().copied() && n.chars().count() >= 4 && fuzzy::within(q, n, *k).is_some() {
                    hits.push((s, e));
                }
            }
        }
        if !self.literals.is_empty() {
            let low = lower_same_len(text);
            for lit in &self.literals {
                let mut from = 0;
                while let Some(i) = low[from..].find(lit.as_str()) {
                    let s = from + i;
                    let e = s + lit.len();
                    if text.is_char_boundary(s) && text.is_char_boundary(e) {
                        hits.push((s, e));
                    }
                    from = e.max(s + 1);
                    while from < low.len() && !low.is_char_boundary(from) {
                        from += 1;
                    }
                }
            }
        }
        hits.sort_unstable();
        // Overlaps join.
        let mut out: Vec<(usize, usize)> = Vec::with_capacity(hits.len());
        for (s, e) in hits {
            match out.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => out.push((s, e)),
            }
        }
        out
    }
}

/// `text` lowercased where that keeps every character's length in bytes
/// (so a byte offset in it is one in `text`), and as it is elsewhere.
fn lower_same_len(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let mut l = c.to_lowercase();
        match (l.next(), l.next()) {
            (Some(x), None) if x.len_utf8() == c.len_utf8() => out.push(x),
            _ => out.push(c),
        }
    }
    out
}

fn escape_into(out: &mut String, s: &str, html: bool) {
    if !html {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
}

/// `text[from..to]` with the spans in it tagged.
fn tagged(text: &str, from: usize, to: usize, spans: &[(usize, usize)], tags: &Tags) -> String {
    let mut out = String::with_capacity(to - from + 16);
    let mut at = from;
    for &(s, e) in spans {
        if e <= from || s >= to {
            continue;
        }
        let (s, e) = (s.max(from), e.min(to));
        escape_into(&mut out, &text[at..s], tags.html);
        out.push_str(&tags.pre);
        escape_into(&mut out, &text[s..e], tags.html);
        out.push_str(&tags.post);
        at = e;
    }
    escape_into(&mut out, &text[at..to], tags.html);
    out
}

/// The whole of `text` with every match tagged.
pub fn highlight(text: &str, marks: &Marks, tags: &Tags) -> String {
    tagged(text, 0, text.len(), &marks.spans(text), tags)
}

fn floor_char(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The stretch of `text` of about `size` characters that holds the most
/// matches (ties: the earliest), from a word's start to a word's end,
/// tagged, its runs of white space made one space, with `…` where it was
/// cut. A text with no match gives its start. `size` 0 is the whole text.
pub fn snippet(text: &str, marks: &Marks, size: usize, tags: &Tags) -> String {
    let spans = marks.spans(text);
    if size == 0 || text.chars().count() <= size {
        return squeeze(&tagged(text, 0, text.len(), &spans, tags));
    }
    // Bytes are about characters for the window; the cut is on chars.
    let ws = words(text);
    let budget = byte_len_of_chars(text, size);
    let (mut best, mut best_n) = (0usize, 0usize);
    for (i, &(s, _)) in spans.iter().enumerate() {
        let n = spans[i..].iter().take_while(|&&(_, e)| e <= s + budget).count();
        if n > best_n {
            best_n = n;
            best = s;
        }
    }
    // A little before the first match, at a word's start.
    let lead = if best_n == 0 { 0 } else { budget / 5 };
    let mut from = if best_n == 0 { 0 } else { best };
    for &(s, _) in ws.iter().rev() {
        if s <= best && best - s <= lead {
            from = s;
        }
        if s + lead < best {
            break;
        }
    }
    if best_n == 0 {
        from = 0;
    }
    let mut to = from;
    for &(_, e) in &ws {
        if e > from && e <= from + budget {
            to = e;
        }
    }
    if to <= from {
        to = floor_char(text, from + budget);
    }
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.push_str(&tagged(text, from, to, &spans, tags));
    if to < text.trim_end().len() {
        out.push('…');
    }
    squeeze(&out)
}

fn byte_len_of_chars(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}

/// Runs of white space as one space.
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.trim().chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::synonyms::Synonyms;

    fn plain() -> Tags {
        Tags { pre: "[".into(), post: "]".into(), html: false }
    }

    #[test]
    fn marks_stems_prefixes_and_phrases() {
        let m = Marks::new("running bro*", Analyzer::English, None);
        assert_eq!(highlight("He runs. The brown dog is running.", &m, &plain()), "He [runs]. The [brown] dog is [running].");
        // A prefix marks the words that start with it as typed and those
        // of the stems it stands for, as MATCH finds them (DEV-201).
        let m = Marks::new("wireless headphone*", Analyzer::English, None);
        assert_eq!(highlight("Wireless Headphones, a headphone stand", &m, &plain()), "[Wireless] [Headphones], a [headphone] stand");
        let m = Marks::new("runni*", Analyzer::English, None);
        assert_eq!(highlight("running shoes for a run", &m, &plain()), "[running] shoes for a [run]");
        let m = Marks::new("\"memory safety\" -rust", Analyzer::English, None);
        assert_eq!(highlight("Memory safety without Rust; safety first, memory later", &m, &plain()), "[Memory safety] without Rust; safety first, memory later");
        // Stopwords between a phrase's words are not counted, as an index does not.
        let m = Marks::new("\"state of the art\"", Analyzer::English, None);
        assert_eq!(highlight("the state of the art compiler", &m, &plain()), "the [state of the art] compiler");
    }

    #[test]
    fn marks_synonyms_and_typos() {
        let syn = Synonyms::parse("k8s, kubernetes\nml, machine learning").unwrap();
        let m = Marks::new("k8s ml", Analyzer::English, Some(syn.of(Analyzer::English)));
        assert_eq!(highlight("Kubernetes and K8s for machine learning", &m, &plain()), "[Kubernetes] and [K8s] for [machine learning]");
        let m = Marks::new("kubernets postgress", Analyzer::English, None);
        assert_eq!(highlight("Running Postgres on Kubernetes", &m, &plain()), "Running [Postgres] on [Kubernetes]");
        // A short word is not a typo of every word one letter away.
        let m = Marks::new("rust", Analyzer::English, None);
        assert_eq!(highlight("just rust, must bust", &m, &plain()), "just [rust], must bust");
        // A word that is in the text as typed marks only itself.
        let m = Marks::new("models", Analyzer::English, None);
        assert_eq!(highlight("model and modes", &m, &plain()), "[model] and modes");
    }

    #[test]
    fn marks_code_where_its_characters_are() {
        let m = Marks::new("C++ std::move .NET", Analyzer::English, None);
        assert_eq!(highlight("Modern c++ uses std::move; .net does not", &m, &plain()), "Modern [c++] uses [std::move]; [.net] does not");
        let m = Marks::new("\"k8s.io/api\"", Analyzer::English, None);
        assert_eq!(highlight("import k8s.io/api/core", &m, &plain()), "import [k8s.io/api]/core");
    }

    #[test]
    fn escapes_html_with_the_default_tags() {
        let m = Marks::new("tag", Analyzer::English, None);
        assert_eq!(highlight("a <b>tag</b> & more", &m, &Tags::default()), "a &lt;b&gt;<mark>tag</mark>&lt;/b&gt; &amp; more");
    }

    #[test]
    fn snippets_hold_the_most_matches() {
        let text = format!("{} the needle is here {} needle and needle again {}", "filler words ".repeat(30), "more filler ".repeat(30), "end ".repeat(20));
        let m = Marks::new("needle", Analyzer::English, None);
        let s = snippet(&text, &m, 60, &plain());
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert_eq!(s.matches("[needle]").count(), 2, "{s}");
        assert!(s.chars().count() < 80, "{s}");
        // No match: the start.
        let m = Marks::new("absent", Analyzer::English, None);
        assert!(snippet(&text, &m, 40, &plain()).starts_with("filler words"));
        // A short text whole, its white space squeezed.
        let m = Marks::new("b", Analyzer::Simple, None);
        assert_eq!(snippet("a\n\n b  c", &m, 100, &plain()), "a [b] c");
    }
}
