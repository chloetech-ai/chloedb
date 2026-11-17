//! A query that groups by the column that places rows on shards (the
//! table's `shard_by`, else its primary key) has every group whole on one
//! shard, so each shard sends only the groups that can be in the answer:
//! the ones `HAVING` keeps, in the query's order, up to its `OFFSET` and
//! `LIMIT`. Merged by `cluster::merge`, the answer is the unsplit
//! database's; grouped by another column, a shard sends every group.

use chloedb::cluster;
use chloedb::db::Db;
use chloedb::query::exec::{ExecOptions, ResultSet};
use chloedb::query::{bind, Params};
use chloedb::storage::DbOptions;
use chloedb::{Field, FieldType, Schema};

const ROWS: usize = 4000;
const REPOS: usize = 300;

/// Events of repo `i % REPOS` (more for low repos, so counts differ), of
/// kind `i % 5`, by person `i % 41`, with `n` commits: the rows for which
/// `holds(repo)`, in a table placed by `repo` (or keyed by it, `by_key`).
fn shard(dir: &std::path::Path, name: &str, by_key: bool, holds: impl Fn(usize) -> bool) -> Db {
    let opts = DbOptions { segment_max_docs: 300, sync: false, ..DbOptions::default() };
    let first = Schema::new("l", vec![Field::new("id", FieldType::Int)], Some("id")).unwrap();
    let db = Db::create(&dir.join(name), first, opts).unwrap();
    let none = Params::new();
    let rows: Vec<(usize, String)> = match by_key {
        false => {
            db.sql("CREATE TABLE ev (id INT PRIMARY KEY, repo INT NOT NULL, kind KEYWORD, actor INT, n INT, x FLOAT) OPTIONS(shard_by = 'repo')", &none).unwrap();
            (0..ROWS)
                .map(|i| {
                    let repo = (i * i / 97 + i) % REPOS;
                    (repo, format!("({i}, {repo}, 'k{}', {}, {}, {}.5)", i % 5, i % 41, i % 9, i % 13))
                })
                .collect()
        }
        true => {
            db.sql("CREATE TABLE ev (repo INT PRIMARY KEY, kind KEYWORD, n INT)", &none).unwrap();
            (0..REPOS).map(|r| (r, format!("({r}, 'k{}', {})", r % 5, (r * 7) % 31))).collect()
        }
    };
    let mine: Vec<&String> = rows.iter().filter(|(r, _)| holds(*r)).map(|(_, v)| v).collect();
    for chunk in mine.chunks(500) {
        let cols = if by_key { "(repo, kind, n)" } else { "(id, repo, kind, actor, n, x)" };
        db.sql(&format!("INSERT INTO ev {cols} VALUES {}", chunk.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")), &none).unwrap();
    }
    db.storage().flush().unwrap();
    db
}

/// The shards' answers in shard mode, and their merge.
fn sharded(shards: &[&Db], sql: &str) -> (Vec<ResultSet>, ResultSet) {
    let snaps: Vec<_> = shards.iter().map(|d| d.snapshot()).collect();
    let view = snaps[0].table("ev").unwrap();
    let select = bind(sql, view.schema(), &Params::new()).unwrap();
    let opts = ExecOptions { shard: true, ..ExecOptions::default() };
    let answers: Vec<ResultSet> = shards.iter().zip(&snaps).map(|(d, s)| d.execute(&s.table("ev").unwrap(), &select, &opts).unwrap()).collect();
    let merged = cluster::merge(&select, answers.clone()).unwrap();
    (answers, merged)
}

fn whole(db: &Db, sql: &str) -> ResultSet {
    db.query(sql, &Params::new()).unwrap()
}

#[test]
fn groups_of_the_placing_column_are_cut_on_each_shard() {
    let dir = tempfile::tempdir().unwrap();
    let all = shard(dir.path(), "all", false, |_| true);
    let a = shard(dir.path(), "a", false, |r| r % 3 == 0);
    let b = shard(dir.path(), "b", false, |r| r % 3 != 0);
    // (statement, the most groups a shard may send)
    let cut = [
        ("SELECT repo, COUNT(*) AS c FROM ev GROUP BY repo ORDER BY c DESC LIMIT 10", 10),
        ("SELECT repo, COUNT(*) FROM ev WHERE kind = 'k2' GROUP BY repo ORDER BY COUNT(*) DESC LIMIT 7 OFFSET 5", 12),
        ("SELECT repo, SUM(n) AS s, AVG(x), MIN(actor), MAX(actor), COUNT(DISTINCT actor) FROM ev GROUP BY repo ORDER BY s DESC LIMIT 8", 8),
        ("SELECT repo, COUNT(*) FROM ev GROUP BY repo HAVING COUNT(*) > 15 ORDER BY repo DESC LIMIT 6", 6),
        ("SELECT repo, kind, COUNT(*) FROM ev GROUP BY repo, kind ORDER BY 3 DESC, 1 LIMIT 9", 9),
        ("SELECT kind, repo, SUM(n) FROM ev GROUP BY kind, repo ORDER BY SUM(n) DESC LIMIT 5", 5),
        ("SELECT repo, PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x) AS p FROM ev GROUP BY repo ORDER BY p DESC LIMIT 4", 4),
        ("SELECT repo, COUNT(*) FROM ev GROUP BY repo LIMIT 3", 3),
        ("SELECT repo FROM ev GROUP BY repo ORDER BY repo LIMIT 5", 5),
    ];
    for (sql, most) in cut {
        let want = whole(&all, sql);
        assert!(!want.rows.is_empty(), "{sql}");
        let (answers, got) = sharded(&[&a, &b], sql);
        assert_eq!(want.columns, got.columns, "{sql}");
        assert_eq!(want.rows, got.rows, "{sql}");
        for s in &answers {
            assert!(s.rows.len() <= most, "{sql}: a shard sent {} groups", s.rows.len());
        }
        eprintln!("ok ({} rows, shards sent {:?}): {sql}", want.rows.len(), answers.iter().map(|s| s.rows.len()).collect::<Vec<_>>());
    }
    // Grouped by another column, or without a LIMIT, every group comes.
    for sql in [
        "SELECT kind, COUNT(*) FROM ev GROUP BY kind ORDER BY COUNT(*) DESC LIMIT 2",
        "SELECT actor, SUM(n) FROM ev GROUP BY actor ORDER BY SUM(n) DESC LIMIT 3",
        "SELECT repo, COUNT(*) FROM ev GROUP BY repo ORDER BY COUNT(*) DESC",
    ] {
        let want = whole(&all, sql);
        let (answers, got) = sharded(&[&a, &b], sql);
        assert_eq!(want.rows, got.rows, "{sql}");
        // Every shard's every group: more than the answer has when it is cut.
        let sent: usize = answers.iter().map(|s| s.rows.len()).sum();
        let cut = sql.contains("LIMIT");
        assert!(if cut { sent > 2 * want.rows.len() } else { sent == want.rows.len() }, "{sql}: shards sent {sent} groups for {}", want.rows.len());
    }
}

#[test]
fn groups_of_the_primary_key_are_cut_when_nothing_else_places_rows() {
    let dir = tempfile::tempdir().unwrap();
    let all = shard(dir.path(), "all", true, |_| true);
    let a = shard(dir.path(), "a", true, |r| r % 2 == 0);
    let b = shard(dir.path(), "b", true, |r| r % 2 == 1);
    let sql = "SELECT repo, SUM(n) AS s FROM ev GROUP BY repo ORDER BY s DESC, repo LIMIT 6";
    let want = whole(&all, sql);
    let (answers, got) = sharded(&[&a, &b], sql);
    assert_eq!(want.rows, got.rows);
    assert!(answers.iter().all(|s| s.rows.len() <= 6));
}
