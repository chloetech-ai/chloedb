//! Highlights: the words of a field's text that its query's words match,
//! as its analyzer reads both, between tags, in fragments of the text
//! around them.

use std::collections::HashSet;

use strata::text::analyzer::{normalize_word, Analyzer};

use super::mapping::Cut;

/// What a highlight asks for.
pub struct Ask<'a> {
    pub pre: &'a str,
    pub post: &'a str,
    /// Characters a fragment holds about (OpenSearch's `fragment_size`).
    pub size: usize,
    /// How many fragments (0: the whole text, highlighted).
    pub count: usize,
    /// Escape the text as HTML (`encoder: html`).
    pub html: bool,
}

/// The terms of query words, as `analyzer` indexes them; a word ending in
/// `*` stays a prefix (`bro*`), which the terms starting with it match.
pub fn terms(words: &[String], analyzer: Analyzer) -> HashSet<String> {
    words
        .iter()
        .filter_map(|w| match w.strip_suffix('*') {
            Some(p) if !p.is_empty() => Some(format!("{}*", normalize_word(p))),
            _ => analyzer.term_of(&normalize_word(w)).map(|t| t.into_owned()),
        })
        .collect()
}

/// Whether the indexed term `t` is one of `terms`, or starts with one of
/// its prefixes.
fn holds(terms: &HashSet<String>, t: &str) -> bool {
    terms.contains(t) || terms.iter().any(|p| p.strip_suffix('*').is_some_and(|p| t.starts_with(p)))
}

fn escape(s: &str, html: bool) -> String {
    if !html {
        return s.to_string();
    }
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// The text from `from` to `to` with the matched spans in it tagged.
fn tagged(text: &str, from: usize, to: usize, hits: &[(usize, usize)], ask: &Ask) -> String {
    let mut out = String::new();
    let mut at = from;
    for &(s, e) in hits.iter().filter(|(s, e)| *s >= from && *e <= to) {
        out.push_str(&escape(&text[at..s], ask.html));
        out.push_str(ask.pre);
        out.push_str(&escape(&text[s..e], ask.html));
        out.push_str(ask.post);
        at = e;
    }
    out.push_str(&escape(&text[at..to], ask.html));
    out
}

/// The fragments of `text` that hold the terms, best first by how many
/// they hold (ties in the order of the text), or none if none does; its
/// words as `cut` and `analyzer` read them.
pub fn fragments(text: &str, terms: &HashSet<String>, analyzer: Analyzer, cut: &Cut, ask: &Ask) -> Vec<String> {
    let spans = cut.spans(text);
    let hits: Vec<(usize, usize)> = spans.iter().copied().filter(|&(s, e)| analyzer.term_of(&normalize_word(&cut.word(&text[s..e]))).is_some_and(|t| holds(terms, t.as_ref()))).collect();
    if hits.is_empty() {
        return Vec::new();
    }
    if ask.count == 0 {
        return vec![tagged(text, 0, text.len(), &hits, ask)];
    }
    // A fragment starts a little before a match, at a word's start, and
    // ends at a word's end about `size` characters on.
    let size = ask.size.max(18);
    let mut frags: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < hits.len() {
        let (s, _) = hits[i];
        let lead = size / 4;
        let mut from = s;
        for &(ws, _) in spans.iter().rev() {
            if ws <= s && s - ws <= lead {
                from = ws;
            }
            if ws < s.saturating_sub(lead) {
                break;
            }
        }
        let limit = from + size;
        let mut to = hits[i].1;
        for &(_, we) in &spans {
            if we > to && we <= limit {
                to = we;
            }
        }
        let mut n = 0;
        while i < hits.len() && hits[i].1 <= to {
            n += 1;
            i += 1;
        }
        if n == 0 {
            i += 1;
            n = 1;
            to = hits[i - 1].1;
        }
        let to = floor_char(text, to);
        frags.push((from, to, n));
    }
    let mut order: Vec<usize> = (0..frags.len()).collect();
    order.sort_by(|&a, &b| frags[b].2.cmp(&frags[a].2).then(a.cmp(&b)));
    order.truncate(ask.count);
    order.into_iter().map(|k| tagged(text, frags[k].0, frags[k].1, &hits, ask)).collect()
}

fn floor_char(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_stemmed_words() {
        let ask = Ask { pre: "<em>", post: "</em>", size: 100, count: 0, html: false };
        let t = terms(&["running".into()], Analyzer::English);
        assert_eq!(fragments("He runs. They ran; she is running.", &t, Analyzer::English, &Cut::default(), &ask), vec!["He <em>runs</em>. They ran; she is <em>running</em>."]);
        let t = terms(&["mug".into()], Analyzer::Simple);
        assert!(fragments("no match here", &t, Analyzer::Simple, &Cut::default(), &ask).is_empty());
        let t = terms(&["quick".into(), "bro*".into()], Analyzer::English);
        assert_eq!(fragments("The quick brown fox", &t, Analyzer::English, &Cut::default(), &ask), vec!["The <em>quick</em> <em>brown</em> fox"]);
        // A word the standard tokenizer keeps whole.
        let std = Cut { join: super::super::mapping::Join::Standard, split: false };
        let t = terms(&[std.apply("merge_segments")], Analyzer::Simple);
        assert_eq!(fragments("run merge_segments, not merge", &t, Analyzer::Simple, &std, &ask), vec!["run <em>merge_segments</em>, not merge"]);
    }

    #[test]
    fn fragments_around_matches() {
        let text = format!("{} needle {} needle {}", "a ".repeat(100), "b ".repeat(100), "c ".repeat(10));
        let t = terms(&["needle".into()], Analyzer::Simple);
        let ask = Ask { pre: "[", post: "]", size: 40, count: 5, html: false };
        let f = fragments(&text, &t, Analyzer::Simple, &Cut::default(), &ask);
        assert_eq!(f.len(), 2, "{f:?}");
        assert!(f.iter().all(|x| x.contains("[needle]") && x.len() < 60), "{f:?}");
    }
}
