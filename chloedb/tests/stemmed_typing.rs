//! Search as you type and typos on words the stemmer changes, found by the
//! persona study's second round (17 November 2026): a prefix that is a
//! whole word, or more of it than its stem (`headphone*`, `battery*`,
//! `running*`, `runn*`), found nothing, since the index holds stems
//! (`headphon`, `batteri`, `run`) and a prefix was matched against them
//! as typed (DEV-201); a misspelling whose stem drifts from the stored one
//! (`choclate` stems to `choclat`, three edits from `chocol`) was not
//! forgiven by `fuzzy => 'auto'`, `word~` or "did you mean" (MAYA-201); and
//! "did you mean" offered a word nobody writes (`wireles` -> `wirees`,
//! DEV-203).

use chloedb::db::Db;
use chloedb::query::exec::ResultSet;
use chloedb::query::Params;
use chloedb::storage::DbOptions;

fn ids(r: &ResultSet) -> Vec<String> {
    let mut v: Vec<String> = r.keys.iter().map(|k| k.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

fn table(dir: &std::path::Path, name: &str, rows: &[(&str, &str)]) -> Db {
    let db = Db::create_empty(&dir.join(format!("{name}.strata")), DbOptions::default()).unwrap();
    let p = Params::new();
    db.sql(&format!("CREATE TABLE {name} (id KEYWORD PRIMARY KEY, name TEXT)"), &p).unwrap();
    for (id, text) in rows {
        db.sql(&format!("INSERT INTO {name} (id, name) VALUES ('{id}', '{text}')"), &p).unwrap();
    }
    db
}

/// A shop's names: words the stemmer cuts (headphones, battery, running,
/// baby), and near neighbours that a prefix must not take for them.
fn shop(dir: &std::path::Path) -> Db {
    table(
        dir,
        "p",
        &[
            ("h1", "wireless headphones"),
            ("h2", "bluetooth headphone stand"),
            ("b1", "battery pack"),
            ("b2", "spare batteries"),
            ("r1", "running shoes"),
            ("r2", "trail runners"),
            ("r3", "run tracker"),
            ("k1", "baby stroller"),
            ("k2", "babies bottle"),
            ("c1", "car seat"),
            ("c2", "shopping cart"),
            ("w1", "wired keyboard"),
            ("w2", "wire cutters"),
            ("w3", "wire rack"),
            ("w4", "wired mouse"),
            ("u1", "happy hour mugs"),
            ("x1", "sun hat"),
        ],
    )
}

fn matched(db: &Db, q: &str) -> Vec<String> {
    let mut p = Params::new();
    p.0.insert("q".into(), chloedb::value::Value::Str(q.to_string()));
    ids(&db.query("SELECT id FROM p WHERE MATCH(name, :q)", &p).unwrap())
}

fn ranked(db: &Db, q: &str) -> Vec<String> {
    let mut p = Params::new();
    p.0.insert("q".into(), chloedb::value::Value::Str(q.to_string()));
    ids(&db.query("SELECT id FROM p ORDER BY bm25(name, :q) DESC LIMIT 20", &p).unwrap())
}

#[test]
fn a_prefix_finds_the_word_as_typed_and_its_stem() {
    let dir = tempfile::tempdir().unwrap();
    let db = shop(dir.path());
    // Each letter of "headphones" typed: the rows never go away.
    for typed in ["head", "headp", "headph", "headpho", "headphon", "headphone", "headphones"] {
        assert_eq!(matched(&db, &format!("{typed}*")), ["h1", "h2"], "{typed}*");
    }
    for typed in ["bat", "batt", "batte", "batter", "batteri", "battery", "batteries"] {
        assert_eq!(matched(&db, &format!("{typed}*")), ["b1", "b2"], "{typed}*");
    }
    // "running": the stem is run, so run's rows come with it, as the
    // word running finds them.
    assert_eq!(matched(&db, "running"), ["r1", "r3"]);
    // runn may be runner's start too.
    assert_eq!(matched(&db, "runn*"), ["r1", "r2", "r3"]);
    for typed in ["runni", "runnin", "running"] {
        assert_eq!(matched(&db, &format!("{typed}*")), ["r1", "r3"], "{typed}*");
    }
    assert_eq!(matched(&db, "run*"), ["r1", "r2", "r3"], "run* starts runner too");
    for typed in ["bab", "baby", "babie", "babies"] {
        assert_eq!(matched(&db, &format!("{typed}*")), ["k1", "k2"], "{typed}*");
    }
    assert_eq!(matched(&db, "stroller*"), ["k1"]);
    assert_eq!(matched(&db, "strollers*"), ["k1"]);
    assert_eq!(matched(&db, "happy*"), ["u1"]);
    // A word that starts another is not taken for its stem: cart is not
    // car, nor wireless wire.
    assert_eq!(matched(&db, "cart*"), ["c2"]);
    assert_eq!(matched(&db, "car*"), ["c1", "c2"]);
    assert_eq!(matched(&db, "wireless*"), ["h1"]);
    // With other words, and as the last word of a phrase.
    assert_eq!(matched(&db, "wireless headphone*"), ["h1"]);
    assert_eq!(matched(&db, "\"wireless headphone*\""), ["h1"]);
    assert_eq!(matched(&db, "\"wireless headphones*\""), ["h1"]);
    assert_eq!(matched(&db, "\"spare battery*\""), ["b2"]);
    // The stems a prefix stands for are words typed, not typos: fuzzy
    // widens neither them nor the prefix (run is one edit from sun).
    let fz = |q: &str| typo(&db, "SELECT id FROM p WHERE MATCH(name, :q, fuzzy => 'auto')", q);
    assert_eq!(fz("runni*"), ["r1", "r3"]);
    assert_eq!(fz("running*"), ["r1", "r3"]);
    assert_eq!(fz("wireles headphone*"), ["h1"]);
    // bm25 reads the prefix as MATCH does.
    assert_eq!(ranked(&db, "headphone*"), ["h1", "h2"]);
    assert_eq!(ranked(&db, "battery*"), ["b1", "b2"]);
    assert_eq!(ranked(&db, "running*"), ["r1", "r3"]);
    assert_eq!(ranked(&db, "runni*"), ["r1", "r3"]);
    let mut p = Params::new();
    p.0.insert("q".into(), chloedb::value::Value::Str("wireless headphone*".into()));
    let top = db.query("SELECT id FROM p ORDER BY bm25(name, :q) DESC LIMIT 1", &p).unwrap();
    assert_eq!(top.keys[0].as_str(), Some("h1"));
}

/// A recipe site's titles, with misspellings whose stems drift.
fn recipes(dir: &std::path::Path) -> Db {
    table(
        dir,
        "p",
        &[
            ("c1", "chocolate cake"),
            ("c2", "dark chocolate mousse"),
            ("c3", "chewy chocolaty brownies"),
            ("c4", "carrot cake"),
            ("v1", "vegetable soup"),
            ("v2", "roasted vegetables"),
            ("s1", "spaghetti carbonara"),
            ("j1", "jacket potato"),
            ("o1", "chocolate orange tart"),
            ("o2", "white chocolate cookies"),
            ("o3", "chocolate chip muffins"),
            ("o4", "hot chocolate"),
        ],
    )
}

fn typo(db: &Db, sql: &str, q: &str) -> Vec<String> {
    let mut p = Params::new();
    p.0.insert("q".into(), chloedb::value::Value::Str(q.to_string()));
    ids(&db.query(sql, &p).unwrap())
}

#[test]
fn typos_are_measured_on_the_word_as_typed() {
    let dir = tempfile::tempdir().unwrap();
    let db = recipes(dir.path());
    let chocolate = ["c1", "c2", "c3", "o1", "o2", "o3", "o4"];
    for sql in ["SELECT id FROM p WHERE MATCH(name, :q, fuzzy => 'auto')", "SELECT id FROM p WHERE MATCH(name, :q, fuzzy => 2)"] {
        assert_eq!(typo(&db, sql, "choclate"), chocolate, "{sql}");
        assert_eq!(typo(&db, sql, "vegtable"), ["v1", "v2"], "{sql}");
        assert_eq!(typo(&db, sql, "vegtables"), ["v1", "v2"], "{sql}");
        assert_eq!(typo(&db, sql, "choclate cake"), ["c1"], "{sql}");
        assert_eq!(typo(&db, sql, "spagetti"), ["s1"], "{sql}");
    }
    assert_eq!(typo(&db, "SELECT id FROM p WHERE MATCH(name, :q)", "choclate~"), chocolate);
    assert_eq!(typo(&db, "SELECT id FROM p WHERE MATCH(name, :q)", "vegtable~1"), ["v1", "v2"]);
    let first = |q: &str| typo(&db, "SELECT id FROM p ORDER BY bm25(name, :q, fuzzy => 'auto') DESC LIMIT 1", q);
    assert_eq!(first("choclate cake"), ["c1"]);
    assert_eq!(first("vegtable soup"), ["v1"]);
    assert!(typo(&db, "SELECT id FROM p ORDER BY bm25(name, :q, fuzzy => 'auto') DESC LIMIT 20", "choclate").len() >= 6);
    // A word spelled right is not taken for another's typo.
    assert_eq!(typo(&db, "SELECT id FROM p WHERE MATCH(name, :q, fuzzy => 'auto')", "carrot"), ["c4"]);
}

/// A table where each word has enough rows for "did you mean" to take a
/// typo for it (a neighbour one edit away in 6 rows or more), and more
/// rows say wire than wireless.
fn catalogue(dir: &std::path::Path) -> Db {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut add = |words: &[&str], tail: &[&str]| {
        for (i, w) in words.iter().enumerate() {
            for t in tail {
                rows.push((format!("{}{i}{t}", w.replace(' ', "_")), format!("{w} {t}")));
            }
        }
    };
    add(&["chocolate", "dark chocolate"], &["cake", "mousse", "tart", "cookies"]);
    add(&["vegetable", "roasted vegetables"], &["soup", "stew", "curry", "pie"]);
    add(&["spaghetti"], &["carbonara", "bolognese", "aglio", "puttanesca", "vongole", "pomodoro"]);
    add(&["wire", "wired"], &["cutters", "rack", "mesh", "keyboard", "mouse", "basket"]);
    add(&["wireless"], &["mouse", "keyboard", "charger", "speaker", "earbuds", "router"]);
    add(&["headphones", "headphone"], &["stand", "case", "cable", "pads"]);
    add(&["stroller"], &["fan", "hook", "cup", "bag", "cover", "light"]);
    let rows: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    table(dir, "p", &rows)
}

#[test]
fn did_you_mean_offers_words_people_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = catalogue(dir.path());
    for (typed, meant) in [
        ("choclate cake", "chocolate cake"),
        ("vegtable soup", "vegetable soup"),
        ("vegtables", "vegetables"),
        ("spagetti", "spaghetti"),
        // More rows say wire than wireless: wireles is still wireless,
        // one letter away, not "wirees", which nobody writes.
        ("wireles headphones", "wireless headphones"),
        ("headphnes", "headphones"),
        ("stroler", "stroller"),
    ] {
        let c = db.correct("p", "name", typed).unwrap();
        assert_eq!(c.text, meant, "{typed}: {c:?}");
    }
    // Words in the table stay.
    for typed in ["chocolate cake", "wire rack", "wired mouse"] {
        let c = db.correct("p", "name", typed).unwrap();
        assert!(!c.changed, "{typed}: {c:?}");
    }
}
