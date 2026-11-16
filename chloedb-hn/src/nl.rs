//! Questions in plain sentences, read by a grammar, without a model:
//! "which post has received the most comments", "who posts the most about
//! rust", "how many Show HN posts in 2024", "when was bitcoin talked about
//! most", "first story about chatgpt". A question's *frame* (what kind of
//! answer it wants, in what order, of stories or of comments) is found in
//! its words and taken out; what is left goes to [`crate::query::parse`],
//! which reads the topic, the dates, the person, the site and the
//! thresholds as it does for any search. Each answer is then one or two
//! ordinary SQL statements, shown with the answer.
//!
//! [`complete`] is the other side: as a question is typed, the grammar's
//! own templates that fit what is there so far, finished, so the box
//! teaches what it understands.

use serde::Serialize;

/// What a question asks for.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ask {
    /// The rows in an order, the first the answer when `one`: "which post
    /// has the most comments" (`by` is a sort: `comments`, `points`,
    /// `date`, `old`, `least_points`, `least_comments`).
    Top { by: String, one: bool },
    /// How many rows match: "how many stories mention rust".
    Count,
    /// People ranked: `stories`, `comments` or `points` they posted on
    /// the topic, `karma`, or who `knows` it best.
    People { by: String },
    /// Sites ranked by how many stories link to them.
    Sites,
    /// The years the topic was talked about most.
    When,
    /// What HN thinks: the cited answer, on its own page.
    Opinion,
}

/// A question's frame and what is left of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub ask: Ask,
    /// `story` or `comment` when the question names one.
    pub only: Option<String>,
    /// Ask HN, Show HN, ... named in the frame.
    pub kinds: Vec<String>,
    /// "top 10": how many it asks for.
    pub limit: Option<usize>,
    /// The words the frame did not take, as typed.
    pub rest: String,
    /// The words the frame took, as typed, for the chip that says so.
    pub said: String,
}

const WH: &[&str] = &["which", "what", "whats", "what's"];
const BE: &[&str] = &["is", "was", "are", "were", "s", "'s"];
const HAVE: &[&str] = &[
    "has", "have", "had", "got", "gets", "get", "received", "receives", "receive", "recieved", "drew", "draws", "attracted", "attracts", "generated", "generates", "earned", "earns", "scored",
    "with", "having", "gathered", "collected", "racked", "accumulated", "garnered", "sparked", "started",
];
const MORE: &[&str] = &["most", "highest", "biggest", "largest", "greatest", "max", "maximum", "top", "mostest"];
const LESS: &[&str] = &["least", "fewest", "lowest", "smallest", "min", "minimum"];
const STORY_WORDS: &[&str] = &[
    "post", "posts", "story", "stories", "submission", "submissions", "article", "articles", "link", "links", "thread", "threads", "item", "items", "discussion", "discussions",
    "headline", "headlines", "entry", "entries",
];
const COMMENT_WORDS: &[&str] = &["comment", "comments", "reply", "replies", "response", "responses"];
const SITE_WORDS: &[&str] = &["site", "sites", "domain", "domains", "website", "websites", "source", "sources", "blog", "blogs", "publication", "publications", "outlet", "outlets", "url", "urls"];
const PEOPLE_WORDS: &[&str] = &[
    "user", "users", "people", "person", "member", "members", "poster", "posters", "commenter", "commenters", "submitter", "submitters", "contributor", "contributors", "account", "accounts",
    "author", "authors", "hackers", "folks",
];
/// Said around a question, not about its topic.
const FILLER: &[&str] = &[
    "the", "a", "an", "is", "was", "were", "are", "be", "been", "has", "have", "had", "ever", "on", "about", "of", "that", "which", "did", "do", "does", "there", "all", "time", "posted",
    "submitted", "mention", "mentions", "mentioning", "mentioned", "regarding", "related", "to", "for", "with", "in", "by", "me", "show", "find", "list", "give", "tell", "please", "hn", "talk",
    "talked", "talking", "discussed", "discuss", "written", "made", "so", "far",
];

#[derive(Clone)]
struct Tok {
    orig: String,
    low: String,
}

fn toks(raw: &str) -> Vec<Tok> {
    raw.split_whitespace()
        .map(|t| Tok { orig: t.to_string(), low: t.to_lowercase().trim_matches(|c: char| matches!(c, '?' | '!' | ',' | ';' | '.' | ':')).replace('’', "'") })
        .filter(|t| !t.low.is_empty())
        .collect()
}

/// A measure word: the sort it asks for.
fn measure(w: &str) -> Option<&'static str> {
    match w {
        "comments" | "comment" | "replies" | "reply" | "responses" | "response" | "discussion" | "discussions" | "engagement" | "debate" | "conversation" => Some("comments"),
        "points" | "point" | "upvotes" | "upvote" | "votes" | "vote" | "score" | "likes" | "popularity" | "attention" | "traction" | "karma" => Some("points"),
        _ => None,
    }
}

/// A superlative adjective said with "most": "most commented".
fn most_adj(w: &str) -> Option<&'static str> {
    match w {
        "commented" | "discussed" | "replied" | "debated" | "controversial" | "talked-about" | "argued" => Some("comments"),
        "upvoted" | "voted" | "popular" | "liked" | "viral" | "famous" | "successful" | "read" | "loved" | "starred" => Some("points"),
        "recent" => Some("date"),
        _ => None,
    }
}

/// An item word: what it names (`story`, `comment`) and its kind.
fn item(t: &[Tok], i: usize) -> Option<(&'static str, Option<&'static str>, usize, bool)> {
    let w = t.get(i)?.low.as_str();
    let next = t.get(i + 1).map(|x| x.low.as_str()).unwrap_or("");
    if matches!(w, "ask" | "show" | "launch" | "tell") && matches!(next, "hn" | "hn's" | "hns") {
        let k = match w {
            "ask" => "ask",
            "show" => "show",
            "launch" => "launch",
            _ => "tell",
        };
        // "show hn posts": the noun after counts as said.
        let more = t.get(i + 2).is_some_and(|x| STORY_WORDS.contains(&x.low.as_str()));
        return Some(("story", Some(k), if more { 3 } else { 2 }, !(next == "hns" || more && t[i + 2].low.ends_with('s'))));
    }
    if matches!(w, "job" | "jobs" | "poll" | "polls") {
        return Some(("story", Some(if w.starts_with("job") { "job" } else { "poll" }), 1, !w.ends_with('s')));
    }
    if STORY_WORDS.contains(&w) {
        return Some(("story", None, 1, !w.ends_with('s') && w != "news"));
    }
    if COMMENT_WORDS.contains(&w) {
        return Some(("comment", None, 1, !(w.ends_with('s') || w == "replies")));
    }
    None
}

struct Reader {
    t: Vec<Tok>,
    used: Vec<bool>,
}

impl Reader {
    fn low(&self, i: usize) -> &str {
        self.t.get(i).map(|x| x.low.as_str()).unwrap_or("")
    }
    fn take(&mut self, from: usize, n: usize) {
        for u in self.used.iter_mut().skip(from).take(n) {
            *u = true;
        }
    }
    fn is(&self, i: usize, set: &[&str]) -> bool {
        set.contains(&self.low(i))
    }
    /// Takes the words before `i` that only lead into the part at `i`:
    /// "has received the", "with the", "that got".
    fn take_lead(&mut self, mut i: usize) {
        while i > 0 {
            let w = self.low(i - 1);
            if self.used[i - 1] || !(matches!(w, "the" | "a" | "that" | "which" | "who" | "ever" | "number" | "amount" | "of") || HAVE.contains(&w) || BE.contains(&w)) {
                break;
            }
            self.used[i - 1] = true;
            i -= 1;
        }
    }
    /// Takes the item word nearest before `i` (or just after), its kind
    /// and number; `(only, kind, one)`.
    fn take_item(&mut self, before: usize, after_too: bool) -> (Option<String>, Option<String>, Option<bool>) {
        let before = before.min(self.t.len());
        let mut j = before;
        while j > 0 {
            j -= 1;
            if self.used[j] {
                continue;
            }
            if let Some((only, kind, n, one)) = item(&self.t, j) {
                self.take(j, n);
                return (Some(only.into()), kind.map(String::from), Some(one));
            }
            // Topic words may sit between: "which post about rust has the most points".
            if before - j > 6 {
                break;
            }
        }
        if after_too {
            let mut j = before;
            while j < self.t.len() && j < before + 3 {
                if !self.used[j] {
                    if let Some((only, kind, n, one)) = item(&self.t, j) {
                        self.take(j, n);
                        return (Some(only.into()), kind.map(String::from), Some(one));
                    }
                }
                j += 1;
            }
        }
        (None, None, None)
    }
    fn rest(&self) -> String {
        let words: Vec<&str> = self.t.iter().zip(&self.used).filter(|(_, u)| !**u).map(|(t, _)| t.orig.trim_end_matches(['?', '!', ',', ';'])).filter(|w| !w.is_empty()).collect();
        trim_filler(&words).join(" ")
    }
    fn said(&self) -> String {
        self.t.iter().zip(&self.used).filter(|(_, u)| **u).map(|(t, _)| t.orig.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// The words without the filler at either end ("about rust" is "rust"),
/// and without "on HN", "of all time", "ever" anywhere.
fn trim_filler<'a>(words: &[&'a str]) -> Vec<&'a str> {
    let low: Vec<String> = words.iter().map(|w| w.to_lowercase().trim_matches(|c: char| matches!(c, '?' | '!' | ',' | ';' | '.')).to_string()).collect();
    let mut keep = vec![true; words.len()];
    for i in 0..words.len() {
        let w = low[i].as_str();
        let next = low.get(i + 1).map(String::as_str).unwrap_or("");
        let next2 = low.get(i + 2).map(String::as_str).unwrap_or("");
        if matches!(w, "on" | "in" | "at") && (next == "hn" || (next == "hacker" && next2 == "news")) {
            keep[i] = false;
            keep[i + 1] = false;
            if next == "hacker" {
                keep[i + 2] = false;
            }
        }
        if w == "of" && next == "all" && next2 == "time" {
            keep[i] = false;
            keep[i + 1] = false;
            keep[i + 2] = false;
        }
        if matches!(w, "ever") || (w == "hacker" && next == "news") || (w == "news" && i > 0 && low[i - 1] == "hacker") {
            keep[i] = false;
        }
        if w.is_empty() {
            keep[i] = false;
        }
    }
    let mut out: Vec<(&str, &str)> = words.iter().zip(&low).zip(&keep).filter(|(_, k)| **k).map(|((w, l), _)| (*w, l.as_str())).collect();
    // Filler at the ends; a date or a person after "in"/"by" stays (the parser reads it).
    while let Some((_, l)) = out.first() {
        let next = out.get(1).map(|x| x.1).unwrap_or("");
        if FILLER.contains(l) && !(matches!(*l, "in" | "by" | "since" | "on" | "from") && !next.is_empty() && !FILLER.contains(&next)) {
            out.remove(0);
        } else {
            break;
        }
    }
    while let Some((_, l)) = out.last() {
        if FILLER.contains(l) {
            out.pop();
        } else {
            break;
        }
    }
    out.into_iter().map(|x| x.0).collect()
}

fn number_word(w: &str) -> Option<usize> {
    match w {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "five" => Some(5),
        "ten" => Some(10),
        "twenty" => Some(20),
        _ => w.parse().ok().filter(|n| (1..=100).contains(n)),
    }
}

/// Reads a question's frame, if it has one. Searches that are not
/// questions ("rust async", "postgres vs mysql after:2020") have none.
pub fn read(raw: &str) -> Option<Frame> {
    let t = toks(raw);
    if t.is_empty() || raw.contains(':') && t.len() == 1 {
        return None;
    }
    let mut r = Reader { used: vec![false; t.len()], t };
    let n = r.t.len();
    let first = r.low(0).to_string();
    let mut f = Frame { ask: Ask::Count, only: None, kinds: Vec::new(), limit: None, rest: String::new(), said: String::new() };
    let set_item = |f: &mut Frame, (only, kind, _): (Option<String>, Option<String>, Option<bool>)| {
        if let Some(o) = only {
            f.only = Some(o);
        }
        if let Some(k) = kind {
            f.kinds.push(k);
        }
    };

    // "how many stories mention rust", "number of comments about go"
    if (first == "how" && r.low(1) == "many") || (matches!(first.as_str(), "number" | "count") && r.low(1) == "of") || (first == "count" && item(&r.t, 1).is_some()) {
        let k = if first == "count" && r.low(1) != "of" { 1 } else { 2 };
        r.take(0, k);
        let it = r.take_item(k + 2, true);
        set_item(&mut f, it);
        // "... are there", "... were posted", "... mention"
        for i in 0..n {
            if !r.used[i] && (r.is(i, &["are", "were", "is", "was", "have", "has", "did", "do"]) && matches!(r.low(i + 1), "there" | "posted" | "submitted" | "written" | "made" | "been" | "")) {
                r.take(i, if r.low(i + 1).is_empty() { 1 } else { 2 });
            }
        }
        f.ask = Ask::Count;
        return Some(finish(r, f));
    }

    // "when was rust most discussed", "when did HN talk about bitcoin the most"
    if first == "when" || (matches!(first.as_str(), "which" | "what" | "in") && matches!(r.low(1), "year" | "years") ) || (first == "in" && matches!(r.low(1), "which" | "what") && r.low(2) == "year") {
        let k = if first == "when" { 1 } else if first == "in" { 3 } else { 2 };
        r.take(0, k);
        for i in k..n {
            let w = r.low(i).to_string();
            if matches!(w.as_str(), "was" | "were" | "did" | "is" | "has" | "had" | "hn" | "people" | "it" | "most" | "the" | "popular" | "discussed" | "talked" | "talk" | "about" | "hot" | "hottest" | "trending" | "mentioned" | "peak" | "peaked" | "biggest" | "big" | "start" | "started" | "talking" | "become" | "became" | "get" | "got" | "posted" | "most?" | "do" | "does")
                && (i == k || r.used[i - 1] || i + 1 == n || matches!(w.as_str(), "most" | "popular" | "discussed" | "peak" | "peaked" | "trending" | "hot" | "hottest" | "mentioned"))
            {
                r.used[i] = true;
            }
        }
        // "about" before the topic is filler; the topic stays.
        f.ask = Ask::When;
        return Some(finish(r, f));
    }

    // "what does HN think about rust", "hn's opinion on nix"
    if (WH.contains(&first.as_str()) && matches!(r.low(1), "does" | "do" | "did") && matches!(r.low(2), "hn" | "people" | "everyone" | "hackers" | "commenters" | "hacker"))
        || (matches!(first.as_str(), "hn's" | "hns") && matches!(r.low(1), "opinion" | "opinions" | "take" | "view" | "views"))
        || (matches!(first.as_str(), "opinions" | "thoughts") && matches!(r.low(1), "on" | "about" | "of"))
    {
        for i in 0..n.min(7) {
            if r.is(i, &["what", "does", "do", "did", "hn", "people", "everyone", "hackers", "commenters", "hacker", "news", "think", "say", "feel", "thought", "said", "hn's", "hns", "opinion", "opinions", "take", "view", "views", "thoughts", "on", "about", "of"]) && (i == 0 || r.used[i - 1]) {
                r.used[i] = true;
            }
        }
        f.ask = Ask::Opinion;
        return Some(finish(r, f));
    }

    // People: "who posted the most stories about rust", "who has the most karma",
    // "who knows the most about postgres", "top commenters on rust", "most active users".
    let people_noun = (0..n.min(4)).find(|&i| r.is(i, PEOPLE_WORDS));
    if first == "who" || first == "whom" || (people_noun.is_some() && (WH.contains(&first.as_str()) || MORE.contains(&first.as_str()) || matches!(first.as_str(), "top" | "best" | "most" | "biggest" | "leading"))) {
        let mut by: Option<&str> = None;
        let lim = n.min(8);
        for i in 0..lim {
            let w = r.low(i).to_string();
            match w.as_str() {
                "knows" | "know" | "expert" | "experts" | "authority" | "authorities" => by = by.or(Some("knows")),
                "karma" => by = Some("karma"),
                "commented" | "comments" | "comment" | "commenting" | "commenters" | "commenter" | "replies" | "replied" | "talks" | "talked" | "talk" | "writes" | "wrote" | "says" | "said" | "argues" | "active" => by = by.or(Some("comments")),
                "posted" | "posts" | "post" | "submitted" | "submits" | "submit" | "shared" | "shares" | "posters" | "poster" | "submitters" | "submitter" | "stories" | "links" => by = by.or(Some("stories")),
                "points" | "upvotes" | "votes" | "upvoted" | "popular" => by = by.or(Some("points")),
                _ => {}
            }
        }
        let by = by.unwrap_or("stories");
        // Take the frame: up to the topic ("about", "on", "in" before a word that is not a date).
        for i in 0..lim {
            let w = r.low(i).to_string();
            if matches!(w.as_str(), "about" | "on" | "regarding" | "for") && i > 0 {
                r.used[i] = true;
                break;
            }
            let frame_word = matches!(w.as_str(), "who" | "whom" | "is" | "are" | "was" | "were" | "the" | "has" | "have" | "had" | "most" | "best" | "top" | "biggest" | "leading" | "highest" | "users" | "user" | "people" | "person" | "knows" | "know" | "expert" | "experts" | "karma" | "active" | "of" | "an" | "a" | "on" | "hn" | "here" | "posted" | "posts" | "post" | "submitted" | "submits" | "submit" | "shared" | "shares" | "commented" | "comments" | "comment" | "commenting" | "talks" | "talked" | "talk" | "writes" | "wrote" | "says" | "said" | "points" | "upvotes" | "votes" | "stories" | "links" | "amount" | "number" | "ever" | "much" | "what" | "which" | "gets" | "got" | "get" | "received")
                || PEOPLE_WORDS.contains(&w.as_str())
                || item(&r.t, i).is_some();
            if !frame_word {
                break;
            }
            r.used[i] = true;
        }
        f.ask = Ask::People { by: by.into() };
        if by == "comments" {
            f.only = Some("comment".into());
        }
        return Some(finish(r, f));
    }

    // Sites: "which sites are posted the most", "top domains about rust", "most posted sites".
    if let Some(si) = (0..n.min(5)).find(|&i| r.is(i, SITE_WORDS)) {
        let lead = (0..si).any(|i| r.is(i, WH) || r.is(i, MORE) || r.is(i, &["popular", "common", "posted", "submitted", "shared", "linked", "best"]));
        if lead {
            r.take(0, si + 1);
            for i in si + 1..n.min(si + 8) {
                if r.is(i, &["are", "is", "get", "gets", "got", "have", "has", "had", "were", "was", "posted", "submitted", "shared", "linked", "the", "most", "often", "frequently", "post", "posts", "appear", "appears", "show", "up", "on", "hn", "to", "in", "do", "does"]) && !(r.is(i, &["in", "on"]) && crate::query::date_span(r.low(i + 1)).is_some()) && !(r.low(i) == "on" && r.low(i + 1) != "hn") {
                    r.used[i] = true;
                } else if r.is(i, &["about", "for", "with", "mentioning", "regarding"]) {
                    r.used[i] = true;
                    break;
                } else {
                    break;
                }
            }
            f.ask = Ask::Sites;
            f.only = Some("story".into());
            return Some(finish(r, f));
        }
    }

    // "which post has received the most comments", "stories with the most points",
    // "which comment has the most replies", "the post with the fewest votes".
    for i in 0..n {
        let more = r.is(i, MORE);
        let less = r.is(i, LESS);
        if !(more || less) {
            continue;
        }
        let mut j = i + 1;
        if matches!(r.low(j), "number" | "amount") && r.low(j + 1) == "of" {
            j += 2;
        }
        let Some(m) = measure(r.low(j)) else { continue };
        // "top comments" is a list of comments, not "most comments".
        if r.low(i) == "top" && j == i + 1 && measure(r.low(j)) == Some("comments") && !r.is(i.wrapping_sub(1), &["the"]) && !(i > 0 && r.is(i - 1, HAVE)) {
            continue;
        }
        r.take(i, j - i + 1);
        r.take_lead(i);
        let it = r.take_item(i, false);
        let one = it.2.unwrap_or(true);
        set_item(&mut f, it);
        // "which ... has", the question words at the start.
        if r.is(0, WH) || r.is(0, &["show", "find", "list", "give", "get"]) {
            r.used[0] = true;
            if r.low(1) == "me" {
                r.used[1] = true;
            }
            for k in 1..3 {
                if r.is(k, BE) || r.is(k, &["the"]) {
                    r.used[k] = true;
                }
            }
        }
        // "comments with the most comments" is replies.
        let by = if less { format!("least_{m}") } else { m.to_string() };
        f.ask = Ask::Top { by, one };
        return Some(finish(r, f));
    }

    // "most commented stories", "most upvoted Show HN", "highest scoring post",
    // "biggest threads", "top 10 stories about rust", "first story about bitcoin".
    for i in 0..n {
        let w = r.low(i).to_string();
        let w1 = r.low(i + 1).to_string();
        let (by, k): (Option<&str>, usize) = match (w.as_str(), w1.as_str()) {
            ("most", a) if most_adj(a).is_some() => (most_adj(a), 2),
            ("highest" | "best" | "top", "scoring" | "scored" | "rated" | "ranked" | "voted" | "upvoted") => (Some("points"), 2),
            ("lowest" | "least", "scoring" | "scored" | "rated" | "voted" | "upvoted") => (Some("least_points"), 2),
            ("least", "commented" | "discussed") => (Some("least_comments"), 2),
            ("biggest" | "longest" | "largest" | "busiest" | "hottest", _) if item(&r.t, i + 1).is_some_and(|_| matches!(r.low(i + 1), "thread" | "threads" | "discussion" | "discussions" | "debate" | "debates")) => (Some("comments"), 1),
            ("first" | "earliest" | "oldest", _) => (Some("old"), 1),
            ("latest" | "newest" | "recent" | "last", _) if i == 0 || r.is(i - 1, &["the", "most"]) => (Some("date"), 1),
            ("top" | "best" | "greatest" | "hottest" | "biggest", _) => (Some("points"), 1),
            _ => (None, 0),
        };
        let Some(by) = by else { continue };
        // A number after: "top 10 stories".
        let mut j = i + k;
        let mut limit = None;
        if let Some(num) = number_word(r.low(j)) {
            limit = Some(num);
            j += 1;
        }
        // The adjective must be about items: an item word follows (or "most commented" alone).
        let item_here = item(&r.t, j).is_some();
        let strong = k == 2 && w == "most";
        if !(item_here || (strong && (i == 0 || j >= n))) {
            continue;
        }
        // "first", "top", "best" mid-title are words of the topic, not a frame.
        if k == 1 && !(i == 0 || r.is(i - 1, &["the", "most"]) || r.is(i - 1, WH) || r.is(i - 1, BE) || r.is(i - 1, &["me"])) {
            continue;
        }
        r.take(i, j - i);
        if i > 0 && r.is(i - 1, &["the", "most"]) {
            r.used[i - 1] = true;
        }
        if (w == "last" || w == "recent") && i > 0 && r.low(i - 1) == "most" {
            r.used[i - 1] = true;
        }
        let it = r.take_item(j + 1, true);
        let one = it.2.unwrap_or(false) && limit.is_none();
        set_item(&mut f, it);
        if r.is(0, WH) || r.is(0, &["show", "find", "list", "give", "get"]) {
            r.used[0] = true;
            for k2 in 1..3 {
                if r.is(k2, BE) || r.is(k2, &["the", "me"]) {
                    r.used[k2] = true;
                }
            }
        }
        f.limit = limit;
        f.ask = Ask::Top { by: by.to_string(), one };
        return Some(finish(r, f));
    }
    None
}

fn finish(r: Reader, mut f: Frame) -> Frame {
    f.rest = r.rest();
    f.said = r.said();
    f.kinds.dedup();
    f
}

/// What a frame asks, said back in a few words for its chip.
pub fn describe(a: &Ask, only: Option<&str>) -> String {
    let what = if only == Some("comment") { "comments" } else { "stories" };
    match a {
        Ask::Top { by, one } => {
            let s = match by.as_str() {
                "comments" if only == Some("comment") => "most replies first".to_string(),
                "comments" => "most comments first".to_string(),
                "points" => "most points first".to_string(),
                "date" => "newest first".to_string(),
                "old" => "oldest first".to_string(),
                "least_points" => "fewest points first".to_string(),
                "least_comments" => "fewest comments first".to_string(),
                other => other.to_string(),
            };
            if *one { format!("the one {what} answer: {s}") } else { s }
        }
        Ask::Count => format!("how many {what}"),
        Ask::People { by } => match by.as_str() {
            "karma" => "people by karma".into(),
            "knows" => "who knows it best".into(),
            "comments" => "people by comments".into(),
            "points" => "people by points".into(),
            _ => "people by stories posted".into(),
        },
        Ask::Sites => "sites by stories".into(),
        Ask::When => "when it was talked about most".into(),
        Ask::Opinion => "what HN thinks".into(),
    }
}

// --- Completion -------------------------------------------------------------

/// Questions the grammar reads, `{t}` the topic. [`complete`] finishes
/// what is typed with them; a test checks each is read as meant.
pub const TEMPLATES: &[&str] = &[
    "which story has the most comments",
    "which story has the most points",
    "which story about {t} has the most comments",
    "which story about {t} has the most points",
    "which Show HN has the most points",
    "which Ask HN has the most comments",
    "which comment has the most replies",
    "most discussed stories about {t}",
    "most upvoted stories about {t}",
    "most upvoted Show HN this year",
    "most commented stories this week",
    "top 10 stories about {t}",
    "first story about {t}",
    "oldest Ask HN about {t}",
    "newest stories about {t}",
    "how many stories mention {t}",
    "how many Show HN posts in 2025",
    "how many comments mention {t}",
    "who posted the most stories",
    "who posted the most stories about {t}",
    "who posted the most about {t}",
    "who commented the most about {t}",
    "who comments the most about {t}",
    "who knows the most about {t}",
    "who has the most karma",
    "which sites are posted the most",
    "which sites post the most about {t}",
    "when was {t} most discussed",
    "what does HN think about {t}",
];

/// The words a question starts with.
pub const LEADS: &[&str] = &["which", "what", "who", "how", "when", "where", "most", "top", "first", "oldest", "newest", "latest", "highest", "biggest", "fewest", "least"];

/// Whether typed words begin a question rather than a search of titles'
/// words: "which post ha", "how many", "top 5 sto", but not "how to learn"
/// or "first contact". `partial`: the last word may be unfinished.
pub fn starts_question(words: &[&str], partial: bool) -> bool {
    let Some(w0) = words.first().map(|w| w.to_lowercase()) else { return false };
    let w1 = words.get(1).map(|w| w.to_lowercase());
    let last1 = words.len() == 2 && partial;
    let is = |w: &str, list: &[&str]| list.contains(&w) || (last1 && w.len() >= 2 && list.iter().any(|x| x.starts_with(w)));
    const THINGS: &[&str] = &["story", "stories", "post", "posts", "submission", "submissions", "article", "articles", "comment", "comments", "thread", "threads", "upvoted", "voted", "commented", "discussed", "popular", "show", "ask", "about", "link", "links"];
    match w0.as_str() {
        "which" | "who" | "when" | "where" | "fewest" | "least" | "what" => words.len() > 1 || !partial,
        "how" => w1.is_some_and(|w| is(&w, &["many", "much", "often"])),
        "most" | "top" | "first" | "oldest" | "newest" | "latest" | "highest" | "biggest" => w1.is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()) || is(&w, THINGS)),
        _ => false,
    }
}

/// Templates offered for a topic that was typed on its own ("postgres").
const FOR_TOPIC: &[&str] = &["most upvoted stories about {t}", "who knows the most about {t}", "when was {t} most discussed", "what does HN think about {t}", "first story about {t}", "how many stories mention {t}"];

/// Words a template's word stands for: typing any of them fits it.
const CLASSES: &[&[&str]] = &[
    &["story", "post", "submission", "article", "link", "thread"],
    &["stories", "posts", "submissions", "articles", "links", "threads"],
    &["points", "upvotes", "votes"],
    &["comments", "replies"],
    &["discussed", "commented"],
    &["upvoted", "popular", "voted"],
    &["sites", "domains", "websites"],
    &["has", "got", "received", "had", "gets"],
    &["mention", "about"],
];

fn class_of(w: &str) -> &'static [&'static str] {
    CLASSES.iter().find(|c| c.contains(&w)).copied().unwrap_or(&[])
}

fn same_word(typed: &str, tpl: &str) -> bool {
    let t = typed.to_lowercase();
    let p = tpl.to_lowercase();
    t == p || class_of(&p).contains(&t.as_str())
}

/// The word an unfinished typed word becomes at a template's word: the
/// template's own, or one it stands for that starts so ("po" is "post").
fn finish_word(typed: &str, tpl: &str) -> Option<String> {
    let t = typed.to_lowercase();
    let p = tpl.to_lowercase();
    if p.starts_with(&t) {
        return Some(tpl.to_string());
    }
    class_of(&p).iter().find(|w| w.starts_with(&t)).map(|w| w.to_string())
}

/// Fits typed words to a template: the finished question, or `None`.
/// `partial` says the last typed word may be unfinished.
fn fit(typed: &[String], partial: bool, tpl: &str, topic_hint: Option<&str>) -> Option<(String, usize)> {
    let tp: Vec<&str> = tpl.split(' ').collect();
    let mut out: Vec<String> = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut matched = 0;
    while i < tp.len() {
        if j >= typed.len() {
            // The rest of the template finishes the question.
            for w in &tp[i..] {
                if *w == "{t}" {
                    match topic_hint {
                        Some(t) => out.push(t.to_string()),
                        // No topic to offer: the question up to it, left
                        // open (a trailing space) for the topic to be typed.
                        None => return (matched >= 2).then(|| (format!("{} ", out.join(" ")), matched)),
                    }
                } else {
                    out.push(w.to_string());
                }
            }
            return Some((out.join(" "), matched));
        }
        let last = j + 1 == typed.len();
        if tp[i] == "{t}" {
            // The topic takes the typed words up to the template's next word.
            let next = tp.get(i + 1);
            let start = j;
            while j < typed.len() {
                if let Some(nw) = next {
                    let lastw = j + 1 == typed.len();
                    if j > start && (same_word(&typed[j], nw) || (lastw && partial && finish_word(&typed[j], nw).is_some())) {
                        break;
                    }
                }
                j += 1;
            }
            out.push(typed[start..j].join(" "));
            matched += j - start;
            i += 1;
            continue;
        }
        let w = &typed[j];
        if last && partial {
            if same_word(w, tp[i]) {
                out.push(w.clone());
            } else {
                out.push(finish_word(w, tp[i])?);
            }
        } else if same_word(w, tp[i]) {
            out.push(w.clone());
        } else {
            return None;
        }
        matched += 1;
        i += 1;
        j += 1;
    }
    // Typed past the template's end: not this one.
    if j < typed.len() {
        return None;
    }
    Some((out.join(" "), matched))
}

/// Questions that finish what is typed, best first. `topic` is a topic to
/// fill in where the template wants one and nothing typed gives it (the
/// best completion of the typed words, when they are a topic).
pub fn complete(typed: &str, topic: Option<&str>, max: usize) -> Vec<String> {
    let partial = !typed.ends_with(' ');
    let words: Vec<String> = typed.split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut got: Vec<(usize, usize, String)> = Vec::new();
    for (k, tpl) in TEMPLATES.iter().enumerate() {
        if let Some((q, matched)) = fit(&words, partial, tpl, topic) {
            // Two letters do not make a question yet.
            if matched == 0 || (words.len() == 1 && words[0].len() < 3) {
                continue;
            }
            if q.trim().to_lowercase() == typed.trim().to_lowercase() {
                continue;
            }
            got.push((matched, k, q));
        }
    }
    // A typed topic, on its own: questions about it.
    let starts_question = words.len() == 1 && partial && LEADS.iter().any(|w| w.starts_with(&words[0].to_lowercase()));
    // Not "how to learn" or "the rust book": a topic is not a sentence's start.
    let sentence = ["how", "why", "what", "the", "a", "an", "to", "is", "i", "my", "we", "you", "do", "does", "can", "should"].contains(&words[0].to_lowercase().as_str()) && words.len() > 1;
    let ws: Vec<&str> = words.iter().map(String::as_str).collect();
    if got.is_empty() && !starts_question && !sentence && !self::starts_question(&ws, partial) && typed.trim().len() >= 3 && read(typed).is_none() && !typed.contains(':') {
        let t = if partial { topic.map(str::to_string).or_else(|| Some(typed.trim().to_string())) } else { Some(typed.trim().to_string()) };
        if let Some(t) = t.filter(|t| !t.is_empty()) {
            for (k, tpl) in FOR_TOPIC.iter().enumerate() {
                got.push((0, 100 + k, tpl.replace("{t}", &t)));
            }
        }
    }
    got.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut seen = std::collections::HashSet::new();
    got.into_iter().filter(|g| seen.insert(g.2.to_lowercase())).take(max).map(|g| g.2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(q: &str) -> (String, bool, Option<String>, String) {
        let f = read(q).unwrap_or_else(|| panic!("no frame in {q:?}"));
        match f.ask {
            Ask::Top { by, one } => (by, one, f.only, f.rest),
            other => panic!("{q:?} read as {other:?}"),
        }
    }

    #[test]
    fn open_questions_wait_for_their_topic() {
        let q = complete("how many st", None, 4);
        assert!(q.contains(&"how many stories mention ".to_string()), "{q:?}");
        let q = complete("which post", None, 6);
        assert!(q.contains(&"which post has the most comments".to_string()), "{q:?}");
        assert!(q.iter().any(|x| x == "which post about "), "{q:?}");
        assert!(complete("how to learn", Some("how to learning"), 4).is_empty());
        assert!(complete("which post haskell ", None, 4).is_empty(), "a question's start is not a topic");
    }

    #[test]
    fn questions_being_typed() {
        for q in ["which post ha", "who posted the most about ru", "how many sto", "top 5 stories", "most upvoted", "first story about", "what does hn"] {
            let w: Vec<&str> = q.split(' ').collect();
            assert!(starts_question(&w, true), "{q}");
        }
        for q in ["how to learn rust", "first contact", "top gun", "rust async", "most"] {
            let w: Vec<&str> = q.split(' ').collect();
            assert!(!starts_question(&w, true), "{q}");
        }
    }

    #[test]
    fn the_most_of_something() {
        assert_eq!(top("which post has received the most comments"), ("comments".into(), true, Some("story".into()), String::new()));
        assert_eq!(top("Which story got the most upvotes?"), ("points".into(), true, Some("story".into()), String::new()));
        assert_eq!(top("what was the post with the highest number of comments"), ("comments".into(), true, Some("story".into()), String::new()));
        assert_eq!(top("stories with the most points"), ("points".into(), false, Some("story".into()), String::new()));
        assert_eq!(top("which post about rust has the most points"), ("points".into(), true, Some("story".into()), "rust".into()));
        assert_eq!(top("which comment has the most replies"), ("comments".into(), true, Some("comment".into()), String::new()));
        assert_eq!(top("which post on hacker news has the most comments of all time"), ("comments".into(), true, Some("story".into()), String::new()));
        assert_eq!(top("posts about kubernetes with the fewest votes"), ("least_points".into(), false, Some("story".into()), "kubernetes".into()));
        let f = read("which show hn got the most points in 2023").unwrap();
        assert_eq!(f.kinds, vec!["show"]);
        assert_eq!(f.rest, "in 2023");
    }

    #[test]
    fn superlative_words() {
        assert_eq!(top("most commented stories"), ("comments".into(), false, Some("story".into()), String::new()));
        assert_eq!(top("most upvoted Show HN of 2023").0, "points");
        assert_eq!(read("most upvoted Show HN of 2023").unwrap().kinds, vec!["show"]);
        assert_eq!(top("most discussed"), ("comments".into(), false, None, String::new()));
        assert_eq!(top("highest scoring post about postgres"), ("points".into(), true, Some("story".into()), "postgres".into()));
        assert_eq!(top("biggest threads about remote work"), ("comments".into(), false, Some("story".into()), "remote work".into()));
        assert_eq!(top("first story about bitcoin"), ("old".into(), true, Some("story".into()), "bitcoin".into()));
        assert_eq!(top("the oldest Ask HN"), ("old".into(), true, Some("story".into()), String::new()));
        assert_eq!(top("latest posts about llms"), ("date".into(), false, Some("story".into()), "llms".into()));
        let f = read("top 10 stories about rust").unwrap();
        assert_eq!(f.limit, Some(10));
        assert_eq!(f.rest, "rust");
    }

    #[test]
    fn searches_are_not_questions() {
        for q in ["rust async", "postgres vs mysql after:2020", "best practices for go", "the first rule of programming", "top", "kubernetes", "most", "author:pg", "How to learn Rust", "what is a monad"] {
            assert!(read(q).is_none(), "{q:?} read as {:?}", read(q));
        }
    }

    #[test]
    fn counts_people_sites_and_years() {
        let f = read("how many stories mention rust").unwrap();
        assert_eq!((f.ask, f.only.as_deref(), f.rest.as_str()), (Ask::Count, Some("story"), "rust"));
        let f = read("how many Show HN posts were there in 2024?").unwrap();
        assert_eq!((f.ask, f.kinds.clone(), f.rest.as_str()), (Ask::Count, vec!["show".to_string()], "in 2024"));
        let f = read("who posted the most stories about rust").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::People { by: "stories".into() }, "rust"));
        let f = read("who comments the most about postgres").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::People { by: "comments".into() }, "postgres"));
        let f = read("who has the most karma").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::People { by: "karma".into() }, ""));
        let f = read("who knows the most about kubernetes").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::People { by: "knows".into() }, "kubernetes"));
        let f = read("top commenters on rust").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::People { by: "comments".into() }, "rust"));
        let f = read("which sites are posted the most").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::Sites, ""));
        let f = read("most popular domains for machine learning").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::Sites, "machine learning"));
        let f = read("when was bitcoin most discussed").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::When, "bitcoin"));
        let f = read("when did HN talk about kubernetes the most").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::When, "kubernetes"));
        let f = read("what does HN think about nix").unwrap();
        assert_eq!((f.ask, f.rest.as_str()), (Ask::Opinion, "nix"));
    }

    #[test]
    fn every_template_is_read_as_a_question() {
        for t in TEMPLATES.iter().chain(FOR_TOPIC) {
            let q = t.replace("{t}", "rust");
            let f = read(&q).unwrap_or_else(|| panic!("{q:?} is not read"));
            let rest = f.rest.to_lowercase();
            assert!(rest.is_empty() || rest.contains("rust") || rest.contains("20") || rest.contains("this"), "{q:?} left {rest:?}");
        }
    }

    #[test]
    fn completes_questions_as_typed() {
        let c = complete("which po", None, 5);
        assert!(c.iter().any(|q| q == "which post has the most comments"), "{c:?}");
        let c = complete("which post about rust has the m", None, 5);
        assert!(c.contains(&"which post about rust has the most points".to_string()), "{c:?}");
        let c = complete("who ", None, 5);
        assert!(c.iter().any(|q| q == "who has the most karma"), "{c:?}");
        let c = complete("how many stories mention zig", None, 5);
        assert!(c.is_empty(), "{c:?}");
        let c = complete("postgres", None, 3);
        assert_eq!(c[0], "most upvoted stories about postgres");
        let c = complete("postg", Some("postgres"), 3);
        assert_eq!(c[1], "who knows the most about postgres");
        assert!(complete("wh", None, 5).is_empty());
    }
}
