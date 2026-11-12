//! Content understanding: built-in models that make `KEYWORD` values of a
//! row's text, for a derived field (`lang KEYWORD OPTIONS(derive = 'body',
//! model = 'builtin:lang')`). They need no model files and run in the
//! writer's thread, before the write queues, as derived text does
//! ([`crate::derive`]); the values are stored and logged with the row, so
//! merges and replicas never run them.
//!
//! * [`LangDetector`] (`builtin:lang`, a `KEYWORD`): the language of a
//!   text, as an ISO 639-1 code (`en`, `de`, `zh`), or `und`. The script
//!   decides most (Han, kana, Hangul, Cyrillic, Arabic, Greek, Devanagari,
//!   Thai, ...), and a Latin text goes by the function words it uses most.
//!   [`analyzer_name`] maps a code to the analyzer for it.
//! * [`Keyphrases`] (`builtin:keyphrases`, a `KEYWORD[]`): a text's five
//!   main phrases by RAKE (Rose et al. 2010): runs of words between
//!   stopwords and punctuation, each scored by the sum over its words of
//!   their co-occurrence degree over their frequency.
//! * [`MinHash`] (`builtin:minhash`, a `KEYWORD[]`): 8 locality-sensitive
//!   bands of a 32-hash MinHash of the text's word 3-grams: two rows that
//!   share a band are likely near duplicates (`GROUP BY` the field, or a
//!   self-join on it). Texts whose 3-grams overlap by a Jaccard of 0.8
//!   share a band 98.5% of the time, by 0.5 40%, by 0.2 1.3%
//!   (`1 - (1 - s^4)^8`).
//! * [`DictEntities`] (`dict:<table>.<field>`, a `KEYWORD[]`): the values of
//!   another table's `KEYWORD` field the text names in full, as query
//!   understanding finds them ([`crate::query::understand`]): a list of
//!   brands, places or products to tag the rows that mention them.
//!
//! The hashes and word lists are part of the values written: a later
//! version that changes them is a new model name.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::derive::Deriver;
use crate::embed::{Input, Modality};
use crate::error::{Error, Result};
use crate::query::understand::Gazetteer;
use crate::text::Analyzer;

/// The prefix of the built-in models' names.
pub const BUILTIN: &str = "builtin:";
/// The prefix of a dictionary's model name: `dict:brands.name`.
pub const DICT: &str = "dict:";
/// The prefix of a classifier's name as a model, `classifier:<name>`: a
/// classifier `CREATE CLASSIFIER` made ([`crate::query::classify`]).
pub const CLASSIFIER: &str = "classifier:";
/// The built-in models, with the values they make.
pub const MODELS: [(&str, &str); 3] = [("builtin:lang", "KEYWORD"), ("builtin:keyphrases", "KEYWORD[]"), ("builtin:minhash", "KEYWORD[]")];
/// The values of a `KEYWORD[]` model's output for one row, joined by this
/// in a [`Deriver`]'s text.
pub const SEP: char = '\n';

fn err(m: impl Into<String>) -> Error {
    Error::Schema(m.into())
}

/// Whether a model's name is one of this module's: `builtin:...` or
/// `dict:...`.
pub fn is_content_model(model: &str) -> bool {
    model.starts_with(BUILTIN) || model.starts_with(DICT) || model.starts_with(CLASSIFIER)
}

/// The classifier a model of name `classifier:<name>` names.
pub fn classifier_name(model: &str) -> Option<&str> {
    model.strip_prefix(CLASSIFIER).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// Whether model `model` makes several values a row (a `KEYWORD[]`
/// field), or one (`KEYWORD`); `None` for a name that is not one.
pub fn makes_many(model: &str) -> Option<bool> {
    if let Some(d) = model.strip_prefix(DICT) {
        return d.split_once('.').filter(|(t, f)| !t.is_empty() && !f.is_empty()).map(|_| true);
    }
    MODELS.iter().find(|(m, _)| *m == model).map(|(_, ty)| ty.ends_with("[]"))
}

/// The built-in model of that name (not a dictionary, which the database
/// opens with its table's values).
pub fn open(model: &str) -> Result<Arc<dyn Deriver>> {
    match model {
        "builtin:lang" => Ok(Arc::new(LangDetector)),
        "builtin:keyphrases" => Ok(Arc::new(Keyphrases { k: 5 })),
        "builtin:minhash" => Ok(Arc::new(MinHash)),
        _ => Err(err(format!("model {model} is not a built-in model (they are {})", MODELS.map(|(m, _)| m).join(", ")))),
    }
}

fn texts<'a>(name: &str, inputs: &'a [Input<'_>]) -> Result<Vec<&'a str>> {
    inputs
        .iter()
        .map(|i| match i {
            Input::Text(t) => Ok(*t),
            i => Err(err(format!("model {name} reads text, not {}s", i.modality().as_str()))),
        })
        .collect()
}

// ---- builtin:lang ----

/// `builtin:lang`: a text's language as an ISO 639-1 code.
pub struct LangDetector;

impl Deriver for LangDetector {
    fn name(&self) -> String {
        "builtin:lang".into()
    }
    fn modality(&self) -> Modality {
        Modality::Text
    }
    fn derive(&self, inputs: &[Input<'_>]) -> Result<Vec<String>> {
        Ok(texts("builtin:lang", inputs)?.into_iter().map(|t| detect(t).to_string()).collect())
    }
}

/// The analyzer for a language's text, by the names analyzers have
/// (`english`, `german`, ...; `standard` for one with no stemmer, and for
/// Chinese, Japanese and Korean, which a segmenter splits).
pub fn analyzer_name(code: &str) -> &'static str {
    match code {
        "en" => "english",
        "ar" => "arabic",
        "da" => "danish",
        "nl" => "dutch",
        "fi" => "finnish",
        "fr" => "french",
        "de" => "german",
        "el" => "greek",
        "hi" => "hindi",
        "hu" => "hungarian",
        "it" => "italian",
        "no" | "nb" | "nn" => "norwegian",
        "fa" => "persian",
        "pt" => "portuguese",
        "ro" => "romanian",
        "ru" => "russian",
        "es" => "spanish",
        "sv" => "swedish",
        "ta" => "tamil",
        "tr" => "turkish",
        _ => "standard",
    }
}

/// Function words of the languages written in Latin letters, the most
/// frequent first. A word in several lists counts for each.
const LATIN: &[(&str, &[&str])] = &[
    ("en", &["the", "of", "and", "to", "in", "is", "that", "for", "it", "with", "as", "was", "on", "are", "be", "this", "by", "you", "not", "or", "have", "from", "at", "which", "but", "they", "his", "her", "we", "an", "were", "been", "has", "their", "would", "there", "what", "will", "can", "when", "who", "all", "if", "so", "about", "more", "one", "my", "your"]),
    ("de", &["der", "die", "und", "in", "den", "von", "zu", "das", "mit", "sich", "des", "auf", "für", "ist", "im", "dem", "nicht", "ein", "eine", "als", "auch", "es", "an", "werden", "aus", "er", "hat", "dass", "sie", "nach", "wird", "bei", "einer", "um", "am", "sind", "noch", "wie", "einem", "über", "einen", "so", "zum", "war", "haben", "nur", "oder", "aber", "ich", "wir"]),
    ("fr", &["de", "la", "le", "et", "les", "des", "en", "un", "du", "une", "que", "est", "pour", "qui", "dans", "a", "par", "plus", "pas", "au", "sur", "ne", "se", "ce", "il", "sont", "avec", "son", "aux", "ou", "cette", "elle", "nous", "vous", "mais", "ont", "été", "je", "leur", "comme", "tout", "fait", "sa", "ses", "y", "était", "même", "deux", "où"]),
    ("es", &["de", "la", "que", "el", "en", "y", "los", "del", "se", "las", "por", "un", "para", "con", "no", "una", "su", "al", "es", "lo", "como", "más", "pero", "sus", "le", "ya", "o", "fue", "este", "ha", "sí", "porque", "esta", "son", "entre", "está", "cuando", "muy", "sin", "sobre", "ser", "también", "me", "hasta", "hay", "donde", "han", "desde", "todo", "nos"]),
    ("it", &["di", "e", "il", "la", "che", "in", "a", "per", "un", "del", "non", "è", "una", "della", "i", "le", "si", "da", "al", "con", "sono", "dei", "come", "anche", "nel", "lo", "più", "ma", "gli", "alla", "ha", "se", "delle", "questo", "ci", "o", "nella", "suo", "essere", "tra", "loro", "mi", "ho", "molto", "sua", "stato", "quando", "dalla", "ne", "cui"]),
    ("pt", &["de", "a", "o", "que", "e", "do", "da", "em", "um", "para", "é", "com", "não", "uma", "os", "no", "se", "na", "por", "mais", "as", "dos", "como", "mas", "foi", "ao", "ele", "das", "tem", "à", "seu", "sua", "ou", "ser", "quando", "muito", "há", "nos", "já", "está", "eu", "também", "só", "pelo", "pela", "até", "isso", "ela", "entre", "era"]),
    ("nl", &["de", "en", "van", "het", "een", "in", "is", "dat", "op", "te", "zijn", "voor", "met", "die", "niet", "aan", "er", "om", "ook", "als", "bij", "door", "maar", "of", "naar", "uit", "wordt", "nog", "dan", "kan", "worden", "over", "tot", "zo", "hij", "ze", "wat", "werd", "meer", "al", "geen", "wel", "jaar", "hun", "deze", "heeft", "was", "zich", "ik", "we"]),
    ("sv", &["och", "i", "att", "det", "som", "en", "på", "är", "av", "för", "med", "till", "den", "har", "de", "inte", "om", "ett", "han", "men", "var", "jag", "sig", "från", "vi", "så", "kan", "man", "när", "år", "säger", "hon", "under", "också", "efter", "eller", "nu", "sin", "där", "vid", "mot", "ska", "skulle", "kommer", "ut", "får", "finns", "vara", "hade", "alla"]),
    ("da", &["og", "i", "at", "det", "er", "en", "til", "på", "som", "de", "med", "for", "af", "den", "har", "ikke", "et", "der", "om", "fra", "var", "kan", "jeg", "men", "vi", "så", "eller", "han", "sig", "skal", "også", "efter", "blev", "være", "hun", "nu", "når", "hvor", "ved", "mod", "sin", "op", "ud", "meget", "havde", "alle", "vil", "deres", "under", "år"]),
    ("no", &["og", "i", "det", "på", "som", "er", "en", "til", "å", "har", "de", "med", "for", "av", "at", "ikke", "et", "den", "om", "jeg", "fra", "var", "men", "vi", "så", "kan", "seg", "han", "eller", "skal", "også", "etter", "ble", "være", "hun", "nå", "når", "hvor", "ved", "mot", "sin", "opp", "ut", "mye", "hadde", "alle", "vil", "deres", "under", "år"]),
    ("fi", &["ja", "on", "ei", "että", "se", "oli", "hän", "mutta", "kun", "ovat", "myös", "niin", "joka", "tai", "sen", "kuin", "ole", "vain", "tämä", "jo", "nyt", "olla", "voi", "sitä", "jos", "mukaan", "sekä", "hänen", "ne", "mitä", "sitten", "minä", "ollut", "siitä", "kanssa", "pitää", "olisi", "kaikki", "vielä", "näin", "jotka", "koska", "aina", "sillä", "siis", "eikä", "me", "te", "he", "tässä"]),
    ("pl", &["i", "w", "się", "na", "nie", "z", "do", "to", "że", "jest", "o", "jak", "ale", "po", "co", "tak", "za", "od", "jego", "go", "przez", "już", "tylko", "jej", "być", "a", "czy", "może", "są", "dla", "który", "tym", "ich", "bardzo", "jeszcze", "też", "gdy", "tego", "było", "mnie", "kiedy", "jako", "pan", "ma", "lub", "tu", "przy", "gdzie", "była", "oraz"]),
    ("cs", &["a", "se", "na", "v", "je", "že", "to", "s", "z", "do", "o", "i", "jsou", "jako", "ale", "by", "k", "pro", "jeho", "které", "který", "jak", "tak", "však", "jen", "po", "od", "být", "bylo", "byl", "jsem", "už", "také", "nebo", "při", "této", "této", "mezi", "jejich", "když", "této", "aby", "ve", "za", "ze", "co", "má", "této", "podle", "než"]),
    ("ro", &["și", "de", "în", "a", "la", "cu", "o", "pe", "care", "nu", "din", "este", "un", "să", "că", "se", "mai", "au", "pentru", "lui", "fost", "sunt", "dar", "ca", "fi", "ce", "va", "prin", "sau", "după", "acest", "această", "ei", "am", "lor", "el", "ea", "foarte", "fără", "când", "doar", "până", "unde", "acum", "toate", "noi", "aici", "ale", "cel", "le"]),
    ("hu", &["a", "az", "és", "hogy", "nem", "is", "egy", "meg", "de", "van", "volt", "csak", "már", "még", "mint", "ez", "azt", "el", "ki", "fel", "vagy", "kell", "lesz", "pedig", "mert", "sem", "lehet", "minden", "ha", "nagyon", "után", "így", "ezt", "akkor", "most", "amely", "által", "között", "amit", "olyan", "majd", "szerint", "valamint", "illetve", "hanem", "ahol", "egyik", "sok", "rá", "mi"]),
    ("tr", &["ve", "bir", "bu", "da", "de", "için", "ile", "olarak", "çok", "daha", "ne", "o", "ama", "gibi", "en", "kadar", "sonra", "var", "değil", "olan", "her", "ki", "mi", "ben", "sen", "biz", "onun", "veya", "ancak", "yani", "şey", "göre", "olduğunu", "üzerinde", "tarafından", "arasında", "yok", "nasıl", "neden", "şimdi", "bütün", "hem", "ise", "zaman", "olduğu", "fazla", "önce", "yeni", "büyük", "iki"]),
    ("id", &["yang", "dan", "di", "ini", "itu", "dengan", "untuk", "tidak", "dari", "dalam", "akan", "pada", "juga", "saya", "ke", "karena", "ada", "bisa", "ia", "mereka", "kami", "kita", "atau", "oleh", "sudah", "telah", "lebih", "seperti", "hanya", "satu", "tahun", "orang", "tersebut", "bahwa", "kepada", "saat", "masih", "jika", "harus", "banyak", "dia", "sebagai", "hal", "belum", "namun", "agar", "secara", "bagi", "para", "dapat"]),
    ("sw", &["na", "ya", "wa", "kwa", "katika", "ni", "la", "za", "kuwa", "hiyo", "huo", "cha", "hii", "pia", "kama", "lakini", "au", "baada", "hata", "zaidi", "sana", "kwamba", "wake", "yake", "vya", "kati", "watu", "mwaka", "hapa", "kutoka", "bado", "hilo", "ambao", "ambayo", "hadi", "tu", "kila", "wao", "mimi", "yeye", "sisi", "nini", "kuna", "alisema", "mtu", "moja", "hayo", "huu", "wengi", "kubwa"]),
    ("vi", &["và", "của", "là", "có", "các", "trong", "được", "cho", "không", "một", "với", "những", "người", "đã", "này", "để", "khi", "từ", "đến", "ra", "về", "cũng", "như", "theo", "tại", "sẽ", "năm", "nhiều", "đó", "bị", "vào", "nhưng", "thì", "lại", "hơn", "còn", "phải", "mà", "nên", "nếu", "đang", "rất", "tôi", "chúng", "sau", "thể", "nào", "vì", "đi", "làm"]),
    ("ca", &["de", "la", "i", "el", "que", "a", "les", "en", "per", "del", "és", "un", "amb", "una", "no", "els", "es", "al", "com", "va", "més", "o", "ha", "però", "seu", "seva", "són", "també", "dels", "aquesta", "aquest", "molt", "hi", "ser", "ja", "quan", "tot", "fins", "entre", "sense", "sobre", "pel", "han", "perquè", "on", "si", "ens", "jo", "tots", "dues"]),
];

/// Function words of Russian and Bulgarian, which share most letters.
const CYRILLIC: &[(&str, &[&str])] = &[
    ("ru", &["и", "в", "не", "на", "что", "он", "с", "как", "а", "то", "все", "она", "так", "его", "но", "это", "по", "к", "у", "же", "вы", "за", "бы", "только", "ее", "мне", "было", "вот", "от", "меня", "еще", "нет", "о", "из", "ему", "теперь", "когда", "уже", "для", "был", "была", "были", "очень", "может", "или", "при", "чтобы", "этот", "который", "также"]),
    ("bg", &["и", "на", "в", "за", "да", "се", "от", "е", "с", "че", "не", "са", "по", "това", "като", "той", "ще", "си", "му", "го", "но", "тя", "има", "до", "след", "още", "които", "който", "която", "може", "беше", "бъде", "при", "или", "те", "както", "така", "много", "този", "тази", "тези", "към", "във", "със", "един", "една", "една", "вече", "само", "между"]),
];

/// The language of a text: its ISO 639-1 code, or `und` when there are
/// no letters to go by.
pub fn detect(text: &str) -> &'static str {
    // Letters by script.
    let mut n = HashMap::<&'static str, usize>::new();
    let mut letters = 0usize;
    for c in text.chars() {
        if !c.is_alphabetic() {
            continue;
        }
        letters += 1;
        let s = match c as u32 {
            0x3040..=0x30FF | 0x31F0..=0x31FF => "kana",
            0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => "hangul",
            0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x20000..=0x2FFFF => "han",
            0x0400..=0x04FF => "cyrillic",
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "arabic",
            0x0370..=0x03FF | 0x1F00..=0x1FFF => "greek",
            0x0590..=0x05FF => "hebrew",
            0x0900..=0x097F => "devanagari",
            0x0980..=0x09FF => "bengali",
            0x0B80..=0x0BFF => "tamil",
            0x0C00..=0x0C7F => "telugu",
            0x0E00..=0x0E7F => "thai",
            0x10A0..=0x10FF => "georgian",
            0x0530..=0x058F => "armenian",
            _ if c.is_ascii_alphabetic() || (0x00C0..=0x024F).contains(&(c as u32)) || (0x1E00..=0x1EFF).contains(&(c as u32)) => "latin",
            _ => "other",
        };
        *n.entry(s).or_default() += 1;
    }
    if letters == 0 {
        return "und";
    }
    let count = |s: &str| n.get(s).copied().unwrap_or(0);
    // Japanese mixes kana with Han; Chinese is Han alone.
    if count("kana") * 20 >= letters && count("kana") > 0 {
        return "ja";
    }
    if count("hangul") * 2 >= letters {
        return "ko";
    }
    if count("han") * 2 >= letters {
        return "zh";
    }
    let (script, most) = n.iter().max_by_key(|(_, c)| **c).map(|(s, c)| (*s, *c)).unwrap_or(("other", 0));
    if most * 2 < letters && script != "latin" {
        return "und";
    }
    let has = |chars: &str| text.chars().any(|c| chars.contains(c));
    match script {
        "cyrillic" => {
            if has("іїєґІЇЄҐ") {
                "uk"
            } else if has("ыэЫЭёЁ") {
                "ru"
            } else if has("ўЎ") {
                "be"
            } else if has("љњџЉЊЏ") {
                "sr"
            } else {
                // Without letters of its own: Bulgarian's words, or its ъ.
                match by_words(text, CYRILLIC, |_| "") {
                    "und" if has("ъЪ") => "bg",
                    "und" => "ru",
                    code => code,
                }
            }
        }
        "arabic" => {
            if has("ٹڈڑںےۓھ") {
                "ur"
            } else if has("پچژگ") || (has("یک") && !has("يك")) {
                "fa"
            } else {
                "ar"
            }
        }
        "greek" => "el",
        "hebrew" => "he",
        "devanagari" => "hi",
        "bengali" => "bn",
        "tamil" => "ta",
        "telugu" => "te",
        "thai" => "th",
        "georgian" => "ka",
        "armenian" => "hy",
        "latin" => latin(text),
        _ => "und",
    }
}

/// The language of a text in Latin letters, by its function words; a few
/// letters only one language uses decide a tie.
fn latin(text: &str) -> &'static str {
    by_words(text, LATIN, latin_marks)
}

/// Letters of one language among those written in Latin letters.
fn latin_marks(code: &str) -> &'static str {
    match code {
        "vi" => "ơưđạảấầẩẫậắằẳẵặẹẻẽếềểễệỉịọỏốồổỗộớờởỡợụủứừửữựỳỵỷỹ",
        "tr" => "ğışİ",
        "pl" => "łąęśżźćń",
        "ro" => "țșăâî",
        "cs" => "řůěčšž",
        "hu" => "őű",
        "de" => "äöüß",
        "sv" | "fi" => "äöå",
        "da" | "no" => "æøå",
        "es" => "ñ¿¡",
        "pt" => "ãõç",
        "fr" => "çœèêàù",
        "it" => "àèìòù",
        "ca" => "çŀ·",
        _ => "",
    }
}

/// The language of `lists` whose function words, and letters of its own
/// (`marks`), `text` holds most; `und` for none.
fn by_words(text: &str, lists: &[(&'static str, &[&str])], marks: impl Fn(&str) -> &'static str) -> &'static str {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut best = ("und", 0.0f64);
    for (code, list) in lists {
        let mut score = 0.0;
        for w in &words {
            if let Some(r) = list.iter().position(|x| x == w) {
                // The most frequent words say the most.
                score += 1.0 + 1.0 / (1.0 + r as f64 / 10.0);
            }
        }
        // Letters of one language.
        let marks = marks(code);
        let marked = lower.chars().filter(|c| marks.contains(*c)).count();
        score += 0.5 * (marked as f64).min(words.len() as f64 / 2.0);
        if score > best.1 {
            best = (code, score);
        }
    }
    best.0
}

// ---- builtin:keyphrases ----

/// `builtin:keyphrases`: a text's `k` main phrases by RAKE, best first.
pub struct Keyphrases {
    pub k: usize,
}

/// Words that end a phrase: the analyzers' query stopwords and other
/// words that say little alone.
const PHRASE_STOPS: &[&str] = &[
    "also", "however", "may", "might", "must", "shall", "should", "would", "could", "can", "will", "one", "two", "three", "use", "used", "using", "based", "new",
    "well", "within", "without", "via", "among", "upon", "per", "whether", "whose", "yet", "thus", "therefore", "hence", "although", "though", "since", "many", "much",
    "several", "various", "including", "include", "includes", "show", "shows", "shown", "found", "make", "makes", "made", "like", "get", "got", "e", "g", "i", "etc",
];

fn is_stop(w: &str) -> bool {
    crate::text::analyzer::QUERY_STOPWORDS.contains(&w) || crate::text::analyzer::STOPWORDS.contains(&w) || PHRASE_STOPS.contains(&w)
}

impl Keyphrases {
    /// The phrases of `text`, best first.
    pub fn of(&self, text: &str) -> Vec<String> {
        // Candidates: runs of words between stopwords, numbers and
        // punctuation that ends a clause.
        let mut phrases: Vec<Vec<String>> = Vec::new();
        let mut cur: Vec<String> = Vec::new();
        let mut word = String::new();
        let end = |cur: &mut Vec<String>, phrases: &mut Vec<Vec<String>>| {
            if !cur.is_empty() {
                phrases.push(std::mem::take(cur));
            }
        };
        for c in text.chars().chain(std::iter::once('.')) {
            if c.is_alphanumeric() || c == '-' || c == '\'' {
                word.extend(c.to_lowercase());
                continue;
            }
            if !word.is_empty() {
                let w = std::mem::take(&mut word);
                let w = w.trim_matches(|c| c == '-' || c == '\'').to_string();
                if w.is_empty() || is_stop(&w) || w.chars().all(|c| c.is_numeric()) || w.chars().count() < 2 {
                    end(&mut cur, &mut phrases);
                } else {
                    cur.push(w);
                }
            }
            if !c.is_whitespace() {
                end(&mut cur, &mut phrases);
            }
        }
        end(&mut cur, &mut phrases);
        // Long runs are lists rather than phrases.
        phrases.retain(|p| p.len() <= 4);
        // RAKE's word score: degree over frequency.
        let mut freq: HashMap<&str, f64> = HashMap::new();
        let mut degree: HashMap<&str, f64> = HashMap::new();
        for p in &phrases {
            for w in p {
                *freq.entry(w).or_default() += 1.0;
                *degree.entry(w).or_default() += p.len() as f64;
            }
        }
        let mut scored: Vec<(String, f64, usize)> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (i, p) in phrases.iter().enumerate() {
            let s: f64 = p.iter().map(|w| degree[w.as_str()] / freq[w.as_str()]).sum();
            let text = p.join(" ");
            match seen.get(&text) {
                // A phrase said again counts again.
                Some(&j) => scored[j].1 += s * 0.5,
                None => {
                    seen.insert(text.clone(), scored.len());
                    scored.push((text, s, i));
                }
            }
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
        scored.into_iter().take(self.k).map(|(t, _, _)| t).collect()
    }
}

impl Deriver for Keyphrases {
    fn name(&self) -> String {
        "builtin:keyphrases".into()
    }
    fn modality(&self) -> Modality {
        Modality::Text
    }
    fn derive(&self, inputs: &[Input<'_>]) -> Result<Vec<String>> {
        Ok(texts("builtin:keyphrases", inputs)?.into_iter().map(|t| self.of(t).join(&SEP.to_string())).collect())
    }
}

// ---- builtin:minhash ----

/// `builtin:minhash`: locality-sensitive bands of a text's MinHash.
pub struct MinHash;

/// Hashes and bands: 32 hashes in 8 bands of 4.
pub const HASHES: usize = 32;
pub const BANDS: usize = 8;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn mix(mut z: u64) -> u64 {
    // SplitMix64's finaliser.
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

impl MinHash {
    /// The MinHash of a text's word 3-grams (its words, lowercased; the
    /// whole text when it has fewer than three).
    pub fn signature(text: &str) -> Option<[u64; HASHES]> {
        let lower = text.to_lowercase();
        let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            return None;
        }
        let shingles: Vec<u64> = if words.len() < 3 { vec![fnv1a(words.join(" ").as_bytes())] } else { words.windows(3).map(|w| fnv1a(w.join(" ").as_bytes())).collect() };
        let mut sig = [u64::MAX; HASHES];
        for s in shingles {
            for (i, m) in sig.iter_mut().enumerate() {
                let h = mix(s ^ mix(i as u64 + 1));
                if h < *m {
                    *m = h;
                }
            }
        }
        Some(sig)
    }

    /// The band values of a text: `b<band>:<hash>`.
    pub fn bands(text: &str) -> Vec<String> {
        let Some(sig) = Self::signature(text) else { return Vec::new() };
        let rows = HASHES / BANDS;
        (0..BANDS)
            .map(|b| {
                let mut h: u64 = b as u64;
                for v in &sig[b * rows..(b + 1) * rows] {
                    h = mix(h ^ *v);
                }
                format!("b{b}:{h:016x}")
            })
            .collect()
    }

    /// The share of hashes two texts' signatures agree on: an estimate of
    /// the Jaccard similarity of their 3-grams.
    pub fn similarity(a: &str, b: &str) -> f64 {
        match (Self::signature(a), Self::signature(b)) {
            (Some(x), Some(y)) => x.iter().zip(&y).filter(|(p, q)| p == q).count() as f64 / HASHES as f64,
            _ => 0.0,
        }
    }
}

impl Deriver for MinHash {
    fn name(&self) -> String {
        "builtin:minhash".into()
    }
    fn modality(&self) -> Modality {
        Modality::Text
    }
    fn derive(&self, inputs: &[Input<'_>]) -> Result<Vec<String>> {
        Ok(texts("builtin:minhash", inputs)?.into_iter().map(|t| Self::bands(t).join(&SEP.to_string())).collect())
    }
}

// ---- dict:<table>.<field> ----

/// `dict:<table>.<field>`: the values of a keyword field a text names in
/// full. The database gives it the field's values ([`DictEntities::set`])
/// as of each write that derives with it.
pub struct DictEntities {
    pub model: String,
    pub table: String,
    pub field: String,
    pub analyzer: Analyzer,
    values: RwLock<Option<Arc<Gazetteer>>>,
}

impl DictEntities {
    /// The model `dict:<table>.<field>`, reading text with `analyzer`.
    pub fn new(model: &str, analyzer: Analyzer) -> Result<DictEntities> {
        let (table, field) = model
            .strip_prefix(DICT)
            .and_then(|d| d.split_once('.'))
            .filter(|(t, f)| !t.is_empty() && !f.is_empty())
            .ok_or_else(|| err(format!("model {model}: a dictionary is dict:<table>.<field>, a KEYWORD field of a table")))?;
        Ok(DictEntities { model: model.to_string(), table: table.to_string(), field: field.to_string(), analyzer, values: RwLock::new(None) })
    }

    /// Its values from now on.
    pub fn set(&self, g: Arc<Gazetteer>) {
        *self.values.write().unwrap_or_else(|e| e.into_inner()) = Some(g);
    }

    /// The values `text` names in full, in the order it names them.
    pub fn of(&self, text: &str) -> Vec<String> {
        let Some(g) = self.values.read().unwrap_or_else(|e| e.into_inner()).clone() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for s in crate::query::understand::spans(text, &[&g], self.analyzer, &[]) {
            for m in s.matches.into_iter().filter(|m| m.full) {
                let v = m.value.replace(SEP, " ");
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
        out
    }
}

/// A classifier's labels of a text, `classifier:<name>`: the text
/// embedded by the model of the field the classifier was trained on, and
/// classified; the best label for a `KEYWORD` field, and the labels
/// `returns => 'labels'` gives for a `KEYWORD[]` one. A text written
/// before the classifier exists has none.
pub struct ClassifierTags {
    pub model: String,
    pub name: String,
    pub many: bool,
    /// The version it holds, its classifier and the field's model.
    state: RwLock<Option<(i64, Arc<crate::query::classify::Classifier>, Option<Arc<dyn crate::embed::Embedder>>)>>,
}

impl ClassifierTags {
    pub fn new(model: &str, many: bool) -> Result<ClassifierTags> {
        let name = classifier_name(model).ok_or_else(|| err(format!("model {model} is not classifier:<name>")))?;
        Ok(ClassifierTags { model: model.to_string(), name: name.to_ascii_lowercase(), many, state: RwLock::new(None) })
    }

    /// The version of the classifier it holds.
    pub fn version(&self) -> Option<i64> {
        self.state.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.0)
    }

    /// Gives it version `version` of the classifier, and the model of its
    /// field (`None` while that model is not open).
    pub fn set(&self, version: i64, c: Arc<crate::query::classify::Classifier>, e: Option<Arc<dyn crate::embed::Embedder>>) {
        *self.state.write().unwrap_or_else(|e| e.into_inner()) = Some((version, c, e));
    }

    /// Forgets the classifier, dropped.
    pub fn clear(&self) {
        *self.state.write().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl Deriver for ClassifierTags {
    fn name(&self) -> String {
        self.model.clone()
    }
    fn modality(&self) -> Modality {
        Modality::Text
    }
    fn derive(&self, inputs: &[Input<'_>]) -> Result<Vec<String>> {
        let ts = texts(&self.model, inputs)?;
        let state = self.state.read().unwrap_or_else(|e| e.into_inner()).clone();
        let Some((_, c, e)) = state else { return Ok(vec![String::new(); ts.len()]) };
        let e = e.ok_or_else(|| err(format!("{}: the model of {}.{} is not open, so its text is not embedded", self.model, c.table, c.field)))?;
        let vs = e.embed(&ts)?;
        let returns = if self.many { crate::query::classify::Returns::Labels } else { crate::query::classify::Returns::Label };
        vs.into_iter()
            .zip(&ts)
            .map(|(v, t)| {
                if t.trim().is_empty() {
                    return Ok(String::new());
                }
                Ok(match c.value(&crate::value::Value::Vector(v.into()), returns)? {
                    crate::value::Value::Str(s) => s,
                    crate::value::Value::List(ls) => ls.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join(&SEP.to_string()),
                    _ => String::new(),
                })
            })
            .collect()
    }
}

impl Deriver for DictEntities {
    fn name(&self) -> String {
        self.model.clone()
    }
    fn modality(&self) -> Modality {
        Modality::Text
    }
    fn derive(&self, inputs: &[Input<'_>]) -> Result<Vec<String>> {
        Ok(texts(&self.model, inputs)?.into_iter().map(|t| self.of(t).join(&SEP.to_string())).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_by_script_and_words() {
        for (text, code) in [
            ("The quick brown fox jumps over the lazy dog, and it was not amused.", "en"),
            ("Der schnelle braune Fuchs springt über den faulen Hund, und das ist nicht schlimm.", "de"),
            ("Le renard brun rapide saute par-dessus le chien paresseux, et il est content.", "fr"),
            ("El rápido zorro marrón salta sobre el perro perezoso y no es para tanto.", "es"),
            ("La volpe marrone veloce salta sopra il cane pigro, e non è un problema.", "it"),
            ("A raposa marrom rápida pula sobre o cão preguiçoso, e não é um problema.", "pt"),
            ("De snelle bruine vos springt over de luie hond, en dat is niet erg.", "nl"),
            ("Быстрая коричневая лиса прыгает через ленивую собаку.", "ru"),
            ("Швидка бура лисиця перестрибує через ледачого пса і її.", "uk"),
            ("敏捷的棕色狐狸跳过了懒狗。", "zh"),
            ("素早い茶色の狐はのろまな犬を飛び越える。", "ja"),
            ("빠른 갈색 여우가 게으른 개를 뛰어넘는다.", "ko"),
            ("الثعلب البني السريع يقفز فوق الكلب الكسول", "ar"),
            ("Η γρήγορη καφέ αλεπού πηδάει πάνω από το τεμπέλικο σκυλί.", "el"),
            ("तेज़ भूरी लोमड़ी आलसी कुत्ते के ऊपर कूदती है।", "hi"),
            ("สุนัขจิ้งจอกสีน้ำตาลกระโดดข้ามสุนัขขี้เกียจ", "th"),
            ("Hızlı kahverengi tilki tembel köpeğin üzerinden atlar ve bu çok güzel bir şey.", "tr"),
            ("Con cáo nâu nhanh nhẹn nhảy qua con chó lười và nó không có vấn đề gì.", "vi"),
            ("1234 5678", "und"),
        ] {
            assert_eq!(detect(text), code, "{text}");
        }
        assert_eq!(analyzer_name("de"), "german");
        assert_eq!(analyzer_name("zh"), "standard");
    }

    #[test]
    fn keyphrases_are_the_runs_that_say_most() {
        let k = Keyphrases { k: 3 };
        let p = k.of("Compatibility of systems of linear constraints over the set of natural numbers. Criteria of compatibility of a system of linear Diophantine equations, strict inequations, and nonstrict inequations are considered.");
        assert!(p.contains(&"linear diophantine equations".to_string()), "{p:?}");
        assert!(p.len() == 3);
        assert!(k.of("").is_empty());
    }

    #[test]
    fn minhash_bands_meet_for_near_duplicates() {
        let a = "Breaking: the city council voted on Tuesday to expand the bike lane network across the downtown area by 2027.";
        let b = "BREAKING - The city council voted on Tuesday to expand the bike lane network across the downtown area by 2028!";
        let c = "A recipe for sourdough bread with a crisp crust and an open crumb, baked in a Dutch oven.";
        let (ba, bb, bc) = (MinHash::bands(a), MinHash::bands(b), MinHash::bands(c));
        assert_eq!(ba.len(), BANDS);
        assert!(ba.iter().any(|x| bb.contains(x)), "near duplicates share a band");
        assert!(!ba.iter().any(|x| bc.contains(x)));
        assert!(MinHash::similarity(a, b) > 0.6 && MinHash::similarity(a, c) < 0.1);
        // Stable: the values are stored.
        assert_eq!(MinHash::bands("one two three")[0], MinHash::bands("One, two; three.")[0]);
    }

    #[test]
    fn dictionaries_tag_the_values_a_text_names() {
        let d = DictEntities::new("dict:brands.name", Analyzer::English).unwrap();
        assert!(d.of("anything").is_empty());
        d.set(Arc::new(Gazetteer::of("name", &[("Acme Corp", 3), ("Globex", 2), ("Corp", 1)], Analyzer::English)));
        assert_eq!(d.of("Globex sued Acme Corp; acme corp settled."), vec!["Globex", "Acme Corp"]);
        assert!(DictEntities::new("dict:brands", Analyzer::English).is_err());
        assert_eq!(makes_many("dict:brands.name"), Some(true));
        assert_eq!(makes_many("builtin:lang"), Some(false));
        assert_eq!(makes_many("builtin:nope"), None);
    }
}
