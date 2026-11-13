//! The Inspector page's API: the memory Claude Code sessions recorded, as
//! `chloectx sync` copies it into the cluster's table [`TABLE`]
//! (`chloectx_memory`, one row per record, keyed `<project>:<id>`), read
//! the way `chloectx ui` reads a local store: by project, by session in
//! the order it was recorded, by kind, and by search.
//!
//! | call | answer |
//! |---|---|
//! | `GET /v1/memory` | `{table, exists, projects: [{project, records, sessions, last}], create}` (`create`: the table's CREATE TABLE) |
//! | `GET /v1/memory/sessions?project=` | `{sessions: [{session, records, first, last, turns}]}`, the latest first |
//! | `GET /v1/memory/records?project=&session=&kind=&q=&limit=&before=` | `{records}`: a session's in the order recorded, else the latest first (`before`: a `seq` to page back from), or the best matches for `q` |
//!
//! Only reads: the page changes nothing, and a key that may only read
//! `chloectx_memory` is enough.

use std::collections::HashMap;

use chloedb::query::Params;
use chloedb::Value;
use serde_json::{json, Value as Json};

use super::{bad, Answer, App};
use crate::sync::TABLE;

/// The most records one call answers.
const MAX_RECORDS: usize = 500;
/// The most characters of a record's text and context an answer carries.
const MAX_TEXT: usize = 4000;

const COLUMNS: &str = "rid, session, kind, tag, turn, seq, ts, importance, files, content, context, links_json";

fn str_of(v: Option<&Value>) -> String {
    v.and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn int_of(v: Option<&Value>) -> i64 {
    v.and_then(|v| v.as_i64()).unwrap_or(0)
}

fn clip(s: String) -> (String, bool) {
    if s.chars().count() <= MAX_TEXT {
        return (s, false);
    }
    (s.chars().take(MAX_TEXT).collect(), true)
}

fn record(row: &[Value]) -> Json {
    let (text, cut) = clip(str_of(row.get(9)));
    let (context, _) = clip(str_of(row.get(10)));
    let links: Json = serde_json::from_str(&str_of(row.get(11))).unwrap_or(json!([]));
    json!({
        "id": str_of(row.first()), "session": str_of(row.get(1)), "kind": str_of(row.get(2)), "tag": str_of(row.get(3)),
        "turn": int_of(row.get(4)), "seq": int_of(row.get(5)), "ts": int_of(row.get(6)),
        "importance": row.get(7).and_then(|v| v.as_f64()).unwrap_or(0.0), "files": str_of(row.get(8)),
        "text": text, "cut": cut, "context": context, "links": links,
    })
}

impl App {
    fn memory_exists(&self) -> Result<bool, super::Fail> {
        Ok(self.cluster.tables()?.iter().any(|t| t == TABLE))
    }

    /// `GET /v1/memory`: the projects synced to the cluster.
    pub fn memory(&self) -> Answer {
        if !self.memory_exists()? {
            return Ok(json!({"table": TABLE, "exists": false, "projects": [], "create": crate::sync::CREATE}));
        }
        let rs = self.cluster.query(&format!("SELECT project, COUNT(*), COUNT(DISTINCT session), MAX(ts) FROM {TABLE} GROUP BY project LIMIT 1000"), &Params::new())?;
        let mut projects: Vec<Json> =
            rs.rows.iter().map(|r| json!({"project": str_of(r.first()), "records": int_of(r.get(1)), "sessions": int_of(r.get(2)), "last": int_of(r.get(3))})).collect();
        projects.sort_by_key(|p| std::cmp::Reverse(p["last"].as_i64().unwrap_or(0)));
        Ok(json!({"table": TABLE, "exists": true, "projects": projects, "create": crate::sync::CREATE}))
    }

    /// `GET /v1/memory/sessions?project=`: a project's sessions, the
    /// latest first.
    pub fn memory_sessions(&self, project: &str) -> Answer {
        if project.is_empty() {
            return Err(bad("which project? (?project=owner/name)"));
        }
        if !self.memory_exists()? {
            return Ok(json!({"sessions": []}));
        }
        let rs = self.cluster.query(
            &format!("SELECT session, COUNT(*), MIN(ts), MAX(ts), MAX(turn) FROM {TABLE} WHERE project = :p GROUP BY session LIMIT 5000"),
            &Params::new().set("p", Value::Str(project.to_string())),
        )?;
        let mut sessions: Vec<Json> = rs
            .rows
            .iter()
            .map(|r| json!({"session": str_of(r.first()), "records": int_of(r.get(1)), "first": int_of(r.get(2)), "last": int_of(r.get(3)), "turns": int_of(r.get(4))}))
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s["last"].as_i64().unwrap_or(0)));
        // What each kind of the project counts, for the page's filters.
        let kinds = self.cluster.query(&format!("SELECT kind, COUNT(*) FROM {TABLE} WHERE project = :p GROUP BY kind LIMIT 50"), &Params::new().set("p", Value::Str(project.to_string())))?;
        let by_kind: HashMap<String, i64> = kinds.rows.iter().map(|r| (str_of(r.first()), int_of(r.get(1)))).collect();
        Ok(json!({"project": project, "sessions": sessions, "by_kind": by_kind}))
    }

    /// `GET /v1/memory/records`: a session's records in the order they were
    /// recorded; else the project's latest (of a kind), or its best matches
    /// for `q`.
    pub fn memory_records(&self, project: &str, session: Option<&str>, kind: Option<&str>, q: Option<&str>, limit: usize, before: Option<i64>) -> Answer {
        if project.is_empty() {
            return Err(bad("which project? (?project=owner/name)"));
        }
        if !self.memory_exists()? {
            return Ok(json!({"records": []}));
        }
        let limit = limit.clamp(1, MAX_RECORDS);
        let mut conds = vec!["project = :p".to_string()];
        let mut params = Params::new().set("p", Value::Str(project.to_string()));
        if let Some(s) = session.filter(|s| !s.is_empty()) {
            conds.push("session = :s".into());
            params = params.set("s", Value::Str(s.to_string()));
        }
        if let Some(k) = kind.filter(|k| !k.is_empty()) {
            if crate::record::Kind::parse(k).is_none() {
                return Err(bad(format!("no kind {k:?}")));
            }
            conds.push("kind = :k".into());
            params = params.set("k", Value::Str(k.to_string()));
        }
        if let Some(b) = before {
            conds.push("seq < :b".into());
            params = params.set("b", Value::Int(b));
        }
        let wher = conds.join(" AND ");
        let q = q.map(str::trim).filter(|q| !q.is_empty());
        let sql = match q {
            Some(_) => format!("SELECT {COLUMNS} FROM {TABLE} WHERE {wher} ORDER BY bm25(content, :q, syntax => 'plain') DESC LIMIT {limit}"),
            None if session.is_some_and(|s| !s.is_empty()) => format!("SELECT {COLUMNS} FROM {TABLE} WHERE {wher} ORDER BY seq ASC LIMIT {limit}"),
            None => format!("SELECT {COLUMNS} FROM {TABLE} WHERE {wher} ORDER BY seq DESC LIMIT {limit}"),
        };
        if let Some(q) = q {
            params = params.set("q", Value::Str(q.to_string()));
        }
        let rs = self.cluster.query(&sql, &params)?;
        let records: Vec<Json> = rs.rows.iter().map(|r| record(r)).collect();
        Ok(json!({"project": project, "session": session, "records": records, "more": records.len() == limit}))
    }
}
