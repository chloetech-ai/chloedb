//! Tiers over the candidates' scores, in the order people expect of a
//! search box (and Algolia's ranking gives HN's): a code word typed as
//! written (`c++`, `c#`, `.net`) in the title, then every word of a short
//! query in the title, as typed (or its stem, or an HN synonym), then with
//! a typo, before stories that match only in their text, their link or by
//! meaning. Within a tier the score decides (BM25, meaning, votes, age).

use std::collections::HashSet;

/// Queries of more words than this are sentences: their stories are
/// ranked by score alone.
const MAX_WORDS: usize = 4;

pub struct Tiers {
    /// Each query word with the words that stand for it (its synonyms).
    words: Vec<Vec<String>>,
    code: Vec<String>,
}

fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// A rough stem: enough that "databases" finds "database" and "running"
/// finds "run" for a tier (the score has the analyzer's own).
fn stem(w: &str) -> &str {
    for suf in ["ing", "ed", "es", "s"] {
        if w.len() > suf.len() + 3 && w.ends_with(suf) {
            return &w[..w.len() - suf.len()];
        }
    }
    w
}

/// Edits between two short words (insert, delete, substitute, swap two
/// neighbours), stopping above `max`.
fn edits(a: &str, b: &str, max: usize) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// How many typos a word may have, as `fuzzy => 'auto'` allows them.
fn allowed(w: &str) -> usize {
    match w.chars().count() {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

impl Tiers {
    pub fn new(words: &str, code: &[String]) -> Tiers {
        let groups: Vec<Vec<String>> = crate::schema::SYNONYMS.lines().map(|l| l.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).collect();
        let code: Vec<String> = code.iter().map(|c| c.to_lowercase()).collect();
        let in_code: HashSet<String> = code.iter().flat_map(|c| tokens(c)).collect();
        let mut ws = Vec::new();
        for w in tokens(words) {
            if w.chars().count() < 2 || crate::complete::is_stop(&w) || in_code.contains(&w) {
                continue;
            }
            let mut alts = vec![w.clone()];
            for g in &groups {
                if g.iter().any(|x| *x == w) {
                    alts.extend(g.iter().filter(|x| **x != w).cloned());
                }
            }
            ws.push(alts);
        }
        if ws.len() > MAX_WORDS {
            ws.clear();
        }
        Tiers { words: ws, code }
    }

    /// The tier of a story: 4 for each code word in its title (2 in its
    /// link), then 2 when the title holds every word, 1 when it holds them
    /// with typos.
    pub fn of(&self, title: &str, url: &str) -> i64 {
        let low = title.to_lowercase();
        let mut t = 0;
        for c in &self.code {
            if low.contains(c.as_str()) {
                t += 4;
            } else if url.to_lowercase().contains(c.as_str()) {
                t += 2;
            }
        }
        if self.words.is_empty() {
            return t;
        }
        let toks = tokens(title);
        let joined = format!(" {} ", toks.join(" "));
        let mut typo = false;
        for alts in &self.words {
            let exact = alts.iter().any(|a| {
                if a.contains(' ') {
                    return joined.contains(&format!(" {a} "));
                }
                toks.iter().any(|x| x == a || (a.len() >= 4 && x.len() >= 4 && stem(x) == stem(a)) || (a.len() >= 5 && x.starts_with(a.as_str()) && x.len() <= a.len() + 2))
            });
            if exact {
                continue;
            }
            let w = &alts[0];
            let n = allowed(w);
            if n > 0 && toks.iter().any(|x| edits(x, w, n) <= n) {
                typo = true;
                continue;
            }
            return t;
        }
        t + if typo { 1 } else { 2 }
    }
}

#[cfg(test)]
mod tests {
    use super::Tiers;

    #[test]
    fn every_word_in_the_title_first() {
        let t = Tiers::new("postgres vs mysql", &[]);
        assert_eq!(t.of("PostgreSQL vs. MySQL in 2024", ""), 2);
        assert_eq!(t.of("Why we moved from MySQL", ""), 0);
        assert_eq!(t.of("Postgers and MySQL", ""), 1);
    }

    #[test]
    fn code_as_typed() {
        let t = Tiers::new("c++ templates", &["c++".to_string()]);
        assert_eq!(t.of("C++ templates explained", ""), 6);
        assert_eq!(t.of("The C programming language", ""), 0);
        assert_eq!(t.of("Modern C++", ""), 4);
    }

    #[test]
    fn synonyms_and_stems() {
        let t = Tiers::new("k8s operators", &[]);
        assert_eq!(t.of("Writing a Kubernetes operator", ""), 2);
        let t = Tiers::new("a b c d e f", &[]);
        assert_eq!(t.of("anything", ""), 0);
    }
}
