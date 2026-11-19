//! Blends on a [`Db`] ([`crate::query::blend`]): `CREATE` and `DROP BLEND`,
//! and `FROM blend(...)` in queries, each source a query of this database.

use std::sync::Arc;
use std::time::Instant;

use super::{Db, SqlResult};
use crate::error::{Error, Result};
use crate::query::blend::{self, Blended, Def, Input, Statement, Want};
use crate::query::exec::{ExecOptions, ResultSet};
use crate::query::function::Function;
use crate::query::Params;
use crate::rank_log::{self, Log, Logged};
use crate::storage::Snapshot;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

impl Db {
    /// The blends `CREATE BLEND` made: each current version's definition,
    /// and the stored function that holds it.
    pub fn blends(&self) -> Result<Vec<(Def, Arc<Function>)>> {
        Ok(blend::defs(&*self.functions()?))
    }

    /// `CREATE BLEND` or `DROP BLEND`: a stored function written or
    /// dropped.
    pub(crate) fn blend_statement(&self, st: &Statement, opts: &ExecOptions) -> Result<SqlResult> {
        let set = self.functions()?;
        match st {
            Statement::Drop { name, if_exists } => match set.current(name).filter(|f| !f.dropped) {
                Some(f) if Def::of_body(&f.def.body).is_some() => {
                    self.sql_with(&format!("DROP FUNCTION {name}"), &Params::new(), opts)?;
                    Ok(SqlResult::Written(0))
                }
                Some(_) => Err(err(format!("function {name} is not a blend: DROP FUNCTION drops it"))),
                None if *if_exists => Ok(SqlResult::Written(0)),
                None => Err(err(format!("no blend named {name}"))),
            },
            Statement::Create { def, replace } => {
                blend::may_define(&set, def, *replace)?;
                self.sql_with(&def.function_sql(*replace), &Params::new(), opts)?;
                let v = self.functions()?.current(&def.name).map_or(0, |f| f.version);
                Ok(SqlResult::Rows(blend::made(def, v)))
            }
        }
    }

    /// `SELECT ... FROM blend(...)`, or `None` for a query that reads no
    /// blend.
    pub(crate) fn blend_query(&self, snap: &Snapshot, q: &sqlparser::ast::Query, params: &Params, opts: &ExecOptions) -> Result<Option<ResultSet>> {
        let Some((call, cols)) = blend::table_call(q, params)? else { return Ok(None) };
        let (def, version) = blend::resolve(&*self.functions_at(snap)?, &call.name, call.version)?;
        let b = self.blend_at(snap, &def, version, &call.params, call.limit.unwrap_or(def.k), Want::default(), opts)?;
        b.result(&cols).map(Some)
    }

    /// Runs blend `def` (as version `version`) into `k` slots: each source
    /// with `params`, as of `snap`, then the fill.
    pub fn blend_at(&self, snap: &Snapshot, def: &Def, version: i64, params: &Params, k: usize, want: Want, opts: &ExecOptions) -> Result<Blended> {
        let mut inputs = Vec::with_capacity(def.sources.len());
        for s in &def.sources {
            let t0 = Instant::now();
            let rs = self.query_at(snap, &s.sql, params, opts).map_err(|e| e.within(&format!("blend {} source {}", def.name, s.name)))?;
            inputs.push(Input { rs, took_ms: t0.elapsed().as_secs_f64() * 1e3 });
        }
        blend::blend(def, version, k, inputs, want)
    }

    /// `SELECT ... FROM blend(...)` run and logged ([`Db::query_logged`]):
    /// the fill explores when `log` does ([`crate::query::blend::Explore`],
    /// drawn from the request's stream), and every row of every source is
    /// logged ([`Blended::log_entries`]) in one transaction.
    pub(crate) fn blend_logged(&self, sql: &str, params: &Params, log: &Log, schema: &crate::schema::Schema, call: blend::Call, cols: &[(String, String)]) -> Result<Logged> {
        rank_log::fits_blend(log)?;
        let explore = match &log.explore {
            Some(p) => blend::Explore::of_policy(p)?,
            None => None,
        };
        let request = match &log.request {
            Some(r) if r.is_empty() => return Err(err("a log's request id is not empty")),
            Some(r) => r.clone(),
            None => format!("{:032x}", rand::random::<u128>()),
        };
        let started = Instant::now();
        let snap = self.snapshot();
        let (def, version) = blend::resolve(&*self.functions_at(&snap)?, &call.name, call.version)?;
        let want = Want { costs: false, explore: explore.map(|e| (e, blend::seed(&request))) };
        let b = self.blend_at(&snap, &def, version, &call.params, call.limit.unwrap_or(def.k), want, &self.exec)?;
        let rs = b.result(cols)?;
        let mut entries = b.log_entries(&def, &request, log.ts.unwrap_or_else(crate::timestamp::now), log);
        rank_log::keep_rows(&mut entries, &rank_log::query_json(sql, params), schema, started.elapsed().as_secs_f64() * 1000.0);
        let now = crate::timestamp::now();
        let mut t = self.storage.begin()?;
        {
            let mut tt = t.table(&schema.table)?;
            for e in &entries {
                tt.upsert(crate::ingest::document_at(schema, &e.to_json(), now)?)?;
            }
        }
        let seq_no = t.commit_seq()?;
        let propensities = match explore {
            Some(_) => b.placed.iter().map(|p| p.chance).collect(),
            None => Vec::new(),
        };
        Ok(Logged { rs, request, seq_no, teams: Vec::new(), propensities })
    }

    /// The logged requests of blend `name` in a log table, read from one
    /// snapshot ([`crate::query::blend_eval`]): those that explored, with
    /// every source's rows and the page shown, ready to value.
    pub fn blend_requests(&self, x: &crate::rank_log::Export, name: &str) -> Result<crate::query::blend_eval::Gathered> {
        let mut logs = crate::query::blend_eval::Logs::default();
        self.export_rank_log(x, |r| {
            logs.add(&r);
            Ok(())
        })?;
        Ok(logs.finish(name))
    }

    /// A blend's rows and receipts on the latest snapshot: a stored
    /// version, or a draft not stored (version 0).
    pub fn blend_run(&self, def: &Def, version: i64, params: &Params, k: usize, want: Want) -> Result<Blended> {
        def.check()?;
        self.blend_at(&self.snapshot(), def, version, params, k, want, &self.exec)
    }
}
