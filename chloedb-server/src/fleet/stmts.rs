//! The fleet check of statements that time out or take seconds a call
//! (gh, 11-16: person pages filtered 11.2 billion events by `actor`, which
//! the table neither indexed nor was clustered by, so each call read every
//! row until it timed out), and what most likely makes each slow: a
//! filter no index serves ([`reads`]), a shard that holds it up, and on
//! that shard a copy whose disk reads slowly ([`super::disk::speed`]).

use std::collections::HashMap;
use std::sync::Arc;

use chloedb::schema::Schema;
use serde_json::{json, Value as Json};
use sqlparser::ast::{BinaryOperator, Expr, GroupByExpr, SetExpr, Statement, TableFactor};

use super::disk::ms;
use super::{pct, Problem};

/// The window statements are judged over.
pub const WINDOW_MS: u64 = 15 * 60_000;
/// Statements told at most, the most time first.
const TOLD: usize = 3;
/// Timeouts that make a statement one that times out: this many, and this
/// share of its calls.
const TIMEOUTS: u64 = 2;
const TIMEOUT_SHARE: f64 = 0.1;
/// A statement this slow on average is told, whatever its p99 objective
/// (and ten times that objective, if more).
const SLOW_MEAN_MS: f64 = 2000.0;

/// Whether statement `s` (a row of [`crate::telemetry::Statements::window`])
/// times out, or takes seconds a call: worth its cause worked out.
pub fn slow(s: &Json, slo_p99_ms: f64) -> bool {
    if s["command_center"] == true {
        return false;
    }
    let calls = s["calls"].as_u64().unwrap_or(0);
    let timeouts = s["outcomes"]["timeout"].as_u64().unwrap_or(0);
    let mean = s["mean_ms"].as_f64().unwrap_or(0.0);
    (timeouts >= TIMEOUTS && timeouts as f64 >= TIMEOUT_SHARE * calls as f64) || (calls >= 3 && mean >= SLOW_MEAN_MS.max(10.0 * slo_p99_ms))
}

fn ident_of(e: &Expr) -> Option<Vec<String>> {
    match e {
        Expr::Identifier(i) => Some(vec![i.value.clone()]),
        Expr::CompoundIdentifier(v) => Some(v.iter().map(|i| i.value.clone()).collect()),
        Expr::Nested(x) => ident_of(x),
        _ => None,
    }
}

/// What one conjunct of a WHERE does with the table's fields.
enum Conjunct {
    /// An index serves it: a search (`MATCH`, `PREFIX`, ...), or a
    /// comparison on an indexed field, the primary key or the field the
    /// table is clustered by.
    Narrows,
    /// A comparison only a scan answers: these fields.
    Scans(Vec<String>),
    Unknown,
}

const SEARCHES: [&str; 8] = ["match", "prefix", "at_least", "near", "phrase", "similarity", "knn", "fuzzy"];

fn split_and<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::BinaryOp { left, op: BinaryOperator::And, right } => {
            split_and(left, out);
            split_and(right, out);
        }
        Expr::Nested(x) => split_and(x, out),
        _ => out.push(e),
    }
}

/// The field of `t` that `name` (a column, `alias.column` or `field.path`) is.
fn field_named(t: &Schema, name: &[String]) -> Option<String> {
    let last = name.last()?;
    if t.field(last).is_some() {
        return Some(last.clone());
    }
    name.iter().find(|n| t.field(n).is_some()).cloned()
}

/// The fields `e` compares, if it is a comparison (or an OR of them) on
/// fields of `t` alone; None for anything else.
fn compared(t: &Schema, e: &Expr, out: &mut Vec<String>) -> bool {
    let field = |x: &Expr| ident_of(x).and_then(|n| field_named(t, &n));
    match e {
        Expr::Nested(x) => compared(t, x, out),
        Expr::BinaryOp { left, op: BinaryOperator::Or, right } => compared(t, left, out) && compared(t, right, out),
        Expr::BinaryOp { left, op, right } if matches!(op, BinaryOperator::Eq | BinaryOperator::NotEq | BinaryOperator::Lt | BinaryOperator::LtEq | BinaryOperator::Gt | BinaryOperator::GtEq) => {
            match (field(left), field(right)) {
                (Some(f), None) | (None, Some(f)) => {
                    out.push(f);
                    true
                }
                _ => false,
            }
        }
        Expr::InList { expr, .. } | Expr::Between { expr, .. } | Expr::Like { expr, .. } | Expr::ILike { expr, .. } | Expr::IsNull(expr) | Expr::IsNotNull(expr) => match field(expr) {
            Some(f) => {
                out.push(f);
                true
            }
            None => false,
        },
        _ => false,
    }
}

fn conjunct(t: &Schema, e: &Expr) -> Conjunct {
    if let Expr::Function(f) = e {
        let name = f.name.to_string().to_lowercase();
        return if SEARCHES.contains(&name.trim_matches('"')) { Conjunct::Narrows } else { Conjunct::Unknown };
    }
    let mut fields = Vec::new();
    if !compared(t, e, &mut fields) {
        return Conjunct::Unknown;
    }
    let pk = t.primary_key.and_then(|i| t.fields.get(i)).map(|f| f.name.as_str());
    let served = |f: &String| t.field(f).is_some_and(|x| x.indexed) || Some(f.as_str()) == pk || t.cluster_by.as_deref() == Some(f.as_str());
    // An OR is served only when each of its sides is.
    let scans: Vec<String> = fields.iter().filter(|f| !served(f)).cloned().collect();
    if scans.is_empty() {
        Conjunct::Narrows
    } else {
        Conjunct::Scans(scans)
    }
}

/// What statement `sql` most likely reads of the table it reads first
/// (`FROM`): `{table, rows?, cluster_by?, every_row, why?, fields?}`.
/// `every_row` when no part of its WHERE is served by an index, the
/// primary key or the table's order, and a part filters on fields only a
/// scan reads (`fields`), or when it has no WHERE and groups or sorts.
/// `rows` is the table's row count when the server knows it.
pub fn reads(sql: &str, catalog: &[Arc<Schema>], rows: &HashMap<String, u64>) -> Json {
    let Ok(statements) = chloedb::query::parse::parse(sql) else { return Json::Null };
    let [Statement::Query(q)] = statements.as_slice() else { return Json::Null };
    let SetExpr::Select(sel) = q.body.as_ref() else { return Json::Null };
    let Some(TableFactor::Table { name, .. }) = sel.from.first().map(|f| &f.relation) else { return Json::Null };
    let tname = name.to_string();
    let tname = tname.rsplit('.').next().unwrap_or(&tname).trim_matches('"').to_string();
    let Some(t) = catalog.iter().find(|s| s.table == tname) else { return json!({"table": tname}) };
    let mut j = json!({"table": t.table, "rows": rows.get(&t.table), "cluster_by": t.cluster_by, "every_row": false});
    match &sel.selection {
        Some(w) => {
            let mut parts = Vec::new();
            split_and(w, &mut parts);
            let kinds: Vec<Conjunct> = parts.iter().map(|p| conjunct(t, p)).collect();
            let narrows = kinds.iter().any(|k| matches!(k, Conjunct::Narrows));
            let mut scans: Vec<String> = kinds.into_iter().flat_map(|k| if let Conjunct::Scans(f) = k { f } else { Vec::new() }).collect();
            scans.dedup();
            if !narrows && !scans.is_empty() {
                j["every_row"] = json!(true);
                j["why"] = json!("filter");
                j["fields"] = json!(scans);
            }
        }
        None => {
            let groups = match &sel.group_by {
                GroupByExpr::All(_) => true,
                GroupByExpr::Expressions(v, _) => !v.is_empty(),
            };
            if groups || q.order_by.is_some() {
                j["every_row"] = json!(true);
                j["why"] = json!("no_filter");
            }
        }
    }
    j
}

/// Billions, millions and thousands in words: "11.2 billion".
fn count(n: u64) -> String {
    let f = n as f64;
    if f >= 1e9 {
        format!("{:.1} billion", f / 1e9).replace(".0 billion", " billion")
    } else if f >= 1e6 {
        format!("{:.1} million", f / 1e6).replace(".0 million", " million")
    } else {
        n.to_string()
    }
}

/// The SQL of a statement, short enough for a title.
fn short_sql(sql: &str) -> String {
    let one: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 90 {
        format!("{}…", one.chars().take(89).collect::<String>())
    } else {
        one
    }
}

/// The statements of `statements` that time out or take seconds ([`slow`]),
/// each told with what most likely makes it slow: `reads` and `by_shard`
/// as the service adds them to the rows it passes, and the disks
/// [`super::disk::speed`] told slow, as (shard, copy).
pub(super) fn check(statements: &[Json], timeout_ms: u64, slo_p99_ms: f64, disk_slow: &[(usize, String)], checks: &mut Vec<Json>, problems: &mut Vec<Problem>) {
    let mut found: Vec<&Json> = statements.iter().filter(|s| slow(s, slo_p99_ms)).collect();
    found.sort_by(|a, b| b["total_ms"].as_f64().unwrap_or(0.0).total_cmp(&a["total_ms"].as_f64().unwrap_or(0.0)));
    let timing_out = found.iter().filter(|s| s["outcomes"]["timeout"].as_u64().unwrap_or(0) > 0).count();
    let mut status = if found.is_empty() { "ok" } else { "warning" };
    for s in found.iter().take(TOLD) {
        let id = s["id"].as_str().unwrap_or("?");
        let fp = s["fingerprint"].as_str().unwrap_or(id);
        let calls = s["calls"].as_u64().unwrap_or(0);
        let timeouts = s["outcomes"]["timeout"].as_u64().unwrap_or(0);
        let mean = s["mean_ms"].as_f64().unwrap_or(0.0);
        let sql = s["sql"].as_str().unwrap_or(fp);
        let r = &s["reads"];
        let table = r["table"].as_str();
        let critical = calls >= 5 && timeouts * 2 >= calls;
        if critical {
            status = "critical";
        }
        let deadline = if timeout_ms > 0 { format!("the {} deadline", ms(timeout_ms as f64)) } else { "its deadline".into() };
        let (title, headline) = if timeouts > 0 {
            (
                format!("Statement {id} times out: {timeouts} of its {calls} calls in the last 15 min passed {deadline}{}", table.map_or(String::new(), |t| format!(" (it reads {t})"))),
                format!("A statement times out: {timeouts} of {calls} calls{}", table.map_or(String::new(), |t| format!(" on {t}"))),
            )
        } else {
            (
                format!("Statement {id} takes {} a call on average over its {calls} calls in the last 15 min (p99 {})", ms(mean), ms(s["p99_ms"].as_f64().unwrap_or(mean))),
                format!("A statement takes {} a call{}", ms(mean), table.map_or(String::new(), |t| format!(" on {t}"))),
            )
        };
        let mut why: Vec<String> = Vec::new();
        if r["every_row"] == true {
            let rows = r["rows"].as_u64().map_or_else(|| "row".to_string(), |n| format!("one of its {} rows", count(n)));
            let t = table.unwrap_or("its table");
            match r["why"].as_str() {
                Some("filter") => {
                    let fields: Vec<String> = r["fields"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).map(String::from).collect();
                    let order = r["cluster_by"].as_str().map_or(String::new(), |c| format!(" (it is clustered by {c})"));
                    why.push(format!(
                        "It filters on {}, which {t} does not index and is not clustered by{order}: each call reads every {rows}. An index on {}, the table clustered by it, or a summary table kept by it reads only the rows it needs",
                        super::both(&fields),
                        super::either(&fields)
                    ));
                }
                _ => why.push(format!("It has no WHERE and groups or sorts: each call reads every {rows} of {t}. A summary table kept as rows are written reads far less")),
            }
        }
        // A shard that holds it up, from the calls the server keeps.
        let bs: Vec<&Json> = s["by_shard"].as_array().into_iter().flatten().collect();
        if bs.len() >= 2 {
            let mut p: Vec<(u64, f64)> = bs.iter().filter_map(|x| Some((x["shard"].as_u64()?, x["p99_ms"].as_f64()?))).collect();
            p.sort_by(|a, b| b.1.total_cmp(&a.1));
            let med = p.get(p.len() / 2).map_or(f64::INFINITY, |x| x.1);
            if let Some(&(shard, worst)) = p.first().filter(|w| p.len() >= 2 && w.1 > 3.0 * med && w.1 > 20.0) {
                let disk = disk_slow.iter().find(|(s, _)| *s as u64 == shard).map_or(String::new(), |(_, c)| format!(", where {c}'s disk reads far slower than its peers' (the problem about it says how much)"));
                why.push(format!("Shard {shard} answers it at p99 {} against {} on the other shards{disk}", ms(worst), ms(med)));
            }
        } else if let (Some((shard, c)), true) = (disk_slow.first(), why.is_empty()) {
            why.push(format!("{c}'s disk, under shard {shard}, reads far slower than its peers': a statement that reads that shard waits on it"));
        }
        if s["rows_per_call"].as_f64().is_some_and(|r| r > 1000.0) {
            why.push(format!("{:.0} rows a call: the coordinator gathers and sorts them all; a smaller LIMIT, or a page by key, costs less", s["rows_per_call"].as_f64().unwrap_or(0.0)));
        }
        if why.is_empty() {
            why.push("Nothing in its figures points at a cause: its page has its slowest calls, by shard and copy, to profile".into());
        }
        let share = s["share"].as_f64().unwrap_or(0.0);
        let impact = if timeouts > 0 {
            format!("Its callers wait {} and get no rows (504){}", if timeout_ms > 0 { ms(timeout_ms as f64) } else { "for the deadline".into() }, if share >= 0.05 { format!("; it took {} of the queries' time here, which the others wait behind", pct(share)) } else { String::new() })
        } else {
            format!("Its callers wait {} a call{}", ms(mean), if share >= 0.05 { format!("; it took {} of the queries' time here", pct(share)) } else { String::new() })
        };
        let mut where_ = vec![format!("statement {id}")];
        where_.extend(table.map(String::from));
        problems.push(Problem {
            rank: if critical { 6 } else { 11 },
            critical,
            headline,
            j: json!({"id": format!("slow-statement-{id}"), "severity": super::severity(critical), "title": title, "where": where_, "since_ms": null,
                "impact": impact, "why": why, "sql": short_sql(sql), "calls": calls, "timeouts": timeouts, "mean_ms": mean, "reads": r,
                "next": {"text": "Open the statement: its slowest calls, where it waits by shard and copy, and who sends it", "view": format!("statement/{id}"), "params": {}}}),
        });
    }
    checks.push(json!({"id": "statements", "status": status, "slow": found.len(), "timing_out": timing_out, "window_s": WINDOW_MS / 1000,
        "where": found.iter().take(TOLD).filter_map(|s| s["id"].as_str()).collect::<Vec<_>>()}));
}
