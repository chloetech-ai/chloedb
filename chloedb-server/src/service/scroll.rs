//! Scrolls: a query's answer read a page at a time from one version of the
//! database, past `--max-result-window`, which then caps each page instead
//! of the whole answer, as Elasticsearch's scroll reads one
//! ([`chloedb::query::exec::Scroll`]). `POST /v1/query` with `page_size`
//! answers the first page and, when rows are left, a `cursor`; the cursor
//! alone asks for the next page, until the last, which answers none.
//!
//! A scroll keeps a lease on the version it read ([`Service::lease_of`];
//! none for one that holds its rows), renewed by each page for its
//! keep-alive. Its cursor is random, is read only by the key that started
//! it, and ends with its last page, when it is closed
//! (`DELETE /v1/query/cursors/:id`), or when no page is asked for over its
//! keep-alive. A plain scan holds nothing between pages; the open scrolls
//! of ordered queries, groups and compound queries hold at most
//! `--max-scroll-rows` rows' places, or rows, between them.
//!
//! A coordinator scrolls its shards' scrolls of a plain scan, or of rows
//! ordered by one score or column, and merges their pages as they come
//! ([`crate::coord::ShardScroll`]), holding the rows of two pages at most;
//! it answers any other query whole and holds its rows, as a server holds
//! a compound query's. A shard's scroll for a coordinator is asked by the
//! shard protocol ([`crate::grpc::SCROLL_PROTOCOL`]), each page of so many
//! rows as the coordinator asks for.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chloedb::query::exec::{ExecOptions, ResultSet, Scroll};
use chloedb::query::stats::TermStats;
use chloedb::storage::Snapshot;

use super::{conflict, too_many, Answer, Backend, Cancel, QueryReq, Running, Service};
use crate::coord::ShardScroll;
use crate::telemetry::LiveQueries;
use crate::ApiError;

/// The longest keep-alive a request may ask for (`keep_alive_ms`).
pub const MAX_KEEP_ALIVE: Duration = Duration::from_secs(3600);

/// The rows the open scrolls hold between them by default
/// (`--max-scroll-rows`): about 24 MB of rows' places.
pub const MAX_SCROLL_ROWS: usize = 1_000_000;

/// What a query asks of a scroll ([`QueryReq::scroll`]).
#[derive(Clone, Debug)]
pub struct ScrollAsk {
    /// The rows a page holds at most (cut to `--max-result-window`); a
    /// coordinator opens a shard's scroll with a page of none.
    pub page: usize,
    /// How long the scroll is kept after each page (`None`: the server's
    /// lease TTL).
    pub keep_alive: Option<Duration>,
    /// Who may read its pages: who started it ([`owner`]).
    pub owner: String,
}

/// Who may read a scroll's pages: the key (or name) of a request's
/// principal.
pub fn owner(who: &crate::keys::Principal) -> String {
    format!("{}/{}", who.name, who.key.as_deref().unwrap_or(""))
}

/// What an open scroll reads its pages from.
pub(super) enum Paged {
    /// This server's database (or the rows it holds).
    Db(Scroll),
    /// A coordinator's shards' scrolls.
    Shards(Box<ShardScroll>),
}

impl Paged {
    fn done(&self) -> bool {
        match self {
            Paged::Db(s) => s.done(),
            Paged::Shards(s) => s.done(),
        }
    }

    fn held(&self) -> usize {
        match self {
            Paged::Db(s) => s.held(),
            Paged::Shards(s) => s.held(),
        }
    }

    /// Ends what it keeps open elsewhere: a coordinator's shards' scrolls.
    fn end(self) {
        if let Paged::Shards(s) = self {
            s.end();
        }
    }
}

/// An open scroll.
pub(super) struct Cursor {
    /// `None` while a page is read.
    scroll: Option<Paged>,
    /// The lease on the version it reads (0: it holds its rows).
    lease: u64,
    table: String,
    sql: String,
    page: usize,
    owner: String,
    keep_alive: Duration,
    until: Instant,
    held: usize,
    /// A shard's scroll for a coordinator: its pages in shard mode, scored
    /// with the statistics it started with.
    shard: bool,
    stats: Option<Arc<TermStats>>,
}

/// A scroll taken from its cursor while a page is read: put back when
/// dropped, as the page left it, or as it was if the page did not run.
struct Taken {
    svc: Arc<Service>,
    cursor: String,
    scroll: Option<Paged>,
}

impl Drop for Taken {
    fn drop(&mut self) {
        if let Some(s) = self.scroll.take() {
            self.svc.put_back(&self.cursor, s);
        }
    }
}

fn gone() -> ApiError {
    ApiError { status: 404, message: "no such cursor: it was read to its last page, closed, or not read for its keep-alive (keep_alive_ms)".into() }
}

impl Service {
    /// The rows a scroll's pages hold: `asked`, at most the window (none
    /// only for a coordinator's opening of a shard's scroll).
    pub(super) fn page_size(&self, asked: usize) -> usize {
        asked.min(self.max_rows.unwrap_or(usize::MAX))
    }

    /// How long a scroll is kept after each page: `asked`, an hour at most
    /// (a shard's for a coordinator, the coordinator's margin more).
    pub(super) fn keep_alive(&self, asked: Option<Duration>, shard: bool) -> Duration {
        let most = if shard { MAX_KEEP_ALIVE + crate::coord::SHARD_KEEP_ALIVE_MARGIN } else { MAX_KEEP_ALIVE };
        asked.unwrap_or(self.lease_ttl).min(most)
    }

    /// Keeps what is left of a scroll that started on `snap` (`None`: one
    /// that holds its rows, or a coordinator's), and returns its cursor;
    /// `None` when no rows are left. Refused (429) when the open scrolls
    /// would hold more than `--max-scroll-rows` rows between them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn keep_scroll(&self, scroll: Paged, snap: Option<Snapshot>, table: &str, sql: &str, ask: &ScrollAsk, shard: bool, stats: Option<Arc<TermStats>>) -> Result<Option<String>, ApiError> {
        if scroll.done() {
            scroll.end();
            return Ok(None);
        }
        let held = scroll.held();
        let keep_alive = self.keep_alive(ask.keep_alive, shard);
        let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        ended(&mut open, now);
        let holding: usize = open.values().map(|c| c.held).sum();
        if holding.saturating_add(held) > self.max_scroll_rows {
            drop(open);
            scroll.end();
            return Err(ApiError {
                status: 429,
                message: format!(
                    "the open scrolls hold {holding} rows of the {} they may hold together (--max-scroll-rows), and this one would hold {held}: close some, or try again when they end",
                    self.max_scroll_rows
                ),
            });
        }
        let lease = match snap.map(|s| self.lease_of(s, keep_alive)).transpose() {
            Ok(l) => l.unwrap_or(0),
            Err(e) => {
                drop(open);
                scroll.end();
                return Err(e);
            }
        };
        let id = format!("{:032x}", rand::random::<u128>());
        let page = self.page_size(ask.page).max(1);
        open.insert(id.clone(), Cursor { scroll: Some(scroll), lease, table: table.to_string(), sql: sql.to_string(), page, owner: ask.owner.clone(), keep_alive, until: now + keep_alive, held, shard, stats });
        Ok(Some(id))
    }

    /// The next page of scroll `cursor`, read by `owner`, who started it:
    /// at most its page size of rows (`page`: so many, as a coordinator
    /// asks of a shard), with the cursor again while rows are left.
    /// `keep_alive` changes how long it is kept after this page.
    pub async fn scroll_next(self: &Arc<Self>, cursor: &str, owner: &str, keep_alive: Option<Duration>, timeout_ms: u64, page: Option<usize>) -> Result<Answer, ApiError> {
        let t0 = Instant::now();
        let timeout = self.timeout_of(timeout_ms);
        let (scroll, lease, table, sql, page, shard, stats, kept_for) = {
            let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let c = open.get_mut(cursor).filter(|c| c.owner == owner && c.until > now).ok_or_else(gone)?;
            let scroll = c.scroll.take().ok_or_else(|| conflict("this cursor's page is being read: ask for the next once it is answered".into()))?;
            if keep_alive.is_some() {
                c.keep_alive = self.keep_alive(keep_alive, c.shard);
            }
            // Kept while the page is read.
            c.until = now + c.keep_alive + timeout;
            let page = page.map_or(c.page, |p| self.page_size(p));
            // A coordinator's shards keep their scrolls the margin longer.
            let kept_for = keep_alive.map(|_| c.keep_alive + crate::coord::SHARD_KEEP_ALIVE_MARGIN);
            (scroll, c.lease, c.table.clone(), c.sql.clone(), page, c.shard, c.stats.clone(), kept_for)
        };
        let mut taken = Taken { svc: self.clone(), cursor: cursor.to_string(), scroll: Some(scroll) };
        let req = QueryReq { sql: sql.clone(), timeout_ms, shard, ..Default::default() };
        let meta = self.meta(&req);
        let live = self.live.start(&sql, "scroll", shard, &meta);
        let res = match (taken.scroll.as_mut(), &self.backend) {
            // A coordinator's: its shards' pages, merged.
            (Some(Paged::Shards(s)), Backend::Cluster(c)) => {
                let r = LiveQueries::run(&live, async {
                    let permit = self.admit(timeout).await.ok_or_else(too_many)?;
                    let r = c.scroll_next(s, page, kept_for, t0 + timeout).await;
                    drop(permit);
                    r
                })
                .await;
                match r {
                    Ok(rs) => Ok((rs, !s.done())),
                    // A shard's scroll failed: the pages cannot be put together.
                    Err(mut e) => {
                        drop(taken);
                        self.end_scroll(cursor);
                        e.message.push_str(" (the scroll ended: start it again)");
                        Err(e)
                    }
                }
            }
            (Some(Paged::Db(scroll)), _) if !scroll.reads() => {
                let rs = scroll.next_rows(page);
                Ok((rs, !scroll.done()))
            }
            (Some(Paged::Db(_)), Backend::Local(db)) => {
                let snap = match self.leased(lease) {
                    Ok(s) => s,
                    Err(e) => {
                        drop(taken);
                        self.end_scroll(cursor);
                        return Err(e);
                    }
                };
                self.renew_lease(lease, timeout + Duration::from_secs(1));
                let db = db.clone();
                LiveQueries::run(&live, async {
                    let permit = self.admit(timeout).await.ok_or_else(too_many)?;
                    let (running, n) = Running::start(&self.running);
                    let cancel = Cancel::default();
                    let opts = ExecOptions {
                        stats,
                        shard,
                        threads: (self.cores / n).max(1),
                        deadline: Some(t0 + timeout),
                        cancel: Some(cancel.0.clone()),
                        synonyms: self.synonyms().or_else(|| db.exec.synonyms.clone()),
                        max_groups: self.max_groups,
                        memory: self.query_memory.clone(),
                        ..db.exec.clone()
                    };
                    // The scroll goes back with the page read, or as it was when
                    // the page fails, also when the caller stopped waiting.
                    let r = tokio::task::spawn_blocking(move || {
                        let _held = (permit, running);
                        let Some(Paged::Db(scroll)) = taken.scroll.as_mut() else { unreachable!("a scroll of this database taken") };
                        let r = match snap.table(&table) {
                            Some(view) => db.scroll_next(&view, scroll, &opts, page).map_err(ApiError::from),
                            None => Err(ApiError::bad(format!("query error: no such table: {table}"))),
                        };
                        let more = !scroll.done();
                        drop(taken);
                        r.map(|rs| (rs, more))
                    })
                    .await;
                    drop(cancel);
                    r.map_err(ApiError::internal)?
                })
                .await
            }
            _ => Err(gone()),
        };
        drop(live);
        let more = matches!(res, Ok((_, true)));
        let a = self.done(&sql, "scroll", t0, None, shard, (false, false), &meta, res.map(|(rs, _): (ResultSet, bool)| (rs, Vec::new())))?;
        Ok(Answer { cursor: more.then(|| cursor.to_string()), ..a })
    }

    /// Puts a scroll back after a page: kept for its keep-alive while rows
    /// are left, ended (its lease released) once none are.
    fn put_back(&self, cursor: &str, scroll: Paged) {
        let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
        let Some(c) = open.get_mut(cursor) else {
            drop(open);
            scroll.end();
            return;
        };
        if scroll.done() {
            let lease = c.lease;
            open.remove(cursor);
            drop(open);
            self.release(&[lease]);
            scroll.end();
            return;
        }
        c.until = Instant::now() + c.keep_alive;
        c.held = scroll.held();
        c.scroll = Some(scroll);
        let (lease, keep_alive) = (c.lease, c.keep_alive);
        drop(open);
        self.renew_lease(lease, keep_alive);
    }

    /// Ends scroll `cursor` and releases its lease; whether it was open.
    fn end_scroll(&self, cursor: &str) -> bool {
        let c = self.scrolls.lock().unwrap_or_else(|e| e.into_inner()).remove(cursor);
        if let Some(c) = c {
            self.release(&[c.lease]);
            if let Some(s) = c.scroll {
                s.end();
            }
            return true;
        }
        false
    }

    /// Closes scroll `cursor` before its last page, for `owner`, who
    /// started it: whether it was open.
    pub fn close_scroll(&self, cursor: &str, owner: &str) -> bool {
        let mine = self.scrolls.lock().unwrap_or_else(|e| e.into_inner()).get(cursor).is_some_and(|c| c.owner == owner);
        mine && self.end_scroll(cursor)
    }

    /// Ends the scrolls not read over their keep-alive; returns how many
    /// are open still and the rows they hold.
    pub fn sweep_scrolls(&self) -> (usize, usize) {
        let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
        ended(&mut open, Instant::now());
        (open.len(), open.values().map(|c| c.held).sum())
    }
}

/// Ends the scrolls of `open` not read over their keep-alive (their
/// leases expire): a coordinator's ends its shards' scrolls.
fn ended(open: &mut std::collections::HashMap<String, Cursor>, now: Instant) {
    let gone: Vec<String> = open.iter().filter(|(_, c)| c.until <= now).map(|(k, _)| k.clone()).collect();
    for k in gone {
        if let Some(s) = open.remove(&k).and_then(|c| c.scroll) {
            s.end();
        }
    }
}
