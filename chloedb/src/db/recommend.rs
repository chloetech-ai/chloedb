//! Recommenders on a [`Db`] ([`crate::query::recommend`]): `CREATE` and
//! `DROP RECOMMENDER`, the feeds of the counts, and `recommend(...)` in
//! queries.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::{Db, SqlResult};
use crate::error::{Error, Result};
use crate::query::exec::{ExecOptions, ResultSet};
use crate::query::recommend::{self, Call, Def, Deltas, Lists, Reads, Rec, Statement};
use crate::query::{bind_command, function, Command, Params};
use crate::schema::FieldType;
use crate::storage::{DropOptions, Snapshot, FUNCTIONS};
use crate::value::Value;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// What one call found.
pub struct Recommended {
    pub recs: Vec<Rec>,
    /// Whether the items' keys are INT.
    pub int: bool,
    /// The plan line that says what it read.
    pub line: String,
}

impl Db {
    /// The recommenders `CREATE RECOMMENDER` made: the current versions of
    /// the stored functions that hold one.
    pub fn recommenders(&self) -> Result<Vec<Def>> {
        Ok(self.functions()?.currents().filter(|f| !f.dropped).filter_map(|f| Def::of_body(&f.def.body)).collect())
    }

    /// Gives the storage the recommenders' feeds as the functions now
    /// define them.
    pub(crate) fn refresh_feeds(&self) {
        if let Ok(r) = self.recommenders() {
            self.storage.set_feeds(r);
        }
    }

    /// `CREATE RECOMMENDER` or `DROP RECOMMENDER`.
    pub(crate) fn recommender_statement(&self, st: &Statement, opts: &ExecOptions) -> Result<SqlResult> {
        match st {
            Statement::Drop { name, if_exists } => {
                if !self.recommenders()?.iter().any(|d| d.name == *name) {
                    return match if_exists {
                        true => Ok(SqlResult::Written(0)),
                        false => Err(err(format!("no recommender named {name}"))),
                    };
                }
                let r = self.sql_with(&format!("DROP FUNCTION {name}"), &Params::new(), opts);
                self.refresh_feeds();
                r?;
                self.drop_tables(&[&recommend::counts_table(name)], DropOptions { if_exists: true, cascade: false })?;
                Ok(SqlResult::Written(0))
            }
            Statement::Create { def, replace } => self.create_recommender(def, *replace, opts).map(SqlResult::Rows),
        }
    }

    fn create_recommender(&self, def: &Def, replace: bool, opts: &ExecOptions) -> Result<ResultSet> {
        let t0 = Instant::now();
        if let Some(f) = self.functions()?.current(&def.name).filter(|f| !f.dropped) {
            match (Def::of_body(&f.def.body), replace) {
                (None, _) => return Err(err(format!("function {} exists and is not a recommender: name the recommender otherwise", def.name))),
                (Some(_), false) => return Err(err(format!("recommender {} exists: CREATE OR REPLACE RECOMMENDER {} counts its events again", def.name, def.name))),
                (Some(_), true) => self.drop_tables(&[&def.counts()], DropOptions { if_exists: true, cascade: false })?,
            }
        }
        def.check(&self.catalog())?;
        let Command::Function(stmt) = bind_command(&def.function_sql(replace), &[], &Params::new())? else { unreachable!("a function statement") };
        self.create_table(def.counts_schema()?)?;
        self.storage.ensure_functions()?;
        let made = (|| {
            let _ddl = self.function_ddl.lock().unwrap_or_else(|e| e.into_inner());
            let change = function::change(&*self.functions()?, &stmt, Some(crate::timestamp::now()), None)?;
            // Under the writer lock, so that no event comes between the
            // count and the feed.
            let mut t = self.storage.begin()?;
            let snap = self.snapshot();
            if snap.table(&def.events).is_none() {
                return Err(err(format!("no table {}", def.events)));
            }
            let sql = format!(
                "SELECT {}, {}, {} FROM {} LIMIT {}",
                crate::query::sql::ident(&def.user),
                crate::query::sql::ident(&def.item),
                crate::query::sql::ident(&def.ts),
                crate::query::sql::ident(&def.events),
                u32::MAX
            );
            let rs = self.query_at(&snap, &sql, &Params::new(), &ExecOptions { max_rows: None, ..opts.clone() })?;
            let mut evs: Vec<(String, String, Option<i64>)> = rs
                .rows
                .iter()
                .filter_map(|r| match (&r[0], &r[1]) {
                    (Value::Null, _) | (_, Value::Null) => None,
                    (u, i) => Some((text(u), text(i), time(&r[2]))),
                })
                .collect();
            evs.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
            let n_events = evs.len();
            let mut deltas = Deltas::default();
            let mut users: HashMap<String, Vec<(i64, String)>> = HashMap::new();
            for (u, item, ts) in evs {
                deltas.place(def, users.entry(u).or_default(), ts, item);
            }
            let n_users = users.len();
            deltas.write(&mut t, &def.counts(), users)?;
            if let function::Change::Put(f) = &change {
                t.table(FUNCTIONS)?.upsert(f.to_row())?;
            }
            let mut feeds: Vec<Def> = self.recommenders()?.into_iter().filter(|d| d.name != def.name).collect();
            feeds.push(def.clone());
            self.storage.set_feeds(feeds);
            let r = t.commit();
            r.map(|_| (n_events, n_users))
        })();
        self.refresh_feeds();
        let (n_events, n_users) = match made {
            Ok(m) => m,
            Err(e) => {
                let _ = self.drop_tables(&[&def.counts()], DropOptions { if_exists: true, cascade: false });
                return Err(e);
            }
        };
        let counted = self.query(&format!("SELECT count(*) FROM {} WHERE kind = 'pair'", crate::query::sql::ident(&def.counts())), &Params::new())?;
        let pairs = counted.rows.first().and_then(|r| r.first()).cloned().unwrap_or(Value::Int(0));
        let columns = ["recommender", "counts", "events", "users", "pairs", "took_ms"].map(String::from).to_vec();
        let row = vec![
            Value::Str(def.name.clone()),
            Value::Str(def.counts()),
            Value::Int(n_events as i64),
            Value::Int(n_users as i64),
            pairs,
            Value::Float((t0.elapsed().as_secs_f64() * 1e4).round() / 10.0),
        ];
        Ok(ResultSet { columns, rows: vec![row], keys: Vec::new(), scores: Vec::new(), parts: None, stats: Default::default() })
    }

    /// The items recommender `call.name` ranks for `call.user`, as of `snap`.
    pub fn recommend_at(&self, snap: &Snapshot, call: &Call, opts: &ExecOptions) -> Result<Recommended> {
        let d = self
            .recommenders()?
            .into_iter()
            .find(|d| d.name == call.name)
            .ok_or_else(|| err(format!("no recommender named {}: CREATE RECOMMENDER {} ON events(user, item, time) makes one", call.name, call.name)))?;
        let o = ExecOptions { max_rows: None, ..opts.clone() };
        let key = |k: String| Params(HashMap::from([("k".to_string(), Value::Str(k))]));
        let pairs = |rs: &ResultSet| -> Vec<(String, f64)> { rs.rows.iter().filter_map(|r| Some((r.first()?.as_str()?.to_string(), r.get(1)?.as_f64()?))).collect() };
        let recent: Vec<String> = match &call.user {
            Value::Null => Vec::new(),
            u => {
                let rs = self.query_at(snap, &Reads::user(&d), &key(recommend::user_key(&text(u))), &o)?;
                rs.rows.first().and_then(|r| r.first()).map(recommend::parse_recent).unwrap_or_default().into_iter().map(|(_, i)| i).collect()
            }
        };
        // Most recent first, each item once.
        let mut order: Vec<String> = Vec::new();
        for i in recent.iter().rev() {
            if !order.contains(i) {
                order.push(i.clone());
            }
        }
        let mut lists = Lists::default();
        // What came next after the last item, and (backed off) the one before.
        let mut next: HashMap<String, f64> = HashMap::new();
        for (w, h) in [1.0, 0.4].iter().zip(&order) {
            let rs = self.query_at(snap, &Reads::pairs(&d, "next"), &key(h.clone()), &o)?;
            for (i, s) in recommend::shares(&pairs(&rs)) {
                *next.entry(i).or_default() += w * s;
            }
        }
        lists.next = recommend::ranked_by(next);
        // Seen together with each recent item, the newer weighing more.
        let mut co: HashMap<String, f64> = HashMap::new();
        for (g, h) in order.iter().enumerate() {
            let rs = self.query_at(snap, &Reads::pairs(&d, "co"), &key(h.clone()), &o)?;
            for (i, s) in recommend::shares(&pairs(&rs)) {
                *co.entry(i).or_default() += s / (g + 1) as f64;
            }
        }
        lists.co = recommend::ranked_by(co);
        let items_schema = d.items.as_ref().and_then(|t| snap.schemas().find(|s| s.table.eq_ignore_ascii_case(t)).cloned());
        if let (Some(s), true) = (&items_schema, call.opts.similar > 0.0) {
            if let Some(sql) = Reads::similar(&d, s, &recent) {
                let rs = self.query_at(snap, &sql, &Params::new(), &o)?;
                lists.similar = rs.rows.iter().filter_map(|r| r.first()).filter(|v| !matches!(v, Value::Null)).map(text).collect();
            }
        }
        if call.opts.popular > 0.0 {
            let rs = self.query_at(snap, &Reads::popular(&d, call.opts.k + order.len()), &Params::new(), &o)?;
            lists.popular = pairs(&rs).into_iter().map(|(i, _)| i).collect();
        }
        let seen: HashSet<String> = order.iter().cloned().collect();
        let recs = recommend::fuse(&lists, &seen, &call.opts);
        let int = snap.schemas().find(|s| s.table.eq_ignore_ascii_case(&d.events)).and_then(|s| s.field(&d.item)).is_some_and(|f| f.ty == FieldType::Int);
        let line = format!(
            "recommend {} for {}: {} items, from {} next, {} seen together, {} similar, {} popular; {} of the user's items left out",
            d.name,
            crate::query::hint::sql_value(&call.user),
            recs.len(),
            lists.next.len(),
            lists.co.len(),
            lists.similar.len(),
            lists.popular.len(),
            seen.len()
        );
        Ok(Recommended { recs, int, line })
    }

    /// A query that calls `recommend`: `FROM recommend(...)`'s rows, or the
    /// query with each call as `ranked(...)` of its keys; `None` for one
    /// that calls none.
    pub fn recommend_sql_at(&self, snap: &Snapshot, q: &sqlparser::ast::Query, params: &Params, opts: &ExecOptions) -> Result<Option<RecommendSql>> {
        if let Some((call, cols)) = recommend::table_call(q, params)? {
            let r = self.recommend_at(snap, &call, opts)?;
            let mut stats: crate::query::exec::ExecStats = Default::default();
            stats.plan.push(r.line.clone());
            return Ok(Some(RecommendSql::Rows(ResultSet { rows: recommend::table_rows(&r.recs, &cols, r.int), columns: cols, keys: Vec::new(), scores: Vec::new(), parts: None, stats })));
        }
        let calls = recommend::calls(q, params)?;
        if calls.is_empty() {
            return Ok(None);
        }
        let (mut keys, mut lines) = (Vec::new(), Vec::new());
        for c in &calls {
            let r = self.recommend_at(snap, c, opts)?;
            let mut ks: Vec<Value> = r.recs.iter().map(|x| recommend::item_value(&x.item, r.int)).collect();
            // Nothing to rank by: a key of no row, which ranks every row last.
            if ks.is_empty() {
                ks.push(if r.int { Value::Int(i64::MIN) } else { Value::Str("\u{1f}none".into()) });
            }
            keys.push(ks);
            lines.push(r.line);
        }
        Ok(Some(RecommendSql::Query { sql: recommend::rewrite(q, &keys).to_string(), lines }))
    }

    /// [`Db::recommend_sql_at`] of the text `sql`, on the latest snapshot:
    /// what a server runs in its place.
    pub fn recommend_sql(&self, sql: &str, params: &Params) -> Result<Option<RecommendSql>> {
        if !recommend::may_recommend(sql) {
            return Ok(None);
        }
        let q = crate::query::sql::parse_query(sql)?;
        self.recommend_sql_at(&self.snapshot(), &q, params, &self.exec)
    }

    pub(crate) fn recommend_query(&self, snap: &Snapshot, q: &sqlparser::ast::Query, params: &Params, opts: &ExecOptions) -> Result<Option<ResultSet>> {
        match self.recommend_sql_at(snap, q, params, opts)? {
            None => Ok(None),
            Some(RecommendSql::Rows(rs)) => Ok(Some(rs)),
            Some(RecommendSql::Query { sql, lines }) => {
                let mut rs = self.query_at(snap, &sql, params, opts)?;
                rs.stats.plan.splice(0..0, lines);
                Ok(Some(rs))
            }
        }
    }
}

/// What a query that calls `recommend` is ([`Db::recommend_sql`]).
pub enum RecommendSql {
    /// The rows of `FROM recommend(...)`.
    Rows(ResultSet),
    /// The query to run in its place, and the plan lines of its calls.
    Query { sql: String, lines: Vec<String> },
}

fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        v => crate::query::hint::sql_value(v),
    }
}

fn time(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) | Value::Timestamp(n) => Some(*n),
        Value::Float(x) if x.is_finite() => Some(*x as i64),
        _ => None,
    }
}
