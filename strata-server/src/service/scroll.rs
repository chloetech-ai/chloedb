//! Scrolls: a query's answer read a page at a time from one version of the
//! database, past `--max-result-window`, which then caps each page instead
//! of the whole answer, as Elasticsearch's scroll reads one
//! ([`strata::query::exec::Scroll`]). `POST /v1/query` with `page_size`
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
//! `--max-scroll-rows` rows' places, or rows, between them. One server
//! scrolls: a coordinator refuses `page_size` (its shards' scrolls are the
//! next step).

use std::sync::Arc;
use std::time::{Duration, Instant};

use strata::query::exec::{ExecOptions, Scroll};
use strata::storage::Snapshot;

use super::{conflict, too_many, Answer, Backend, Cancel, QueryReq, Running, Service};
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
    /// The rows a page holds at most (cut to `--max-result-window`).
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

/// An open scroll.
pub(super) struct Cursor {
    /// `None` while a page is read.
    scroll: Option<Scroll>,
    /// The lease on the version it reads (0: it holds its rows).
    lease: u64,
    table: String,
    sql: String,
    page: usize,
    owner: String,
    keep_alive: Duration,
    until: Instant,
    held: usize,
}

/// A scroll taken from its cursor while a page is read: put back when
/// dropped, as the page left it, or as it was if the page did not run.
struct Taken {
    svc: Arc<Service>,
    cursor: String,
    scroll: Option<Scroll>,
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
    /// The rows a scroll's pages hold: `asked`, at most the window.
    pub(super) fn page_size(&self, asked: usize) -> usize {
        asked.max(1).min(self.max_rows.unwrap_or(usize::MAX))
    }

    fn keep_alive(&self, asked: Option<Duration>) -> Duration {
        asked.unwrap_or(self.lease_ttl).min(MAX_KEEP_ALIVE)
    }

    /// Keeps what is left of a scroll that started on `snap` (`None`: one
    /// that holds its rows), and returns its cursor; `None` when no rows
    /// are left. Refused (429) when the open scrolls would hold more than
    /// `--max-scroll-rows` rows between them.
    pub(super) fn keep_scroll(&self, scroll: Scroll, snap: Option<Snapshot>, table: &str, sql: &str, ask: &ScrollAsk) -> Result<Option<String>, ApiError> {
        if scroll.done() {
            return Ok(None);
        }
        let held = scroll.held();
        let keep_alive = self.keep_alive(ask.keep_alive);
        let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        open.retain(|_, c| c.until > now);
        let holding: usize = open.values().map(|c| c.held).sum();
        if holding.saturating_add(held) > self.max_scroll_rows {
            return Err(ApiError {
                status: 429,
                message: format!(
                    "the open scrolls hold {holding} rows of the {} they may hold together (--max-scroll-rows), and this one would hold {held}: close some, or try again when they end",
                    self.max_scroll_rows
                ),
            });
        }
        let lease = match snap {
            Some(s) => self.lease_of(s, keep_alive)?,
            None => 0,
        };
        let id = format!("{:032x}", rand::random::<u128>());
        open.insert(id.clone(), Cursor { scroll: Some(scroll), lease, table: table.to_string(), sql: sql.to_string(), page: self.page_size(ask.page), owner: ask.owner.clone(), keep_alive, until: now + keep_alive, held });
        Ok(Some(id))
    }

    /// The next page of scroll `cursor`, read by `owner`, who started it:
    /// at most its page size of rows, with the cursor again while rows are
    /// left. `keep_alive` changes how long it is kept after this page.
    pub async fn scroll_next(self: &Arc<Self>, cursor: &str, owner: &str, keep_alive: Option<Duration>, timeout_ms: u64) -> Result<Answer, ApiError> {
        let t0 = Instant::now();
        let Backend::Local(db) = &self.backend else { return Err(gone()) };
        let timeout = self.timeout_of(timeout_ms);
        let (scroll, lease, table, sql, page) = {
            let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let c = open.get_mut(cursor).filter(|c| c.owner == owner && c.until > now).ok_or_else(gone)?;
            let scroll = c.scroll.take().ok_or_else(|| conflict("this cursor's page is being read: ask for the next once it is answered".into()))?;
            if keep_alive.is_some() {
                c.keep_alive = self.keep_alive(keep_alive);
            }
            // Kept while the page is read.
            c.until = now + c.keep_alive + timeout;
            (scroll, c.lease, c.table.clone(), c.sql.clone(), c.page)
        };
        let snap = match lease {
            0 => None,
            l => {
                self.renew_lease(l, timeout + Duration::from_secs(1));
                match self.leased(l) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        self.end_scroll(cursor);
                        return Err(e);
                    }
                }
            }
        };
        let req = QueryReq { sql: sql.clone(), timeout_ms, ..Default::default() };
        let meta = self.meta(&req);
        let live = self.live.start(&sql, "scroll", false, &meta);
        let mut taken = Taken { svc: self.clone(), cursor: cursor.to_string(), scroll: Some(scroll) };
        let db = db.clone();
        let res = LiveQueries::run(&live, async {
            let permit = self.admit(timeout).await.ok_or_else(too_many)?;
            let (running, n) = Running::start(&self.running);
            let cancel = Cancel::default();
            let opts = ExecOptions {
                threads: (self.cores / n).max(1),
                deadline: Some(t0 + timeout),
                cancel: Some(cancel.0.clone()),
                synonyms: self.synonyms().or_else(|| db.exec.synonyms.clone()),
                max_groups: self.max_groups,
                memory: self.query_memory.clone(),
                ..db.exec.clone()
            };
            // The scroll goes back with the page read, or as it was when the
            // page fails, also when the caller stopped waiting.
            let r = tokio::task::spawn_blocking(move || {
                let _held = (permit, running);
                let scroll = taken.scroll.as_mut().expect("the scroll taken");
                let r = match &snap {
                    Some(snap) => match snap.table(&table) {
                        Some(view) => db.scroll_next(&view, scroll, &opts, page).map_err(ApiError::from),
                        None => Err(ApiError::bad(format!("query error: no such table: {table}"))),
                    },
                    None => Ok(scroll.next_rows(page)),
                };
                let more = !scroll.done();
                drop(taken);
                r.map(|rs| (rs, more))
            })
            .await;
            drop(cancel);
            r.map_err(ApiError::internal)?
        })
        .await;
        drop(live);
        let more = matches!(res, Ok((_, true)));
        let a = self.done(&sql, "scroll", t0, None, false, (false, false), &meta, res.map(|(rs, _)| (rs, Vec::new())))?;
        Ok(Answer { cursor: more.then(|| cursor.to_string()), ..a })
    }

    /// Puts a scroll back after a page: kept for its keep-alive while rows
    /// are left, ended (its lease released) once none are.
    fn put_back(&self, cursor: &str, scroll: Scroll) {
        let mut open = self.scrolls.lock().unwrap_or_else(|e| e.into_inner());
        let Some(c) = open.get_mut(cursor) else {
            return;
        };
        if scroll.done() {
            let lease = c.lease;
            open.remove(cursor);
            drop(open);
            self.release(&[lease]);
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
        if let Some(c) = &c {
            self.release(&[c.lease]);
        }
        c.is_some()
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
        let now = Instant::now();
        open.retain(|_, c| c.until > now);
        (open.len(), open.values().map(|c| c.held).sum())
    }
}
