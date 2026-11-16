//! The listeners' safety: what a request may send (a gzip body's size,
//! headers that never end, connections past the cap), what each answer
//! says to a browser (headers against framing and sniffing), who may
//! call what on the OpenSearch listener and what its audit log keeps,
//! which key may post a coordinator's alerts, and how a server stops with
//! requests under way (over TLS too) and a follower's feed open (over HTTP
//! and gRPC). And what a connection may hold (`chloedb_server::listener`):
//! a share of the cap per client, room past it for the probes, HTTP/2
//! connections closed when idle, slow bodies and slow readers cut off.

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Limits, Secret, Tls};
use chloedb_server::keys::{Keys, Scope};
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A server's service on one database, its writes numbered (so that it
/// has a feed), with a keys file.
fn service(dir: &Path) -> Service {
    std::fs::create_dir_all(dir).unwrap();
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join("one.strata"), demo::schema(10), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.storage().start_seq_no().unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.join("keys.json")).unwrap());
    svc
}

fn tls() -> Tls {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tls");
    let read = |f: &str| std::fs::read(dir.join(f)).unwrap();
    Tls { cert: read("cert.pem"), key: read("key.pem") }
}

/// A stop button for a listener: `stop()` ends its `stop` future.
fn stopper() -> (impl FnOnce(), impl std::future::Future<Output = ()> + Send) {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    (
        move || {
            let _ = tx.send(());
        },
        async move {
            let _ = rx.await;
        },
    )
}

async fn listen() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

/// Serves `svc`'s HTTP API with `access`: its URL.
async fn http(svc: Arc<Service>, access: Access) -> String {
    let (l, port) = listen().await;
    tokio::spawn(server::serve_http(svc, l, access));
    format!("http://127.0.0.1:{port}")
}

/// Serves `svc`'s OpenSearch API with `access`: its URL.
async fn opensearch(svc: Arc<Service>, access: Access) -> String {
    let (l, port) = listen().await;
    tokio::spawn(server::serve_opensearch(svc, l, access, chloedb_server::opensearch::VERSION_NUMBER));
    format!("http://127.0.0.1:{port}")
}

fn client() -> reqwest::Client {
    chloedb_server::http_client().build().unwrap()
}

fn basic(pass: &str) -> String {
    use base64::Engine;
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("admin:{pass}")))
}

fn audit(svc: &Service) -> Vec<Json> {
    svc.audit.latest(100).iter().map(|e| serde_json::to_value(e).unwrap()).collect()
}

/// D-B2: a gzip body inflates to no more than the listener takes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_gzip_body_inflates_no_further_than_the_body_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut svc = service(dir.path());
    svc.max_body = 1 << 20;
    let base = opensearch(Arc::new(svc), Access::default()).await;
    let gz = |text: &[u8]| {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(text).unwrap();
        e.finish().unwrap()
    };
    // 16 MiB of spaces, some 16 KiB sent: past the 1 MiB limit once inflated.
    let bomb = gz(&vec![b' '; 16 << 20]);
    assert!(bomb.len() < 64 << 10, "{}", bomb.len());
    let r = client().post(format!("{base}/_search")).header("content-encoding", "gzip").header("content-type", "application/json").body(bomb).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 413, "{}", r.text().await.unwrap());
    // A body within the limit is read as before.
    let small = gz(json!({"query": {"match_all": {}}}).to_string().as_bytes());
    let r = client().post(format!("{base}/_search")).header("content-encoding", "gzip").header("content-type", "application/json").body(small).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
}

/// D-B3 and D-B4: OpenSearch reads need the read scope, and the listener
/// keeps refusals and an admin's changes in the audit log.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn opensearch_reads_need_read_and_its_changes_are_audited() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let feed = svc.keys.create("follower", Scope::Feed, None, true, false).unwrap().1;
    let reader = svc.keys.create("dashboards", Scope::Read, None, true, false).unwrap().1;
    let base = opensearch(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let c = client();
    let send = |m: reqwest::Method, path: &str, auth: &str, body: Option<Json>| {
        let r = c.request(m, format!("{base}{path}")).header("authorization", auth);
        let r = match body {
            Some(b) => r.json(&b),
            None => r,
        };
        async move { r.send().await.unwrap().status().as_u16() }
    };
    let root = basic("root-token");
    assert_eq!(send(reqwest::Method::PUT, "/books", &root, Some(json!({"mappings": {"properties": {"title": {"type": "text"}}}}))).await, 200);
    // A feed key: whether the server is up, but no documents.
    let f = format!("Bearer {feed}");
    assert_eq!(send(reqwest::Method::GET, "/", &f, None).await, 200);
    assert_eq!(send(reqwest::Method::GET, "/_cluster/health", &f, None).await, 200);
    for path in ["/books/_search", "/_search", "/books/_count", "/books/_doc/1", "/books/_mapping", "/_cat/indices", "/books"] {
        assert_eq!(send(reqwest::Method::GET, path, &f, None).await, 403, "{path}");
    }
    assert_eq!(send(reqwest::Method::GET, "/books/_search", &format!("Bearer {reader}"), None).await, 200);
    // A wrong password, and an index dropped: both in the audit log.
    assert_eq!(send(reqwest::Method::GET, "/books/_search", &basic("guess"), None).await, 401);
    assert_eq!(send(reqwest::Method::DELETE, "/books", &root, None).await, 200);
    let log = audit(&svc);
    let refused = log.iter().find(|e| e["status"] == 401).unwrap_or_else(|| panic!("no refusal in {log:?}"));
    assert_eq!(refused["kind"], "refusal");
    assert!(refused["route"].as_str().unwrap().starts_with("opensearch "), "{refused}");
    let dropped = log.iter().find(|e| e["method"] == "DELETE" && e["status"] == 200).unwrap_or_else(|| panic!("no DELETE in {log:?}"));
    assert_eq!(dropped["scope"], "admin");
    assert!(log.iter().any(|e| e["status"] == 403 && e["scope"] == "feed"), "{log:?}");
    // A read answered is not audited.
    assert!(!log.iter().any(|e| e["method"] == "GET" && e["status"] == 200), "{log:?}");
}

/// D-B8: only another coordinator, with the cluster's token, posts its
/// alerts' state; an admin key is refused, and the refusal audited.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_admin_key_may_not_post_a_coordinators_alerts() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let admin = svc.keys.create("ops", Scope::Admin, None, true, false).unwrap().1;
    let base = http(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let post = |auth: String| client().post(format!("{base}/v1/alerts/peer")).bearer_auth(auth).header(chloedb_server::peers::PEER_HEADER, "http://10.0.0.9:7700").json(&json!({})).send();
    let r = post(admin).await.unwrap();
    assert_eq!(r.status().as_u16(), 403, "{}", r.text().await.unwrap());
    assert!(audit(&svc).iter().any(|e| e["route"] == "/v1/alerts/peer" && e["status"] == 403));
    assert_eq!(post("root-token".into()).await.unwrap().status().as_u16(), 200);
}

/// D-B12: the UI and the API tell a browser not to frame them, nor to
/// guess a type.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn answers_carry_headers_against_framing_and_sniffing() {
    let dir = tempfile::tempdir().unwrap();
    let base = http(Arc::new(service(dir.path())), Access::default()).await;
    for path in ["/", "/v1/health", "/no/such/page"] {
        let r = client().get(format!("{base}{path}")).send().await.unwrap();
        let h = r.headers();
        assert_eq!(h.get("x-content-type-options").map(|v| v.to_str().unwrap()), Some("nosniff"), "{path}");
        assert_eq!(h.get("x-frame-options").map(|v| v.to_str().unwrap()), Some("SAMEORIGIN"), "{path}");
        let csp = h.get("content-security-policy").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        assert!(csp.contains("frame-ancestors 'self'"), "{path}: {csp:?}");
        assert!(h.contains_key("referrer-policy"), "{path}");
    }
    let os = opensearch(Arc::new(service(&dir.path().join("os"))), Access::default()).await;
    let r = client().get(format!("{os}/")).send().await.unwrap();
    assert_eq!(r.headers().get("x-content-type-options").map(|v| v.to_str().unwrap()), Some("nosniff"));
}

/// D-B7: a client that sends its headers a byte at a time, or keeps a
/// connection open and idle, is cut off.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn headers_that_never_end_and_idle_connections_are_cut_off() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits { header_read: Duration::from_millis(500), ..Default::default() };
    for tls_on in [false, true] {
        let access = Access { limits, tls: tls_on.then(tls), ..Default::default() };
        let (l, port) = listen().await;
        tokio::spawn(server::serve_http(Arc::new(service(&dir.path().join(format!("t{tls_on}")))), l, access));
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        // Over TLS the handshake never starts: a connection that sends
        // nothing at all.
        if !tls_on {
            s.write_all(b"GET /v1/health HTTP/1.1\r\nHo").await.unwrap();
        }
        let t0 = Instant::now();
        let closed = until_closed(&mut s, Duration::from_secs(5)).await;
        assert!(closed && t0.elapsed() < Duration::from_secs(4), "tls {tls_on}: open {:?} after the first byte", t0.elapsed());
    }
    // A keep-alive connection, idle after its answer, is closed too.
    let (l, port) = listen().await;
    tokio::spawn(server::serve_http(Arc::new(service(&dir.path().join("idle"))), l, Access { limits, ..Default::default() }));
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await.unwrap();
    let mut buf = vec![0u8; 64 << 10];
    assert!(s.read(&mut buf).await.unwrap() > 0);
    let t0 = Instant::now();
    assert!(until_closed(&mut s, Duration::from_secs(5)).await, "an idle connection stayed open for {:?}", t0.elapsed());
}

/// Reads `s` until the server closes it (true), or `limit` passes (false).
async fn until_closed(s: &mut TcpStream, limit: Duration) -> bool {
    let mut buf = vec![0u8; 64 << 10];
    tokio::time::timeout(limit, async {
        loop {
            match s.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(_) => continue,
            }
        }
    })
    .await
    .is_ok()
}

/// D-B7: past the cap, the probes are still answered (and only they),
/// a client's further connections are refused, and once one closes a new
/// connection is served in full.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn past_the_cap_only_the_probes_are_answered() {
    let dir = tempfile::tempdir().unwrap();
    let access = Access { limits: Limits { max_connections: 2, per_client: 100, ..Default::default() }, ..Default::default() };
    let (l, port) = listen().await;
    tokio::spawn(server::serve_http(Arc::new(service(dir.path())), l, access));
    let a = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let _b = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Past the cap: a probe is answered at once, anything else is told to
    // come back, and both connections close after their answer.
    for (path, want) in [("/readyz", "HTTP/1.1 200"), ("/v1/health", "HTTP/1.1 200"), ("/v1/tables", "HTTP/1.1 503")] {
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.unwrap();
        let answer = read_all(&mut c, Duration::from_secs(3)).await;
        assert!(answer.starts_with(want), "{path}: {answer}");
        assert!(answer.to_ascii_lowercase().contains("connection: close"), "{path}: {answer}");
    }
    // Two probes at once from one address are taken; a third is closed
    // before it is read.
    let _p1 = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let _p2 = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut p3 = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let _ = p3.write_all(b"GET /readyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await;
    assert!(read_all(&mut p3, Duration::from_secs(3)).await.is_empty(), "a third probe at once from one address");
    // Once a connection closes, a new one is served in full.
    drop(a);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    c.write_all(b"GET /v1/tables HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await.unwrap();
    let answer = read_all(&mut c, Duration::from_secs(5)).await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

/// What the server sends on `s` until it closes it, or `limit` passes.
async fn read_all(s: &mut TcpStream, limit: Duration) -> String {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 << 10];
    let _ = tokio::time::timeout(limit, async {
        loop {
            match s.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
    })
    .await;
    String::from_utf8_lossy(&out).into_owned()
}

/// N-8: one client address holds its share of the connections and no
/// more, and the others still get theirs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_client_holds_its_share_of_the_connections() {
    let dir = tempfile::tempdir().unwrap();
    let access = Access { limits: Limits { max_connections: 8, per_client: 2, ..Default::default() }, ..Default::default() };
    // Every address, so that 127.0.0.2 can call too.
    let l = TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(server::serve_http(Arc::new(service(dir.path())), l, access));
    let _a = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let _b = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let _ = c.write_all(b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await;
    let t0 = Instant::now();
    assert!(read_all(&mut c, Duration::from_secs(3)).await.is_empty(), "a third connection of one client was served");
    assert!(t0.elapsed() < Duration::from_secs(2), "closed at once, not left waiting: {:?}", t0.elapsed());
    // Another address (Linux answers on all of 127/8) is served.
    if cfg!(target_os = "linux") {
        let sock = tokio::net::TcpSocket::new_v4().unwrap();
        sock.bind("127.0.0.2:0".parse().unwrap()).unwrap();
        let mut d = sock.connect(format!("127.0.0.1:{port}").parse().unwrap()).await.unwrap();
        d.write_all(b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await.unwrap();
        let answer = read_all(&mut d, Duration::from_secs(5)).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    }
}

/// HTTP/2's preface and an empty SETTINGS frame: a client that says it
/// speaks HTTP/2 and then asks nothing.
const H2_HELLO: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\x00\x00\x00\x04\x00\x00\x00\x00\x00";

/// N-8: an HTTP/2 connection that asks nothing is closed after the idle
/// timeout, though it would answer the server's pings, on the HTTP
/// listener and on gRPC's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_idle_http2_connection_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let limits = Limits { idle: Duration::from_millis(600), ..Default::default() };
    for grpc in [false, true] {
        let (l, port) = listen().await;
        let access = Access { limits, ..Default::default() };
        match grpc {
            false => drop(tokio::spawn(server::serve_http(svc.clone(), l, access))),
            true => drop(tokio::spawn(server::serve_grpc(svc.clone(), l, access))),
        }
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(H2_HELLO).await.unwrap();
        let t0 = Instant::now();
        assert!(until_closed(&mut s, Duration::from_secs(5)).await, "grpc {grpc}: still open after {:?}", t0.elapsed());
        assert!(t0.elapsed() >= Duration::from_millis(500), "grpc {grpc}: closed before its idle time: {:?}", t0.elapsed());
    }
    // With no idle limit it stays open (HTTP/2 has no header timeout).
    let (l, port) = listen().await;
    tokio::spawn(server::serve_grpc(svc, l, Access { limits: Limits { idle: Duration::ZERO, ..Default::default() }, ..Default::default() }));
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(H2_HELLO).await.unwrap();
    // The server's pings (every --header-timeout-secs, 30 s) are not
    // answered here, so it would close the connection once one timed out:
    // within 1.5 s it is still open.
    assert!(!until_closed(&mut s, Duration::from_millis(1500)).await, "closed without an idle limit");
}

/// N-8: a request body that pauses, or trickles a byte at a time, is cut
/// off rather than holding its connection for as long as the client
/// likes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_bodies_are_cut_off() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits { body: Duration::from_millis(500), ..Default::default() };
    let (l, port) = listen().await;
    tokio::spawn(server::serve_http(Arc::new(service(dir.path())), l, Access { limits, ..Default::default() }));
    let head = |len: usize| format!("POST /v1/query HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {len}\r\n\r\n");
    // A pause: ten bytes of a thousand, then nothing.
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("{}{{\"sql\": \"S", head(1000)).as_bytes()).await.unwrap();
    let t0 = Instant::now();
    let answer = read_all(&mut s, Duration::from_secs(5)).await;
    assert!(t0.elapsed() < Duration::from_secs(3), "a paused body held its request {:?}", t0.elapsed());
    assert!(answer.starts_with("HTTP/1.1 4"), "{answer}");
    // A trickle: a byte every 100 ms, under 1 KiB a second.
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(head(1000).as_bytes()).await.unwrap();
    let t0 = Instant::now();
    let trickle = async {
        loop {
            if s.write_all(b" ").await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            if t0.elapsed() > Duration::from_secs(5) {
                return;
            }
        }
    };
    trickle.await;
    assert!(t0.elapsed() < Duration::from_secs(3), "a trickled body held its request {:?}", t0.elapsed());
    // A body sent at once is read as before.
    let r = client().post(format!("http://127.0.0.1:{port}/v1/query")).json(&json!({"sql": "SELECT COUNT(*) FROM products"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "{}", r.text().await.unwrap());
}

/// N-8: a client that stops reading its answer has its connection closed
/// after the write timeout, which frees its place under the cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_that_stops_reading_is_cut_off() {
    const BIG: usize = 64 << 20;
    let router = axum::Router::new().route("/big", axum::routing::get(|| async { vec![b'x'; BIG] })).route("/small", axum::routing::get(|| async { "small" }));
    let access = Access { limits: Limits { write: Duration::from_millis(500), max_connections: 1, per_client: 1, ..Default::default() }, ..Default::default() };
    let (l, port) = listen().await;
    let (_stop, stopped) = stopper();
    tokio::spawn(async move { server::serve_router_until(router, l, &access, stopped).await });
    let sock = tokio::net::TcpSocket::new_v4().unwrap();
    sock.set_recv_buffer_size(4096).unwrap();
    let mut s = sock.connect(format!("127.0.0.1:{port}").parse().unwrap()).await.unwrap();
    s.write_all(b"GET /big HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await.unwrap();
    // Reads nothing for a while: the server's write waits, then fails.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut got = 0usize;
    let mut buf = vec![0u8; 1 << 20];
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match s.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => got += n,
            }
        }
    })
    .await;
    assert!(got < BIG, "the whole answer came: {got} bytes");
    // Its place under the cap is free again.
    let r = tokio::time::timeout(Duration::from_secs(5), client().get(format!("http://127.0.0.1:{port}/small")).send()).await.expect("served once the slow reader was cut").unwrap();
    assert_eq!(r.text().await.unwrap(), "small");
}

/// D-B5: a server over TLS that stops waits for the requests under way,
/// as it does without TLS, rather than returning (and the process
/// exiting) with them cut.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tls_server_that_stops_finishes_the_requests_under_way() {
    let ca = reqwest::Certificate::from_pem(&std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tls/ca.pem")).unwrap()).unwrap();
    let c = chloedb_server::http_client().add_root_certificate(ca).build().unwrap();
    for tls_on in [false, true] {
        let router = axum::Router::new().route(
            "/slow",
            axum::routing::get(|| async {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                "done"
            }),
        );
        let access = Access { tls: tls_on.then(tls), ..Default::default() };
        let (l, port) = listen().await;
        let (stop, stopped) = stopper();
        let served = tokio::spawn(async move { server::serve_router_until(router, l, &access, stopped).await });
        let url = format!("{}://localhost:{port}/slow", if tls_on { "https" } else { "http" });
        let req = tokio::spawn(c.get(url).send());
        tokio::time::sleep(Duration::from_millis(400)).await;
        stop();
        tokio::time::timeout(Duration::from_secs(5), served).await.expect("the server stopped").unwrap().unwrap();
        // Once serving returns, a process exits: the answer was sent by
        // then (the request needs 1.5 s; the stop came at 0.4).
        let r = tokio::time::timeout(Duration::from_millis(300), req).await.unwrap_or_else(|_| panic!("tls {tls_on}: the server stopped with a request under way"));
        let r = r.unwrap().unwrap_or_else(|e| panic!("tls {tls_on}: the request under way was cut: {e}"));
        assert_eq!(r.text().await.unwrap(), "done");
    }
}

/// D-B6: a server with a follower's feed open stops: the feed ends, and
/// the server with it, well before a supervisor would kill it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_with_a_feed_open_stops() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let (l, port) = listen().await;
    let (stop, stopped) = stopper();
    let served = tokio::spawn(server::serve_http_until(svc, l, Access::default(), stopped));
    let mut feed = client().get(format!("http://127.0.0.1:{port}/v1/feed?from=1")).send().await.unwrap();
    assert_eq!(feed.status().as_u16(), 200);
    assert!(feed.chunk().await.unwrap().is_some(), "the feed's first line");
    let t0 = Instant::now();
    stop();
    // The feed ends cleanly, rather than staying open forever.
    let end = tokio::time::timeout(Duration::from_secs(10), async {
        while let Ok(Some(_)) = feed.chunk().await {}
    })
    .await;
    assert!(end.is_ok(), "the feed was still open {:?} after the stop", t0.elapsed());
    tokio::time::timeout(Duration::from_secs(10), served).await.expect("the server stopped").unwrap().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
}

/// D-B6: a gRPC server with a follower's feed open stops within its
/// grace time: the feed ends, rather than holding the process until a
/// supervisor kills it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_grpc_server_with_a_feed_open_stops() {
    use chloedb_server::pb;
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let (l, port) = listen().await;
    let (stop, stopped) = stopper();
    let served = tokio::spawn(server::serve_grpc_until(svc, l, Access::default(), stopped));
    let mut c = pb::strata_client::StrataClient::connect(format!("http://127.0.0.1:{port}")).await.unwrap();
    let mut feed = c.feed(pb::FeedRequest { from: 1, ..Default::default() }).await.unwrap().into_inner();
    // Its first line says where it starts; then, with no writes, it
    // sends a heartbeat now and then, and would go on for ever.
    assert!(feed.message().await.unwrap().is_some(), "the feed's first line");
    let t0 = Instant::now();
    stop();
    let end = tokio::time::timeout(Duration::from_secs(10), async { while let Ok(Some(_)) = feed.message().await {} }).await;
    assert!(end.is_ok(), "the feed was still open {:?} after the stop", t0.elapsed());
    tokio::time::timeout(Duration::from_secs(10), served).await.expect("the server stopped").unwrap().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
}

/// N-D11: a key limited to tables refused on the gRPC port leaves an entry
/// in the audit log, as an HTTP refusal does; and gRPC over TLS still
/// answers on the new accept loop.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_limited_keys_grpc_refusal_is_audited() {
    use chloedb_server::keys::Grant;
    use chloedb_server::pb;
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let (admin, admin_secret) = svc.keys.create("ops", Scope::Admin, None, false, false).unwrap();
    let (_, secret) = svc.keys.create_limited("an-app", Scope::Read, Some(vec![Grant::new("products", Scope::Read).unwrap()]), None, None, None, false, false).unwrap();
    let (l, port) = listen().await;
    tokio::spawn(server::serve_grpc(svc.clone(), l, Access { tls: Some(tls()), ..Default::default() }));
    let ca = tonic::transport::Certificate::from_pem(std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tls/ca.pem")).unwrap());
    let ch = tonic::transport::Channel::from_shared(format!("https://localhost:{port}")).unwrap().tls_config(tonic::transport::ClientTlsConfig::new().ca_certificate(ca).domain_name("localhost")).unwrap().connect().await.unwrap();
    let call = |secret: String| {
        let mut c = pb::strata_client::StrataClient::with_interceptor(ch.clone(), move |mut req: tonic::Request<()>| {
            req.metadata_mut().insert("authorization", format!("Bearer {secret}").parse().unwrap());
            Ok(req)
        });
        async move { c.query(pb::QueryRequest { sql: "SELECT COUNT(*) FROM products".into(), ..Default::default() }).await }
    };
    let e = call(secret).await.unwrap_err();
    assert_eq!(e.code(), tonic::Code::PermissionDenied, "{e:?}");
    let refused: Vec<Json> = audit(&svc).into_iter().filter(|e| e["who"] == "an-app").collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!((refused[0]["status"].as_u64(), refused[0]["method"].as_str()), (Some(403), Some("gRPC")));
    // An admin key's call is answered, over TLS.
    call(admin_secret).await.unwrap_or_else(|e| panic!("{} over TLS: {e:?}", admin.name));
}

/// N-D12: a server started with --no-fetch-models refuses an admin's
/// Fetch, rather than downloading what it was told not to.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_does_not_fetch_models_refuses_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let mut svc = service(dir.path());
    svc.fetch_models = false;
    let base = http(Arc::new(svc), Access::default()).await;
    let known = chloedb::embed::KNOWN_MODELS[0].0;
    let r = client().post(format!("{base}/v1/admin/models/fetch")).json(&json!({"model": known})).send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    assert_eq!(status, 409, "{text}");
    assert!(text.contains("--no-fetch-models"), "{text}");
}

/// H-12: the published image (the Dockerfile's last stage) has a user
/// other than root that owns its data, which the operator's pods run as
/// (operator::kube's `run_as`), from base images pinned by digest, without
/// the benchmark tools or `tc`; those are the `bench` stage's. Its USER
/// stays root for now: an operator from before 0.2.1 gives its pods no
/// user and runs new images on volumes a root image wrote.
#[test]
fn the_image_runs_as_a_user_from_pinned_bases() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/Dockerfile")).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.starts_with('#')).collect();
    for from in lines.iter().filter(|l| l.starts_with("FROM ")) {
        let image = from.split_whitespace().nth(1).unwrap();
        // A stage built on another stage names it, not an image.
        let stage = lines.iter().any(|l| l.starts_with("FROM ") && l.ends_with(&format!(" AS {image}")));
        assert!(stage || image.contains("@sha256:"), "not pinned by digest: {from}");
    }
    // The last stage is the default target: the image published.
    let last = lines.iter().rposition(|l| l.starts_with("FROM ")).unwrap();
    let published = &lines[last..];
    assert!(text.contains("useradd --system --uid 10001 --gid 10001") && text.contains("chown 10001:10001 /data"), "the image's user 10001 owns /data");
    assert!(published.iter().all(|l| !l.starts_with("USER ")), "the published stage keeps root as its user until older operators are gone (they would run it as 10001 on volumes root wrote)");
    let bench = lines.iter().position(|l| l.starts_with("FROM ") && l.ends_with(" AS bench")).expect("a bench stage");
    // What the published stage takes in: from its base stage down.
    let base = published[0].split_whitespace().nth(1).unwrap();
    let base_at = lines.iter().position(|l| l.starts_with("FROM ") && l.ends_with(&format!(" AS {base}"))).unwrap_or(last);
    let taken: Vec<&&str> = lines[base_at..].iter().enumerate().filter(|(i, _)| !(base_at + i >= bench && base_at + i < last)).map(|(_, l)| l).collect();
    for tool in ["chloedb-server-bench", "engine-compare", "iproute2"] {
        assert!(!taken.iter().any(|l| l.contains(tool)), "the published image holds {tool}");
    }
}
