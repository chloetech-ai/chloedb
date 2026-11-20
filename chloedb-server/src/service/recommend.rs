//! Recommenders through a coordinator ([`chloedb::query::recommend`]).
//!
//! `CREATE RECOMMENDER` runs on every shard, and each shard counts the
//! events it holds, as they are written, in its own rows of the counts
//! table: under keys that end with its part, the shard and the slot map it
//! holds ([`chloedb::query::recommend::Def::part`]), which no other shard
//! writes. On a cluster of several shards the events table is placed by
//! its user column (`OPTIONS(shard_by = 'usr')`), so that each user's
//! events are counted in order on one shard. `recommend()` then reads the
//! counts as one database does, but summed over the shards' rows: what
//! came after an item and what was seen with it are `sum(next)` and
//! `sum(co)` of its pairs, grouped by the other item, popularity is
//! `sum(n)`, and a user's recent events are those of every row of theirs
//! (one, unless their slot moved). The call becomes `ranked(...)` of the
//! keys, which every shard ranks alike.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use chloedb::db::{RecommendSql, Recommended};
use chloedb::query::exec::ResultSet;
use chloedb::query::recommend::{self, Call, Def, Lists, Statement};
use chloedb::query::sql::ident;
use chloedb::query::Params;
use chloedb::schema::FieldType;
use chloedb::value::Value;
use futures::future::join_all;

use super::{Answer, QueryReq, Service, SqlAnswer, Written};
use crate::coord::Coordinator;
use crate::ApiError;

/// Candidates each list gives at most, as one database's.
const DEPTH: usize = 200;

fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        v => chloedb::query::hint::sql_value(v),
    }
}

impl Service {
    /// `CREATE` or `DROP RECOMMENDER` through coordinator `c`: on every
    /// shard's primary, each counting the events it holds. Over several
    /// shards the events table is placed by its user column.
    pub(super) async fn cluster_recommender(self: &Arc<Self>, c: &Coordinator, sql: &str, st: &Statement) -> Result<SqlAnswer, ApiError> {
        let t0 = Instant::now();
        let catalog = c.catalog();
        if let Statement::Create { def, .. } = st {
            let ev = catalog.iter().find(|s| s.table.eq_ignore_ascii_case(&def.events)).ok_or_else(|| ApiError::bad(format!("query error: no table {}", def.events)))?;
            let user = ev.field_index(&def.user);
            if c.shard_count() > 1 && (user.is_none() || ev.shard_by_field() != user) {
                return Err(ApiError::bad(format!(
                    "a recommender through a coordinator of several shards counts each user's events on the shard that holds them: place {} by its user column (CREATE TABLE {} (...) OPTIONS(shard_by = '{}')), and load its events again",
                    ev.table, ev.table, def.user
                )));
            }
        }
        c.ddl(sql, &Params::new()).await?;
        self.learn_tables(true).await;
        let took_ms = t0.elapsed().as_secs_f64() * 1e3;
        let Statement::Create { def, .. } = st else { return Ok(SqlAnswer::Written { w: Written::default(), took_ms }) };
        // What one database answers, counted over the shards.
        let count = |sql: String| async move {
            let req = QueryReq { sql, ..Default::default() };
            let a = Box::pin(self.query(req)).await?;
            Ok::<Value, ApiError>(a.rs.rows.first().and_then(|r| r.first()).cloned().unwrap_or(Value::Int(0)))
        };
        let counts = ident(&def.counts());
        let events = count(format!("SELECT count(*) FROM {}", ident(&def.events))).await?;
        let users = count(format!("SELECT count(*) FROM {counts} WHERE kind = 'user'")).await?;
        let pairs = count(format!("SELECT count(*) FROM {counts} WHERE kind = 'pair'")).await?;
        let columns = ["recommender", "counts", "events", "users", "pairs", "took_ms"].map(String::from).to_vec();
        let row = vec![Value::Str(def.name.clone()), Value::Str(def.counts()), events, users, pairs, Value::Float((t0.elapsed().as_secs_f64() * 1e4).round() / 10.0)];
        let rs = ResultSet { columns, rows: vec![row], keys: Vec::new(), scores: Vec::new(), parts: None, stats: Default::default() };
        Ok(SqlAnswer::Rows(Answer { rs, took_ms, kind: "recommender", missing: Vec::new(), lease: 0, arrow: Vec::new(), stale: false, cursor: None }))
    }

    /// A query that calls `recommend(...)` through a coordinator, as
    /// [`chloedb::Db::recommend_sql_at`] answers it on one database: the
    /// rows of `FROM recommend(...)`, or the query with each call as
    /// `ranked(...)` of its keys. `None` for one that calls none.
    pub(super) async fn cluster_recommend_sql(self: &Arc<Self>, req: &QueryReq) -> Result<Option<RecommendSql>, ApiError> {
        let q = chloedb::query::sql::parse_query(&req.sql)?;
        if let Some((call, cols)) = recommend::table_call(&q, &req.params)? {
            let r = self.cluster_recommend(req, &call).await?;
            let mut stats: chloedb::query::exec::ExecStats = Default::default();
            stats.plan.push(r.line.clone());
            return Ok(Some(RecommendSql::Rows(ResultSet { rows: recommend::table_rows(&r.recs, &cols, r.int), columns: cols, keys: Vec::new(), scores: Vec::new(), parts: None, stats })));
        }
        let calls = recommend::calls(&q, &req.params)?;
        if calls.is_empty() {
            return Ok(None);
        }
        let (mut keys, mut lines) = (Vec::new(), Vec::new());
        for c in &calls {
            let r = self.cluster_recommend(req, c).await?;
            let mut ks: Vec<Value> = r.recs.iter().map(|x| recommend::item_value(&x.item, r.int)).collect();
            if ks.is_empty() {
                ks.push(if r.int { Value::Int(i64::MIN) } else { Value::Str("\u{1f}none".into()) });
            }
            keys.push(ks);
            lines.push(r.line);
        }
        Ok(Some(RecommendSql::Query { sql: recommend::rewrite(&q, &keys).to_string(), lines }))
    }

    /// The items recommender `call.name` ranks for `call.user`, from the
    /// counts of every shard.
    async fn cluster_recommend(self: &Arc<Self>, req: &QueryReq, call: &Call) -> Result<Recommended, ApiError> {
        let d: Def = self
            .functions()
            .currents()
            .filter(|f| !f.dropped)
            .filter_map(|f| Def::of_body(&f.def.body))
            .find(|d| d.name == call.name)
            .ok_or_else(|| ApiError::bad(format!("query error: no recommender named {}: CREATE RECOMMENDER {} ON events(user, item, time) makes one", call.name, call.name)))?;
        let counts = ident(&d.counts());
        let read = |sql: String, params: Params| {
            let r = QueryReq { sql, params, scroll: None, ..req.clone() };
            async move { Box::pin(self.query(r)).await.map(|a| a.rs) }
        };
        let key = |k: String| Params(HashMap::from([("k".to_string(), Value::Str(k))]));
        // The user's recent events: every row of theirs, oldest first, the
        // last `history`.
        let recent: Vec<String> = match &call.user {
            Value::Null => Vec::new(),
            u => {
                let rs = read(format!("SELECT recent FROM {counts} WHERE kind = 'user' AND src = :k LIMIT 1000"), key(text(u))).await?;
                let mut evs: Vec<(i64, String)> = rs.rows.iter().filter_map(|r| r.first()).flat_map(recommend::parse_recent).collect();
                evs.sort_by_key(|e| e.0);
                let skip = evs.len().saturating_sub(d.history);
                evs.into_iter().skip(skip).map(|(_, i)| i).collect()
            }
        };
        let mut order: Vec<String> = Vec::new();
        for i in recent.iter().rev() {
            if !order.contains(i) {
                order.push(i.clone());
            }
        }
        // Each pair's count is the sum of its rows over the shards.
        let pairs_sql = |field: &str| format!("SELECT dst, sum({field}) AS c FROM {counts} WHERE kind = 'pair' AND src = :k AND {field} != 0 GROUP BY dst ORDER BY c DESC, dst LIMIT {DEPTH}");
        // As one database reads them: the pairs counted above 0.
        let positive = |ps: Vec<(String, f64)>| -> Vec<(String, f64)> { ps.into_iter().filter(|p| p.1 > 0.0).collect() };
        let pairs = |rs: &ResultSet| -> Vec<(String, f64)> { rs.rows.iter().filter_map(|r| Some((r.first()?.as_str()?.to_string(), r.get(1)?.as_f64()?))).collect() };
        let next_of: Vec<_> = [1.0, 0.4].into_iter().zip(order.iter()).map(|(w, h)| (w, read(pairs_sql("next"), key(h.clone())))).collect();
        let co_of: Vec<_> = order.iter().enumerate().map(|(g, h)| (g, read(pairs_sql("co"), key(h.clone())))).collect();
        let (next_rs, co_rs) = futures::join!(join_all(next_of.into_iter().map(|(w, f)| async move { (w, f.await) })), join_all(co_of.into_iter().map(|(g, f)| async move { (g, f.await) })));
        let mut lists = Lists::default();
        let mut next: HashMap<String, f64> = HashMap::new();
        for (w, rs) in next_rs {
            for (i, s) in recommend::shares(&positive(pairs(&rs?))) {
                *next.entry(i).or_default() += w * s;
            }
        }
        lists.next = recommend::ranked_by(next);
        let mut co: HashMap<String, f64> = HashMap::new();
        for (g, rs) in co_rs {
            for (i, s) in recommend::shares(&positive(pairs(&rs?))) {
                *co.entry(i).or_default() += s / (g + 1) as f64;
            }
        }
        lists.co = recommend::ranked_by(co);
        let catalog = self.catalog();
        let items_schema = d.items.as_ref().and_then(|t| catalog.iter().find(|s| s.table.eq_ignore_ascii_case(t)).cloned());
        if let (Some(s), true) = (&items_schema, call.opts.similar > 0.0) {
            if let Some(sql) = recommend::Reads::similar(&d, s, &recent) {
                let rs = read(sql, Params::new()).await?;
                lists.similar = rs.rows.iter().filter_map(|r| r.first()).filter(|v| !matches!(v, Value::Null)).map(text).collect();
            }
        }
        if call.opts.popular > 0.0 {
            let k = (call.opts.k + order.len()).min(recommend::MAX_K);
            let rs = read(format!("SELECT src, sum(n) AS c FROM {counts} WHERE kind = 'item' GROUP BY src ORDER BY c DESC, src LIMIT {k}"), Params::new()).await?;
            lists.popular = pairs(&rs).into_iter().filter(|(_, c)| *c > 0.0).map(|(i, _)| i).collect();
        }
        let seen: HashSet<String> = order.iter().cloned().collect();
        let recs = recommend::fuse(&lists, &seen, &call.opts);
        let int = catalog.iter().find(|s| s.table.eq_ignore_ascii_case(&d.events)).and_then(|s| s.field(&d.item)).is_some_and(|f| f.ty == FieldType::Int);
        let line = format!(
            "recommend {} for {}: {} items, from {} next, {} seen together, {} similar, {} popular; {} of the user's items left out; counts summed over the shards",
            d.name,
            chloedb::query::hint::sql_value(&call.user),
            recs.len(),
            lists.next.len(),
            lists.co.len(),
            lists.similar.len(),
            lists.popular.len(),
            seen.len()
        );
        Ok(Recommended { recs, int, line })
    }
}
