//! The `standard` analyzer and the language analyzers built on it: text
//! in any script, where `english` and `simple` read words as runs of
//! alphanumerics (which cuts Hindi at its virama and Thai at its tone
//! marks, and leaves a Chinese or Japanese sentence one term).
//!
//! 1. **Normalize**: NFKC (fullwidth and halfwidth forms, ligatures and
//!    presentation forms become the plain characters), then lowercase
//!    (`ς` as `σ`; `İ` as `i`, and for `turkish` `I` as `ı`; Cyrillic `ё`
//!    as `е`, as Russian writes it more often than not, and no stress
//!    marks over Cyrillic vowels). Soft hyphens
//!    and zero-width joiners and non-joiners are dropped (`persian` cuts a
//!    word at a zero-width non-joiner, as its writers mean).
//! 2. **Split** into tokens, by script:
//!    * Chinese and Japanese (Han, Hiragana, Katakana), Thai, Lao, Khmer
//!      and Burmese, written without spaces: the **words** ICU's
//!      dictionaries find in each run (`東京都庁に` is `東京`, `都庁`,
//!      `に`).
//!    * Korean (Hangul): the overlapping **bigrams** of each run
//!      (`서울특별시에` is `서울`, `울특`, `특별`, `별시`, `시에`), a
//!      character alone as itself, so that a word is found without the
//!      particle its sentence gives it.
//!    * Every other script: maximal runs of letters, digits and combining
//!      marks, and a `.` or `,` between two digits stays in its number
//!      (`3.14`, `1,000`).
//!
//!    `cjk` makes bigrams of Chinese and Japanese too (`東京都庁` is
//!    `東京`, `京都`, `都庁`): no dictionary decides where a word ends, so
//!    a search for any two characters or more that follow each other
//!    finds them, as a phrase of its bigrams, at the price of matches
//!    across words (`京都` in `東京都`) and a ranking a little worse
//!    (`results/miracl`).
//! 3. **Language** ([`Language`]), for the tokens of the third kind: the
//!    language's normalization and its stemmer. `standard` has none.
//!    (`chinese` is `standard` with traditional characters read as the
//!    simplified ones, in step 1.)
//! 4. **Fold** the accents of Latin and Greek letters, after the stemmer,
//!    which reads them (`élèves` is `élev`, then `elev`): people leave
//!    accents out of what they search for, and of much of what they write.
//!    Other scripts keep their marks, which are their vowels and tones.
//!
//! No analyzer here removes stopwords from documents, so a phrase matches
//! exactly the words it names.
//!
//! What a term is must never change under an index that holds it: the
//! segmenter and its dictionaries are pinned to one version in
//! `Cargo.toml`, and another version of them is another version of these
//! analyzers.

use std::borrow::Cow;
use std::sync::OnceLock;

use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed};
use rust_stemmers::{Algorithm, Stemmer};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::{is_nfkc_quick, IsNormalized, UnicodeNormalization};

use super::analyzer::{fold as fold_letter, MAX_STEM_BYTES};

/// A language whose analyzer is `standard` plus its normalization and its
/// stemmer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Arabic,
    Bengali,
    Chinese,
    Danish,
    Dutch,
    Finnish,
    French,
    German,
    Greek,
    Hindi,
    Hungarian,
    Italian,
    Norwegian,
    Persian,
    Portuguese,
    Romanian,
    Russian,
    Spanish,
    Swedish,
    Tamil,
    Telugu,
    Turkish,
}

impl Language {
    pub const ALL: [Language; 22] = [
        Language::Arabic,
        Language::Bengali,
        Language::Chinese,
        Language::Danish,
        Language::Dutch,
        Language::Finnish,
        Language::French,
        Language::German,
        Language::Greek,
        Language::Hindi,
        Language::Hungarian,
        Language::Italian,
        Language::Norwegian,
        Language::Persian,
        Language::Portuguese,
        Language::Romanian,
        Language::Russian,
        Language::Spanish,
        Language::Swedish,
        Language::Tamil,
        Language::Telugu,
        Language::Turkish,
    ];

    /// Its analyzer's name.
    pub fn name(self) -> &'static str {
        match self {
            Language::Arabic => "arabic",
            Language::Bengali => "bengali",
            Language::Chinese => "chinese",
            Language::Danish => "danish",
            Language::Dutch => "dutch",
            Language::Finnish => "finnish",
            Language::French => "french",
            Language::German => "german",
            Language::Greek => "greek",
            Language::Hindi => "hindi",
            Language::Hungarian => "hungarian",
            Language::Italian => "italian",
            Language::Norwegian => "norwegian",
            Language::Persian => "persian",
            Language::Portuguese => "portuguese",
            Language::Romanian => "romanian",
            Language::Russian => "russian",
            Language::Spanish => "spanish",
            Language::Swedish => "swedish",
            Language::Tamil => "tamil",
            Language::Telugu => "telugu",
            Language::Turkish => "turkish",
        }
    }

    /// Its Snowball stemmer; Bengali, Chinese, Hindi, Persian and Telugu
    /// have none there.
    fn snowball(self) -> Option<Algorithm> {
        Some(match self {
            Language::Arabic => Algorithm::Arabic,
            Language::Danish => Algorithm::Danish,
            Language::Dutch => Algorithm::Dutch,
            Language::Finnish => Algorithm::Finnish,
            Language::French => Algorithm::French,
            Language::German => Algorithm::German,
            Language::Greek => Algorithm::Greek,
            Language::Hungarian => Algorithm::Hungarian,
            Language::Italian => Algorithm::Italian,
            Language::Norwegian => Algorithm::Norwegian,
            Language::Portuguese => Algorithm::Portuguese,
            Language::Romanian => Algorithm::Romanian,
            Language::Russian => Algorithm::Russian,
            Language::Spanish => Algorithm::Spanish,
            Language::Swedish => Algorithm::Swedish,
            Language::Tamil => Algorithm::Tamil,
            Language::Turkish => Algorithm::Turkish,
            Language::Bengali | Language::Chinese | Language::Hindi | Language::Persian | Language::Telugu => return None,
        })
    }
}

/// How a character's script is split into tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// No part of any token.
    Break,
    /// A letter, digit or mark of a script written with spaces.
    Word,
    /// Han, Hiragana, Katakana or Bopomofo: dictionary words, or bigrams
    /// for `cjk`.
    Han,
    /// Hangul: bigrams.
    Hangul,
    /// Thai, Lao, Khmer or Burmese: dictionary words.
    Sea,
}

fn class(c: char) -> Class {
    if c.is_ascii() {
        return if c.is_ascii_alphanumeric() { Class::Word } else { Class::Break };
    }
    let part = c.is_alphanumeric() || is_combining_mark(c);
    if !part {
        return Class::Break;
    }
    match c as u32 {
        // Thai, Lao, Myanmar (and its extensions), Khmer; their digits
        // are numbers like any others.
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => {
            if c.is_numeric() {
                Class::Word
            } else {
                Class::Sea
            }
        }
        // Hangul's Jamo and syllables.
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Class::Hangul,
        // The iteration marks and zero, Hiragana, Katakana, Bopomofo,
        // Katakana's phonetic extensions and Han (with extension A, the
        // compatibility ideographs and the supplementary planes).
        0x3005..=0x3007 | 0x3040..=0x30FF | 0x3100..=0x312F | 0x31A0..=0x31BF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3FFFF => Class::Han,
        _ => Class::Word,
    }
}

/// Step 1: `text` normalized, or `text` itself when it already is (most
/// ASCII and much else).
fn normalize(text: &str, language: Option<Language>) -> Cow<'_, str> {
    let turkish = language == Some(Language::Turkish);
    let chinese = language == Some(Language::Chinese);
    if text.is_ascii() && !turkish {
        return if text.bytes().any(|b| b.is_ascii_uppercase()) { Cow::Owned(text.to_ascii_lowercase()) } else { Cow::Borrowed(text) };
    }
    let mut out = String::with_capacity(text.len());
    let mut push = |c: char| match c {
        '\u{ad}' | '\u{200d}' => {}
        // A stress mark over a Cyrillic vowel, and `ё`, which is `е` more
        // often than not: before the stemmers, which know neither.
        '\u{300}' | '\u{301}' if out.chars().next_back().is_some_and(|p| ('\u{400}'..='\u{4ff}').contains(&p)) => {}
        'Ё' | 'ё' => out.push('е'),
        c if chinese && c >= '\u{3400}' => out.push(simplified(c)),
        '\u{200c}' => {
            if language == Some(Language::Persian) {
                out.push(' ');
            }
        }
        'İ' => out.push('i'),
        'I' if turkish => out.push('ı'),
        'ς' => out.push('σ'),
        c if c.is_lowercase() => out.push(c),
        c => out.extend(c.to_lowercase()),
    };
    if is_nfkc_quick(text.chars()) == IsNormalized::Yes {
        text.chars().for_each(&mut push);
    } else {
        text.nfkc().for_each(&mut push);
    }
    Cow::Owned(out)
}

/// A traditional Chinese character as the simplified one it is written
/// as (`src/text/hant_hans.txt`), others as they are: `chinese` reads both
/// writings as one, as a search in either expects.
fn simplified(c: char) -> char {
    static T: OnceLock<std::collections::HashMap<char, char>> = OnceLock::new();
    let t = T.get_or_init(|| {
        include_str!("hant_hans.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut cs = l.chars();
                Some((cs.next()?, cs.next()?))
            })
            .collect()
    });
    t.get(&c).copied().unwrap_or(c)
}

fn segmenter() -> WordSegmenterBorrowed<'static> {
    static S: OnceLock<WordSegmenterBorrowed<'static>> = OnceLock::new();
    *S.get_or_init(|| WordSegmenter::new_dictionary(Default::default()))
}

/// Calls `f` with every token of `text` (steps 1 and 2), in order;
/// `bigrams`: of Chinese and Japanese too (`cjk`).
pub(super) fn for_each_token(text: &str, language: Option<Language>, bigrams: bool, mut f: impl FnMut(&str)) {
    let text = normalize(text, language);
    let text = text.as_ref();
    let mut chars = text.char_indices().peekable();
    // The run being read: where it starts, its class, and whether its
    // last character is a digit.
    let mut run: Option<(usize, Class)> = None;
    let mut digit = false;
    while let Some((i, c)) = chars.next() {
        let mut k = class(c);
        // A separator inside a number.
        if k == Class::Break && matches!(c, '.' | ',') && digit && matches!(run, Some((_, Class::Word))) && chars.peek().is_some_and(|&(_, n)| n.is_numeric() && class(n) == Class::Word) {
            k = Class::Word;
        }
        match run {
            Some((_, r)) if r == k => {}
            _ => {
                if let Some((s, r)) = run.take() {
                    emit(&text[s..i], r, bigrams, &mut f);
                }
                if k != Class::Break {
                    run = Some((i, k));
                }
            }
        }
        digit = k == Class::Word && c.is_numeric();
    }
    if let Some((s, r)) = run {
        emit(&text[s..], r, bigrams, &mut f);
    }
}

/// The tokens of one run of class `class`.
fn emit(run: &str, class: Class, bigrams: bool, f: &mut impl FnMut(&str)) {
    match class {
        Class::Break => {}
        Class::Word => f(run),
        Class::Hangul | Class::Han if class == Class::Hangul || bigrams => {
            let mut at = run.char_indices().map(|(i, _)| i).chain([run.len()]);
            let (Some(mut a), Some(mut b)) = (at.next(), at.next()) else { return };
            let Some(mut c) = at.next() else { return f(run) };
            loop {
                f(&run[a..c]);
                (a, b) = (b, c);
                match at.next() {
                    Some(n) => c = n,
                    None => break,
                }
            }
        }
        Class::Han | Class::Hangul | Class::Sea => {
            let mut from = 0;
            for to in segmenter().segment_str(run) {
                if to > from {
                    f(&run[from..to]);
                }
                from = to;
            }
        }
    }
}

/// Steps 3 and 4: the term of a token, or `None` when nothing is left of
/// it. A bigram or a dictionary word is its own term. A word longer than
/// [`MAX_STEM_BYTES`] is not stemmed.
pub(super) fn term_of(token: &str, language: Option<Language>) -> Option<Cow<'_, str>> {
    let first = token.chars().next()?;
    if class(first) != Class::Word {
        return Some(Cow::Borrowed(token));
    }
    let stem = match language.filter(|_| token.len() <= MAX_STEM_BYTES) {
        Some(Language::Bengali) => Cow::Owned(indic::bengali_stem(indic::bengali(token))),
        Some(Language::Hindi) => Cow::Owned(indic::hindi_stem(indic::hindi(token))),
        Some(Language::Telugu) => Cow::Owned(indic::telugu_stem(indic::telugu(token))),
        Some(Language::Persian) => Cow::Owned(persian(token)),
        Some(l) => match l.snowball() {
            Some(a) => Stemmer::create(a).stem(token),
            None => Cow::Borrowed(token),
        },
        None => Cow::Borrowed(token),
    };
    let term = match fold(&stem) {
        Cow::Owned(folded) => Cow::Owned(folded),
        Cow::Borrowed(_) => stem,
    };
    (!term.is_empty()).then_some(term)
}

/// A token as a prefix query matches it against terms: folded as terms
/// are, not stemmed.
pub(super) fn prefix_of(token: &str) -> String {
    fold(token).into_owned()
}

/// Step 4: `word` without the accents of its Latin and Greek letters
/// (their canonical decompositions without the combining marks, and the
/// letters `english` folds, such as `ø`, `ß` and `ı`), and without the
/// combining diacritical marks that have no composed form with their
/// letter.
fn fold(word: &str) -> Cow<'_, str> {
    // Latin-1 to Latin Extended-B, the combining diacritical marks, Greek,
    // Latin Extended Additional and Greek Extended.
    let folds = |c: char| matches!(c as u32, 0xC0..=0x24F | 0x300..=0x3FF | 0x1E00..=0x1FFF);
    if !word.chars().any(folds) {
        return Cow::Borrowed(word);
    }
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        if !folds(c) {
            out.push(c);
            continue;
        }
        unicode_normalization::char::decompose_canonical(c, |d| match d as u32 {
            0x300..=0x36F => {}
            _ => match fold_letter(d) {
                Some(s) => out.push_str(s),
                None => out.push(d),
            },
        });
    }
    Cow::Owned(out)
}

/// Persian as Lucene's `PersianNormalizer` and `ArabicNormalizer` read
/// it: the Arabic and Persian forms of yeh, kaf and heh are one letter
/// each, alef loses its hamza or madda, and the vowel marks and the
/// tatweel go.
fn persian(token: &str) -> String {
    token
        .chars()
        .filter_map(|c| {
            Some(match c {
                'آ' | 'أ' | 'إ' => 'ا',
                'ى' | 'ي' | 'ے' => 'ی',
                'ك' => 'ک',
                'ة' | 'ۀ' | 'ہ' => 'ه',
                '\u{640}' | '\u{64b}'..='\u{652}' | '\u{654}' | '\u{670}' => return None,
                c => c,
            })
        })
        .collect()
}

/// Hindi, Bengali and Telugu: one spelling for the variants each is
/// written in, then a light stemmer that takes the longest inflectional
/// suffix off a word. The rules are those of Apache Lucene's
/// `HindiNormalizer` and `HindiStemmer` (after Larkey, Connell and
/// Abduljaleel, "Hindi CLIR in thirty days", and Ramanathan and Rao, "A
/// lightweight stemmer for Hindi"), `BengaliNormalizer` and
/// `BengaliStemmer` (after Sengupta and Chaudhuri, and Savoy's light
/// stemmer) and `TeluguNormalizer` and `TeluguStemmer`, with two
/// differences: the suffixes are normalized as the words are, so that
/// those with a long vowel still match, and the text is NFKC, in which the
/// Bengali letters with a nukta are two characters.
mod indic {
    use std::sync::OnceLock;

    use unicode_normalization::UnicodeNormalization;

    /// Hindi: `न` before a virama and the chandrabindu as the anusvara, no
    /// nukta, no virama, the short and the candra vowels as the plain
    /// ones, long vowels as short ones.
    pub fn hindi(token: &str) -> String {
        let cs: Vec<char> = token.chars().collect();
        let mut out = String::with_capacity(token.len());
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            i += 1;
            out.push(match c {
                'न' if cs.get(i) == Some(&'\u{94d}') => {
                    i += 1;
                    '\u{902}'
                }
                '\u{901}' => '\u{902}',
                '\u{93c}' | '\u{94d}' | '\u{200c}' | '\u{200d}' => continue,
                '\u{929}' => 'न',
                '\u{931}' => 'र',
                '\u{934}' => 'ळ',
                '\u{958}' => 'क',
                '\u{959}' => 'ख',
                '\u{95a}' => 'ग',
                '\u{95b}' => 'ज',
                '\u{95c}' => 'ड',
                '\u{95d}' => 'ढ',
                '\u{95e}' => 'फ',
                '\u{95f}' => 'य',
                'ऍ' | 'ऎ' | 'ऐ' => 'ए',
                'ऑ' | 'ऒ' | 'औ' => 'ओ',
                'ॲ' | 'आ' => 'अ',
                'ई' => 'इ',
                'ऊ' => 'उ',
                'ॠ' => 'ऋ',
                'ॡ' => 'ऌ',
                '\u{945}' | '\u{946}' | '\u{948}' => '\u{947}',
                '\u{949}' | '\u{94a}' | '\u{94c}' => '\u{94b}',
                '\u{940}' => '\u{93f}',
                '\u{942}' => '\u{941}',
                '\u{944}' => '\u{943}',
                '\u{963}' => '\u{962}',
                c => c,
            });
        }
        out
    }

    /// Telugu: the chandrabindus as the anusvara, no visarga, long vowels
    /// as short ones.
    pub fn telugu(token: &str) -> String {
        let cs: Vec<char> = token.chars().collect();
        let mut out = String::with_capacity(token.len());
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            i += 1;
            out.push(match c {
                '\u{c00}' | '\u{c01}' => '\u{c02}',
                '\u{c03}' | '\u{200c}' | '\u{200d}' => continue,
                'ఔ' => 'ఓ',
                'ఐ' => 'ఏ',
                'ఆ' => 'అ',
                'ఈ' => 'ఇ',
                'ఊ' => 'ఉ',
                '\u{c40}' => '\u{c3f}',
                '\u{c42}' => '\u{c41}',
                '\u{c47}' => '\u{c46}',
                '\u{c4b}' => '\u{c4a}',
                // The diphthongs written in two parts.
                '\u{c46}' if cs.get(i) == Some(&'\u{c56}') => {
                    i += 1;
                    '\u{c48}'
                }
                'ఒ' if cs.get(i) == Some(&'\u{c55}') => {
                    i += 1;
                    'ఓ'
                }
                'ఒ' if cs.get(i) == Some(&'\u{c4c}') => {
                    i += 1;
                    'ఔ'
                }
                c => c,
            });
        }
        out
    }

    /// Bengali: no chandrabindu, long i and u as short ones, `ঙ` as the
    /// anusvara, the ya-phala and the ba-phala (conjuncts that are not
    /// pronounced as written) gone, the visarga as what it sounds like,
    /// and one letter for each of the sibilants, the n's, the r's and the
    /// two t's.
    pub fn bengali(token: &str) -> String {
        const VIRAMA: char = '\u{9cd}';
        let mut s: Vec<char> = token.chars().collect();
        let mut i = 0isize;
        while (i as usize) < s.len() {
            let u = i as usize;
            match s[u] {
                '\u{981}' | '\u{200c}' | '\u{200d}' => {
                    s.remove(u);
                    i -= 1;
                }
                '\u{9c0}' => s[u] = '\u{9bf}',
                '\u{9c2}' => s[u] = '\u{9c1}',
                'ঙ' => s[u] = '\u{982}',
                // The ya-phala: after a word's first consonant it sounds
                // as e (and takes a following aa with it), elsewhere not
                // at all.
                'য' if s.get(u + 1) != Some(&'\u{9bc}') && u >= 1 && s[u - 1] == VIRAMA => {
                    if u == 2 {
                        s[u - 1] = '\u{9c7}';
                        if s.get(u + 1) == Some(&'\u{9be}') {
                            s.remove(u + 1);
                        }
                        s.remove(u);
                        i -= 1;
                    } else {
                        s.remove(u);
                        s.remove(u - 1);
                        i -= 2;
                    }
                }
                // The ba-phala: silent after a word's first consonant and
                // after a conjunct, elsewhere it doubles the consonant
                // before it.
                'ব' if u >= 1 && s[u - 1] == VIRAMA => {
                    if u == 2 || (u >= 5 && s[u - 3] == VIRAMA) {
                        s.remove(u);
                        s.remove(u - 1);
                        i -= 2;
                    } else if u >= 2 {
                        s[u - 1] = s[u - 2];
                        s.remove(u);
                        i -= 1;
                    }
                }
                // The visarga: h at the end of a short word, nothing at
                // the end of a longer one, inside a word the letter after
                // it, doubled.
                '\u{983}' => {
                    if u + 1 == s.len() {
                        if s.len() <= 3 {
                            s[u] = 'হ';
                        } else {
                            s.remove(u);
                            i -= 1;
                        }
                    } else {
                        s[u] = s[u + 1];
                    }
                }
                'শ' | 'ষ' => s[u] = 'স',
                'ণ' => s[u] = 'ন',
                // ড় and ঢ়, which NFKC writes as the letter and a nukta.
                '\u{9bc}' if u >= 1 && matches!(s[u - 1], 'ড' | 'ঢ') => {
                    s[u - 1] = 'র';
                    s.remove(u);
                    i -= 1;
                }
                '\u{9dc}' | '\u{9dd}' => s[u] = 'র',
                'ৎ' => s[u] = 'ত',
                _ => {}
            }
            i += 1;
        }
        s.into_iter().collect()
    }

    /// A stemmer's suffixes, in the order it tries them (longest first),
    /// normalized by `normalize` as the words are.
    fn suffixes(lists: &[&[&str]], normalize: fn(&str) -> String) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in lists.iter().flat_map(|l| l.iter()) {
            let s = normalize(&s.nfkc().collect::<String>());
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// `word` without the first of `suffixes` it ends with that leaves a
    /// stem of two characters or more.
    fn strip(word: String, suffixes: &[String]) -> String {
        let len = word.chars().count();
        match suffixes.iter().find(|s| word.ends_with(s.as_str()) && len > s.chars().count() + 1) {
            Some(s) => word[..word.len() - s.len()].to_string(),
            None => word,
        }
    }

    pub fn hindi_stem(word: String) -> String {
        static S: OnceLock<Vec<String>> = OnceLock::new();
        let s = S.get_or_init(|| {
            suffixes(
                &[
                    &["ाएंगी", "ाएंगे", "ाऊंगी", "ाऊंगा", "ाइयाँ", "ाइयों", "ाइयां"],
                    &["ाएगी", "ाएगा", "ाओगी", "ाओगे", "एंगी", "ेंगी", "एंगे", "ेंगे", "ूंगी", "ूंगा", "ातीं", "नाओं", "नाएं", "ताओं", "ताएं", "ियाँ", "ियों", "ियां"],
                    &["ाकर", "ाइए", "ाईं", "ाया", "ेगी", "ेगा", "ोगी", "ोगे", "ाने", "ाना", "ाते", "ाती", "ाता", "तीं", "ाओं", "ाएं", "ुओं", "ुएं", "ुआं"],
                    &["कर", "ाओ", "िए", "ाई", "ाए", "ने", "नी", "ना", "ते", "ीं", "ती", "ता", "ाँ", "ां", "ों", "ें"],
                    &["ो", "े", "ू", "ु", "ी", "ि", "ा"],
                ],
                hindi,
            )
        });
        strip(word, s)
    }

    pub fn telugu_stem(word: String) -> String {
        static S: OnceLock<Vec<String>> = OnceLock::new();
        let s = S.get_or_init(|| {
            suffixes(
                &[&["ళ్ళు", "డ్లు"], &["డు", "ము", "వు", "లు", "ని", "ను", "చే", "కై", "లో", "ది", "కి", "సు", "వై", "పై"], &["ి", "ీ", "ు", "ూ", "ె", "ే", "ొ", "ో", "ా"]],
                telugu,
            )
        });
        strip(word, s)
    }

    pub fn bengali_stem(word: String) -> String {
        static S: OnceLock<Vec<String>> = OnceLock::new();
        let s = S.get_or_init(|| {
            suffixes(
                &[
                    &["িয়াছিলাম", "িতেছিলাম", "িতেছিলেন", "ইতেছিলেন", "িয়াছিলেন", "ইয়াছিলেন"],
                    &["িতেছিলি", "িতেছিলে", "িয়াছিলা", "িয়াছিলে", "িতেছিলা", "িয়াছিলি", "য়েদেরকে"],
                    &["িতেছিস", "িতেছেন", "িয়াছিস", "িয়াছেন", "েছিলাম", "েছিলেন", "েদেরকে"],
                    &["িতেছি", "িতেছা", "িতেছে", "ছিলাম", "ছিলেন", "িয়াছি", "িয়াছা", "িয়াছে", "েছিলে", "েছিলা", "য়েদের", "দেরকে"],
                    &["িলাম", "িলেন", "িতাম", "িতেন", "িবেন", "ছিলি", "ছিলে", "ছিলা", "তেছে", "িতেছ", "খানা", "খানি", "গুলো", "গুলি", "য়েরা", "েদের"],
                    &[
                        "লাম", "িলি", "ইলি", "িলে", "ইলে", "লেন", "িলা", "ইলা", "তাম", "িতি", "ইতি", "িতে", "ইতে", "তেন", "িতা", "িবা", "ইবা", "িবি", "ইবি", "বেন", "িবে", "ইবে", "ছেন",
                        "য়োন", "য়ের", "েরা", "দের",
                    ],
                    &["িস", "েন", "লি", "লে", "লা", "তি", "তে", "তা", "বি", "বে", "বা", "ছি", "ছা", "ছে", "ুন", "ুক", "টা", "টি", "নি", "ের", "রা", "কে"],
                    &["ি", "ী", "া", "ো", "ে", "ব", "ত"],
                ],
                bengali,
            )
        });
        strip(word, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str, language: Option<Language>) -> Vec<String> {
        let mut out = Vec::new();
        for_each_token(text, language, false, |t| out.push(t.to_string()));
        out
    }

    /// As `cjk` splits.
    fn bigrams(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for_each_token(text, None, true, |t| out.push(t.to_string()));
        out
    }

    fn terms(text: &str, language: Language) -> Vec<String> {
        tokens(text, Some(language)).iter().filter_map(|t| term_of(t, Some(language)).map(|t| t.into_owned())).collect()
    }

    #[test]
    fn splits_by_script() {
        let t = |s: &str| tokens(s, None);
        // Chinese and Japanese: dictionary words.
        assert_eq!(t("東京都庁に行きました"), ["東京", "都庁", "に", "行き", "まし", "た"]);
        assert_eq!(t("我爱北京天安门"), ["我", "爱", "北京", "天安门"]);
        assert_eq!(t("コンピュータサイエンスを勉強"), ["コンピュータサイエンス", "を", "勉強"]);
        assert_eq!(t("東"), ["東"]);
        // `cjk`: bigrams, across Han and kana.
        assert_eq!(bigrams("東京都庁に行きました"), ["東京", "京都", "都庁", "庁に", "に行", "行き", "きま", "まし", "した"]);
        assert_eq!(bigrams("我爱北京"), ["我爱", "爱北", "北京"]);
        assert_eq!(bigrams("東"), ["東"]);
        assert_eq!(bigrams("東京"), ["東京"]);
        assert_eq!(bigrams("ภาษาไทย café"), t("ภาษาไทย café"));
        // Korean: bigrams of each word, so a word without its particle is found.
        assert_eq!(t("서울특별시에 갑니다"), ["서울", "울특", "특별", "별시", "시에", "갑니", "니다"]);
        // Thai, Lao, Khmer, Burmese: dictionary words.
        assert_eq!(t("ภาษาไทยง่ายมาก"), ["ภาษา", "ไทย", "ง่าย", "มาก"]);
        assert_eq!(t("ຂ້ອຍຢາກໄປຕະຫຼາດ"), ["ຂ້ອຍ", "ຢາກ", "ໄປ", "ຕະຫຼາດ"]);
        assert_eq!(t("ខ្ញុំចង់ទៅផ្សារ").len(), 3);
        assert_eq!(t("ကျွန်တော်မြန်မာ"), ["ကျွန်တော်", "မြန်မာ"]);
        // Indic scripts keep their viramas and vowel signs in the word.
        assert_eq!(t("हिन्दी भाषा"), ["हिन्दी", "भाषा"]);
        assert_eq!(t("தமிழ் மொழி"), ["தமிழ்", "மொழி"]);
        assert_eq!(t("বাংলা ভাষা"), ["বাংলা", "ভাষা"]);
        // Arabic keeps its vowel marks at this step (a language removes them).
        assert_eq!(t("مَكْتَبَة"), ["مَكْتَبَة"]);
        // Scripts change inside a run of characters.
        assert_eq!(t("東京2020オリンピック"), ["東京", "2020", "オリンピック"]);
        assert_eq!(bigrams("東京2020オリンピック"), ["東京", "2020", "オリ", "リン", "ンピ", "ピッ", "ック"]);
        assert_eq!(bigrams("iPhone15を買った"), ["iphone15", "を買", "買っ", "った"]);
        assert_eq!(t("한글漢字"), ["한글", "漢字"]);
        assert_eq!(t("ไทย2024"), ["ไทย", "2024"]);
    }

    #[test]
    fn normalizes() {
        let t = |s: &str| tokens(s, None);
        assert_eq!(t("ｆｕｌｌｗｉｄｔｈ ﬁle Ⅻ ①"), ["fullwidth", "file", "xii", "1"]);
        assert_eq!(bigrams("ﾊﾝｶｸ"), ["ハン", "ンカ", "カク"]);
        assert_eq!(t("cafe\u{301} CAFÉ"), ["café", "café"]);
        assert_eq!(t("ΟΔΟΣ οδός"), ["οδοσ", "οδόσ"]);
        assert_eq!(t("İSTANBUL Iğdır"), ["istanbul", "iğdır"]);
        assert_eq!(tokens("İSTANBUL Iğdır KIŞ", Some(Language::Turkish)), ["istanbul", "ığdır", "kış"]);
        assert_eq!(t("co\u{ad}operate zero\u{200d}width"), ["cooperate", "zerowidth"]);
        assert_eq!(t("می\u{200c}خواهم"), ["میخواهم"]);
        assert_eq!(tokens("می\u{200c}خواهم", Some(Language::Persian)), ["می", "خواهم"]);
        assert_eq!(t("Plain ASCII, Mixed-Case"), ["plain", "ascii", "mixed", "case"]);
    }

    #[test]
    fn words_and_numbers() {
        let t = |s: &str| tokens(s, None);
        assert_eq!(t("pi is 3.14, or 1,000.5; v1.2.3"), ["pi", "is", "3.14", "or", "1,000.5", "v1.2.3"]);
        assert_eq!(t("end. 3. a,b 4,x x.5"), ["end", "3", "a", "b", "4", "x", "x", "5"]);
        assert_eq!(t("don't l'avion snake_case e-mail example.com"), ["don", "t", "l", "avion", "snake", "case", "e", "mail", "example", "com"]);
        assert_eq!(t("Москва Ελλάδα ישראל"), ["москва", "ελλάδα", "ישראל"]);
        assert!(t("").is_empty());
        assert!(t(" \u{3000}、。!? 🙂").is_empty());
    }

    #[test]
    fn languages() {
        assert_eq!(terms("Les élèves mangeaient des chevaux", Language::French), ["le", "elev", "mang", "de", "cheval"]);
        assert_eq!(terms("eleves", Language::French), ["elev"]);
        assert_eq!(terms("Häuser und Bücher Straße", Language::German), ["haus", "und", "buch", "strass"]);
        assert_eq!(terms("Los niños corrían rápidamente", Language::Spanish), ["los", "nin", "corr", "rapid"]);
        assert_eq!(terms("Книги и библиотеки", Language::Russian), ["книг", "и", "библиотек"]);
        assert_eq!(terms("ανθρώπου Άνθρωπος", Language::Greek), ["ανθρωπ", "ανθρωπ"]);
        assert_eq!(terms("KİTAPLAR ışıklar", Language::Turkish), ["kitap", "isik"]);
        assert_eq!(terms("Ёлка ёж за\u{301}мок Йод", Language::Russian), terms("елка еж замок йод", Language::Russian));
        assert_ne!(terms("йод", Language::Russian), terms("иод", Language::Russian));
        // Arabic: with or without vowel marks, tatweel and hamza forms.
        let ar = terms("مَكْتَبَة مكتبة مكتبـــة المكتبة", Language::Arabic);
        assert!(ar.iter().all(|t| t == "مكتب"), "{ar:?}");
        assert_eq!(terms("أحمد", Language::Arabic), terms("احمد", Language::Arabic));
        // Persian: Arabic and Persian letter forms are one.
        assert_eq!(terms("كتاب\u{64b} کتاب", Language::Persian), ["کتاب", "کتاب"]);
        assert_eq!(terms("ايران", Language::Persian), terms("ایران", Language::Persian));
        // Hindi: spelling variants and inflections.
        assert_eq!(terms("हिन्दी हिंदी", Language::Hindi), ["हिंद", "हिंद"]);
        assert_eq!(terms("लड़का लड़के लड़कों", Language::Hindi), ["लडक", "लडक", "लडक"]);
        assert_eq!(terms("क़िताब किताब", Language::Hindi), ["किताब", "किताब"]);
        // Bengali: plurals, cases and spellings that sound the same.
        assert_eq!(terms("ছেলেরা ছেলেদের", Language::Bengali), ["ছেল", "ছেল"]);
        assert_eq!(terms("বইগুলো বইটি বই", Language::Bengali), ["বই", "বই", "বই"]);
        assert_eq!(terms("বাড়ি বাড়ী", Language::Bengali), terms("বারি বারি", Language::Bengali));
        assert_eq!(terms("বিশ্ব", Language::Bengali), ["বিসস"]);
        // Telugu: case endings and long vowels.
        assert_eq!(terms("పుస్తకాలను పుస్తకాలలో", Language::Telugu), ["పుస్తకాల", "పుస్తకాల"]);
        assert_eq!(terms("గ్రామం గ్రామంలో", Language::Telugu), ["గ్రామం", "గ్రామం"]);
        assert_eq!(terms("ఆంధ్ర", Language::Telugu), terms("అంధ్ర", Language::Telugu));
        // Chinese: traditional characters as the simplified ones, then bigrams.
        assert_eq!(terms("開發中國", Language::Chinese), terms("开发中国", Language::Chinese));
        assert_eq!(terms("開發中國", Language::Chinese), ["开发", "中国"]);
        assert_eq!(terms("開發", Language::French), ["開發"]);
        // A language leaves other scripts' bigrams and dictionary words alone.
        assert_eq!(terms("東京 ภาษาไทย", Language::French), ["東京", "ภาษา", "ไทย"]);
    }

    #[test]
    fn folds_accents() {
        let t = |s: &str| tokens(s, None).iter().filter_map(|t| term_of(t, None).map(|t| t.into_owned())).collect::<Vec<_>>();
        assert_eq!(t("Café naïve Œuvre straße Smørrebrød Łódź"), ["cafe", "naive", "oeuvre", "strasse", "smorrebrod", "lodz"]);
        // Yoruba and Vietnamese: marks with and without a composed form.
        assert_eq!(t("akọkọ ẹ̀kọ́ ṣe Việt"), ["akoko", "eko", "se", "viet"]);
        assert_eq!(t("Άνθρωπος ΟΔΌΣ οδός"), ["ανθρωποσ", "οδοσ", "οδοσ"]);
        // Other scripts keep their marks: they are vowels and tones.
        assert_eq!(t("हिन्दी ง่าย مَكْتَبَة 東京"), ["हिन्दी", "ง่าย", "مَكْتَبَة", "東京"]);
        assert_eq!(prefix_of("café"), "cafe");
        assert_eq!(prefix_of("東京"), "東京");
    }

    // Any text at all: tokens and terms are never empty.
    #[test]
    fn arbitrary_text() {
        let pool: Vec<char> = "aZ9 .,'’-_İIςßéё\u{301}\u{300}\u{ad}\u{200c}\u{200d}東京にカ々ー한글ไท่ๆ๑ລາខ្មမြहि्ंक़ড়தمَـآیﬁＡ①㍿🙂\u{0}\u{10ffff}\u{e000}\n".chars().collect();
        let mut x = 0x9e3779b97f4a7c15u64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for round in 0..3000 {
            let len = (next() % 40) as usize;
            let text: String = (0..len).map(|_| if next() % 8 == 0 { char::from_u32((next() % 0x30000) as u32).unwrap_or('x') } else { pool[(next() % pool.len() as u64) as usize] }).collect();
            let language = match round % 4 {
                0 => None,
                _ => Some(Language::ALL[(next() % Language::ALL.len() as u64) as usize]),
            };
            let toks = if round % 5 == 0 { bigrams(&text) } else { tokens(&text, language) };
            let language = language.filter(|_| round % 5 != 0);
            assert!(toks.iter().all(|t| !t.is_empty()), "{text:?}");
            let terms: Vec<_> = toks.iter().filter_map(|t| term_of(t, language).map(|t| t.into_owned())).collect();
            assert!(terms.iter().all(|t| !t.is_empty()), "{text:?}");
        }
    }

    #[test]
    fn long_runs_are_fast() {
        let t0 = std::time::Instant::now();
        assert_eq!(bigrams(&"東".repeat(1 << 18)).len(), (1 << 18) - 1);
        assert_eq!(tokens(&"東京".repeat(1 << 16), None).concat().chars().count(), 1 << 17);
        assert_eq!(tokens(&"ก".repeat(1 << 16), None).concat().chars().count(), 1 << 16);
        let huge = "y".repeat(1 << 20);
        assert_eq!(terms(&huge, Language::French), [huge.clone()]);
        assert!(t0.elapsed() < std::time::Duration::from_secs(5), "{:?}", t0.elapsed());
    }
}
