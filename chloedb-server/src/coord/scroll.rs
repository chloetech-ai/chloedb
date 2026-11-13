//! Scrolls over shards: a query's answer read a page at a time past the
//! window, each shard reading the version it had when the scroll started.
//!
//! A query that [`chloedb::cluster::streams`] (rows in no order, or ordered
//! by one score or column) starts a scroll on every shard it asks, in shard
//! mode ([`SCROLL_PROTOCOL`]): the copy that answers keeps it, on a lease
//! of its snapshot, and each later page of that shard is asked of that
//! copy by its cursor. The coordinator merges the shards' pages as they
//! come ([`Pages`]): rows in no order one shard's after another's, ordered
//! rows by asking each shard whose held rows run out before the page is
//! full for more, so that between pages it holds at most two pages of the
//! shards' rows. A shard not asked for a page has its scroll kept open by
//! a page of no rows ([`RENEW_AFTER`]). A scroll whose shard fails ends:
//! its pages cannot be put together without that shard's.
//!
//! Any other query (groups, counts, fused scores, rerank phases, guards)
//! a coordinator answers whole and holds its rows, as one server holds
//! those of a compound query ([`crate::service`]).

use std::time::{Duration, Instant};

use futures::future::join_all;
use chloedb::cluster::Pages;
use chloedb::query::exec::ResultSet;
use chloedb::query::ir::Plan;
use chloedb::query::stats::TermStats;
use chloedb::query::{Projection, Select};
use chloedb::schema::Schema;

use super::{ms_left, Client, Coordinator, Read, ONE_REPLICA};
use crate::grpc::{result_from_pb, SCROLL_PROTOCOL};
use crate::pb;
use crate::service::QueryReq;
use crate::ApiError;

/// How long a shard's scroll is kept past the coordinator's: a shard not
/// asked for a page is asked for one of no rows once its scroll was last
/// kept this long ago, half of it, so that it lasts as long as the
/// coordinator's ([`RENEW_AFTER`]).
pub const SHARD_KEEP_ALIVE_MARGIN: Duration = Duration::from_secs(60);

/// When a shard's scroll is kept open by a page of no rows: once it was
/// last read or kept this long ago.
const RENEW_AFTER: Duration = Duration::from_secs(30);

/// A shard's scroll: the copy that keeps it, and its cursor.
struct Opened {
    /// The copy's name in errors.
    name: String,
    client: Client,
    cursor: String,
    /// When it was last read or kept.
    kept: Instant,
}

/// A scroll over shards ([`Coordinator::scroll`]): the pages its shards'
/// scrolls answered and not yet answered, and those scrolls (`None`: one
/// with no rows left, or a shard not asked).
pub struct ShardScroll {
    pages: Pages,
    shards: Vec<Option<Opened>>,
    /// How long each shard keeps its scroll after a page.
    keep_alive: Duration,
}

impl ShardScroll {
    /// Whether every row has been answered.
    pub fn done(&self) -> bool {
        self.pages.done()
    }

    /// The shards' rows the coordinator holds between pages.
    pub fn held(&self) -> usize {
        self.pages.held()
    }

    /// Ends the shards' scrolls that have rows left (a scroll closed, or
    /// read to its LIMIT, before them), in the background.
    pub fn end(self) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        for o in self.shards.into_iter().flatten() {
            let mut c = o.client;
            rt.spawn(async move { tokio::time::timeout(Duration::from_secs(5), c.release(pb::ReleaseRequest { snapshots: Vec::new(), cursors: vec![o.cursor] })).await });
        }
    }
}

impl Coordinator {
    /// Starts a scroll of `select` over the shards (`select` must
    /// [`chloedb::cluster::streams`]): its first page, of at most `page`
    /// rows, and the scroll of the rest while rows are left. Each shard
    /// keeps its scroll for `keep_alive` after each page.
    pub async fn scroll(&self, req: &QueryReq, schema: &Schema, select: &Select, page: usize, keep_alive: Duration, deadline: Instant) -> Result<(ResultSet, Option<ShardScroll>), ApiError> {
        if !select.graphs.is_empty() {
            return Err(ApiError::bad(super::GRAPHS_ON_SHARDS));
        }
        self.routed(|| self.scroll_once(req, schema, select, page, keep_alive, deadline)).await
    }

    async fn scroll_once(&self, req: &QueryReq, schema: &Schema, select: &Select, page: usize, keep_alive: Duration, deadline: Instant) -> Result<(ResultSet, Option<ShardScroll>), ApiError> {
        let t = &self.topo();
        let n = t.replicas.len();
        let protocol = t.protocol();
        if protocol < SCROLL_PROTOCOL {
            return Err(ApiError::bad(format!(
                "a scroll over shards needs every shard to speak protocol {SCROLL_PROTOCOL}, and one speaks {protocol}: upgrade it, or page by a column, WHERE n > :last ORDER BY n LIMIT k"
            )));
        }
        if select.projection.iter().any(|(_, p)| *p == Projection::SeqNo) && !(0..n).all(|i| self.numbered_alike(t, i)) {
            return Err(ApiError::bad(ONE_REPLICA));
        }
        let mut pages = Pages::new(select, n)?;
        let ranked_text = chloedb::cluster::scores_bm25(select);
        // As a query does: one pinning its key asks only its shards; BM25
        // scores need every shard's statistics.
        let asked: Vec<usize> = match if ranked_text || n == 1 { None } else { self.pinned_shards(t, schema, select) } {
            Some(a) => a.into_iter().collect(),
            None => (0..n).collect(),
        };
        for i in (0..n).filter(|i| !asked.contains(i)) {
            pages.ended(i);
        }
        let read = &Read::new(&req.sql, &req.params, req.plan.as_deref(), protocol, t.epoch(), || Ok(Plan::of_select(schema, select)))?.after(self.after(t, &req.after)?).profiled(req.profile);
        let start = self.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Summed BM25 statistics, from the snapshot each shard's scroll
        // then reads, whose lease the scroll's start ends.
        let mut leased: Vec<Option<(usize, u64)>> = vec![None; n];
        let stats_json = match &req.stats {
            Some(s) => serde_json::to_vec(s).unwrap_or_default(),
            None if ranked_text && n > 1 => {
                let mut sum = TermStats::default();
                let mut failed = None;
                for (i, a) in self.shard_stats(t, read, protocol, asked.clone(), start, deadline).await {
                    match a {
                        Ok((s, r, lease, _)) => {
                            sum.add(&s);
                            leased[i] = Some((r, lease));
                        }
                        Err(e) => failed = failed.or(Some(e)),
                    }
                }
                if let Some(e) = failed {
                    self.release(t, leased.iter().enumerate().filter_map(|(i, l)| l.map(|(r, lease)| (i, r, lease))));
                    return Err(e);
                }
                serde_json::to_vec(&sum).unwrap_or_default()
            }
            None => Vec::new(),
        };
        // The first page: in no order the first shard's rows (the others'
        // scrolls opened on their snapshots, reading none); ordered, each
        // shard's share.
        let want = page.saturating_add(select.offset.min(page));
        let share = want.div_ceil(asked.len().max(1)).max(chloedb::cluster::MIN_ASK).min(want);
        let keep_alive_ms = keep_alive.as_millis() as u64;
        let leased = &leased;
        let stats_json = &stats_json;
        let calls = asked.iter().enumerate().map(|(k, &i)| {
            let ask = match select.order {
                None if k == 0 => want,
                None => 0,
                Some(_) => share,
            } as u64;
            async move {
                let first = leased[i].map(|(r, _)| r);
                let a = self
                    .call(t, i, start, first, read.min(i), deadline, |r, mut c| {
                        let snapshot = leased[i].filter(|(lr, _)| *lr == r).map_or(0, |(_, l)| l);
                        async move {
                            let p = self.speak(t, i, r, &mut c, protocol).await?;
                            self.reads_after(t, i, r, read.min(i))?;
                            if p < SCROLL_PROTOCOL {
                                return Err(tonic::Status::failed_precondition(format!("it speaks protocol {p}, and a scroll needs {SCROLL_PROTOCOL}")));
                            }
                            let q = pb::QueryRequest { timeout_ms: ms_left(deadline), shard: true, stats_json: stats_json.clone(), snapshot, release: snapshot != 0, scroll: true, page_size: ask, keep_alive_ms, ..read.request(p, i) };
                            c.query(q).await
                        }
                    })
                    .await;
                (i, a)
            }
        });
        let mut shards: Vec<Option<Opened>> = (0..n).map(|_| None).collect();
        let mut failed = None;
        for (i, a) in join_all(calls).await {
            match a {
                Ok((r, mut a)) => {
                    let cursor = std::mem::take(&mut a.cursor);
                    let more = !cursor.is_empty();
                    let rep = &t.replicas[i][r];
                    if more {
                        shards[i] = Some(Opened { name: self.copy_name(&rep.url), client: rep.client.clone(), cursor, kept: Instant::now() });
                    }
                    if let Err(e) = result_from_pb(a).and_then(|rs| pages.add(i, rs, more).map_err(ApiError::from)) {
                        failed = failed.or(Some(e));
                    }
                }
                Err(e) => failed = failed.or(Some(e)),
            }
        }
        let mut s = ShardScroll { pages, shards, keep_alive };
        if let Some(e) = failed {
            s.end();
            return Err(e);
        }
        let rs = match self.scroll_page(&mut s, page, deadline).await {
            Ok(rs) => rs,
            Err(e) => {
                s.end();
                return Err(e);
            }
        };
        let mut rs = rs;
        rs.stats.plan.insert(0, format!("scroll over {} of {n} shards, merged as their pages come", asked.len()));
        match s.done() {
            true => {
                s.end();
                Ok((rs, None))
            }
            false => Ok((rs, Some(s))),
        }
    }

    /// The next page of scroll `s`, of at most `page` rows; with
    /// `keep_alive`, each shard keeps its scroll that long after each page
    /// from now. A shard that fails ends the scroll: the caller ends `s` on
    /// an error.
    pub async fn scroll_next(&self, s: &mut ShardScroll, page: usize, keep_alive: Option<Duration>, deadline: Instant) -> Result<ResultSet, ApiError> {
        // The scrolls of shards this page may not ask, kept open: every
        // one, for a keep-alive that changed.
        if let Some(k) = keep_alive {
            s.keep_alive = k;
        }
        let renew = if keep_alive.is_some() { Duration::ZERO } else { RENEW_AFTER };
        let stale: Vec<usize> = (0..s.shards.len()).filter(|&i| s.shards[i].as_ref().is_some_and(|o| o.kept.elapsed() >= renew)).collect();
        if !stale.is_empty() {
            self.ask_shards(s, stale.into_iter().map(|i| (i, 0)).collect(), deadline).await?;
        }
        self.scroll_page(s, page, deadline).await
    }

    /// Asks the shards for the rows the next page needs until it has them,
    /// then takes it (rows OFFSET skips taken first).
    async fn scroll_page(&self, s: &mut ShardScroll, page: usize, deadline: Instant) -> Result<ResultSet, ApiError> {
        loop {
            loop {
                let needs = s.pages.needs(page);
                if needs.is_empty() {
                    break;
                }
                self.ask_shards(s, needs, deadline).await?;
            }
            let rs = s.pages.page(page);
            if !rs.rows.is_empty() || s.pages.done() {
                return Ok(rs);
            }
        }
    }

    /// Asks each of `asks`' shards for its scroll's next page of so many
    /// rows (0: keep it open), and hands the pages to `s`.
    async fn ask_shards(&self, s: &mut ShardScroll, asks: Vec<(usize, usize)>, deadline: Instant) -> Result<(), ApiError> {
        let keep_alive_ms = s.keep_alive.as_millis() as u64;
        let calls = asks.into_iter().filter_map(|(i, ask)| {
            let o = s.shards[i].as_ref()?;
            let mut c = o.client.clone();
            let q = pb::QueryRequest { cursor: o.cursor.clone(), page_size: ask as u64, keep_alive_ms, timeout_ms: ms_left(deadline), shard: true, protocol: SCROLL_PROTOCOL, ..Default::default() };
            Some(async move { (i, tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), c.query(q)).await) })
        });
        let mut failed = None;
        for (i, a) in join_all(calls).await {
            let name = s.shards[i].as_ref().map_or_else(String::new, |o| o.name.clone());
            let a = match a {
                Ok(Ok(a)) => a.into_inner(),
                Ok(Err(e)) => {
                    let mut e = ApiError::from(e);
                    e.message = format!("{name}: its scroll: {}", e.message);
                    failed = failed.or(Some(e));
                    continue;
                }
                Err(_) => {
                    failed = failed.or(Some(ApiError { status: 504, message: format!("{name}: its scroll's page ran past the deadline") }));
                    continue;
                }
            };
            let more = !a.cursor.is_empty();
            match more {
                true => {
                    if let Some(o) = &mut s.shards[i] {
                        o.kept = Instant::now();
                    }
                }
                false => s.shards[i] = None,
            }
            if let Err(e) = result_from_pb(a).and_then(|rs| s.pages.add(i, rs, more).map_err(ApiError::from)) {
                failed = failed.or(Some(e));
            }
        }
        match failed {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
