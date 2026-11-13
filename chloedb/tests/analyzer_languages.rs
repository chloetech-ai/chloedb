//! The `standard` and `cjk` analyzers and the languages' through SQL: text in
//! scripts that `english` and `simple` cannot cut into words (Chinese,
//! Japanese, Korean, Thai, Hindi) and languages with their own stemmers, found by
//! MATCH words, phrases and prefixes and ranked by bm25, in the memtable,
//! in segments, merged and reopened; and the features their fields need.

use std::path::Path;

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::{Analyzer, Field, FieldType, Schema};
use chloedb::storage::{DbOptions, MergePolicy};
use chloedb::text::Language;
use chloedb::value::Value;

fn run(db: &Db, sql: &str, params: &Params) -> SqlResult {
    db.sql(sql, params).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// The ids of the rows whose `field` matches `q`, sorted.
fn matches(db: &Db, field: &str, q: &str) -> Vec<String> {
    let p = Params::new().set("q", Value::Str(q.into()));
    let SqlResult::Rows(r) = run(db, &format!("SELECT id FROM docs WHERE MATCH({field}, :q)"), &p) else { panic!() };
    let mut v: Vec<String> = r.rows.into_iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

/// The ids bm25 ranks for `q` with a score, best first.
fn ranked(db: &Db, field: &str, q: &str) -> Vec<String> {
    let p = Params::new().set("q", Value::Str(q.into()));
    let SqlResult::Rows(r) = run(db, &format!("SELECT id, score() FROM docs ORDER BY bm25({field}, :q) DESC LIMIT 10"), &p) else { panic!() };
    r.rows.into_iter().filter(|r| r[1].as_f64().unwrap() > 0.0).map(|r| r[0].as_str().unwrap().to_string()).collect()
}

const ROWS: [(&str, &str); 12] = [
    ("ja1", "東京都庁に行きました。東京は大きい都市です。"),
    ("ja2", "京都のお寺を見ました。"),
    ("zh1", "我爱北京天安门，天安门上太阳升。"),
    ("zh2", "上海是中国最大的城市。"),
    ("ko1", "서울특별시에 갑니다."),
    ("ko2", "부산에서 왔습니다."),
    ("th1", "ภาษาไทยง่ายมาก ผมรักเมืองไทย"),
    ("th2", "ฉันอยากไปตลาดเพื่อซื้อผัก"),
    ("hi1", "हिन्दी भारत की भाषा है।"),
    ("hi2", "लड़के स्कूल जाते हैं।"),
    ("mix", "ＣｈｌｏｅＤＢ ３.１４ ﬁle café I’ll"),
    ("fr1", "Les élèves mangeaient des pommes"),
];

fn add(db: &Db, rows: &[(&str, &str)]) {
    for (id, text) in rows {
        let p = Params::new().set("id", Value::Str((*id).into())).set("t", Value::Str((*text).into()));
        run(db, "INSERT INTO docs (id, body, simple, fr, hi, cjk) VALUES (:id, :t, :t, :t, :t, :t)", &p);
    }
}

fn searches(db: &Db) {
    let m = |q: &str| matches(db, "body", q);
    // Japanese: the words of a dictionary, and a longer run as a phrase of
    // its words.
    assert_eq!(m("東京"), ["ja1"]);
    assert_eq!(m("京都"), ["ja2"]);
    assert_eq!(m("東京都庁"), ["ja1"]);
    assert_eq!(m("\"東京都庁に行きました\""), ["ja1"]);
    assert_eq!(m("お寺"), ["ja2"]);
    assert_eq!(m("都京"), Vec::<String>::new());
    // Words are what the dictionary cuts: 京都 is not a word of 東京都庁,
    // and a run it cuts another way than the document's is not found.
    assert_eq!(m("京都庁"), Vec::<String>::new());
    // `cjk` indexes every pair of characters: any run of two characters or
    // more is found, as a phrase of its pairs, whatever the words are.
    let c = |q: &str| matches(db, "cjk", q);
    assert_eq!(c("東京"), ["ja1"]);
    assert_eq!(c("京都"), ["ja1", "ja2"]);
    assert_eq!(c("京都庁"), ["ja1"]);
    assert_eq!(c("都京"), Vec::<String>::new());
    assert_eq!(c("天安门"), ["zh1"]);
    assert_eq!(c("安门上"), ["zh1"]);
    assert_eq!(c("서울특별시"), ["ko1"]);
    assert_eq!(c("ไทย"), ["th1"]);
    assert_eq!(c("東*"), ["ja1"]);
    assert_eq!(ranked(db, "cjk", "京都のお寺")[0], "ja2");
    // Chinese.
    assert_eq!(m("天安门"), ["zh1"]);
    assert_eq!(m("北京 OR 上海"), ["zh1", "zh2"]);
    assert_eq!(m("城市 NOT 上海"), Vec::<String>::new());
    // Korean: a word without the particle its sentence gives it.
    assert_eq!(m("서울특별시"), ["ko1"]);
    assert_eq!(m("부산"), ["ko2"]);
    // Thai: dictionary words.
    assert_eq!(m("ไทย"), ["th1"]);
    assert_eq!(m("ตลาด"), ["th2"]);
    assert_eq!(m("\"ภาษาไทย\""), ["th1"]);
    // Hindi: whole words, however the query spells the conjunct.
    assert_eq!(m("हिन्दी"), ["hi1"]);
    assert_eq!(m("भाषा"), ["hi1"]);
    assert_eq!(matches(db, "hi", "हिंदी"), ["hi1"]);
    assert_eq!(matches(db, "hi", "लड़का"), ["hi2"]);
    assert_eq!(m("लड़का"), Vec::<String>::new());
    // Compatibility forms are the plain ones, on both sides.
    assert_eq!(m("chloedb"), ["mix"]);
    assert_eq!(m("ＣＨＬＯＥＤＢ"), ["mix"]);
    assert_eq!(m("3.14"), ["mix"]);
    assert_eq!(m("file"), ["mix"]);
    // Accents fold, however the text or the query writes them.
    assert_eq!(m("CAFE\u{301}"), ["mix"]);
    assert_eq!(m("cafe"), ["mix"]);
    assert_eq!(m("caf\u{e9}*"), ["mix"]);
    assert_eq!(m("\"i'll\""), ["mix"]);
    // Prefixes and typos.
    assert_eq!(m("chloe*"), ["mix"]);
    assert_eq!(m("東*"), ["ja1"]);
    assert_eq!(m("chloedc~1"), ["mix"]);
    // `simple` cannot find a word inside these sentences.
    assert_eq!(matches(db, "simple", "東京"), Vec::<String>::new());
    assert_eq!(matches(db, "simple", "ไทย"), Vec::<String>::new());
    // A language stems; `standard` does not.
    assert_eq!(matches(db, "fr", "élève"), ["fr1"]);
    assert_eq!(matches(db, "fr", "eleve"), ["fr1"]);
    assert_eq!(matches(db, "fr", "manger pomme"), ["fr1"]);
    assert_eq!(m("élève"), Vec::<String>::new());
    assert_eq!(m("élèves"), ["fr1"]);
    // A stopword is a word of a phrase.
    assert_eq!(matches(db, "fr", "\"les élèves\""), ["fr1"]);
    assert_eq!(matches(db, "fr", "\"des élèves\""), Vec::<String>::new());

    // bm25 ranks by the same terms.
    assert_eq!(ranked(db, "body", "東京")[0], "ja1");
    assert_eq!(ranked(db, "body", "京都のお寺")[0], "ja2");
    assert_eq!(ranked(db, "body", "天安门在哪里"), ["zh1"]);
    assert_eq!(ranked(db, "body", "เมืองไทย"), ["th1"]);
    assert_eq!(ranked(db, "body", "भारत की भाषा")[0], "hi1");
    assert_eq!(ranked(db, "fr", "la pomme des élèves")[0], "fr1");
    assert_eq!(ranked(db, "simple", "東京"), Vec::<String>::new());
}

fn through(path: &Path) {
    let opts = || DbOptions { sync: false, ..DbOptions::default() };
    let plain = Schema::new("plain", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(path, plain, opts()).unwrap();
    run(
        &db,
        "CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT OPTIONS(analyzer = 'standard'), simple TEXT OPTIONS(analyzer = 'simple'),
           fr TEXT OPTIONS(analyzer = 'French'), hi TEXT OPTIONS(analyzer = 'hindi'), cjk TEXT OPTIONS(analyzer = 'cjk'))",
        &Params::new(),
    );
    let docs = db.storage().table("docs").unwrap();
    assert_eq!(docs.fields[1].analyzer, Some(Analyzer::Standard));
    assert_eq!(docs.fields[3].analyzer, Some(Analyzer::Language(Language::French)));
    assert_eq!(docs.fields[5].analyzer, Some(Analyzer::Cjk));
    assert_eq!(docs.to_short_json()["fields"][3], serde_json::json!({"name": "fr", "type": "text", "analyzer": "french"}));
    let features = chloedb::storage::manifest::read(path).unwrap().features();
    for f in ["analyzer:standard:1", "analyzer:simple:1", "analyzer:french:1", "analyzer:hindi:1", "analyzer:cjk:1"] {
        assert!(features.iter().any(|x| x == f), "{f}: {features:?}");
    }

    // In the memtable, then in segments, then merged into one, then reopened.
    add(&db, &ROWS[..7]);
    add(&db, &ROWS[7..]);
    searches(&db);
    db.storage().flush().unwrap();
    searches(&db);
    add(&db, &[("zz", "nothing here")]);
    db.storage().flush().unwrap();
    let stats = db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 1_000, ..MergePolicy::default() }).unwrap();
    assert!(stats.docs_merged > 0, "{stats:?}");
    searches(&db);
    drop(db);
    let db = Db::open(path, opts()).unwrap();
    searches(&db);
}

#[test]
fn any_script_through_sql() {
    let dir = tempfile::tempdir().unwrap();
    through(&dir.path().join("l.strata"));
}

// Every analyzer has a name that finds it again, a feature of its own
// (but `english`), and analyses a text the same way twice.
#[test]
fn every_analyzer_by_name() {
    let text = "The Quick brown foxes — 東京都庁 ภาษาไทย हिन्दी مَكْتَبَة élèves 3.14";
    let mut names = std::collections::HashSet::new();
    for a in Analyzer::ALL {
        assert_eq!(Analyzer::of(a.name(), a.version()).unwrap(), a);
        assert!(names.insert(a.name()), "{}", a.name());
        assert_eq!(a.feature().is_none(), a == Analyzer::English);
        let terms = a.analyze(text);
        assert!(!terms.is_empty() && terms == a.analyze(text), "{}", a.name());
        let j = serde_json::to_value(a).unwrap();
        assert_eq!(serde_json::from_value::<Analyzer>(j).unwrap(), a);
        // A query's terms are its documents' terms.
        assert_eq!(a.query_term_seq("東京都庁 ภาษาไทย", false), a.analyze("東京都庁 ภาษาไทย"), "{}", a.name());
    }
    assert_eq!(Analyzer::ALL.len(), 26);
    assert!(Analyzer::of("klingon", 1).is_err());
    assert!(Analyzer::of("standard", 2).is_err());
}
