//! Aggregations as `GROUP BY` queries over the rows a search's condition
//! keeps: `terms`, `histogram`, `date_histogram`, `range`, `date_range`,
//! `filters`, `filter`, `missing`, `global` and `composite` buckets, with metrics
//! (`avg`, `sum`, `min`, `max`, `value_count`, `cardinality`, `stats`,
//! `extended_stats`, `percentiles`) and `top_hits` under them.
//!
//! A bucket's sub-aggregations run over its rows: the parent's condition
//! and the bucket's own. Metrics under `terms` are columns of its query,
//! so that `order` can name them.

use futures::future::BoxFuture;
use serde_json::{json, Map, Value as Json};

use super::dsl::{add_units, compile, date_math, num, Ctx, Target};
use super::mapping::{ident, lit, Index, Kind};
use super::{Os, OsError};

/// The condition of the rows an aggregation reads (`None`: all), with
/// another one joined to it.
fn with(pred: &Option<String>, more: String) -> Option<String> {
    Some(match pred {
        Some(p) => format!("({p}) AND ({more})"),
        None => more,
    })
}

fn where_of(pred: &Option<String>) -> String {
    pred.as_ref().map(|p| format!(" WHERE {p}")).unwrap_or_default()
}

/// The SQL a field's values are read by, refusing a text field (which
/// OpenSearch aggregates only with fielddata).
fn value_of(cx: &Ctx, field: &str) -> Result<(String, bool), OsError> {
    match cx.target(field) {
        Target::Text(..) => Err(OsError::new(400, "illegal_argument_exception", format!("Text fields are not optimised for operations that require per-document field data like aggregations and sorting, so these operations are disabled by default. Please use a keyword field instead (field [{field}])"))),
        Target::Vector(_) => Err(OsError::new(400, "illegal_argument_exception", format!("Field [{field}] of type [knn_vector] is not supported for aggregation"))),
        t @ Target::Date(_) => Ok((t.sql(), true)),
        // A number no document holds several of: its column, which Strata
        // aggregates from the columns, not a path of `_source`.
        Target::Json(_, Some(Kind::Long | Kind::Double)) if cx.ix.single_numbers.as_ref().is_some_and(|s| s.contains(field)) => Ok((ident(field), false)),
        t => Ok((t.sql(), false)),
    }
}

/// A date column's value as OpenSearch answers it: milliseconds.
fn date_ms(v: &Json) -> Option<i64> {
    v.as_str().and_then(strata::timestamp::parse).map(|t| t.div_euclid(1000))
}

fn date_text(ms: i64) -> String {
    let s = strata::timestamp::format(ms * 1000);
    // OpenSearch writes milliseconds always.
    match s.find('.') {
        Some(_) => s,
        None => s.replacen('Z', ".000Z", 1),
    }
}

/// The metric kinds folded into a bucket query as columns.
const FOLDED: [&str; 7] = ["avg", "sum", "min", "max", "value_count", "cardinality", "stats"];

/// The aggregate columns of a metric (for `stats`, five), each with the
/// key of its answer.
fn metric_cols(cx: &Ctx, kind: &str, spec: &Json) -> Result<Vec<(String, String)>, OsError> {
    Ok(metric_cols_of(kind, &metric_value(cx, kind, spec)?))
}

/// The value a metric reads.
fn metric_value(cx: &Ctx, kind: &str, spec: &Json) -> Result<String, OsError> {
    let field = spec.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing(format!("[{kind}] requires a field")))?;
    Ok(value_of(cx, field)?.0)
}

/// A metric's columns over the value `x`.
fn metric_cols_of(kind: &str, x: &str) -> Vec<(String, String)> {
    match kind {
        "avg" => vec![("value".into(), format!("AVG({x})"))],
        "sum" => vec![("value".into(), format!("SUM({x})"))],
        "min" => vec![("value".into(), format!("MIN({x})"))],
        "max" => vec![("value".into(), format!("MAX({x})"))],
        "value_count" => vec![("value".into(), format!("COUNT({x})"))],
        "cardinality" => vec![("value".into(), format!("COUNT(DISTINCT {x})"))],
        "stats" => vec![("count".into(), format!("COUNT({x})")), ("min".into(), format!("MIN({x})")), ("max".into(), format!("MAX({x})")), ("avg".into(), format!("AVG({x})")), ("sum".into(), format!("SUM({x})"))],
        _ => unreachable!("a folded metric"),
    }
}

/// A metric's answer from its columns' values.
fn metric_json(kind: &str, cx: &Ctx, spec: &Json, vals: &[(String, Json)]) -> Json {
    let date = spec.get("field").and_then(Json::as_str).is_some_and(|f| matches!(cx.target(f), Target::Date(_)));
    let counts = matches!(kind, "value_count" | "cardinality");
    let fix = |k: &str, v: &Json| -> Json {
        match v {
            Json::Null if counts || (kind == "stats" && k == "count") => json!(0),
            Json::Null if kind == "sum" || (kind == "stats" && k == "sum") => json!(0.0),
            Json::String(_) if date => date_ms(v).map_or(Json::Null, |m| json!(m as f64)),
            // A metric of numbers is a double (an INT column's sum or
            // minimum too); counts are whole.
            Json::Number(n) if !counts && k != "count" => json!(n.as_f64().unwrap_or(0.0)),
            v => v.clone(),
        }
    };
    let mut m = Map::new();
    for (k, v) in vals {
        m.insert(k.clone(), fix(k, v));
    }
    if kind == "value_count" || kind == "cardinality" || m.get("count").is_some() {
        // Counts are whole.
        for k in ["value", "count"] {
            if let Some(Json::Number(n)) = m.get(k) {
                if let Some(f) = n.as_f64() {
                    if kind != "avg" && kind != "sum" && kind != "min" && kind != "max" {
                        m.insert(k.into(), json!(f as i64));
                    }
                }
            }
        }
    }
    if date {
        for k in ["value", "min", "max"] {
            if let Some(ms) = m.get(k).and_then(Json::as_f64) {
                m.insert(format!("{k}_as_string"), json!(date_text(ms as i64)));
            }
        }
    }
    Json::Object(m)
}

/// All of `aggs` (a request's `aggs`/`aggregations` object) over the rows
/// `pred` keeps.
pub fn run<'a>(os: &'a Os, ix: &'a Index, pred: Option<String>, aggs: &'a Json) -> BoxFuture<'a, Result<Json, OsError>> {
    Box::pin(async move {
        let m = aggs.as_object().ok_or_else(|| OsError::parsing("aggregations are an object of named aggregations"))?;
        let mut out = Map::new();
        let mut jobs = Vec::new();
        for (name, spec) in m {
            let pred = pred.clone();
            jobs.push(async move { (name.clone(), one(os, ix, pred, spec).await) });
        }
        for (name, r) in futures::future::join_all(jobs).await {
            out.insert(name, r?);
        }
        Ok(Json::Object(out))
    })
}

/// The kind and body of an aggregation, and its sub-aggregations.
fn parts(spec: &Json) -> Result<(String, Json, Option<Json>), OsError> {
    let m = spec.as_object().ok_or_else(|| OsError::parsing("an aggregation is an object"))?;
    let subs = m.get("aggs").or_else(|| m.get("aggregations")).cloned();
    let (kind, body) = m.iter().find(|(k, _)| !matches!(k.as_str(), "aggs" | "aggregations" | "meta")).ok_or_else(|| OsError::parsing("an aggregation names its type"))?;
    Ok((kind.clone(), body.clone(), subs))
}

fn one<'a>(os: &'a Os, ix: &'a Index, pred: Option<String>, spec: &'a Json) -> BoxFuture<'a, Result<Json, OsError>> {
    Box::pin(async move {
        let (kind, body, subs) = parts(spec)?;
        let cx = Ctx::new(ix);
        let mut answer = match kind.as_str() {
            k if FOLDED.contains(&k) => {
                let cols = metric_cols(&cx, k, &body)?;
                let sql = format!("SELECT {} FROM {}{}", cols.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>().join(", "), ix.table, where_of(&pred));
                let r = os.rows(&sql, &[]).await?;
                let row = r.rows.into_iter().next().unwrap_or_default();
                let vals: Vec<(String, Json)> = cols.iter().map(|(k, _)| k.clone()).zip(row).collect();
                return Ok(metric_json(k, &cx, &body, &vals));
            }
            "extended_stats" => {
                let field = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[extended_stats] requires a field"))?;
                let (x, _) = value_of(&cx, field)?;
                let sql = format!("WITH r AS (SELECT {x} AS x FROM {}{}) SELECT COUNT(x), MIN(x), MAX(x), AVG(x), SUM(x), SUM(x * x) FROM r", ix.table, where_of(&with(&pred, format!("{x} IS NOT NULL"))));
                let r = os.rows(&sql, &[]).await?;
                let row = r.rows.into_iter().next().unwrap_or_default();
                let f = |i: usize| row.get(i).and_then(Json::as_f64);
                let (count, sum, sq) = (f(0).unwrap_or(0.0), f(4).unwrap_or(0.0), f(5).unwrap_or(0.0));
                let avg = f(3);
                let var = avg.map(|a| (sq / count - a * a).max(0.0));
                json!({
                    "count": count as i64, "min": f(1), "max": f(2), "avg": avg, "sum": sum,
                    "sum_of_squares": sq, "variance": var, "variance_population": var, "variance_sampling": var.map(|v| if count > 1.0 { v * count / (count - 1.0) } else { v }),
                    "std_deviation": var.map(f64::sqrt), "std_deviation_population": var.map(f64::sqrt), "std_deviation_sampling": var.map(|v| if count > 1.0 { (v * count / (count - 1.0)).sqrt() } else { v.sqrt() }),
                    "std_deviation_bounds": {"upper": avg.zip(var).map(|(a, v)| a + 2.0 * v.sqrt()), "lower": avg.zip(var).map(|(a, v)| a - 2.0 * v.sqrt())},
                })
            }
            "percentiles" => {
                let field = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[percentiles] requires a field"))?;
                let (x, date) = value_of(&cx, field)?;
                let ps: Vec<f64> = body.get("percents").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_f64).collect()).unwrap_or_else(|| vec![1.0, 5.0, 25.0, 50.0, 75.0, 95.0, 99.0]);
                let x_num = if date { format!("EXTRACT(epoch FROM {x}) * 1000") } else { x.clone() };
                let cols: Vec<String> = ps.iter().map(|p| format!("PERCENTILE_CONT({}) WITHIN GROUP (ORDER BY x)", p / 100.0)).collect();
                let sql = format!("WITH r AS (SELECT {x_num} AS x FROM {}{}) SELECT {} FROM r", ix.table, where_of(&with(&pred, format!("{x} IS NOT NULL"))), cols.join(", "));
                let r = os.rows(&sql, &[]).await?;
                let row = r.rows.into_iter().next().unwrap_or_default();
                let keyed = body.get("keyed").and_then(Json::as_bool).unwrap_or(true);
                let vals: Vec<(String, Json)> = ps.iter().zip(row).map(|(p, v)| (format!("{p:?}"), v)).collect();
                if keyed {
                    json!({"values": Json::Object(vals.into_iter().collect())})
                } else {
                    json!({"values": vals.into_iter().map(|(k, v)| json!({"key": k.parse::<f64>().unwrap_or(0.0), "value": v})).collect::<Vec<_>>()})
                }
            }
            "top_hits" => return os.top_hits(ix, pred.clone(), &body).await,
            "terms" => return terms(os, ix, &cx, pred.clone(), &body, subs.as_ref()).await,
            "composite" => return composite(os, ix, &cx, pred.clone(), &body, subs.as_ref()).await,
            "histogram" | "date_histogram" => return histogram(os, ix, &cx, pred.clone(), &body, subs.as_ref(), kind == "date_histogram").await,
            "range" | "date_range" => return ranges(os, ix, &cx, pred.clone(), &body, subs.as_ref(), kind == "date_range").await,
            "filters" => {
                let fs = body.get("filters").ok_or_else(|| OsError::parsing("[filters] requires filters"))?;
                let mut named: Vec<(Option<String>, Option<String>)> = Vec::new();
                let mut cx2 = Ctx::new(ix);
                match fs {
                    Json::Object(m) => {
                        for (k, q) in m {
                            named.push((Some(k.clone()), compile(q, &mut cx2)?.pred));
                        }
                    }
                    Json::Array(a) => {
                        for q in a {
                            named.push((None, compile(q, &mut cx2)?.pred));
                        }
                    }
                    _ => return Err(OsError::parsing("[filters] is an object or an array of queries")),
                }
                let mut buckets_obj = Map::new();
                let mut buckets_arr = Vec::new();
                for (name, p) in &named {
                    let p2 = match p {
                        Some(p) => with(&pred, p.clone()),
                        None => pred.clone(),
                    };
                    let b = bucket(os, ix, p2, Map::new(), subs.as_ref()).await?;
                    match name {
                        Some(n) => {
                            buckets_obj.insert(n.clone(), b);
                        }
                        None => buckets_arr.push(b),
                    }
                }
                if body.get("other_bucket").and_then(Json::as_bool) == Some(true) || body.get("other_bucket_key").is_some() {
                    let others: Vec<String> = named.iter().filter_map(|(_, p)| p.clone()).map(|p| format!("NOT ({p})")).collect();
                    let p2 = if others.is_empty() { Some(super::dsl::NONE.to_string()) } else { with(&pred, others.join(" AND ")) };
                    let b = bucket(os, ix, p2, Map::new(), subs.as_ref()).await?;
                    let key = body.get("other_bucket_key").and_then(Json::as_str).unwrap_or("_other_").to_string();
                    if matches!(fs, Json::Object(_)) {
                        buckets_obj.insert(key, b);
                    } else {
                        buckets_arr.push(b);
                    }
                }
                return Ok(if matches!(fs, Json::Object(_)) { json!({"buckets": buckets_obj}) } else { json!({"buckets": buckets_arr}) });
            }
            "filter" => {
                let mut cx2 = Ctx::new(ix);
                let p = compile(&body, &mut cx2)?.pred;
                let p2 = match p {
                    Some(p) => with(&pred, p),
                    None => pred.clone(),
                };
                return bucket(os, ix, p2, Map::new(), subs.as_ref()).await;
            }
            "missing" => {
                let field = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[missing] requires a field"))?;
                let x = cx.target(field).sql();
                return bucket(os, ix, with(&pred, format!("{x} IS NULL")), Map::new(), subs.as_ref()).await;
            }
            "global" => return bucket(os, ix, None, Map::new(), subs.as_ref()).await,
            other => return Err(OsError::parsing(format!("Unknown aggregation type [{other}]: Strata's OpenSearch API answers terms, histogram, date_histogram, range, date_range, filters, filter, missing, global, avg, sum, min, max, value_count, cardinality, stats, extended_stats, percentiles and top_hits")).kind("unknown_named_object_exception")),
        };
        if let Some(meta) = spec.get("meta") {
            answer["meta"] = meta.clone();
        }
        Ok(answer)
    })
}

/// A single bucket: its rows' count and its sub-aggregations.
fn bucket<'a>(os: &'a Os, ix: &'a Index, pred: Option<String>, mut head: Map<String, Json>, subs: Option<&'a Json>) -> BoxFuture<'a, Result<Json, OsError>> {
    Box::pin(async move {
        let sql = format!("SELECT COUNT(*) FROM {}{}", ix.table, where_of(&pred));
        let n = os.rows(&sql, &[]).await?.rows.first().and_then(|r| r.first()).and_then(Json::as_i64).unwrap_or(0);
        head.insert("doc_count".into(), json!(n));
        if let Some(s) = subs {
            if let Json::Object(m) = run(os, ix, pred, s).await? {
                head.extend(m);
            }
        }
        Ok(Json::Object(head))
    })
}

/// Sub-aggregations of a bucket whose rows `pred` keeps, into `b`, all
/// but the metrics the bucket's query answered (`folded`).
async fn subs_into(os: &Os, ix: &Index, pred: Option<String>, subs: &Json, folded: &[String], b: &mut Map<String, Json>) -> Result<(), OsError> {
    let Some(m) = subs.as_object() else { return Ok(()) };
    let rest: Map<String, Json> = m.iter().filter(|(k, _)| !folded.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    if rest.is_empty() {
        return Ok(());
    }
    if let Json::Object(r) = run(os, ix, pred, &Json::Object(rest)).await? {
        b.extend(r);
    }
    Ok(())
}

/// The value of a JSON key as SQL compares a field with it.
fn key_sql(v: &Json, date: bool) -> String {
    match v {
        Json::String(s) => lit(s),
        Json::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        Json::Number(n) if date => lit(&strata::timestamp::format(n.as_i64().unwrap_or(0) * 1000)),
        Json::Number(n) => n.to_string(),
        _ => "NULL".into(),
    }
}

async fn terms(os: &Os, ix: &Index, cx: &Ctx<'_>, pred: Option<String>, body: &Json, subs: Option<&Json>) -> Result<Json, OsError> {
    let field = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[terms] requires a field (scripts are not supported)"))?;
    let (x, date) = value_of(cx, field)?;
    let size = body.get("size").and_then(Json::as_u64).unwrap_or(10) as usize;
    let min_docs = body.get("min_doc_count").and_then(Json::as_u64).unwrap_or(1).max(1);
    let missing = body.get("missing").cloned();
    // The metrics under it, as columns.
    let mut folded = Vec::new();
    let mut cols = Vec::new();
    if let Some(Json::Object(m)) = subs {
        for (name, s) in m {
            let (k, b, sub_subs) = parts(s)?;
            if FOLDED.contains(&k.as_str()) && sub_subs.is_none() {
                let mc = metric_cols(cx, &k, &b)?;
                folded.push((name.clone(), k.clone(), b.clone(), cols.len(), mc.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()));
                cols.extend(mc.into_iter().map(|(_, c)| c));
            }
        }
    }
    // The order: by count (the default, largest first), by key, or by a
    // metric under it.
    let mut order = Vec::new();
    let orders: Vec<(String, String)> = match body.get("order") {
        None => vec![("_count".into(), "desc".into())],
        Some(Json::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("desc").to_string())).collect(),
        Some(Json::Array(a)) => a.iter().filter_map(Json::as_object).flat_map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("desc").to_string()))).collect(),
        Some(_) => return Err(OsError::parsing("[terms] order is an object")),
    };
    for (k, dir) in &orders {
        let dir = if dir.eq_ignore_ascii_case("asc") { "ASC" } else { "DESC" };
        match k.as_str() {
            "_count" => order.push(format!("2 {dir}")),
            "_key" | "_term" => order.push(format!("1 {dir}")),
            m => {
                let (name, sub) = m.split_once('.').map_or((m, "value"), |(a, b)| (a, b));
                let f = folded.iter().find(|f| f.0 == name).ok_or_else(|| OsError::parsing(format!("Invalid aggregator order path [{m}]: order by a metric under the terms aggregation")))?;
                let i = f.4.iter().position(|c| c == sub).ok_or_else(|| OsError::parsing(format!("Invalid aggregator order path [{m}]")))?;
                order.push(format!("{} {dir}", 3 + f.3 + i));
            }
        }
    }
    if !orders.iter().any(|(k, _)| k == "_key" || k == "_term") {
        order.push("1 ASC".into());
    }
    let mut conds = Vec::new();
    if missing.is_none() {
        conds.push(format!("{x} IS NOT NULL"));
    }
    match body.get("include") {
        Some(Json::Array(a)) => conds.push(format!("{x} IN ({})", a.iter().map(|v| key_sql(v, false)).collect::<Vec<_>>().join(", "))),
        Some(Json::String(_)) => return Err(OsError::parsing("Strata's terms aggregation includes a list of values, not a pattern")),
        _ => {}
    }
    match body.get("exclude") {
        Some(Json::Array(a)) if !a.is_empty() => conds.push(format!("{x} NOT IN ({})", a.iter().map(|v| key_sql(v, false)).collect::<Vec<_>>().join(", "))),
        Some(Json::String(_)) => return Err(OsError::parsing("Strata's terms aggregation excludes a list of values, not a pattern")),
        _ => {}
    }
    let p = if conds.is_empty() { pred.clone() } else { with(&pred, conds.join(" AND ")) };
    let extra: String = cols.iter().map(|c| format!(", {c}")).collect();
    let having = if min_docs > 1 { format!(" HAVING COUNT(*) >= {min_docs}") } else { String::new() };
    let sql = format!("SELECT {x}, COUNT(*){extra} FROM {}{} GROUP BY 1{having} ORDER BY {} LIMIT {size}", ix.table, where_of(&p), order.join(", "));
    // Every bucket's count summed: values, not documents, so that a
    // field of arrays counts each of its values as OpenSearch does.
    let total_sql = format!("WITH g AS (SELECT {x} AS k, COUNT(*) AS c FROM {}{} GROUP BY 1) SELECT SUM(c) FROM g", ix.table, where_of(&p));
    let (r, total) = futures::join!(os.rows(&sql, &[]), os.rows(&total_sql, &[]));
    let (r, total) = (r?, total?.rows.first().and_then(|r| r.first()).and_then(Json::as_i64).unwrap_or(0));
    let mut buckets = Vec::new();
    let mut sum = 0;
    for row in r.rows {
        let key = row.first().cloned().unwrap_or(Json::Null);
        let n = row.get(1).and_then(Json::as_i64).unwrap_or(0);
        sum += n;
        let mut b = Map::new();
        let cond = match &key {
            Json::Null => format!("{x} IS NULL"),
            k => format!("{x} = {}", key_sql(k, false)),
        };
        match (&key, date) {
            (Json::Null, _) => {
                b.insert("key".into(), missing.clone().unwrap_or(Json::Null));
            }
            (Json::Bool(v), _) => {
                b.insert("key".into(), json!(if *v { 1 } else { 0 }));
                b.insert("key_as_string".into(), json!(v.to_string()));
            }
            (k, true) => {
                let ms = date_ms(k).unwrap_or(0);
                b.insert("key".into(), json!(ms));
                b.insert("key_as_string".into(), json!(date_text(ms)));
            }
            (k, false) => {
                b.insert("key".into(), k.clone());
            }
        }
        b.insert("doc_count".into(), json!(n));
        for (name, k, spec, at, keys) in &folded {
            let vals: Vec<(String, Json)> = keys.iter().enumerate().map(|(i, c)| (c.clone(), row.get(2 + at + i).cloned().unwrap_or(Json::Null))).collect();
            b.insert(name.clone(), metric_json(k, cx, spec, &vals));
        }
        if let Some(s) = subs {
            let names: Vec<String> = folded.iter().map(|f| f.0.clone()).collect();
            subs_into(os, ix, with(&p, cond), s, &names, &mut b).await?;
        }
        buckets.push(Json::Object(b));
    }
    Ok(json!({"doc_count_error_upper_bound": 0, "sum_other_doc_count": (total - sum).max(0), "buckets": buckets}))
}

/// The most groups a composite aggregation pages through.
const COMPOSITE_GROUPS: usize = 9_999;

/// Compares two bucket keys as a composite aggregation orders them:
/// missing first, then numbers, booleans and text by value.
fn key_order(a: &Json, b: &Json) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Json::Null, Json::Null) => Ordering::Equal,
        (Json::Null, _) => Ordering::Less,
        (_, Json::Null) => Ordering::Greater,
        (Json::Number(x), Json::Number(y)) => x.as_f64().unwrap_or(0.0).total_cmp(&y.as_f64().unwrap_or(0.0)),
        (Json::Bool(x), Json::Bool(y)) => x.cmp(y),
        (Json::String(x), Json::String(y)) => x.as_bytes().cmp(y.as_bytes()),
        (x, y) => x.to_string().cmp(&y.to_string()),
    }
}

/// `composite`: every combination of its sources' values (`terms`, or a
/// `date_histogram` by a calendar unit), in order, a page of `size` after
/// `after`.
async fn composite(os: &Os, ix: &Index, cx: &Ctx<'_>, pred: Option<String>, body: &Json, subs: Option<&Json>) -> Result<Json, OsError> {
    let size = body.get("size").and_then(Json::as_u64).unwrap_or(10) as usize;
    let sources = body.get("sources").and_then(Json::as_array).ok_or_else(|| OsError::parsing("[composite] requires sources"))?;
    // Each source: its name, value, unit (a date_histogram's), and
    // whether it is descending.
    let mut srcs: Vec<(String, String, Option<strata::timestamp::Unit>, bool, bool)> = Vec::new();
    let mut conds = Vec::new();
    for src in sources {
        let (name, spec) = src.as_object().and_then(|m| m.iter().next()).ok_or_else(|| OsError::parsing("a composite source is {\"name\": {\"terms\": {...}}}"))?;
        let (kind, b) = spec.as_object().and_then(|m| m.iter().next()).ok_or_else(|| OsError::parsing(format!("composite source [{name}] names its type")))?;
        let field = b.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing(format!("composite source [{name}] requires a field")))?;
        let (x, date) = value_of(cx, field)?;
        let desc = b.get("order").and_then(Json::as_str).is_some_and(|o| o.eq_ignore_ascii_case("desc"));
        let (x, unit) = match kind.as_str() {
            "terms" => (x, None),
            "date_histogram" if date => {
                let i = b.get("calendar_interval").or_else(|| b.get("fixed_interval")).or_else(|| b.get("interval")).and_then(Json::as_str).unwrap_or_default();
                let u = calendar_unit(i).ok_or_else(|| OsError::parsing(format!("Strata's composite date_histogram takes a calendar unit (day, week, month, ...), not [{i}]")))?;
                (format!("date_trunc('{}', {x})", u.name()), Some(u))
            }
            k => return Err(OsError::parsing(format!("Strata's composite aggregation takes terms and date_histogram sources, not [{k}]"))),
        };
        if b.get("missing_bucket").and_then(Json::as_bool) != Some(true) {
            conds.push(format!("{} IS NOT NULL", value_of(cx, field)?.0));
        }
        srcs.push((name.clone(), x, unit, desc, date));
    }
    if srcs.is_empty() {
        return Err(OsError::parsing("[composite] requires sources"));
    }
    let p = if conds.is_empty() { pred.clone() } else { with(&pred, conds.join(" AND ")) };
    let cols: Vec<String> = srcs.iter().map(|s| s.1.clone()).collect();
    let groups: Vec<String> = (1..=srcs.len()).map(|i| i.to_string()).collect();
    let order: Vec<String> = srcs.iter().enumerate().map(|(i, s)| format!("{} {}", i + 1, if s.3 { "DESC" } else { "ASC" })).collect();
    let sql = format!("SELECT {}, COUNT(*) FROM {}{} GROUP BY {} ORDER BY {} LIMIT {}", cols.join(", "), ix.table, where_of(&p), groups.join(", "), order.join(", "), COMPOSITE_GROUPS + 1);
    let r = os.rows(&sql, &[]).await?;
    // The keys as OpenSearch writes them (dates in milliseconds).
    let key_of = |row: &[Json]| -> Vec<Json> {
        srcs.iter().enumerate().map(|(i, s)| match (&row[i], s.4) {
            (Json::String(t), true) => date_ms(&json!(t)).map_or(Json::Null, |ms| json!(ms)),
            (v, _) => v.clone(),
        }).collect()
    };
    let after: Option<Vec<Json>> = body.get("after").and_then(Json::as_object).map(|a| srcs.iter().map(|s| a.get(&s.0).cloned().unwrap_or(Json::Null)).collect());
    let past = |k: &[Json]| -> bool {
        let Some(a) = &after else { return true };
        for (i, s) in srcs.iter().enumerate() {
            let o = key_order(&k[i], &a[i]);
            let o = if s.3 { o.reverse() } else { o };
            match o {
                std::cmp::Ordering::Equal => continue,
                o => return o == std::cmp::Ordering::Greater,
            }
        }
        false
    };
    let mut page = Vec::new();
    for row in &r.rows {
        let k = key_of(row);
        if past(&k) {
            page.push((k, row.get(srcs.len()).and_then(Json::as_i64).unwrap_or(0)));
            if page.len() == size {
                break;
            }
        }
    }
    if page.len() < size && r.rows.len() > COMPOSITE_GROUPS {
        return Err(OsError::new(400, "illegal_argument_exception", format!("Strata's composite aggregation pages through at most {COMPOSITE_GROUPS} groups")));
    }
    let mut buckets = Vec::new();
    for (k, n) in &page {
        let key: Map<String, Json> = srcs.iter().zip(k).map(|(s, v)| (s.0.clone(), v.clone())).collect();
        let mut b = Map::new();
        b.insert("key".into(), Json::Object(key));
        b.insert("doc_count".into(), json!(n));
        if let Some(s) = subs {
            let mut cs = Vec::new();
            for (src, v) in srcs.iter().zip(k) {
                let field_x = &src.1;
                cs.push(match (v, src.2) {
                    (Json::Null, _) => format!("{field_x} IS NULL"),
                    (Json::Number(ms), Some(u)) => {
                        // The unit's rows: a range of the date itself.
                        let t = ms.as_i64().unwrap_or(0) * 1000;
                        let col = field_x.trim_start_matches(&format!("date_trunc('{}', ", u.name())).trim_end_matches(')').to_string();
                        format!("{col} >= {} AND {col} < {}", lit(&strata::timestamp::format(t)), lit(&strata::timestamp::format(add_units(t, u, 1))))
                    }
                    (v, _) => format!("{field_x} = {}", key_sql(v, false)),
                });
            }
            subs_into(os, ix, with(&p, cs.join(" AND ")), s, &[], &mut b).await?;
        }
        buckets.push(Json::Object(b));
    }
    let mut out = json!({"buckets": buckets});
    if let Some((k, _)) = page.last() {
        out["after_key"] = Json::Object(srcs.iter().zip(k).map(|(s, v)| (s.0.clone(), v.clone())).collect());
    }
    Ok(out)
}

/// An interval: a calendar unit (`date_trunc`), or a fixed length.
enum Interval {
    Calendar(strata::timestamp::Unit),
    /// Microseconds (dates) or the field's units (numbers).
    Fixed(f64),
}

fn calendar_unit(s: &str) -> Option<strata::timestamp::Unit> {
    use strata::timestamp::Unit;
    Some(match s {
        "minute" | "1m" => Unit::Minute,
        "hour" | "1h" => Unit::Hour,
        "day" | "1d" => Unit::Day,
        "week" | "1w" => Unit::Week,
        "month" | "1M" => Unit::Month,
        "quarter" | "1q" => Unit::Quarter,
        "year" | "1y" => Unit::Year,
        "second" | "1s" => Unit::Second,
        _ => return None,
    })
}

async fn histogram(os: &Os, ix: &Index, cx: &Ctx<'_>, pred: Option<String>, body: &Json, subs: Option<&Json>, dates: bool) -> Result<Json, OsError> {
    use strata::timestamp::Unit;
    let what = if dates { "date_histogram" } else { "histogram" };
    let field = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing(format!("[{what}] requires a field")))?;
    let (x, is_date) = value_of(cx, field)?;
    if dates && !is_date {
        return Err(OsError::parsing(format!("Strata's date_histogram reads a date field; [{field}] is not one (a histogram reads numbers)")));
    }
    if !dates && cx.ix.single_numbers.as_ref().is_some_and(|s| !s.contains(field)) && matches!(cx.target(field), Target::Json(_, Some(Kind::Long | Kind::Double))) {
        return Err(OsError::new(400, "illegal_argument_exception", format!("Strata's histogram reads a number field of one value a document; documents of [{field}] hold arrays of them")));
    }
    let interval = if dates {
        let cal = body.get("calendar_interval").and_then(Json::as_str);
        let fixed = body.get("fixed_interval").or_else(|| body.get("interval")).and_then(Json::as_str);
        match (cal, fixed) {
            (Some(c), _) => Interval::Calendar(calendar_unit(c).ok_or_else(|| OsError::parsing(format!("The supplied interval [{c}] could not be parsed as a calendar interval.")))?),
            (None, Some(f)) => match calendar_unit(f).filter(|u| matches!(u, Unit::Second | Unit::Minute | Unit::Hour | Unit::Day)) {
                Some(u) => Interval::Calendar(u),
                None => Interval::Fixed(super::dsl::duration_secs(f).ok_or_else(|| OsError::parsing(format!("failed to parse setting [date_histogram.fixedInterval] with value [{f}] as a time value")))? * 1e6),
            },
            (None, None) => return Err(OsError::parsing("[date_histogram] requires calendar_interval or fixed_interval")),
        }
    } else {
        let i = body.get("interval").and_then(Json::as_f64).ok_or_else(|| OsError::parsing("[histogram] requires interval"))?;
        if i <= 0.0 {
            return Err(OsError::parsing("[interval] must be >0 for histogram aggregation"));
        }
        Interval::Fixed(i)
    };
    let offset = if dates { body.get("offset").and_then(Json::as_str).and_then(|o| super::dsl::duration_secs(o.trim_start_matches('+'))).unwrap_or(0.0) * 1e6 } else { body.get("offset").and_then(Json::as_f64).unwrap_or(0.0) };
    let min_docs = body.get("min_doc_count").and_then(Json::as_u64).unwrap_or(0);
    let p = with(&pred, format!("{x} IS NOT NULL"));
    // The metrics under it, as columns of the one query: (name, kind,
    // spec, first column, its columns' names).
    let mut folded = Vec::new();
    let mut values = Vec::new();
    if let Some(Json::Object(m)) = subs {
        for (name, s) in m {
            let (k, b, sub_subs) = parts(s)?;
            if FOLDED.contains(&k.as_str()) && sub_subs.is_none() {
                values.push((k.clone(), metric_value(cx, &k, &b)?));
                folded.push((name.clone(), k, b));
            }
        }
    }
    let mut metric_cols = Vec::new();
    let mut at = Vec::new();
    for (j, (k, v)) in values.iter().enumerate() {
        // Over the table itself for calendar buckets, over the value
        // carried through for fixed ones.
        let over = if matches!(interval, Interval::Calendar(_)) { v.clone() } else { format!("m{j}") };
        let cols = metric_cols_of(k, &over);
        at.push((metric_cols.len(), cols.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()));
        metric_cols.extend(cols.into_iter().map(|(_, c)| c));
    }
    let extra: String = metric_cols.iter().map(|c| format!(", {c}")).collect();
    // Bucket keys: microseconds for dates, the values for numbers.
    let sql = match &interval {
        Interval::Calendar(u) => format!("SELECT date_trunc('{}', {x}), COUNT(*){extra} FROM {}{} GROUP BY 1 ORDER BY 1", u.name(), ix.table, where_of(&p)),
        Interval::Fixed(i) => {
            let v = if dates { format!("EXTRACT(epoch FROM {x}) * 1000000") } else { x.clone() };
            let carried: String = values.iter().enumerate().map(|(j, (_, v))| format!(", {v} AS m{j}")).collect();
            let through: String = (0..values.len()).map(|j| format!(", m{j}")).collect();
            // floor(), which Strata's row functions lack, as y - y % 1,
            // less 1 below 0.
            format!(
                "WITH r AS (SELECT ({v} - {off}) / {i} AS y{carried} FROM {}{}), s AS (SELECT (y - (y % 1.0)) - CASE WHEN y % 1.0 < 0 THEN 1.0 ELSE 0.0 END AS b{through} FROM r) SELECT b * {i} + {off} AS k, COUNT(*){extra} FROM s GROUP BY k ORDER BY k",
                ix.table,
                where_of(&p),
                i = num(*i),
                off = num(offset)
            )
        }
    };
    let r = os.rows(&sql, &[]).await?;
    let mut got: Vec<(f64, i64, Vec<Json>)> = Vec::new();
    for row in &r.rows {
        let k = match (&interval, row.first()) {
            (Interval::Calendar(_), Some(v)) => v.as_str().and_then(strata::timestamp::parse).map(|t| t as f64),
            (_, Some(v)) => v.as_f64(),
            _ => None,
        };
        if let Some(k) = k {
            got.push((k, row.get(1).and_then(Json::as_i64).unwrap_or(0), row.iter().skip(2).cloned().collect()));
        }
    }
    // Empty buckets between the first and the last (and the bounds asked
    // for), as min_doc_count 0 asks.
    let next = |k: f64| -> f64 {
        match &interval {
            Interval::Calendar(u) => add_units(k as i64, *u, 1) as f64,
            Interval::Fixed(i) => k + i,
        }
    };
    let align = |v: f64| -> f64 {
        match &interval {
            Interval::Calendar(u) => strata::timestamp::trunc(v as i64, *u).unwrap_or(v as i64) as f64,
            Interval::Fixed(i) => ((v - offset) / i).floor() * i + offset,
        }
    };
    let bound = |v: &Json| -> Option<f64> {
        if dates {
            date_math(v, false).ok().map(|t| t as f64)
        } else {
            v.as_f64()
        }
    };
    let (mut lo, mut hi) = (got.first().map(|g| g.0), got.last().map(|g| g.0));
    if let Some(eb) = body.get("extended_bounds") {
        if let Some(m) = eb.get("min").and_then(bound) {
            lo = Some(lo.map_or(align(m), |l| l.min(align(m))));
        }
        if let Some(m) = eb.get("max").and_then(bound) {
            hi = Some(hi.map_or(align(m), |h| h.max(align(m))));
        }
    }
    let mut keys: Vec<(f64, i64, Vec<Json>)> = Vec::new();
    if min_docs == 0 {
        if let (Some(lo), Some(hi)) = (lo, hi) {
            let mut k = lo;
            let mut i = 0;
            let mut steps = 0;
            while k <= hi + 1e-9 && steps < 100_000 {
                while i < got.len() && got[i].0 < k - 1e-9 {
                    i += 1;
                }
                keys.push(if i < got.len() && (got[i].0 - k).abs() < 1e-6 { (k, got[i].1, got[i].2.clone()) } else { (k, 0, Vec::new()) });
                k = next(k);
                steps += 1;
            }
        }
    } else {
        keys = got.into_iter().filter(|g| g.1 as u64 >= min_docs).collect();
    }
    let names: Vec<String> = folded.iter().map(|f| f.0.clone()).collect();
    let mut buckets = Vec::new();
    for (k, n, vals) in keys {
        let mut b = Map::new();
        let end = next(k);
        if dates {
            let ms = (k / 1000.0).round() as i64;
            b.insert("key_as_string".into(), json!(date_text(ms)));
            b.insert("key".into(), json!(ms));
        } else {
            b.insert("key".into(), json!(k));
        }
        b.insert("doc_count".into(), json!(n));
        for ((name, kind, spec), (first, cols)) in folded.iter().zip(&at) {
            let v: Vec<(String, Json)> = cols.iter().enumerate().map(|(i, c)| (c.clone(), vals.get(first + i).cloned().unwrap_or(Json::Null))).collect();
            b.insert(name.clone(), metric_json(kind, cx, spec, &v));
        }
        if let Some(s) = subs {
            let cond = if dates {
                format!("{x} >= {} AND {x} < {}", lit(&strata::timestamp::format(k as i64)), lit(&strata::timestamp::format(end as i64)))
            } else {
                format!("{x} >= {} AND {x} < {}", num(k), num(end))
            };
            subs_into(os, ix, with(&p, cond), s, &names, &mut b).await?;
        }
        buckets.push(Json::Object(b));
    }
    if body.get("keyed").and_then(Json::as_bool) == Some(true) {
        let m: Map<String, Json> = buckets.into_iter().map(|b| (b.get("key_as_string").and_then(Json::as_str).map(String::from).unwrap_or_else(|| b["key"].to_string()), b)).collect();
        return Ok(json!({"buckets": m}));
    }
    Ok(json!({"buckets": buckets}))
}

async fn ranges(os: &Os, ix: &Index, cx: &Ctx<'_>, pred: Option<String>, body: &Json, subs: Option<&Json>, dates: bool) -> Result<Json, OsError> {
    let what = if dates { "date_range" } else { "range" };
    let field = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing(format!("[{what}] requires a field")))?;
    let (x, is_date) = value_of(cx, field)?;
    let rs = body.get("ranges").and_then(Json::as_array).ok_or_else(|| OsError::parsing(format!("[{what}] requires ranges")))?;
    let keyed = body.get("keyed").and_then(Json::as_bool) == Some(true);
    let mut buckets = Vec::new();
    for r in rs {
        let (from, to) = (r.get("from").filter(|v| !v.is_null()), r.get("to").filter(|v| !v.is_null()));
        let mut conds = Vec::new();
        let mut head = Map::new();
        let fmt = |v: &Json| -> Result<(String, Json, String), OsError> {
            if is_date || dates {
                let t = date_math(v, false).map_err(|w| OsError::parsing(format!("[{what}] bound: {w}")))?;
                let ms = t.div_euclid(1000);
                Ok((lit(&strata::timestamp::format(t)), json!(ms as f64), date_text(ms)))
            } else {
                let f = v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or_else(|| OsError::parsing(format!("[{what}] bound [{v}] is a number")))?;
                Ok((num(f), json!(f), format!("{f:?}")))
            }
        };
        let mut key = String::new();
        match from {
            Some(f) => {
                let (s, j, t) = fmt(f)?;
                conds.push(format!("{x} >= {s}"));
                head.insert("from".into(), j);
                if is_date || dates {
                    head.insert("from_as_string".into(), json!(t));
                }
                key.push_str(&t);
            }
            None => key.push('*'),
        }
        key.push('-');
        match to {
            Some(f) => {
                let (s, j, t) = fmt(f)?;
                conds.push(format!("{x} < {s}"));
                head.insert("to".into(), j);
                if is_date || dates {
                    head.insert("to_as_string".into(), json!(t));
                }
                key.push_str(&t);
            }
            None => key.push('*'),
        }
        let key = r.get("key").and_then(Json::as_str).map(String::from).unwrap_or(key);
        let mut b = Map::new();
        b.insert("key".into(), json!(key));
        b.extend(head);
        let p = if conds.is_empty() { with(&pred, format!("{x} IS NOT NULL")) } else { with(&pred, conds.join(" AND ")) };
        let out = bucket(os, ix, p, b, subs).await?;
        buckets.push(out);
    }
    if keyed {
        // Keyed by their keys, which the buckets then leave out.
        let m: Map<String, Json> = buckets
            .into_iter()
            .map(|mut b| {
                let k = b.as_object_mut().and_then(|o| o.remove("key")).and_then(|k| k.as_str().map(String::from)).unwrap_or_default();
                (k, b)
            })
            .collect();
        return Ok(json!({"buckets": m}));
    }
    Ok(json!({"buckets": buckets}))
}
