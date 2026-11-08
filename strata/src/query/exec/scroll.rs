//! A query's answer read a page at a time from one snapshot, as
//! Elasticsearch's scroll reads one ([`Scroll`]): a page holds at most the
//! rows asked of it, and the scroll as many as the query answers, so that
//! a server's cap on the rows of an answer (`--max-result-window`) caps
//! each page instead of the whole.
//!
//! A plain scan (no ORDER BY, groups, counts, guard, rerank phases or
//! graphs) resumes after the place of its last row, in the order of the
//! snapshot's segments and docIDs, and holds nothing between pages but
//! that place and the filter of the segment it is in. Any other query
//! runs once, when the scroll starts: an ordered one holds its rows'
//! places and scores in order and reads each page's rows from them; one of
//! groups, counts or graphs holds its rows. Either holds at most the
//! `hold` rows its start allows.

use std::sync::Arc;

use super::{agg, err, Executor, Hit, ResultSet};
use crate::error::Result;
use crate::query::logical::{Projection, Select};
use crate::segment::{DocId, DocSet};

/// A row's place in the snapshot a query read: its segment's ordinal and
/// its docID.
type Place = (usize, DocId);

/// What is left to read of a query's answer ([`Executor::scroll`],
/// [`crate::db::Db::scroll`]). Its places are those of the snapshot it
/// started on, which each page must read.
pub struct Scroll {
    columns: Vec<String>,
    at: Paging,
}

enum Paging {
    /// A plain scan's rows after `after` (`None`: from the first), at most
    /// `left` more (its LIMIT's; `None`: no LIMIT), and the filter of the
    /// segment it is in.
    Scan { q: Box<Select>, after: Option<Place>, left: Option<usize>, set: Option<(usize, Arc<DocSet>)> },
    /// An ordered query's rows, by place and score, the next at `at`.
    Held { q: Box<Select>, hits: Vec<Hit>, at: usize },
    /// An answer made of rows (groups, counts, graphs), the next at `at`.
    Rows { rs: ResultSet, at: usize },
    Done,
}

impl Scroll {
    /// Whether every row has been read.
    pub fn done(&self) -> bool {
        matches!(self.at, Paging::Done)
    }

    /// The rows, or rows' places, the scroll holds (none for a scan): what a
    /// server counts against what its scrolls may hold.
    pub fn held(&self) -> usize {
        match &self.at {
            Paging::Held { hits, .. } => hits.len(),
            Paging::Rows { rs, .. } => rs.rows.len(),
            Paging::Scan { .. } | Paging::Done => 0,
        }
    }

    /// Whether its next page reads the snapshot (not when it holds its
    /// rows, or has none left).
    pub fn reads(&self) -> bool {
        matches!(self.at, Paging::Scan { .. } | Paging::Held { .. })
    }

    /// The scroll of an answer already made (rows read by key): its first
    /// page of at most `page` rows, and the rest.
    pub fn of_rows(rs: ResultSet, page: usize) -> (ResultSet, Scroll) {
        let mut s = Scroll { columns: rs.columns.clone(), at: Paging::Rows { rs, at: 0 } };
        let first = s.next_rows(page);
        (first, s)
    }

    /// The next page of an answer the scroll holds: at most `page` rows.
    /// Empty when it holds none (a scan, or the last page read).
    pub fn next_rows(&mut self, page: usize) -> ResultSet {
        let Paging::Rows { rs, at } = &mut self.at else {
            return ResultSet { columns: self.columns.clone(), rows: Vec::new(), keys: Vec::new(), scores: Vec::new(), parts: None, stats: Default::default() };
        };
        let end = at.saturating_add(page.max(1)).min(rs.rows.len());
        let out = ResultSet {
            columns: self.columns.clone(),
            rows: rs.rows[*at..end].to_vec(),
            keys: rs.keys.get(*at..end).map_or_else(Vec::new, <[_]>::to_vec),
            scores: rs.scores.get(*at..end).map_or_else(Vec::new, <[_]>::to_vec),
            parts: None,
            stats: if *at == 0 { std::mem::take(&mut rs.stats) } else { Default::default() },
        };
        *at = end;
        if end == rs.rows.len() {
            self.at = Paging::Done;
        }
        out
    }
}

impl<'a> Executor<'a> {
    /// Starts a scroll of `q`: its first page, of at most `page` rows, and
    /// the scroll that reads the rest ([`Executor::scroll_next`]). A query
    /// other than a plain scan may hold at most `hold` rows: one that
    /// answers more fails. Expired rows are those of the scroll's start.
    pub fn scroll(mut self, q: &Select, page: usize, hold: usize) -> Result<(ResultSet, Scroll)> {
        if self.opts.shard {
            return Err(err("a scroll reads a database's answer, not a shard's for a coordinator"));
        }
        let page = page.max(1);
        let grouped = !q.group_by.is_empty() || q.projection.iter().any(|(_, p)| matches!(p, Projection::Count | Projection::Agg { .. })) || agg::is_general(q, |f| self.facet_field(f));
        if grouped || !q.graphs.is_empty() {
            let rs = match self.answer(q, None)? {
                super::Leaf::Rows(rs) => rs,
                #[cfg(feature = "datafusion")]
                super::Leaf::Arrow(_) => unreachable!("rows asked for"),
            };
            if rs.rows.len() > hold {
                return Err(err(format!("the query answers {} rows; a scroll of groups, counts or graphs holds at most {hold}", rs.rows.len())));
            }
            return Ok(Scroll::of_rows(rs, page));
        }
        let unexpired = self.unexpired(q);
        let q = unexpired.as_ref();
        let m = self.mark();
        let settled = self.settle_guard(q)?;
        if q.guard.is_some() {
            self.step(m, "guard", String::new(), None);
        }
        let q = settled.as_ref().unwrap_or(q);
        self.refuse(q)?;
        let columns: Vec<String> = q.projection.iter().map(|(n, _)| n.clone()).collect();
        let scan = q.order.is_none() && q.phases.is_empty() && q.per.is_none();
        let mut s = if scan {
            Scroll { columns, at: Paging::Scan { q: Box::new(Select { offset: 0, limit: None, ..q.clone() }), after: None, left: q.limit, set: None } }
        } else {
            let m = self.mark();
            let mut hits = self.held_hits(q, hold)?;
            let (op, detail) = self.order_label(q);
            self.step(m, op, detail, Some(hits.len() as u64));
            hits.drain(..q.offset.min(hits.len()));
            Scroll { columns, at: Paging::Held { q: Box::new(q.clone()), hits, at: 0 } }
        };
        let first = self.page(&mut s, page, if scan { q.offset } else { 0 })?;
        Ok((first, s))
    }

    /// The next page of scroll `s`, of at most `page` rows, read from the
    /// snapshot it started on. The scroll moves on only once the page is
    /// read: a page that fails is read again by the next call.
    pub fn scroll_next(mut self, s: &mut Scroll, page: usize) -> Result<ResultSet> {
        self.page(s, page.max(1), 0)
    }

    /// The next page of `s`, at most `page` rows after the first `skip`
    /// (a scan's OFFSET, on its first page).
    fn page(&mut self, s: &mut Scroll, page: usize, skip: usize) -> Result<ResultSet> {
        let columns = &s.columns;
        match &mut s.at {
            Paging::Rows { .. } | Paging::Done => Ok(s.next_rows(page)),
            Paging::Scan { q, after, left, set } => {
                let n = left.map_or(page, |l| l.min(page));
                // One row past the page shows whether another follows.
                let m = self.mark();
                let mut hits = self.scan_from(q, *after, set, skip.saturating_add(n).saturating_add(1))?;
                let resumed = after.map_or("the first".into(), |(seg, doc)| format!("seg {seg} doc {doc}"));
                self.step(m, "scan", format!("rows WHERE keeps, {n} from {resumed}"), Some(hits.len() as u64));
                hits.drain(..skip.min(hits.len()));
                let more = hits.len() > n;
                hits.truncate(n);
                let rs = self.rows_of(q, &hits, columns)?;
                let rest = left.map(|l| l - hits.len());
                match hits.last() {
                    Some(h) if more && rest != Some(0) => (*after, *left) = (Some((h.seg, h.doc)), rest),
                    _ => s.at = Paging::Done,
                }
                Ok(rs)
            }
            Paging::Held { q, hits, at } => {
                let end = at.saturating_add(page).min(hits.len());
                let rs = self.rows_of(q, &hits[*at..end], columns)?;
                *at = end;
                if end == hits.len() {
                    s.at = Paging::Done;
                }
                Ok(rs)
            }
        }
    }

    /// The rows of a plain scan after `after`, at most `want` of them,
    /// keeping the filter of the segment the last is in.
    fn scan_from(&mut self, q: &Select, after: Option<Place>, kept: &mut Option<(usize, Arc<DocSet>)>, want: usize) -> Result<Vec<Hit>> {
        let mut out = Vec::new();
        let segs = self.segs.clone();
        let mut tick = self.ticker();
        for s in segs.iter().filter(|s| after.is_none_or(|(seg, _)| s.ord >= seg)) {
            self.check_deadline()?;
            if out.len() >= want {
                break;
            }
            let set = match kept {
                Some((ord, set)) if *ord == s.ord => set.clone(),
                _ => {
                    let set = self.filter_set(s, &q.filter)?;
                    *kept = Some((s.ord, set.clone()));
                    set
                }
            };
            let from = match after {
                Some((seg, doc)) if seg == s.ord => doc.saturating_add(1),
                _ => 0,
            };
            for d in members_from(&set, from) {
                tick.tick()?;
                if out.len() >= want {
                    break;
                }
                self.charge.room(&mut out)?;
                out.push(Hit { seg: s.ord, doc: d, score: 0.0 });
            }
        }
        Ok(out)
    }

    /// An ordered query's rows, OFFSET included, at most `hold` of them.
    fn held_hits(&mut self, q: &Select, hold: usize) -> Result<Vec<Hit>> {
        let reach = q.limit.unwrap_or(0).saturating_add(q.offset);
        if reach > hold {
            return Err(err(format!("LIMIT and OFFSET reach row {reach}; a scroll of an ordered query holds at most {hold} rows (one without ORDER BY holds none)")));
        }
        let hits = if q.phases.is_empty() {
            let want = q.limit.map_or(hold.saturating_add(1), |l| l.saturating_add(q.offset)).min(self.n_live());
            self.ordered_hits_to(q, Some(want))?
        } else {
            self.phased(q, Some(hold))?
        };
        if q.limit.is_none() && hits.len() > hold {
            return Err(err(format!("the query matches more than {hold} rows, the most a scroll of an ordered query holds: add a LIMIT, or leave out ORDER BY to scroll any number")));
        }
        Ok(hits)
    }

    /// The rows of `q`'s projection at `hits`, typed, with their keys and
    /// scores, and the steps that read them.
    fn rows_of(&mut self, q: &Select, hits: &[Hit], columns: &[String]) -> Result<ResultSet> {
        let m = self.mark();
        let mut rows = self.project(q, hits)?;
        self.step(m, "project", format!("{} column{}", columns.len(), if columns.len() == 1 { "" } else { "s" }), Some(rows.len() as u64));
        q.typed(&mut rows);
        let keys = hits.iter().map(|h| self.key(h)).collect();
        let scores = hits.iter().map(|h| h.score).collect();
        Ok(ResultSet { columns: columns.to_vec(), rows, keys, scores, parts: None, stats: std::mem::take(&mut self.stats) })
    }
}

/// The members of `set` from `from` on, in increasing order: where a scan
/// resumes in a segment.
fn members_from(set: &DocSet, from: DocId) -> Box<dyn Iterator<Item = DocId> + '_> {
    match set {
        DocSet::Sorted { docs, .. } => Box::new(docs[docs.partition_point(|&d| d < from)..].iter().copied()),
        DocSet::Bits { words, .. } => {
            let first = from as usize / 64;
            Box::new(words.iter().enumerate().skip(first).flat_map(move |(i, &w)| {
                let mut w = if i == first { w & (!0u64 << (from % 64)) } else { w };
                std::iter::from_fn(move || {
                    (w != 0).then(|| {
                        let t = w.trailing_zeros();
                        w &= w - 1;
                        i as DocId * 64 + t
                    })
                })
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_from_resumes_where_asked() {
        let n = 1000;
        let dense: Vec<u32> = (0..n).filter(|x| x % 3 == 0).collect();
        let sparse: Vec<u32> = (0..n).filter(|x| x % 70 == 1).collect();
        for docs in [dense, sparse] {
            let set = DocSet::from_sorted(docs.clone(), n);
            for from in [0, 1, 63, 64, 65, 127, 128, 500, 998, 999, 1000, 5000] {
                let want: Vec<u32> = docs.iter().copied().filter(|&d| d >= from).collect();
                assert_eq!(members_from(&set, from).collect::<Vec<_>>(), want, "from {from}");
            }
        }
        assert!(matches!(DocSet::from_sorted((0..n).filter(|x| x % 3 == 0).collect(), n), DocSet::Bits { .. }));
    }
}
