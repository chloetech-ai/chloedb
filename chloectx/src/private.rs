//! Private asides: the parts of a prompt the user asked not to keep ("Off
//! the record, please don't save this anywhere: ...", "(Side note: the
//! client is Bramble Bakery; keep that name private)", "Keep that between
//! us"). They are cut out of the prompt before anything is written: the
//! prompt record, the session state, the context every record of the turn
//! carries, the episode card, the picks of rules and decisions from its
//! words and the exchange the distiller reads (WEI-301: such an aside was
//! stored, made a "user preference" every session was given and synced to
//! the team, while Claude told the user it was not saved; JONAH-301). What
//! the reply says of it is cut too ([`scrub`]), and the hook tells the
//! session what was left out, so that Claude can say what was kept.
//!
//! A prompt the user asked not to keep at all ("don't save any of this")
//! keeps nothing of its turn ([`Cut::whole`]).

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;

/// What stands where an aside was.
pub const LEFT_OUT: &str = "[private aside left out]";

/// A prompt without its private asides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cut {
    /// The prompt, each aside replaced by [`LEFT_OUT`]; empty when
    /// [`Cut::whole`].
    pub kept: String,
    /// The asides, as written.
    pub asides: Vec<String>,
    /// The user asked that none of the prompt be kept.
    pub whole: bool,
}

impl Cut {
    /// Whether anything was left out.
    pub fn any(&self) -> bool {
        self.whole || !self.asides.is_empty()
    }
}

/// Words that ask, whatever else the sentence says, that something not be
/// kept or passed on: "off the record", "between you and me", "don't save
/// this anywhere", "don't remember that", "not for memory", "keep this out
/// of your notes", "don't tell the team".
fn strong_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let neg = r"(?:don[’']?t|do not|dont|never|please don[’']?t|please do not)";
        let it = r"(?:this|that|it|these|those|any of (?:this|that|it)|anything (?:of )?(?:this|here)|this (?:part|bit|one|name|detail|note|aside))";
        let place = r"(?:anywhere|down|for later|(?:in|to|into) (?:memory|your memory|the memory|long[- ]term memory|chloectx|notes|your notes|the notes|the record|any (?:notes?|memory|records?)))";
        Regex::new(&format!(
            r"(?i)\boff[- ]the[- ]record\b|\b(?:just |strictly )?between (?:you and me|you and i|us|ourselves)\b|\bnot for (?:the )?(?:memory|record|notes|chloectx)\b|\b{neg}\s+(?:save|store|keep|record|log|put|write|note|jot)\s+{it}\s+{place}|\b{neg}\s+(?:remember|memori[sz]e|note down|make a note of|keep a (?:note|record|copy) of)\s+{it}\b|\b(?:no need to|you don[’']?t need to|don[’']?t bother to|don[’']?t bother) (?:remember|memori[sz]e|save|note|record)\s+{it}\b|\bkeep\s+{it}\s+out of (?:memory|your memory|chloectx|your notes|the notes|the record)\b|\b{neg}\s+(?:share|sync|pass on|tell)\s+(?:{it}\s+)?(?:with |to )?(?:anyone|anybody|the team|others|other people|my (?:team|teammates|colleagues|manager|boss))\b|\bforget (?:i said|that i said|what i (?:just )?said)\b"
        ))
        .unwrap()
    })
}

/// Words that ask it too, but that are also said of code ("keep that
/// method private", "that field is private"): an aside only in a sentence
/// that names no code.
fn soft_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bkeep (?:this|that|it|these|those)(?: [a-z]+){0,2} (?:private|confidential|secret|quiet|to yourself|between us)\b|\b(?:this|that)(?:[’']s| is| was)(?: [a-z]+)? (?:private|confidential|personal|not public)\b|\b(?:private|personal|confidential) (?:aside|matter|detail|details)\b|\b(?:please )?(?:don[’']?t|do not) save (?:this|that)\s*(?:[:.!?)]|$)").unwrap()
    })
}

/// Words that name code, which make the soft words above about the code.
fn code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)`|::|\w\(\)|\b(?:methods?|functions?|fn|func|fields?|class(?:es)?|structs?|modules?|members?|variables?|vars?|attributes?|propert(?:y|ies)|constructors?|traits?|interfaces?|endpoints?|helpers?|impl|crates?|packages?|repos?|repositor(?:y|ies)|buckets?|branch(?:es)?|columns?|tables?|getters?|setters?|namespaces?|types?|enums?|consts?|constants?|static|mod|routes?|handlers?|components?|props?|api|apis|sdk|objects?|instances?|visibility|scope)\b").unwrap()
    })
}

/// An aside about the whole prompt: "don't save any of this", "this whole
/// conversation is off the record".
fn whole_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:any|none|all) of (?:this|it|what i (?:say|write))\b|\b(?:this|the) (?:whole |entire )?(?:conversation|chat|session|thread|message|prompt|request)\b|\beverything (?:here|i say|i write|in this (?:message|prompt))\b").unwrap()
    })
}

/// A marker that points back at what was just said: "keep that private",
/// "don't remember that", "forget I said that".
fn back_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(?:that|it|those)\b|\bforget (?:i said|that i said|what i (?:just )?said)").unwrap())
}

/// A sentence that goes back to the work after an aside: "Anyway, ...",
/// "Back to the deploy", "For the deploy work: ...".
fn return_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\W*(?:anyway|anyways|anyhow|back to|ok(?:ay)?\b|so\b|now\b|moving on|that said|on to|in any case|regardless|for the|regarding|re:|as for|enough|onto)").unwrap())
}

/// A sentence that asks the agent for something: "Add a .env.example",
/// "Can you fix the login?", "Please run the tests", "Let's ...".
fn request_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^\W*(?:(?:then|next|now|also|and|please|ok|okay|first|finally)\b\W*)*(?:(?:can|could|would|will) you\b|let[’']?s\b|i (?:need|want|would like) you to\b|help me\b|(?:add|fix|write|create|make|run|use|show|tell|implement|build|update|change|delete|remove|rename|install|test|try|start|stop|commit|push|open|check|list|explain|set|move|refactor|review|look|find|search|read|give|put|generate|draft|deploy|summari[sz]e)\b)").unwrap()
    })
}

/// Whether `sentence` asks for something not to be kept.
fn marks(sentence: &str) -> bool {
    let s = sentence.replace(LEFT_OUT, " ");
    strong_re().is_match(&s) || (soft_re().is_match(&s) && !code_re().is_match(&s))
}

/// Whether `text` holds words that ask for something not to be kept: a
/// record written before asides were cut out ([`retire_once`], sync).
pub fn holds_aside(text: &str) -> bool {
    spans(text).iter().any(|&(s, e, _)| marks(&text[s..e])) || paren_spans(text).iter().any(|&(s, e)| marks(&text[s..e]))
}

/// The sentences of `text`: byte ranges ending after their `.`, `!` or `?`
/// (a `.` that ends a word, not the one in "3.8"), or at a line break; and
/// whether a line break ends it.
fn spans(text: &str) -> Vec<(usize, usize, bool)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let next = it.peek().map(|x| x.1);
        let (end, line) = match c {
            '\n' => (Some(i), true),
            '!' | '?' => (Some(i + 1), false),
            '.' if next.is_none_or(char::is_whitespace) => (Some(i + 1), next == Some('\n')),
            _ => (None, false),
        };
        if let Some(e) = end {
            if !text[start..e].trim().is_empty() {
                out.push((start, e, line));
            }
            start = i + c.len_utf8();
        }
    }
    if !text[start..].trim().is_empty() {
        out.push((start, text.len(), true));
    }
    out
}

/// Parenthesised parts of `text` ("(Side note: ...)"), at most 1,000 bytes
/// and on one paragraph; one left open runs to the end of its paragraph.
fn paren_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'(' {
            let rest = &text[i..];
            let para = rest.find("\n\n").unwrap_or(rest.len());
            let end = match rest[..para].find(')') {
                Some(j) => i + j + 1,
                None => i + para,
            };
            if end - i <= 1000 {
                out.push((i, end));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The words a sentence holds besides its marker and filler: how much of
/// it is the aside itself ("Keep that private." holds none).
fn content_words(sentence: &str) -> usize {
    let mut s = strong_re().replace_all(sentence, " ").into_owned();
    s = soft_re().replace_all(&s, " ").into_owned();
    const FILLER: &[&str] = &[
        "please", "also", "and", "but", "so", "side", "note", "unrelated", "btw", "by", "the", "way", "ok", "okay", "just", "fyi", "aside", "quick", "oh", "this", "that", "it", "is", "a", "an", "of", "to", "for", "me", "you", "i", "anywhere", "here", "thanks", "thank", "private", "anyway",
    ];
    s.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !FILLER.contains(&w.to_lowercase().as_str()))
        .count()
}

/// `prompt` without its private asides.
pub fn cut(prompt: &str) -> Cut {
    let mut asides = Vec::new();
    // Parentheses first: an aside in them ends where they do.
    let mut text = String::with_capacity(prompt.len());
    let mut at = 0;
    for (s, e) in paren_spans(prompt) {
        if marks(&prompt[s..e]) {
            if whole_re().is_match(&prompt[s..e]) {
                return Cut { kept: String::new(), asides: vec![prompt.trim().to_string()], whole: true };
            }
            text.push_str(&prompt[at..s]);
            text.push_str(LEFT_OUT);
            asides.push(prompt[s..e].to_string());
            at = e;
        }
    }
    text.push_str(&prompt[at..]);
    // Then sentences: the one that asks, with what it points to.
    let sp = spans(&text);
    let mut drop = vec![false; sp.len()];
    let mut i = 0;
    while i < sp.len() {
        let (s, e, _) = sp[i];
        let sentence = &text[s..e];
        if drop[i] || !marks(sentence) {
            i += 1;
            continue;
        }
        if whole_re().is_match(sentence) {
            return Cut { kept: String::new(), asides: vec![prompt.trim().to_string()], whole: true };
        }
        drop[i] = true;
        let plain = sentence.replace(LEFT_OUT, " ");
        let m = strong_re().find(&plain).or_else(|| soft_re().find(&plain));
        let opens = m.is_some_and(|m| content_words(&plain[..m.start()]) == 0);
        let bare = content_words(&plain) < 3;
        let back = m.is_some_and(|m| back_re().is_match(&plain[..m.end()]));
        if back && (bare || opens) {
            // "I'm interviewing at Acme. Keep that between us."
            if i > 0 && !drop[i - 1] && !request_re().is_match(&text[sp[i - 1].0..sp[i - 1].1]) {
                drop[i - 1] = true;
            }
        } else if opens || sentence.trim_end().ends_with(':') {
            // "Off the record: I'm interviewing next week. My manager does
            // not know yet. Anyway, ...": on to the end of its paragraph,
            // a sentence that goes back to the work or asks for something.
            let mut j = i + 1;
            let mut line = sp[i].2;
            while j < sp.len() && !(bare && j > i + 1 && line) {
                let next = &text[sp[j].0..sp[j].1];
                let gap = &text[sp[j - 1].1..sp[j].0];
                if gap.contains("\n\n") || return_re().is_match(next) || request_re().is_match(next) || (line && !bare) {
                    break;
                }
                drop[j] = true;
                line = sp[j].2;
                j += 1;
            }
        }
        i += 1;
    }
    if !drop.iter().any(|d| *d) && asides.is_empty() {
        return Cut { kept: prompt.to_string(), asides, whole: false };
    }
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    let mut k = 0;
    while k < sp.len() {
        if !drop[k] {
            k += 1;
            continue;
        }
        // A run of dropped sentences is one aside.
        let start = sp[k].0;
        let mut end = sp[k].1;
        while k + 1 < sp.len() && drop[k + 1] {
            k += 1;
            end = sp[k].1;
        }
        let lead = text[start..end].len() - text[start..end].trim_start().len();
        kept.push_str(&text[at..start + lead]);
        kept.push_str(LEFT_OUT);
        asides.push(text[start..end].trim().to_string());
        at = end;
        k += 1;
    }
    kept.push_str(&text[at..]);
    // Only the words of the user's work are kept: an aside that was all of
    // it is the whole prompt.
    let rest = kept.replace(LEFT_OUT, " ");
    if content_words(&rest) == 0 {
        return Cut { kept: String::new(), asides: vec![prompt.trim().to_string()], whole: true };
    }
    Cut { kept, asides, whole: false }
}

/// The analysed terms of `t`.
fn terms(t: &str) -> BTreeSet<String> {
    chloedb::text::analyze(t).into_iter().collect()
}

/// What of the asides a reply could repeat: the terms they hold that the
/// kept prompt does not, and of them the ones that name something on their
/// own (a name, a capitalised word, a long word): "bramble", "bakery",
/// "interview", "thursday".
pub struct Echo {
    weak: BTreeSet<String>,
    strong: BTreeSet<String>,
}

impl Echo {
    pub fn new(asides: &[String], kept: &str) -> Echo {
        let kept = terms(kept);
        let mut markers = String::new();
        let mut weak = BTreeSet::new();
        let mut strong = BTreeSet::new();
        for a in asides {
            for m in strong_re().find_iter(a).chain(soft_re().find_iter(a)) {
                markers.push_str(m.as_str());
                markers.push(' ');
            }
        }
        let markers = terms(&markers);
        for a in asides {
            for t in terms(a) {
                if !kept.contains(&t) && !markers.contains(&t) && t.chars().count() >= 3 {
                    if t.chars().count() >= 7 {
                        strong.insert(t.clone());
                    }
                    weak.insert(t);
                }
            }
            // A word written with a capital inside a sentence names something.
            for s in spans(a) {
                for (n, w) in a[s.0..s.1].split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).enumerate() {
                    if n > 0 && w.chars().next().is_some_and(char::is_uppercase) && w.chars().skip(1).any(char::is_lowercase) {
                        for t in terms(w) {
                            if weak.contains(&t) {
                                strong.insert(t);
                            }
                        }
                    }
                }
            }
        }
        Echo { weak, strong }
    }

    /// Whether `sentence` says again what an aside said.
    fn repeats(&self, sentence: &str) -> bool {
        let t = terms(sentence);
        t.iter().any(|x| self.strong.contains(x)) || t.iter().filter(|x| self.weak.contains(*x)).count() >= 2
    }
}

/// `text` (a reply, the commentary between tool calls) without the
/// sentences that say again what the turn's asides said, each replaced by
/// [`LEFT_OUT`] once.
pub fn scrub(text: &str, echo: &Echo) -> String {
    if echo.weak.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut last_cut = false;
    for (s, e, _) in spans(text) {
        if !echo.repeats(&text[s..e]) {
            last_cut = false;
            continue;
        }
        let lead = text[s..e].len() - text[s..e].trim_start().len();
        out.push_str(&text[at..s + lead]);
        if !last_cut {
            out.push_str(LEFT_OUT);
        }
        last_cut = true;
        at = e;
    }
    out.push_str(&text[at..]);
    out
}

/// What the hook tells the session when a prompt held a private aside: what
/// ChloeCtx left out and what it keeps, so that Claude can tell the user
/// the truth about it.
pub fn notice(c: &Cut) -> String {
    if c.whole {
        "ChloeCtx: the user asked that this prompt not be kept. ChloeCtx keeps nothing of this turn: not the prompt, not your reply, not the tool calls, and no note or rule is written from it or synced. If the user asks, say so.".to_string()
    } else {
        "ChloeCtx: part of this prompt is a private aside the user asked not to keep. ChloeCtx left it out: it is not in the stored prompt, in any note or rule, or in what sync sends. The rest of the prompt and this turn's work are kept as usual, and so is your reply: do not repeat the aside in it. If the user asks what was kept, say so; memory_forget deletes anything else they want gone.".to_string()
    }
}

/// Records of a store written before asides were cut out that are an
/// aside's own words: a prompt's sentence kept as a rule or a decision
/// (`cue`, `stated`). Retired, so that no briefing, recall or overview
/// shows them; their text stays in the store until it is forgotten
/// (`chloectx forget`).
pub fn retire_asides(store: &crate::store::Store) -> crate::Result<usize> {
    let rs = store.query(&format!("SELECT {} FROM memory WHERE kind IN ('preference', 'decision') AND tag IN ('cue', 'stated') LIMIT 1000000", crate::record::COLUMNS))?;
    let mut n = 0;
    for r in rs.rows.iter().filter_map(|r| crate::store::row_record(r)) {
        if holds_aside(&r.text) && store.retag(&r.id, crate::record::RETIRED)? {
            n += 1;
        }
    }
    Ok(n)
}

/// [`retire_asides`] once per store (a file in its directory says it ran),
/// for a daemon to call when it starts.
pub fn retire_once(dir: &std::path::Path, store: &crate::store::Store) -> Option<usize> {
    let mark = dir.join("asides-retired");
    if mark.exists() {
        return None;
    }
    let n = retire_asides(store).ok()?;
    let _ = std::fs::write(&mark, format!("{n}\n"));
    Some(n)
}

/// A record as sync sends it: one written before asides were cut out has
/// them cut from its text and context, and one that is nothing but an
/// aside (a prompt's chunk, a rule picked from it) is not sent.
pub fn for_push(r: &crate::record::Record) -> Option<crate::record::Record> {
    use crate::record::Kind;
    let raw = matches!(r.kind, Kind::Prompt | Kind::Episode) || (matches!(r.kind, Kind::Preference | Kind::Decision) && matches!(r.tag.as_str(), "cue" | "stated"));
    let mut out = r.clone();
    if raw && holds_aside(&r.text) {
        let c = cut(&r.text);
        if c.whole || content_words(&c.kept.replace(LEFT_OUT, " ")) < 2 || r.kind != Kind::Prompt && r.kind != Kind::Episode {
            return None;
        }
        out.text = c.kept;
    }
    if holds_aside(&r.ctx) {
        // "Turn 3 request: <the prompt's opening>": the prompt is cut.
        let line = |l: &str| match l.split_once(" request: ").filter(|(head, _)| head.starts_with("Turn ")) {
            Some((head, p)) => format!("{head} request: {}", cut(p).kept),
            None => cut(l).kept,
        };
        out.ctx = r.ctx.lines().map(|l| if holds_aside(l) { line(l) } else { l.to_string() }).collect::<Vec<_>>().join("\n");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEI: &str = "Off the record, please don't save this anywhere: I'm interviewing at another company next week, so I may be slow on reviews on Thursday. Anyway, for the deploy work: staging uses DATABASE_URL=postgres://ledger_app:[REDACTED:password]@db.staging.acme.internal:5432/ledger. Add a .env.example with placeholders for these and a one-paragraph note in the README on where each value comes from.";
    const JONAH: &str = "Add a --version flag that prints the version from pyproject. (Side note, unrelated: the client I'm eventually building this for is Bramble Bakery; keep that name private, it's not public yet.) Commit when the tests pass.";

    #[test]
    fn an_aside_is_cut_and_the_work_kept() {
        let c = cut(WEI);
        assert!(!c.whole, "{c:?}");
        assert_eq!(c.asides, ["Off the record, please don't save this anywhere: I'm interviewing at another company next week, so I may be slow on reviews on Thursday."], "{c:?}");
        assert!(c.kept.starts_with(LEFT_OUT) && c.kept.contains("Anyway, for the deploy work") && c.kept.contains(".env.example"), "{}", c.kept);
        assert!(!c.kept.contains("interview"), "{}", c.kept);
        let c = cut(JONAH);
        assert_eq!(c.asides.len(), 1, "{c:?}");
        assert!(c.asides[0].starts_with("(Side note") && c.asides[0].ends_with("yet.)"), "{c:?}");
        assert_eq!(c.kept, format!("Add a --version flag that prints the version from pyproject. {LEFT_OUT} Commit when the tests pass."));
    }

    #[test]
    fn what_a_marker_points_to_goes_with_it() {
        // Back: the sentence before, unless it asks for the work.
        let c = cut("Fix the flaky login test. I'm interviewing at Acme next week. Keep that between us.");
        assert_eq!(c.kept, format!("Fix the flaky login test. {LEFT_OUT}"), "{c:?}");
        let c = cut("Fix the flaky login test. Please keep that to yourself.");
        assert_eq!(c.kept, format!("Fix the flaky login test. {LEFT_OUT}"), "{c:?}");
        // Forward: on to a sentence that goes back to the work.
        let c = cut("Off the record: my manager is leaving. Nobody knows yet. Anyway, rename the cart module to basket.");
        assert_eq!(c.kept, format!("{LEFT_OUT} Anyway, rename the cart module to basket."), "{c:?}");
        let c = cut("Between you and me:\nthe Q3 launch slips to November.\n\nNow add the migration for the refunds table.");
        assert!(c.kept.starts_with(LEFT_OUT) && c.kept.ends_with("Now add the migration for the refunds table.") && !c.kept.contains("Q3"), "{c:?}");
        // The whole prompt.
        for p in ["Off the record, don't save any of this: I think the CTO is wrong about Kafka.", "Don't remember this.", "This whole conversation is off the record. How do I ask for a raise?"] {
            let c = cut(p);
            assert!(c.whole && c.kept.is_empty(), "{p}: {c:?}");
        }
    }

    #[test]
    fn code_talk_is_not_an_aside() {
        for p in [
            "Keep that method private and add a test for it.",
            "Add a helper and keep it private.",
            "The field is private, so the test can't read it.",
            "If validation fails, don't save it.",
            "Don't store it in the database, compute it on the fly.",
            "Don't log this, it is noisy.",
            "Always keep the repo private.",
            "Make the API key an env var; that is private to the deploy job.",
            "Implement the parser",
        ] {
            let c = cut(p);
            assert!(!c.any() && c.kept == p, "{p}: {c:?}");
        }
        assert!(holds_aside("User preference: Off the record, please don't save this anywhere: I'm interviewing at another company next week"));
        assert!(!holds_aside("User preference: always run clippy before committing"));
    }

    #[test]
    fn a_reply_loses_what_repeats_the_aside() {
        let c = cut(JONAH);
        let e = Echo::new(&c.asides, &c.kept);
        let reply = "Added --version; it reads pyproject.toml. I haven't saved the client name anywhere. The client is Bramble Bakery, noted. All 12 tests pass.";
        let s = scrub(reply, &e);
        assert_eq!(s, format!("Added --version; it reads pyproject.toml. I haven't saved the client name anywhere. {LEFT_OUT} All 12 tests pass."), "{s}");
        let c = cut(WEI);
        let e = Echo::new(&c.asides, &c.kept);
        let s = scrub("Good luck with the interview! I added .env.example with placeholders for DATABASE_URL and the Stripe key.", &e);
        assert_eq!(s, format!("{LEFT_OUT} I added .env.example with placeholders for DATABASE_URL and the Stripe key."), "{s}");
        // Nothing to cut: the text as it was.
        let none = Echo::new(&[], "x");
        assert_eq!(scrub("The client is Bramble Bakery.", &none), "The client is Bramble Bakery.");
    }

    #[test]
    fn old_records_go_out_cut_or_not_at_all() {
        use crate::record::{Kind, Record};
        let rec = |kind: Kind, tag: &str, text: &str, ctx: &str| Record { id: "a:1".into(), session: "a".into(), kind, tag: tag.into(), turn: 1, seq: 1, ts: 0, importance: 0.5, files: String::new(), text: text.into(), ctx: ctx.into(), links: Vec::new() };
        let ctx = format!("Turn 1 request: {}", &WEI[..200]);
        assert!(for_push(&rec(Kind::Preference, "cue", &format!("User preference: {}", &WEI[..150]), &ctx)).is_none());
        let p = for_push(&rec(Kind::Prompt, "-", WEI, &ctx)).unwrap();
        assert!(!p.text.contains("interview") && !p.ctx.contains("interview") && p.ctx.starts_with("Turn 1 request: "), "{p:?}");
        let d = for_push(&rec(Kind::Decision, "distilled", "Staging uses its own database.", &ctx)).unwrap();
        assert_eq!(d.text, "Staging uses its own database.");
        assert!(!d.ctx.contains("interview"), "{}", d.ctx);
        let plain = rec(Kind::Prompt, "-", "Add a test", "Turn 2 request: Add a test");
        assert_eq!(for_push(&plain).unwrap().text, "Add a test");
    }
}
