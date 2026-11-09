//! chloedb-sheets against a real chloedb-server: `serve --local` starts one,
//! and the workbooks it keeps there are made, changed, imported, filled from
//! queries, saved as tables, searched, and read back after a restart.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release}/chloedb-server`), and is skipped
//! without it unless `CHLOEDB_SHEETS_NEED_SERVER` is set (as CI does).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TOKEN: &str = "sheets-test-token";

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN").or_else(|| std::env::var_os("STRATA_SERVER_BIN")) {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Sheets {
    child: Child,
    url: String,
}

impl Sheets {
    fn start(dir: &std::path::Path, bin: &std::path::Path) -> Sheets {
        let (port, strata) = (free_port(), free_port());
        let child = Command::new(env!("CARGO_BIN_EXE_chloedb-sheets"))
            .args(["serve", "--local"])
            .arg(dir)
            .arg("--server-bin")
            .arg(bin)
            .args(["--listen", &format!("127.0.0.1:{port}"), "--local-listen", &format!("127.0.0.1:{strata}"), "--token", TOKEN])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-sheets");
        Sheets { child, url: format!("http://127.0.0.1:{port}") }
    }

    /// SIGTERM: it saves what it holds and stops its chloedb-server.
    fn stop(mut self) {
        // SAFETY: a signal to a child this test started.
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(30) {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        panic!("chloedb-sheets did not stop on SIGTERM");
    }
}

impl Drop for Sheets {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct Api {
    http: reqwest::Client,
    url: String,
}

impl Api {
    async fn ready(url: &str) -> Api {
        let http = reqwest::Client::new();
        let t0 = Instant::now();
        loop {
            if let Ok(r) = http.get(format!("{url}/v1/books")).bearer_auth(TOKEN).send().await {
                if r.status().is_success() {
                    break;
                }
            }
            assert!(t0.elapsed() < Duration::from_secs(90), "chloedb-sheets did not answer");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Api { http, url: url.to_string() }
    }
    async fn get(&self, path: &str) -> Value {
        let r = self.http.get(format!("{}{path}", self.url)).bearer_auth(TOKEN).send().await.unwrap();
        let status = r.status();
        let body = r.text().await.unwrap();
        assert!(status.is_success(), "GET {path}: {status} {body}");
        serde_json::from_str(&body).unwrap()
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        let r = self.http.post(format!("{}{path}", self.url)).bearer_auth(TOKEN).json(&body).send().await.unwrap();
        let status = r.status();
        let text = r.text().await.unwrap();
        assert!(status.is_success(), "POST {path}: {status} {text}");
        serde_json::from_str(&text).unwrap()
    }
    async fn upload(&self, path: &str, bytes: Vec<u8>) -> Value {
        let r = self.http.post(format!("{}{path}", self.url)).bearer_auth(TOKEN).header("content-type", "application/octet-stream").body(bytes).send().await.unwrap();
        let status = r.status();
        let text = r.text().await.unwrap();
        assert!(status.is_success(), "POST {path}: {status} {text}");
        serde_json::from_str(&text).unwrap()
    }
    async fn ops(&self, id: &str, ops: Value) -> Value {
        let j = self.post(&format!("/v1/books/{id}/ops"), json!({ "ops": ops })).await;
        assert!(j["error"].is_null(), "ops: {}", j["error"]);
        j
    }
    /// The cells of an area, as `(row, col) -> shown`.
    async fn shown(&self, id: &str, sheet: u32, r1: i32, c1: i32) -> std::collections::HashMap<(i64, i64), String> {
        let j = self.get(&format!("/v1/books/{id}/cells?sheet={sheet}&r0=1&r1={r1}&c0=1&c1={c1}")).await;
        j["cells"].as_array().unwrap().iter().map(|c| ((c[0].as_i64().unwrap(), c[1].as_i64().unwrap()), c[2].as_str().unwrap_or("").to_string())).collect()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbooks_live_in_a_cluster() {
    let Some(bin) = server_bin().filter(|b| b.exists()) else {
        assert!(std::env::var_os("CHLOEDB_SHEETS_NEED_SERVER").is_none(), "no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        eprintln!("skipped: no chloedb-server binary (build chloedb-server, or set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sheets = Sheets::start(dir.path(), &bin);
    let api = Api::ready(&sheets.url).await;

    // A workbook: values, a formula, a style, a chart; the formula computes.
    let b = api.post("/v1/books", json!({"name": "Budget"})).await;
    let id = b["id"].as_str().unwrap().to_string();
    api.ops(
        &id,
        json!([
            {"op": "set_many", "sheet": 0, "row": 1, "col": 1, "values": [["Region", "Sales"], ["North", "10"], ["South", "32.5"], ["East", "7"], ["Total", "=SUM(B2:B4)"]]},
            {"op": "style", "sheet": 0, "area": {"row": 1, "col": 1, "width": 2}, "path": "font.b", "value": "true"},
            {"op": "chart", "sheet": 0, "range": "A1:B4", "kind": "column", "row": 1, "col": 4},
            {"op": "filter", "sheet": 0, "area": {"row": 1, "col": 1, "width": 2, "height": 4}, "header": true, "criteria": [{"col": 1, "values": ["North", "South"]}]},
        ]),
    )
    .await;
    let cells = api.shown(&id, 0, 6, 3).await;
    assert_eq!(cells[&(5, 2)], "49.5");
    let summary = api.get(&format!("/v1/books/{id}")).await;
    assert_eq!(summary["charts"].as_array().unwrap().len(), 1);
    assert_eq!(summary["filters"][0]["criteria"][0]["values"], json!(["North", "South"]));

    // A sheet saved as a table of the cluster, then a query of it filling cells.
    let t = api.post(&format!("/v1/books/{id}/to-table"), json!({"sheet": 0, "table": "regions", "key": "region"})).await;
    assert_eq!(t["rows"], 4, "{t}");
    let q = api.post(&format!("/v1/books/{id}/query"), json!({"sheet": 0, "row": 8, "col": 1, "sql": "SELECT region, sales FROM regions WHERE sales > 8 AND sales < 40 ORDER BY sales DESC"})).await;
    assert!(q["error"].is_null(), "{}", q["error"]);
    let cells = api.shown(&id, 0, 11, 2).await;
    assert_eq!((cells[&(8, 1)].as_str(), cells[&(9, 1)].as_str(), cells[&(10, 1)].as_str()), ("region", "South", "North"));
    assert!(!cells.contains_key(&(11, 1)), "only the rows the query gave");

    // A table of the cluster as a new workbook.
    let w = api.post("/v1/from-table", json!({"table": "regions"})).await;
    assert_eq!(w["copied"]["rows"], 4, "{w}");

    // Imports: CSV, and a SQLite file's tables as sheets.
    let csv = api.upload("/v1/import?name=cities.csv", b"city,people\nLima,10000000\nOslo,700000\n".to_vec()).await;
    let csv_id = csv["id"].as_str().unwrap().to_string();
    assert_eq!(api.shown(&csv_id, 0, 3, 2).await[&(3, 1)], "Oslo");
    let db = dir.path().join("shop.sqlite");
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE orders (id INTEGER PRIMARY KEY, item TEXT, total REAL); INSERT INTO orders VALUES (1, 'tea', 4.5), (2, 'cake', 12.25); CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('fresh mint');").unwrap();
    }
    let sq = api.upload("/v1/import?name=shop.sqlite", std::fs::read(&db).unwrap()).await;
    let sq_id = sq["id"].as_str().unwrap().to_string();
    let names: Vec<&str> = sq["sheets"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["notes", "orders"], "a SQLite file's tables, by name");
    assert_eq!(api.shown(&sq_id, 1, 3, 3).await[&(3, 3)], "12.25");

    // Every workbook can be searched, by its cells.
    let t0 = Instant::now();
    loop {
        let s = api.get("/v1/search?q=mint").await;
        if s["cells"]["rows"].as_array().is_some_and(|r| !r.is_empty()) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "search found nothing: {s}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // An xlsx out.
    let r = api.http.get(format!("{}/v1/books/{id}/export?format=xlsx", api.url)).bearer_auth(TOKEN).send().await.unwrap();
    assert!(r.status().is_success());
    assert!(r.bytes().await.unwrap().starts_with(b"PK"), "an xlsx is a zip");

    // A restart: the cells, the formula, the chart and the query come back from Strata.
    sheets.stop();
    let sheets = Sheets::start(dir.path(), &bin);
    let api = Api::ready(&sheets.url).await;
    let cells = api.shown(&id, 0, 10, 2).await;
    assert_eq!(cells[&(5, 2)], "49.5");
    assert_eq!(cells[&(9, 1)], "South");
    let summary = api.get(&format!("/v1/books/{id}")).await;
    assert_eq!(summary["queries"].as_array().unwrap().len(), 1);
    assert_eq!(summary["charts"].as_array().unwrap().len(), 1);
    let list = api.get("/v1/books").await;
    assert_eq!(list["books"].as_array().unwrap().len(), 4);
    sheets.stop();
}
