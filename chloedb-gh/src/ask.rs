//! Questions in a sentence, answered by one statement (or two) of SQL
//! that the page shows: no model reads them. Each kind of question is a
//! pattern; what it found (a language, a year, a repo, a number) fills the
//! statement's parameters. A question no pattern knows becomes a search.

use std::sync::OnceLock;

use chrono::{Datelike, TimeZone, Utc};
use regex::Regex;
use serde_json::{json, Value};

use crate::pages::{self, Ctx, DAY, REPO_COLS, YEAR};
use crate::strata::{Result, RowExt};

/// The languages a question may name, as GitHub spells them, with the
/// words for each. A word that is also an English word ("go", "r", "c")
/// counts only next to "in", "written in", "repos" or "projects".
const LANGUAGES: &[(&str, &[&str])] = &[
    ("Rust", &["rust"]),
    ("Python", &["python"]),
    ("JavaScript", &["javascript", "js"]),
    ("TypeScript", &["typescript", "ts"]),
    ("Go", &["go", "golang"]),
    ("Java", &["java"]),
    ("C++", &["c++", "cpp"]),
    ("C#", &["c#", "csharp"]),
    ("C", &["c"]),
    ("Ruby", &["ruby"]),
    ("PHP", &["php"]),
    ("Swift", &["swift"]),
    ("Kotlin", &["kotlin"]),
    ("Scala", &["scala"]),
    ("Haskell", &["haskell"]),
    ("Elixir", &["elixir"]),
    ("Erlang", &["erlang"]),
    ("Zig", &["zig"]),
    ("Julia", &["julia"]),
    ("R", &["r"]),
    ("Dart", &["dart"]),
    ("Lua", &["lua"]),
    ("Shell", &["shell", "bash"]),
    ("Nim", &["nim"]),
    ("OCaml", &["ocaml"]),
    ("Clojure", &["clojure"]),
    ("Perl", &["perl"]),
    ("Jupyter Notebook", &["jupyter", "notebook", "notebooks"]),
    ("Vue", &["vue"]),
    ("Svelte", &["svelte"]),
    ("HTML", &["html"]),
    ("CSS", &["css"]),
    ("Objective-C", &["objective-c", "objc"]),
    ("Elm", &["elm"]),
    ("Crystal", &["crystal"]),
    ("V", &["vlang"]),
    ("Solidity", &["solidity"]),
    ("Fortran", &["fortran"]),
    ("Assembly", &["assembly", "asm"]),
];

const AMBIGUOUS: &[&str] = &[
    "go",
    "c",
    "r",
    "ts",
    "js",
    "notebook",
    "notebooks",
    "assembly",
    "crystal",
    "elm",
    "shell",
];

/// The language a question names, and the question without it.
pub fn language(q: &str) -> Option<(String, String)> {
    let l = q.to_lowercase();
    let words: Vec<&str> = l
        .split(|c: char| c.is_whitespace() || c == ',' || c == '?')
        .filter(|w| !w.is_empty())
        .collect();
    for (i, w) in words.iter().enumerate() {
        for (name, alts) in LANGUAGES {
            if !alts.contains(w) {
                continue;
            }
            let prev = if i > 0 { words[i - 1] } else { "" };
            let next = words.get(i + 1).copied().unwrap_or("");
            let near = matches!(prev, "in" | "written" | "language")
                || matches!(
                    next,
                    "repos"
                        | "repositories"
                        | "projects"
                        | "libraries"
                        | "tools"
                        | "code"
                        | "language"
                );
            if AMBIGUOUS.contains(w) && !near {
                continue;
            }
            let rest: Vec<&str> = words
                .iter()
                .enumerate()
                .filter(|(j, x)| *j != i && !(*j + 1 == i && matches!(**x, "in" | "written")))
                .map(|(_, x)| *x)
                .collect();
            return Some((name.to_string(), rest.join(" ")));
        }
    }
    None
}

/// A span of time a question names: "in 2019", "since 2020", "this
/// year", "last month", "this week". Seconds, from and to.
pub fn period(q: &str, latest: i64) -> Option<(i64, i64, String)> {
    static YEAR_RE: OnceLock<Regex> = OnceLock::new();
    let l = q.to_lowercase();
    let re = YEAR_RE.get_or_init(|| {
        Regex::new(r"\b(in|since|during|from|of)?\s*(20[01][0-9]|202[0-9])\b").unwrap()
    });
    let end = latest + DAY;
    if let Some(c) = re.captures(&l) {
        let y: i32 = c[2].parse().ok()?;
        let from = Utc.with_ymd_and_hms(y, 1, 1, 0, 0, 0).single()?.timestamp();
        let to = Utc
            .with_ymd_and_hms(y + 1, 1, 1, 0, 0, 0)
            .single()?
            .timestamp();
        return Some(
            if c.get(1)
                .is_some_and(|m| m.as_str() == "since" || m.as_str() == "from")
            {
                (from, end, format!("since {y}"))
            } else {
                (from, to.min(end), format!("in {y}"))
            },
        );
    }
    let now = Utc.timestamp_opt(latest, 0).single()?;
    let jan1 = Utc
        .with_ymd_and_hms(now.year(), 1, 1, 0, 0, 0)
        .single()?
        .timestamp();
    if l.contains("this year") {
        return Some((jan1, end, format!("in {}", now.year())));
    }
    if l.contains("last year") || l.contains("past year") || l.contains("last 12 months") {
        return Some((latest - 365 * DAY, end, "in the last 12 months".into()));
    }
    if l.contains("this month")
        || l.contains("last month")
        || l.contains("past month")
        || l.contains("last 30 days")
    {
        return Some((latest - 30 * DAY, end, "in the last 30 days".into()));
    }
    if l.contains("this week")
        || l.contains("last week")
        || l.contains("past week")
        || l.contains("last 7 days")
    {
        return Some((latest - 7 * DAY, end, "in the last 7 days".into()));
    }
    if l.contains("today") || l.contains("yesterday") {
        return Some((latest - DAY, end, "on the last day".into()));
    }
    None
}

/// A repo a question names, "owner/name".
pub fn repo_name(q: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?:github\.com/)?\b([A-Za-z0-9][A-Za-z0-9-]{0,38})/([A-Za-z0-9._-]{1,100})\b")
            .unwrap()
    });
    re.captures(q)
        .map(|c| format!("{}/{}", &c[1], c[2].trim_end_matches(['.', ','])))
}

fn number(q: &str) -> Option<usize> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b(?:top|best|first)\s+(\d{1,3})\b|\b(\d{1,3})\s+(?:most|top|biggest|fastest|new|repos|projects)").unwrap());
    re.captures(&q.to_lowercase())
        .and_then(|c| c.get(1).or(c.get(2)))
        .and_then(|m| m.as_str().parse().ok())
        .map(|n: usize| n.clamp(1, 100))
}

fn has(l: &str, words: &[&str]) -> bool {
    words.iter().any(|w| l.contains(w))
}

/// What a question asks for, before it is run.
#[derive(Debug, PartialEq)]
pub enum Kind {
    Open(String),
    Person(String),
    TopRepos,
    Rising,
    NewRepos,
    Busiest,
    Contributors(String),
    RepoCount(String, &'static str),
    Alike(String),
    Languages,
    Topics,
    PerYear(&'static str),
    BusiestDays,
    Search,
}

/// Which kind of question it is.
pub fn kind(q: &str) -> Kind {
    let l = q.to_lowercase();
    let repo = repo_name(q);
    if let Some(r) = &repo {
        if has(
            &l,
            &["who ", "contributor", "maintainer", "people", "committers"],
        ) {
            return Kind::Contributors(r.clone());
        }
        if has(
            &l,
            &["like ", "alternative", "similar", "instead of", "replace"],
        ) {
            return Kind::Alike(r.clone());
        }
        for (w, col) in [
            ("star", "stars"),
            ("fork", "forks"),
            ("push", "pushes"),
            ("commit", "commits"),
            ("pull request", "prs"),
            ("pr", "prs"),
            ("issue", "issues"),
            ("release", "releases"),
            ("comment", "comments"),
        ] {
            if has(&l, &["how many", "number of", "count"]) && l.contains(w) {
                return Kind::RepoCount(r.clone(), col);
            }
        }
        if q.split_whitespace().count() <= 2 {
            return Kind::Open(r.clone());
        }
    }
    static WHO: OnceLock<Regex> = OnceLock::new();
    let who = WHO.get_or_init(|| {
        Regex::new(r"(?i)^(?:what (?:did|does|has)|who is)\s+@?([A-Za-z0-9-]{1,39})\b").unwrap()
    });
    if let Some(c) = who.captures(q) {
        return Kind::Person(c[1].to_string());
    }
    if let Some(r) = repo {
        return Kind::Alike(r);
    }
    if has(
        &l,
        &[
            "busiest day",
            "most stars in a day",
            "biggest days",
            "busiest days",
        ],
    ) {
        return Kind::BusiestDays;
    }
    if has(
        &l,
        &[
            "per year",
            "each year",
            "by year",
            "a year",
            "over the years",
            "every year",
        ],
    ) && !has(&l, &["language", "topic"])
    {
        let what = if l.contains("fork") {
            "forks"
        } else if l.contains("release") {
            "releases"
        } else if l.contains("push") || l.contains("commit") {
            "pushes"
        } else if l.contains("pull") || l.contains(" pr") {
            "prs"
        } else if l.contains("repo") && !l.contains("star") {
            "repos"
        } else {
            "stars"
        };
        return Kind::PerYear(what);
    }
    if l.contains("language") && !has(&l, &["repos", "projects"]) {
        return Kind::Languages;
    }
    if l.contains("topic") && !has(&l, &["repos", "projects"]) {
        return Kind::Topics;
    }
    if l.starts_with("new ")
        || has(
            &l,
            &[
                " new ",
                "newest",
                "created in",
                "created this",
                "started in",
                "launched",
            ],
        )
    {
        return Kind::NewRepos;
    }
    if has(
        &l,
        &[
            "fastest growing",
            "fastest-growing",
            "rising",
            "trending",
            "hottest",
            "grew",
            "growing",
            "gained",
            "most stars this",
            "most stars last",
            "most stars in the last",
        ],
    ) {
        return Kind::Rising;
    }
    if has(
        &l,
        &[
            "most active",
            "busiest repos",
            "busiest projects",
            "most commits",
            "most pushes",
            "most contributors",
        ],
    ) {
        return Kind::Busiest;
    }
    if has(
        &l,
        &[
            "most starred",
            "most popular",
            "top ",
            "biggest",
            "best ",
            "most stars",
        ],
    ) {
        return Kind::TopRepos;
    }
    Kind::Search
}

/// Topic words left after the language, period and question words are
/// taken out: "most starred rust repos for game engines" gives "game
/// engines".
fn subject(q: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b(what|which|are|is|the|most|starred|popular|top|best|biggest|fastest|growing|fastest-growing|rising|trending|hottest|new|newest|repos|repositories|projects|libraries|tools|with|stars|star|in|since|during|this|last|past|year|years|month|week|today|for|about|on|of|a|an|that|were|was|created|started|launched|written|language|20[0-2][0-9]|\d+|show|me|list|give|active|busiest|commits|pushes|contributors|gained|grew|by|most|ever|github|do|does|did|and|to|vs|than|more|less|per|each|over)\b").unwrap()
    });
    let s = re.replace_all(q, " ");
    s.split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

/// The question answered: what it was understood as, the rows, how the
/// page should draw them, and the SQL (in the trace).
pub async fn ask(c: &Ctx<'_>, q: &str, latest: i64) -> Result<Value> {
    let k = kind(q);
    let lang = language(q);
    let lang_name = lang.as_ref().map(|l| l.0.clone());
    let rest = lang
        .as_ref()
        .map(|l| l.1.clone())
        .unwrap_or_else(|| q.to_string());
    let per = period(q, latest);
    let n = number(q).unwrap_or(20);
    let topic = subject(&rest);
    let mut understood = Vec::new();
    let lang_cond = |p: &mut Value| -> String {
        match &lang_name {
            Some(l) => {
                p["lang"] = json!(l);
                " AND language = :lang".into()
            }
            None => String::new(),
        }
    };
    let topic_cond = |p: &mut Value| -> String {
        if topic.is_empty() {
            return String::new();
        }
        p["t"] = json!(topic);
        " AND (MATCH(topics, :t) OR MATCH(description, :t, min => '100%') OR MATCH(title, :t))"
            .into()
    };
    let out = match &k {
        Kind::Open(r) => return Ok(json!({"q": q, "kind": "open", "open": {"repo": r}})),
        Kind::Person(l) => return Ok(json!({"q": q, "kind": "person", "open": {"person": l}})),
        Kind::Search => return Ok(json!({"q": q, "kind": "search", "open": {"search": q}})),
        Kind::TopRepos | Kind::NewRepos => {
            let mut p = json!({});
            let mut cond = lang_cond(&mut p) + &topic_cond(&mut p);
            let new = k == Kind::NewRepos;
            let (from, to, said) = per.clone().unwrap_or(if new {
                (
                    latest - 30 * DAY,
                    latest + DAY,
                    "in the last 30 days".into(),
                )
            } else {
                (0, 0, String::new())
            });
            if from > 0 {
                cond += " AND first_seen >= :from AND first_seen < :to";
                p["from"] = json!(from);
                p["to"] = json!(to);
            }
            understood.push(format!(
                "the {n} {}most starred {}repos{}{}",
                if new { "new, " } else { "" },
                lang_name
                    .as_ref()
                    .map(|l| format!("{l} "))
                    .unwrap_or_default(),
                if topic.is_empty() {
                    String::new()
                } else {
                    format!(" about “{topic}”")
                },
                if from > 0 {
                    format!(", first seen {said}")
                } else {
                    String::new()
                }
            ));
            let rows = c.rows("the repos", format!("SELECT {REPO_COLS} FROM gh_repos WHERE stars > 0{cond} ORDER BY stars DESC LIMIT {n}"), p).await?;
            json!({"chart": "repos", "rows": rows})
        }
        Kind::Rising | Kind::Busiest => {
            let (from, to, said) = per.clone().unwrap_or((
                latest - 30 * DAY,
                latest + DAY,
                "in the last 30 days".into(),
            ));
            let busy = k == Kind::Busiest;
            let col = if busy { "commits" } else { "stars" };
            let filtered = lang_name.is_some() || !topic.is_empty();
            // Months for a year or more; days for less.
            let long = to - from > 100 * DAY;
            let sql = if long {
                format!("SELECT repo, SUM({col}) AS n FROM gh_repo_months WHERE month >= :from AND month < :to GROUP BY repo ORDER BY n DESC LIMIT {}", if filtered { 5000 } else { n })
            } else if busy {
                format!("SELECT repo, COUNT(*) AS n FROM gh_events WHERE t >= :from AND t < :to AND kind = 'push' GROUP BY repo ORDER BY n DESC LIMIT {}", if filtered { 5000 } else { n })
            } else {
                format!("SELECT repo, SUM(stars) AS n FROM gh_stars_daily WHERE day >= :from AND day < :to GROUP BY repo ORDER BY n DESC LIMIT {}", if filtered { 5000 } else { n })
            };
            let from_m = if long {
                pages::months_back(from + 15 * DAY, 0)
            } else {
                from
            };
            let top = c
                .rows(
                    if busy {
                        "the busiest repos"
                    } else {
                        "stars gained, by repo"
                    },
                    &sql,
                    json!({"from": from_m, "to": to}),
                )
                .await?;
            let ids = pages::ids(&top, "repo");
            let mut p = json!({"ids": ids});
            let cond = lang_cond(&mut p) + &topic_cond(&mut p);
            let info = if ids.is_empty() {
                Vec::new()
            } else {
                c.rows(
                    "those repos",
                    format!(
                        "SELECT {REPO_COLS} FROM gh_repos WHERE repo IN (:ids){cond} LIMIT 10000"
                    ),
                    p,
                )
                .await?
            };
            let info: std::collections::HashMap<i64, crate::strata::Row> =
                info.into_iter().map(|r| (r.i("repo"), r)).collect();
            let rows: Vec<Value> = top
                .iter()
                .filter_map(|t| {
                    let mut v = Value::Object(info.get(&t.i("repo"))?.clone());
                    v["gained"] = json!(t.i("n"));
                    Some(v)
                })
                .take(n)
                .collect();
            understood.push(format!(
                "the {n} {}repos{}{} with the most {} {said}",
                lang_name
                    .as_ref()
                    .map(|l| format!("{l} "))
                    .unwrap_or_default(),
                if topic.is_empty() {
                    String::new()
                } else {
                    format!(" about “{topic}”")
                },
                "",
                if busy {
                    if long {
                        "commits"
                    } else {
                        "pushes"
                    }
                } else {
                    "new stars"
                }
            ));
            json!({"chart": "repos", "gained": if busy { "commits" } else { "stars" }, "rows": rows})
        }
        Kind::Contributors(r) => {
            let Some(id) = pages::find_repo(c, r).await else {
                return Ok(
                    json!({"q": q, "kind": "none", "error": format!("no repo {r} in the archive")}),
                );
            };
            let mut p = json!({"id": id});
            let mut cond = String::new();
            if let Some((from, to, said)) = &per {
                cond = " AND t >= :from AND t < :to".into();
                p["from"] = json!(from);
                p["to"] = json!(to);
                understood.push(format!("who did the most work on {r} {said}"));
            } else {
                understood.push(format!("who did the most work on {r} (pushes, pull requests, issues, reviews, comments, releases)"));
            }
            let rows = c.rows("the work, by person", format!("SELECT actor, COUNT(*) AS n, MIN(t) AS first, MAX(t) AS last FROM gh_events WHERE repo = :id AND kind IN {}{cond} GROUP BY actor ORDER BY n DESC LIMIT {n}", pages::WORK), p).await?;
            let who = pages::logins(c, &pages::ids(&rows, "actor")).await;
            let rows: Vec<Value> = rows
                .iter()
                .filter(|x| !who.get(&x.i("actor")).is_some_and(|l| l.ends_with("[bot]")))
                .map(|x| {
                    let mut v = Value::Object(x.clone());
                    v["login"] = json!(who.get(&x.i("actor")).cloned().unwrap_or_default());
                    v
                })
                .collect();
            json!({"chart": "people", "rows": rows, "repo": r})
        }
        Kind::RepoCount(r, col) => {
            let Some(id) = pages::find_repo(c, r).await else {
                return Ok(
                    json!({"q": q, "kind": "none", "error": format!("no repo {r} in the archive")}),
                );
            };
            let mut p = json!({"id": id});
            let mut cond = String::new();
            let said = if let Some((from, to, said)) = &per {
                cond = " AND month >= :from AND month < :to".into();
                p["from"] = json!(pages::months_back(from + 15 * DAY, 0));
                p["to"] = json!(to);
                said.clone()
            } else {
                "ever".into()
            };
            understood.push(format!("the {col} {r} got {said}, year by year"));
            let rows = c.rows("its months", format!("SELECT month, {col} AS n FROM gh_repo_months WHERE repo = :id{cond} ORDER BY month LIMIT 400"), p).await?;
            let total: i64 = rows.iter().map(|x| x.i("n")).sum();
            let mut years: std::collections::BTreeMap<i64, i64> = Default::default();
            for x in &rows {
                *years
                    .entry(1970 + (x.i("month") + 15 * DAY) / YEAR)
                    .or_default() += x.i("n");
            }
            json!({"chart": "years", "total": total, "what": col, "rows": years.into_iter().map(|(y, n)| json!({"year": y, "n": n})).collect::<Vec<_>>(), "months": rows})
        }
        Kind::Alike(r) => {
            let Some(id) = pages::find_repo(c, r).await else {
                return Ok(
                    json!({"q": q, "kind": "none", "error": format!("no repo {r} in the archive")}),
                );
            };
            return Ok(
                json!({"q": q, "kind": "open", "open": {"repo": r, "id": id, "tab": "alternatives"}}),
            );
        }
        Kind::Languages | Kind::Topics => {
            let (table, col) = if k == Kind::Topics {
                ("gh_topic_months", "topic")
            } else {
                ("gh_lang_months", "language")
            };
            let (from, to, said) = per.clone().unwrap_or((
                pages::months_back(latest, 12),
                latest + DAY,
                "in the last 12 months".into(),
            ));
            let what = if has(&q.to_lowercase(), &["repos", "most used", "used"]) {
                "repos"
            } else {
                "stars"
            };
            understood.push(format!("{}s by the {what} their repos got {said}", col));
            let rows = c.rows(format!("{col}s by {what}"), format!("SELECT {col} AS name, SUM({what}) AS n FROM {table} WHERE month >= :from AND month < :to GROUP BY name ORDER BY n DESC LIMIT {n}"), json!({"from": pages::months_back(from + 15 * DAY, 0), "to": to})).await?;
            json!({"chart": "bars", "rows": rows, "what": what})
        }
        Kind::PerYear(what) => {
            understood.push(format!(
                "{} on GitHub year by year{}",
                match *what {
                    "repos" => "repos that got stars".to_string(),
                    w => w.to_string(),
                },
                lang_name
                    .as_ref()
                    .map(|l| format!(", {l} repos"))
                    .unwrap_or_default()
            ));
            let rows = match &lang_name {
                Some(l) => c.rows("the months", format!("SELECT month, SUM({what}) AS n FROM gh_lang_months WHERE language = :lang GROUP BY month ORDER BY month LIMIT 400"), json!({"lang": l})).await?,
                None if *what == "stars" || *what == "forks" => c.rows("stars and forks a day, summed by year", format!("SELECT day - day % {YEAR} AS y, SUM({what}) AS n FROM gh_stars_daily GROUP BY y ORDER BY y LIMIT 100"), Value::Null).await?,
                None => c.rows("the months", format!("SELECT month, SUM({what}) AS n FROM gh_lang_months GROUP BY month ORDER BY month LIMIT 400"), Value::Null).await?,
            };
            let mut years: std::collections::BTreeMap<i64, i64> = Default::default();
            for x in &rows {
                let t = if x.contains_key("y") {
                    x.i("y") + 15 * DAY
                } else {
                    x.i("month") + 15 * DAY
                };
                *years.entry(1970 + t / YEAR).or_default() += x.i("n");
            }
            json!({"chart": "years", "what": what, "rows": years.into_iter().map(|(y, n)| json!({"year": y, "n": n})).collect::<Vec<_>>()})
        }
        Kind::BusiestDays => {
            let mut p = json!({});
            let mut cond = String::new();
            if let Some((from, to, said)) = &per {
                cond = " WHERE day >= :from AND day < :to".into();
                p["from"] = json!(from);
                p["to"] = json!(to);
                understood.push(format!("the days with the most stars across GitHub {said}"));
            } else {
                understood.push("the days with the most stars across GitHub".into());
            }
            let rows = c.rows("stars a day, across every repo", format!("SELECT day, SUM(stars) AS n FROM gh_stars_daily{cond} GROUP BY day ORDER BY n DESC LIMIT {n}"), p).await?;
            json!({"chart": "days", "rows": rows})
        }
    };
    let mut out = out;
    out["q"] = json!(q);
    out["kind"] = json!(format!("{k:?}")
        .split('(')
        .next()
        .unwrap_or("")
        .to_lowercase());
    out["understood"] = json!(understood.join("; "));
    out["language"] = json!(lang_name);
    out["topic"] = json!(topic);
    Ok(out)
}

/// Questions the page offers as examples, each answered by a pattern.
pub const EXAMPLES: &[&str] = &[
    "fastest growing rust repos this year",
    "most starred python repos for web scraping",
    "who works on rust-lang/rust",
    "how many stars did ollama/ollama get in 2024",
    "new typescript repos with the most stars this month",
    "languages by stars in 2015",
    "stars per year",
    "busiest days",
    "alternatives to elastic/elasticsearch",
    "topics by stars this year",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind("fastest growing rust repos this year"), Kind::Rising);
        assert_eq!(
            kind("who works on rust-lang/rust"),
            Kind::Contributors("rust-lang/rust".into())
        );
        assert_eq!(
            kind("how many stars did ollama/ollama get in 2024"),
            Kind::RepoCount("ollama/ollama".into(), "stars")
        );
        assert_eq!(
            kind("alternatives to elastic/elasticsearch"),
            Kind::Alike("elastic/elasticsearch".into())
        );
        assert_eq!(kind("torvalds/linux"), Kind::Open("torvalds/linux".into()));
        assert_eq!(kind("languages by stars in 2015"), Kind::Languages);
        assert_eq!(kind("stars per year"), Kind::PerYear("stars"));
        assert_eq!(kind("busiest days"), Kind::BusiestDays);
        assert_eq!(
            kind("what did torvalds do"),
            Kind::Person("torvalds".into())
        );
        assert_eq!(
            kind("most starred python repos for web scraping"),
            Kind::TopRepos
        );
        assert_eq!(
            kind("new typescript repos with the most stars this month"),
            Kind::NewRepos
        );
        assert_eq!(kind("self hosted photo library"), Kind::Search);
    }

    #[test]
    fn languages() {
        assert_eq!(language("fastest growing rust repos").unwrap().0, "Rust");
        assert_eq!(language("top go repos").unwrap().0, "Go");
        assert_eq!(language("where do I go from here"), None);
        assert_eq!(language("written in c++ for games").unwrap().0, "C++");
        assert_eq!(language("a c library"), None);
    }

    #[test]
    fn subjects() {
        assert_eq!(
            subject(
                &language("most starred python repos for web scraping")
                    .unwrap()
                    .1
            ),
            "web scraping"
        );
        assert_eq!(subject("fastest growing repos this year"), "");
    }

    #[test]
    fn periods() {
        let latest = Utc
            .with_ymd_and_hms(2026, 11, 14, 0, 0, 0)
            .unwrap()
            .timestamp();
        let (f, t, s) = period("languages by stars in 2015", latest).unwrap();
        assert_eq!(s, "in 2015");
        assert_eq!(t - f, 365 * DAY);
        assert_eq!(period("since 2020", latest).unwrap().1, latest + DAY);
        assert!(period("stars per year", latest).is_none());
    }
}
