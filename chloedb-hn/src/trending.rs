//! Trending: the words and phrases HN's titles suddenly use far more than
//! they did the year before, found by their counts alone (no list of
//! topics, no model). Each month of the archive is compared with the twelve
//! before it (`hn_terms`, written once from the rows; the month's spikes
//! kept in `hn_spikes`), and the last hours of the live feed with the
//! archive's last year, as they are asked for.
//!
//! A term is a title's word or pair of words. Its spike is scored by
//! Dunning's log-likelihood ratio (G²) of its share of the window's titles
//! against its share of the baseline's, which favours a jump that many
//! titles show over a rare word's first appearance; it must be at least
//! three times its usual share, and in enough titles.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use serde_json::{json, Value};

use crate::search::Ctx;
use crate::strata::RowExt;

/// Words of titles that are tags or filler, not subjects.
const FILLER: &[&str] = &[
    "pdf", "video", "audio", "part", "year", "years", "day", "days", "week", "weeks", "month", "months", "time", "first", "last", "best", "top", "free", "make", "made", "way", "ways",
    "like", "need", "want", "know", "things", "thing", "good", "better", "people", "work", "world", "really", "still", "after", "over", "back", "should", "would", "could", "every",
    "some", "any", "own", "other", "my", "his", "her", "their", "they", "them", "he", "she", "there", "here", "where", "these", "those", "too", "very", "being", "been", "had", "did",
    "don't", "doesn't", "isn't", "you're", "i'm", "we're", "go", "goes", "going", "say", "says", "said", "see", "look", "find", "found", "take", "big", "small", "long", "old", "great",
    "via", "between", "under", "against", "without", "before", "while", "during", "less", "much", "many", "few", "two", "three", "one's", "lot", "lots", "s", "t",
];

fn filler(w: &str) -> bool {
    crate::complete::is_stop(w) || FILLER.contains(&w) || w.chars().count() < 2 || w.chars().all(|c| !c.is_alphanumeric())
}

fn digits(w: &str) -> bool {
    w.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// A title's terms: its words that name something, and its pairs of
/// neighbouring words that do (the second may be a version: "gpt 4").
pub fn terms(title: &str) -> Vec<String> {
    // "Huang's" counts as "huang": a possessive is the same name.
    let ws: Vec<String> = crate::complete::words(title).into_iter().map(|w| {
        for s in ["'s", "\u{2019}s"] {
            if let Some(x) = w.strip_suffix(s).filter(|x| !x.is_empty()) {
                return x.to_string();
            }
        }
        w
    }).collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (i, w) in ws.iter().enumerate() {
        if filler(w) || digits(w) {
            continue;
        }
        if seen.insert(w.clone()) {
            out.push(w.clone());
        }
        if let Some(x) = ws.get(i + 1) {
            if !filler(x) || (digits(x) && x.chars().any(|c| c.is_ascii_digit())) {
                let g = format!("{w} {x}");
                if seen.insert(g.clone()) {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// Dunning's G² for `a` of `big_a` against `b` of `big_b`.
pub fn g2(a: f64, big_a: f64, b: f64, big_b: f64) -> f64 {
    let n = big_a + big_b;
    let k = a + b;
    let cell = |o: f64, e: f64| if o > 0.0 && e > 0.0 { o * (o / e).ln() } else { 0.0 };
    let e1 = big_a * k / n;
    let e2 = big_b * k / n;
    let e3 = big_a * (n - k) / n;
    let e4 = big_b * (n - k) / n;
    2.0 * (cell(a, e1) + cell(b, e2) + cell(big_a - a, e3) + cell(big_b - b, e4))
}

#[derive(Clone, Debug)]
pub struct Spike {
    pub term: String,
    pub n: u64,
    pub expected: f64,
    pub ratio: f64,
    pub score: f64,
    pub points: u64,
    pub top: i64,
    pub top_points: i64,
}

/// The spikes among `cands` (term, count in the window, points, top story,
/// its points, count in the baseline), for a window of `big_a` titles and a
/// baseline of `big_b`: at least `min_n` titles and three times the usual
/// share, best first, a word left out when a pair holding it explains it.
pub fn spikes(cands: Vec<(String, u64, u64, i64, i64, u64)>, big_a: f64, big_b: f64, min_n: u64, keep: usize) -> Vec<Spike> {
    let mut out: Vec<Spike> = cands
        .into_iter()
        .filter(|c| c.1 >= min_n)
        .filter_map(|(term, a, points, top, top_points, b)| {
            let expected = big_a * (b as f64 + 1.0) / big_b.max(1.0);
            let ratio = a as f64 / expected;
            if ratio < 3.0 {
                return None;
            }
            let score = g2(a as f64, big_a, b as f64, big_b);
            Some(Spike { term, n: a, expected, ratio, score, points, top, top_points })
        })
        .collect();
    out.sort_by(|x, y| y.score.total_cmp(&x.score));
    let pairs: Vec<(String, u64)> = out.iter().filter(|s| s.term.contains(' ')).map(|s| (s.term.clone(), s.n)).collect();
    out.retain(|s| {
        if s.term.contains(' ') {
            return true;
        }
        !pairs.iter().any(|(p, n)| p.split(' ').any(|w| w == s.term) && *n * 2 >= s.n)
    });
    // Two pairs of the same story ("apple vision", "vision pro"): the
    // stronger one.
    let mut tops = HashSet::new();
    out.retain(|s| s.n > 6 || tops.insert(s.top));
    out.truncate(keep);
    out
}

#[derive(Default, Clone, Copy)]
struct Agg {
    n: u32,
    points: u64,
    top: i64,
    top_points: i64,
}

/// Reads `dir/hn_stories.jsonl` and writes `dir/hn_terms.jsonl` (each
/// month's terms in three titles or more, and its count of titles as term
/// `*`) and `dir/hn_spikes.jsonl` (each month's spikes against the twelve
/// before it); the rows written.
pub fn build(dir: &Path, progress: impl Fn(&str)) -> Result<(u64, u64), String> {
    let path = dir.join("hn_stories.jsonl");
    let f = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut by: HashMap<(u16, u32), Agg> = HashMap::new();
    let mut months: HashMap<u16, u64> = HashMap::new();
    let mut titles: HashMap<i64, String> = HashMap::new();
    let mut n = 0u64;
    for line in BufReader::with_capacity(1 << 20, f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let created = v["created"].as_str().unwrap_or("");
        let (Some(y), Some(m)) = (created.get(0..4).and_then(|x| x.parse::<u16>().ok()), created.get(5..7).and_then(|x| x.parse::<u16>().ok())) else { continue };
        if !(2006..2100).contains(&y) {
            continue;
        }
        let mi = (y - 2006) * 12 + (m - 1);
        let points = v["points"].as_i64().unwrap_or(0).max(0);
        let id = v["id"].as_i64().unwrap_or(0);
        let title = v["title"].as_str().unwrap_or("");
        *months.entry(mi).or_default() += 1;
        for t in terms(title) {
            let k = match ids.get(&t) {
                Some(k) => *k,
                None => {
                    let k = names.len() as u32;
                    names.push(t.clone());
                    ids.insert(t, k);
                    k
                }
            };
            let a = by.entry((mi, k)).or_default();
            a.n += 1;
            a.points += points as u64;
            if points > a.top_points || a.top == 0 {
                a.top = id;
                a.top_points = points;
            }
        }
        if points >= 30 {
            titles.insert(id, title.to_string());
        }
        n += 1;
        if n % 1_000_000 == 0 {
            progress(&format!("trending: {n} stories, {} terms, {} month-terms", names.len(), by.len()));
        }
    }
    let month_name = |mi: u16| format!("{:04}-{:02}", 2006 + mi / 12, mi % 12 + 1);
    // hn_terms: the month-terms in three titles or more, and each month's count.
    let out = dir.join("hn_terms.jsonl");
    let mut w = BufWriter::new(std::fs::File::create(&out).map_err(|e| e.to_string())?);
    let mut rows = 0u64;
    for (&mi, &c) in &months {
        let m = month_name(mi);
        writeln!(w, "{}", json!({"id": format!("{m}|*"), "month": m, "term": "*", "n": c, "points": 0, "top": 0})).map_err(|e| e.to_string())?;
        rows += 1;
    }
    for (&(mi, k), a) in &by {
        if a.n >= 3 {
            let m = month_name(mi);
            writeln!(w, "{}", json!({"id": format!("{m}|{}", names[k as usize]), "month": m, "term": names[k as usize], "n": a.n, "points": a.points, "top": a.top})).map_err(|e| e.to_string())?;
            rows += 1;
        }
    }
    w.flush().map_err(|e| e.to_string())?;
    // hn_spikes: each month against the twelve before it.
    let mut per_month: HashMap<u16, Vec<(u32, Agg)>> = HashMap::new();
    for (&(mi, k), a) in &by {
        if a.n >= 5 {
            per_month.entry(mi).or_default().push((k, *a));
        }
    }
    let out2 = dir.join("hn_spikes.jsonl");
    let mut w2 = BufWriter::new(std::fs::File::create(&out2).map_err(|e| e.to_string())?);
    let mut srows = 0u64;
    let mut ms: Vec<u16> = per_month.keys().copied().filter(|m| *m >= 12).collect();
    ms.sort_unstable();
    for mi in ms {
        let big_a = *months.get(&mi).unwrap_or(&0) as f64;
        let big_b: f64 = (mi - 12..mi).map(|x| *months.get(&x).unwrap_or(&0) as f64).sum();
        if big_a < 500.0 || big_b < 5000.0 {
            continue;
        }
        let cands: Vec<(String, u64, u64, i64, i64, u64)> = per_month[&mi]
            .iter()
            .map(|(k, a)| {
                let b: u64 = (mi - 12..mi).map(|x| by.get(&(x, *k)).map(|g| g.n as u64).unwrap_or(0)).sum();
                (names[*k as usize].clone(), a.n as u64, a.points, a.top, a.top_points, b)
            })
            .collect();
        let m = month_name(mi);
        for (r, s) in spikes(cands, big_a, big_b, 6, 12).into_iter().enumerate() {
            writeln!(w2, "{}", json!({
                "id": format!("{m}|{:02}", r + 1), "month": m, "year": 2006 + mi / 12, "rank": r + 1, "term": s.term, "n": s.n,
                "expected": (s.expected * 10.0).round() / 10.0, "ratio": (s.ratio * 10.0).round() / 10.0, "score": s.score.round(),
                "points": s.points, "top": s.top, "top_title": titles.get(&s.top).cloned().unwrap_or_default(),
            })).map_err(|e| e.to_string())?;
            srows += 1;
        }
    }
    w2.flush().map_err(|e| e.to_string())?;
    progress(&format!("trending: wrote {rows} month-terms to {} and {srows} spikes to {}", out.display(), out2.display()));
    Ok((rows, srows))
}

type Cached = (std::time::Instant, Value);
static NOW: std::sync::Mutex<Option<HashMap<i64, Cached>>> = std::sync::Mutex::new(None);

/// What is trending in the last `hours`: the terms of the newest titles
/// against the archive's last twelve months, kept ten minutes.
pub async fn now(c: &Ctx<'_>, hours: i64) -> Value {
    let hours = hours.clamp(6, 24 * 31);
    if let Some(v) = NOW.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).get(&hours).filter(|x| x.0.elapsed().as_secs() < 600).map(|x| x.1.clone()) {
        return v;
    }
    // A query returns at most 10,000 rows: a month's titles come a page at
    // a time, newest first, each page below the last one's smallest id.
    let mut rows = Vec::new();
    let mut below = i64::MAX;
    for _ in 0..6 {
        let page = c.soft("the newest titles", &format!("SELECT id, title, points, comments, created FROM hn_stories WHERE created > now() - INTERVAL '{hours} hours' AND id < {below} ORDER BY id DESC LIMIT 10000"), Value::Null).await;
        let n = page.len();
        if let Some(last) = page.last() {
            below = last.i("id");
        }
        rows.extend(page);
        if n < 10000 {
            break;
        }
    }
    let big_a = rows.len() as f64;
    let mut agg: HashMap<String, (u64, u64, i64, i64)> = HashMap::new();
    let mut titles: HashMap<i64, (String, i64, i64)> = HashMap::new();
    for r in &rows {
        let (id, pts) = (r.i("id"), r.i("points"));
        titles.insert(id, (r.s("title"), pts, r.i("comments")));
        for t in terms(&r.s("title")) {
            let e = agg.entry(t).or_insert((0, 0, 0, -1));
            e.0 += 1;
            e.1 += pts.max(0) as u64;
            if pts > e.3 {
                e.2 = id;
                e.3 = pts;
            }
        }
    }
    let min_n = if hours <= 48 { 3 } else { 4 };
    let mut cands: Vec<(String, (u64, u64, i64, i64))> = agg.into_iter().filter(|(_, v)| v.0 >= min_n).collect();
    cands.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    cands.truncate(600);
    // The baseline: the archive's last twelve months.
    let ms = c.soft("the archive's months", "SELECT month, n FROM hn_terms WHERE term = '*' ORDER BY month DESC LIMIT 12", Value::Null).await;
    let big_b: f64 = ms.iter().map(|r| r.i("n") as f64).sum();
    let from = ms.last().map(|r| r.s("month")).unwrap_or_default();
    let mut base: HashMap<String, u64> = HashMap::new();
    if !cands.is_empty() && big_b > 0.0 {
        let list: Vec<String> = cands.iter().map(|(t, _)| format!("'{}'", t.replace('\'', "''"))).collect();
        // month is a KEYWORD: the twelve months by name, not a range.
        let months: Vec<String> = ms.iter().map(|r| format!("'{}'", r.s("month").replace('\'', "''"))).collect();
        let got = c.soft("the same terms over the year before", &format!("SELECT term, SUM(n) AS n FROM hn_terms WHERE month IN ({}) AND term IN ({}) GROUP BY term", months.join(", "), list.join(", ")), Value::Null).await;
        for r in got {
            base.insert(r.s("term"), r.i("n").max(0) as u64);
        }
    }
    let list: Vec<(String, u64, u64, i64, i64, u64)> = cands.into_iter().map(|(t, v)| {
        let b = base.get(&t).copied().unwrap_or(0);
        (t, v.0, v.1, v.2, v.3, b)
    }).collect();
    let found = if big_b > 0.0 { spikes(list, big_a, big_b, min_n, 24) } else { Vec::new() };
    let v = json!({
        "hours": hours, "stories": rows.len(), "baseline": {"from": from, "months": ms.len(), "stories": big_b},
        "terms": found.iter().map(|s| {
            let t = titles.get(&s.top);
            json!({"term": s.term, "n": s.n, "expected": (s.expected * 100.0).round() / 100.0, "ratio": (s.ratio * 10.0).round() / 10.0, "score": s.score.round(), "points": s.points,
                   "top": {"id": s.top, "title": t.map(|x| x.0.clone()), "points": t.map(|x| x.1), "comments": t.map(|x| x.2)}})
        }).collect::<Vec<_>>(),
    });
    if rows.len() >= 200 {
        NOW.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(hours, (std::time::Instant::now(), v.clone()));
    }
    v
}

/// The spikes of a year's months (all twelve of each), or the strongest
/// three of every month when no year is given.
pub async fn history(c: &Ctx<'_>, year: Option<i64>) -> Value {
    let rows = match year {
        Some(y) => c.soft("a year's spikes", "SELECT month, rank, term, n, expected, ratio, score, points, \"top\", top_title FROM hn_spikes WHERE year = :y ORDER BY id LIMIT 200", json!({"y": y})).await,
        None => c.soft("every month's strongest spikes", "SELECT month, rank, term, n, expected, ratio, score, points, \"top\", top_title FROM hn_spikes WHERE rank <= 3 ORDER BY id LIMIT 1000", Value::Null).await,
    };
    json!({"year": year, "spikes": rows})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn possessives_are_the_name() {
        let t = terms("Jensen Huang's keynote and Jensen Huang");
        assert!(t.contains(&"jensen huang".to_string()));
        assert!(!t.iter().any(|x| x.contains("huang's")));
    }

    #[test]
    fn terms_of_a_title() {
        let t = terms("Show HN: Apple Vision Pro review [video]");
        assert!(t.contains(&"apple".to_string()));
        assert!(t.contains(&"vision pro".to_string()));
        assert!(!t.iter().any(|x| x == "video" || x == "show"));
        let t = terms("GPT 4 is out");
        assert!(t.contains(&"gpt 4".to_string()));
        assert!(!t.contains(&"4".to_string()));
    }

    #[test]
    fn a_jump_beats_a_rare_word() {
        // 40 of 1,000 titles this month, 12 of 12,000 before: a spike.
        let s = spikes(vec![("heartbleed".into(), 40, 9000, 1, 3000, 12), ("zork".into(), 3, 10, 2, 5, 0), ("rust".into(), 30, 900, 3, 100, 340)], 1000.0, 12000.0, 3, 10);
        assert_eq!(s[0].term, "heartbleed");
        assert!(!s.iter().any(|x| x.term == "rust"), "rust is at its usual share");
    }

    #[test]
    fn a_pair_explains_its_words() {
        let s = spikes(vec![("vision pro".into(), 30, 0, 1, 0, 0), ("vision".into(), 34, 0, 1, 0, 2), ("apple".into(), 80, 0, 2, 0, 3000)], 1000.0, 12000.0, 3, 10);
        assert!(s.iter().any(|x| x.term == "vision pro"));
        assert!(!s.iter().any(|x| x.term == "vision"));
    }
}
