//! The traffic simulator's parts that the server and `engine-compare`
//! share: the query kinds (`KINDS`, Strata SQL), a profile of a workload
//! read from TOML ([`profile`]), the seeded draws ([`dist`]), and the
//! profiles of `deploy/compare/profiles` ([`PRESETS`]).
//!
//! `engine-compare plan` and `replay` send a profile's traffic at an
//! engine loaded with a benchmark's collection (see
//! deploy/compare/README.md, "Traffic simulator"). The command center's
//! Simulator sends it at the cluster it runs on, over a table of test
//! documents of its own, and fits a model of the cluster to what it
//! measured ([`run`], [`model`], [`report`]): how much it holds within
//! its SLO, where it saturates first, and what more copies, more shards
//! or other machines would change, and cost.

pub mod data;
pub mod dist;
pub mod model;
pub mod profile;
pub mod report;
pub mod run;

/// The kinds of `strata-server-bench`, then five more (phrase, typo,
/// prefix, similar items, SQL aggregate), as Strata SQL over the table
/// `docs`. `:seen`, `:y` and the filters' values (`:fy`, `:fc`, `:fr`,
/// `:fs`, engine-compare's `Knobs`) are written into the statement as
/// literals (lists, and numbers that vary by query).
pub const KINDS: &[(&str, &str)] = &[
    ("bm25", "SELECT id, score() FROM docs ORDER BY bm25(body, :t, syntax => 'plain') DESC LIMIT 10"),
    ("bm25 + filter", "SELECT id, score() FROM docs WHERE year >= :fy AND category IN (:fc) ORDER BY bm25(body, :t, syntax => 'plain') DESC LIMIT 10"),
    ("vector", "SELECT id, score() FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10"),
    ("vector + filter", "SELECT id, score() FROM docs WHERE rating >= :fr ORDER BY similarity(emb, :v) DESC LIMIT 10"),
    ("hybrid (RRF)", "SELECT id, score() FROM docs ORDER BY rrf(bm25(body, :t, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 10"),
    ("hybrid (weighted)", "SELECT id, score() FROM docs ORDER BY 0.2 * minmax(bm25(body, :t, syntax => 'plain')) + 0.8 * minmax(similarity(emb, :v)) DESC LIMIT 10"),
    ("boosted (bm25 × decay)", "SELECT id, score() FROM docs ORDER BY bm25(body, :t, syntax => 'plain') * decay(year, 2024, 5) DESC LIMIT 10"),
    ("facet", "SELECT category, COUNT(*) FROM docs WHERE MATCH(body, :w) GROUP BY category ORDER BY 2 DESC LIMIT 5"),
    ("key lookup", "SELECT id, title, year, rating FROM docs WHERE id = :k"),
    // The rows with two neighbouring words of the query as a phrase, ranked by all of its words.
    ("phrase", "SELECT id, score() FROM docs ORDER BY bm25(body, :p) DESC LIMIT 10"),
    // The query with two letters of its longest word swapped, each word matching terms an edit or two away.
    ("typo", "SELECT id, score() FROM docs ORDER BY bm25(body, :x, syntax => 'plain', fuzzy => 'auto') DESC LIMIT 10"),
    // Search as you type: the rows with a word starting with the last word's first letters, ranked by the words before it.
    ("prefix", "SELECT id, score() FROM docs WHERE MATCH(body, :pre) ORDER BY bm25(body, :before, syntax => 'plain') DESC LIMIT 10"),
    // A recommender's request: the items nearest an item, rated 3 or more, minus the 20 the user has seen.
    ("similar items", "SELECT id, score() FROM docs WHERE rating >= :fs AND id NOT IN (:seen) ORDER BY similarity(emb, :i) DESC LIMIT 10"),
    // Analytics over the structured fields alone.
    ("sql aggregate", "SELECT category, COUNT(*), AVG(rating) FROM docs WHERE year >= :y GROUP BY category HAVING COUNT(*) > 1000 ORDER BY 2 DESC LIMIT 10"),
];

/// The profiles of `deploy/compare/profiles`, by name: what a search box
/// in an app sends, what retrieval for a model sends, and `run`'s mix.
pub const PRESETS: &[(&str, &str)] = &[
    ("search-app", include_str!("../../deploy/compare/profiles/search-app.toml")),
    ("rag", include_str!("../../deploy/compare/profiles/rag.toml")),
    ("today", include_str!("../../deploy/compare/profiles/today.toml")),
];

/// A preset profile, read.
pub fn preset(name: &str) -> Option<profile::Profile> {
    PRESETS.iter().find(|(n, _)| *n == name).and_then(|(_, t)| profile::Profile::parse(t).ok())
}

/// The SQL of a kind.
pub fn sql_of(kind: &str) -> Option<&'static str> {
    KINDS.iter().find(|(k, _)| *k == kind).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_presets_read() {
        for (name, _) in super::PRESETS {
            let p = super::preset(name).unwrap_or_else(|| panic!("{name} does not read"));
            assert!(!p.mix.is_empty());
        }
    }
}
