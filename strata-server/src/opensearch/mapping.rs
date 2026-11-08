//! An OpenSearch index as a Strata table: its mapping's fields, the
//! table's columns, and documents both ways.
//!
//! A table of an index holds `_id` (its key), `_version`, `_hash` (the
//! id's hash, an order of the documents that scrolls page through and
//! that breaks ties of sorts) and `_source`, a
//! JSON field with the document, whose index answers `term`, `terms`,
//! `range`, `exists`, `prefix` and the aggregations on every keyword,
//! number and boolean, mapped or not. Fields that need more get columns of
//! their own, named by their path: `text` a TEXT column (its analyzer
//! `english` or `simple`), `date` a TIMESTAMP column, `knn_vector` a
//! VECTOR column, and a number an INT or FLOAT column too (its smallest
//! value, for scores: a ranking reads columns, not `_source`). A text
//! field without a keyword sub-field and a vector
//! are kept in their columns alone, not in `_source`, and put back into
//! the document read. A text field whose analyzer makes n-grams
//! (`edge_ngram`, `ngram`) holds them in its column, each one a word, so
//! that BM25 over them counts as Lucene's does.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value as Json};

use super::OsError;

/// The table that holds the indexes' names, mappings and aliases.
pub const META: &str = "opensearch_indices";

/// The key, version and document columns of an index's table.
pub const ID: &str = "_id";
pub const VERSION: &str = "_version";
pub const HASH: &str = "_hash";
pub const SOURCE: &str = "_source";
/// The number fields of which a document holds more than one value (an
/// array): their columns hold the smallest, so that aggregations read
/// such a field's values from `_source` ([`Index::single_numbers`]).
pub const MULTI: &str = "_multi";

/// A document's place in its index's order ([`HASH`]): its id's xxh3,
/// 63 bits.
pub fn hash_of(id: &str) -> i64 {
    (twox_hash::XxHash3_64::oneshot(id.as_bytes()) >> 1) as i64
}

/// The JSON field's limits ([`strata::json`]): a document past them is
/// refused with the limit it passed, before the write.
const MAX_DEPTH: usize = 16;
const MAX_PATH: usize = 256;
const MAX_VALUES: usize = 1000;

/// What a field of a mapping is, as far as queries are concerned.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A TEXT column of this analyzer, holding the text or its n-grams,
    /// and how its analyzer and its search analyzer cut words ([`Cut`]).
    Text { analyzer: &'static str, grams: Option<Grams>, cut: Cut, query_cut: Cut },
    /// A string in `_source`.
    Keyword,
    /// A number in `_source` (`long`, `integer`, `short`, `byte`), and
    /// its smallest value in an INT column.
    Long,
    /// A number in `_source` (`double`, `float`, `half_float`,
    /// `scaled_float`), and its smallest value in a FLOAT column.
    Double,
    /// A boolean in `_source`.
    Bool,
    /// A TIMESTAMP column (the value as given stays in `_source` too).
    Date,
    /// A VECTOR column of this many dimensions.
    Vector { dim: usize },
    /// Kept in `_source` and found by its terms there (`ip`, `geo_point`,
    /// `flat_object`...).
    Other,
}

/// A field a query can name: `title`, `title.keyword`, `user.name`.
#[derive(Clone, Debug)]
pub struct Field {
    pub kind: Kind,
    /// The type as the mapping names it (`text`, `long`).
    pub ty: String,
    /// The document path its value comes from: the field's own, or for a
    /// sub-field (`title.keyword`) its parent's.
    pub source: String,
}

impl Field {
    /// The column that holds it, if it has one.
    pub fn column(&self, name: &str) -> Option<String> {
        matches!(self.kind, Kind::Text { .. } | Kind::Date | Kind::Vector { .. } | Kind::Long | Kind::Double).then(|| name.to_string())
    }
}

/// The n-grams an analyzer makes (an `edge_ngram` or `ngram` tokenizer or
/// token filter), which its field's column holds as words.
#[derive(Clone, Debug, PartialEq)]
pub struct Grams {
    pub min: usize,
    pub max: usize,
    /// Only a token's first characters (`edge_ngram`).
    pub edge: bool,
    /// How the text is cut into the tokens grams are made of.
    pub split: Split,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Split {
    /// The whole text is one token (an n-gram tokenizer without
    /// `token_chars`, or a `keyword` tokenizer).
    Whole,
    /// Words, as Strata's analyzers cut them (`standard` and the others).
    Words,
    /// Runs of characters of these classes (an n-gram tokenizer's
    /// `token_chars`: `letter`, `digit`, `whitespace`, `punctuation`,
    /// `symbol`, and `custom` with these characters).
    Classes(Vec<String>, String),
}

/// What a character that Strata's analyzers would cut a word at becomes
/// inside a gram or a word kept whole ([`Cut`]), so that it stays one
/// word (`john s` is `john𠀠s`): an ideograph of CJK Extension B, a
/// letter to Strata that text all but never holds, chosen by the
/// character, so that `a_b` and `a b` stay two words.
pub fn joined(c: char) -> char {
    char::from_u32(0x20000 + (c as u32) % 0xA6D7).unwrap_or('\u{20000}')
}

impl Grams {
    /// The grams of `text`, as words of a TEXT column.
    pub fn of(&self, text: &str) -> String {
        let tokens: Vec<Vec<char>> = match &self.split {
            Split::Whole => vec![text.chars().collect()],
            Split::Words => {
                let mut out = Vec::new();
                strata::text::analyzer::for_each_word(text, |w| out.push(w.chars().collect()));
                out
            }
            Split::Classes(classes, custom) => {
                let keep = |c: char| {
                    classes.iter().any(|k| match k.as_str() {
                        "letter" => c.is_alphabetic(),
                        "digit" => c.is_numeric(),
                        "whitespace" => c.is_whitespace(),
                        "punctuation" => c.is_ascii_punctuation() && !"$+<=>^`|~".contains(c),
                        "symbol" => "$+<=>^`|~".contains(c) || (!c.is_ascii() && !c.is_alphanumeric() && !c.is_whitespace()),
                        "custom" => custom.contains(c),
                        _ => false,
                    })
                };
                text.chars().collect::<Vec<char>>().split(|c| !keep(*c)).filter(|t| !t.is_empty()).map(|t| t.to_vec()).collect()
            }
        };
        let mut out = String::new();
        for t in &tokens {
            let n = t.len();
            let starts = if self.edge { 0..1 } else { 0..n };
            for s in starts {
                for len in self.min..=self.max {
                    if s + len > n {
                        break;
                    }
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.extend(t[s..s + len].iter().map(|&c| {
                        let word = if c.is_ascii() { c.is_ascii_alphanumeric() } else { c.is_alphanumeric() };
                        if word { c } else { joined(c) }
                    }));
                }
            }
        }
        out
    }
}

/// An index: its name, table and mapping.
#[derive(Clone, Debug)]
pub struct Index {
    pub name: String,
    pub table: String,
    pub uuid: String,
    pub created_ms: i64,
    /// The mapping as stored (`{"properties": {...}, "dynamic": ...}`).
    pub mappings: Json,
    pub settings: Json,
    pub aliases: Vec<String>,
    /// Every field a query may name, by name.
    pub fields: BTreeMap<String, Field>,
    /// The number fields no document holds an array of, when a search has
    /// asked ([`MULTI`]): aggregations read their columns, else `_source`.
    pub single_numbers: Option<std::sync::Arc<std::collections::BTreeSet<String>>>,
}

impl Index {
    pub fn new(name: &str, table: &str, uuid: &str, created_ms: i64, mappings: Json, settings: Json, aliases: Vec<String>) -> Result<Index, OsError> {
        let mut fields = BTreeMap::new();
        if let Some(props) = mappings.get("properties") {
            walk_properties(props, "", &settings, &mut fields)?;
        }
        Ok(Index { name: name.into(), table: table.into(), uuid: uuid.into(), created_ms, mappings, settings, aliases, fields, single_numbers: None })
    }

    /// The index's `dynamic` setting: `true` (the default), `false` or
    /// `strict`.
    pub fn dynamic(&self) -> &str {
        match self.mappings.get("dynamic") {
            Some(Json::Bool(false)) => "false",
            Some(Json::String(s)) if s == "false" || s == "strict" => s.as_str(),
            _ => "true",
        }
    }

    /// The fields with columns, in name order: (column, kind).
    pub fn columns(&self) -> Vec<(String, Kind)> {
        self.fields.iter().filter_map(|(n, f)| f.column(n).map(|c| (c, f.kind.clone()))).collect()
    }

    /// Whether the string at document path `p` is kept in its TEXT
    /// column alone: a text field with no other field (a keyword
    /// sub-field) reading it from `_source`.
    pub fn stripped_text(&self, p: &str) -> bool {
        let text = matches!(self.fields.get(p), Some(Field { kind: Kind::Text { grams: None, .. }, source, .. }) if source == p);
        text && !self.fields.values().any(|f| f.source == p && !matches!(f.kind, Kind::Text { .. }))
    }

    /// The text columns that hold a string their document no longer
    /// does, and the vector columns: read back into `_source`.
    pub fn kept_apart(&self) -> Vec<String> {
        self.fields.iter().filter(|(n, f)| (f.source == **n && self.stripped_text(n)) || matches!(f.kind, Kind::Vector { .. })).map(|(n, _)| n.clone()).collect()
    }

    /// The row's columns to read for documents: `_id`, `_version`,
    /// `_source`, then [`Self::kept_apart`].
    pub fn read_columns(&self) -> Vec<String> {
        let mut c = vec![ID.to_string(), VERSION.to_string(), SOURCE.to_string()];
        c.extend(self.kept_apart());
        c
    }

    /// The index as `GET /index` answers it.
    pub fn json(&self) -> Json {
        let aliases: Map<String, Json> = self.aliases.iter().map(|a| (a.clone(), json!({}))).collect();
        let mut settings = if self.settings.is_object() { self.settings.clone() } else { json!({}) };
        let mut index = settings.get("index").cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
        index["uuid"] = json!(self.uuid);
        index["provided_name"] = json!(self.name);
        index["creation_date"] = json!(self.created_ms.to_string());
        for (k, v) in [("number_of_shards", "1"), ("number_of_replicas", "0")] {
            if index.get(k).is_none() {
                index[k] = json!(settings.get(k).cloned().unwrap_or(json!(v)));
            }
        }
        index["version"] = json!({"created": "136327827"});
        settings["index"] = index;
        json!({"aliases": aliases, "mappings": self.mappings, "settings": settings})
    }

    /// The row of [`META`] that keeps it.
    pub fn meta_row(&self) -> Json {
        let meta = json!({"table": self.table, "uuid": self.uuid, "created_ms": self.created_ms, "mappings": self.mappings, "settings": self.settings});
        json!({"name": self.name, "aliases": self.aliases, "meta": meta.to_string()})
    }

    /// The index a [`META`] row keeps.
    pub fn of_meta(name: &str, aliases: Vec<String>, meta: &str) -> Result<Index, OsError> {
        let m: Json = serde_json::from_str(meta).map_err(|e| OsError::internal(format!("index {name}: its stored mapping is not JSON: {e}")))?;
        Index::new(name, m["table"].as_str().unwrap_or_default(), m["uuid"].as_str().unwrap_or_default(), m["created_ms"].as_i64().unwrap_or(0), m["mappings"].clone(), m["settings"].clone(), aliases)
    }
}

/// A part of the index's `analysis` settings (`analyzer`, `tokenizer`,
/// `filter`) by name, at `analysis` or `index.analysis`.
fn analysis<'a>(settings: &'a Json, part: &str, name: &str) -> &'a Json {
    let v = &settings["analysis"][part][name];
    if v.is_null() {
        &settings["index"]["analysis"][part][name]
    } else {
        v
    }
}

/// The n-grams of an n-gram tokenizer or filter (`spec` its definition,
/// or null for a built-in one of `name`).
fn grams_in(name: &str, spec: &Json) -> Option<(usize, usize, bool)> {
    let ty = spec.get("type").and_then(Json::as_str).unwrap_or(name);
    let edge = match ty {
        "edge_ngram" | "edgeNGram" => true,
        "ngram" | "nGram" => false,
        _ => return None,
    };
    let n = |k: &str, d: usize| spec.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).map_or(d, |x| x as usize);
    let (min, max) = (n("min_gram", 1).max(1), n("max_gram", 2));
    Some((min, max.max(min), edge))
}

/// How an analyzer cuts text into words where Strata's analyzers (runs
/// of letters and digits) would cut it otherwise. The text is rewritten
/// before Strata reads it, in writes and in queries alike: a character
/// a word holds becomes a letter ([`joined`]), as does a letter whose
/// accent the analyzer keeps, and [`BREAK`] goes where a word is cut.
/// [`Cut::unapply`] gives the text back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    pub join: Join,
    /// Words are cut where their case or digits change (a
    /// `word_delimiter_graph` filter: `CatsOf2026` is `cats of 2026`).
    pub split: bool,
    /// A word's `'s` is dropped (the `english` analyzer's possessive
    /// filter: `children's` is `children`).
    pub possessive: bool,
    /// Accents are kept (`sévère` is not `severe`), as they are unless an
    /// `asciifolding` filter folds them; Strata's analyzers fold them.
    pub keep_accents: bool,
}

/// What [`Cut::apply`] puts where a word is cut that Strata's analyzers
/// would not cut: a character of no word (private use), which
/// [`Cut::unapply`] takes out.
pub const BREAK: char = '\u{E000}';

/// Whether the standard tokenizer makes `c` a word of its own: an
/// ideograph or a hiragana.
fn lone(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FFFF | 0x3007 | 0x3021..=0x3029 | 0x3038..=0x303A | 0x3041..=0x3096 | 0x309D..=0x309F | 0x2E80..=0x2EF3 | 0x2F00..=0x2FD5 | 0x1F200)
}

/// Whether `c` is a katakana, whose runs are words of their own.
fn katakana(c: char) -> bool {
    matches!(c as u32, 0x30A0..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F | 0x3031..=0x3035 | 0x309B | 0x309C | 0x32D0..=0x32FE | 0x3300..=0x3357)
}

/// Whether `c` is a letter that Unicode's word breaks join across `.`,
/// `'` and `:` (ALetter).
fn aletter(c: char) -> bool {
    c.is_alphabetic() && !lone(c) && !katakana(c)
}

/// Whether `c` is a character that Unicode's word breaks keep with the
/// one before it (Format and Extend: a soft hyphen, a joiner, a variation
/// selector, a combining mark, a virama, a tone mark).
fn attached(c: char) -> bool {
    if c.is_ascii() {
        return false;
    }
    if matches!(c as u32, 0xAD | 0x200C..=0x200F | 0x2060..=0x2064 | 0xFEFF) {
        return true;
    }
    // Extend and SpacingMark are what goes on with a letter's grapheme.
    let mut b = [b'a'; 5];
    let n = c.encode_utf8(&mut b[1..]).len();
    let s = std::str::from_utf8(&b[..1 + n]).unwrap_or("a");
    unicode_segmentation::UnicodeSegmentation::graphemes(s, true).nth(1).is_none()
}

/// Whether `c`, no letter or digit to Strata, is one to the standard
/// tokenizer: modifier letters (`ʾ`), radicals (`⼈`), the katakana and
/// hiragana symbols (`゛`, `㋐`, `🈀`), a few marks of Armenian, Hebrew,
/// Arabic and the scripts of Southeast Asia.
fn letterlike(c: char) -> bool {
    matches!(c as u32, 0x2C2..=0x2C5 | 0x2D2..=0x2D7 | 0x2DE..=0x2DF | 0x2ED | 0x2EF..=0x2FF | 0x55B..=0x55C | 0x55E | 0x5F3 | 0x66B | 0x109E..=0x109F | 0x19DE..=0x19DF | 0x1AA0..=0x1AA6 | 0x1AA8..=0x1AAD | 0x2E80..=0x2E99 | 0x2E9B..=0x2EF3 | 0x2F00..=0x2FD5 | 0x309B..=0x309C | 0x30A0 | 0x32D0..=0x32FE | 0x3300..=0x3357 | 0x1F200)
}

/// Whether Strata's analyzers fold the lowercase letter `c`.
fn folds(c: char) -> bool {
    if c.is_ascii() {
        return false;
    }
    let mut b = [0u8; 4];
    let s = c.encode_utf8(&mut b);
    strata::text::analyzer::normalize_word(s) != *s
}

/// The characters other than letters and digits that a word holds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Join {
    /// None (Strata's own words; the `letter` and `pattern` tokenizers
    /// and the n-gram ones, whose grams hold their own).
    #[default]
    None,
    /// The `standard` tokenizer's, as Unicode's word breaks keep them:
    /// `_` beside a letter or digit (`merge_segments`), `.`, `'`, `:`
    /// and `·` between letters (`mastodon.social`, `children's`), `.`,
    /// `,`, `;` and `'` between digits (`3.14`).
    Standard,
    /// Every one but whitespace (`whitespace`).
    Whitespace,
    /// Every one (`keyword`: the text is one word).
    Whole,
}

/// Whether `c` is an emoji to the standard tokenizer: the characters
/// OpenSearch 2.11's `_analyze` makes a word of type `<EMOJI>`, asked one
/// by one (Unicode's `Extended_Pictographic` as Lucene's tables have it;
/// `★` is one, `☆` and `✓` are not).
fn pictographic(c: char) -> bool {
    matches!(c as u32,
        0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x2199 | 0x21A9..=0x21AA
        | 0x231A..=0x231B | 0x2328 | 0x2388 | 0x23CF | 0x23E9..=0x23F3 | 0x23F8..=0x23FA | 0x24C2
        | 0x25AA..=0x25AB | 0x25B6 | 0x25C0 | 0x25FB..=0x25FE | 0x2600..=0x2605 | 0x2607..=0x2612
        | 0x2614..=0x2685 | 0x2690..=0x2705 | 0x2708..=0x2712 | 0x2714 | 0x2716 | 0x271D | 0x2721
        | 0x2728 | 0x2733..=0x2734 | 0x2744 | 0x2747 | 0x274C | 0x274E | 0x2753..=0x2755 | 0x2757
        | 0x2763..=0x2767 | 0x2795..=0x2797 | 0x27A1 | 0x27B0 | 0x27BF | 0x2934..=0x2935
        | 0x2B05..=0x2B07 | 0x2B1B..=0x2B1C | 0x2B50 | 0x2B55 | 0x3030 | 0x303D | 0x3297 | 0x3299
        | 0x1F000..=0x1F0FF | 0x1F10D..=0x1F10F | 0x1F12F | 0x1F16C..=0x1F171 | 0x1F17E..=0x1F17F
        | 0x1F18E | 0x1F191..=0x1F19A | 0x1F1AD..=0x1F1E5 | 0x1F201..=0x1F20F | 0x1F21A | 0x1F22F
        | 0x1F232..=0x1F23A | 0x1F23C..=0x1F23F | 0x1F249..=0x1F53D | 0x1F546..=0x1F64F
        | 0x1F680..=0x1F6FF | 0x1F774..=0x1F77F | 0x1F7D5..=0x1F7FF | 0x1F80C..=0x1F80F
        | 0x1F848..=0x1F84F | 0x1F85A..=0x1F85F | 0x1F888..=0x1F88F | 0x1F8AE..=0x1F8FF
        | 0x1F90C..=0x1F93A | 0x1F93C..=0x1F945 | 0x1F947..=0x1FBFF)
}

fn word_char(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric()
    } else {
        c.is_alphanumeric() || ('\u{300}'..='\u{36f}').contains(&c)
    }
}

/// Whether `c` is a letter or digit to Strata and in no word to the
/// standard tokenizer: a number that is not a digit (superscripts,
/// fractions, circled numbers: `²`, `¼`, `①`), `〆`, the letters newer
/// than Lucene's tables.
fn stray(c: char) -> bool {
    matches!(c as u32, 0xB2 | 0xB3 | 0xB9 | 0xBC..=0xBE | 0x9F4..=0x9F9 | 0xB72..=0xB77 | 0xBF0..=0xBF2 | 0xC78..=0xC7E | 0xD58..=0xD5E | 0xD70..=0xD78 | 0xF2A..=0xF33 | 0x1369..=0x137C | 0x17F0..=0x17F9 | 0x2070 | 0x2074..=0x2079 | 0x2080..=0x2089 | 0x2150..=0x215F | 0x2189 | 0x2460..=0x249B | 0x24EA..=0x24FF | 0x2776..=0x2793 | 0x2C2F | 0x2C5F | 0x2CFD | 0x3006 | 0x3192..=0x3195 | 0x31BB..=0x31BF | 0x3220..=0x3229 | 0x3248..=0x325F | 0x3280..=0x3289 | 0x32B1..=0x32BF | 0x1F100..=0x1F10C)
}

/// What stands for `c`, a character of no word that Strata's analyzers
/// would read as one ([`stray`]): a private use character, which
/// [`Cut::unapply`] gives back.
fn unworded(c: char) -> char {
    char::from_u32(0xF0000 + c as u32).unwrap_or(BREAK)
}

impl Cut {
    /// Whether character `i` of `c`, which is no letter or digit, is in
    /// a word.
    fn joins(&self, c: &[char], i: usize) -> bool {
        let (p, n) = (i.checked_sub(1).map(|k| c[k]), c.get(i + 1).copied());
        match self.join {
            Join::None => false,
            Join::Whole => true,
            Join::Whitespace => !c[i].is_whitespace(),
            Join::Standard => {
                let letters = p.is_some_and(aletter) && n.is_some_and(aletter);
                let digit = |x: char| x.is_numeric() && !stray(x);
                let digits = p.is_some_and(digit) && n.is_some_and(digit);
                match c[i] {
                    x if attached(x) => p.is_some_and(|x| self.is_word(x)) || (i > 0 && self.joins(c, i - 1)),
                    '_' => p.is_some_and(|x| self.is_word(x) && !lone(x)) || n.is_some_and(|x| self.is_word(x) && !lone(x)),
                    // Unicode's MidNumLet and Single_Quote.
                    '.' | '\'' | '\u{2018}' | '\u{2019}' | '\u{2024}' | '\u{FE52}' | '\u{FF07}' | '\u{FF0E}' => letters || digits,
                    // MidLetter.
                    ':' | '\u{B7}' | '\u{387}' | '\u{5F4}' | '\u{2027}' | '\u{FE13}' | '\u{FE55}' | '\u{FF1A}' => letters,
                    // MidNum.
                    ',' | ';' | '\u{37E}' | '\u{589}' | '\u{60C}' | '\u{60D}' | '\u{66C}' | '\u{7F8}' | '\u{2044}' | '\u{FE10}' | '\u{FE14}' | '\u{FE50}' | '\u{FE54}' | '\u{FF0C}' | '\u{FF1B}' => digits,
                    _ => false,
                }
            }
        }
    }

    /// Whether `c` is a letter or digit of a word ([`word_char`]; to the
    /// standard tokenizer, but for [`stray`] and with [`letterlike`]).
    fn is_word(&self, c: char) -> bool {
        if self.join == Join::Standard {
            (word_char(c) && !stray(c)) || letterlike(c)
        } else {
            word_char(c)
        }
    }

    /// Whether a word is cut before character `i` of `c`, a letter or
    /// digit after another: where case or digits change (`split`), or the
    /// standard tokenizer's scripts do (each ideograph a word, runs of
    /// katakana).
    fn splits(&self, c: &[char], i: usize) -> bool {
        if i == 0 {
            return false;
        }
        if self.join == Join::Standard && (lone(c[i]) || lone(c[i - 1]) || katakana(c[i]) != katakana(c[i - 1])) {
            return true;
        }
        if !self.split {
            return false;
        }
        let (p, x) = (c[i - 1], c[i]);
        let next_lower = c.get(i + 1).is_some_and(|n| n.is_lowercase());
        (p.is_lowercase() && x.is_uppercase()) || (p.is_uppercase() && x.is_uppercase() && next_lower) || (p.is_alphabetic() && x.is_numeric()) || (p.is_numeric() && x.is_alphabetic())
    }

    pub fn is_plain(&self) -> bool {
        *self == Cut::default()
    }

    /// `text` as Strata should read it.
    pub fn apply(&self, text: &str) -> String {
        if self.is_plain() {
            return text.to_string();
        }
        let c: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len() + 8);
        let mut i = 0;
        while i < c.len() {
            if let Some(end) = self.emoji(&c, i) {
                out.push(BREAK);
                out.extend(c[i..end].iter().map(|&x| joined(x)));
                out.push(BREAK);
                i = end;
                continue;
            }
            if self.is_word(c[i]) {
                if i > 0 && self.is_word(c[i - 1]) && self.splits(&c, i) {
                    out.push(BREAK);
                }
                self.put(c[i], &mut out);
            } else if word_char(c[i]) {
                out.push(unworded(c[i]));
            } else if self.joins(&c, i) {
                if self.possessive_at(&c, i) {
                    i += 2;
                    continue;
                }
                out.push(joined(c[i]));
            } else {
                out.push(c[i]);
            }
            i += 1;
        }
        out
    }

    /// Letter or digit `ch` of a word, as Strata should read it: a letter
    /// whose accent is kept is [`joined`] (lowercased first, since Strata
    /// does not lowercase what it stands for).
    fn put(&self, ch: char, out: &mut String) {
        if !word_char(ch) {
            out.push(joined(ch));
            return;
        }
        if !self.keep_accents || ch.is_ascii() {
            out.push(ch);
            return;
        }
        // Lowercased one character at a time, as Lucene does: `İ` is `i`.
        if ch == '\u{130}' {
            out.push('i');
            return;
        }
        for l in ch.to_lowercase() {
            out.push(if folds(l) { joined(l) } else { l });
        }
    }

    /// The text that [`Cut::apply`] gave `t` of: none of its breaks, and the
    /// characters [`joined`] stands for (which [`Index`] checks before it
    /// keeps a text in its column alone: a letter lowercased or a
    /// character beyond those [`joined`] tells apart does not come back).
    pub fn unapply(&self, t: &str) -> String {
        if self.is_plain() {
            return t.to_string();
        }
        t.chars().filter(|&c| c != BREAK).map(|c| match c as u32 {
            x @ 0x20000..=0x2A6D6 => char::from_u32(x - 0x20000).unwrap_or(c),
            x @ 0xF0000..=0xFFFFD => char::from_u32(x - 0xF0000).unwrap_or(c),
            _ => c,
        }).collect()
    }

    /// Whether the apostrophe at character `i` of `c`, in a word, and the
    /// `s` after it end the word, and are dropped ([`Cut::possessive`]).
    fn possessive_at(&self, c: &[char], i: usize) -> bool {
        self.possessive
            && matches!(c[i], '\'' | '\u{2019}' | '\u{FF07}')
            && matches!(c.get(i + 1), Some('s' | 'S'))
            && c.get(i + 2).is_none_or(|&x| !self.is_word(x) && !self.joins(c, i + 2))
    }

    /// Where the emoji at character `i` of `c` ends, if one starts there:
    /// the standard tokenizer makes each one a word of its own (`🌱`,
    /// `👩‍💻`, a flag), which Strata's analyzers would drop.
    fn emoji(&self, c: &[char], i: usize) -> Option<usize> {
        if self.join != Join::Standard {
            return None;
        }
        let ri = |x: char| ('\u{1F1E6}'..='\u{1F1FF}').contains(&x);
        if ri(c[i]) {
            // A flag is two; one alone is no word.
            return c.get(i + 1).copied().is_some_and(ri).then_some(i + 2);
        }
        if !pictographic(c[i]) {
            return None;
        }
        let mut j = i + 1;
        while j < c.len() {
            if c[j] == '\u{FE0F}' || ('\u{1F3FB}'..='\u{1F3FF}').contains(&c[j]) {
                j += 1;
            } else if c[j] == '\u{200D}' && c.get(j + 1).copied().is_some_and(pictographic) {
                j += 2;
            } else {
                break;
            }
        }
        Some(j)
    }

    /// The byte spans of the words of `text`, as Strata reads it after
    /// [`Cut::apply`]: for highlights of the text as written.
    pub fn spans(&self, text: &str) -> Vec<(usize, usize)> {
        let at: Vec<(usize, char)> = text.char_indices().collect();
        let c: Vec<char> = at.iter().map(|x| x.1).collect();
        let pos = |i: usize| at.get(i).map_or(text.len(), |x| x.0);
        let mut out = Vec::new();
        let mut start: Option<usize> = None;
        let mut i = 0;
        while i < c.len() {
            if let Some(end) = self.emoji(&c, i) {
                if let Some(s) = start.take() {
                    out.push((s, pos(i)));
                }
                out.push((pos(i), pos(end)));
                i = end;
                continue;
            }
            if self.is_word(c[i]) || self.joins(&c, i) {
                match start {
                    Some(s) if self.is_word(c[i]) && self.is_word(c[i - 1]) && self.splits(&c, i) => {
                        out.push((s, pos(i)));
                        start = Some(pos(i));
                    }
                    Some(_) => {}
                    None => start = Some(pos(i)),
                }
            } else if let Some(s) = start.take() {
                out.push((s, pos(i)));
            }
            i += 1;
        }
        if let Some(s) = start {
            out.push((s, text.len()));
        }
        out
    }

    /// A word of the text as written as Strata indexes it (its joined
    /// characters [`joined`]).
    pub fn word(&self, w: &str) -> String {
        let c: Vec<char> = w.chars().collect();
        let n = c.len();
        let c = if n > 2 && self.possessive_at(&c, n - 2) { &c[..n - 2] } else { &c[..] };
        let mut out = String::new();
        for &ch in c {
            if word_char(ch) {
                self.put(ch, &mut out);
            } else {
                out.push(joined(ch));
            }
        }
        out
    }
}

/// How the analyzer named `name` (none: the index's default, `standard`)
/// cuts words ([`Cut`]).
fn cut_of(name: Option<&str>, settings: &Json) -> Cut {
    let name = name.unwrap_or("default");
    let custom = analysis(settings, "analyzer", name);
    if custom.is_null() {
        let join = match name {
            "whitespace" => Join::Whitespace,
            "keyword" => Join::Whole,
            "simple" | "stop" | "pattern" | "fingerprint" => Join::None,
            _ => Join::Standard,
        };
        return Cut { join, split: false, possessive: name == "english", keep_accents: true };
    }
    let filters: Vec<&str> = custom.get("filter").and_then(Json::as_array).map(|fs| fs.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
    let possessive = filters.iter().any(|f| {
        let spec = analysis(settings, "filter", f);
        let ty = spec.get("type").and_then(Json::as_str).unwrap_or(f);
        let lang = spec.get("language").or_else(|| spec.get("name")).and_then(Json::as_str);
        ty == "stemmer" && lang == Some("possessive_english")
    });
    let keep_accents = !filters.iter().any(|f| {
        let spec = analysis(settings, "filter", f);
        matches!(spec.get("type").and_then(Json::as_str).unwrap_or(f), "asciifolding" | "icu_folding")
    });
    let split = filters.iter().any(|f| {
        let spec = analysis(settings, "filter", f);
        let ty = spec.get("type").and_then(Json::as_str).unwrap_or(f);
        let on = |k: &str| spec.get(k).map_or(true, |v| v.as_bool().unwrap_or(v.as_str() != Some("false")));
        matches!(ty, "word_delimiter" | "word_delimiter_graph") && on("split_on_case_change") && on("split_on_numerics")
    });
    // A word delimiter cuts at every character but letters and digits.
    if split {
        return Cut { join: Join::None, split, possessive: false, keep_accents };
    }
    let tok_name = custom.get("tokenizer").and_then(Json::as_str).unwrap_or("standard");
    let tok = analysis(settings, "tokenizer", tok_name);
    let join = match tok.get("type").and_then(Json::as_str).unwrap_or(tok_name) {
        "standard" | "classic" | "uax_url_email" => Join::Standard,
        "whitespace" => Join::Whitespace,
        "keyword" => Join::Whole,
        _ => Join::None,
    };
    Cut { join, split: false, possessive, keep_accents }
}

/// The n-grams a custom analyzer makes, if it makes them.
fn grams_of(name: Option<&str>, settings: &Json) -> Option<Grams> {
    let custom = analysis(settings, "analyzer", name?);
    let tok_name = custom.get("tokenizer").and_then(Json::as_str).unwrap_or("standard");
    let tok = analysis(settings, "tokenizer", tok_name);
    if let Some((min, max, edge)) = grams_in(tok_name, tok) {
        let classes: Vec<String> = tok.get("token_chars").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default();
        let custom_chars = tok.get("custom_token_chars").and_then(Json::as_str).unwrap_or_default().to_string();
        let split = if classes.is_empty() { Split::Whole } else { Split::Classes(classes, custom_chars) };
        return Some(Grams { min, max, edge, split });
    }
    let filters = custom.get("filter").and_then(Json::as_array)?;
    let (min, max, edge) = filters.iter().filter_map(Json::as_str).find_map(|f| grams_in(f, analysis(settings, "filter", f)))?;
    let split = if tok_name == "keyword" { Split::Whole } else { Split::Words };
    Some(Grams { min, max, edge, split })
}

/// The analyzer a mapping's analyzer name comes to: `english` for an
/// English analyzer, or a custom one that stems; `simple` (lowercased
/// words, nothing dropped or stemmed) for every other, `standard`
/// included.
fn analyzer_of(name: Option<&str>, settings: &Json) -> &'static str {
    let Some(name) = name else { return "simple" };
    if name == "english" {
        return "english";
    }
    let custom = analysis(settings, "analyzer", name);
    let stems = |f: &Json| match f.as_str() {
        Some(f) => matches!(f, "porter_stem" | "stemmer" | "snowball" | "kstem" | "english_stemmer"),
        None => false,
    };
    let filters = custom.get("filter").and_then(Json::as_array).map(|a| a.iter().any(stems)).unwrap_or(false);
    if filters || custom.get("type").and_then(Json::as_str) == Some("english") {
        "english"
    } else {
        "simple"
    }
}

/// The kind of a mapping type, or an error for one Strata cannot hold.
fn kind_of(ty: &str, spec: &Json, settings: &Json) -> Result<Kind, OsError> {
    Ok(match ty {
        "text" | "match_only_text" | "search_as_you_type" => {
            let name = spec.get("analyzer").and_then(Json::as_str);
            let search = spec.get("search_analyzer").and_then(Json::as_str).or(name);
            let query_cut = cut_of(search, settings);
            match grams_of(name, settings) {
                Some(g) => Kind::Text { analyzer: "simple", grams: Some(g), cut: Cut::default(), query_cut },
                None => Kind::Text { analyzer: analyzer_of(name, settings), grams: None, cut: cut_of(name, settings), query_cut },
            }
        }
        "keyword" | "constant_keyword" | "wildcard" | "version" => Kind::Keyword,
        "long" | "integer" | "short" | "byte" | "unsigned_long" => Kind::Long,
        "double" | "float" | "half_float" | "scaled_float" => Kind::Double,
        "boolean" => Kind::Bool,
        "date" | "date_nanos" => Kind::Date,
        "knn_vector" | "dense_vector" => {
            let dim = spec.get("dimension").or_else(|| spec.get("dims")).and_then(Json::as_u64).ok_or_else(|| OsError::mapper(format!("a {ty} field needs its dimension")))?;
            if dim == 0 || dim > 4096 {
                return Err(OsError::mapper(format!("a {ty} field has 1 to 4096 dimensions, not {dim}")));
            }
            Kind::Vector { dim: dim as usize }
        }
        "ip" | "geo_point" | "geo_shape" | "flat_object" | "flattened" | "binary" | "token_count" | "integer_range" | "long_range" | "float_range" | "double_range" | "date_range" | "ip_range" | "alias" | "join" | "percolator" | "rank_feature" | "rank_features" => Kind::Other,
        other => return Err(OsError::mapper(format!("no handler for type [{other}]: Strata maps text, keyword, numbers, boolean, date, knn_vector, object and nested fields"))),
    })
}

/// Adds the fields of `props` (a mapping's `properties`) under `prefix`.
fn walk_properties(props: &Json, prefix: &str, settings: &Json, out: &mut BTreeMap<String, Field>) -> Result<(), OsError> {
    let Some(props) = props.as_object() else {
        return Err(OsError::mapper("properties is an object of fields"));
    };
    for (name, spec) in props {
        if name.is_empty() {
            return Err(OsError::mapper("a field name is empty"));
        }
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
        let ty = spec.get("type").and_then(Json::as_str).unwrap_or("object");
        if ty == "object" || ty == "nested" {
            if let Some(p) = spec.get("properties") {
                walk_properties(p, &path, settings, out)?;
            }
            continue;
        }
        let kind = kind_of(ty, spec, settings)?;
        out.insert(path.clone(), Field { kind, ty: ty.to_string(), source: path.clone() });
        if let Some(subs) = spec.get("fields").and_then(Json::as_object) {
            for (sub, s) in subs {
                let sty = s.get("type").and_then(Json::as_str).unwrap_or("keyword");
                let kind = kind_of(sty, s, settings)?;
                if matches!(kind, Kind::Vector { .. }) {
                    return Err(OsError::mapper("a vector is not a sub-field"));
                }
                out.insert(format!("{path}.{sub}"), Field { kind, ty: sty.to_string(), source: path.clone() });
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ tables

/// A name in double quotes, as SQL takes any name.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Text as a SQL string.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The table of an index named `name`: `os_` and the name, its other
/// characters as `_` and then, if any were, a hash of the name, so that
/// two names never share a table.
pub fn table_of(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c.to_ascii_lowercase() } else { '_' }).collect();
    if clean == name {
        return format!("os_{clean}");
    }
    format!("os_{clean}_{:08x}", twox_hash::XxHash3_64::oneshot(name.as_bytes()) as u32)
}

/// Checks an index name as OpenSearch does.
pub fn check_name(name: &str) -> Result<(), OsError> {
    let bad = |why: &str| Err(OsError::new(400, "invalid_index_name_exception", format!("Invalid index name [{name}], {why}")).index(name));
    if name.is_empty() || name == "." || name == ".." {
        return bad("must not be empty, '.' or '..'");
    }
    if name.len() > 255 {
        return bad("index name is too long, (> 255)");
    }
    if name.starts_with(['_', '-', '+']) {
        return bad("must not start with '_', '-', or '+'");
    }
    if name.chars().any(|c| c.is_uppercase()) {
        return bad("must be lowercase");
    }
    if name.contains(['\\', '/', '*', '?', '"', '<', '>', '|', ' ', ',', '#', ':']) {
        return bad("must not contain the following characters [ , \", *, \\, <, |, ,, >, /, ?, #, :]");
    }
    Ok(())
}

/// The column of a field as `CREATE TABLE` and `ALTER TABLE` write it.
pub fn column_sql(name: &str, kind: &Kind) -> String {
    match kind {
        Kind::Text { analyzer, .. } => format!("{} TEXT OPTIONS(analyzer = '{analyzer}')", ident(name)),
        Kind::Date => format!("{} TIMESTAMP", ident(name)),
        Kind::Vector { dim } => format!("{} VECTOR({dim})", ident(name)),
        Kind::Long => format!("{} INT", ident(name)),
        Kind::Double => format!("{} FLOAT", ident(name)),
        _ => unreachable!("only text, date, vector and number fields have columns"),
    }
}

/// `CREATE TABLE` of an index.
pub fn create_sql(ix: &Index) -> String {
    let mut cols = vec![format!("{} KEYWORD PRIMARY KEY", ident(ID)), format!("{} INT", ident(VERSION)), format!("{} INT", ident(HASH)), format!("{} JSON", ident(SOURCE)), format!("{} KEYWORD[]", ident(MULTI))];
    cols.extend(ix.columns().iter().map(|(c, k)| column_sql(c, k)));
    format!("CREATE TABLE {} ({})", ix.table, cols.join(", "))
}

/// `CREATE TABLE` of [`META`].
pub fn create_meta_sql() -> String {
    format!("CREATE TABLE IF NOT EXISTS {META} (name KEYWORD PRIMARY KEY, aliases KEYWORD[], meta TEXT OPTIONS(indexed = false))")
}

// ------------------------------------------------------------------ documents

/// A document with dotted keys made objects (`{"a.b": 1}` is
/// `{"a": {"b": 1}}`), as OpenSearch reads them.
pub fn expand_dots(doc: Map<String, Json>) -> Result<Map<String, Json>, OsError> {
    let mut out = Map::new();
    for (k, v) in doc {
        let v = match v {
            Json::Object(m) => Json::Object(expand_dots(m)?),
            Json::Array(xs) => Json::Array(xs.into_iter().map(|x| if let Json::Object(m) = x { expand_dots(m).map(Json::Object) } else { Ok(x) }).collect::<Result<_, _>>()?),
            v => v,
        };
        let keys: Vec<&str> = k.split('.').collect();
        if keys.iter().any(|k| k.is_empty()) {
            return Err(OsError::mapper(format!("field name cannot be empty or hold an empty part: [{k}]")));
        }
        insert_path(&mut out, &keys, v)?;
    }
    Ok(out)
}

fn insert_path(m: &mut Map<String, Json>, keys: &[&str], v: Json) -> Result<(), OsError> {
    if keys.len() == 1 {
        match (m.get_mut(keys[0]), v) {
            (Some(Json::Object(a)), Json::Object(b)) => {
                for (k, x) in b {
                    insert_path(a, &[k.as_str()], x)?;
                }
            }
            (_, v) => {
                m.insert(keys[0].to_string(), v);
            }
        }
        return Ok(());
    }
    let next = m.entry(keys[0].to_string()).or_insert_with(|| json!({}));
    if !next.is_object() {
        *next = json!({});
    }
    insert_path(next.as_object_mut().expect("an object"), &keys[1..], v)
}

/// The values at document path `p` (`a.b`), through arrays of objects,
/// leaves of arrays flattened.
pub fn values_at<'a>(doc: &'a Json, p: &str) -> Vec<&'a Json> {
    let mut cur = vec![doc];
    for k in p.split('.') {
        let mut next = Vec::new();
        for c in cur {
            match c {
                Json::Object(m) => {
                    if let Some(v) = m.get(k) {
                        next.push(v);
                    }
                }
                Json::Array(xs) => {
                    for x in xs {
                        if let Json::Object(m) = x {
                            if let Some(v) = m.get(k) {
                                next.push(v);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        cur = next;
    }
    let mut out = Vec::new();
    for c in cur {
        match c {
            Json::Array(xs) => out.extend(xs.iter().filter(|x| !x.is_null())),
            Json::Null => {}
            c => out.push(c),
        }
    }
    out
}

/// Takes the value at path `p` out of `doc`, if objects alone lead to it.
fn take_path(doc: &mut Map<String, Json>, p: &str) -> Option<Json> {
    let keys: Vec<&str> = p.split('.').collect();
    let mut m = doc;
    for k in &keys[..keys.len() - 1] {
        m = m.get_mut(*k)?.as_object_mut()?;
    }
    m.remove(*keys.last()?)
}

/// Puts `v` at path `p` of `doc` unless something is there.
fn put_path(doc: &mut Map<String, Json>, p: &str, v: Json) {
    let keys: Vec<&str> = p.split('.').collect();
    let mut m = doc;
    for k in &keys[..keys.len() - 1] {
        let next = m.entry(k.to_string()).or_insert_with(|| json!({}));
        let Some(o) = next.as_object_mut() else { return };
        m = o;
    }
    m.entry(keys[keys.len() - 1].to_string()).or_insert(v);
}

/// A row of an index's table for a document: its write operation's
/// `doc`. Checks the document against the mapping (a date that is not
/// one, a vector of the wrong length) and the JSON field's limits.
pub fn row_of(ix: &Index, id: &str, version: i64, doc: &Map<String, Json>) -> Result<Json, OsError> {
    let mut source = doc.clone();
    let mut row = Map::new();
    row.insert(ID.into(), json!(id));
    row.insert(VERSION.into(), json!(version));
    row.insert(HASH.into(), json!(hash_of(id)));
    let whole = Json::Object(doc.clone());
    let mut multi = Vec::new();
    for (name, f) in &ix.fields {
        let vals = values_at(&whole, &f.source);
        if vals.is_empty() {
            continue;
        }
        match &f.kind {
            Kind::Text { grams, cut, .. } => {
                let text: Vec<String> = vals.iter().map(|v| match v {
                    Json::String(s) => cut.apply(s),
                    other => other.to_string(),
                }).collect();
                let text = match grams {
                    Some(g) => text.iter().map(|t| g.of(t)).collect::<Vec<_>>().join(" "),
                    None => text.join("\n"),
                };
                row.insert(name.clone(), json!(text));
            }
            Kind::Date => {
                if vals.len() > 1 {
                    return Err(OsError::mapper(format!("failed to parse field [{name}] of type [date]: Strata holds one date a field")));
                }
                let t = parse_date(vals[0]).map_err(|why| OsError::mapper(format!("failed to parse field [{name}] of type [date] in document with id '{id}': {why}")))?;
                row.insert(name.clone(), json!(strata::timestamp::format(t)));
            }
            Kind::Vector { dim } => {
                let Some(Json::Array(xs)) = take_path(&mut source, &f.source) else {
                    return Err(OsError::mapper(format!("failed to parse field [{name}] of type [knn_vector]: a vector is an array of numbers, at a path of objects")));
                };
                if xs.len() != *dim || !xs.iter().all(Json::is_number) {
                    return Err(OsError::mapper(format!("Vector dimension mismatch. Expected: {dim}, Given: {} (field [{name}])", xs.len())));
                }
                row.insert(name.clone(), Json::Array(xs));
            }
            Kind::Long | Kind::Double => {
                if vals.len() > 1 {
                    multi.push(name.clone());
                }
                let mut least: Option<f64> = None;
                for v in &vals {
                    let x = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()));
                    let Some(x) = x.filter(|x| x.is_finite()) else {
                        return Err(OsError::mapper(format!("failed to parse field [{name}] of type [{}] in document with id '{id}'. Preview of field's value: '{v}'", f.ty)));
                    };
                    least = Some(least.map_or(x, |l| l.min(x)));
                }
                // A long truncates a fraction, as OpenSearch's does.
                match (least, &f.kind) {
                    (Some(x), Kind::Long) => row.insert(name.clone(), json!(x.trunc() as i64)),
                    (Some(x), _) => row.insert(name.clone(), json!(x)),
                    (None, _) => None,
                };
            }
            Kind::Bool => {
                for v in &vals {
                    let ok = v.is_boolean() || matches!(v.as_str(), Some("true" | "false" | ""));
                    if !ok {
                        return Err(OsError::mapper(format!("failed to parse field [{name}] of type [boolean] in document with id '{id}'. Preview of field's value: '{v}'")));
                    }
                }
            }
            Kind::Keyword | Kind::Other => {}
        }
    }
    if !multi.is_empty() {
        row.insert(MULTI.into(), json!(multi));
    }
    // Strings kept in their text column alone.
    // (The column holds it as its analyzer's cut rewrote it, which
    // `doc_of` takes back; a string that does not come back stays in
    // `_source`.)
    for name in ix.kept_apart() {
        if let Kind::Text { cut, .. } = &ix.fields[&name].kind {
            if matches!(peek_path(&source, &name), Some(Json::String(s)) if cut.is_plain() || cut.unapply(&cut.apply(s)) == *s) {
                take_path(&mut source, &name);
            }
        }
    }
    check_limits(&source)?;
    row.insert(SOURCE.into(), Json::Object(source));
    Ok(Json::Object(row))
}

fn peek_path<'a>(doc: &'a Map<String, Json>, p: &str) -> Option<&'a Json> {
    let keys: Vec<&str> = p.split('.').collect();
    let mut m = doc;
    for k in &keys[..keys.len() - 1] {
        m = m.get(*k)?.as_object()?;
    }
    m.get(*keys.last()?)
}

/// The document of a row read with [`Index::read_columns`]: `_source`,
/// with the strings and vectors kept apart put back.
pub fn doc_of(ix: &Index, columns: &[String], row: &[Json]) -> (String, i64, Json) {
    let get = |c: &str| columns.iter().position(|x| x == c).and_then(|i| row.get(i));
    let id = get(ID).and_then(Json::as_str).unwrap_or_default().to_string();
    let version = get(VERSION).and_then(Json::as_i64).unwrap_or(1);
    let mut source = match get(SOURCE) {
        Some(Json::String(s)) => serde_json::from_str::<Map<String, Json>>(s).unwrap_or_default(),
        Some(Json::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    for name in ix.kept_apart() {
        // A string kept in `_source` as written.
        if peek_path(&source, &ix.fields[&name].source).is_some() {
            continue;
        }
        let v = match (get(&name), &ix.fields[&name].kind) {
            (Some(Json::Null) | None, _) => continue,
            (Some(Json::Array(xs)), _) => Json::Array(xs.iter().map(|x| x.as_f64().map(|f| json!(shortest(f as f32))).unwrap_or(Json::Null)).collect()),
            (Some(Json::String(t)), Kind::Text { cut, .. }) => json!(cut.unapply(t)),
            (Some(v), _) => v.clone(),
        };
        put_path(&mut source, &ix.fields[&name].source, v);
    }
    (id, version, Json::Object(source))
}

/// A float32 as the shortest decimal that reads back as it, as a double:
/// `0.1`, not `0.10000000149011612`.
pub fn shortest(x: f32) -> f64 {
    x.to_string().parse::<f64>().unwrap_or(x as f64)
}

/// Refuses a document the JSON field would ([`strata::json`]), naming the
/// limit, before it reaches the writer.
fn check_limits(doc: &Map<String, Json>) -> Result<(), OsError> {
    fn walk(v: &Json, path: usize, depth: usize, n: &mut usize) -> Result<(), OsError> {
        *n += 1;
        if *n > MAX_VALUES {
            return Err(OsError::mapper(format!("Strata keeps at most {MAX_VALUES} values and objects in a document")));
        }
        match v {
            Json::Object(m) => {
                if depth > MAX_DEPTH {
                    return Err(OsError::mapper(format!("Strata keeps objects nested at most {MAX_DEPTH} deep")));
                }
                for (k, x) in m {
                    if k.contains('\0') {
                        return Err(OsError::mapper("a field name holds the character \\0"));
                    }
                    let len = path + if path > 0 { 1 } else { 0 } + strata::json::escape(k).len();
                    if len > MAX_PATH {
                        return Err(OsError::mapper(format!("Strata keeps field paths of at most {MAX_PATH} bytes")));
                    }
                    walk(x, len, depth + 1, n)?;
                }
            }
            Json::Array(xs) => {
                for x in xs {
                    walk(x, path, depth, n)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut n = 0;
    for (k, x) in doc {
        if k.is_empty() {
            return Err(OsError::mapper("field name cannot be an empty string"));
        }
        let len = strata::json::escape(k).len();
        if len > MAX_PATH {
            return Err(OsError::mapper(format!("Strata keeps field paths of at most {MAX_PATH} bytes")));
        }
        walk(x, len, 1, &mut n)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ dynamic mapping

/// Fields of `doc` the mapping lacks, as `dynamic: true` adds them:
/// strings `text` with a `keyword` sub-field (or `date`, if they read as
/// one), whole numbers `long`, others `float`, booleans `boolean`,
/// objects their fields. Returns the new properties, merged into the
/// mapping's, or `None` when nothing is new.
pub fn new_fields(ix: &Index, doc: &Map<String, Json>) -> Option<Json> {
    fn walk(m: &Map<String, Json>, prefix: &str, ix: &Index, props: &mut Map<String, Json>, changed: &mut bool) {
        for (k, v) in m {
            let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            let first = match v {
                Json::Array(xs) => match xs.iter().find(|x| !x.is_null()) {
                    Some(x) => x,
                    None => continue,
                },
                v => v,
            };
            if let Json::Object(sub) = first {
                // An object mapped as something else (a vector's parent
                // cannot be one) is left as it is.
                if ix.fields.contains_key(&path) {
                    continue;
                }
                let entry = props.entry(k.clone()).or_insert_with(|| json!({"properties": {}}));
                if entry.get("type").is_some_and(|t| t != "object" && t != "nested") {
                    continue;
                }
                if !entry.get("properties").is_some_and(Json::is_object) {
                    entry["properties"] = json!({});
                }
                let inner = entry["properties"].as_object_mut().expect("properties");
                walk(sub, &path, ix, inner, changed);
                continue;
            }
            if ix.fields.contains_key(&path) || props.contains_key(k) {
                continue;
            }
            let spec = match first {
                Json::String(s) if looks_like_date(s) => json!({"type": "date"}),
                Json::String(_) => json!({"type": "text", "fields": {"keyword": {"type": "keyword", "ignore_above": 256}}}),
                Json::Number(n) if n.is_i64() || n.is_u64() => json!({"type": "long"}),
                Json::Number(_) => json!({"type": "float"}),
                Json::Bool(_) => json!({"type": "boolean"}),
                _ => continue,
            };
            props.insert(k.clone(), spec);
            *changed = true;
        }
    }
    let mut mappings = ix.mappings.clone();
    if !mappings.is_object() {
        mappings = json!({});
    }
    if !mappings.get("properties").is_some_and(Json::is_object) {
        mappings["properties"] = json!({});
    }
    let mut changed = false;
    walk(doc, "", ix, mappings["properties"].as_object_mut().expect("properties"), &mut changed);
    changed.then_some(mappings)
}

/// Whether a string reads as a date by OpenSearch's default dynamic date
/// detection (`strict_date_optional_time`): `2026-11-08`,
/// `2026-11-08T10:00:00Z`, `2026-11-08T10:00:00.123+02:00`.
fn looks_like_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 || !b[..4].iter().all(u8::is_ascii_digit) || b[4] != b'-' || !b[5..7].iter().all(u8::is_ascii_digit) || b[7] != b'-' || !b[8..10].iter().all(u8::is_ascii_digit) {
        return false;
    }
    b.len() == 10 || ((b[10] == b'T' || b[10] == b't') && strata::timestamp::parse(s).is_some())
}

/// A date as a field holds it: ISO 8601 text (a date, or a date and a
/// time), or milliseconds since 1970 (`epoch_millis`), as a number or
/// digits. Microseconds since 1970.
pub fn parse_date(v: &Json) -> Result<i64, String> {
    match v {
        Json::Number(n) => {
            let ms = n.as_f64().ok_or("not a number")?;
            Ok((ms * 1000.0).round() as i64)
        }
        Json::String(s) => {
            let t = s.trim();
            if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '-') && !t[1..].contains('-') {
                let ms: i64 = t.parse().map_err(|_| format!("[{s}] is not milliseconds"))?;
                return Ok(ms.saturating_mul(1000));
            }
            strata::timestamp::parse(t).ok_or_else(|| format!("[{s}] is not a date (ISO 8601, or milliseconds since 1970)"))
        }
        other => Err(format!("[{other}] is not a date")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_of_names() {
        assert_eq!(table_of("books"), "os_books");
        let t = table_of("gitea_codes.v1");
        assert!(t.starts_with("os_gitea_codes_v1_"), "{t}");
        assert_ne!(table_of("a.b"), table_of("a-b"));
    }

    #[test]
    fn dots_expand() {
        let d = expand_dots(serde_json::from_str(r#"{"a.b": 1, "a": {"c": 2}, "x": [{"y.z": 3}]}"#).unwrap()).unwrap();
        assert_eq!(Json::Object(d), json!({"a": {"b": 1, "c": 2}, "x": [{"y": {"z": 3}}]}));
    }

    #[test]
    fn text_kept_apart_and_back() {
        let ix = Index::new(
            "i",
            "os_i",
            "u",
            0,
            json!({"properties": {"title": {"type": "text", "fields": {"raw": {"type": "keyword"}}}, "body": {"type": "text", "analyzer": "english"}, "at": {"type": "date"}, "v": {"type": "knn_vector", "dimension": 2}, "n": {"type": "long"}}}),
            json!({}),
            vec![],
        )
        .unwrap();
        assert_eq!(ix.fields["body"].kind, Kind::Text { analyzer: "english", grams: None, cut: Cut { join: Join::Standard, split: false, possessive: true, keep_accents: true }, query_cut: Cut { join: Join::Standard, split: false, possessive: true, keep_accents: true } });
        assert_eq!(ix.fields["title.raw"].source, "title");
        assert!(!ix.stripped_text("title"));
        assert!(ix.stripped_text("body"));
        let doc: Map<String, Json> = serde_json::from_value(json!({"title": "Hi", "body": "long words", "at": "2026-11-08", "v": [0.1, 0.2], "n": 5})).unwrap();
        let row = row_of(&ix, "1", 1, &doc).unwrap();
        assert_eq!(row["_source"], json!({"title": "Hi", "at": "2026-11-08", "n": 5}));
        assert_eq!(row["body"], json!("long words"));
        assert_eq!(row["at"], json!("2026-11-08T00:00:00Z"));
        assert_eq!(row["n"], json!(5));
        let cols = ix.read_columns();
        let vals: Vec<Json> = cols.iter().map(|c| if c == "_source" { json!(row["_source"].to_string()) } else { row[c].clone() }).collect();
        let (id, v, back) = doc_of(&ix, &cols, &vals);
        assert_eq!((id.as_str(), v), ("1", 1));
        assert_eq!(back, Json::Object(doc));
    }

    #[test]
    fn dynamic_fields() {
        let ix = Index::new("i", "os_i", "u", 0, json!({"properties": {"a": {"type": "keyword"}}}), json!({}), vec![]).unwrap();
        let doc: Map<String, Json> = serde_json::from_value(json!({"a": "x", "b": "words", "c": 3, "d": 1.5, "e": true, "f": {"g": "2026-01-01"}, "h": [1, 2]})).unwrap();
        let m = new_fields(&ix, &doc).unwrap();
        let p = &m["properties"];
        assert_eq!(p["b"]["type"], "text");
        assert_eq!(p["b"]["fields"]["keyword"]["type"], "keyword");
        assert_eq!(p["c"]["type"], "long");
        assert_eq!(p["d"]["type"], "float");
        assert_eq!(p["e"]["type"], "boolean");
        assert_eq!(p["f"]["properties"]["g"]["type"], "date");
        assert_eq!(p["h"]["type"], "long");
        let ix2 = Index::new("i", "os_i", "u", 0, m, json!({}), vec![]).unwrap();
        assert!(new_fields(&ix2, &doc).is_none());
    }

    #[test]
    fn grams() {
        let settings = json!({"analysis": {
            "tokenizer": {"edge": {"type": "edge_ngram", "min_gram": 1, "max_gram": 5}, "words": {"type": "edge_ngram", "min_gram": 2, "max_gram": 3, "token_chars": ["letter"]}},
            "filter": {"tri": {"type": "ngram", "min_gram": 3, "max_gram": 3}},
            "analyzer": {"edge": {"tokenizer": "edge", "filter": ["lowercase"]}, "words": {"tokenizer": "words"}, "tri": {"tokenizer": "standard", "filter": ["lowercase", "tri"]}}
        }});
        let props = json!({"properties": {"a": {"type": "text", "analyzer": "edge"}, "b": {"type": "text", "analyzer": "words"}, "c": {"type": "text", "analyzer": "tri"}}});
        let ix = Index::new("i", "os_i", "u", 0, props, settings, vec![]).unwrap();
        let g = |f: &str| match &ix.fields[f].kind {
            Kind::Text { grams: Some(g), .. } => g.clone(),
            k => panic!("{k:?}"),
        };
        // The whole text, spaces kept inside the grams.
        let sp = joined(' ');
        assert_eq!(g("a").of("Jo Sm"), format!("J Jo Jo{sp} Jo{sp}S Jo{sp}Sm"));
        assert_eq!(g("b").of("ab cde"), "ab cd cde");
        assert_eq!(g("c").of("Hello ab"), "hel ell llo");
        // A gram is one word to Strata's analyzer.
        let mut words = Vec::new();
        strata::text::analyzer::for_each_word(&g("a").of("Jo Sm"), |w| words.push(w.to_string()));
        assert_eq!(words, ["j".to_string(), "jo".into(), format!("jo{sp}"), format!("jo{sp}s"), format!("jo{sp}sm")]);
        assert_ne!(joined(' '), joined('_'));
        assert!(joined('_').is_alphanumeric() && joined('\u{3000}').is_alphanumeric());
        // Kept in `_source`: the column holds grams, not the text.
        assert!(!ix.stripped_text("a"));
    }

    #[test]
    fn cuts() {
        let b = BREAK;
        let split = Cut { join: Join::None, split: true, possessive: false, keep_accents: false };
        assert_eq!(split.apply("CatsOfMastodon"), format!("Cats{b}Of{b}Mastodon"));
        assert_eq!(split.apply("XMLParser food2026 iPhone"), format!("XML{b}Parser food{b}2026 i{b}Phone"));
        let std = Cut { join: Join::Standard, split: false, possessive: false, keep_accents: false };
        let j = |c: char| joined(c);
        assert_eq!(std.apply("merge_segments at mastodon.social, 3.14 or 1,000; e.g. _x"), format!("merge{}segments at mastodon{}social, 3{}14 or 1{}000; e{}g. {}x", j('_'), j('.'), j('.'), j(','), j('.'), j('_')));
        // Apostrophes, colons and middle dots between letters; `;` and
        // `'` between digits; none at a word's edge.
        assert_eq!(std.apply("children's school\u{2019}s a:b x\u{B7}y 1;2 3'4 dogs' :a"), format!("children{}s school{}s a{}b x{}y 1{}2 3{}4 dogs' :a", j('\''), j('\u{2019}'), j(':'), j('\u{B7}'), j(';'), j('\'')));
        // The english analyzer drops a possessive `'s`, and only that.
        let en = Cut { possessive: true, ..std };
        assert_eq!(en.apply("the children's toys, CHILDREN'S, it's an s's don't"), format!("the children toys, CHILDREN, it an s don{}t", j('\'')));
        assert_eq!(en.word("children's"), "children");
        // Each ideograph and hiragana a word, runs of katakana; a soft
        // hyphen inside a word.
        assert_eq!(std.apply("與2019新型 ひらがなカタカナ漢字 abc漢"), format!("與{b}2019{b}新{b}型 ひ{b}ら{b}が{b}な{b}カタカナ{b}漢{b}字 abc{b}漢"));
        assert_eq!(std.apply("SARS\u{AD}CoV\u{AD}2"), format!("SARS{}CoV{}2", j('\u{AD}'), j('\u{AD}')));
        // Superscripts and fractions are no part of a word; `İ` is `i`.
        let text = "Infection¼ in m² ①";
        let spans: Vec<&str> = std.spans(text).iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(spans, ["Infection", "in", "m"]);
        let mut words = Vec::new();
        strata::text::analyzer::for_each_word(&std.apply(text), |w| words.push(w.to_string()));
        assert_eq!(words, ["infection", "in", "m"]);
        // Accents kept: Strata does not fold what a letter stands for.
        let keep = Cut { keep_accents: true, ..std };
        assert_eq!(keep.apply("SÉVÈRE straße"), format!("S{}V{}RE stra{}e", j('é'), j('è'), j('ß')));
        let mut words = Vec::new();
        strata::text::analyzer::for_each_word(&keep.apply("Sévère severe"), |w| words.push(w.to_string()));
        assert_ne!(words[0], words[1]);
        assert_eq!(strata::text::analyzer::normalize_word(&keep.word("Sévère")), words[0]);
        // The text comes back but for the case of a letter whose accent is kept.
        for t in ["merge_segments at 3.14", "與2019新型", "CatsOf2026", "SARS\u{AD}CoV\u{AD}2", "sévère"] {
            for cut in [std, split, keep] {
                assert_eq!(cut.unapply(&cut.apply(t)), t, "{cut:?}");
            }
        }
        assert_ne!(keep.unapply(&keep.apply("SÉVÈRE")), "SÉVÈRE");
        assert_eq!(keep.apply("İnfection"), "infection");
        // What OpenSearch 2.11's standard analyzer makes of these, word for
        // word: `☆`, `✓`, a lone flag letter and `〆` are none; a tone
        // mark, a virama, `゛` and a modifier letter go on with the word.
        let words = |t: &str| {
            let mut out = Vec::new();
            strata::text::analyzer::for_each_word(&keep.apply(t), |w| out.push(keep.unapply(w)));
            out
        };
        let w = words("Networks ☆ ✓ ★ 🇺 🇺🇸");
        assert_eq!((w.len(), &w[..2]), (3, &["networks".to_string(), "★".to_string()][..]));
        assert_eq!(words("ผู้ใหญ่ हिन्दी தமிழ் ខ្មែរ"), ["ผู้ใหญ่", "हिन्दी", "தமிழ்", "ខ្មែរ"]);
        assert_eq!(words("ab〆cd ⼈⼈ カ゛ガ aʾb x¹²"), ["ab", "cd", "⼈", "⼈", "カ゛ガ", "aʾb", "x"]);
        assert_eq!(std.unapply(&std.apply("Infection¼ in m² ①")), "Infection¼ in m² ①");
        let text = "the children's toys";
        let spans: Vec<&str> = en.spans(text).iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(spans, ["the", "children's", "toys"]);
        let text = "see merge_segments and fooBar";
        let spans: Vec<&str> = std.spans(text).iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(spans, ["see", "merge_segments", "and", "fooBar"]);
        let spans: Vec<&str> = split.spans(text).iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(spans, ["see", "merge", "segments", "and", "foo", "Bar"]);
        // Emoji are words of their own.
        let text = "Trent🌱 👩\u{200D}💻 🇩🇪!";
        let spans: Vec<&str> = std.spans(text).iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(spans, ["Trent", "🌱", "👩\u{200D}💻", "🇩🇪"]);
        let mut words = Vec::new();
        strata::text::analyzer::for_each_word(&std.apply(text), |w| words.push(w.to_string()));
        assert_eq!(words.len(), 4, "{words:?}");
        assert_eq!(words[1], std.word("🌱"));
        let settings = json!({"analysis": {"analyzer": {"tag": {"tokenizer": "keyword", "filter": ["word_delimiter_graph", "lowercase"]}, "ws": {"tokenizer": "whitespace"}}}});
        let ix = Index::new("i", "os_i", "u", 0, json!({"properties": {"t": {"type": "text", "analyzer": "tag"}, "w": {"type": "text", "analyzer": "ws"}, "k": {"type": "text", "analyzer": "keyword"}, "s": {"type": "text", "analyzer": "simple"}}}), settings, vec![]).unwrap();
        let doc: Map<String, Json> = serde_json::from_value(json!({"t": "CatsOf2026", "w": "a-b c", "k": "New York", "s": "a_b"})).unwrap();
        let row = row_of(&ix, "1", 1, &doc).unwrap();
        assert_eq!(row["t"], json!(format!("Cats{b}Of{b}2026")));
        assert_eq!(row["w"], json!(format!("a{}b c", joined('-'))));
        assert_eq!(row["k"], json!(format!("New{}York", joined(' '))));
        assert_eq!(row["s"], json!("a_b"));
        // In the columns alone: each comes back as written.
        assert_eq!(row["_source"], json!({}));
        let cols = ix.read_columns();
        let vals: Vec<Json> = cols.iter().map(|c| if c == "_source" { json!(row["_source"].to_string()) } else { row[c].clone() }).collect();
        assert_eq!(doc_of(&ix, &cols, &vals).2, Json::Object(doc));
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date(&json!(1000)).unwrap(), 1_000_000);
        assert_eq!(parse_date(&json!("1000")).unwrap(), 1_000_000);
        assert!(parse_date(&json!("2026-11-08T10:00:00Z")).is_ok());
        assert!(parse_date(&json!("nope")).is_err());
        assert!(looks_like_date("2026-11-08"));
        assert!(!looks_like_date("2026-11-08 is a day"));
    }
}
