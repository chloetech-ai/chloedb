//! Forgetting a text, wherever a store keeps it (JONAH-1, JONAH-2,
//! LENA-5 of the persona study). The records whose own text holds it are
//! deleted. A record that holds it only in its context (the turn's request,
//! which every record of a turn carries, so a rule distilled from a prompt
//! that also said something private) is kept, with the lines of its
//! context that held it cut. The store's other files lose it too: the
//! session state, the event logs and the receipts. For a while after a
//! forget, and until the end of a turn that called `memory_forget`, nothing
//! that holds it is written again ([`Live`]): what was being written when it
//! was forgotten (a distillation, a search's answer, the forget turn's own
//! reply) does not bring it back.
//!
//! What is forgotten is kept only in the daemon's memory, never in a file.

use std::collections::HashSet;
use std::sync::Mutex;

use serde_json::Value as Json;

/// What replaces a string, or a line of a record's context, that held a
/// forgotten text.
pub const REMOVED: &str = "(removed: it held text that was forgotten)";

/// How long after a forget nothing that holds its text is written.
pub const LIVE_SECS: i64 = 300;

/// The shortest text that can be forgotten by its words.
pub const MIN_CHARS: usize = 3;

/// A text to look for, as it is matched: trimmed and lowercase; `None`
/// when it is shorter than [`MIN_CHARS`].
pub fn needle(text: &str) -> Option<String> {
    let t = text.trim().to_lowercase();
    (t.chars().count() >= MIN_CHARS).then_some(t)
}

/// The byte offset in `hay` of the first `needle` (lowercase), whatever
/// the case of `hay`.
pub fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    if hay.is_ascii() {
        return hay.to_ascii_lowercase().find(needle);
    }
    // Lowercasing can change a character's length: each lowercase
    // character keeps the offset of the one it came from.
    let mut low = String::with_capacity(hay.len());
    let mut from = Vec::with_capacity(hay.len());
    for (i, c) in hay.char_indices() {
        for l in c.to_lowercase() {
            from.push((low.len(), i));
            low.push(l);
        }
    }
    let at = low.find(needle)?;
    from.iter().rev().find(|(lo, _)| *lo <= at).map(|(_, i)| *i)
}

/// Whether `hay` holds any of `needles` (lowercase), whatever its case.
pub fn holds(hay: &str, needles: &[String]) -> bool {
    needles.iter().any(|n| find_ci(hay, n).is_some())
}

/// `ctx` without what it holds of `needles`: each line that holds one is
/// replaced by [`REMOVED`], keeping a turn's label (`Turn 3 request: `),
/// so the record still says which turn it came from. `None` when no line
/// holds one.
pub fn scrub_ctx(ctx: &str, needles: &[String]) -> Option<String> {
    if !holds(ctx, needles) {
        return None;
    }
    let lines: Vec<String> = ctx
        .lines()
        .map(|l| {
            if !holds(l, needles) {
                return l.to_string();
            }
            match l.split_once(" request: ") {
                Some((label, _)) if label.starts_with("Turn ") && !holds(label, needles) => format!("{label} request: {REMOVED}"),
                _ => REMOVED.to_string(),
            }
        })
        .collect();
    Some(lines.join("\n"))
}

/// Replaces each string in `v` that holds one of `needles` with
/// [`REMOVED`] (object keys are names and stay). Whether any changed.
pub fn scrub_json(v: &mut Json, needles: &[String]) -> bool {
    match v {
        Json::String(s) => {
            if holds(s, needles) {
                *s = REMOVED.to_string();
                true
            } else {
                false
            }
        }
        Json::Array(a) => a.iter_mut().fold(false, |c, x| scrub_json(x, needles) | c),
        Json::Object(o) => o.values_mut().fold(false, |c, x| scrub_json(x, needles) | c),
        _ => false,
    }
}

/// `text` (a log: one JSON object a line, or plain lines) without what it
/// holds of `needles`: in a JSON line each string that holds one is
/// replaced, another line that holds one is replaced whole. The text, and
/// how many lines changed.
pub fn scrub_lines(text: &str, needles: &[String]) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut changed = 0;
    for line in text.split_inclusive('\n') {
        let (body, nl) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
        if !holds(body, needles) {
            out += line;
            continue;
        }
        changed += 1;
        let new = match serde_json::from_str::<Json>(body) {
            Ok(mut v) if v.is_object() => {
                scrub_json(&mut v, needles);
                let s = v.to_string();
                // A key that held it: the line goes whole.
                if holds(&s, needles) {
                    REMOVED.to_string()
                } else {
                    s
                }
            }
            _ => REMOVED.to_string(),
        };
        out += &new;
        out += nl;
    }
    (out, changed)
}

/// The texts forgotten lately, which nothing written may hold, kept in
/// memory only: each for [`LIVE_SECS`] after its forget, and one that a
/// turn's `memory_forget` named until that turn ends. Also the turns that
/// called `memory_forget`, which are not distilled and whose reply is not
/// stored.
#[derive(Default)]
pub struct Live {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    needles: Vec<Needle>,
    turns: HashSet<(String, i64)>,
}

struct Needle {
    text: String,
    until: i64,
    turn: Option<(String, i64)>,
}

impl Live {
    /// `needles` were forgotten at `now`.
    pub fn add(&self, needles: &[String], now: i64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for n in needles {
            match g.needles.iter_mut().find(|x| &x.text == n) {
                Some(x) => x.until = x.until.max(now + LIVE_SECS),
                None => g.needles.push(Needle { text: n.clone(), until: now + LIVE_SECS, turn: None }),
            }
        }
    }

    /// Turn `turn` of session `sid` called `memory_forget`, with the text
    /// it forgot when it named one: that text stays out of what is written
    /// until the turn ends, however long it runs.
    pub fn forgot_in(&self, sid: &str, turn: i64, text: Option<&str>, now: i64) {
        let key = (sid.to_string(), turn);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.turns.insert(key.clone());
        if let Some(n) = text.and_then(needle) {
            match g.needles.iter_mut().find(|x| x.text == n) {
                Some(x) => x.turn = Some(key),
                None => g.needles.push(Needle { text: n, until: now + LIVE_SECS, turn: Some(key) }),
            }
        }
    }

    /// Whether turn `turn` of `sid` called `memory_forget`.
    pub fn is_forget_turn(&self, sid: &str, turn: i64) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).turns.contains(&(sid.to_string(), turn))
    }

    /// The turn ended: `None` when it called no `memory_forget`, else the
    /// texts it forgot, which from now on live their [`LIVE_SECS`] only.
    pub fn end_turn(&self, sid: &str, turn: i64) -> Option<Vec<String>> {
        let key = (sid.to_string(), turn);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !g.turns.remove(&key) {
            return None;
        }
        let mut texts = Vec::new();
        for n in g.needles.iter_mut().filter(|n| n.turn.as_ref() == Some(&key)) {
            n.turn = None;
            texts.push(n.text.clone());
        }
        Some(texts)
    }

    /// The texts nothing written may hold at `now`.
    pub fn current(&self, now: i64) -> Vec<String> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.needles.is_empty() {
            return Vec::new();
        }
        g.needles.retain(|n| n.until > now || n.turn.is_some());
        g.needles.iter().map(|n| n.text.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn needles_match_whatever_the_case() {
        assert_eq!(needle("  Quokka-Bundle "), Some("quokka-bundle".into()));
        assert_eq!(needle("ab"), None);
        let n = vec![needle("bundle").unwrap()];
        assert!(holds("sold as part of a BUNDLE later", &n));
        assert!(holds("Straße BUNDLE", &n));
        assert!(!holds("bundl e", &n));
        assert_eq!(find_ci("İstanbul bundle", "bundle"), Some("İstanbul ".len()));
    }

    #[test]
    fn a_context_keeps_what_does_not_hold_it() {
        let n = vec!["bundle".to_string()];
        let ctx = "Turn 1 request: a todo app I'll sell in a bundle later. Rules: pytest.\nWhen: writing tests\nBy: user";
        assert_eq!(scrub_ctx(ctx, &n).unwrap(), format!("Turn 1 request: {REMOVED}\nWhen: writing tests\nBy: user"));
        assert_eq!(scrub_ctx("When: selling the bundle\nBy: user", &n).unwrap(), format!("{REMOVED}\nBy: user"));
        assert_eq!(scrub_ctx("Turn 2 request: tests", &n), None);
    }

    #[test]
    fn json_and_log_lines_lose_the_strings_that_hold_it() {
        let n = vec!["bundle".to_string()];
        let mut v = json!({"episode": {"prompt": "the Bundle plan", "files": ["a.py"]}, "last_prompts": ["x", "sell the bundle"], "turn": 3});
        assert!(scrub_json(&mut v, &n));
        assert_eq!(v, json!({"episode": {"prompt": REMOVED, "files": ["a.py"]}, "last_prompts": ["x", REMOVED], "turn": 3}));
        assert!(!scrub_json(&mut v, &n));

        let log = "{\"event\":\"search\",\"query\":\"text:selling tdo in a bundle\"}\nplain bundle line\n{\"event\":\"x\"}\n";
        let (out, changed) = scrub_lines(log, &n);
        assert_eq!(changed, 2);
        assert!(!holds(&out, &n), "{out}");
        assert!(out.starts_with("{\"event\":\"search\""), "{out}");
        assert!(out.ends_with("{\"event\":\"x\"}\n"), "{out}");
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn a_forgotten_text_lives_for_a_while_and_through_its_turn() {
        let l = Live::default();
        assert!(l.current(100).is_empty());
        l.add(&["alpha".into()], 100);
        assert_eq!(l.current(100 + LIVE_SECS - 1), vec!["alpha".to_string()]);
        assert!(l.current(100 + LIVE_SECS).is_empty());

        // A turn's memory_forget: until the turn ends, however long it runs.
        l.forgot_in("s", 4, Some(" Beta "), 1000);
        assert!(l.is_forget_turn("s", 4) && !l.is_forget_turn("s", 5));
        assert_eq!(l.current(1000 + 10 * LIVE_SECS), vec!["beta".to_string()]);
        assert_eq!(l.end_turn("s", 4), Some(vec!["beta".to_string()]));
        assert!(l.current(1000 + 10 * LIVE_SECS).is_empty());
        assert_eq!(l.end_turn("s", 4), None);
        // By id only: the turn is a forget turn, with no text of its own.
        l.forgot_in("s", 6, None, 2000);
        assert_eq!(l.end_turn("s", 6), Some(vec![]));
    }
}
