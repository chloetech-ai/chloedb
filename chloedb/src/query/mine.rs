//! Synonyms mined from a table, as proposals for its synonym list
//! ([`crate::query::table_synonyms`]): nothing mined applies until a
//! `CREATE SYNONYMS` takes it.
//!
//! * **From vectors** ([`by_vectors`]): the words of a text field's
//!   stored rows, the most frequent first, each embedded by the model of
//!   one of the table's vector fields; two words whose vectors are close
//!   (cosine at least `min_cos`, 0.6) and whose analysed terms differ are
//!   proposed as equivalent (`a, b`), unless they are spelled alike
//!   ([`spelled_alike`]: a model of word pieces puts `adel` by `adela`).
//!   Close vectors also join related
//!   words that are not synonyms (colours, sizes, brands of one kind), so
//!   the proposals are for a person, or a measurement, to keep or drop.
//! * **From a query log** ([`by_log`]): a table of searches with columns
//!   `context` (a session, a user), `ts` (seconds) and `text`. Two
//!   searches of one context at most `window` seconds apart (60) that
//!   differ in one word, at the same place, propose a one-way rule from
//!   the first word to the second (`a => b`): a search rewritten by the
//!   person who typed it. Rules seen in at least `min_support` contexts
//!   (2) are proposed.
//!
//! `Db::mine_synonyms` (and `POST /v1/synonyms/mine` on a server) runs
//! either and answers the proposals with their scores, and the rules as
//! text for `CREATE SYNONYMS ON table AS '...'`.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::text::analyzer::{for_each_word, Analyzer};

/// What mining reads and keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The rows of text read at most.
    pub max_rows: usize,
    /// The words embedded at most, the most frequent first.
    pub max_words: usize,
    /// The rows a word must be in.
    pub min_df: usize,
    /// The least cosine of two words' vectors.
    pub min_cos: f32,
    /// Proposals kept for one word at most.
    pub per_word: usize,
    /// Of a log: seconds between two searches of a context.
    pub window: i64,
    /// Of a log: contexts that must make a rewrite.
    pub min_support: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { max_rows: 50_000, max_words: 3000, min_df: 3, min_cos: 0.6, per_word: 3, window: 60, min_support: 2 }
    }
}

/// The most rows of text a mining reads ([`Options::bounded`]).
pub const MAX_ROWS: usize = 200_000;
/// The most words a mining embeds and compares ([`Options::bounded`]):
/// the comparisons grow as their square.
pub const MAX_WORDS: usize = 5_000;
/// The least cosine a mining takes ([`Options::bounded`]): below it nearly
/// every pair of words passes.
pub const MIN_COS: f32 = 0.3;
/// The most proposals for one word ([`Options::bounded`]).
pub const MAX_PER_WORD: usize = 10;

impl Options {
    /// These options within the bounds a mining keeps to, whoever asks:
    /// [`MAX_ROWS`], [`MAX_WORDS`], [`MIN_COS`] and [`MAX_PER_WORD`].
    pub fn bounded(&self) -> Options {
        Options {
            max_rows: self.max_rows.clamp(1, MAX_ROWS),
            max_words: self.max_words.clamp(1, MAX_WORDS),
            min_df: self.min_df.max(1),
            min_cos: if self.min_cos.is_nan() { MIN_COS } else { self.min_cos.clamp(MIN_COS, 1.0) },
            per_word: self.per_word.clamp(1, MAX_PER_WORD),
            window: self.window.max(0),
            min_support: self.min_support.max(1),
        }
    }
}

/// Where proposals come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The words of the text field, embedded by the model of the table's
    /// vector field `vector`.
    Vectors { vector: String },
    /// The searches of table `log` (columns `context`, `ts`, `text`).
    Log { log: String },
}

/// What to mine: the synonyms of text field `field` of `table` (whose
/// analyzer decides when two words are one term), from `source`.
#[derive(Clone, Debug)]
pub struct Request {
    pub table: String,
    pub field: String,
    pub source: Source,
    pub options: Options,
}

/// A word of a table's text.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    /// Its most frequent spelling, lowercased.
    pub word: String,
    /// Its analysed term.
    pub term: String,
    /// The rows that hold its term.
    pub df: usize,
}

/// One proposed rule.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Proposal {
    pub from: String,
    pub to: String,
    /// `a, b` (both find each other) or `a => b` (a search for `a` also
    /// finds `b`).
    pub rule: String,
    /// The cosine of their vectors, or the contexts that made the rewrite.
    pub score: f32,
    /// How many rows hold each word (from vectors), or searches of each
    /// word (from a log).
    pub from_count: usize,
    pub to_count: usize,
}

/// What [`crate::Db::mine_synonyms`] found.
#[derive(Clone, Debug, Serialize)]
pub struct Mined {
    /// `vectors` or `log`.
    pub source: String,
    pub table: String,
    pub field: String,
    /// The rows (or searches) read, and the distinct words weighed.
    pub rows: usize,
    pub words: usize,
    pub proposals: Vec<Proposal>,
    /// The proposals as a synonym list's text.
    pub rules: String,
    pub took_ms: f64,
}

fn term(analyzer: Analyzer, word: &str) -> Option<String> {
    analyzer.term_of(word).map(|t| t.into_owned())
}

fn worth(word: &str) -> bool {
    word.chars().count() >= 3 && !word.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',')
}

/// The words of `texts` worth weighing, by how many texts hold their
/// term (at least `min_df`), most first, `max_words` at most: stopwords,
/// numbers and words under three characters are left out.
pub fn vocabulary<'a>(texts: impl Iterator<Item = &'a str>, analyzer: Analyzer, opts: &Options) -> (usize, Vec<Word>) {
    // term -> (rows, spelling -> count)
    let mut terms: HashMap<String, (usize, HashMap<String, usize>)> = HashMap::new();
    let mut rows = 0;
    for text in texts.take(opts.max_rows) {
        rows += 1;
        let mut seen = HashSet::new();
        for_each_word(text, |w| {
            if !worth(w) {
                return;
            }
            let Some(t) = term(analyzer, w) else { return };
            let e = terms.entry(t.clone()).or_default();
            *e.1.entry(w.to_string()).or_default() += 1;
            if seen.insert(t) {
                e.0 += 1;
            }
        });
    }
    let mut words: Vec<Word> = terms
        .into_iter()
        .filter(|(_, (df, _))| *df >= opts.min_df)
        .map(|(term, (df, spellings))| {
            let word = spellings.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).map(|(w, _)| w).unwrap_or_default();
            Word { word, term, df }
        })
        .collect();
    words.sort_by(|a, b| b.df.cmp(&a.df).then_with(|| a.word.cmp(&b.word)));
    words.truncate(opts.max_words);
    (rows, words)
}

fn normalized(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / n).collect()
}

/// Whether two words are spelled alike: half or more of their letter
/// trigrams shared (Dice), or one starts the other. A model that reads
/// words by their pieces puts such words close whatever they mean (glenn and
/// glenni, adel and adela), and typo tolerance and stemming already join the
/// ones that mean the same.
pub fn spelled_alike(a: &str, b: &str) -> bool {
    if a.starts_with(b) || b.starts_with(a) {
        return true;
    }
    alike_by(a, b, &grams(a), &grams(b))
}

/// The letter trigrams of `w` that [`spelled_alike`] compares.
fn grams(w: &str) -> HashSet<String> {
    let cs: Vec<char> = format!("^{w}$").chars().collect();
    cs.windows(3).map(|g| g.iter().collect()).collect()
}

/// [`spelled_alike`] of `a` and `b`, their trigrams `x` and `y` given.
fn alike_by(a: &str, b: &str, x: &HashSet<String>, y: &HashSet<String>) -> bool {
    if a.starts_with(b) || b.starts_with(a) {
        return true;
    }
    2.0 * x.intersection(y).count() as f32 >= (x.len() + y.len()) as f32 * 0.5
}

/// Pairs of `words` whose `vectors` (one a word, in order) are close:
/// equivalences, each word in at most `per_word` of them, closest first.
///
/// Each word keeps its `2 * per_word` closest words (a short list, not
/// every pair that passes `min_cos`), so what it holds grows with the
/// words and not their square; the closest pairs of those lists are taken.
pub fn by_vectors(words: &[Word], vectors: &[Vec<f32>], opts: &Options) -> Vec<Proposal> {
    let opts = opts.bounded();
    let vs: Vec<Vec<f32>> = vectors.iter().map(|v| normalized(v)).collect();
    let gs: Vec<HashSet<String>> = words.iter().map(|w| grams(&w.word)).collect();
    let keep = opts.per_word * 2;
    // Each word's closest: (cosine, other word), the least close first.
    let mut best: Vec<Vec<(f32, usize)>> = vec![Vec::new(); words.len()];
    let least = |l: &Vec<(f32, usize)>| if l.len() < keep { f32::NEG_INFINITY } else { l[0].0 };
    let put = |l: &mut Vec<(f32, usize)>, c: f32, j: usize| {
        let at = l.partition_point(|&(d, _)| d < c);
        l.insert(at, (c, j));
        if l.len() > keep {
            l.remove(0);
        }
    };
    for i in 0..words.len() {
        for j in i + 1..words.len() {
            if words[i].term == words[j].term || vs[i].len() != vs[j].len() {
                continue;
            }
            let c: f32 = vs[i].iter().zip(&vs[j]).map(|(a, b)| a * b).sum();
            if c < opts.min_cos || (c <= least(&best[i]) && c <= least(&best[j])) {
                continue;
            }
            if alike_by(&words[i].word, &words[j].word, &gs[i], &gs[j]) {
                continue;
            }
            if c > least(&best[i]) {
                put(&mut best[i], c, j);
            }
            if c > least(&best[j]) {
                put(&mut best[j], c, i);
            }
        }
    }
    let mut pairs: Vec<(f32, usize, usize)> = best.iter().enumerate().flat_map(|(i, l)| l.iter().map(move |&(c, j)| (c, i.min(j), i.max(j)))).collect();
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| (a.1, a.2).cmp(&(b.1, b.2))));
    pairs.dedup_by(|a, b| (a.1, a.2) == (b.1, b.2));
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| (a.1, a.2).cmp(&(b.1, b.2))));
    let mut used = vec![0usize; words.len()];
    let mut terms: HashSet<(&str, &str)> = HashSet::new();
    let mut out = Vec::new();
    for (c, i, j) in pairs {
        let (ti, tj) = (words[i].term.as_str(), words[j].term.as_str());
        if used[i] >= opts.per_word || used[j] >= opts.per_word || !terms.insert((ti.min(tj), ti.max(tj))) {
            continue;
        }
        used[i] += 1;
        used[j] += 1;
        let (a, b) = (&words[i], &words[j]);
        out.push(Proposal { from: a.word.clone(), to: b.word.clone(), rule: format!("{}, {}", a.word, b.word), score: c, from_count: a.df, to_count: b.df });
    }
    out
}

/// One search of a log: its context, when (seconds) and its text.
pub type Search = (String, i64, String);

/// One-word rewrites of searches in `log` ([module docs](self)), seen in
/// at least `min_support` contexts, most seen first.
pub fn by_log(log: &[Search], analyzer: Analyzer, opts: &Options) -> (usize, Vec<Proposal>) {
    let mut by_context: BTreeMap<&str, Vec<(i64, Vec<String>)>> = BTreeMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (ctx, ts, text) in log {
        let mut words = Vec::new();
        for_each_word(text, |w| words.push(w.to_string()));
        for w in &words {
            *counts.entry(w.clone()).or_default() += 1;
        }
        by_context.entry(ctx).or_default().push((*ts, words));
    }
    // (from, to) -> the contexts that rewrote it
    let mut rewrites: HashMap<(String, String), HashSet<&str>> = HashMap::new();
    for (ctx, searches) in &mut by_context {
        searches.sort_by_key(|s| s.0);
        for w in searches.windows(2) {
            let ((t0, a), (t1, b)) = (&w[0], &w[1]);
            if t1 - t0 > opts.window || a.len() != b.len() {
                continue;
            }
            let differ: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
            let [i] = differ.as_slice() else { continue };
            let (from, to) = (&a[*i], &b[*i]);
            if !worth(from) || !worth(to) || term(analyzer, from).is_none() || term(analyzer, to).is_none() || term(analyzer, from) == term(analyzer, to) {
                continue;
            }
            rewrites.entry((from.clone(), to.clone())).or_default().insert(ctx);
        }
    }
    let mut out: Vec<Proposal> = rewrites
        .into_iter()
        .filter(|(_, cs)| cs.len() >= opts.min_support)
        .map(|((from, to), cs)| Proposal {
            rule: format!("{from} => {to}"),
            score: cs.len() as f32,
            from_count: counts.get(&from).copied().unwrap_or(0),
            to_count: counts.get(&to).copied().unwrap_or(0),
            from,
            to,
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.rule.cmp(&b.rule)));
    (counts.len(), out)
}

/// The text of a synonym list of `proposals`, after a comment `header`.
pub fn rules(header: &str, proposals: &[Proposal]) -> String {
    let mut out = format!("# {header}\n");
    for p in proposals {
        out.push_str(&p.rule);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_keep_to_their_bounds() {
        let o = Options { max_rows: 10_000_000, max_words: 20_000, min_df: 0, min_cos: -1.0, per_word: 100, window: -5, min_support: 0 }.bounded();
        assert_eq!((o.max_rows, o.max_words, o.min_df, o.per_word, o.window, o.min_support), (MAX_ROWS, MAX_WORDS, 1, MAX_PER_WORD, 0, 1));
        assert_eq!(o.min_cos, MIN_COS);
        assert_eq!(Options { min_cos: f32::NAN, ..Options::default() }.bounded().min_cos, MIN_COS);
        assert_eq!(Options::default().bounded(), Options::default());
    }

    #[test]
    fn vocabulary_counts_rows_by_term() {
        let texts = ["Blue sofa and two sofas", "a sofa bed", "Couch, blue", "the couch 2024", "couch"];
        let opts = Options { min_df: 2, ..Options::default() };
        let (rows, words) = vocabulary(texts.into_iter(), Analyzer::English, &opts);
        assert_eq!(rows, 5);
        let got: Vec<(&str, usize)> = words.iter().map(|w| (w.word.as_str(), w.df)).collect();
        assert_eq!(got, [("couch", 3), ("blue", 2), ("sofa", 2)]);
    }

    #[test]
    fn each_word_keeps_its_closest_and_no_more() {
        let w = |word: &str| Word { word: word.into(), term: term(Analyzer::English, word).unwrap(), df: 3 };
        // Two groups of three; within each, the first two closest.
        let words = [w("sofa"), w("couch"), w("settee"), w("lamp"), w("light"), w("lantern")];
        let vectors = vec![vec![1.0, 0.0, 0.0], vec![0.99, 0.1, 0.0], vec![0.9, 0.3, 0.0], vec![0.0, 0.0, 1.0], vec![0.0, 0.1, 0.99], vec![0.0, 0.3, 0.9]];
        let one = Options { per_word: 1, min_cos: 0.5, ..Options::default() };
        let ps = by_vectors(&words, &vectors, &one);
        assert_eq!(ps.iter().map(|p| p.rule.as_str()).collect::<Vec<_>>(), ["sofa, couch", "lamp, light"], "{ps:?}");
        let two = Options { per_word: 2, ..one };
        let rules: Vec<String> = by_vectors(&words, &vectors, &two).into_iter().map(|p| p.rule).collect();
        assert_eq!(rules.len(), 6, "each word in two pairs: {rules:?}");
        assert!(rules.iter().all(|r| !(r.contains("sofa") && r.contains("lamp"))), "{rules:?}");
    }

    #[test]
    fn close_vectors_and_rewrites_become_rules() {
        let w = |word: &str, df| Word { word: word.into(), term: term(Analyzer::English, word).unwrap(), df };
        let words = [w("sofa", 9), w("couch", 7), w("lamp", 5), w("sofas", 2)];
        let vectors = vec![vec![1.0, 0.1], vec![0.95, 0.15], vec![0.0, 1.0], vec![1.0, 0.1]];
        let ps = by_vectors(&words, &vectors, &Options::default());
        assert_eq!(ps.iter().map(|p| p.rule.as_str()).collect::<Vec<_>>(), ["sofa, couch"], "sofas is sofa's term, and their pair one rule: {ps:?}");
        assert!(ps[0].score > 0.99);
        assert!(!spelled_alike("b10", "b11") && spelled_alike("adel", "adela") && spelled_alike("glenn", "glenni") && !spelled_alike("sofa", "couch") && !spelled_alike("tv", "television"));
        let log: Vec<Search> = [("u1", 0, "red sofa"), ("u1", 10, "red couch"), ("u2", 100, "sofa"), ("u2", 130, "couch"), ("u3", 0, "sofa"), ("u3", 500, "couch"), ("u4", 0, "lamp"), ("u4", 5, "lamps")]
            .into_iter()
            .map(|(c, t, s)| (c.to_string(), t, s.to_string()))
            .collect();
        let (words, ps) = by_log(&log, Analyzer::English, &Options::default());
        assert_eq!(words, 5);
        assert_eq!(ps.iter().map(|p| (p.rule.as_str(), p.score)).collect::<Vec<_>>(), [("sofa => couch", 2.0)], "u3 waited too long, lamps is lamp: {ps:?}");
        assert_eq!(rules("mined", &ps), "# mined\nsofa => couch\n");
    }
}
