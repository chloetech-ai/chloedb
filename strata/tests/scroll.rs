//! Scrolls ([`strata::query::exec::Scroll`]): a query's answer read a page
//! at a time from one snapshot holds the rows the query answers at once,
//! in its order, whatever the size of a page, and after rows are written
//! and deleted between pages. A plain scan holds nothing between pages;
//! an ordered query holds its rows' places, and one of groups its rows, at
//! most as many as its start allows. Shards scroll for a coordinator,
//! which merges their pages into the answer one database would give.

use std::time::{Duration, Instant};

use std::sync::Arc;

use strata::cluster::{self, Pages};
use strata::data::{self, col};
use strata::db::Db;
use strata::query::exec::{ExecOptions, ResultSet, Scroll};
use strata::query::stats::TermStats;
use strata::query::logical::Select;
use strata::query::{compound, Params};
use strata::storage::{DbOptions, Snapshot};
use strata::value::Value;
use strata::VectorIndexKind;

const N: usize = 2400;
const SEG: usize = 300;

fn open(dir: &std::path::Path) -> (Db, Vec<strata::Document>) {
    let opts = DbOptions { segment_max_docs: SEG, ..DbOptions::default() };
    let db = Db::create(&dir.join("scroll.strata"), data::beir_schema(8, VectorIndexKind::Flat), opts).unwrap();
    let docs = data::synthetic(N, 8, 5);
    db.load(docs.iter().cloned()).unwrap();
    // Holes in the segments: deleted rows a scan passes over.
    db.sql("DELETE FROM docs WHERE category = 'c05'", &Params::new()).unwrap();
    (db, docs)
}

fn select(snap: &Snapshot, sql: &str) -> Select {
    let plan = compound::plan(sql, snap.schema(), &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"));
    plan.to_select(snap.schema()).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// The query's answer at once, with no cap on its rows.
fn whole(db: &Db, snap: &Snapshot, q: &Select) -> ResultSet {
    db.execute(snap, q, &ExecOptions::default()).unwrap()
}

/// Every page of a scroll of `q`, `page` rows at most each; also checks
/// that only the last page is short and that the scroll ends with it.
fn pages(db: &Db, snap: &Snapshot, q: &Select, page: usize, hold: usize) -> (Vec<ResultSet>, usize) {
    let opts = ExecOptions::default();
    let (first, mut s) = db.scroll(snap, q, &opts, page, hold).unwrap();
    let held = s.held();
    let mut out = vec![first];
    while !s.done() {
        let rs = db.scroll_next(snap, &mut s, &opts, page).unwrap();
        out.push(rs);
        assert!(out.len() <= N + 2, "the scroll does not end");
    }
    for (i, p) in out.iter().enumerate() {
        assert!(p.rows.len() <= page, "page {i} has {} rows", p.rows.len());
        if i + 1 < out.len() {
            assert_eq!(p.rows.len(), page, "page {i} of {} is short", out.len());
        }
    }
    // Read past the end: no rows.
    assert!(db.scroll_next(snap, &mut s, &opts, page).unwrap().rows.is_empty());
    (out, held)
}

fn joined(pages: &[ResultSet]) -> (Vec<Vec<Value>>, Vec<Value>, Vec<f64>) {
    let mut rows = Vec::new();
    let mut keys = Vec::new();
    let mut scores = Vec::new();
    for p in pages {
        rows.extend(p.rows.iter().cloned());
        keys.extend(p.keys.iter().cloned());
        scores.extend(p.scores.iter().copied());
    }
    (rows, keys, scores)
}

fn same(sql: &str, page: usize, got: &[ResultSet], want: &ResultSet) {
    let (rows, keys, scores) = joined(got);
    assert_eq!(got[0].columns, want.columns, "{sql}");
    assert_eq!(rows.len(), want.rows.len(), "{sql}, pages of {page}");
    assert_eq!(rows, want.rows, "{sql}, pages of {page}");
    assert_eq!(keys, want.keys, "{sql}, pages of {page}");
    assert_eq!(scores.len(), want.scores.len(), "{sql}");
    for (a, b) in scores.iter().zip(&want.scores) {
        assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{sql}: score {a} vs {b}");
    }
}

#[test]
fn a_scan_pages_through_every_row_and_holds_none() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(dir.path());
    let snap = db.snapshot();
    assert!(snap.segments().len() >= 8, "{} segments", snap.segments().len());
    for (sql, sizes) in [
        ("SELECT id, year, rating FROM docs", &[7, 299, 300, 301, 10_000][..]),
        ("SELECT id, category, title FROM docs WHERE year >= 2000 AND category <> 'c01'", &[13, 1000]),
        ("SELECT id FROM docs WHERE category = 'c03' LIMIT 40 OFFSET 7", &[1, 6, 40, 41]),
        ("SELECT id FROM docs LIMIT 700 OFFSET 250", &[100, 333]),
        ("SELECT id, year FROM docs OFFSET 2000", &[64]),
        ("SELECT id FROM docs WHERE year > 3000", &[10]),
    ] {
        let q = select(&snap, sql);
        let want = whole(&db, &snap, &q);
        for &page in sizes {
            let (got, held) = pages(&db, &snap, &q, page, 0);
            assert_eq!(held, 0, "{sql}: a scan holds no rows");
            same(sql, page, &got, &want);
            if want.rows.is_empty() {
                assert_eq!(got.len(), 1, "{sql}");
            }
        }
    }
}

#[test]
fn ordered_queries_and_groups_hold_their_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (db, docs) = open(dir.path());
    let snap = db.snapshot();
    let word = match &docs[3][col::TITLE] {
        Value::Str(t) => t.split(' ').next().unwrap().to_string(),
        v => panic!("{v:?}"),
    };
    for (sql, page, rows) in [
        ("SELECT id, rating FROM docs ORDER BY rating DESC, id".to_string(), 250, None),
        ("SELECT id, year FROM docs WHERE year < 2010 ORDER BY year, id LIMIT 500 OFFSET 30".into(), 64, Some(500)),
        (format!("SELECT id, score() FROM docs ORDER BY bm25(body, '{word}') DESC LIMIT 150"), 15, None),
        ("SELECT category, COUNT(*) AS n FROM docs GROUP BY category ORDER BY n DESC".into(), 5, None),
        ("SELECT COUNT(*) FROM docs WHERE year > 2000".into(), 5, Some(1)),
        ("SELECT id, title FROM docs WHERE id IN ('d0000001', 'd0000002', 'd0000700', 'd0002399')".into(), 3, Some(4)),
    ] {
        let q = select(&snap, &sql);
        let want = whole(&db, &snap, &q);
        assert!(!want.rows.is_empty(), "{sql}");
        let (got, held) = pages(&db, &snap, &q, page, 100_000);
        same(&sql, page, &got, &want);
        if let Some(n) = rows {
            assert_eq!(want.rows.len(), n, "{sql}");
        }
        // What it holds: each row's place, or (groups, keys) its rows;
        // nothing once its first page holds them all.
        assert_eq!(held, if got.len() == 1 { 0 } else { want.rows.len() }, "{sql}");
    }
}

#[test]
fn a_scroll_holds_at_most_what_its_start_allows() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(dir.path());
    let snap = db.snapshot();
    let opts = ExecOptions { max_rows: Some(10), ..ExecOptions::default() };
    let fails = |sql: &str, hold: usize| match db.scroll(&snap, &select(&snap, sql), &opts, 10, hold) {
        Ok(_) => panic!("{sql}: a scroll holding {hold} rows"),
        Err(e) => e.to_string(),
    };
    let e = fails("SELECT id FROM docs ORDER BY rating DESC", 100);
    assert!(e.contains("more than 100 rows") && e.contains("leave out ORDER BY"), "{e}");
    let e = fails("SELECT id FROM docs ORDER BY rating DESC LIMIT 90 OFFSET 20", 100);
    assert!(e.contains("reach row 110") && e.contains("at most 100"), "{e}");
    let e = fails("SELECT category, COUNT(*) FROM docs GROUP BY category", 3);
    assert!(e.contains("holds at most 3"), "{e}");
    // A scan holds nothing, and max_rows caps neither it nor its pages.
    let (first, s) = db.scroll(&snap, &select(&snap, "SELECT id FROM docs"), &opts, 50, 0).unwrap();
    assert_eq!((first.rows.len(), s.held(), s.done()), (50, 0, false));
}

#[test]
fn a_scroll_reads_the_snapshot_it_started_on() {
    let dir = tempfile::tempdir().unwrap();
    let (db, docs) = open(dir.path());
    let snap = db.snapshot();
    let opts = ExecOptions::default();
    let mut scrolls: Vec<(String, ResultSet, Vec<ResultSet>, Scroll)> = Vec::new();
    for sql in ["SELECT id, year FROM docs WHERE category <> 'c00'", "SELECT id, rating FROM docs ORDER BY rating, id"] {
        let q = select(&snap, sql);
        let want = whole(&db, &snap, &q);
        let (first, s) = db.scroll(&snap, &q, &opts, 100, 100_000).unwrap();
        scrolls.push((sql.to_string(), want, vec![first], s));
    }
    // Between pages: rows deleted, changed and added; the segments merged.
    db.sql("DELETE FROM docs WHERE year < 2000", &Params::new()).unwrap();
    db.sql("UPDATE docs SET rating = 9.5, year = 1900 WHERE year >= 2020", &Params::new()).unwrap();
    let mut more = data::synthetic(300, 8, 9);
    for (i, d) in more.iter_mut().enumerate() {
        d[col::ID] = Value::Str(format!("d{:07}", N + i));
    }
    db.load(more).unwrap();
    db.optimize().unwrap();
    assert_ne!(whole(&db, &db.snapshot(), &select(&db.snapshot(), "SELECT id FROM docs")).rows.len(), docs.len());
    for (sql, want, mut got, mut s) in scrolls {
        while !s.done() {
            got.push(db.scroll_next(&snap, &mut s, &opts, 100).unwrap());
        }
        same(&sql, 100, &got, &want);
    }
}

#[test]
fn a_page_that_fails_is_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(dir.path());
    let snap = db.snapshot();
    let q = select(&snap, "SELECT id FROM docs WHERE year >= 1995");
    let want = whole(&db, &snap, &q);
    let opts = ExecOptions::default();
    let (first, mut s) = db.scroll(&snap, &q, &opts, 500, 0).unwrap();
    let late = ExecOptions { deadline: Some(Instant::now() - Duration::from_secs(1)), ..ExecOptions::default() };
    assert!(db.scroll_next(&snap, &mut s, &late, 500).is_err());
    let mut got = vec![first];
    while !s.done() {
        got.push(db.scroll_next(&snap, &mut s, &opts, 500).unwrap());
    }
    same("after a timeout", 500, &got, &want);
}

/// The rows of `open`'s database split over three shards by row, each
/// with segments of its own and the same rows deleted.
fn shards(dir: &std::path::Path, docs: &[strata::Document]) -> Vec<Db> {
    (0..3)
        .map(|i| {
            let opts = DbOptions { segment_max_docs: SEG / 3, ..DbOptions::default() };
            let db = Db::create(&dir.join(format!("shard{i}.strata")), data::beir_schema(8, VectorIndexKind::Flat), opts).unwrap();
            db.load(docs.iter().enumerate().filter(|(n, _)| n % 3 == i).map(|(_, d)| d.clone())).unwrap();
            db.sql("DELETE FROM docs WHERE category = 'c05'", &Params::new()).unwrap();
            db
        })
        .collect()
}

/// The pages a coordinator merges from the shards' scrolls of `q` (in
/// shard mode, with the shards' statistics summed): each scroll opened
/// with a page of `open` rows, then the rows `Pages::needs` asks for.
fn merged_pages(shards: &[Db], snaps: &[Snapshot], q: &Select, opts: &ExecOptions, page: usize, open: usize) -> Vec<ResultSet> {
    let mut pages = Pages::new(q, shards.len()).unwrap();
    let mut scrolls = Vec::new();
    for (i, (d, s)) in shards.iter().zip(snaps).enumerate() {
        let (rs, sc) = d.scroll(s, q, opts, open, 1_000_000).unwrap();
        assert!(rs.rows.len() <= open);
        pages.add(i, rs, !sc.done()).unwrap();
        scrolls.push(sc);
    }
    let mut out = Vec::new();
    let mut asked = 0;
    // A first page always, as a coordinator answers one.
    loop {
        loop {
            let needs = pages.needs(page);
            if needs.is_empty() {
                break;
            }
            for (i, ask) in needs {
                assert!(ask >= 1 && ask <= 2 * page.max(cluster::MIN_ASK), "asked {ask} rows for a page of {page}");
                let rs = shards[i].scroll_next(&snaps[i], &mut scrolls[i], opts, ask).unwrap();
                pages.add(i, rs, !scrolls[i].done()).unwrap();
                asked += 1;
                assert!(asked < 100_000, "the shards are asked without end");
            }
        }
        // It holds at most what the shards answered for two pages.
        assert!(pages.held() <= shards.len() * 2 * page.max(cluster::MIN_ASK), "holds {}", pages.held());
        let p = pages.page(page);
        assert!(p.rows.len() <= page, "a page of {} rows", p.rows.len());
        out.push(p);
        if pages.done() {
            break;
        }
    }
    // Pages that only skipped OFFSET's rows are left out; the last is
    // empty when the rows ran out on a full page.
    let first = out.first().map(|p| ResultSet { rows: Vec::new(), keys: Vec::new(), scores: Vec::new(), parts: None, stats: Default::default(), columns: p.columns.clone() });
    out.retain(|p| !p.rows.is_empty());
    out.extend(first.filter(|_| out.is_empty()));
    out
}

#[test]
fn shards_scroll_pages_a_coordinator_merges_as_they_come() {
    let dir = tempfile::tempdir().unwrap();
    let docs = data::synthetic(N, 8, 5);
    let shards = shards(dir.path(), &docs);
    let snaps: Vec<Snapshot> = shards.iter().map(|d| d.snapshot()).collect();
    assert!(snaps.iter().all(|s| s.segments().len() >= 8));
    let word = match &docs[3][col::TITLE] {
        Value::Str(t) => t.split(' ').next().unwrap().to_string(),
        v => panic!("{v:?}"),
    };
    for (sql, sizes) in [
        ("SELECT id, year, rating FROM docs".to_string(), &[7, 300, 10_000][..]),
        ("SELECT id, category FROM docs WHERE year >= 2000 LIMIT 500 OFFSET 333".into(), &[64, 1000]),
        ("SELECT id FROM docs OFFSET 1500".into(), &[100]),
        ("SELECT id, rating FROM docs ORDER BY rating DESC, id".into(), &[250, 1]),
        ("SELECT id, year FROM docs WHERE year < 2010 ORDER BY year, id LIMIT 500 OFFSET 30".into(), &[64]),
        (format!("SELECT id, score() FROM docs ORDER BY bm25(body, '{word}') DESC LIMIT 150"), &[15]),
        ("SELECT id FROM docs WHERE year > 3000".into(), &[10]),
    ] {
        let q = select(&snaps[0], &sql);
        assert!(cluster::streams(&q), "{sql}");
        let stats: Vec<TermStats> = shards.iter().zip(&snaps).map(|(d, s)| d.term_stats(s, &q).unwrap()).collect();
        let opts = ExecOptions { stats: Some(Arc::new(TermStats::sum(&stats))), shard: true, ..ExecOptions::default() };
        let answers = shards.iter().zip(&snaps).map(|(d, s)| d.execute(s, &q, &opts).unwrap()).collect();
        let want = cluster::merge(&q, answers).unwrap();
        for &page in sizes {
            for open in [0, page] {
                let got = merged_pages(&shards, &snaps, &q, &opts, page, open);
                same(&format!("{sql} (opened with {open})"), page, &got, &want);
                // Every page full but the last.
                for p in got.iter().rev().skip(1) {
                    assert_eq!(p.rows.len(), page, "{sql}");
                }
            }
        }
    }
    // Groups, counts and fused scores are answered whole.
    for sql in ["SELECT category, COUNT(*) FROM docs GROUP BY category", "SELECT COUNT(*) FROM docs", "SELECT id FROM docs ORDER BY rrf(bm25(body, 'the'), bm25(body, 'of')) LIMIT 10"] {
        let q = select(&snaps[0], sql);
        assert!(!cluster::streams(&q), "{sql}");
        let opts = ExecOptions { shard: true, ..ExecOptions::default() };
        let e = shards[0].scroll(&snaps[0], &q, &opts, 10, 1000).err().unwrap_or_else(|| panic!("{sql}: a shard's scroll"));
        assert!(e.to_string().contains("answers any other query whole"), "{e}");
    }
}
