//! `PACK n TOKENS` ([`chloedb::query::pack`]): a query's rows taken to
//! fill a budget of tokens, on the audit's worked example: an agent asks
//! memory how payments retry should back off, with 1,500 tokens to spend.

use chloedb::db::{Db, SqlResult};
use chloedb::query::pack::count_tokens;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

fn run(db: &Db, sql: &str) -> SqlResult {
    db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The worked example's memory, and rows of other work around it.
fn memory(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("m.strata"), seed, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    run(&db, "CREATE TABLE memory (id KEYWORD PRIMARY KEY, kind KEYWORD, text TEXT, superseded BOOL, links EDGES)");
    let log: String = (0..400)
        .map(|i| match i {
            200 => "ERROR payments retry 7 failed: backoff exceeded cap after 5 s, giving up on charge ch_91. ".to_string(),
            _ => format!("INFO worker {i} heartbeat ok queue depth {} latency {} ms. ", i % 13, i % 97),
        })
        .collect();
    let rows: Vec<(&str, &str, String, bool, &str)> = vec![
        ("d1", "decision", "Decision: payments retry uses backoff v2, exponential from 100 ms, capped at 5 s, with full jitter, so a retry storm cannot pile up on the processor.".into(), false, "[]"),
        ("e2", "episode", "Episode: on Tuesday a retry storm in payments took the processor down for twenty minutes; every client retried at once with a fixed one second delay and no backoff.".into(), false, "[]"),
        ("n3", "note", "Note: payments retry uses exponential backoff capped at 5 s with jitter.".into(), false, "[]"),
        ("t4", "tool", log, false, "[]"),
        ("f5", "file", "File note for retry.rs: the payments retry loop lives in retry.rs; it reads the backoff policy from config and wraps each charge.".into(), false, "[\"d6\"]"),
        ("d6", "decision", "Decision: every charge request must carry an idempotency key, so that a repeated request is applied once.".into(), false, "[]"),
        ("s7", "decision", "Decision (old): payments retry waits a fixed one second between attempts, no backoff.".into(), true, "[]"),
        ("o8", "note", "Note: the search page caches facets for a minute.".into(), false, "[]"),
    ];
    let values: Vec<String> = rows.iter().map(|(id, kind, text, sup, links)| format!("('{id}', '{kind}', {}, {sup}, '{links}')", q(text))).collect();
    run(&db, &format!("INSERT INTO memory (id, kind, text, superseded, links) VALUES {}", values.join(", ")));
    db
}

const PACKED: &str = "SELECT id, kind, text FROM memory WHERE superseded = false ORDER BY bm25(text, :task) DESC PACK 1500 TOKENS EXCERPT 300 DEDUPE 0.8 FOLLOW links 1 HOP WITH RECEIPT";

fn task() -> Params {
    Params::new().set("task", Value::Str("payments retry backoff policy".into()))
}

fn col(cols: &[String], name: &str) -> usize {
    cols.iter().position(|c| c == name).unwrap_or_else(|| panic!("no column {name} in {cols:?}"))
}

#[test]
fn packs_the_worked_example() {
    let dir = tempfile::tempdir().unwrap();
    let db = memory(dir.path());
    let p = db.pack(PACKED, &task()).unwrap();
    let rs = &p.rs;
    let (id, text, tokens, decision, used) = (col(&rs.columns, "id"), col(&rs.columns, "text"), col(&rs.columns, "pack_tokens"), col(&rs.columns, "pack_decision"), col(&rs.columns, "pack_used"));
    assert_eq!(&rs.columns[..3], &["id", "kind", "text"]);
    let of = |k: &str| rs.rows.iter().position(|r| r[id] == Value::Str(k.into()));
    let decision_of = |k: &str| rs.rows[of(k).unwrap_or_else(|| panic!("{k} decided: {:?}", p.receipt.rows))][decision].as_str().unwrap().to_string();
    let summary: Vec<String> = rs.rows.iter().map(|r| format!("{} {:?} {}", r[id].as_str().unwrap(), r[tokens], r[decision].as_str().unwrap())).collect();

    // The superseded decision is not a candidate; the unrelated note does
    // not match.
    assert!(of("s7").is_none() && of("o8").is_none(), "{summary:?}");
    // The short note says what the decision says: one of the two is
    // skipped as the other's near-duplicate, whichever ranked lower.
    let (d1, n3) = (decision_of("d1"), decision_of("n3"));
    assert!(d1 == "skip: near-duplicate of n3" && n3 == "take" || n3 == "skip: near-duplicate of d1" && d1 == "take", "{summary:?}");
    // The log dump is cut to the lines around the error.
    assert_eq!(decision_of("t4"), "excerpt", "{summary:?}");
    let cut = rs.rows[of("t4").unwrap()][text].as_str().unwrap();
    assert!(cut.contains("backoff exceeded cap"), "{cut}");
    assert!(count_tokens(cut) <= 300, "{}", count_tokens(cut));
    // The idempotency decision matches no word of the task, and comes by
    // its link from the file note, right after it.
    assert_eq!(decision_of("d6"), "linked from f5", "{summary:?}");
    assert_eq!(of("d6").unwrap(), of("f5").unwrap() + 1, "{summary:?}");
    // The budget holds, and the receipt adds up.
    let spent: i64 = rs.rows.iter().map(|r| match r[tokens] {
        Value::Int(n) => n,
        _ => panic!(),
    }).sum();
    assert!(spent as usize <= 1500);
    assert_eq!(spent as usize, p.receipt.used);
    assert_eq!(rs.rows.last().unwrap()[used], Value::Int(spent));
    assert_eq!(p.receipt.rows.len(), rs.rows.len());
    assert_eq!(p.receipt.txn, Some(db.snapshot().txn()));
    for (r, d) in rs.rows.iter().zip(&p.receipt.rows) {
        assert_eq!(r[id], d.key);
        assert_eq!(r[decision], Value::Str(d.decision.clone()));
        assert_eq!(d.tokens == 0, d.decision.starts_with("skip"));
    }
}

#[test]
fn answers_the_rows_taken_through_sql() {
    let dir = tempfile::tempdir().unwrap();
    let db = memory(dir.path());
    let sql = PACKED.replace(" WITH RECEIPT", "");
    let SqlResult::Rows(rs) = db.sql(&sql, &task()).unwrap() else { panic!("rows") };
    assert_eq!(rs.columns, vec!["id", "kind", "text", "pack_tokens", "pack_decision"]);
    assert!(rs.rows.iter().all(|r| !r[4].as_str().unwrap().starts_with("skip")));
    assert_eq!(rs.rows.len(), rs.keys.len());
    // The same through Db::query.
    assert_eq!(db.query(&sql, &task()).unwrap().rows, rs.rows);
    // The plan says what the pack did.
    assert!(rs.stats.plan.iter().any(|l| l.starts_with("pack: ")), "{:?}", rs.stats.plan);
}

#[test]
fn a_small_budget_takes_the_best_rows_whole_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("n.strata"), seed, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    run(&db, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT, pack INT)");
    let values: Vec<String> = (0..2000).map(|i| format!("({i}, 'note {i} about the retry {}', {})", "policy ".repeat(i % 5), i % 3)).collect();
    run(&db, &format!("INSERT INTO notes (id, body, pack) VALUES {}", values.join(", ")));
    let p = db.pack("SELECT id, body FROM notes ORDER BY bm25(body, 'retry policy') DESC PACK 100 TOKENS WITH RECEIPT", &Params::new()).unwrap();
    // One slice was read: its first rows filled the budget.
    assert_eq!(p.receipt.queries, 1);
    assert!(p.receipt.used <= 100 && p.receipt.used > 100 - 32, "{}", p.receipt.used);
    // The rows taken are the first of the query's order, whole.
    let top = db.query("SELECT id FROM notes ORDER BY bm25(body, 'retry policy') DESC LIMIT 64", &Params::new()).unwrap();
    let taken: Vec<&Value> = p.receipt.rows.iter().filter(|d| d.taken).map(|d| &d.key).collect();
    assert_eq!(taken, top.keys[..taken.len()].iter().collect::<Vec<_>>());
    assert!(p.receipt.rows.iter().all(|d| d.decision == "take" || d.decision == "skip: over budget"));
    // The id is a number, so it is not counted: only body is.
    let body = p.rs.columns.iter().position(|c| c == "body").unwrap();
    for (r, d) in p.rs.rows.iter().zip(&p.receipt.rows).filter(|(_, d)| d.taken) {
        assert_eq!(d.tokens, count_tokens(r[body].as_str().unwrap()));
    }
    // A column named pack is a column.
    let rs = db.query("SELECT pack FROM notes WHERE pack > 1 ORDER BY id LIMIT 2", &Params::new()).unwrap();
    assert_eq!(rs.rows, vec![vec![Value::Int(2)], vec![Value::Int(2)]]);
    // LIMIT caps the candidates read, and OFFSET starts them later.
    let p = db.pack("SELECT id, body FROM notes ORDER BY bm25(body, 'retry policy') DESC PACK 100000 TOKENS LIMIT 10 OFFSET 5", &Params::new()).unwrap();
    assert_eq!(p.receipt.rows.len(), 10);
    assert_eq!(p.receipt.rows[0].key, top.keys[5]);
}

#[test]
fn of_names_the_columns_counted_and_errors_say_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let db = memory(dir.path());
    let p = db.pack("SELECT id, kind, text FROM memory ORDER BY bm25(text, :task) DESC PACK 2000 TOKENS OF kind", &task()).unwrap();
    for (r, d) in p.rs.rows.iter().zip(&p.receipt.rows) {
        assert_eq!(d.tokens, count_tokens(r[1].as_str().unwrap()));
    }
    let e = |sql: &str| db.pack(sql, &task()).unwrap_err().to_string();
    assert!(e("SELECT id, text FROM memory PACK 100 TOKENS OF body").contains("no column body"));
    assert!(e("SELECT id FROM memory ORDER BY bm25(text, :task) DESC PACK 100 TOKENS").contains("text columns"));
    assert!(e("SELECT id, text FROM memory PACK 100 TOKENS FOLLOW links 9 HOPS").contains("at most 3"));
    assert!(e("SELECT id, text FROM memory").contains("no PACK"));
}

#[test]
fn dedupe_on_a_vector_field_compares_meanings() {
    let dir = tempfile::tempdir().unwrap();
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("v.strata"), seed, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    run(&db, "CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT, rank INT, emb VECTOR(3))");
    run(
        &db,
        "INSERT INTO notes (id, body, rank, emb) VALUES \
         ('a', 'retries back off exponentially up to five seconds', 3, '[1.0, 0.0, 0.0]'), \
         ('b', 'the wait between attempts doubles, at most 5 s', 2, '[0.98, 0.05, 0.0]'), \
         ('c', 'charges carry idempotency keys', 1, '[0.0, 1.0, 0.0]')",
    );
    let p = db.pack("SELECT id, body FROM notes ORDER BY rank DESC PACK 500 TOKENS DEDUPE 0.9 ON emb WITH RECEIPT", &Params::new()).unwrap();
    // The vector is read for the comparison, not answered.
    assert_eq!(p.rs.columns, vec!["id", "body", "pack_tokens", "pack_decision", "pack_score", "pack_used"]);
    let decisions: Vec<(String, String)> = p.receipt.rows.iter().map(|d| (d.key.as_str().unwrap().to_string(), d.decision.clone())).collect();
    assert_eq!(decisions, vec![("a".into(), "take".into()), ("b".into(), "skip: near-duplicate of a".into()), ("c".into(), "take".into())]);
    // The two say it in different words: without ON, both are taken.
    let p = db.pack("SELECT id, body FROM notes ORDER BY rank DESC PACK 500 TOKENS DEDUPE 0.9", &Params::new()).unwrap();
    assert_eq!(p.rs.rows.len(), 3);
}
