//! Scrolls through `POST /v1/query`: `page_size` answers the first page and
//! a `cursor`, which reads the next pages past `--max-result-window`, from
//! the version the scroll started on, as JSON or Arrow, until the last
//! page answers no cursor. A cursor is its owner's, can be closed, ends
//! when not read over its keep-alive, and the rows scrolls hold between
//! them are capped (`--max-scroll-rows`). A coordinator scrolls its
//! shards' scrolls, merging their pages, and ends them with its own.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{json as js, server};
use tokio::net::TcpListener;

/// The window: the most rows a query (or a page) answers.
const WINDOW: usize = 100;
const ROWS: usize = 1000;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

async fn call(url: &str, method: &str, path: &str, body: Json) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let r = match method {
        "DELETE" => c.delete(format!("{url}{path}")),
        _ => c.post(format!("{url}{path}")).json(&body),
    };
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Json::Null))
}

async fn query(url: &str, body: Json) -> (u16, Json) {
    call(url, "POST", "/v1/query", body).await
}

/// A server of `docs` (ROWS rows over several segments) and `tags` (two a
/// doc), its window WINDOW rows, and its database.
async fn served(dir: &std::path::Path, scroll_rows: usize) -> (String, Arc<Db>, Arc<Service>) {
    let db = Arc::new(Db::create_empty(&dir.join("one.strata"), DbOptions { segment_max_docs: 150, ..DbOptions::default() }).unwrap());
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db.clone()), schema, agent(), 16, Duration::from_secs(20));
    svc.max_rows = Some(WINDOW);
    svc.max_scroll_rows = scroll_rows;
    let svc = Arc::new(svc);
    let url = http(svc.clone()).await;
    load(&url).await;
    (url, db, svc)
}

/// Creates `docs` and `tags` through the server at `url`, and writes their
/// rows.
async fn load(url: &str) {
    for t in ["CREATE TABLE docs (id KEYWORD PRIMARY KEY, g KEYWORD, n INT, x FLOAT)", "CREATE TABLE tags (id KEYWORD PRIMARY KEY, doc KEYWORD, t KEYWORD)"] {
        let (s, j) = call(url, "POST", "/v1/sql", json!({"sql": t})).await;
        assert_eq!(s, 200, "{j}");
    }
    for _ in 0..50 {
        if query(url, json!({"sql": "SELECT COUNT(*) FROM tags"})).await.0 == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for chunk in (0..ROWS).collect::<Vec<_>>().chunks(200) {
        let mut ops: Vec<Json> = chunk.iter().map(|i| json!({"op": "upsert", "doc": {"id": format!("d{i:04}"), "g": format!("g{}", i % 7), "n": (i * 37) % 1000, "x": *i as f64 / 8.0}})).collect();
        ops.extend(chunk.iter().flat_map(|i| (0..2).map(move |k| json!({"op": "upsert", "table": "tags", "doc": {"id": format!("t{i:04}{k}"), "doc": format!("d{i:04}"), "t": format!("k{}", (i + k) % 5)}}))));
        let (s, j) = call(url, "POST", "/v1/write", json!({"ops": ops})).await;
        assert_eq!(s, 200, "{j}");
    }
}

/// The rows of `sql` on the database, at once and without a window.
fn whole(db: &Db, sql: &str) -> Vec<Json> {
    match db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")) {
        SqlResult::Rows(rs) => js::result(&rs, 0.0)["rows"].as_array().unwrap().clone(),
        SqlResult::Written(_) => panic!("{sql}"),
    }
}

/// Every page of a scroll of `sql`, `page_size` asked, as JSON: the rows
/// and each page's count. `between` runs after the first page.
async fn scroll(url: &str, sql: &str, page_size: usize, between: impl AsyncFnOnce()) -> (Vec<Json>, Vec<usize>) {
    let (s, mut j) = query(url, json!({"sql": sql, "page_size": page_size})).await;
    assert_eq!(s, 200, "{sql}: {j}");
    between().await;
    let mut rows = Vec::new();
    let mut counts = Vec::new();
    loop {
        let page = j["rows"].as_array().unwrap();
        counts.push(page.len());
        rows.extend(page.iter().cloned());
        let Some(c) = j["cursor"].as_str().map(String::from) else { break };
        let (s, next) = query(url, json!({"cursor": c})).await;
        assert_eq!(s, 200, "{sql}: {next}");
        j = next;
        assert!(counts.len() <= ROWS * 2, "{sql}: the scroll does not end");
    }
    (rows, counts)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scroll_reads_every_row_past_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let (url, db, _) = served(dir.path(), 5000).await;
    let all = "SELECT id, g, n, x FROM docs";
    let (s, j) = query(&url, json!({"sql": all})).await;
    assert_eq!(s, 400, "the window holds: {j}");
    let want = whole(&db, all);
    assert_eq!(want.len(), ROWS);
    // Pages hold at most the window, whatever is asked; rows written and
    // deleted after the first page are not in the scroll.
    let (rows, counts) = scroll(&url, all, 5000, async || {
        for sql in ["DELETE FROM docs WHERE n < 500", "INSERT INTO docs (id, g, n, x) VALUES ('new1', 'g1', 1, 1.0), ('new2', 'g2', 2, 2.0)"] {
            let (s, j) = call(&url, "POST", "/v1/sql", json!({"sql": sql})).await;
            assert_eq!(s, 200, "{j}");
        }
    })
    .await;
    assert_eq!(counts, vec![WINDOW; ROWS / WINDOW], "{counts:?}");
    assert_eq!(rows, want);
    assert!(whole(&db, all).len() < ROWS, "the deletes landed");
    let want = whole(&db, all);
    for (sql, page) in [
        (all.to_string(), 30),
        ("SELECT id FROM docs WHERE g = 'g3' LIMIT 50 OFFSET 5".into(), 7),
        ("SELECT id, n FROM docs ORDER BY n DESC, id".into(), 64),
        ("SELECT g, COUNT(*) AS c FROM docs GROUP BY g ORDER BY g".into(), 2),
        ("SELECT d.id, t.t FROM docs d JOIN tags t ON t.doc = d.id ORDER BY t.id".into(), 99),
        ("SELECT id FROM docs WHERE n > 100000".into(), 10),
    ] {
        let want = if sql == all { want.clone() } else { whole(&db, &sql) };
        let (rows, counts) = scroll(&url, &sql, page, async || {}).await;
        assert_eq!(rows, want, "{sql}");
        assert!(counts.iter().all(|&c| c <= page) && counts.iter().rev().skip(1).all(|&c| c == page), "{sql}: {counts:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arrow_pages_carry_the_cursor_in_a_header() {
    let dir = tempfile::tempdir().unwrap();
    let (url, db, _) = served(dir.path(), 5000).await;
    let sql = "SELECT id, n, x FROM docs WHERE n >= 200";
    let want = whole(&db, sql);
    let client = chloedb_server::http_client().build().unwrap();
    let mut body = json!({"sql": sql, "page_size": 64, "format": "arrow"});
    let mut rows = Vec::new();
    let mut pages = 0;
    loop {
        let r = client.post(format!("{url}/v1/query")).json(&body).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let cursor = r.headers().get("x-strata-cursor").map(|v| v.to_str().unwrap().to_string());
        let batch = chloedb::query::arrow::batch_of_ipc(&r.bytes().await.unwrap()).unwrap();
        assert!(batch.num_rows() <= 64);
        let cols: Vec<Vec<chloedb::Value>> = batch.columns().iter().map(|a| chloedb::query::arrow::values_of(a).unwrap()).collect();
        rows.extend((0..batch.num_rows()).map(|r| json!(cols.iter().map(|c| js::to_json(&c[r])).collect::<Vec<_>>())));
        pages += 1;
        match cursor {
            Some(c) => body = json!({"cursor": c, "format": "arrow"}),
            None => break,
        }
    }
    assert_eq!(rows, want);
    assert_eq!(pages, want.len().div_ceil(64));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cursor_is_its_owners_and_ends() {
    let dir = tempfile::tempdir().unwrap();
    let (url, _, svc) = served(dir.path(), 5000).await;
    let start = async |body: Json| {
        let (s, j) = query(&url, body).await;
        assert_eq!(s, 200, "{j}");
        j["cursor"].as_str().unwrap().to_string()
    };
    // Another key's cursor is none of its own.
    let c = start(json!({"sql": "SELECT id FROM docs", "page_size": 10})).await;
    let other = svc.scroll_next(&c, "reader/k2", None, 0, None).await.err().unwrap();
    assert_eq!(other.status, 404, "{}", other.message);
    assert_eq!(query(&url, json!({"cursor": c})).await.0, 200);
    // Closed: no more pages, and closing again closes nothing.
    let (s, j) = call(&url, "DELETE", &format!("/v1/query/cursors/{c}"), Json::Null).await;
    assert_eq!((s, &j["closed"]), (200, &json!(true)), "{j}");
    let (s, j) = query(&url, json!({"cursor": c})).await;
    assert_eq!(s, 404, "{j}");
    assert!(j["error"].as_str().unwrap().contains("no such cursor"), "{j}");
    assert_eq!(call(&url, "DELETE", &format!("/v1/query/cursors/{c}"), Json::Null).await.1["closed"], json!(false));
    // Not read over its keep-alive: gone.
    let c = start(json!({"sql": "SELECT id FROM docs ORDER BY n", "page_size": 10, "keep_alive_ms": 200})).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(query(&url, json!({"cursor": c})).await.0, 404);
    // Read to its end: gone too.
    let c = start(json!({"sql": "SELECT id FROM docs LIMIT 15", "page_size": 10})).await;
    let (s, j) = query(&url, json!({"cursor": c})).await;
    assert_eq!((s, j["rows"].as_array().unwrap().len(), j.get("cursor")), (200, 5, None), "{j}");
    assert_eq!(query(&url, json!({"cursor": c})).await.0, 404);
    // A page that fits answers no cursor.
    let (s, j) = query(&url, json!({"sql": "SELECT id FROM docs LIMIT 10", "page_size": 10})).await;
    assert_eq!((s, j.get("cursor")), (200, None), "{j}");
    // What a request may not ask.
    for (body, says) in [
        (json!({"cursor": "x", "sql": "SELECT id FROM docs"}), "cursor alone"),
        (json!({"sql": "SELECT id FROM docs", "page_size": 0}), "page_size is"),
        (json!({"sql": "SELECT id FROM docs", "keep_alive_ms": 1000}), "start one with page_size"),
        (json!({"sql": "SELECT id FROM docs", "page_size": 5, "keep_alive_ms": 7_200_000}), "keep_alive_ms is"),
        (json!({"sql": "SELECT id FROM docs", "page_size": 5, "log": true}), "leave out page_size"),
        (json!({"cursor": 7}), "cursor is the string"),
    ] {
        let (s, j) = query(&url, body.clone()).await;
        assert_eq!(s, 400, "{body}: {j}");
        assert!(j["error"].as_str().unwrap().contains(says), "{body}: {j}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scrolls_hold_at_most_max_scroll_rows_between_them() {
    let dir = tempfile::tempdir().unwrap();
    let (url, _, _) = served(dir.path(), 1500).await;
    // One that would hold more than all of it.
    let (s, j) = query(&url, json!({"sql": "SELECT d.id, t.t FROM docs d JOIN tags t ON t.doc = d.id", "page_size": 100})).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("--max-scroll-rows"), "{j}");
    let (s, j) = query(&url, json!({"sql": "SELECT id FROM tags ORDER BY t, id", "page_size": 100})).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("1500"), "{j}");
    // Two that fit apart but not together; a scan holds nothing.
    let (s, a) = query(&url, json!({"sql": "SELECT id FROM docs ORDER BY n", "page_size": 100})).await;
    assert_eq!(s, 200, "{a}");
    let (s, j) = query(&url, json!({"sql": "SELECT id FROM docs ORDER BY x DESC", "page_size": 100})).await;
    assert_eq!(s, 429, "{j}");
    assert!(j["error"].as_str().unwrap().contains("hold 1000 rows of the 1500"), "{j}");
    let (s, j) = query(&url, json!({"sql": "SELECT id FROM tags", "page_size": 100})).await;
    assert_eq!((s, j["cursor"].is_string()), (200, true), "{j}");
    // Closing the first makes room.
    call(&url, "DELETE", &format!("/v1/query/cursors/{}", a["cursor"].as_str().unwrap()), Json::Null).await;
    let (s, j) = query(&url, json!({"sql": "SELECT id FROM docs ORDER BY x DESC", "page_size": 100})).await;
    assert_eq!(s, 200, "{j}");
    // Two open: the scan (none held) and the last (a place a row).
    let text = chloedb_server::http_client().build().unwrap().get(format!("{url}/metrics")).send().await.unwrap().text().await.unwrap();
    let gauge = |name: &str| text.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix(' ')?.parse::<f64>().ok());
    assert_eq!((gauge("chloedb_scrolls_open"), gauge("chloedb_scroll_rows_held")), (Some(2.0), Some(1000.0)), "{}", text.lines().filter(|l| l.contains("scroll")).collect::<Vec<_>>().join("\n"));
}

/// Three shards and a coordinator over them, each with a window of
/// WINDOW rows, `docs` and `tags` loaded through the coordinator: its URL,
/// its service and the shards'.
async fn cluster(dir: &std::path::Path) -> (String, Arc<Service>, Vec<Arc<Service>>) {
    let mut urls = Vec::new();
    let mut shards = Vec::new();
    for i in 0..3 {
        let db = Db::create_empty(&dir.join(format!("s{i}.strata")), DbOptions { segment_max_docs: 60, ..DbOptions::default() }).unwrap();
        let schema = (*db.schema()).clone();
        let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(20));
        svc.max_rows = Some(WINDOW);
        let svc = Arc::new(svc);
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!("http://{}", l.local_addr().unwrap()));
        tokio::spawn(server::serve_grpc(svc.clone(), l, Access::default()));
        shards.push(svc);
    }
    let c = Coordinator::connect(urls).await.unwrap();
    let schema = c.schema.clone();
    let mut svc = Service::new(Backend::Cluster(Arc::new(c)), schema, agent(), 16, Duration::from_secs(20));
    svc.max_rows = Some(WINDOW);
    let svc = Arc::new(svc);
    let url = http(svc.clone()).await;
    load(&url).await;
    (url, svc, shards)
}

/// The scrolls the shards keep open, after the coordinator's ends were
/// sent (in the background).
async fn shard_scrolls(shards: &[Arc<Service>]) -> usize {
    let mut open = 0;
    for _ in 0..50 {
        open = shards.iter().map(|s| s.sweep_scrolls().0).sum();
        if open == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    open
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_scrolls_its_shards() {
    let dir = tempfile::tempdir().unwrap();
    let (url, coord, shards) = cluster(dir.path()).await;
    // The same rows on one server, for the answers' order.
    let one = dir.path().join("one");
    std::fs::create_dir(&one).unwrap();
    let (_, db, _) = served(&one, 5000).await;
    let all = "SELECT id, g, n, x FROM docs";
    let (s, j) = query(&url, json!({"sql": all})).await;
    assert_eq!(s, 400, "the window holds: {j}");
    assert!(j["error"].as_str().unwrap().contains("a scroll, page_size"), "{j}");
    let sorted = |mut rows: Vec<Json>| {
        rows.sort_by_key(|r| r.to_string());
        rows
    };
    // In no order, one shard's rows after another's; writes after the
    // first page are not in the scroll.
    let want = whole(&db, all);
    let (rows, counts) = scroll(&url, all, 64, async || {
        let (s, j) = call(&url, "POST", "/v1/sql", json!({"sql": "DELETE FROM docs WHERE n < 500"})).await;
        assert_eq!(s, 200, "{j}");
    })
    .await;
    assert_eq!(sorted(rows), sorted(want), "{counts:?}");
    assert!(counts.iter().rev().skip(1).all(|&c| c == 64), "{counts:?}");
    assert_eq!(shard_scrolls(&shards).await, 0, "the shards' scrolls end with the last page");
    db.sql("DELETE FROM docs WHERE n < 500", &Params::new()).unwrap();
    for (sql, page) in [
        ("SELECT id, n FROM docs ORDER BY n DESC, id".to_string(), 30),
        ("SELECT id, x FROM docs WHERE g <> 'g2' ORDER BY x, id LIMIT 200 OFFSET 45".into(), 64),
        ("SELECT g, COUNT(*) AS c, MAX(n) AS m FROM docs GROUP BY g ORDER BY g".into(), 2),
        ("SELECT d.id, t.t FROM docs d JOIN tags t ON t.doc = d.id ORDER BY t.id".into(), 99),
        ("SELECT id FROM docs WHERE n > 100000".into(), 10),
    ] {
        let want = whole(&db, &sql);
        let (rows, counts) = scroll(&url, &sql, page, async || {}).await;
        assert_eq!(rows, want, "{sql}");
        assert!(counts.iter().all(|&c| c <= page) && counts.iter().rev().skip(1).all(|&c| c == page), "{sql}: {counts:?}");
    }
    // LIMIT and OFFSET in no order: as many rows, each a row of the table.
    let (rows, _) = scroll(&url, "SELECT id, g, n, x FROM docs LIMIT 150 OFFSET 230", 40, async || {}).await;
    let every = whole(&db, all);
    assert_eq!(rows.len(), 150);
    assert!(rows.iter().all(|r| every.contains(r)));
    assert_eq!(shard_scrolls(&shards).await, 0, "the shards' scrolls end at the LIMIT");
    // Closed early, or by id read to a LIMIT before the shards' rows end:
    // the shards' scrolls end with it.
    let (s, j) = query(&url, json!({"sql": "SELECT id FROM docs ORDER BY n", "page_size": 10})).await;
    assert_eq!((s, j["rows"].as_array().unwrap().len()), (200, 10), "{j}");
    assert!(shards.iter().all(|s| s.sweep_scrolls().0 == 1), "each shard keeps a scroll");
    let c = j["cursor"].as_str().unwrap().to_string();
    // A keep-alive asked with a page reaches the shards' scrolls.
    let (s, j) = query(&url, json!({"cursor": c, "keep_alive_ms": 600_000})).await;
    assert_eq!((s, j["rows"].as_array().unwrap().len()), (200, 10), "{j}");
    let (s, j) = call(&url, "DELETE", &format!("/v1/query/cursors/{c}"), Json::Null).await;
    assert_eq!((s, &j["closed"]), (200, &json!(true)), "{j}");
    assert_eq!(shard_scrolls(&shards).await, 0, "closed with the coordinator's");
    // Not read over its keep-alive: the coordinator's sweep ends them.
    let (s, j) = query(&url, json!({"sql": "SELECT id, x FROM docs ORDER BY x DESC", "page_size": 10, "keep_alive_ms": 200})).await;
    assert_eq!((s, j["cursor"].is_string()), (200, true), "{j}");
    assert!(shards.iter().all(|s| s.sweep_scrolls().0 == 1));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(coord.sweep_scrolls(), (0, 0));
    assert_eq!(shard_scrolls(&shards).await, 0, "ended with the coordinator's");
    assert_eq!(query(&url, json!({"cursor": j["cursor"]})).await.0, 404);
}
