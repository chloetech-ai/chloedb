//! The test table the Simulator loads and searches: documents drawn as a
//! catalogue's are, so that its queries cost what they would on real
//! rows (word frequencies by Zipf's law, so that common words have long
//! posting lists; topics, which the words, the categories and the vectors
//! follow; years growing, ratings skewed high), and the requests of each
//! kind of [`super::KINDS`] over it.
//!
//! Every document is a function of the seed and its number, so a write
//! that changes one, a key lookup and a similar-items request all name
//! documents without keeping them.

use rand::Rng;
use serde_json::{json, Value as Json};

use super::dist::{stream, Rand, Ranks};

/// The table's name: one no application uses.
pub const TABLE: &str = "strata_sim";

/// Topics, and the categories `c00`..`c63` that follow them.
const TOPICS: usize = 64;
/// Words of the vocabulary, and of each topic's own.
const VOCAB: usize = 30_000;
const TOPIC_WORDS: usize = 300;

/// The words of documents and queries, the topics' words and centres.
pub struct Corpus {
    pub seed: u64,
    pub dim: usize,
    words: Vec<String>,
    global: Ranks,
    topic_rank: Ranks,
    topic_words: Vec<Vec<u32>>,
    topic_pick: Ranks,
    centres: Vec<Vec<f32>>,
}

/// The statement that makes the table.
pub fn create_sql(dim: usize) -> String {
    format!("CREATE TABLE {TABLE} (id KEYWORD PRIMARY KEY, title TEXT, body TEXT, year INT, category KEYWORD, rating FLOAT, emb VECTOR({dim}))")
}

/// The key of document `i`.
pub fn key(i: u64) -> String {
    format!("d{i:08}")
}

fn unit(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    for x in v.iter_mut() {
        *x /= n;
    }
}

fn gauss(rng: &mut Rand) -> f32 {
    // Box-Muller.
    let u1: f64 = rng.gen::<f64>().max(1e-12);
    let u2: f64 = rng.gen();
    ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
}

impl Corpus {
    pub fn new(seed: u64, dim: usize) -> Corpus {
        let mut rng = stream(seed, "words");
        const ON: &[&str] = &["b", "c", "d", "f", "g", "h", "k", "l", "m", "n", "p", "r", "s", "t", "v", "w", "z", "br", "ch", "st", "tr", "pl"];
        const NU: &[&str] = &["a", "e", "i", "o", "u", "ai", "ou", "ea"];
        const CO: &[&str] = &["", "", "", "n", "r", "l", "m", "x"];
        let mut seen = std::collections::HashSet::new();
        let mut words = Vec::with_capacity(VOCAB);
        while words.len() < VOCAB {
            let syll = if rng.gen_bool(0.35) { 3 } else { 2 };
            let mut w = String::new();
            for _ in 0..syll {
                w.push_str(ON[rng.gen_range(0..ON.len())]);
                w.push_str(NU[rng.gen_range(0..NU.len())]);
            }
            w.push_str(CO[rng.gen_range(0..CO.len())]);
            if seen.insert(w.clone()) {
                words.push(w);
            }
        }
        let mut trng = stream(seed, "topics");
        let topic_words = (0..TOPICS).map(|_| (0..TOPIC_WORDS).map(|_| trng.gen_range(40..VOCAB as u32)).collect()).collect();
        let centres = (0..TOPICS)
            .map(|_| {
                let mut v: Vec<f32> = (0..dim).map(|_| gauss(&mut trng)).collect();
                unit(&mut v);
                v
            })
            .collect();
        Corpus { seed, dim, words, global: Ranks::new(VOCAB, 1.05), topic_rank: Ranks::new(TOPICS, 1.0), topic_words, topic_pick: Ranks::new(TOPIC_WORDS, 0.7), centres }
    }

    fn word(&self, rng: &mut Rand, topic: usize, own: f64) -> &str {
        let i = if rng.gen::<f64>() < own { self.topic_words[topic][self.topic_pick.draw(rng)] as usize } else { self.global.draw(rng) };
        &self.words[i]
    }

    fn vector(&self, rng: &mut Rand, topic: usize) -> Vec<f32> {
        let spread = 0.9 / (self.dim as f32).sqrt();
        let mut v: Vec<f32> = self.centres[topic].iter().map(|c| c + spread * gauss(rng)).collect();
        unit(&mut v);
        v
    }

    /// Document `i`: always the same for a seed.
    pub fn doc(&self, i: u64) -> Json {
        let mut rng = stream(self.seed ^ i.wrapping_mul(0x9e37_79b9_7f4a_7c15), "doc");
        let topic = self.topic_rank.draw(&mut rng);
        let len = (30.0 + 40.0 * (-(1.0 - rng.gen::<f64>()).ln())).min(240.0) as usize;
        let body: Vec<&str> = (0..len).map(|_| self.word(&mut rng, topic, 0.35)).collect();
        let title: Vec<&str> = (0..4).map(|_| self.word(&mut rng, topic, 0.8)).collect();
        // Years growing to 2024; ratings skewed high, to the hundredth.
        let year = (2024.0 - (-(1.0 - rng.gen::<f64>()).ln()) * 8.0).max(1990.0) as i64;
        let rating = ((5.0 - (-(1.0 - rng.gen::<f64>()).ln()) * 1.2).max(0.0) * 100.0).round() / 100.0;
        json!({
            "id": key(i), "title": title.join(" "), "body": body.join(" "), "year": year,
            "category": format!("c{topic:02}"), "rating": rating, "emb": self.vector(&mut rng, topic),
        })
    }

    /// The vector of document `i`, without the rest.
    pub fn doc_vector(&self, i: u64) -> Vec<f32> {
        self.doc(i)["emb"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default()
    }

    /// Query `i` of the pool: two to five words of a topic, and a vector near it.
    pub fn query(&self, i: u64) -> (String, Vec<f32>) {
        let mut rng = stream(self.seed ^ i.wrapping_mul(0xbf58_476d_1ce4_e5b9), "query");
        let topic = self.topic_rank.draw(&mut rng);
        let n = rng.gen_range(2..6);
        let words: Vec<&str> = (0..n).map(|_| self.word(&mut rng, topic, 0.7)).collect();
        (words.join(" "), self.vector(&mut rng, topic))
    }
}

/// Which documents and queries a request names: popular ones by Zipf's
/// law, and some queries never sent before.
pub struct Picks {
    queries: Ranks,
    tail_share: f64,
    docs: Ranks,
    next_tail: u64,
    head: u64,
}

impl Picks {
    pub fn new(docs: u64, head: usize, zipf_s: f64, tail_share: f64, docs_zipf: f64) -> Picks {
        Picks { queries: Ranks::new(head.max(1), zipf_s), tail_share, docs: Ranks::new(docs.clamp(1, 10_000_000) as usize, docs_zipf), next_tail: 0, head: head as u64 }
    }
    pub fn query(&mut self, rng: &mut Rand) -> u64 {
        if rng.gen::<f64>() < self.tail_share {
            self.next_tail += 1;
            self.head + self.next_tail
        } else {
            self.queries.draw(rng) as u64
        }
    }
    pub fn doc(&self, rng: &mut Rand) -> u64 {
        self.docs.draw(rng) as u64
    }
}

/// One request of a kind: its statement (with the table's name and the
/// literal knobs written in) and its parameters, or a read by key.
pub enum Ask {
    Sql { sql: String, params: Json },
    Key(String),
}

/// The words of a text, as written.
fn words(t: &str) -> Vec<&str> {
    t.split_whitespace().collect()
}

/// A request of `kind` over `corpus` (None: the kind is not one of [`super::KINDS`]).
pub fn ask(kind: &str, corpus: &Corpus, picks: &mut Picks, rng: &mut Rand) -> Option<Ask> {
    let sql = super::sql_of(kind)?;
    if kind == "key lookup" {
        return Some(Ask::Key(key(picks.doc(rng))));
    }
    let (text, v) = corpus.query(picks.query(rng));
    let ws = words(&text);
    let mut params = serde_json::Map::new();
    if sql.contains(":t") {
        params.insert("t".into(), json!(text));
    }
    if sql.contains(":v") {
        params.insert("v".into(), json!(v));
    }
    if sql.contains(":w") {
        params.insert("w".into(), json!(ws[0]));
    }
    if kind == "phrase" {
        let a = rng.gen_range(0..ws.len() - 1);
        params.insert("p".into(), json!(format!("+\"{} {}\" {}", ws[a], ws[a + 1], text)));
    }
    if kind == "typo" {
        // Two letters of the longest word swapped.
        let (i, w) = ws.iter().enumerate().max_by_key(|(_, w)| w.len()).map(|(i, w)| (i, w.to_string())).unwrap_or((0, String::new()));
        let mut c: Vec<char> = w.chars().collect();
        if c.len() > 3 {
            let j = rng.gen_range(1..c.len() - 1);
            c.swap(j, j + 1);
        }
        let mut out: Vec<String> = ws.iter().map(|x| x.to_string()).collect();
        out[i] = c.into_iter().collect();
        params.insert("x".into(), json!(out.join(" ")));
    }
    if kind == "prefix" {
        let last = ws[ws.len() - 1];
        let pre: String = last.chars().take(3).collect();
        params.insert("pre".into(), json!(format!("{pre}*")));
        params.insert("before".into(), json!(ws[..ws.len() - 1].join(" ")));
    }
    let mut sql = sql.replace(" FROM docs", &format!(" FROM {TABLE}"));
    if kind == "similar items" {
        let item = picks.doc(rng);
        params.insert("i".into(), json!(corpus.doc_vector(item)));
        let seen: Vec<String> = (0..20).map(|_| format!("'{}'", key(picks.doc(rng)))).collect();
        sql = sql.replace(":seen", &seen.join(", "));
    }
    let sql = sql
        .replace(":y", &rng.gen_range(1995..2021).to_string())
        .replace(":fy", "2010")
        .replace(":fc", "'c00', 'c01', 'c02', 'c03'")
        .replace(":fr", "4")
        .replace(":fs", "3");
    Some(Ask::Sql { sql, params: Json::Object(params) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_are_functions_of_their_number() {
        let c = Corpus::new(7, 16);
        assert_eq!(c.doc(42), c.doc(42));
        assert_ne!(c.doc(42)["body"], c.doc(43)["body"]);
        let d = c.doc(5);
        assert_eq!(d["emb"].as_array().unwrap().len(), 16);
        assert!(d["category"].as_str().unwrap().starts_with('c'));
        let y = d["year"].as_i64().unwrap();
        assert!((1990..=2024).contains(&y));
        assert_eq!(c.doc_vector(5).len(), 16);
    }

    #[test]
    fn every_kind_has_a_request() {
        let c = Corpus::new(1, 8);
        let mut p = Picks::new(1000, 100, 0.8, 0.3, 1.0);
        let mut rng = stream(1, "t");
        for (kind, _) in super::super::KINDS {
            for _ in 0..20 {
                match ask(kind, &c, &mut p, &mut rng).unwrap() {
                    Ask::Key(k) => assert!(k.starts_with('d')),
                    Ask::Sql { sql, params } => {
                        assert!(sql.contains(TABLE), "{sql}");
                        assert!(!sql.contains(" docs "), "{sql}");
                        for name in [":seen", ":fy", ":fc", ":fr", ":fs"] {
                            assert!(!sql.contains(name), "{kind}: {sql}");
                        }
                        for (k, _) in params.as_object().unwrap() {
                            assert!(sql.contains(&format!(":{k}")), "{kind}: {k} not in {sql}");
                        }
                    }
                }
            }
        }
    }
}
