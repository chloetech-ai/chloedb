//! Query understanding: what a search box's words ask for. Operators as
//! HN people type them (`author:pg points>100 after:2020`), and the same
//! said in a sentence ("Show HN posts by pg since 2020 with over 100
//! points"). What is understood leaves the text and becomes a filter, a
//! sort or a kind, and is said back to the person as chips they can undo.

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Parsed {
    /// The words left to search by (for BM25 and for meaning).
    pub text: String,
    /// Quoted phrases, searched as phrases.
    pub phrases: Vec<String>,
    /// Words written with a minus: rows that hold them are left out.
    pub exclude: Vec<String>,
    /// Text in backticks: found exactly as written, inside words and
    /// punctuation too, by the substring index (`ILIKE`).
    pub exact: Vec<String>,
    /// Words that look like code (`std::move`, `node.js`, `x86_64`,
    /// `foo()`): searched by words, and also exactly as written.
    pub code: Vec<String>,
    pub author: Option<String>,
    /// A person named in a sentence ("by tptacek"): a filter only if such a
    /// person exists, which the search checks.
    pub author_maybe: Option<String>,
    /// The words that named it, put back in the text when nobody has that name.
    pub author_maybe_words: String,
    pub kinds: Vec<String>,
    pub site: Option<String>,
    /// Unix seconds, inclusive start and exclusive end.
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub points_min: Option<i64>,
    pub points_max: Option<i64>,
    pub comments_min: Option<i64>,
    pub story: Option<i64>,
    /// A topic the stories must carry (`topic:rust`).
    pub topic: Option<String>,
    /// `story` or `comment` when the words ask for only one.
    pub only: Option<String>,
    /// `date`, `points` or `comments` (and from a question `old`,
    /// `least_points`, `least_comments`).
    pub sort: Option<String>,
    /// What a question in a sentence asks for ([`crate::nl`]).
    pub ask: Option<crate::nl::Ask>,
    /// "top 10": how many a question asks for.
    pub limit: Option<usize>,
    /// What was understood, in words, each with the part of the search it came from.
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Note {
    pub what: String,
    pub from: String,
}

impl Parsed {
    fn note(&mut self, what: impl Into<String>, from: impl Into<String>) {
        self.notes.push(Note { what: what.into(), from: from.into() });
    }
    /// The person named in a sentence was found: a filter now.
    pub fn confirm_author(&mut self, found: bool) {
        let Some(a) = self.author_maybe.take() else { return };
        if found {
            self.note(format!("by {a}"), self.author_maybe_words.clone());
            self.author = Some(a);
        } else {
            self.text = if self.text.is_empty() { self.author_maybe_words.clone() } else { format!("{} {}", self.text, self.author_maybe_words) };
        }
    }
    pub fn has_filters(&self) -> bool {
        self.author.is_some() || !self.kinds.is_empty() || self.site.is_some() || self.after.is_some() || self.before.is_some() || self.points_min.is_some() || self.points_max.is_some() || self.comments_min.is_some() || self.story.is_some() || self.topic.is_some()
    }
}

const KINDS: &[&str] = &["story", "ask", "show", "launch", "tell", "job", "poll"];

fn ymd(y: i32, m: u32, d: u32) -> i64 {
    NaiveDate::from_ymd_opt(y, m.clamp(1, 12), d.clamp(1, 28)).and_then(|d| d.and_hms_opt(0, 0, 0)).map(|t| Utc.from_utc_datetime(&t).timestamp()).unwrap_or(0)
}

/// A date as typed (`2020`, `2020-06`, `2020-06-15`): its first second, and
/// the first second after it.
pub fn date_span(s: &str) -> Option<(i64, i64)> {
    let p: Vec<&str> = s.split(['-', '/']).collect();
    let y: i32 = p.first()?.parse().ok()?;
    if !(2006..=2100).contains(&y) {
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

fn is_user(s: &str) -> bool {
    (2..=15).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn number(s: &str) -> Option<i64> {
    let t = s.trim_end_matches('+').replace(',', "");
    if let Some(k) = t.strip_suffix(['k', 'K']) {
        return k.parse::<f64>().ok().map(|x| (x * 1000.0) as i64);
    }
    t.parse().ok()
}

/// Reads a search box. `now` is the current Unix time, for "last year".
pub fn parse(raw: &str, now: i64) -> Parsed {
    // A question's frame first ("which post has the most comments"); the
    // rest is read as any search is.
    if let Some(f) = crate::nl::read(raw) {
        let mut p = parse_words(&f.rest, now);
        if let Some(o) = &f.only {
            p.only = Some(o.clone());
        }
        for k in &f.kinds {
            if !p.kinds.contains(k) {
                p.kinds.push(k.clone());
            }
            p.only.get_or_insert_with(|| "story".into());
        }
        if let crate::nl::Ask::Top { by, .. } = &f.ask {
            p.sort = Some(by.clone());
        }
        let what = crate::nl::describe(&f.ask, p.only.as_deref());
        p.notes.insert(0, Note { what, from: f.said.clone() });
        p.ask = Some(f.ask);
        p.limit = f.limit;
        return p;
    }
    parse_words(raw, now)
}

fn parse_words(raw: &str, now: i64) -> Parsed {
    let mut p = Parsed::default();
    // Quoted phrases first.
    let mut rest = String::new();
    let mut in_q = false;
    let mut cur = String::new();
    let mut in_tick = false;
    for c in raw.chars() {
        if c == '`' && !in_q {
            if in_tick {
                let ex = cur.trim().to_string();
                if !ex.is_empty() {
                    p.note(format!("exactly “{ex}”"), format!("`{ex}`"));
                    p.exact.push(ex);
                }
                cur.clear();
            }
            in_tick = !in_tick;
        } else if in_tick {
            cur.push(c);
        } else if c == '"' {
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
    let toks: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
    let mut keep: Vec<String> = Vec::new();
    // Operators.
    for t in toks {
        let low = t.to_ascii_lowercase();
        if let Some((k, v)) = low.split_once(':').filter(|(k, v)| !k.is_empty() && !v.is_empty() && !k.contains('/')) {
            let orig_v = &t[k.len() + 1..];
            match k {
                "author" | "by" | "user" if is_user(orig_v) => {
                    p.author = Some(orig_v.to_string());
                    p.note(format!("by {orig_v}"), t.clone());
                    continue;
                }
                "site" | "domain" | "from" | "url" if v.contains('.') => {
                    let s = v.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").trim_end_matches('/');
                    p.site = Some(s.to_string());
                    p.note(format!("on {s}"), t.clone());
                    continue;
                }
                "topic" | "tag" => {
                    let v = v.replace(['_', '-'], " ");
                    p.note(format!("about {v}"), t.clone());
                    p.topic = Some(v);
                    p.only.get_or_insert_with(|| "story".into());
                    continue;
                }
                "story" | "thread" | "item" if v.chars().all(|c| c.is_ascii_digit()) => {
                    p.story = v.parse().ok();
                    p.only = Some("comment".into());
                    p.note(format!("in thread {v}"), t.clone());
                    continue;
                }
                "type" | "is" | "kind" => {
                    let k2 = match v {
                        "comment" | "comments" => {
                            p.only = Some("comment".into());
                            p.note("comments", t.clone());
                            continue;
                        }
                        "stories" => "story",
                        "jobs" => "job",
                        "polls" => "poll",
                        other => other,
                    };
                    if KINDS.contains(&k2) {
                        if !p.kinds.contains(&k2.to_string()) {
                            p.kinds.push(k2.to_string());
                        }
                        p.only.get_or_insert_with(|| "story".into());
                        p.note(kind_name(k2), t.clone());
                        continue;
                    }
                }
                "after" | "since" | "from" | "year" | "date" | "before" | "until" | "in" => {
                    if let Some((a, b)) = date_span(v) {
                        match k {
                            "after" | "since" | "from" => {
                                p.after = Some(a);
                                p.note(format!("since {v}"), t.clone());
                            }
                            "before" | "until" => {
                                p.before = Some(a);
                                p.note(format!("before {v}"), t.clone());
                            }
                            _ => {
                                p.after = Some(a);
                                p.before = Some(b);
                                p.note(format!("in {v}"), t.clone());
                            }
                        }
                        continue;
                    }
                }
                "sort" | "order" => {
                    let s = match v {
                        "date" | "new" | "newest" | "recent" | "time" => Some("date"),
                        "points" | "top" | "score" | "popular" | "popularity" => Some("points"),
                        "comments" | "discussed" => Some("comments"),
                        _ => None,
                    };
                    if let Some(s) = s {
                        p.sort = Some(s.into());
                        p.note(sort_name(s), t.clone());
                        continue;
                    }
                }
                "points" | "comments" if v.contains("..") => {
                    let (a, b) = v.split_once("..").unwrap_or(("", ""));
                    if k == "points" {
                        p.points_min = number(a);
                        p.points_max = number(b);
                    } else {
                        p.comments_min = number(a);
                    }
                    p.note(format!("{k} {a}–{b}"), t.clone());
                    continue;
                }
                _ => {}
            }
        }
        // points>100, comments>=50, points<10
        let mut done = false;
        for field in ["points", "comments", "score"] {
            if let Some(r) = low.strip_prefix(field) {
                let (op, num) = if let Some(n) = r.strip_prefix(">=") { (">=", n) } else if let Some(n) = r.strip_prefix('>') { (">", n) } else if let Some(n) = r.strip_prefix("<=") { ("<=", n) } else if let Some(n) = r.strip_prefix('<') { ("<", n) } else { ("", "") };
                if let (false, Some(n)) = (op.is_empty(), number(num)) {
                    let n = match op { ">" => n + 1, "<" => n - 1, _ => n };
                    if field == "comments" {
                        p.comments_min = Some(n);
                    } else if op.starts_with('>') {
                        p.points_min = Some(n);
                    } else {
                        p.points_max = Some(n);
                    }
                    p.note(format!("{field} {op} {num}"), t.clone());
                    done = true;
                }
            }
        }
        if done {
            continue;
        }
        if let Some(w) = t.strip_prefix('-').filter(|w| !w.is_empty() && w.chars().next().is_some_and(char::is_alphanumeric)) {
            p.exclude.push(w.to_string());
            p.note(format!("without “{w}”"), t.clone());
            continue;
        }
        keep.push(t);
    }
    sentence(&mut p, keep, now);
    p.code = p.text.split_whitespace().filter(|w| code_like(w)).map(|w| w.trim_matches(|c: char| matches!(c, ',' | '?' | '!' | ';')).to_string()).collect();
    p
}

/// A word that reads as code, not prose: punctuation inside it that a
/// word analyzer splits on or drops (`std::move`, `node.js`, `x86_64`,
/// `foo()`, `C++20`, `a->b`, `$PATH`, `@decorator`, `--flag`).
pub fn code_like(w: &str) -> bool {
    let w = w.trim_matches(|c: char| matches!(c, ',' | '?' | '!' | ';' | '"' | '\''));
    if w.chars().count() < 2 || w.starts_with("http") {
        return false;
    }
    let inner = |c: char| matches!(c, ':' | '>' | '(' | ')' | '[' | ']' | '_' | '/' | '$' | '@' | '=' | '{' | '}' | '<' | '+' | '#' | '.' | '-');
    let chars: Vec<char> = w.chars().collect();
    let letters = chars.iter().filter(|c| c.is_alphanumeric()).count();
    if letters == 0 {
        return false;
    }
    // A plain hyphenated word ("open-source") or a sentence's period is prose.
    if chars.iter().all(|c| c.is_alphanumeric() || *c == '-') {
        return w.starts_with("--");
    }
    if chars.iter().all(|c| c.is_alphanumeric() || *c == '.') && w.ends_with('.') {
        return false;
    }
    chars.iter().enumerate().any(|(i, &c)| inner(c) && (i > 0 || matches!(c, '$' | '@' | '.' | '#')))
}

pub fn kind_name(k: &str) -> String {
    match k {
        "ask" => "Ask HN".into(),
        "show" => "Show HN".into(),
        "launch" => "Launch HN".into(),
        "tell" => "Tell HN".into(),
        "job" => "jobs".into(),
        "poll" => "polls".into(),
        _ => "stories".into(),
    }
}

fn sort_name(s: &str) -> String {
    match s {
        "date" => "newest first".into(),
        "points" => "most points first".into(),
        _ => "most discussed first".into(),
    }
}

/// The same filters said in words: what is left of the search after the operators.
fn sentence(p: &mut Parsed, toks: Vec<String>, now: i64) {
    let low: Vec<String> = toks.iter().map(|t| t.to_lowercase().trim_matches(|c: char| matches!(c, ',' | '?' | '!' | ';')).to_string()).collect();
    let n = low.len();
    let mut used = vec![false; n];
    let mut i = 0;
    let this_year = Utc.timestamp_opt(now, 0).single().map(|t| t.year()).unwrap_or(2026);
    let at = |i: usize| -> &str { low.get(i).map(String::as_str).unwrap_or("") };
    while i < n {
        let w = at(i);
        let w1 = at(i + 1);
        let w2 = at(i + 2);
        // Ask HN / Show HN / Launch HN / Tell HN
        if w1 == "hn" && matches!(w, "ask" | "show" | "launch" | "tell") {
            if !p.kinds.contains(&w.to_string()) {
                p.kinds.push(w.to_string());
            }
            p.only.get_or_insert_with(|| "story".into());
            p.note(kind_name(w), format!("{} {}", toks[i], toks[i + 1]));
            used[i] = true;
            used[i + 1] = true;
            i += 2;
            continue;
        }
        // "by pg", "from tptacek": a person, if one by that name exists.
        if (w == "by" || (w == "from" && !w1.contains('.'))) && is_user(&toks.get(i + 1).cloned().unwrap_or_default()) && p.author.is_none() && !matches!(w1, "the" | "a" | "an" | "me" | "my" | "people" | "users" | "hn") && !w1.chars().all(|c| c.is_ascii_digit()) {
            p.author_maybe = Some(toks[i + 1].clone());
            p.author_maybe_words = format!("{} {}", toks[i], toks[i + 1]);
            used[i] = true;
            used[i + 1] = true;
            i += 2;
            continue;
        }
        // "from github.com", "on nytimes.com"
        if matches!(w, "from" | "on" | "at" | "site") && w1.contains('.') && w1.len() > 3 && !w1.ends_with('.') {
            let s = w1.trim_start_matches("www.").trim_end_matches('/').to_string();
            p.note(format!("on {s}"), format!("{} {}", toks[i], toks[i + 1]));
            p.site = Some(s);
            used[i] = true;
            used[i + 1] = true;
            i += 2;
            continue;
        }
        // Years and spans: "in 2019", "since 2018", "before 2015", "between 2010 and 2012", "2010s"
        if matches!(w, "in" | "from" | "during" | "since" | "after" | "before" | "until" | "of") {
            if let Some((a, b)) = date_span(w1) {
                match w {
                    "since" | "after" => p.after = Some(if w == "after" { b } else { a }),
                    "before" | "until" => p.before = Some(a),
                    _ => {
                        p.after = Some(a);
                        p.before = Some(b);
                    }
                }
                p.note(format!("{w} {w1}"), format!("{} {}", toks[i], toks[i + 1]));
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
        }
        if w == "between" && at(i + 2) == "and" {
            if let (Some((a, _)), Some((_, b))) = (date_span(w1), date_span(at(i + 3))) {
                p.after = Some(a);
                p.before = Some(b);
                p.note(format!("{w1} to {}", at(i + 3)), toks[i..i + 4].join(" "));
                for u in used.iter_mut().skip(i).take(4) {
                    *u = true;
                }
                i += 4;
                continue;
            }
        }
        if let Some(dec) = w.strip_suffix('s').and_then(|d| d.parse::<i32>().ok()).filter(|d| (2000..=2090).contains(d) && d % 10 == 0) {
            p.after = Some(ymd(dec.max(2006), 1, 1));
            p.before = Some(ymd(dec + 10, 1, 1));
            p.note(format!("the {w}"), toks[i].clone());
            used[i] = true;
            i += 1;
            continue;
        }
        // "last year", "this year", "past month", "last week", "today"
        if matches!(w, "last" | "past" | "this") && matches!(w1, "year" | "month" | "week" | "day" | "24h") {
            let span = match w1 {
                "year" => 365 * 86_400,
                "month" => 30 * 86_400,
                "week" => 7 * 86_400,
                _ => 86_400,
            };
            if w == "this" && w1 == "year" {
                p.after = Some(ymd(this_year, 1, 1));
            } else {
                p.after = Some(now - span);
            }
            p.note(format!("{w} {w1}"), format!("{} {}", toks[i], toks[i + 1]));
            used[i] = true;
            used[i + 1] = true;
            i += 2;
            continue;
        }
        if matches!(w, "today" | "yesterday") && n > 1 {
            p.after = Some(now - if w == "today" { 86_400 } else { 2 * 86_400 });
            p.note(w.to_string(), toks[i].clone());
            used[i] = true;
            i += 1;
            continue;
        }
        // "over 100 points", "more than 500 points", "100+ points", "at least 50 comments"
        let (num_at, skip) = match (w, w1) {
            ("over" | "above", _) => (i + 1, 1),
            ("more", "than") | ("at", "least") => (i + 2, 2),
            _ if number(w).is_some() && w.ends_with('+') => (i, 0),
            _ => (usize::MAX, 0),
        };
        if num_at != usize::MAX {
            if let Some(num) = number(at(num_at)) {
                let unit = at(num_at + 1);
                if matches!(unit, "points" | "upvotes" | "votes" | "point" | "comments" | "replies") {
                    if unit.starts_with("comment") || unit == "replies" {
                        p.comments_min = Some(num);
                    } else {
                        p.points_min = Some(num);
                    }
                    p.note(format!("{}+ {unit}", num), toks[i..=num_at + 1].join(" "));
                    for u in used.iter_mut().skip(i).take(skip + 2) {
                        *u = true;
                    }
                    i = num_at + 2;
                    continue;
                }
            }
        }
        // "comments about rust", "what people said about": comments only.
        if matches!(w, "comments" | "comment" | "replies") && matches!(w1, "about" | "on" | "mentioning" | "regarding" | "that" | "saying" | "where" | "by") {
            p.only = Some("comment".into());
            p.note("comments", toks[i].clone());
            used[i] = true;
            if w1 != "by" {
                used[i + 1] = true;
            }
            i += 2;
            continue;
        }
        // Order words: "top", "best", "most upvoted", "latest", "newest", "most discussed".
        let order = match (w, w1) {
            ("most", "upvoted" | "popular" | "points" | "voted") => Some(("points", 2)),
            ("most", "discussed" | "commented" | "comments") => Some(("comments", 2)),
            ("top" | "best" | "popular" | "highest", _) if i == 0 && n > 1 => Some(("points", 1)),
            ("latest" | "newest" | "recent", _) if n > 1 => Some(("date", 1)),
            _ => None,
        };
        if let Some((s, k)) = order {
            p.sort = Some(s.into());
            p.note(sort_name(s), toks[i..i + k].join(" "));
            for u in used.iter_mut().skip(i).take(k) {
                *u = true;
            }
            i += k;
            continue;
        }
        let _ = w2;
        i += 1;
    }
    // Filler that only framed the parts understood.
    let any = used.iter().any(|&u| u);
    let mut words: Vec<String> = Vec::new();
    for (j, t) in toks.iter().enumerate() {
        if used[j] {
            continue;
        }
        let l = &low[j];
        if any && matches!(l.as_str(), "posts" | "post" | "stories" | "story" | "articles" | "about" | "submissions" | "threads" | "discussions" | "with" | "and" | "of" | "all") && (j == 0 || used.get(j.wrapping_sub(1)).copied().unwrap_or(false) || used.get(j + 1).copied().unwrap_or(false)) {
            continue;
        }
        words.push(t.clone());
    }
    p.text = words.join(" ");
}

/// The query's words as a ChloeDB text query, for `MATCH` and `bm25`: the
/// words, the phrases quoted, the excluded words with a minus. Characters
/// the grammar reads as operators are taken as spaces.
pub fn match_text(p: &Parsed) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in p.text.split_whitespace() {
        let clean: String = w.chars().map(|c| if c.is_alphanumeric() || matches!(c, '\'' | '.' | '+' | '#') { c } else { ' ' }).collect();
        for part in clean.split_whitespace() {
            let part = part.trim_matches(|c: char| c == '.' || c == '\'');
            let up = part.to_ascii_uppercase();
            if part.is_empty() || matches!(up.as_str(), "AND" | "OR" | "NOT") {
                continue;
            }
            out.push(part.to_string());
        }
    }
    for ph in &p.phrases {
        let clean: String = ph.chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
        let c = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        if !c.is_empty() {
            out.push(format!("\"{c}\""));
        }
    }
    out.join(" ")
}

/// The words alone (no phrases, no operators), for BM25's ranking and for meaning.
pub fn plain_words(p: &Parsed) -> String {
    let mut s = p.text.clone();
    for ph in &p.phrases {
        s.push(' ');
        s.push_str(ph);
    }
    s.split(|c: char| !(c.is_alphanumeric() || matches!(c, '\'' | '+' | '#' | '.' | '-'))).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_794_960_000; // 2026-11-16

    #[test]
    fn code_and_exact_text() {
        for w in ["std::move", "node.js", "x86_64", "foo()", "C++20", "a->b", "$PATH", "@decorator", "--flag", ".NET", "c#"] {
            assert!(code_like(w), "{w}");
        }
        for w in ["open-source", "rust", "end.", "go", "it's", "https://x.com"] {
            assert!(!code_like(w), "{w}");
        }
        let p = parse("why `left-pad` broke npm", NOW);
        assert_eq!(p.exact, vec!["left-pad"]);
        assert_eq!(p.text, "why broke npm");
        let p = parse("std::move vs copy", NOW);
        assert_eq!(p.code, vec!["std::move"]);
    }

    #[test]
    fn operators() {
        let p = parse("rust author:pg points>100 after:2020 type:show -crypto", NOW);
        assert_eq!(p.text, "rust");
        assert_eq!(p.author.as_deref(), Some("pg"));
        assert_eq!(p.points_min, Some(101));
        assert_eq!(p.after, date_span("2020").map(|x| x.0));
        assert_eq!(p.kinds, vec!["show"]);
        assert_eq!(p.exclude, vec!["crypto"]);
        let p = parse("\"memory safety\" site:github.com sort:date", NOW);
        assert_eq!(p.phrases, vec!["memory safety"]);
        assert_eq!(p.site.as_deref(), Some("github.com"));
        assert_eq!(p.sort.as_deref(), Some("date"));
        assert_eq!(match_text(&p), "\"memory safety\"");
    }

    #[test]
    fn sentences() {
        let p = parse("Show HN posts by pg since 2020 with over 100 points", NOW);
        assert_eq!(p.kinds, vec!["show"]);
        assert_eq!(p.author_maybe.as_deref(), Some("pg"));
        assert_eq!(p.after, date_span("2020").map(|x| x.0));
        assert_eq!(p.points_min, Some(100));
        assert_eq!(p.text, "", "{p:?}");
        let p = parse("most upvoted comments about kubernetes in 2019", NOW);
        assert_eq!(p.sort.as_deref(), Some("points"));
        assert_eq!(p.only.as_deref(), Some("comment"));
        assert_eq!(p.text, "kubernetes");
        assert_eq!((p.after, p.before), (date_span("2019").map(|x| x.0), date_span("2019").map(|x| x.1)));
        let p = parse("rust vs go", NOW);
        assert_eq!(p.text, "rust vs go");
        assert!(!p.has_filters());
        let p = parse("databases from the 2010s", NOW);
        assert_eq!(p.text, "databases from the", "only the decade is a filter");
        assert!(p.after.is_some());
        let p = parse("ask hn burnout last year", NOW);
        assert_eq!(p.kinds, vec!["ask"]);
        assert_eq!(p.text, "burnout");
        assert_eq!(p.after, Some(NOW - 365 * 86_400));
        // A word that only looks like a person stays a word until the person is found.
        let p = parse("written by hand", NOW);
        assert_eq!(p.author_maybe.as_deref(), Some("hand"));
    }
}
