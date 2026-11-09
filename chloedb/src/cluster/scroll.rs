//! A scroll over shards: the pages of every shard's scroll merged into
//! the pages of the whole collection as they come ([`Pages`]).
//!
//! A coordinator starts a scroll ([`crate::query::exec::Scroll`]) on every
//! shard in shard mode, of a query that [`streams`]: rows in no order,
//! which go one shard's after another's, as [`merge`] puts them; or rows
//! ordered by one score or column, which each shard answers in that
//! order, so that the next of the whole collection is the first of some
//! shard's. It hands the rows each shard answers to [`Pages::add`], asks
//! the shards [`Pages::needs`] names for more until it names none, and
//! then takes the page ([`Pages::page`]): a row is answered only once every
//! shard that may still hold one before it has answered past it. Pages
//! put together are [`merge`]'s answer of the shards' whole answers, rows
//! tied exactly on score in another order perhaps; between pages the
//! coordinator holds at most the rows the shards answered past the last.
//!
//! [`merge`]: super::merge

use std::cmp::Ordering;
use std::collections::VecDeque;

use super::{by_key, by_score, err, Row};
use crate::error::Result;
use crate::query::agg;
use crate::query::exec::{path_order, ResultSet};
use crate::query::logical::{OrderBy, Projection, Score, Select, Tiebreak};
use crate::query::rank::Rank;

/// The fewest rows a coordinator asks of a shard for a page, unless the
/// page needs fewer: so that a page of a few rows over many shards takes
/// a few calls, not one for each row.
pub const MIN_ASK: usize = 256;

/// Whether a coordinator merges `q`'s pages from its shards' scrolls as
/// they come ([`Pages`]): rows in no order, or ordered by a column, a
/// path, or one score (BM25, a similarity, a ranking expression), and not
/// fused, reranked, grouped, counted, guarded or cut per group. A
/// coordinator answers any other query whole and holds its rows.
pub fn streams(q: &Select) -> bool {
    let plain = q.group_by.is_empty()
        && q.having.is_none()
        && q.phases.is_empty()
        && q.graphs.is_empty()
        && q.guard.is_none()
        && q.per.is_none()
        && !agg::is_general(q, |_| false)
        && !q.projection.iter().any(|(_, p)| matches!(p, Projection::Count | Projection::Agg { .. }));
    plain
        && match &q.order {
            None | Some(OrderBy::Path { .. }) => true,
            Some(OrderBy::Score { score, .. }) => Rank::of(score, 0).is_none() && !score.has_cross_encoder(),
            Some(_) => false,
        }
}

/// The order of the merged rows.
enum Order {
    /// One shard's after another's.
    Shards,
    /// By score, then by key.
    Score { desc: bool },
    /// By the value of column `column`, then by key.
    Path { column: usize, desc: bool, nulls_first: bool },
}

/// The pages of a scroll over shards, merged from the pages of each
/// shard's scroll (see [the module](self)).
pub struct Pages {
    q: Select,
    order: Order,
    key_desc: bool,
    columns: Vec<String>,
    score_columns: Vec<usize>,
    /// Each shard's rows answered and not yet in a page, in its order.
    rows: Vec<VecDeque<Row>>,
    /// Whether each shard's scroll has rows left to answer.
    more: Vec<bool>,
    /// The rows OFFSET skips still to skip.
    skip: usize,
    /// The rows LIMIT leaves (`None`: no LIMIT).
    left: Option<usize>,
}

impl Pages {
    /// The pages of `q`'s rows over `shards` shards, each of whose scroll
    /// has rows until [`Pages::add`] says otherwise.
    pub fn new(q: &Select, shards: usize) -> Result<Pages> {
        if !streams(q) {
            return Err(err("a scroll over shards merges rows in no order, or ordered by one score or column, as they come"));
        }
        let order = match &q.order {
            None => Order::Shards,
            Some(OrderBy::Score { score, desc, .. }) => Order::Score { desc: *desc || !matches!(score, Score::Column { .. }) },
            Some(OrderBy::Path { path, desc, nulls_first }) => {
                let column = q.projection.iter().position(|(_, p)| *p == Projection::Path(*path)).ok_or_else(|| err("rows ordered by a path over shards select the path"))?;
                Order::Path { column, desc: *desc, nulls_first: *nulls_first }
            }
            Some(_) => return Err(err("ORDER BY COUNT(*) needs GROUP BY")),
        };
        Ok(Pages {
            order,
            key_desc: matches!(q.tiebreak, Some(Tiebreak::Key { desc: true })),
            columns: q.projection.iter().map(|(n, _)| n.clone()).collect(),
            score_columns: q.projection.iter().enumerate().filter(|(_, (_, p))| *p == Projection::Score).map(|(i, _)| i).collect(),
            rows: (0..shards).map(|_| VecDeque::new()).collect(),
            more: vec![true; shards],
            skip: q.offset,
            left: q.limit,
            q: q.clone(),
        })
    }

    /// Takes a page that shard `shard`'s scroll answered (in shard mode),
    /// and whether its scroll has rows left after it.
    pub fn add(&mut self, shard: usize, rs: ResultSet, more: bool) -> Result<()> {
        if rs.keys.len() != rs.rows.len() || rs.scores.len() != rs.rows.len() {
            return Err(err(format!("shard {shard} answered a page without keys and scores (not in shard mode?)")));
        }
        let rows = &mut self.rows[shard];
        for ((values, key), score) in rs.rows.into_iter().zip(rs.keys).zip(rs.scores) {
            rows.push_back(Row { values, key, score, inputs: Vec::new(), ranked: Vec::new(), phases: Vec::new() });
        }
        self.more[shard] = more;
        Ok(())
    }

    /// Shard `shard`'s scroll has no rows left: it failed to, or was
    /// ended (the rows it answered stay).
    pub fn ended(&mut self, shard: usize) {
        self.more[shard] = false;
    }

    /// Whether every row has been answered: those LIMIT allows, or those
    /// the shards had.
    pub fn done(&self) -> bool {
        self.left == Some(0) || (self.more.iter().all(|m| !m) && self.rows.iter().all(VecDeque::is_empty))
    }

    /// The shards whose scrolls have rows left: those to end when the
    /// scroll is done or closed early.
    pub fn open(&self) -> Vec<usize> {
        (0..self.more.len()).filter(|&i| self.more[i]).collect()
    }

    /// The rows the shards answered that are in no page yet: what the
    /// coordinator holds.
    pub fn held(&self) -> usize {
        self.rows.iter().map(VecDeque::len).sum()
    }

    /// The first `want` rows held in the merged order: how many of each
    /// shard's, and whether a shard with rows left ran out of held rows
    /// first, which stops the walk, since its next row may come next.
    fn walk(&self, want: usize) -> (Vec<usize>, bool) {
        let n = self.rows.len();
        let mut took = vec![0; n];
        let mut total = 0;
        match self.order {
            Order::Shards => {
                for i in 0..n {
                    let k = self.rows[i].len().min(want - total);
                    took[i] = k;
                    total += k;
                    if total == want {
                        return (took, false);
                    }
                    if self.more[i] {
                        return (took, true);
                    }
                }
                (took, false)
            }
            Order::Score { .. } | Order::Path { .. } => {
                while total < want {
                    let mut best: Option<usize> = None;
                    for i in 0..n {
                        let Some(r) = self.rows[i].get(took[i]) else {
                            if self.more[i] {
                                // Its next row may come before any held.
                                return (took, true);
                            }
                            continue;
                        };
                        if best.is_none_or(|b| self.cmp(r, &self.rows[b][took[b]]) == Ordering::Less) {
                            best = Some(i);
                        }
                    }
                    let Some(b) = best else { break };
                    took[b] += 1;
                    total += 1;
                }
                (took, false)
            }
        }
    }

    /// The rows a page of `page` rows takes: those OFFSET still skips and
    /// then those LIMIT allows, `page` at most, or while OFFSET skips more
    /// than a page, a page of those it skips (so that the shards' rows
    /// held for a page are at most two pages').
    fn want(&self, page: usize) -> usize {
        let page = page.max(1);
        match self.skip > page {
            true => page,
            false => self.skip + self.left.map_or(page, |l| l.min(page)),
        }
    }

    /// The shards to ask for more rows before a page of at most `page`
    /// rows can be answered, and how many rows to ask each for: none when
    /// it can be.
    pub fn needs(&self, page: usize) -> Vec<(usize, usize)> {
        if self.done() {
            return Vec::new();
        }
        let want = self.want(page);
        let (took, short) = self.walk(want);
        if !short {
            return Vec::new();
        }
        // The walk stopped where a shard with rows left had no more held:
        // its next row may be the page's next, and at most the rest of the
        // page are its.
        let rest = want - took.iter().sum::<usize>();
        let mut needs: Vec<(usize, usize)> = (0..self.rows.len()).filter(|&i| self.more[i] && took[i] == self.rows[i].len()).map(|i| (i, rest)).collect();
        if let Order::Shards = self.order {
            needs.truncate(1);
        }
        // Several shards share the page, each its share at first.
        let share = want.div_ceil(needs.len().max(1)).max(MIN_ASK);
        for (_, ask) in &mut needs {
            *ask = (*ask).min(share).max(1);
        }
        needs
    }

    /// The next page, of at most `page` rows: once [`Pages::needs`] names
    /// no shard, every row of it is in its place. Empty while OFFSET skips
    /// rows, until the rows it skips run out: ask again.
    pub fn page(&mut self, page: usize) -> ResultSet {
        let want = self.want(page);
        let (took, _) = self.walk(want);
        let mut out: Vec<Row> = Vec::with_capacity(took.iter().sum());
        match self.order {
            Order::Shards => {
                for (i, &k) in took.iter().enumerate() {
                    out.extend(self.rows[i].drain(..k));
                }
            }
            _ => {
                let mut left = took;
                while left.iter().any(|&k| k > 0) {
                    let mut best: Option<usize> = None;
                    for i in 0..left.len() {
                        if left[i] == 0 {
                            continue;
                        }
                        if best.is_none_or(|b| self.cmp(&self.rows[i][0], &self.rows[b][0]) == Ordering::Less) {
                            best = Some(i);
                        }
                    }
                    let b = best.expect("a shard with rows to take");
                    out.push(self.rows[b].pop_front().expect("a row taken"));
                    left[b] -= 1;
                }
            }
        }
        let skipped = self.skip.min(out.len());
        self.skip -= skipped;
        let mut rs = ResultSet { columns: self.columns.clone(), rows: Vec::with_capacity(out.len() - skipped), keys: Vec::new(), scores: Vec::new(), parts: None, stats: Default::default() };
        for mut r in out.into_iter().skip(skipped) {
            for &c in &self.score_columns {
                // Rows without the ordering column's value score ±inf.
                r.values[c] = if r.score.is_finite() { crate::value::Value::Float(r.score) } else { crate::value::Value::Null };
            }
            rs.keys.push(r.key);
            rs.scores.push(r.score);
            rs.rows.push(r.values);
        }
        if let Some(l) = &mut self.left {
            *l -= rs.rows.len().min(*l);
        }
        self.q.typed(&mut rs.rows);
        rs
    }

    /// The order of two rows, as [`merge`](super::merge) sorts them.
    fn cmp(&self, a: &Row, b: &Row) -> Ordering {
        let first = match self.order {
            Order::Shards => Ordering::Equal,
            Order::Score { desc } => by_score(a.score, b.score, desc),
            Order::Path { column, desc, nulls_first } => path_order(&a.values[column], &b.values[column], desc, nulls_first),
        };
        first.then_with(|| by_key(&a.key, &b.key, self.key_desc))
    }
}
