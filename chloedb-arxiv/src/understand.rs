//! What a search box asks, read without a model: operators as people type
//! them (`author:hinton cat:cs.LG since:2020 cites>100 has:code`), the same
//! said in a sentence ("surveys of diffusion models since 2022 with code"),
//! and a question's frame ("who writes most about graph neural networks",
//! "how many papers on RLHF per year", "is mamba growing", "most cited
//! papers on retrieval augmented generation", "LoRA vs QLoRA"). What is
//! understood leaves the text and becomes a filter, a sort or a question,
//! and is said back as chips that can be undone; what is left is the topic,
//! searched by words and by meaning.

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use serde::Serialize;

use crate::concepts;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ask {
    /// Papers in an order: `cites`, `date`, `old` (and the first is the
    /// answer when `one`).
    Top { by: String, one: bool },
    /// How many papers, and by year or month.
    Count { per: String },
    /// Who writes on the topic, by `papers` or `cites`.
    People { by: String },
    /// Who a person writes with.
    Coauthors,
    /// The topic year by year, its share of all papers.
    Trend,
    /// Two or more topics side by side, year by year.
    Compare { terms: Vec<String> },
    /// What grows fastest (topics, techniques) in what the filters keep.
    Hot,
    /// Where the topic began: its first papers and its first famous ones.
    Origin,
    /// Surveys and reviews of the topic.
    Survey,
    /// The topic summed up from its papers, each claim cited.
    Summary,
    /// Where papers on the topic are published.
    Venues,
    /// The techniques papers on the topic use.
    Techniques,
    /// The domains a technique is applied to.
    Applications,
    /// A path through a reading list: surveys, foundations, the newest.
    ReadingList,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Note {
    pub what: String,
    pub from: String,
    /// What the chip is (`author`, `category`, `date`, `technique`, ...),
    /// for the page to link it.
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Parsed {
    /// The words left to search by.
    pub text: String,
    pub phrases: Vec<String>,
    pub exclude: Vec<String>,
    /// Text in backticks, found exactly as written by the substring index.
    pub exact: Vec<String>,
    /// People's keys, confirmed.
    pub authors: Vec<String>,
    /// Words that may name a person ("by hinton"): a filter if someone has
    /// that name, which the search checks.
    pub author_maybe: Option<String>,
    pub author_maybe_words: String,
    pub cats: Vec<String>,
    pub area: Option<String>,
    /// Techniques and domains asked for by name (`technique:lora`).
    pub techniques: Vec<String>,
    pub domains: Vec<String>,
    /// Concepts the words name, which boost and are offered as chips.
    pub concepts: Vec<String>,
    pub kinds: Vec<String>,
    pub venue: Option<String>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub cites_min: Option<i64>,
    pub has_code: bool,
    pub topic: Option<i64>,
    /// What is rising: the days of each of the two spans compared (the
    /// last `window` days against the `window` before), a year when none.
    pub window: Option<i64>,
    /// `date`, `cites`, `old` (relevance when none).
    pub sort: Option<String>,
    pub ask: Option<Ask>,
    pub limit: Option<usize>,
    /// An arXiv id typed alone, or "papers like 1706.03762".
    pub paper: Option<String>,
    pub notes: Vec<Note>,
}

impl Parsed {
    fn note(&mut self, what: impl Into<String>, from: impl Into<String>, kind: &str, value: impl Into<String>) {
        self.notes.push(Note { what: what.into(), from: from.into(), kind: kind.into(), value: value.into() });
    }
    pub fn confirm_author(&mut self, key: Option<(String, String)>) {
        let Some(_) = self.author_maybe.take() else { return };
        match key {
            Some((key, name)) => {
                self.note(format!("by {name}"), self.author_maybe_words.clone(), "author", key.clone());
                self.authors.push(key);
            }
            None => {
                self.text = if self.text.is_empty() { self.author_maybe_words.clone() } else { format!("{} {}", self.text, self.author_maybe_words) };
            }
        }
    }
    pub fn has_filters(&self) -> bool {
        !self.authors.is_empty() || !self.cats.is_empty() || self.area.is_some() || !self.techniques.is_empty() || !self.domains.is_empty() || !self.kinds.is_empty() || self.venue.is_some() || self.after.is_some() || self.before.is_some() || self.cites_min.is_some() || self.has_code || self.topic.is_some()
    }
    /// The words to search by: the text and the phrases.
    pub fn words(&self) -> String {
        let mut s = self.text.clone();
        for p in &self.phrases {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(p);
        }
        s.trim().to_string()
    }
    /// The text for MATCH: phrases quoted.
    pub fn match_text(&self) -> String {
        let mut s = self.text.clone();
        for p in &self.phrases {
            s.push_str(&format!(" \"{}\"", p.replace('"', "")));
        }
        s.trim().to_string()
    }
}

fn ymd(y: i32, m: u32, d: u32) -> i64 {
    NaiveDate::from_ymd_opt(y, m.clamp(1, 12), d.clamp(1, 28)).and_then(|d| d.and_hms_opt(0, 0, 0)).map(|t| Utc.from_utc_datetime(&t).timestamp()).unwrap_or(0)
}

/// A date as typed (`2020`, `2020-06`, `2020-06-15`): its first second and
/// the first second after it.
pub fn date_span(s: &str) -> Option<(i64, i64)> {
    let p: Vec<&str> = s.split(['-', '/']).collect();
    let y: i32 = p.first()?.parse().ok()?;
    if !(1991..=2100).contains(&y) {
        return None;
    }
    match p.len() {
        1 => Some((ymd(y, 1, 1), ymd(y + 1, 1, 1))),
        2 => {
            let m: u32 = p[1].parse().ok().filter(|m| (1..=12).contains(m))?;
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            Some((ymd(y, m, 1), ymd(ny, nm, 1)))
        }
        3 => {
            let m: u32 = p[1].parse().ok()?;
            let d: u32 = p[2].parse().ok()?;
            let t = NaiveDate::from_ymd_opt(y, m, d)?.and_hms_opt(0, 0, 0)?;
            let t = Utc.from_utc_datetime(&t).timestamp();
            Some((t, t + 86_400))
        }
        _ => None,
    }
}

fn number(s: &str) -> Option<i64> {
    let t = s.trim_end_matches('+').replace(',', "");
    if let Some(k) = t.strip_suffix(['k', 'K']) {
        return k.parse::<f64>().ok().map(|x| (x * 1000.0) as i64);
    }
    t.parse().ok()
}

fn is_arxiv_id(s: &str) -> bool {
    let s = s.trim_start_matches("arxiv:").trim_start_matches("arXiv:");
    let s = s.split('v').next().unwrap_or(s);
    let new = s.len() >= 9 && s.as_bytes()[4] == b'.' && s[..4].chars().all(|c| c.is_ascii_digit()) && s[5..].chars().all(|c| c.is_ascii_digit());
    let old = s.contains('/') && s.rsplit('/').next().is_some_and(|n| n.len() == 7 && n.chars().all(|c| c.is_ascii_digit()));
    new || old
}

pub fn clean_id(s: &str) -> String {
    let s = s.trim().trim_start_matches("https://arxiv.org/abs/").trim_start_matches("http://arxiv.org/abs/").trim_start_matches("arxiv.org/abs/").trim_start_matches("arxiv:").trim_start_matches("arXiv:");
    // Drop a version: 1706.03762v5.
    match s.rfind('v') {
        Some(i) if i > 4 && s[i + 1..].chars().all(|c| c.is_ascii_digit()) && !s[i + 1..].is_empty() => s[..i].to_string(),
        _ => s.to_string(),
    }
}

const VENUES: &[&str] = &[
    "neurips", "nips", "icml", "iclr", "cvpr", "iccv", "eccv", "wacv", "acl", "emnlp", "naacl", "eacl", "coling", "aaai", "ijcai", "kdd", "www", "sigir", "recsys", "wsdm", "cikm", "ecir", "icra", "iros", "corl", "uai", "aistats", "colt", "miccai",
    "interspeech", "icassp", "siggraph", "chi", "uist", "osdi", "sosp", "nsdi", "sigmod", "vldb", "icde", "stoc", "focs", "soda", "crypto", "eurocrypt", "ccs", "ndss", "pldi", "popl", "icse", "fse", "tmlr", "jmlr", "mlsys", "aamas",
];

fn venue_name(w: &str) -> Option<&'static str> {
    let l = w.to_lowercase();
    let l = l.trim_end_matches(|c: char| c.is_ascii_digit() || c == '\'' || c == '’');
    VENUES.iter().find(|v| **v == l).map(|v| match *v {
        "neurips" | "nips" => "NeurIPS",
        "icml" => "ICML",
        "iclr" => "ICLR",
        "cvpr" => "CVPR",
        "iccv" => "ICCV",
        "eccv" => "ECCV",
        "wacv" => "WACV",
        "acl" => "ACL",
        "emnlp" => "EMNLP",
        "naacl" => "NAACL",
        "eacl" => "EACL",
        "coling" => "COLING",
        "aaai" => "AAAI",
        "ijcai" => "IJCAI",
        "kdd" => "KDD",
        "www" => "WWW",
        "sigir" => "SIGIR",
        "recsys" => "RecSys",
        "wsdm" => "WSDM",
        "cikm" => "CIKM",
        "ecir" => "ECIR",
        "icra" => "ICRA",
        "iros" => "IROS",
        "corl" => "CoRL",
        "uai" => "UAI",
        "aistats" => "AISTATS",
        "colt" => "COLT",
        "miccai" => "MICCAI",
        "interspeech" => "Interspeech",
        "icassp" => "ICASSP",
        "siggraph" => "SIGGRAPH",
        "chi" => "CHI",
        "uist" => "UIST",
        "osdi" => "OSDI",
        "sosp" => "SOSP",
        "nsdi" => "NSDI",
        "sigmod" => "SIGMOD",
        "vldb" => "VLDB",
        "icde" => "ICDE",
        "stoc" => "STOC",
        "focs" => "FOCS",
        "soda" => "SODA",
        "crypto" => "CRYPTO",
        "eurocrypt" => "EUROCRYPT",
        "ccs" => "CCS",
        "ndss" => "NDSS",
        "pldi" => "PLDI",
        "popl" => "POPL",
        "icse" => "ICSE",
        "fse" => "FSE",
        "tmlr" => "TMLR",
        "jmlr" => "JMLR",
        "mlsys" => "MLSys",
        "aamas" => "AAMAS",
        _ => "",
    }).filter(|v| !v.is_empty())
}

fn kind_word(w: &str) -> Option<&'static str> {
    match w {
        "survey" | "surveys" | "review" | "reviews" | "overview" | "overviews" | "tutorial" | "tutorials" => Some("survey"),
        "benchmark" | "benchmarks" => Some("benchmark"),
        "dataset" | "datasets" | "corpus" | "corpora" => Some("dataset"),
        "library" | "libraries" | "toolkit" | "toolkits" | "tool" | "tools" | "software" | "package" | "packages" => Some("tool"),
        "position" => Some("position"),
        "theory" | "theoretical" | "proofs" => Some("theory"),
        _ => None,
    }
}

pub fn kind_name(k: &str) -> &'static str {
    match k {
        "survey" => "surveys and reviews",
        "benchmark" => "benchmarks",
        "dataset" => "datasets",
        "tool" => "tools and libraries",
        "position" => "position papers",
        "theory" => "theory",
        _ => "papers",
    }
}

fn area_word(w: &str) -> Option<&'static str> {
    match w {
        "physics" => Some("physics"),
        "math" | "mathematics" | "maths" => Some("math"),
        "statistics" | "stats" => Some("stat"),
        "economics" | "econ" => Some("econ"),
        "biology" => Some("q-bio"),
        "finance" => Some("q-fin"),
        "astrophysics" | "astronomy" => Some("astro-ph"),
        "cs" => Some("cs"),
        _ => None,
    }
}

/// Fields named in a word that arXiv files under categories rather than an
/// area ("in sociology"): read as those categories when the word ends the
/// subject, so "attention in vision transformers" keeps its words.
fn field_cats(w: &str) -> Option<&'static [&'static str]> {
    Some(match w {
        "sociology" | "sociologists" => &["physics.soc-ph", "cs.SI", "cs.CY"],
        "linguistics" | "nlp" => &["cs.CL"],
        "neuroscience" => &["q-bio.NC"],
        "genomics" => &["q-bio.GN"],
        "epidemiology" => &["q-bio.PE"],
        "chemistry" => &["physics.chem-ph"],
        "cosmology" => &["astro-ph.CO"],
        "education" => &["physics.ed-ph", "cs.CY"],
        "cryptography" | "security" => &["cs.CR"],
        "databases" => &["cs.DB"],
        "networking" => &["cs.NI"],
        "graphics" => &["cs.GR"],
        "hci" => &["cs.HC"],
        "robotics" => &["cs.RO"],
        _ => return None,
    })
}

const PAPER_WORDS: &[&str] = &["paper", "papers", "article", "articles", "preprint", "preprints", "work", "works", "publication", "publications", "research", "studies", "study", "results"];
const FILLER: &[&str] = &["the", "a", "an", "is", "was", "were", "are", "be", "been", "has", "have", "had", "ever", "on", "about", "of", "that", "which", "did", "do", "does", "there", "all", "to", "for", "with", "in", "me", "show", "find", "list", "give", "tell", "please", "regarding", "related", "arxiv", "any", "some", "written", "published", "posted", "so", "far", "what", "whats", "what's", "most", "top", "best", "and", "or", "i", "should", "can", "you", "my"];

/// Reads a search box. `now` is the current Unix time, for "last year".
pub fn parse(raw: &str, now: i64) -> Parsed {
    let raw = raw.trim();
    let mut p = Parsed::default();
    // An arXiv id alone opens the paper.
    if is_arxiv_id(raw) && !raw.contains(' ') {
        p.paper = Some(clean_id(raw));
        p.note("this paper", raw, "paper", clean_id(raw));
        return p;
    }
    // Quoted phrases and backticks.
    let mut rest = String::new();
    let (mut in_q, mut in_tick) = (false, false);
    let mut cur = String::new();
    for c in raw.chars() {
        if c == '`' && !in_q {
            if in_tick {
                let ex = cur.trim().to_string();
                if !ex.is_empty() {
                    p.note(format!("exactly “{ex}”"), format!("`{ex}`"), "exact", ex.clone());
                    p.exact.push(ex);
                }
                cur.clear();
            }
            in_tick = !in_tick;
        } else if in_tick {
            cur.push(c);
        } else if c == '"' || c == '“' || c == '”' {
            if in_q {
                let ph = cur.trim().to_string();
                if !ph.is_empty() {
                    p.phrases.push(ph);
                }
                cur.clear();
            }
            in_q = !in_q;
        } else if in_q {
            cur.push(c);
        } else {
            rest.push(c);
        }
    }
    if (in_q || in_tick) && !cur.trim().is_empty() {
        rest.push(' ');
        rest.push_str(&cur);
    }
    let mut keep: Vec<String> = Vec::new();
    let toks: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].clone();
        i += 1;
        let low = t.to_lowercase();
        if let Some((k, v)) = low.split_once(':').filter(|(k, v)| !k.is_empty() && !v.is_empty() && !k.contains('/') && !k.starts_with("http")) {
            let orig_v = t[k.len() + 1..].to_string();
            let mut took = true;
            match k {
                "author" | "au" | "by" => {
                    // author:"Yann LeCun" came in as a phrase.
                    let name = if orig_v.is_empty() { p.phrases.pop().unwrap_or_default() } else { orig_v.replace(['_', '+'], " ") };
                    p.author_maybe = Some(name.clone());
                    p.author_maybe_words = name;
                }
                "cat" | "category" | "in" if v.contains('.') || concepts::categories().iter().any(|(c, _, _)| c.eq_ignore_ascii_case(v)) => {
                    let code = concepts::categories().iter().find(|(c, _, _)| c.eq_ignore_ascii_case(v)).map(|(c, _, _)| c.clone()).unwrap_or(orig_v.clone());
                    p.note(format!("in {}", concepts::category_name(&code)), t.clone(), "category", code.clone());
                    p.cats.push(code);
                }
                "area" | "group" => {
                    p.note(format!("in {v}"), t.clone(), "area", v);
                    p.area = Some(v.to_string());
                }
                "technique" | "tech" | "method" | "domain" | "concept" => {
                    let slug = v.replace([' ', '_'], "-");
                    if let Some(c) = concepts::dict().by_slug(&slug) {
                        if c.kind == "domain" {
                            p.domains.push(slug.clone());
                        } else {
                            p.techniques.push(slug.clone());
                        }
                        p.note(c.name.clone(), t.clone(), &c.kind, slug);
                    } else {
                        took = false;
                    }
                }
                "kind" | "type" | "is" => match kind_word(v) {
                    Some(kd) => {
                        p.kinds.push(kd.into());
                        p.note(kind_name(kd), t.clone(), "kind", kd);
                    }
                    None => took = false,
                },
                "venue" | "at" => {
                    p.note(format!("at {orig_v}"), t.clone(), "venue", orig_v.clone());
                    p.venue = Some(venue_name(&orig_v).map(str::to_string).unwrap_or(orig_v));
                }
                "has" if v == "code" => {
                    p.has_code = true;
                    p.note("with code", t.clone(), "code", "");
                }
                "topic" => match v.parse() {
                    Ok(n) => {
                        p.topic = Some(n);
                        p.note(format!("topic {n}"), t.clone(), "topic", v);
                    }
                    Err(_) => took = false,
                },
                "after" | "since" | "from" | "year" | "date" | "before" | "until" => match date_span(v) {
                    Some((a, b)) => match k {
                        "after" | "since" | "from" => {
                            p.after = Some(a);
                            p.note(format!("since {v}"), t.clone(), "date", "");
                        }
                        "before" | "until" => {
                            p.before = Some(a);
                            p.note(format!("before {v}"), t.clone(), "date", "");
                        }
                        _ => {
                            p.after = Some(a);
                            p.before = Some(b);
                            p.note(format!("in {v}"), t.clone(), "date", "");
                        }
                    },
                    None => took = false,
                },
                "sort" | "order" => {
                    let s = match v {
                        "date" | "new" | "newest" | "recent" | "latest" => Some("date"),
                        "cites" | "citations" | "cited" | "top" => Some("cites"),
                        "old" | "oldest" => Some("old"),
                        _ => None,
                    };
                    match s {
                        Some(s) => {
                            p.sort = Some(s.into());
                            p.note(sort_name(s), t.clone(), "sort", s);
                        }
                        None => took = false,
                    }
                }
                _ => took = false,
            }
            if took {
                continue;
            }
        }
        // cites>100, citations>=50
        let mut done = false;
        for field in ["cites", "citations", "cited"] {
            if let Some(r) = low.strip_prefix(field) {
                let (op, num) = if let Some(n) = r.strip_prefix(">=") { (">=", n) } else if let Some(n) = r.strip_prefix('>') { (">", n) } else { ("", "") };
                if let (false, Some(n)) = (op.is_empty(), number(num)) {
                    let n = if op == ">" { n + 1 } else { n };
                    p.cites_min = Some(n);
                    p.note(format!("cited {num}+ times"), t.clone(), "cites", n.to_string());
                    done = true;
                }
            }
        }
        if done {
            continue;
        }
        if let Some(w) = t.strip_prefix('-').filter(|w| !w.is_empty() && w.chars().next().is_some_and(char::is_alphanumeric)) {
            p.exclude.push(w.to_string());
            p.note(format!("without “{w}”"), t.clone(), "exclude", w.to_string());
            continue;
        }
        // An arXiv category code written bare: cs.LG, hep-th.
        if concepts::categories().iter().any(|(c, _, _)| c == &t) && (t.contains('.') || t.contains('-')) {
            p.note(format!("in {}", concepts::category_name(&t)), t.clone(), "category", t.clone());
            p.cats.push(t.clone());
            continue;
        }
        keep.push(t);
    }
    sentence(&mut p, keep, now);
    p
}

fn sort_name(s: &str) -> String {
    match s {
        "date" => "newest first".into(),
        "cites" => "most cited first".into(),
        "old" => "oldest first".into(),
        _ => s.into(),
    }
}

#[derive(Clone)]
struct Tok {
    orig: String,
    low: String,
}

fn toks(words: &[String]) -> Vec<Tok> {
    words
        .iter()
        .map(|t| Tok { orig: t.trim_matches(|c: char| matches!(c, '?' | '!' | ',' | ';' | ':')).trim_end_matches('.').to_string(), low: t.to_lowercase().trim_matches(|c: char| matches!(c, '?' | '!' | ',' | ';' | ':')).trim_end_matches('.').replace('’', "'") })
        .filter(|t| !t.low.is_empty())
        .collect()
}

fn lows(t: &[Tok]) -> Vec<&str> {
    t.iter().map(|x| x.low.as_str()).collect()
}

/// The sentence: a question's frame, then dates, citations, kinds, venues,
/// people, areas; the rest is the topic.
fn sentence(p: &mut Parsed, words: Vec<String>, now: i64) {
    let mut t = toks(&words);
    frame(p, &mut t);
    let today = Utc.timestamp_opt(now, 0).single().unwrap_or_default();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let w = t[i].low.clone();
        let next = t.get(i + 1).map(|x| x.low.clone()).unwrap_or_default();
        let next2 = t.get(i + 2).map(|x| x.low.clone()).unwrap_or_default();
        let said = |a: usize, b: usize, t: &[Tok]| t[a..b.min(t.len())].iter().map(|x| x.orig.clone()).collect::<Vec<_>>().join(" ");
        // Dates.
        if matches!(w.as_str(), "since" | "after" | "from" | "before" | "until" | "in" | "during") {
            if let Some((a, b)) = date_span(&next) {
                // "from 2018 to 2022", "2018-2022"
                if (w == "from" || w == "between") && matches!(next2.as_str(), "to" | "and" | "until") {
                    if let Some((_, b2)) = t.get(i + 3).and_then(|x| date_span(&x.low)) {
                        p.after = Some(a);
                        p.before = Some(b2);
                        p.note(format!("{next}–{}", t[i + 3].low), said(i, i + 4, &t), "date", "");
                        i += 4;
                        continue;
                    }
                }
                match w.as_str() {
                    "since" | "after" | "from" => {
                        p.after = Some(if w == "after" { b } else { a });
                        p.note(format!("since {next}"), said(i, i + 2, &t), "date", "");
                    }
                    "before" | "until" => {
                        p.before = Some(a);
                        p.note(format!("before {next}"), said(i, i + 2, &t), "date", "");
                    }
                    _ => {
                        p.after = Some(a);
                        p.before = Some(b);
                        p.note(format!("in {next}"), said(i, i + 2, &t), "date", "");
                    }
                }
                i += 2;
                continue;
            }
        }
        if let Some((a, b)) = w.split_once('-').and_then(|(a, b)| Some((date_span(a)?, date_span(b)?))).filter(|_| w.len() == 9) {
            p.after = Some(a.0);
            p.before = Some(b.1);
            p.note(format!("{w}"), t[i].orig.clone(), "date", "");
            i += 1;
            continue;
        }
        // "last year", "this month", "past 3 years", "last 30 days"
        if matches!(w.as_str(), "last" | "past" | "this" | "recent") {
            let (n, unit, used) = match next.parse::<i64>() {
                Ok(n) => (n, next2.clone(), 3),
                Err(_) => (1, next.clone(), 2),
            };
            let secs = match unit.trim_end_matches('s') {
                "day" => Some(86_400),
                "week" => Some(7 * 86_400),
                "month" => Some(30 * 86_400),
                "year" => Some(365 * 86_400),
                "decade" => Some(3650 * 86_400),
                _ => None,
            };
            if let Some(s) = secs {
                if w == "this" && unit.starts_with("year") {
                    p.after = Some(ymd(today.year(), 1, 1));
                } else if w == "this" && unit.starts_with("month") {
                    p.after = Some(ymd(today.year(), today.month(), 1));
                } else {
                    p.after = Some(now - n * s);
                }
                p.note(said(i, i + used, &t).to_lowercase(), said(i, i + used, &t), "date", "");
                i += used;
                continue;
            }
        }
        if matches!(w.as_str(), "today") {
            p.after = Some(now - 86_400);
            p.note("today", t[i].orig.clone(), "date", "");
            i += 1;
            continue;
        }
        if let Some((a, b)) = date_span(&w).filter(|_| w.len() == 4 && (i == 0 || !matches!(t[i - 1].low.as_str(), "gpt" | "llama" | "top" | "the"))) {
            // A year alone: "transformers 2021". Kept as a word too when it
            // may be a model's name ("gpt 2023" is rare); a bare year filters.
            p.after = Some(a);
            p.before = Some(b);
            p.note(format!("in {w}"), t[i].orig.clone(), "date", "");
            i += 1;
            continue;
        }
        // Recency and citation words.
        if matches!(w.as_str(), "latest" | "newest" | "recent" | "new") && (PAPER_WORDS.contains(&next.as_str()) || next.is_empty() || i == 0) {
            if p.sort.is_none() {
                p.sort = Some("date".into());
                p.note("newest first", t[i].orig.clone(), "sort", "date");
            }
            i += 1;
            continue;
        }
        if (w == "highly" || w == "most" || w == "widely") && next == "cited" {
            if p.sort.is_none() {
                p.sort = Some("cites".into());
                p.note("most cited first", said(i, i + 2, &t), "sort", "cites");
            }
            i += 2;
            continue;
        }
        if matches!(w.as_str(), "influential" | "seminal" | "classic" | "foundational" | "landmark" | "must-read" | "important" | "famous" | "key") && p.sort.is_none() {
            p.sort = Some("cites".into());
            p.note("most cited first", t[i].orig.clone(), "sort", "cites");
            i += 1;
            continue;
        }
        // "with over 100 citations", "cited more than 50 times"
        if matches!(w.as_str(), "over" | "above" | "least" | "than") {
            if let (Some(n), true) = (number(&next), matches!(next2.as_str(), "citations" | "cites" | "times" | "cited")) {
                p.cites_min = Some(n);
                p.note(format!("cited {n}+ times"), said(i, i + 3, &t), "cites", n.to_string());
                i += 3;
                continue;
            }
        }
        // "with code", "code available", "open source"
        if (w == "with" && matches!(next.as_str(), "code" | "github" | "implementation" | "implementations")) || (w == "open" && matches!(next.as_str(), "source" | "sourced")) {
            p.has_code = true;
            p.note("with code", said(i, i + 2, &t), "code", "");
            i += 2;
            continue;
        }
        if w == "code" && matches!(next.as_str(), "available" | "released") {
            p.has_code = true;
            p.note("with code", said(i, i + 2, &t), "code", "");
            i += 2;
            continue;
        }
        // Kinds: "surveys of", "datasets for" (a kind word next to a
        // filler or at the ends of the box).
        if let Some(k) = kind_word(&w) {
            let edge = i == 0 || i + 1 == t.len() || matches!(next.as_str(), "of" | "on" | "for" | "about" | "in") || PAPER_WORDS.contains(&next.as_str());
            if edge && !(k == "tool" && i > 0 && t.len() > 2) {
                if !p.kinds.contains(&k.to_string()) {
                    p.kinds.push(k.into());
                    p.note(kind_name(k), t[i].orig.clone(), "kind", k);
                }
                i += 1;
                if PAPER_WORDS.contains(&next.as_str()) {
                    i += 1;
                }
                continue;
            }
        }
        // Venues: "at NeurIPS", "NeurIPS 2023 papers", "ICLR".
        if let Some(v) = venue_name(&w).filter(|_| t[i].orig.chars().any(|c| c.is_uppercase()) || matches!(t.get(i.wrapping_sub(1)).map(|x| x.low.as_str()), Some("at" | "in" | "from"))) {
            p.venue = Some(v.into());
            p.note(format!("at {v}"), t[i].orig.clone(), "venue", v);
            if out.last().is_some_and(|x| matches!(x.low.as_str(), "at" | "in" | "from")) {
                out.pop();
            }
            if let Some((a, b)) = date_span(&next) {
                p.after = Some(a);
                p.before = Some(b);
                i += 1;
            }
            i += 1;
            continue;
        }
        // Areas: "in physics", "in math"; fields: "in sociology".
        if matches!(w.as_str(), "in" | "within") {
            if let Some(a) = area_word(&next) {
                p.area = Some(a.into());
                p.note(format!("in {next}"), said(i, i + 2, &t), "area", a);
                i += 2;
                continue;
            }
            let ends = i + 2 >= t.len() || matches!(next2.as_str(), "since" | "after" | "before" | "from" | "during" | "this" | "last" | "over" | "in") || date_span(&next2).is_some() || PAPER_WORDS.contains(&next2.as_str());
            if let Some(cs) = field_cats(&next).filter(|_| ends) {
                for c in cs {
                    if !p.cats.iter().any(|x| x == c) {
                        p.cats.push(c.to_string());
                    }
                }
                p.note(format!("in {next}"), said(i, i + 2, &t), "category", cs.join(","));
                i += 2;
                continue;
            }
        }
        // People: "by Geoffrey Hinton", "by lecun".
        if w == "by" && i + 1 < t.len() {
            let mut j = i + 1;
            let mut name = Vec::new();
            while j < t.len() && name.len() < 4 {
                let x = &t[j];
                let looks = x.orig.chars().next().is_some_and(|c| c.is_alphabetic()) && !FILLER.contains(&x.low.as_str()) && date_span(&x.low).is_none() && !PAPER_WORDS.contains(&x.low.as_str());
                if !looks || (name.len() >= 1 && matches!(x.low.as_str(), "since" | "after" | "before" | "in" | "on" | "about" | "with")) {
                    break;
                }
                name.push(x.orig.clone());
                j += 1;
            }
            if !name.is_empty() {
                p.author_maybe = Some(name.join(" "));
                p.author_maybe_words = name.join(" ");
                i = j;
                continue;
            }
        }
        out.push(t[i].clone());
        i += 1;
    }
    // The topic: what is left, its filler at the ends dropped.
    while out.first().is_some_and(|x| FILLER.contains(&x.low.as_str()) || PAPER_WORDS.contains(&x.low.as_str())) {
        out.remove(0);
    }
    while out.last().is_some_and(|x| FILLER.contains(&x.low.as_str()) || PAPER_WORDS.contains(&x.low.as_str())) {
        out.pop();
    }
    // "papers on X" in the middle: drop "papers on".
    let mut text: Vec<String> = Vec::new();
    let mut k = 0;
    while k < out.len() {
        if PAPER_WORDS.contains(&out[k].low.as_str()) && out.get(k + 1).is_some_and(|x| matches!(x.low.as_str(), "on" | "about" | "in" | "of" | "for")) {
            k += 2;
            continue;
        }
        text.push(out[k].orig.clone());
        k += 1;
    }
    p.text = text.join(" ");
    // A question about a field alone ("what's hot in sociology", the "in"
    // taken by the question): the field's categories, not the word.
    if p.ask.is_some() && text.len() == 1 {
        if let Some(cs) = field_cats(&text[0].to_lowercase()) {
            for c in cs {
                if !p.cats.iter().any(|x| x == c) {
                    p.cats.push(c.to_string());
                }
            }
            p.note(text[0].to_lowercase(), text[0].clone(), "category", cs.join(","));
            p.text.clear();
        }
    }
    // "What's hot in robotics in the last 3 months": the span is what is
    // compared (those months against the ones before), not a filter.
    if p.ask == Some(Ask::Hot) && p.before.is_none() {
        if let Some(a) = p.after.take() {
            p.window = Some(((now - a) / 86_400).clamp(7, 3650));
            for n in p.notes.iter_mut().filter(|n| n.kind == "date") {
                n.kind = "window".into();
            }
        }
    }
    // A paper named by its id: "papers like 1706.03762".
    if let Some(id) = p.text.split_whitespace().find(|w| is_arxiv_id(w)) {
        p.paper = Some(clean_id(id));
    }
    // Concepts the words name.
    let (tech, dom) = concepts::dict().tag(&p.words(), "", "cs");
    for s in tech.into_iter().chain(dom) {
        if !p.concepts.contains(&s) {
            p.concepts.push(s);
        }
    }
}

/// A question's frame, found in its words and taken out of them.
fn frame(p: &mut Parsed, t: &mut Vec<Tok>) {
    if t.is_empty() {
        return;
    }
    let l: Vec<String> = lows(t).into_iter().map(str::to_string).collect();
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    let joined = l.join(" ");
    let take = |p: &mut Parsed, t: &mut Vec<Tok>, a: usize, b: usize, ask: Ask, what: &str| {
        let said: String = t[a..b.min(t.len())].iter().map(|x| x.orig.clone()).collect::<Vec<_>>().join(" ");
        t.drain(a..b.min(t.len()));
        p.note(what, said, "ask", "");
        p.ask = Some(ask);
    };
    let has = |w: &str| l.contains(&w);
    // "X vs Y", "X versus Y", "X compared to Y"
    for sep in [" vs ", " vs. ", " versus ", " compared to ", " compared with ", " or "] {
        if sep == " or " && !(joined.starts_with("which") || joined.contains("more popular")) {
            continue;
        }
        if let Some(ix) = joined.find(sep) {
            let left = joined[..ix].trim();
            let right = joined[ix + sep.len()..].trim();
            let strip = |s: &str| -> String {
                let s = s.trim_start_matches("which is more popular").trim_start_matches("which is").trim_start_matches("compare").trim();
                s.trim_end_matches("which is more popular").trim_end_matches('?').trim().to_string()
            };
            let (a, b) = (strip(left), strip(right));
            if !a.is_empty() && !b.is_empty() {
                let mut terms = vec![a];
                for x in b.split([',']).map(str::trim) {
                    for y in x.split(" vs ") {
                        if !y.trim().is_empty() {
                            terms.push(y.trim().to_string());
                        }
                    }
                }
                p.note(format!("{} side by side", terms.join(" · ")), joined.clone(), "ask", "");
                p.ask = Some(Ask::Compare { terms });
                t.clear();
                return;
            }
        }
    }
    let n = l.len();
    // Leading "top 10", "10 most cited".
    let limit = l.iter().take(3).find_map(|w| w.parse::<usize>().ok()).filter(|n| (1..=100).contains(n));
    if let Some(lim) = limit {
        p.limit = Some(lim);
        if let Some(ix) = l.iter().position(|w| w.parse::<usize>().ok() == Some(lim)) {
            t.remove(ix);
        }
        // The rest of the frame without the number: "top 5 authors in X".
        return frame(p, t);
    }
    // People.
    let people_words = ["authors", "author", "researchers", "researcher", "people", "scientists", "experts", "expert", "groups", "labs", "writers"];
    if l[0] == "who" {
        // "who collaborates with X", "who has X worked with"
        if l.iter().any(|w| matches!(*w, "collaborates" | "collaborated" | "coauthors" | "co-authors" | "collaborate")) || joined.contains("worked with") || joined.contains("works with") || joined.contains("writes with") {
            let stop = l.iter().position(|w| matches!(*w, "with" | "collaborates" | "collaborated" | "collaborate")).map(|i| i + 1).unwrap_or(1);
            let stop = if l.get(stop - 1) == Some(&"collaborates") && l.get(stop) == Some(&"with") { stop + 1 } else { stop };
            take(p, t, 0, stop, Ask::Coauthors, "who they write with");
            if let Some(last) = t.last().map(|x| x.low.clone()) {
                if matches!(last.as_str(), "worked" | "work" | "with") {
                    t.pop();
                }
            }
            let name: Vec<String> = t.iter().map(|x| x.orig.clone()).filter(|w| !matches!(w.to_lowercase().as_str(), "has" | "did" | "does" | "worked" | "work" | "with")).collect();
            p.author_maybe = Some(name.join(" "));
            p.author_maybe_words = name.join(" ");
            t.clear();
            return;
        }
        // "who introduced X", "who invented X"
        if l.get(1).is_some_and(|w| matches!(*w, "introduced" | "invented" | "proposed" | "coined" | "created" | "discovered" | "started" | "pioneered")) {
            take(p, t, 0, 2, Ask::Origin, "where it began");
            strip_phrases(t, &["the term", "the phrase", "the word", "the name", "the idea of", "the concept of"]);
            return;
        }
        // "who first used X", "who came up with X"
        let came = l.get(1) == Some(&"came") && l.get(2) == Some(&"up") && l.get(3) == Some(&"with");
        if came || l.get(1) == Some(&"first") && n > 2 {
            take(p, t, 0, if came { 4 } else { 3 }, Ask::Origin, "where it began");
            strip_phrases(t, &["the term", "the phrase", "the word", "the name", "the idea of", "the concept of"]);
            return;
        }
        let by = if has("cited") || has("influential") { "cites" } else { "papers" };
        let stop = l.iter().position(|w| matches!(*w, "about" | "on" | "in" | "for" | "of")).map(|i| i + 1).unwrap_or(1.min(n));
        take(p, t, 0, stop, Ask::People { by: by.into() }, if by == "cites" { "who is cited most on it" } else { "who writes most on it" });
        return;
    }
    if let Some(ix) = l.iter().position(|w| people_words.contains(w)) {
        let before_ok = ix == 0 || l[..ix].iter().all(|w| matches!(*w, "top" | "best" | "leading" | "prolific" | "most" | "cited" | "influential" | "the" | "key" | "main" | "famous" | "important" | "biggest" | "major" | "prominent" | "notable" | "list" | "show" | "find" | "who"));
        let after = l.get(ix + 1).copied().unwrap_or("");
        if before_ok && (matches!(after, "in" | "on" | "of" | "for" | "about" | "working" | "studying" | "doing") || ix + 1 == n) {
            let by = if l[..ix].iter().any(|w| matches!(*w, "cited" | "influential" | "famous")) { "cites" } else { "papers" };
            let mut stop = ix + 1;
            while stop < n && matches!(l[stop], "in" | "on" | "of" | "for" | "about" | "working" | "studying" | "doing") {
                stop += 1;
            }
            take(p, t, 0, stop, Ask::People { by: by.into() }, if by == "cites" { "who is cited most on it" } else { "who writes most on it" });
            return;
        }
    }
    // Coauthors: "coauthors of X", "X's collaborators"
    if let Some(ix) = l.iter().position(|w| matches!(*w, "coauthors" | "co-authors" | "collaborators" | "collaborator")) {
        take(p, t, 0, (ix + 2).min(n), Ask::Coauthors, "who they write with");
        let name: Vec<String> = t.iter().map(|x| x.orig.trim_end_matches("'s").to_string()).collect();
        p.author_maybe = Some(name.join(" "));
        p.author_maybe_words = name.join(" ");
        t.clear();
        return;
    }
    // Counts.
    if l[0] == "how" && l.get(1) == Some(&"many") || l[0] == "number" && l.get(1) == Some(&"of") || l[0] == "count" {
        let per = if joined.contains("per month") || joined.contains("by month") || joined.contains("each month") || joined.contains("monthly") { "month" } else if joined.contains("per year") || joined.contains("by year") || joined.contains("each year") || joined.contains("yearly") || joined.contains("over time") { "year" } else { "" };
        let mut stop = 2.min(n);
        while stop < n && (PAPER_WORDS.contains(&l[stop]) || matches!(l[stop], "are" | "were" | "have" | "been" | "there" | "on" | "about" | "mention" | "mentioning" | "with" | "use" | "using")) {
            stop += 1;
        }
        take(p, t, 0, stop, Ask::Count { per: per.into() }, "how many");
        strip_phrases(t, &["per month", "by month", "each month", "per year", "by year", "each year", "over time", "monthly", "yearly"]);
        return;
    }
    // Origins.
    if (l[0] == "when" && (has("start") || has("begin") || has("began") || has("started") || has("appear") || has("first") || has("introduced") || has("popular")))
        || joined.starts_with("first paper")
        || joined.starts_with("first papers")
        || joined.starts_with("earliest")
        || joined.starts_with("origin of")
        || joined.starts_with("origins of")
        || joined.starts_with("history of")
        || joined.starts_with("which paper introduced")
        || joined.starts_with("what paper introduced")
        || joined.starts_with("first use of")
        || joined.starts_with("first mention of")
        || joined.starts_with("first mentions of")
    {
        let stop = l.iter().position(|w| matches!(*w, "on" | "about" | "of" | "did" | "introduced" | "was" | "is")).map(|i| i + 1).unwrap_or(1);
        take(p, t, 0, stop, Ask::Origin, "where it began");
        strip_words(t, &["start", "begin", "began", "started", "appear", "appeared", "first", "become", "became", "popular", "get", "introduced"]);
        strip_phrases(t, &["the term", "the phrase", "the word", "the name", "the idea of", "the concept of"]);
        return;
    }
    // "where did X come from"
    if l[0] == "where" && matches!(l.get(1), Some(&"did") | Some(&"does") | Some(&"do")) && n > 4 && l[n - 2] == "come" && l[n - 1] == "from" {
        t.truncate(t.len() - 2);
        take(p, t, 0, 2, Ask::Origin, "where it began");
        strip_phrases(t, &["the term", "the phrase", "the word", "the name", "the idea of", "the concept of"]);
        return;
    }
    // Trends.
    if (l[0] == "is" || l[0] == "are") && l.iter().any(|w| matches!(*w, "growing" | "declining" | "dying" | "dead" | "popular" | "hot" | "rising" | "trending" | "still" | "shrinking" | "fading" | "over")) {
        take(p, t, 0, 1, Ask::Trend, "its trend");
        strip_words(t, &["growing", "declining", "dying", "dead", "popular", "hot", "rising", "trending", "still", "shrinking", "fading", "over", "becoming", "more", "less", "getting"]);
        return;
    }
    for lead in ["trend of", "trends in", "trends of", "trend in", "trend for", "popularity of", "growth of", "rise of", "decline of", "evolution of", "history of research on"] {
        if joined.starts_with(lead) {
            let k = lead.split(' ').count();
            take(p, t, 0, k, Ask::Trend, "its trend");
            return;
        }
    }
    if joined.ends_with(" over time") || joined.ends_with(" over the years") || joined.ends_with(" trend") || joined.ends_with(" trends") {
        let k = if joined.ends_with(" over the years") { 3 } else if joined.ends_with(" over time") { 2 } else { 1 };
        take(p, t, n - k, n, Ask::Trend, "its trend");
        return;
    }
    if joined.starts_with("how has") || joined.starts_with("how did") {
        take(p, t, 0, 2, Ask::Trend, "its trend");
        strip_words(t, &["evolved", "changed", "grown", "developed", "evolve", "change", "grow"]);
        return;
    }
    // What is hot.
    if joined.starts_with("what's hot") || joined.starts_with("whats hot") || joined.starts_with("what is hot") || l.iter().take(3).any(|w| matches!(*w, "trending" | "rising" | "emerging" | "hot" | "hottest" | "fastest")) || joined.starts_with("what's new in") || joined.starts_with("what is new in") || joined.starts_with("whats new in") {
        let stop = l.iter().position(|w| matches!(*w, "in" | "on" | "for" | "of" | "about")).map(|i| i + 1).unwrap_or(n.min(3));
        take(p, t, 0, stop, Ask::Hot, "what is growing fastest");
        strip_words(t, &["topics", "techniques", "methods", "papers", "areas", "research", "growing", "fastest", "right", "now", "today"]);
        return;
    }
    // Summaries.
    for lead in ["summarize", "summarise", "summary of", "explain", "what is", "what are", "what's", "whats", "tell me about", "tldr", "tl;dr", "state of the art in", "state of the art for", "state of the art of", "sota in", "sota for", "what do we know about", "overview of research on", "introduction to", "eli5"] {
        if joined.starts_with(lead) && n > lead.split(' ').count() {
            let k = lead.split(' ').count();
            take(p, t, 0, k, Ask::Summary, "summed up from its papers");
            return;
        }
    }
    // Reading lists.
    for lead in ["reading list for", "reading list on", "reading list about", "what should i read about", "what should i read on", "what to read about", "what to read on", "where to start with", "where do i start with", "how to get into", "learning path for", "curriculum for"] {
        if joined.starts_with(lead) && n > lead.split(' ').count() {
            take(p, t, 0, lead.split(' ').count(), Ask::ReadingList, "a reading list");
            return;
        }
    }
    // Surveys.
    for lead in ["survey of", "survey on", "surveys of", "surveys on", "review of", "reviews of", "review papers on", "review papers of", "overview of", "tutorial on", "tutorials on"] {
        if joined.starts_with(lead) && n > lead.split(' ').count() {
            take(p, t, 0, lead.split(' ').count(), Ask::Survey, "surveys and reviews");
            if !p.kinds.contains(&"survey".to_string()) {
                p.kinds.push("survey".into());
            }
            return;
        }
    }
    // Venues.
    if (l[0] == "where" && (has("published") || has("publish") || has("appear") || has("submit"))) || l.iter().take(3).any(|w| matches!(*w, "venues" | "conferences" | "journals" | "venue" | "conference" | "journal")) {
        let stop = l.iter().position(|w| matches!(*w, "for" | "on" | "about" | "in" | "are" | "do" | "is")).map(|i| i + 1).unwrap_or(1);
        take(p, t, 0, stop, Ask::Venues, "where it is published");
        strip_words(t, &["published", "publish", "appear", "papers", "get", "submit", "usually", "most"]);
        return;
    }
    // Techniques used.
    if (matches!(l[0], "what" | "which") && l.get(1).is_some_and(|w| matches!(*w, "techniques" | "methods" | "approaches" | "models" | "architectures" | "algorithms"))) || joined.starts_with("methods for") || joined.starts_with("techniques for") || joined.starts_with("approaches to") || joined.starts_with("approaches for") || joined.starts_with("how do people") || joined.starts_with("how to") {
        let stop = l.iter().position(|w| matches!(*w, "for" | "in" | "to" | "on")).map(|i| i + 1).unwrap_or(2.min(n));
        take(p, t, 0, stop, Ask::Techniques, "the techniques it uses");
        strip_words(t, &["used", "use", "are", "is", "do", "does", "solve", "approach", "tackle"]);
        return;
    }
    // Applications.
    if joined.starts_with("applications of") || joined.starts_with("uses of") || joined.starts_with("where is") || (joined.starts_with("what is") && joined.ends_with("used for")) {
        let stop = l.iter().position(|w| matches!(*w, "of" | "is")).map(|i| i + 1).unwrap_or(1);
        take(p, t, 0, stop, Ask::Applications, "where it is applied");
        strip_words(t, &["used", "applied", "for", "use"]);
        return;
    }
    // Tops: "most cited papers on X", "best papers on X", "latest papers on X".
    frame_after_limit(p, t, take)
}

fn frame_after_limit(p: &mut Parsed, t: &mut Vec<Tok>, take: impl Fn(&mut Parsed, &mut Vec<Tok>, usize, usize, Ask, &str)) {
    let l: Vec<String> = t.iter().map(|x| x.low.clone()).collect();
    let n = l.len();
    if n == 0 {
        return;
    }
    let first = l[0].as_str();
    let paper_at = l.iter().take(4).position(|w| PAPER_WORDS.contains(&w.as_str()));
    let (by, what) = match first {
        "most" | "highly" | "top" | "best" | "influential" | "seminal" | "classic" | "important" | "key" | "foundational" | "famous" | "landmark" | "must-read" | "greatest" => ("cites", "most cited first"),
        "latest" | "newest" | "recent" | "new" => ("date", "newest first"),
        "oldest" | "earliest" | "first" => ("old", "oldest first"),
        _ => return,
    };
    // "most cited papers on X": take the frame up to the paper word and its preposition.
    let mut stop = paper_at.map(|i| i + 1).unwrap_or(if first == "most" && l.get(1).is_some_and(|w| w == "cited") { 2 } else { 1 });
    while stop < n && matches!(l[stop].as_str(), "on" | "about" | "in" | "of" | "for" | "using" | "with" | "cited") {
        stop += 1;
    }
    let one = paper_at.is_some_and(|i| matches!(l[i].as_str(), "paper" | "article" | "preprint" | "work"));
    if p.limit.is_none() && one {
        p.limit = Some(1);
    }
    take(p, t, 0, stop, Ask::Top { by: by.into(), one }, what);
    p.sort = Some(by.into());
}

fn strip_words(t: &mut Vec<Tok>, ws: &[&str]) {
    t.retain(|x| !ws.contains(&x.low.as_str()));
}

fn strip_phrases(t: &mut Vec<Tok>, ps: &[&str]) {
    for ph in ps {
        let w: Vec<&str> = ph.split(' ').collect();
        if let Some(i) = (0..t.len()).find(|&i| i + w.len() <= t.len() && (0..w.len()).all(|k| t[i + k].low == w[k])) {
            t.drain(i..i + w.len());
        }
    }
}

/// Questions the grammar reads, finished from what is typed so far, for
/// the box's suggestions: the templates whose start matches, the topic
/// filled with `topic` (the best completion of the words) when the typed
/// text has none.
pub fn complete(typed: &str, topic: Option<&str>, n: usize) -> Vec<String> {
    let low = typed.trim_start().to_lowercase();
    if low.len() < 2 {
        return Vec::new();
    }
    let topic = topic.unwrap_or("").trim();
    const T: &[&str] = &[
        "most cited papers on {}",
        "latest papers on {}",
        "who writes most about {}",
        "top authors in {}",
        "how many papers on {} per year",
        "is {} growing",
        "trend of {}",
        "first papers on {}",
        "who introduced {}",
        "surveys of {}",
        "summarize {}",
        "what is {}",
        "reading list for {}",
        "what's hot in {}",
        "datasets for {}",
        "benchmarks for {}",
        "{} papers with code",
        "where are {} papers published",
        "what techniques are used for {}",
        "applications of {}",
        "{} vs ",
    ];
    let mut out = Vec::new();
    for tpl in T {
        let head = tpl.split("{}").next().unwrap_or("");
        if head.is_empty() {
            continue;
        }
        if head.starts_with(&low) && !topic.is_empty() {
            out.push(tpl.replace("{}", topic));
        } else if let Some(after) = low.strip_prefix(head) {
            // The frame is typed; the topic is being typed.
            let tail = tpl.split("{}").nth(1).unwrap_or("");
            let typed_topic = after.trim();
            if !typed_topic.is_empty() && !tail.is_empty() && !low.ends_with(tail.trim()) {
                out.push(format!("{head}{typed_topic}{tail}"));
            }
        }
        if out.len() >= n {
            break;
        }
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_794_000_000; // 2026-11

    #[test]
    fn operators() {
        let p = parse("diffusion cat:cs.CV since:2022 cites>100 has:code -video", NOW);
        assert_eq!(p.text, "diffusion");
        assert_eq!(p.cats, vec!["cs.CV"]);
        assert!(p.after.is_some());
        assert_eq!(p.cites_min, Some(101));
        assert!(p.has_code);
        assert_eq!(p.exclude, vec!["video"]);
    }

    #[test]
    fn sentences() {
        let p = parse("surveys of retrieval augmented generation since 2023 with code", NOW);
        assert_eq!(p.kinds, vec!["survey"]);
        assert_eq!(p.text, "retrieval augmented generation");
        assert!(p.has_code && p.after.is_some());
        assert!(p.concepts.contains(&"rag".to_string()), "{:?}", p.concepts);
        let p = parse("graph neural networks by Jure Leskovec", NOW);
        assert_eq!(p.author_maybe.as_deref(), Some("Jure Leskovec"));
        assert_eq!(p.text, "graph neural networks");
        let p = parse("mixture of experts at NeurIPS 2024", NOW);
        assert_eq!(p.venue.as_deref(), Some("NeurIPS"));
        assert_eq!(p.text, "mixture of experts");
    }

    #[test]
    fn fields() {
        let p = parse("What's hot in sociology", 1_790_000_000);
        assert_eq!(p.cats, vec!["physics.soc-ph", "cs.SI", "cs.CY"]);
        assert!(p.words().is_empty(), "{:?}", p.words());
        assert!(matches!(p.ask, Some(Ask::Hot)));
        let p = parse("who writes most in robotics since 2023", 1_790_000_000);
        assert_eq!(p.cats, vec!["cs.RO"]);
        // A field word inside the subject stays a word.
        let p = parse("privacy in security protocols", 1_790_000_000);
        assert!(p.cats.is_empty());
        assert!(p.words().contains("security"), "{:?}", p.words());
    }

    #[test]
    fn rising_span() {
        // The span of a rising question is what is compared, not a filter.
        let p = parse("what's hot in robotics in the last 3 months", 1_790_000_000);
        assert!(matches!(p.ask, Some(Ask::Hot)));
        assert_eq!(p.window, Some(90));
        assert_eq!(p.after, None);
        assert_eq!(p.cats, vec!["cs.RO"]);
        assert!(p.words().is_empty(), "{:?}", p.words());
        // Elsewhere it stays a filter.
        let p = parse("diffusion models in the last 3 months", 1_790_000_000);
        assert_eq!(p.window, None);
        assert!(p.after.is_some());
        assert_eq!(crate::answer::span_words(91), "the last 3 months");
        assert_eq!(crate::answer::span_words(730), "the last 2 years");
    }

    #[test]
    fn questions() {
        let p = parse("who writes most about graph neural networks", NOW);
        assert_eq!(p.ask, Some(Ask::People { by: "papers".into() }));
        assert_eq!(p.text, "graph neural networks");
        let p = parse("how many papers on RLHF per year", NOW);
        assert_eq!(p.ask, Some(Ask::Count { per: "year".into() }));
        assert_eq!(p.text, "RLHF");
        let p = parse("most cited papers on retrieval augmented generation", NOW);
        assert_eq!(p.ask, Some(Ask::Top { by: "cites".into(), one: false }));
        assert_eq!(p.text, "retrieval augmented generation");
        let p = parse("is mamba growing", NOW);
        assert_eq!(p.ask, Some(Ask::Trend));
        assert_eq!(p.text, "mamba");
        let p = parse("LoRA vs QLoRA vs DoRA", NOW);
        assert_eq!(p.ask, Some(Ask::Compare { terms: vec!["lora".into(), "qlora".into(), "dora".into()] }));
        let p = parse("top 5 authors in quantum error correction", NOW);
        assert_eq!(p.limit, Some(5));
        assert_eq!(p.ask, Some(Ask::People { by: "papers".into() }));
        assert_eq!(p.text, "quantum error correction");
        let p = parse("who introduced the transformer", NOW);
        assert_eq!(p.ask, Some(Ask::Origin));
        assert_eq!(p.text, "transformer");
        for (q, want) in [("who coined the term foundation model", "foundation model"), ("who first used chain of thought", "chain of thought"), ("where did retrieval augmented generation come from", "retrieval augmented generation"), ("who came up with dropout", "dropout"), ("first use of the phrase attention is all you need", "attention is all you need")] {
            let p = parse(q, NOW);
            assert_eq!(p.ask, Some(Ask::Origin), "{q}");
            assert_eq!(p.text, want, "{q}");
        }
        let p = parse("summarize speculative decoding", NOW);
        assert_eq!(p.ask, Some(Ask::Summary));
        assert_eq!(p.text, "speculative decoding");
        let p = parse("what's hot in cs.CL", NOW);
        assert_eq!(p.ask, Some(Ask::Hot));
        assert_eq!(p.cats, vec!["cs.CL"]);
        let p = parse("1706.03762v5", NOW);
        assert_eq!(p.paper.as_deref(), Some("1706.03762"));
        let p = parse("who collaborates with Yoshua Bengio", NOW);
        assert_eq!(p.ask, Some(Ask::Coauthors));
        assert_eq!(p.author_maybe.as_deref(), Some("Yoshua Bengio"));
        let p = parse("latest papers on world models", NOW);
        assert_eq!(p.sort.as_deref(), Some("date"));
        assert_eq!(p.text, "world models");
    }

    #[test]
    fn completes() {
        let c = complete("most ci", Some("diffusion models"), 3);
        assert_eq!(c.first().map(String::as_str), Some("most cited papers on diffusion models"));
        let c = complete("how many papers on rlhf", None, 3);
        assert_eq!(c.first().map(String::as_str), Some("how many papers on rlhf per year"));
    }
}
