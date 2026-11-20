//! `POST /v1/sql/check`: whether a statement would run on this
//! deployment (one server, or a coordinator over shards), without running
//! it, so that a pipeline's CI can test its SQL against the cluster it
//! deploys to. It binds the statement against the tables served, plans a
//! read, and applies what a coordinator refuses or does differently
//! (docs/chloedb/cluster-limits.md).

use serde_json::{json, Value as Json};

use chloedb::query::{bind_command, Command, Conflict, Params, Write};
use chloedb::schema::Alter;

use crate::service::Service;
use crate::ApiError;

/// `{runs, deployment, kind, error?, notes}`: `runs` false with the
/// reason the statement would be refused here, `notes` what runs
/// differently on a cluster.
pub fn check(svc: &Service, sql: &str, params: &Params) -> Json {
    let cluster = svc.coordinator().is_ok();
    let deployment = if cluster { "cluster" } else { "single" };
    let answer = |kind: &str, r: Result<Vec<String>, ApiError>| match r {
        Ok(notes) => json!({"runs": true, "deployment": deployment, "kind": kind, "notes": notes}),
        Err(e) => json!({"runs": false, "deployment": deployment, "kind": kind, "error": e.message, "status": e.status, "notes": []}),
    };
    let catalog = svc.catalog();
    let tables: Vec<&chloedb::Schema> = catalog.iter().map(|t| &**t).collect();
    // `INSERT ... SELECT` and `CREATE TABLE ... AS SELECT`: its SELECT is
    // planned, and its table looked for (SAM-8).
    match chloedb::query::from_select::parse_statement(sql, params) {
        Err(e) => return answer("unknown", Err(e.into())),
        Ok(Some(st)) => {
            let kind = if st.creates() { "ddl" } else { "write" };
            let there = tables.iter().any(|t| t.table.eq_ignore_ascii_case(st.table()));
            let r = match (st.creates(), there) {
                (true, true) if !st.if_not_exists() => Err(ApiError::bad(format!("query error: table {} already exists", st.table()))),
                (false, false) => Err(ApiError::bad(format!("query error: {}", chloedb::query::hint::no_table(st.table(), tables.iter().map(|t| t.table.as_str()))))),
                _ => {
                    let profiles = |t: &str, n: &str| svc.rank_profile(t, n);
                    chloedb::query::compound::plan_read_calling(&st.query_sql(), &tables, params, &profiles, &svc.functions()).map_err(ApiError::from).map(|_| match cluster {
                        true => vec![format!("runs its SELECT through the coordinator, then writes the rows as INSERTs of {} rows each: each is a write of its own, not one transaction", chloedb::query::from_select::BATCH)],
                        false => Vec::new(),
                    })
                }
            };
            return answer(kind, r);
        }
        Ok(None) => {}
    }
    let command = match bind_command(sql, &tables, params) {
        Ok(c) => c,
        Err(e) => return answer("unknown", Err(e.into())),
    };
    match command {
        Command::Query => {
            let profiles = |t: &str, n: &str| svc.rank_profile(t, n);
            let r = chloedb::query::compound::plan_read_calling(sql, &tables, params, &profiles, &svc.functions()).map_err(ApiError::from).and_then(|plan| {
                if !cluster || plan.root.uses_operators() {
                    return Ok(Vec::new());
                }
                let schema = tables.iter().find(|t| t.table.eq_ignore_ascii_case(plan.table())).ok_or_else(|| ApiError::bad(format!("query error: no such table: {}", plan.table())))?;
                let select = chloedb::query::bind(&chloedb::query::profile::written_out(sql, &profiles)?, schema, params)?;
                if !select.graphs.is_empty() {
                    return Err(ApiError::bad(crate::coord::GRAPHS_ON_SHARDS));
                }
                Ok(Vec::new())
            });
            answer("query", r)
        }
        Command::Explain { .. } => answer("explain", Err(ApiError::bad("query error: EXPLAIN runs through POST /v1/explain"))),
        Command::ShowTables => answer("query", Ok(Vec::new())),
        Command::AlterTable { alters, .. } if cluster && alters.iter().any(|a| matches!(a, Alter::DropField(_))) => {
            answer("ddl", Err(ApiError::bad("ALTER TABLE ... DROP COLUMN does not run through a coordinator yet (its plans name columns by place); ADD COLUMN and RENAME COLUMN do")))
        }
        Command::CreateTable { .. } | Command::DropTable { .. } | Command::AlterTable { .. } | Command::CreateRankProfile { .. } | Command::DropRankProfile { .. } | Command::Function(_) => {
            let notes = match cluster {
                true => vec!["runs on every shard, one change per shard: a shard that fails leaves the others changed (run it again with IF NOT EXISTS, IF EXISTS or OR REPLACE)".to_string()],
                false => Vec::new(),
            };
            answer("ddl", Ok(notes))
        }
        Command::Write { table, write } => {
            // An UPDATE or DELETE whose rows a query picks runs when that
            // SELECT would.
            if let Some(p) = write.pick() {
                let profiles = |t: &str, n: &str| svc.rank_profile(t, n);
                if let Err(e) = chloedb::query::compound::plan_read_calling(&p.sql(), &tables, &p.params, &profiles, &svc.functions()) {
                    return answer("write", Err(e.into()));
                }
            }
            let mut notes = Vec::new();
            let placed = catalog.iter().find(|t| t.table == table).is_some_and(|t| t.shard_by.is_some() && t.shard_by_field() != t.primary_key);
            if cluster {
                match &write {
                    Write::Insert { conflict: Conflict::Update(_), docs } => {
                        notes.push(format!("ON CONFLICT DO UPDATE through a coordinator runs when all its rows go to one shard ({} rows here); rows of several shards are refused", docs.len()));
                    }
                    Write::Insert { conflict: Conflict::Fail | Conflict::Skip, .. } if placed => {
                        notes.push("a row of a table placed by shard_by is checked against the other shards for its key first: one key read a shard".to_string());
                    }
                    Write::Insert { conflict: Conflict::Replace, .. } if placed => {
                        notes.push("INSERT OR REPLACE replaces a row only on the shard of its shard_by value: a row with the key and another shard_by value stays".to_string());
                    }
                    w @ (Write::Update { .. } | Write::Delete { .. }) if w.limited() => {
                        notes.push("UPDATE or DELETE with a LIMIT through a coordinator picks its rows by key first, then changes those that still match: two steps, not one".to_string());
                    }
                    w @ (Write::Update { .. } | Write::Delete { .. }) if w.picks_first() => {
                        notes.push(format!("UPDATE or DELETE whose WHERE has a subquery, through a coordinator, picks the keys of its rows first (at most {}), then changes the rows of those keys: two steps, not one", crate::service::MAX_GET_KEYS));
                    }
                    _ => {}
                }
            }
            answer("write", Ok(notes))
        }
    }
}
