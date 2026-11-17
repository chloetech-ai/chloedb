//! What a paper is, read from its words without a model: the techniques it
//! uses and the domains it is about (a dictionary of phrases and acronyms,
//! `data/concepts.tsv`), what kind of paper it is (a survey, a benchmark, a
//! dataset, a tool, a position paper, theory), where it was published (a
//! conference named in its comments, or the journal of its reference), and
//! the links to its code, models and data.
//!
//! The same reading tags the snapshot's papers in `prepare` and each new
//! paper the live feed brings, so a paper is tagged alike whichever way it
//! came.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

pub const CONCEPTS_TSV: &str = include_str!("../data/concepts.tsv");
pub const CATEGORIES_TSV: &str = include_str!("../data/categories.tsv");
pub const GROUPS_TSV: &str = include_str!("../data/groups.tsv");

#[derive(Debug, Clone)]
pub struct Concept {
    pub kind: String,
    pub slug: String,
    pub name: String,
    pub phrases: Vec<String>,
    pub acronyms: Vec<String>,
    /// Category groups whose papers it applies to (empty: any).
    pub groups: Vec<String>,
}

pub struct Dict {
    pub concepts: Vec<Concept>,
    /// A phrase (its words joined by one space) and the concepts it names.
    phrases: HashMap<String, Vec<u16>>,
    acronyms: HashMap<String, Vec<u16>>,
    /// The first words of the phrases of two words or more, so a scan stops
    /// as soon as no phrase can go on.
    prefixes: std::collections::HashSet<String>,
    max_words: usize,
}

pub fn dict() -> &'static Dict {
    static D: OnceLock<Dict> = OnceLock::new();
    D.get_or_init(|| Dict::parse(CONCEPTS_TSV))
}

impl Dict {
    pub fn parse(tsv: &str) -> Dict {
        let mut concepts = Vec::new();
        let mut phrases: HashMap<String, Vec<u16>> = HashMap::new();
        let mut acronyms: HashMap<String, Vec<u16>> = HashMap::new();
        let mut max_words = 1;
        let mut prefixes = std::collections::HashSet::new();
        for line in tsv.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 4 {
                continue;
            }
            let list = |i: usize| -> Vec<String> { f.get(i).map(|s| s.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default() };
            let c = Concept { kind: f[0].to_string(), slug: f[1].to_string(), name: f[2].to_string(), phrases: list(3), acronyms: list(4), groups: list(5) };
            let ix = concepts.len() as u16;
            for p in &c.phrases {
                let words: Vec<String> = words(p);
                max_words = max_words.max(words.len());
                for k in 1..words.len() {
                    prefixes.insert(words[..k].join(" "));
                }
                let e = phrases.entry(words.join(" ")).or_default();
                if !e.contains(&ix) {
                    e.push(ix);
                }
            }
            for a in &c.acronyms {
                let e = acronyms.entry(a.clone()).or_default();
                if !e.contains(&ix) {
                    e.push(ix);
                }
            }
            concepts.push(c);
        }
        Dict { concepts, phrases, acronyms, prefixes, max_words }
    }

    pub fn by_slug(&self, slug: &str) -> Option<&Concept> {
        self.concepts.iter().find(|c| c.slug == slug)
    }

    /// The techniques and the domains a title and an abstract name, best
    /// first: a phrase in the title counts three times; in the abstract, a
    /// phrase of several words or an acronym once is enough, one word must
    /// come twice.
    pub fn tag(&self, title: &str, abstract_: &str, group: &str) -> (Vec<String>, Vec<String>) {
        let mut score: HashMap<u16, (u32, u32, bool)> = HashMap::new();
        let count = |text: &str, title: bool, score: &mut HashMap<u16, (u32, u32, bool)>| {
            let w = words(text);
            for i in 0..w.len() {
                let mut key = String::new();
                for n in 1..=self.max_words.min(w.len() - i) {
                    if n > 1 {
                        key.push(' ');
                    }
                    key.push_str(&w[i + n - 1]);
                    if let Some(cs) = self.phrases.get(&key) {
                        for &c in cs {
                            let e = score.entry(c).or_insert((0, 0, false));
                            if title {
                                e.0 += 1;
                            } else {
                                e.1 += 1;
                            }
                            if n > 1 {
                                e.2 = true;
                            }
                        }
                    }
                    if !self.prefixes.contains(&key) {
                        break;
                    }
                }
            }
            for t in acronym_tokens(text) {
                if let Some(cs) = self.acronyms.get(t) {
                    for &c in cs {
                        let e = score.entry(c).or_insert((0, 0, false));
                        if title {
                            e.0 += 1;
                        } else {
                            e.1 += 1;
                        }
                        e.2 = true;
                    }
                }
            }
        };
        count(title, true, &mut score);
        count(abstract_, false, &mut score);
        let mut picked: Vec<(u32, &Concept)> = score
            .into_iter()
            .filter(|(_, (t, a, strong))| *t > 0 || (*strong && *a > 0) || *a >= 2)
            .map(|(c, (t, a, _))| (3 * t + a, &self.concepts[c as usize]))
            .filter(|(_, c)| c.groups.is_empty() || c.groups.iter().any(|g| g == group))
            .collect();
        picked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.slug.cmp(&b.1.slug)));
        let take = |kind: &str| -> Vec<String> { picked.iter().filter(|(_, c)| c.kind == kind).take(8).map(|(_, c)| c.slug.clone()).collect() };
        (take("technique"), take("domain"))
    }
}

/// Lowercase words, hyphens and slashes inside a word kept (`self-attention`,
/// `ads/cft`), LaTeX's markup dropped.
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t = cur.trim_matches(|c| c == '-' || c == '/' || c == '+');
        if !t.is_empty() {
            out.push(t.to_string());
        }
        cur.clear();
    };
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '/' || ch == '+' {
            for l in ch.to_lowercase() {
                cur.push(l);
            }
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Tokens as written, for acronyms (`LoRA`, `U-Net`, `GPT-4`): letters,
/// digits and inner hyphens.
fn acronym_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '-')).map(|t| t.trim_matches('-')).filter(|t| !t.is_empty())
}

/// The kinds of paper its title and abstract say it is.
pub fn kinds(title: &str, abstract_: &str, primary: &str) -> Vec<String> {
    static R: OnceLock<Vec<(&'static str, Regex, Option<Regex>)>> = OnceLock::new();
    let rules = R.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("a kind's pattern");
        vec![
            ("survey", r(r"(?i)\b(survey|a review|review of|reviews of|overview|tutorial|primer|systematic (literature )?review|lecture notes|state of the art and)\b"), Some(r(r"(?i)\b(this|we) (survey|review)\b|\bin this survey\b|\bcomprehensive (survey|review|overview)\b"))),
            ("benchmark", r(r"(?i)\bbench(mark)?(s|ing)?\b"), Some(r(r"(?i)\bwe (introduce|present|propose|release|construct|build|develop|curate)\b[^.]{0,80}\bbenchmark"))),
            ("dataset", r(r"(?i)\b(dataset|data set|corpus|corpora)\b"), Some(r(r"(?i)\bwe (introduce|present|release|construct|collect|curate|build|create)\b[^.]{0,80}\b(dataset|data set|corpus)\b"))),
            ("tool", r(r"(?i)\b(library|toolkit|toolbox|python package|open-source framework|software package|an open-source)\b"), Some(r(r"(?i)\b(open[- ]source|we release)\b[^.]{0,40}\b(library|toolkit|toolbox|package|framework|software|tool)\b"))),
            ("position", r(r"(?i)^position:|\bposition paper\b|\bmanifesto\b"), Some(r(r"(?i)\bthis position paper\b|\bin this position\b"))),
            ("theory", r(r"(?i)\b(theorem|conjecture|lower bound|upper bound|hardness)\b"), Some(r(r"(?i)\bwe prove\b|\bwe establish\b[^.]{0,40}\b(bound|theorem)|\bwe show that\b[^.]{0,60}\b(np-hard|undecidable|optimal rate)"))),
        ]
    });
    let mut out = Vec::new();
    for (k, t, a) in rules {
        if t.is_match(title) || a.as_ref().is_some_and(|a| a.is_match(abstract_)) {
            out.push(k.to_string());
        }
    }
    // Most of mathematics proves; the kind marks theory elsewhere.
    if primary.starts_with("math.") && !primary.starts_with("math.ST") {
        out.retain(|k| k != "theory");
    }
    out
}

/// A conference a paper's comments or journal reference name, with its
/// year (`NeurIPS 2024`), or the journal of its reference (`Phys. Rev. Lett.`).
pub fn venue(comments: &str, journal_ref: &str, year: i32) -> (String, Option<i32>) {
    static R: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (conf, jy) = R.get_or_init(|| {
        let names = "NeurIPS|NIPS|ICML|ICLR|CVPR|ICCV|ECCV|WACV|BMVC|ACL|EMNLP|NAACL|EACL|COLING|LREC|CoNLL|AAAI|IJCAI|KDD|SIGKDD|WWW|TheWebConf|SIGIR|RecSys|WSDM|CIKM|ECIR|ICDM|ECML|ICRA|IROS|CoRL|RSS|UAI|AISTATS|COLT|ALT|MICCAI|Interspeech|INTERSPEECH|ICASSP|SIGGRAPH|CHI|UIST|CSCW|OSDI|SOSP|NSDI|EuroSys|MLSys|ASPLOS|ISCA|MICRO|HPCA|SIGMOD|VLDB|PVLDB|ICDE|PODS|STOC|FOCS|SODA|ICALP|ITCS|CRYPTO|EUROCRYPT|ASIACRYPT|CCS|NDSS|USENIX Security|PLDI|POPL|OOPSLA|ICSE|FSE|ESEC/FSE|ASE|ISSTA|LICS|CAV|TACAS|SIGCOMM|INFOCOM|MobiCom|DAC|TMLR|JMLR|ACM MM|ICLR Workshop|CoLLAs|AAMAS|ICAPS|KR|ISIT|GECCO";
        (
            Regex::new(&format!(r"\b({names})\b(?:[\s'’-]*(\d{{4}}|\d{{2}})\b)?")).expect("venues"),
            Regex::new(r"^\s*([A-Za-z][A-Za-z.&:,' -]{2,60}?)[\s,]*(?:\d|\(|$)").expect("journals"),
        )
    });
    for text in [comments, journal_ref] {
        if let Some(c) = conf.captures(text) {
            let mut name = c.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            if name == "NIPS" {
                name = "NeurIPS".into();
            }
            if name == "INTERSPEECH" {
                name = "Interspeech".into();
            }
            if name == "SIGKDD" {
                name = "KDD".into();
            }
            if name == "PVLDB" {
                name = "VLDB".into();
            }
            if name == "TheWebConf" {
                name = "WWW".into();
            }
            let y = c.get(2).and_then(|m| m.as_str().parse::<i32>().ok()).map(|y| if y < 100 { 2000 + y } else { y }).filter(|y| (1990..=year + 2).contains(y));
            return (name, y);
        }
    }
    if let Some(c) = jy.captures(journal_ref) {
        let j = c.get(1).map(|m| m.as_str().trim().trim_end_matches([',', ':', '-']).trim()).unwrap_or("");
        if j.len() >= 3 && !j.eq_ignore_ascii_case("proceedings") && !j.eq_ignore_ascii_case("in") {
            return (short_journal(j), None);
        }
    }
    (String::new(), None)
}

fn short_journal(j: &str) -> String {
    let j = j.trim_start_matches("Proceedings of the ").trim_start_matches("Proc. ");
    let mut s: String = j.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.len() > 48 {
        s = s.chars().take(48).collect::<String>().trim_end().to_string();
    }
    s
}

/// Links to a paper's code (`github.com/owner/repo`) and to models or data
/// on Hugging Face, from its abstract and comments.
pub fn links(abstract_: &str, comments: &str) -> (String, String) {
    static R: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (code, hf) = R.get_or_init(|| {
        (
            Regex::new(r"(?i)(?:https?://)?(?:www\.)?((?:github\.com|gitlab\.com|bitbucket\.org|codeberg\.org)/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)").expect("code links"),
            Regex::new(r"(?i)(?:https?://)?(huggingface\.co/(?:datasets/|spaces/)?[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)").expect("hugging face links"),
        )
    });
    let pick = |r: &Regex| -> String {
        for text in [abstract_, comments] {
            // LaTeX's line breaks inside a URL ("github. com/x") are rare; spaces end it.
            if let Some(c) = r.captures(text) {
                let u = c.get(1).map(|m| m.as_str()).unwrap_or("").trim_end_matches(['.', ',', ')', ';', ':', '}', '\'']);
                let u = u.trim_end_matches(".git");
                if u.split('/').count() >= 3 {
                    return format!("https://{u}");
                }
            }
        }
        String::new()
    };
    (pick(code), pick(hf))
}

/// Pages and figures from a comments line ("12 pages, 5 figures").
pub fn pages_figures(comments: &str) -> (Option<i64>, Option<i64>) {
    static R: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (p, f) = R.get_or_init(|| (Regex::new(r"(?i)(\d{1,4})\s*(?:\+\s*\d+\s*)?pages?\b").expect("pages"), Regex::new(r"(?i)(\d{1,3})\s*(?:figures?|figs?\.?)\b").expect("figures")));
    let g = |r: &Regex| r.captures(comments).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse().ok());
    (g(p), g(f))
}

/// The categories: code, name and group.
pub fn categories() -> &'static Vec<(String, String, String)> {
    static C: OnceLock<Vec<(String, String, String)>> = OnceLock::new();
    C.get_or_init(|| {
        CATEGORIES_TSV
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (f.len() >= 3).then(|| (f[0].to_string(), f[1].to_string(), f[2].to_string()))
            })
            .collect()
    })
}

pub fn groups() -> &'static Vec<(String, String)> {
    static G: OnceLock<Vec<(String, String)>> = OnceLock::new();
    G.get_or_init(|| GROUPS_TSV.lines().filter_map(|l| l.split_once('\t').map(|(a, b)| (a.to_string(), b.to_string()))).collect())
}

/// The group of a category: `cs` for `cs.LG`, `hep` for `hep-th`, and the
/// old archives (`solv-int`, `alg-geom`) by their subject.
pub fn group_of(cat: &str) -> String {
    if let Some((_, _, g)) = categories().iter().find(|(c, _, _)| c == cat) {
        return g.clone();
    }
    let head = cat.split('.').next().unwrap_or(cat);
    let g = match head {
        "hep-th" | "hep-ph" | "hep-ex" | "hep-lat" => "hep",
        "nucl-th" | "nucl-ex" => "nucl",
        "alg-geom" | "dg-ga" | "funct-an" | "q-alg" => "math",
        "chao-dyn" | "solv-int" | "patt-sol" | "adap-org" | "comp-gas" => "nlin",
        "mtrl-th" | "supr-con" => "cond-mat",
        "acc-phys" | "ao-sci" | "atom-ph" | "chem-ph" | "plasm-ph" | "bayes-an" => "physics",
        "cmp-lg" => "cs",
        h => h,
    };
    g.to_string()
}

pub fn category_name(cat: &str) -> String {
    categories().iter().find(|(c, _, _)| c == cat).map(|(_, n, _)| n.clone()).unwrap_or_else(|| cat.to_string())
}

/// A person's key: their names folded to ASCII, lowercase, without dots
/// (`yann lecun`, `j r r tolkien`), the same for every way a name is
/// accented or spaced.
pub fn person_key(forenames: &str, last: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let whole = format!("{forenames} {last}");
    let mut out = String::with_capacity(whole.len());
    let mut sp = true;
    for ch in whole.nfkd() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            sp = false;
        } else if ch.is_alphabetic() && !ch.is_ascii() {
            // A letter with no ASCII form (Chinese, Cyrillic) is kept.
            for l in ch.to_lowercase() {
                out.push(l);
            }
            sp = false;
        } else if (ch.is_whitespace() || ch == '.' || ch == '-' || ch == '\'') && !sp {
            out.push(' ');
            sp = true;
        }
    }
    out.trim().to_string()
}

/// A name as it is shown: forenames then last name, LaTeX accents undone.
pub fn display_name(forenames: &str, last: &str, suffix: &str) -> String {
    let mut s = format!("{} {}", unlatex(forenames).trim(), unlatex(last).trim());
    if !suffix.trim().is_empty() {
        s.push(' ');
        s.push_str(suffix.trim());
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// LaTeX's accents and braces as the letters they write (`\"{o}` is ö).
pub fn unlatex(s: &str) -> String {
    if !s.contains('\\') && !s.contains('{') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let acc = chars[i + 1];
            let mut j = i + 2;
            let mut base = None;
            if j < chars.len() && chars[j] == '{' {
                if j + 1 < chars.len() {
                    base = Some(chars[j + 1]);
                }
                j += 2;
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                j += 1;
            } else if j < chars.len() && !matches!(acc, 'a'..='z' | 'A'..='Z') {
                base = Some(chars[j]);
                j += 1;
            }
            let mark = match acc {
                '"' => Some('\u{308}'),
                '\'' => Some('\u{301}'),
                '`' => Some('\u{300}'),
                '^' => Some('\u{302}'),
                '~' => Some('\u{303}'),
                'c' => Some('\u{327}'),
                'v' => Some('\u{30c}'),
                '=' => Some('\u{304}'),
                '.' => Some('\u{307}'),
                'u' => Some('\u{306}'),
                'H' => Some('\u{30b}'),
                'r' => Some('\u{30a}'),
                _ => None,
            };
            match (mark, base) {
                (Some(m), Some(b)) => {
                    use unicode_normalization::UnicodeNormalization;
                    let composed: String = [b, m].iter().collect::<String>().nfc().collect();
                    out.push_str(&composed);
                    i = j;
                    continue;
                }
                _ => {
                    let word: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
                    let rep = match word.as_str() {
                        "o" => "ø",
                        "O" => "Ø",
                        "l" => "ł",
                        "L" => "Ł",
                        "ss" => "ß",
                        "ae" => "æ",
                        "aa" => "å",
                        "i" => "ı",
                        "&" => "&",
                        _ => "",
                    };
                    if !rep.is_empty() {
                        out.push_str(rep);
                        i += 1 + word.len();
                        continue;
                    }
                    i += 1;
                    continue;
                }
            }
        }
        if c != '{' && c != '}' {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// A title or an abstract on one line, its runs of spaces made one.
pub fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_techniques_and_domains() {
        let d = dict();
        let (t, dm) = d.tag("LoRA fine-tuning of large language models for recommendation", "We study parameter-efficient fine-tuning with LoRA for recommender systems and click-through rate prediction.", "cs");
        assert!(t.contains(&"lora".to_string()), "{t:?}");
        assert!(t.contains(&"llm".to_string()), "{t:?}");
        assert!(dm.contains(&"recommender-systems".to_string()), "{dm:?}");
        // One word once in the abstract is not enough.
        let (t, _) = d.tag("On groups", "We consider the bandit.", "math");
        assert!(t.is_empty(), "{t:?}");
        // Quantization of a field is physics, not a model's weights.
        let (t, _) = d.tag("Post-training quantization of gauge fields", "x", "hep");
        assert!(!t.contains(&"quantization".to_string()));
    }

    #[test]
    fn kinds_and_venues() {
        assert!(kinds("A Survey of Retrieval-Augmented Generation", "", "cs.CL").contains(&"survey".to_string()));
        assert!(kinds("X", "We introduce a new benchmark for agents.", "cs.CL").contains(&"benchmark".to_string()));
        assert_eq!(venue("Accepted at NeurIPS 2024, 12 pages", "", 2024), ("NeurIPS".into(), Some(2024)));
        assert_eq!(venue("ICLR'23 camera ready", "", 2023), ("ICLR".into(), Some(2023)));
        assert_eq!(venue("", "Phys. Rev. Lett. 98, 010401 (2007)", 2007).0, "Phys. Rev. Lett.");
        let (c, h) = links("Code is at https://github.com/foo/bar.", "models: huggingface.co/foo/baz");
        assert_eq!(c, "https://github.com/foo/bar");
        assert_eq!(h, "https://huggingface.co/foo/baz");
        assert_eq!(pages_figures("12 pages, 5 figures"), (Some(12), Some(5)));
    }

    #[test]
    fn names() {
        assert_eq!(person_key("Jos\u{e9} A.", "Garc\u{ed}a-L\u{f3}pez"), "jose a garcia lopez");
        assert_eq!(unlatex("G\\\"{o}del"), "G\u{f6}del");
        assert_eq!(unlatex("Erd\\H{o}s"), "Erd\u{151}s");
        assert_eq!(display_name("Yann", "LeCun", ""), "Yann LeCun");
        assert_eq!(group_of("hep-th"), "hep");
        assert_eq!(group_of("cs.LG"), "cs");
        assert_eq!(group_of("solv-int"), "nlin");
    }
}
