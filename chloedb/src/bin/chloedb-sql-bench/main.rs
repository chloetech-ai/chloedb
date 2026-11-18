//! Times analytical SQL: groups, aggregates, joins, windows and functions
//! of times over a table of a few hundred thousand rows.
//!
//! ```text
//! chloedb-sql-bench [--rows N] [--seed S] [--reps R] [--out FILE.json] [--check] [--plan]
//! chloedb-sql-bench --db DIR --queries FILE.tsv [--reps R] [--out FILE.json] [--plan]
//! ```
//!
//! The first loads tables shaped like NYC's green taxi trips (`taxi.rs`:
//! 224,000 trips by default, made up from the seed, and 265 zones) into a
//! new database in a temporary folder and runs the persona study's
//! battery on them; the second runs the statements of a file
//! (`id<TAB>statement` a line) on a database that exists, read only.
//!
//! Prints `id<TAB>median ms<TAB>rows` per statement: the median of `--reps`
//! runs (default 7) after two warm-ups, on as many threads as the machine
//! has cores (or `--threads`). `--out` writes the same as JSON, with the load's seconds,
//! for the speed gate (`scripts/perf-gate.py --sql`). `--check` also runs
//! each statement on the operators alone and fails when an answer differs
//! from the default engine's (floats within 1e-9, rows in any order
//! without an ORDER BY). `--plan` prints each one's plan and the steps it
//! took, with their times.

use std::time::Instant;

use chloedb::db::Db;
use chloedb::query::exec::{Engine, ExecOptions, ResultSet};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;

mod taxi;

struct Args {
    threads: Option<usize>,
    db: Option<String>,
    queries: Option<String>,
    rows: usize,
    seed: u64,
    reps: usize,
    out: Option<String>,
    check: bool,
    plan: bool,
}

fn args() -> Args {
    let mut a = Args { threads: None, db: None, queries: None, rows: taxi::ROWS, seed: 7, reps: 7, out: None, check: false, plan: false };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| usage(&format!("{k} needs a value")));
        match k.as_str() {
            "--db" => a.db = Some(v()),
            "--queries" => a.queries = Some(v()),
            "--rows" => a.rows = v().replace('_', "").parse().unwrap_or_else(|_| usage("--rows takes a number")),
            "--seed" => a.seed = v().parse().unwrap_or_else(|_| usage("--seed takes a number")),
            "--reps" => a.reps = v().parse().unwrap_or_else(|_| usage("--reps takes a number")),
            "--out" => a.out = Some(v()),
            "--threads" => a.threads = Some(v().parse().unwrap_or_else(|_| usage("--threads takes a number"))),
            "--check" => a.check = true,
            "--plan" => a.plan = true,
            "-h" | "--help" => usage(""),
            _ => usage(&format!("unknown argument {k}")),
        }
    }
    if a.db.is_some() != a.queries.is_some() {
        usage("--db and --queries go together");
    }
    a.reps = a.reps.max(1);
    a
}

fn usage(why: &str) -> ! {
    if !why.is_empty() {
        eprintln!("chloedb-sql-bench: {why}");
    }
    eprintln!("usage: chloedb-sql-bench [--rows N] [--seed S] [--reps R] [--out FILE.json] [--check] [--plan]");
    eprintln!("       chloedb-sql-bench --db DIR --queries FILE.tsv [--reps R] [--out FILE.json] [--plan]");
    std::process::exit(if why.is_empty() { 0 } else { 2 });
}

/// A row's sort key, with floats rounded: engines add floats in other
/// orders.
fn key(row: &[Value]) -> String {
    row.iter()
        .map(|v| match v {
            Value::Float(f) => format!("{f:.6e}"),
            v => format!("{v:?}"),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Why `got` is not `want`'s answer to `sql`: in the same order under
/// ORDER BY, as the same rows otherwise, floats within 1e-9 relative.
fn differs(sql: &str, want: &ResultSet, got: &ResultSet) -> Option<String> {
    if want.rows.len() != got.rows.len() {
        return Some(format!("{} rows against {}", got.rows.len(), want.rows.len()));
    }
    let (mut a, mut b): (Vec<&Vec<Value>>, Vec<&Vec<Value>>) = (want.rows.iter().collect(), got.rows.iter().collect());
    if !sql.contains("ORDER BY") || sql.contains(" OVER (") {
        a.sort_by_key(|r| key(r));
        b.sort_by_key(|r| key(r));
    }
    for (x, y) in a.iter().zip(&b) {
        let same = x.len() == y.len()
            && x.iter().zip(y.iter()).all(|(p, q)| match (p, q) {
                (Value::Float(p), Value::Float(q)) => (p - q).abs() <= 1e-9 * p.abs().max(q.abs()).max(1.0),
                (p, q) => p == q,
            });
        if !same {
            return Some(format!("{y:?} where the operators give {x:?}"));
        }
    }
    None
}

fn main() {
    let a = args();
    let t = Instant::now();
    let tmp;
    let (mut db, statements): (Db, Vec<(String, String)>) = match (&a.db, &a.queries) {
        (Some(dir), Some(file)) => {
            let db = Db::open(std::path::Path::new(dir), DbOptions { read_only: true, publish_snapshots: true, ..DbOptions::default() })
                .unwrap_or_else(|e| usage(&format!("{dir}: {e}")));
            let text = std::fs::read_to_string(file).unwrap_or_else(|e| usage(&format!("{file}: {e}")));
            let qs = text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(|l| {
                    let (id, sql) = l.split_once('\t').unwrap_or_else(|| usage(&format!("{file}: a line is id<TAB>statement: {l}")));
                    (id.to_string(), sql.to_string())
                })
                .collect();
            (db, qs)
        }
        _ => {
            tmp = std::env::temp_dir().join(format!("chloedb-sql-bench-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).expect("a temporary folder");
            let db = Db::create(&tmp.join("taxi.chloedb"), taxi::zones_schema(), DbOptions { sync: false, ..DbOptions::default() }).expect("a database");
            taxi::create(&db, a.rows, a.seed);
            (db, taxi::QUERIES.iter().map(|(id, sql)| (id.to_string(), sql.to_string())).collect())
        }
    };
    let load_s = t.elapsed().as_secs_f64();
    eprintln!("ready in {load_s:.1} s");
    db.exec.threads = a.threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let p = Params::new();
    let ops = ExecOptions { engine: Engine::Operators, ..db.exec.clone() };
    let mut results = Vec::new();
    let mut wrong = Vec::new();
    for (id, sql) in &statements {
        let mut last = None;
        for _ in 0..2 {
            last = Some(db.query(sql, &p));
        }
        let rs = match last.expect("a run") {
            Ok(rs) => rs,
            Err(e) => {
                println!("{id}\terror\t{}", e.to_string().chars().take(160).collect::<String>());
                results.push(format!("{:?}: {{\"error\": {:?}}}", id, e.to_string()));
                continue;
            }
        };
        let mut ms: Vec<f64> = (0..a.reps)
            .map(|_| {
                let s = Instant::now();
                let _ = db.query(sql, &p);
                s.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        let median = ms[ms.len() / 2];
        println!("{id}\t{median:.2}\t{}", rs.rows.len());
        if a.plan {
            for l in &rs.stats.plan {
                println!("\t{l}");
            }
            for st in &rs.stats.steps {
                println!("\t{}{} {}: {:.2} ms", "  ".repeat(st.depth as usize + 1), st.op, st.detail, st.us / 1e3);
            }
        }
        if a.check {
            match db.query_with(sql, &p, &ops) {
                Ok(want) => {
                    if let Some(why) = differs(sql, &want, &rs) {
                        wrong.push(format!("{id}: {why}"));
                    }
                }
                Err(e) => wrong.push(format!("{id}: the operators fail: {e}")),
            }
        }
        results.push(format!("{:?}: {{\"ms\": {median:.3}, \"rows\": {}}}", id, rs.rows.len()));
    }
    if let Some(out) = &a.out {
        let json = format!("{{\"load_s\": {load_s:.3}, \"rows\": {}, \"queries\": {{{}}}}}\n", a.rows, results.join(", "));
        std::fs::write(out, json).unwrap_or_else(|e| usage(&format!("{out}: {e}")));
    }
    drop(db);
    if a.db.is_none() {
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("chloedb-sql-bench-{}", std::process::id())));
    }
    if !wrong.is_empty() {
        for w in &wrong {
            eprintln!("differs: {w}");
        }
        std::process::exit(1);
    }
}
