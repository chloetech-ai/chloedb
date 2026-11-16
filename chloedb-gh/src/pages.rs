//! What each page shows, in SQL over the cluster's tables:
//!
//! - `gh_events`: every public event since 2011 (`t` in seconds, `kind`,
//!   `repo`, `actor`, `n`), placed on shards by repo and sorted by it in
//!   each segment, so one repo's history is a short range on one shard.
//! - `gh_stars`: every star again (`repo`, `actor`, `actor_k`, `t`), the
//!   person also a keyword, whose list of rows answers "what did these
//!   people star" without reading every event.
//! - `gh_stars_daily`: stars and forks a repo got each day.
//! - `gh_repos`: every repo with two stars or more, its latest name and
//!   what events last said of it, searched by words and by meaning.
//! - `gh_vec`: the repos of `gh_repos` that have a vector (ten stars or
//!   more), with only their stars and language beside it, in a cluster of
//!   its own when the app is given one (`--vec`): there the vectors stay
//!   in memory, where next to the events they are read from the disk.
//! - `gh_repo_names`, `gh_actors`: old names, and people's public logins.
//! - `gh_repo_months`, `gh_lang_months`, `gh_topic_months`: what happened
//!   each month, per repo, language and topic.
//!
//! Every query is kept in the request's trace, which the page shows.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::strata::{Result, Row, RowExt, Strata, Trace};

pub const DAY: i64 = 86_400;
/// A year as `t - t % YEAR` buckets it (365.2425 days).
pub const YEAR: i64 = 31_556_952;

/// The columns of a repo as the page lists it.
pub const REPO_COLS: &str = "repo, name, owner, description, language, topics, license, homepage, stars, forks, stars_api, forks_api, created, first_seen, archived, fork";

/// The people whose stars say what else a repo's stargazers like (its
/// latest), and of the repos a person starred for their "for you". Each one's stars
/// are rows all over the stars table, sorted by repo: on 8 GB machines
/// over 524 million stars, 1,500 people took past the server's 5 s, 400
/// take about a second from the disk and tens of milliseconds once read.
pub const COSTAR_PEOPLE: usize = 400;
/// How long those statements may take.
pub const COSTAR_MS: u64 = 15_000;

/// What people do in a repo, as opposed to starring or forking it.
pub const WORK: &str = "('push', 'pr_opened', 'pr_merged', 'pr_closed', 'issue_opened', 'issue_closed', 'comment', 'review', 'review_comment', 'commit_comment', 'release', 'create_branch', 'create_tag')";

pub struct Ctx<'a> {
    pub db: &'a Strata,
    /// The cluster of `gh_vec`, if there is one.
    pub vec: Option<&'a Strata>,
    pub trace: &'a Trace,
}

/// A search by meaning: the repos nearest `text`, ranked by
/// `similarity * (stars + 1)^boost`, among those `cond` keeps (a condition
/// on `repo`, `stars` or `language`, the columns both tables have).
pub struct Near<'s> {
    pub text: &'s str,
    pub cond: &'s str,
    pub boost: f64,
    pub limit: usize,
}

impl Ctx<'_> {
    /// The repos nearest in meaning, as `repo` and `sim`: from `gh_vec`
    /// when there is a vector cluster, else from `gh_repos` (whose repos
    /// with a vector are those with ten stars or more). `params` are the
    /// condition's.
    pub async fn near(&self, what: &str, n: &Near<'_>, mut params: Value) -> Vec<Row> {
        let (db, table, stars) = match self.vec {
            Some(v) => (v, "gh_vec", ""),
            None => (self.db, "gh_repos", "stars >= 10"),
        };
        let cond = [stars, n.cond]
            .into_iter()
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>()
            .join(" AND ");
        let filter = if cond.is_empty() { String::new() } else { format!(" WHERE {cond}") };
        let sql = format!(
            "SELECT repo, similarity(emb, :v) AS sim FROM {table}{filter} ORDER BY similarity(emb, :v) * pow(stars + 1, {}) DESC LIMIT {}",
            n.boost, n.limit
        );
        params["v"] = json!({"embed": n.text, "field": "emb", "table": table});
        db.traced(self.trace, what, &sql, params).await.unwrap_or_default()
    }

    /// How close in meaning to `text` each of `ids` is, by repo (none for
    /// a repo without a vector).
    pub async fn closeness(&self, what: &str, text: &str, ids: &[i64]) -> HashMap<i64, f64> {
        if ids.is_empty() {
            return HashMap::new();
        }
        let (db, table) = match self.vec {
            Some(v) => (v, "gh_vec"),
            None => (self.db, "gh_repos"),
        };
        let sql = format!("SELECT repo, similarity(emb, :v) AS sim FROM {table} WHERE repo IN (:ids)");
        let p = json!({"ids": ids, "v": {"embed": text, "field": "emb", "table": table}});
        db.traced(self.trace, what, &sql, p)
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.get("sim").is_some_and(|v| !v.is_null()))
            .map(|r| (r.i("repo"), r.f("sim")))
            .collect()
    }

    pub async fn rows(
        &self,
        what: impl AsRef<str>,
        sql: impl AsRef<str>,
        params: Value,
    ) -> Result<Vec<Row>> {
        self.db
            .traced(self.trace, what.as_ref(), sql.as_ref(), params)
            .await
    }
    /// As [`Ctx::soft`], given `ms` milliseconds rather than the server's
    /// default deadline: for a statement that reads rows of many people,
    /// which come from all over the stars table.
    pub async fn soft_within(
        &self,
        what: impl AsRef<str>,
        sql: impl AsRef<str>,
        params: Value,
        ms: u64,
    ) -> Vec<Row> {
        self.db
            .traced_within(self.trace, what.as_ref(), sql.as_ref(), params, Some(ms))
            .await
            .unwrap_or_default()
    }

    /// The rows, or none when the query fails (the trace says why).
    pub async fn soft(
        &self,
        what: impl AsRef<str>,
        sql: impl AsRef<str>,
        params: Value,
    ) -> Vec<Row> {
        self.rows(what, sql, params).await.unwrap_or_default()
    }
}

pub fn ids(rows: &[Row], col: &str) -> Vec<i64> {
    let mut seen = HashSet::new();
    rows.iter()
        .map(|r| r.i(col))
        .filter(|i| *i != 0 && seen.insert(*i))
        .collect()
}

fn by_id(rows: Vec<Row>, col: &str) -> HashMap<i64, Row> {
    rows.into_iter().map(|r| (r.i(col), r)).collect()
}

fn obj(r: &Row) -> Value {
    Value::Object(r.clone())
}

/// Logins of actors, by id.
pub async fn logins(c: &Ctx<'_>, actors: &[i64]) -> HashMap<i64, String> {
    if actors.is_empty() {
        return HashMap::new();
    }
    c.soft(
        "their logins",
        "SELECT actor, login FROM gh_actors WHERE actor IN (:a)",
        json!({"a": actors}),
    )
    .await
    .into_iter()
    .map(|r| (r.i("actor"), r.s("login")))
    .collect()
}

/// Repos by id, as the page lists them.
/// People's ids as gh_stars' keyword values (an `IN` there reads each
/// value's own rows).
pub fn keys(ids: &[i64]) -> Vec<String> {
    ids.iter().map(i64::to_string).collect()
}

pub async fn repos_of(c: &Ctx<'_>, what: &str, ids: &[i64]) -> HashMap<i64, Row> {
    if ids.is_empty() {
        return HashMap::new();
    }
    by_id(
        c.soft(
            what,
            format!("SELECT {REPO_COLS} FROM gh_repos WHERE repo IN (:ids)"),
            json!({"ids": ids}),
        )
        .await,
        "repo",
    )
}

/// Each repo's stars a month since `since`, as `{repo: [[month, stars]]}`.
pub async fn sparks(c: &Ctx<'_>, ids: &[i64], since: i64) -> Map<String, Value> {
    let mut out = Map::new();
    if ids.is_empty() {
        return out;
    }
    let rows = c.soft("their stars a month", "SELECT repo, month, stars FROM gh_repo_months WHERE repo IN (:ids) AND month >= :since ORDER BY repo, month LIMIT 10000", json!({"ids": ids, "since": since})).await;
    for r in rows {
        let e = out
            .entry(r.i("repo").to_string())
            .or_insert_with(|| json!([]));
        e.as_array_mut()
            .unwrap()
            .push(json!([r.i("month"), r.i("stars")]));
    }
    out
}

/// The first second of the month `n` months before the one of `t`.
pub fn months_back(t: i64, n: i64) -> i64 {
    use chrono::{Datelike, TimeZone, Utc};
    let d = Utc.timestamp_opt(t, 0).single().unwrap_or_default();
    let m = d.year() as i64 * 12 + d.month0() as i64 - n;
    Utc.with_ymd_and_hms((m / 12) as i32, (m % 12) as u32 + 1, 1, 0, 0, 0)
        .single()
        .map_or(0, |x| x.timestamp())
}

// ------------------------------------------------------------- search

pub struct Search {
    pub q: String,
    pub language: String,
    pub sort: String,
    pub page: usize,
}

const TEXT_RANK: &str = "(1.6 * coalesce(bm25(title, :q), 0) + coalesce(bm25(description, :q), 0) + 0.8 * coalesce(bm25(topics, :q), 0))";
const MATCHED: &str = "(MATCH(title, :q, min => :min) OR MATCH(description, :q, min => :min) OR MATCH(topics, :q, min => :min))";

/// Repos by what they do: by their words (name, description, topics) and
/// their meaning (a vector of the same), ranked with their stars, with the
/// languages of every match and each result's stars month by month.
pub async fn search(c: &Ctx<'_>, s: &Search, latest: i64) -> Result<Value> {
    let q = s.q.trim();
    let mut cond = String::new();
    let mut params =
        json!({"q": q, "min": "60%", "v": {"embed": q, "field": "emb", "table": "gh_repos"}});
    if !s.language.is_empty() {
        cond = " AND language = :lang".into();
        params["lang"] = json!(s.language);
    }
    let limit = 30;
    let near = Near { text: q, cond: if s.language.is_empty() { "" } else { "language = :lang" }, boost: 0.12, limit: 120 };
    let (words, meaning, langs, total) = tokio::join!(
        c.soft("repos with the words", format!("SELECT repo FROM gh_repos WHERE {MATCHED}{cond} ORDER BY {TEXT_RANK} * pow(stars + 1, 0.35) DESC LIMIT 200"), params.clone()),
        c.near("repos by meaning", &near, params.clone()),
        c.soft("the languages of the matches", format!("SELECT language, COUNT(*) AS n FROM gh_repos WHERE {MATCHED} GROUP BY language ORDER BY n DESC LIMIT 14"), params.clone()),
        c.soft("how many repos have the words", format!("SELECT COUNT(*) AS n FROM gh_repos WHERE {MATCHED}{cond}"), params.clone()),
    );
    let mut cand = ids(&words, "repo");
    cand.extend(ids(&meaning, "repo"));
    let cand: Vec<i64> = cand
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let order = match s.sort.as_str() {
        "stars" => "stars DESC".to_string(),
        "new" => "first_seen DESC".to_string(),
        _ => format!("(0.6 * {TEXT_RANK} + 18.0 * coalesce(similarity(emb, :v), 0)) * pow(stars + 1, 0.15) DESC"),
    };
    let mut p = params.clone();
    p["ids"] = json!(cand);
    let offset = s.page * limit;
    let rows = if cand.is_empty() {
        Vec::new()
    } else {
        c.rows("the repos, ranked", format!("SELECT {REPO_COLS} FROM gh_repos WHERE repo IN (:ids) ORDER BY {order} LIMIT {limit} OFFSET {offset}"), p).await?
    };
    let rids = ids(&rows, "repo");
    let spark = sparks(c, &rids, months_back(latest, 12)).await;
    let mut out: Vec<Value> = rows.iter().map(obj).collect();
    if s.sort == "rising" {
        // Stars in the last three months.
        let recent = |id: i64| {
            spark
                .get(&id.to_string())
                .and_then(Value::as_array)
                .map_or(0, |a| {
                    a.iter()
                        .rev()
                        .take(3)
                        .map(|x| x[1].as_i64().unwrap_or(0))
                        .sum::<i64>()
                })
        };
        out.sort_by_key(|r| -recent(r["repo"].as_i64().unwrap_or(0)));
    }
    Ok(json!({
        "q": q, "language": s.language, "sort": s.sort, "page": s.page,
        "total": total.first().map(|r| r.i("n")).unwrap_or(0),
        "candidates": {"words": words.len(), "meaning": meaning.len()},
        "languages": langs.iter().filter(|r| !r.s("language").is_empty()).map(obj).collect::<Vec<_>>(),
        "repos": out, "spark": spark,
    }))
}

// --------------------------------------------------------------- repo

/// A repo's id by its name, now or before ("owner/name", any case).
pub async fn find_repo(c: &Ctx<'_>, name: &str) -> Option<i64> {
    let l = name.trim().trim_matches('/').to_lowercase();
    if let Some(id) = l.strip_prefix("id:").and_then(|x| x.parse().ok()) {
        return Some(id);
    }
    let now = c
        .soft(
            "the repo by its name",
            "SELECT repo FROM gh_repos WHERE lname = :n LIMIT 1",
            json!({"n": l}),
        )
        .await;
    if let Some(r) = now.first() {
        return Some(r.i("repo"));
    }
    let before = c.soft("a repo that had that name", "SELECT repo, first_seen FROM gh_repo_names WHERE lname = :n ORDER BY first_seen DESC LIMIT 1", json!({"n": l})).await;
    before.first().map(|r| r.i("repo"))
}

/// A repo: what it is, its stars and work month by month, its last
/// months day by day, who keeps it going, how alive it is, what does the
/// same job and what the same people star.
pub async fn repo(c: &Ctx<'_>, id: i64, latest: i64) -> Result<Value> {
    let p = json!({"id": id});
    let year_ago = latest - 365 * DAY;
    let (info, months, days, last, names) = tokio::join!(
        c.rows("the repo", format!("SELECT {REPO_COLS} FROM gh_repos WHERE repo = :id"), p.clone()),
        c.soft("its months", "SELECT month, stars, forks, pushes, commits, prs, merged, issues, comments, releases, people FROM gh_repo_months WHERE repo = :id ORDER BY month LIMIT 400", p.clone()),
        c.soft("its last 120 days", "SELECT day, stars, forks FROM gh_stars_daily WHERE repo = :id AND day > :since ORDER BY day LIMIT 200", json!({"id": id, "since": latest - 120 * DAY})),
        c.soft("its last event", format!("SELECT MAX(t) AS last, MIN(t) AS first, COUNT(*) AS n FROM gh_events WHERE repo = :id AND kind IN {WORK}"), p.clone()),
        c.soft("the names it had", "SELECT name, first_seen FROM gh_repo_names WHERE repo = :id ORDER BY first_seen LIMIT 20", p.clone()),
    );
    let info = info?;
    let Some(r) = info.into_iter().next() else {
        return Ok(json!({"found": false, "id": id}));
    };
    let (recent, ever, gazers) = tokio::join!(
        c.soft("who did the work this last year", format!("SELECT actor, COUNT(*) AS n FROM gh_events WHERE repo = :id AND t > :since AND kind IN {WORK} GROUP BY actor ORDER BY n DESC LIMIT 30"), json!({"id": id, "since": year_ago})),
        c.soft("who did the work ever", format!("SELECT actor, COUNT(*) AS n, MIN(t) AS first, MAX(t) AS last FROM gh_events WHERE repo = :id AND kind IN {WORK} GROUP BY actor ORDER BY n DESC LIMIT 30"), p.clone()),
        c.soft("its latest stargazers", format!("SELECT actor FROM gh_stars WHERE repo = :id ORDER BY t DESC LIMIT {COSTAR_PEOPLE}"), p.clone()),
    );
    let mut people: Vec<i64> = ids(&recent, "actor");
    people.extend(ids(&ever, "actor"));
    let text = format!(
        "{} {} {}",
        r.s("name").replace(['/', '-', '_'], " "),
        r.s("description"),
        r.s("topics").replace(',', " ")
    );
    let gz = ids(&gazers, "actor");
    let (who, alike, co) = tokio::join!(
        logins(c, &people),
        async {
            let near = c.near("repos that do the same job (by meaning)", &Near { text: &text, cond: "repo <> :id", boost: 0.1, limit: 40 }, json!({"id": id})).await;
            let info = repos_of(c, "those repos", &ids(&near, "repo")).await;
            near.iter()
                .filter_map(|n| {
                    let mut r = info.get(&n.i("repo"))?.clone();
                    r.insert("sim".into(), json!(n.f("sim")));
                    Some(r)
                })
                .collect::<Vec<Row>>()
        },
        async {
            if gz.len() < 20 {
                return Vec::new();
            }
            c.soft_within("what the same people starred", "SELECT repo, COUNT(*) AS n FROM gh_stars WHERE actor_k IN (:a) GROUP BY repo ORDER BY n DESC LIMIT 60", json!({"a": keys(&gz)}), COSTAR_MS).await
        },
    );
    let co_ids: Vec<i64> = ids(&co, "repo").into_iter().filter(|x| *x != id).collect();
    let co_info = repos_of(c, "those repos", &co_ids).await;
    // Lift: shared stargazers against how starred the other repo is.
    let mut costar: Vec<Value> = co
        .iter()
        .filter_map(|x| {
            let info = co_info.get(&x.i("repo"))?;
            let shared = x.i("n");
            let lift = shared as f64 / ((info.i("stars") + 50) as f64).sqrt();
            let mut v = obj(info);
            v["shared"] = json!(shared);
            v["lift"] = json!((lift * 100.0).round() / 100.0);
            Some(v)
        })
        .collect();
    costar.sort_by(|a, b| {
        b["lift"]
            .as_f64()
            .partial_cmp(&a["lift"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Alternatives: what does the same job by meaning, raised when the same
    // people starred it too (people star the alternatives they weigh), and
    // co-starred repos close enough in meaning.
    let lifts: HashMap<i64, f64> = costar
        .iter()
        .map(|v| {
            (
                v["repo"].as_i64().unwrap_or(0),
                v["lift"].as_f64().unwrap_or(0.0),
            )
        })
        .collect();
    let max_lift = lifts.values().cloned().fold(0.0, f64::max).max(1e-9);
    let alike_ids: HashSet<i64> = ids(&alike, "repo").into_iter().collect();
    let extra: Vec<i64> = lifts
        .keys()
        .copied()
        .filter(|x| !alike_ids.contains(x))
        .collect();
    let extra_sims = c
        .closeness("how close in meaning the co-starred repos are", &text, &extra)
        .await;
    let top_sim = alike.iter().map(|r| r.f("sim")).fold(0.0, f64::max);
    let mut alts: Vec<Value> = alike.iter().map(obj).collect();
    for e in &extra {
        if let Some(v) = costar.iter().find(|v| v["repo"].as_i64() == Some(*e)) {
            let mut v = v.clone();
            v["sim"] = json!(extra_sims.get(e).copied().unwrap_or(0.0));
            alts.push(v);
        }
    }
    let score = |v: &Value| {
        let sim = v["sim"].as_f64().unwrap_or(0.0);
        let lift = lifts
            .get(&v["repo"].as_i64().unwrap_or(0))
            .copied()
            .unwrap_or(0.0)
            / max_lift;
        sim * (1.0 + 0.8 * lift)
    };
    alts.retain(|v| v["sim"].as_f64().unwrap_or(0.0) >= top_sim * 0.72);
    for v in alts.iter_mut() {
        v["costarred"] = json!(lifts.contains_key(&v["repo"].as_i64().unwrap_or(0)));
    }
    alts.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    alts.truncate(12);
    costar.truncate(12);
    // Bots (logins ending in "[bot]") do much of the work in big repos:
    // who keeps a repo going, and its bus factor, are about people.
    let is_bot = |r: &Row| who.get(&r.i("actor")).is_some_and(|l| l.ends_with("[bot]"));
    let recent: Vec<Row> = recent.into_iter().filter(|r| !is_bot(r)).collect();
    let ever: Vec<Row> = ever.into_iter().filter(|r| !is_bot(r)).collect();
    let named = |rows: &[Row]| -> Vec<Value> {
        rows.iter()
            .map(|x| {
                let mut v = obj(x);
                v["login"] = json!(who.get(&x.i("actor")).cloned().unwrap_or_default());
                v
            })
            .collect()
    };
    let vitals = vitals(
        &months,
        &recent,
        &last,
        latest,
        r.get("archived").and_then(Value::as_bool).unwrap_or(false),
    );
    let spikes = spikes(&days, &months);
    Ok(json!({
        "found": true, "repo": obj(&r), "months": months.iter().map(obj).collect::<Vec<_>>(), "days": days.iter().map(obj).collect::<Vec<_>>(),
        "names": names.iter().map(obj).collect::<Vec<_>>(),
        "work": last.first().map(obj), "people_year": named(&recent), "people_ever": named(&ever),
        "alike": alts, "costarred": costar, "stargazers_sampled": gz.len(),
        "vitals": vitals, "spikes": spikes, "latest": latest,
    }))
}

/// How alive a repo is: this last year against the one before, how much
/// rests on one person, and a word for it.
fn vitals(months: &[Row], recent: &[Row], last: &[Row], latest: i64, archived: bool) -> Value {
    let y1 = months_back(latest, 12);
    let y2 = months_back(latest, 24);
    let sum = |from: i64, to: i64, col: &str| {
        months
            .iter()
            .filter(|m| m.i("month") >= from && m.i("month") < to)
            .map(|m| m.i(col))
            .sum::<i64>()
    };
    let cols = [
        "stars", "pushes", "commits", "prs", "merged", "issues", "comments", "releases",
    ];
    let now: Map<String, Value> = cols
        .iter()
        .map(|c| (c.to_string(), json!(sum(y1, i64::MAX, c))))
        .collect();
    let before: Map<String, Value> = cols
        .iter()
        .map(|c| (c.to_string(), json!(sum(y2, y1, c))))
        .collect();
    let work_now: i64 = ["pushes", "prs", "issues", "comments", "releases"]
        .iter()
        .map(|c| sum(y1, i64::MAX, c))
        .sum();
    let work_before: i64 = ["pushes", "prs", "issues", "comments", "releases"]
        .iter()
        .map(|c| sum(y2, y1, c))
        .sum();
    let total: i64 = recent.iter().map(|r| r.i("n")).sum();
    let top = recent.first().map(|r| r.i("n")).unwrap_or(0);
    // How many people do half the work.
    let mut acc = 0;
    let mut half = 0;
    for r in recent {
        if acc * 2 >= total {
            break;
        }
        acc += r.i("n");
        half += 1;
    }
    let last_t = last.first().map(|r| r.i("last")).unwrap_or(0);
    let quiet_days = if last_t > 0 {
        (latest - last_t) / DAY
    } else {
        -1
    };
    let word = if archived {
        "archived"
    } else if last_t == 0 || quiet_days > 365 {
        "dormant"
    } else if work_before > 20 && work_now * 10 < work_before * 4 {
        "slowing"
    } else if quiet_days > 120 {
        "quiet"
    } else {
        "active"
    };
    let merge_rate = if now["prs"].as_i64().unwrap_or(0) > 0 {
        now["merged"].as_f64().unwrap_or(0.0) / now["prs"].as_f64().unwrap_or(1.0)
    } else {
        0.0
    };
    json!({
        "word": word, "year": now, "year_before": before, "last_work": last_t, "quiet_days": quiet_days,
        "people_year": recent.len(), "top_share": if total > 0 { top as f64 / total as f64 } else { 0.0 }, "half_by": half,
        "merge_rate": (merge_rate * 100.0).round() / 100.0,
    })
}

/// The days (or months, before the last 120 days) a repo got far more
/// stars than it usually does.
fn spikes(days: &[Row], months: &[Row]) -> Value {
    let mut out = Vec::new();
    if days.len() >= 7 {
        let mut v: Vec<i64> = days.iter().map(|d| d.i("stars")).collect();
        v.sort_unstable();
        let median = v[v.len() / 2].max(1);
        let mut top: Vec<&Row> = days
            .iter()
            .filter(|d| d.i("stars") >= 20 && d.i("stars") >= median * 5)
            .collect();
        top.sort_by_key(|d| -d.i("stars"));
        for d in top.into_iter().take(4) {
            out.push(json!({"day": d.i("day"), "stars": d.i("stars"), "usual": median}));
        }
    }
    let mut ms: Vec<i64> = months.iter().map(|m| m.i("stars")).collect();
    ms.sort_unstable();
    let med = ms.get(ms.len() / 2).copied().unwrap_or(0).max(1);
    let mut big: Vec<&Row> = months
        .iter()
        .filter(|m| m.i("stars") >= 50 && m.i("stars") >= med * 4)
        .collect();
    big.sort_by_key(|m| -m.i("stars"));
    let months: Vec<Value> = big.into_iter().take(5).map(|m| json!({"month": m.i("month"), "stars": m.i("stars"), "usual": med, "releases": m.i("releases")})).collect();
    json!({"days": out, "months": months})
}

// ------------------------------------------------------------- people

/// A person or organisation: what they did, year by year, where, what
/// they starred lately, and the repos they own.
pub async fn person(c: &Ctx<'_>, login: &str) -> Result<Value> {
    let l = login.trim().to_lowercase();
    let (who, owned) = tokio::join!(
        c.soft(
            "the login",
            "SELECT actor, login, first_seen, last_seen FROM gh_actors WHERE llogin = :l LIMIT 1",
            json!({"l": l})
        ),
        c.soft(
            "the repos they own",
            format!(
                "SELECT {REPO_COLS} FROM gh_repos WHERE lowner = :l ORDER BY stars DESC LIMIT 24"
            ),
            json!({"l": l})
        ),
    );
    let Some(w) = who.first() else {
        return Ok(
            json!({"found": !owned.is_empty(), "login": login, "owned": owned.iter().map(obj).collect::<Vec<_>>()}),
        );
    };
    let a = w.i("actor");
    let p = json!({"a": a});
    let (kinds, years, repos, stars) = tokio::join!(
        c.soft("what they did", "SELECT kind, COUNT(*) AS n FROM gh_events WHERE actor = :a GROUP BY kind ORDER BY n DESC LIMIT 30", p.clone()),
        c.soft("year by year", format!("SELECT t - t % {YEAR} AS y, COUNT(*) AS n FROM gh_events WHERE actor = :a AND kind IN {WORK} GROUP BY y ORDER BY y LIMIT 40"), p.clone()),
        c.soft_within("where they work", format!("SELECT repo, COUNT(*) AS n, MIN(t) AS first, MAX(t) AS last FROM gh_events WHERE actor = :a AND kind IN {WORK} GROUP BY repo ORDER BY n DESC LIMIT 24"), p.clone(), COSTAR_MS),
        c.soft("what they starred lately", "SELECT repo, t FROM gh_stars WHERE actor_k = :k ORDER BY t DESC LIMIT 24", json!({"k": a.to_string()})),
    );
    let mut rids = ids(&repos, "repo");
    rids.extend(ids(&stars, "repo"));
    let info = repos_of(c, "those repos", &rids).await;
    let with = |rows: &[Row]| -> Vec<Value> {
        rows.iter()
            .map(|x| {
                let mut v = obj(x);
                if let Some(i) = info.get(&x.i("repo")) {
                    v["info"] = obj(i);
                }
                v
            })
            .collect()
    };
    Ok(json!({
        "found": true, "login": w.s("login"), "actor": a, "first_seen": w.i("first_seen"), "last_seen": w.i("last_seen"),
        "kinds": kinds.iter().map(obj).collect::<Vec<_>>(), "years": years.iter().map(|y| json!({"year": 1970 + y.i("y") / YEAR, "n": y.i("n")})).collect::<Vec<_>>(),
        "repos": with(&repos), "starred": with(&stars), "owned": owned.iter().map(obj).collect::<Vec<_>>(),
    }))
}

// ------------------------------------------------------------- rising

/// The repos whose stars jumped in the `days` days to `at` (a day), against
/// the four weeks before, with what may explain it.
pub async fn rising(c: &Ctx<'_>, at: i64, days: i64, hn: Option<&crate::api::Hn>) -> Result<Value> {
    rising_about(c, at, days, hn, None).await
}

/// The repos most starred in a window that have `topic` among their topics:
/// of the window's [`TOPIC_POOL`] most starred repos.
pub const TOPIC_POOL: usize = 5000;

/// Whether a repo's topics (as kept: `a,b,c`) hold `topic` itself, not
/// only its words (`machine-learning` is not `machine-learning-algorithms`).
pub fn has_topic(topics: &str, topic: &str) -> bool {
    topics.split(',').any(|t| t.trim().eq_ignore_ascii_case(topic))
}

/// As [`rising`], of the repos with `topic` when given.
pub async fn rising_about(c: &Ctx<'_>, at: i64, days: i64, hn: Option<&crate::api::Hn>, topic: Option<&str>) -> Result<Value> {
    let a0 = at - days * DAY;
    let b0 = a0 - 28 * DAY;
    let recent = match topic {
        None => c.rows("stars in the window, by repo", "SELECT repo, SUM(stars) AS s FROM gh_stars_daily WHERE day > :a0 AND day <= :a1 GROUP BY repo ORDER BY s DESC LIMIT 400", json!({"a0": a0, "a1": at})).await?,
        Some(t) => {
            let pool = c.rows("stars in the window, by repo", format!("SELECT repo, SUM(stars) AS s FROM gh_stars_daily WHERE day > :a0 AND day <= :a1 GROUP BY repo ORDER BY s DESC LIMIT {TOPIC_POOL}"), json!({"a0": a0, "a1": at})).await?;
            let about = c.rows("those with the topic", format!("SELECT repo, topics FROM gh_repos WHERE repo IN (:ids) AND MATCH(topics, :t) LIMIT {TOPIC_POOL}"), json!({"ids": ids(&pool, "repo"), "t": t})).await?;
            let keep: std::collections::HashSet<i64> = about.iter().filter(|r| has_topic(&r.s("topics"), t)).map(|r| r.i("repo")).collect();
            pool.into_iter().filter(|r| keep.contains(&r.i("repo"))).take(400).collect()
        }
    };
    let rids = ids(&recent, "repo");
    let (base, info, rel, daily) = tokio::join!(
        c.soft("their stars in the four weeks before", "SELECT repo, SUM(stars) AS b FROM gh_stars_daily WHERE repo IN (:ids) AND day > :b0 AND day <= :a0 GROUP BY repo", json!({"ids": rids, "a0": a0, "b0": b0})),
        repos_of(c, "the repos", &rids),
        c.soft("their releases in the window", "SELECT repo, COUNT(*) AS n FROM gh_events WHERE repo IN (:ids) AND kind = 'release' AND t > :a0 AND t <= :a1 GROUP BY repo", json!({"ids": rids, "a0": a0, "a1": at + DAY})),
        c.soft("their stars day by day", "SELECT repo, day, stars FROM gh_stars_daily WHERE repo IN (:ids) AND day > :b0 AND day <= :a1 ORDER BY repo, day LIMIT 20000", json!({"ids": rids, "b0": b0, "a1": at})),
    );
    let base: HashMap<i64, i64> = base.iter().map(|r| (r.i("repo"), r.i("b"))).collect();
    let rel: HashMap<i64, i64> = rel.iter().map(|r| (r.i("repo"), r.i("n"))).collect();
    let mut by_day: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
    for d in &daily {
        by_day
            .entry(d.i("repo"))
            .or_default()
            .push((d.i("day"), d.i("stars")));
    }
    let mut out: Vec<Value> = recent
        .iter()
        .filter_map(|r| {
            let id = r.i("repo");
            let i = info.get(&id)?;
            let s = r.i("s");
            let b = base.get(&id).copied().unwrap_or(0);
            let expected = b as f64 * days as f64 / 28.0;
            let spike = s as f64 / (expected + 3.0);
            let mut why = Vec::new();
            let first = i.i("first_seen");
            if first > b0 {
                let d = ((at - first) / DAY).max(0);
                why.push(json!({"kind": "new", "text": match d { 0 => "new: first seen that day".to_string(), 1 => "new: first seen the day before".to_string(), d => format!("new: first seen {d} days before") }}));
            }
            if let Some(n) = rel.get(&id) {
                why.push(json!({"kind": "release", "text": if *n == 1 { "a release in the window".to_string() } else { format!("{n} releases in the window") }}));
            }
            let series = by_day.get(&id).cloned().unwrap_or_default();
            if let Some((d, n)) = series.iter().filter(|x| x.0 > a0).max_by_key(|x| x.1) {
                if *n as f64 >= s as f64 * 0.4 && s >= 30 {
                    why.push(json!({"kind": "day", "day": d, "text": format!("{} of its {} stars came in one day", n, s)}));
                }
            }
            let mut v = obj(i);
            v["window"] = json!(s);
            v["before"] = json!(b);
            v["spike"] = json!((spike * 10.0).round() / 10.0);
            v["why"] = json!(why);
            v["daily"] = json!(series);
            Some(v)
        })
        .filter(|v| v["window"].as_i64().unwrap_or(0) >= 10)
        .collect();
    out.sort_by(|a, b| {
        b["spike"]
            .as_f64()
            .partial_cmp(&a["spike"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(60);
    if let Some(hn) = hn {
        let names: Vec<String> = out
            .iter()
            .take(12)
            .map(|v| v["name"].as_str().unwrap_or("").to_string())
            .collect();
        let found = hn.mentions(&names, a0 - 3 * DAY, at + DAY).await;
        for v in out.iter_mut().take(12) {
            if let Some(m) = found.get(v["name"].as_str().unwrap_or("")) {
                v["why"].as_array_mut().unwrap().push(json!({"kind": "hn", "text": format!("on Hacker News: “{}” ({} points)", m["title"].as_str().unwrap_or(""), m["points"]), "link": m["url"], "points": m["points"]}));
            }
        }
    }
    Ok(json!({"at": at, "days": days, "repos": out}))
}

// ------------------------------------------------------------- trends

/// Languages or topics over the years: each one's share of the stars (and
/// of the repos that got any) year by year, and what grew most this last
/// year against the one before.
pub async fn trends(c: &Ctx<'_>, kind: &str, names: &[String], latest: i64) -> Result<Value> {
    let (table, col) = if kind == "topic" {
        ("gh_topic_months", "topic")
    } else {
        ("gh_lang_months", "language")
    };
    let y1 = months_back(latest, 12);
    let y2 = months_back(latest, 24);
    let (now, before, totals) = tokio::join!(
        c.soft("this last year", format!("SELECT {col} AS k, SUM(stars) AS s, SUM(repos) AS r FROM {table} WHERE month >= :y1 GROUP BY k ORDER BY s DESC LIMIT 300"), json!({"y1": y1})),
        c.soft("the year before", format!("SELECT {col} AS k, SUM(stars) AS s FROM {table} WHERE month >= :y2 AND month < :y1 GROUP BY k ORDER BY s DESC LIMIT 2000"), json!({"y1": y1, "y2": y2})),
        c.soft("every month's total", format!("SELECT month, SUM(stars) AS s, SUM(repos) AS r FROM {table} GROUP BY month ORDER BY month LIMIT 400"), Value::Null),
    );
    let picked: Vec<String> = if names.is_empty() {
        now.iter().take(6).map(|r| r.s("k")).collect()
    } else {
        names.to_vec()
    };
    let series = c.soft("the picked ones month by month", format!("SELECT month, {col} AS k, SUM(stars) AS s, SUM(repos) AS r, SUM(pushes) AS p FROM {table} WHERE {col} IN (:k) GROUP BY month, k ORDER BY month LIMIT 10000"), json!({"k": picked})).await;
    let year = |m: i64| 1970 + (m + 15 * DAY) / YEAR;
    let mut tot: HashMap<i64, (i64, i64)> = HashMap::new();
    for t in &totals {
        let e = tot.entry(year(t.i("month"))).or_default();
        e.0 += t.i("s");
        e.1 += t.i("r");
    }
    let mut per: HashMap<String, HashMap<i64, (i64, i64)>> = HashMap::new();
    for r in &series {
        let e = per
            .entry(r.s("k"))
            .or_default()
            .entry(year(r.i("month")))
            .or_default();
        e.0 += r.i("s");
        e.1 += r.i("r");
    }
    let this_year = year(latest);
    let lines: Vec<Value> = picked
        .iter()
        .map(|k| {
            let mut pts: Vec<(i64, i64, i64)> = per.get(k).map(|m| m.iter().map(|(y, v)| (*y, v.0, v.1)).collect()).unwrap_or_default();
            pts.sort_unstable();
            let share: Vec<Value> = pts.iter().filter(|p| p.0 >= 2012).map(|(y, s, _)| json!([y, (*s as f64 / tot.get(y).map_or(1, |t| t.0.max(1)) as f64 * 10000.0).round() / 100.0])).collect();
            json!({"name": k, "share": share, "stars": pts.iter().map(|(y, s, _)| json!([y, s])).collect::<Vec<_>>(), "repos": pts.iter().map(|(y, _, r)| json!([y, r])).collect::<Vec<_>>()})
        })
        .collect();
    let before: HashMap<String, i64> = before.iter().map(|r| (r.s("k"), r.i("s"))).collect();
    let all_now: i64 = now.iter().map(|r| r.i("s")).sum::<i64>().max(1);
    let all_before: i64 = before.values().sum::<i64>().max(1);
    let mut growth: Vec<Value> = now
        .iter()
        .filter(|r| r.i("s") >= 500)
        .map(|r| {
            let k = r.s("k");
            let a = r.i("s") as f64 / all_now as f64;
            let b = before.get(&k).copied().unwrap_or(0) as f64 / all_before as f64;
            json!({"name": k, "stars": r.i("s"), "repos": r.i("r"), "share": (a * 10000.0).round() / 100.0, "share_before": (b * 10000.0).round() / 100.0, "change": if b > 0.0 { ((a / b - 1.0) * 100.0).round() } else { 999.0 }})
        })
        .collect();
    growth.sort_by(|a, b| {
        b["change"]
            .as_f64()
            .partial_cmp(&a["change"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let falling: Vec<Value> = growth
        .iter()
        .rev()
        .filter(|g| g["change"].as_f64().unwrap_or(0.0) < 0.0)
        .take(8)
        .cloned()
        .collect();
    growth.retain(|g| g["change"].as_f64().unwrap_or(0.0) > 0.0);
    growth.truncate(12);
    Ok(
        json!({"kind": kind, "picked": picked, "lines": lines, "rising": growth, "falling": falling, "top": now.iter().take(30).map(obj).collect::<Vec<_>>(), "this_year": this_year}),
    )
}

// ------------------------------------------------------------ for you

/// What to star next for a GitHub login, from what they starred and
/// worked on: the repos the people who starred the same things starred,
/// against how starred each is anyway.
pub async fn for_you(c: &Ctx<'_>, login: &str) -> Result<Value> {
    let l = login.trim().trim_start_matches('@').to_lowercase();
    let who = c
        .soft(
            "the login",
            "SELECT actor, login FROM gh_actors WHERE llogin = :l LIMIT 1",
            json!({"l": l}),
        )
        .await;
    let Some(w) = who.first() else {
        return Ok(json!({"found": false, "login": login}));
    };
    let a = w.i("actor");
    let (starred, worked) = tokio::join!(
        c.soft("what they starred", "SELECT repo, t FROM gh_stars WHERE actor_k = :k ORDER BY t DESC LIMIT 200", json!({"k": a.to_string()})),
        c.soft("where they work", format!("SELECT repo, COUNT(*) AS n FROM gh_events WHERE actor = :a AND kind IN {WORK} GROUP BY repo ORDER BY n DESC LIMIT 10"), json!({"a": a})),
    );
    let mut seeds = ids(&starred, "repo");
    seeds.truncate(40);
    seeds.extend(ids(&worked, "repo"));
    let have: HashSet<i64> = ids(&starred, "repo")
        .into_iter()
        .chain(ids(&worked, "repo"))
        .collect();
    if seeds.is_empty() {
        return Ok(json!({"found": true, "login": w.s("login"), "seeds": [], "picks": []}));
    }
    let nb = c
        .soft(
            "people who starred the same repos",
            format!("SELECT actor FROM gh_stars WHERE repo IN (:s) ORDER BY t DESC LIMIT {COSTAR_PEOPLE}"),
            json!({"s": seeds}),
        )
        .await;
    let nb: Vec<i64> = ids(&nb, "actor").into_iter().filter(|x| *x != a).collect();
    let cand = if nb.is_empty() {
        Vec::new()
    } else {
        c.soft_within("what those people starred", "SELECT repo, COUNT(*) AS n FROM gh_stars WHERE actor_k IN (:p) GROUP BY repo ORDER BY n DESC LIMIT 400", json!({"p": keys(&nb)}), COSTAR_MS).await
    };
    let cids: Vec<i64> = ids(&cand, "repo")
        .into_iter()
        .filter(|x| !have.contains(x))
        .collect();
    let mut look = cids.clone();
    look.extend(seeds.iter().copied());
    let info = repos_of(c, "the repos", &look).await;
    let mut picks: Vec<Value> = cand
        .iter()
        .filter(|x| !have.contains(&x.i("repo")))
        .filter_map(|x| {
            let i = info.get(&x.i("repo"))?;
            let n = x.i("n");
            let score = n as f64 / ((i.i("stars") + 200) as f64).powf(0.45);
            let mut v = obj(i);
            v["shared"] = json!(n);
            v["score"] = json!((score * 1000.0).round() / 1000.0);
            Some(v)
        })
        .collect();
    picks.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .partial_cmp(&a["score"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    picks.truncate(30);
    let seeds_v: Vec<Value> = seeds.iter().filter_map(|s| info.get(s).map(obj)).collect();
    Ok(
        json!({"found": true, "login": w.s("login"), "seeds": seeds_v, "neighbors": nb.len(), "picks": picks}),
    )
}

// ------------------------------------------------------------- weekly

/// A week of GitHub to `at`: what jumped, what is new and already
/// starred, and the releases of the most starred repos.
pub async fn weekly(
    c: &Ctx<'_>,
    at: i64,
    hn: Option<&crate::api::Hn>,
    big: &[i64],
) -> Result<Value> {
    let w0 = at - 7 * DAY;
    let (rise, fresh, rel) = tokio::join!(
        rising(c, at, 7, hn),
        c.soft("new repos with the most stars", format!("SELECT {REPO_COLS} FROM gh_repos WHERE first_seen > :f ORDER BY stars DESC LIMIT 12"), json!({"f": at - 30 * DAY})),
        async {
            if big.is_empty() {
                return Vec::new();
            }
            c.soft("releases of the most starred repos", "SELECT repo, COUNT(*) AS n, MAX(t) AS t FROM gh_events WHERE repo IN (:big) AND kind = 'release' AND t > :w0 AND t <= :w1 GROUP BY repo ORDER BY n DESC LIMIT 40", json!({"big": big, "w0": w0, "w1": at + DAY})).await
        },
    );
    let rids = ids(&rel, "repo");
    let info = repos_of(c, "those repos", &rids).await;
    let mut releases: Vec<Value> = rel
        .iter()
        .filter_map(|r| {
            let i = info.get(&r.i("repo"))?;
            let mut v = obj(i);
            v["releases"] = json!(r.i("n"));
            v["t"] = json!(r.i("t"));
            Some(v)
        })
        .collect();
    releases.sort_by_key(|v| -v["stars"].as_i64().unwrap_or(0));
    releases.truncate(16);
    let mut rise = rise?;
    if let Some(a) = rise["repos"].as_array_mut() {
        a.truncate(15);
    }
    Ok(
        json!({"at": at, "from": w0, "rising": rise["repos"], "new": fresh.iter().map(obj).collect::<Vec<_>>(), "releases": releases}),
    )
}

// --------------------------------------------------------------- home

pub async fn home(c: &Ctx<'_>, latest: i64, hn: Option<&crate::api::Hn>) -> Result<Value> {
    let (rise, langs, top) = tokio::join!(
        rising(c, latest, 7, hn),
        trends(c, "language", &[], latest),
        c.soft(
            "the most starred repos",
            format!("SELECT {REPO_COLS} FROM gh_repos ORDER BY stars DESC LIMIT 10"),
            Value::Null
        ),
    );
    let mut rise = rise?;
    if let Some(a) = rise["repos"].as_array_mut() {
        a.truncate(10);
    }
    let langs = langs?;
    Ok(
        json!({"latest": latest, "rising": rise["repos"], "languages": langs["rising"], "top": top.iter().map(obj).collect::<Vec<_>>()}),
    )
}

/// How big it is: the rows of every table, the span of the events, and
/// what each shard holds.
pub async fn scale(c: &Ctx<'_>) -> Value {
    let tables = [
        "gh_events",
        "gh_stars",
        "gh_stars_daily",
        "gh_repos",
        "gh_repo_names",
        "gh_actors",
        "gh_repo_months",
        "gh_lang_months",
        "gh_topic_months",
    ];
    let counts = futures_join(tables.iter().map(|t| async move {
        (
            t.to_string(),
            c.soft(
                format!("rows of {t}"),
                format!("SELECT COUNT(*) AS n FROM {t}"),
                Value::Null,
            )
            .await
            .first()
            .map(|r| r.i("n"))
            .unwrap_or(0),
        )
    }))
    .await;
    let span = c
        .soft(
            "the first event",
            // Ordered by time, the column's block bounds find each end
            // at once; MIN and MAX read every row.
            "SELECT t FROM gh_events ORDER BY t LIMIT 1",
            Value::Null,
        )
        .await;
    let last = c
        .soft(
            "the latest event",
            "SELECT t FROM gh_events ORDER BY t DESC LIMIT 1",
            Value::Null,
        )
        .await;
    let span: Vec<Row> = match (span.first(), last.first()) {
        (Some(a), Some(b)) => vec![json!({"first": a.i("t"), "last": b.i("t")}).as_object().cloned().unwrap_or_default()],
        _ => Vec::new(),
    };
    json!({"tables": counts.into_iter().collect::<Map<String, Value>>().into_iter().map(|(k, v)| (k, v)).collect::<Map<String, Value>>(), "span": span.first().map(obj)})
}

async fn futures_join<F: std::future::Future<Output = (String, i64)>>(
    it: impl Iterator<Item = F>,
) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (k, v) in futures_util::future::join_all(it).await {
        out.push((k, json!(v)));
    }
    out
}

#[cfg(test)]
mod topic_tests {
    #[test]
    fn a_topic_is_one_of_the_list_not_a_word_of_one() {
        assert!(super::has_topic("classification,machine-learning,python", "machine-learning"));
        assert!(super::has_topic("ai, LLM", "llm"));
        assert!(!super::has_topic("machine-learning-algorithms,optimization", "machine-learning"));
        assert!(!super::has_topic("", "llm"));
    }
}
