//! A paper's row: what arXiv says of it, with what the app reads from its
//! words (techniques, domains, kinds, venue, links) and its citations. The
//! same function makes the rows of `prepare` and of the live feed.

use serde_json::{json, Value};

use crate::concepts::{self, display_name, one_line, person_key, unlatex};
use crate::raw::Raw;

/// One author of a paper: their key and their name as shown.
#[derive(Debug, Clone)]
pub struct Person {
    pub key: String,
    pub name: String,
}

pub fn people(r: &Raw) -> Vec<Person> {
    let mut out: Vec<Person> = Vec::new();
    for (last, first, suffix) in &r.authors {
        let (last, first) = (unlatex(last), unlatex(first));
        let key = person_key(&first, &last);
        if key.is_empty() || out.iter().any(|p| p.key == key) {
            continue;
        }
        out.push(Person { key, name: display_name(&first, &last, suffix) });
    }
    out
}

pub fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

pub fn year_month(t: i64) -> (i32, String) {
    use chrono::Datelike;
    let d = chrono::DateTime::from_timestamp(t, 0).unwrap_or_default();
    (d.year(), format!("{:04}-{:02}", d.year(), d.month()))
}

/// When a paper was first posted: its first version, else the month of a
/// new-style id (`2401.01234`), else 1991.
pub fn created(r: &Raw) -> i64 {
    if let Some(t) = r.versions.first() {
        return *t;
    }
    let digits: String = r.id.rsplit('/').next().unwrap_or("").chars().take(4).collect();
    if digits.len() == 4 && digits.chars().all(|c| c.is_ascii_digit()) {
        let yy: i32 = digits[..2].parse().unwrap_or(91);
        let mm: u32 = digits[2..].parse().unwrap_or(1);
        let y = if yy >= 91 { 1900 + yy } else { 2000 + yy };
        return chrono::NaiveDate::from_ymd_opt(y, mm.clamp(1, 12), 1).and_then(|d| d.and_hms_opt(0, 0, 0)).map(|d| d.and_utc().timestamp()).unwrap_or(0);
    }
    662_688_000
}

pub fn license_short(l: &str) -> &'static str {
    let l = l.to_lowercase();
    if l.contains("by-nc-sa") {
        "CC BY-NC-SA"
    } else if l.contains("by-nc-nd") {
        "CC BY-NC-ND"
    } else if l.contains("by-sa") {
        "CC BY-SA"
    } else if l.contains("by-nc") {
        "CC BY-NC"
    } else if l.contains("/by/") {
        "CC BY"
    } else if l.contains("publicdomain") || l.contains("zero") {
        "CC0"
    } else if l.contains("nonexclusive") {
        "arXiv"
    } else {
        ""
    }
}

/// The derived parts of a paper that `prepare` also counts.
pub struct Built {
    pub row: Value,
    pub people: Vec<Person>,
    pub created: i64,
    pub techniques: Vec<String>,
    pub domains: Vec<String>,
    pub primary: String,
    pub cats: Vec<String>,
}

/// The row of a paper with `cites` citations (Crossref's count) and its
/// `topic` (the cluster of `tools/topics.py`, if known).
pub fn build(r: &Raw, cites: u32, topic: Option<i64>) -> Built {
    let title = one_line(&r.title);
    let abstract_ = one_line(&r.abstract_);
    let people = people(r);
    let created = created(r);
    let updated = r.versions.last().copied().unwrap_or(created);
    let (year, month) = year_month(created);
    let primary = r.cats.first().cloned().unwrap_or_default();
    let group = concepts::group_of(&primary);
    let mut groups: Vec<String> = Vec::new();
    for c in &r.cats {
        let g = concepts::group_of(c);
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let (techniques, domains) = concepts::dict().tag(&title, &abstract_, &group);
    let kinds = concepts::kinds(&title, &abstract_, &primary);
    let (venue, venue_year) = concepts::venue(&r.comments, &r.journal_ref, year);
    let (code, hf) = concepts::links(&abstract_, &r.comments);
    let (pages, figures) = concepts::pages_figures(&r.comments);
    let names: Vec<&str> = people.iter().map(|p| p.name.as_str()).collect();
    let authors_line = if names.len() > 30 { format!("{} et al. ({} authors)", names[..30].join(", "), names.len()) } else { names.join(", ") };
    let row = json!({
        "id": r.id,
        "title": title,
        "abstract": abstract_,
        "authors": authors_line,
        "people": people.iter().take(60).map(|p| p.key.clone()).collect::<Vec<_>>(),
        "first_author": people.first().map(|p| p.key.clone()).unwrap_or_default(),
        "nauthors": people.len(),
        "cats": r.cats,
        "pcat": primary,
        "area": group,
        "areas": groups,
        "created": iso(created),
        "updated": iso(updated),
        "year": year,
        "month": month,
        "versions": r.versions.len().max(1),
        "doi": r.doi,
        "journal": one_line(&r.journal_ref),
        "venue": venue,
        "venue_year": venue_year,
        "comments": one_line(&r.comments),
        "pages": pages,
        "figures": figures,
        "code": code,
        "hf": hf,
        "techniques": techniques,
        "domains": domains,
        "kinds": kinds,
        "cites": cites,
        "pop": ((1.0 + cites as f64).ln() * 1000.0).round() / 1000.0,
        "license": license_short(&r.license),
        "topic": topic,
    });
    Built { row, people, created, techniques, domains, primary, cats: r.cats.clone() }
}
