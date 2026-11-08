//! OpenTelemetry logs in (OTLP): records sent over OTLP/HTTP (protobuf,
//! gzip-compressed protobuf and JSON) and over OTLP/gRPC land as rows of
//! `otel_logs`, which the first request creates, and answer SQL as rows
//! do; records the table refuses are counted in a partial success; and a
//! coordinator of two shards takes them too, placing each request's rows
//! by its batch, and connects again over that table.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use prost::Message;
use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::otlp::pb::opentelemetry::proto::common::v1::{any_value, AnyValue, InstrumentationScope, KeyValue, KeyValueList};
use strata_server::otlp::pb::opentelemetry::proto::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use strata_server::otlp::pb::opentelemetry::proto::resource::v1::Resource;
use strata_server::otlp::{logs_service_client::LogsServiceClient, ExportLogsServiceRequest, ExportLogsServiceResponse};
use strata_server::server;
use strata_server::service::{AgentConfig, Backend, Service};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn service(backend: Backend, schema: strata::Schema) -> Arc<Service> {
    Arc::new(Service::new(backend, schema, agent(), 16, Duration::from_secs(20)))
}

fn empty(dir: &std::path::Path, name: &str) -> Arc<Service> {
    let db = Db::create_empty(&dir.join(name), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    service(Backend::Local(Arc::new(db)), schema)
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

async fn grpc(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

fn s(v: &str) -> Option<AnyValue> {
    Some(AnyValue { value: Some(any_value::Value::StringValue(v.into())) })
}

fn kv(k: &str, v: Option<AnyValue>) -> KeyValue {
    KeyValue { key: k.into(), value: v }
}

/// `n` records of service `service`, from line `from` on: one a second
/// from 2025-11-08, every third an error with an http.status_code.
fn request(service: &str, from: u64, n: u64) -> ExportLogsServiceRequest {
    let records = (from..from + n)
        .map(|i| LogRecord {
            time_unix_nano: (1_762_560_000 + i) * 1_000_000_000,
            severity_number: if i % 3 == 0 { 17 } else { 9 },
            body: s(&format!("line {i}: {}", if i % 3 == 0 { "payment declined" } else { "request served" })),
            attributes: if i % 3 == 0 { vec![kv("http.status_code", Some(AnyValue { value: Some(any_value::Value::IntValue(402)) }))] } else { vec![] },
            trace_id: if i % 2 == 0 { (0..16).map(|b| b as u8 ^ i as u8).collect() } else { vec![] },
            ..Default::default()
        })
        .collect();
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource { attributes: vec![kv("service.name", s(service)), kv("host.name", s("web-1")), kv("deployment.environment", s("prod"))], ..Default::default() }),
            scope_logs: vec![ScopeLogs { scope: Some(InstrumentationScope { name: "app".into(), ..Default::default() }), log_records: records, ..Default::default() }],
            ..Default::default()
        }],
    }
}

/// A POST of `body` to `/v1/logs`: its status, content type and bytes.
async fn post(url: &str, content_type: &str, gzip: bool, body: Vec<u8>) -> (u16, String, Vec<u8>) {
    let c = strata_server::http_client().build().unwrap();
    let mut r = c.post(format!("{url}/v1/logs")).header("content-type", content_type);
    let body = if gzip {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&body).unwrap();
        r = r.header("content-encoding", "gzip");
        e.finish().unwrap()
    } else {
        body
    };
    let a = r.body(body).send().await.unwrap();
    let status = a.status().as_u16();
    let kind = a.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    (status, kind, a.bytes().await.unwrap().to_vec())
}

async fn sql(url: &str, q: &str) -> Json {
    let c = strata_server::http_client().build().unwrap();
    let a = c.post(format!("{url}/v1/query")).json(&json!({"sql": q})).send().await.unwrap();
    let status = a.status().as_u16();
    let j: Json = a.json().await.unwrap();
    assert_eq!(status, 200, "{q}: {j}");
    j["rows"].clone()
}

/// The records' JSON in OTLP's encoding, as an exporter sends it.
fn otlp_json(service: &str, from: u64, n: u64) -> Json {
    let records: Vec<Json> = (from..from + n)
        .map(|i| json!({"timeUnixNano": ((1_762_560_000 + i) * 1_000_000_000).to_string(), "severityText": "INFO", "body": {"stringValue": format!("line {i}: from json")}}))
        .collect();
    json!({"resourceLogs": [{"resource": {"attributes": [{"key": "service.name", "value": {"stringValue": service}}]}, "scopeLogs": [{"logRecords": records}]}]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_takes_otlp_logs_over_http_and_grpc() {
    let dir = tempfile::tempdir().unwrap();
    let svc = empty(dir.path(), "one.strata");
    let url = http(svc.clone()).await;

    // Protobuf, then gzip-compressed protobuf: the first creates the table.
    let (status, kind, body) = post(&url, "application/x-protobuf", false, request("checkout", 0, 30).encode_to_vec()).await;
    assert_eq!((status, kind.as_str()), (200, "application/x-protobuf"), "{}", String::from_utf8_lossy(&body));
    assert_eq!(ExportLogsServiceResponse::decode(body.as_slice()).unwrap(), ExportLogsServiceResponse::default());
    let (status, ..) = post(&url, "application/x-protobuf", true, request("checkout", 30, 30).encode_to_vec()).await;
    assert_eq!(status, 200);
    // JSON, which answers in JSON.
    let (status, kind, body) = post(&url, "application/json", false, serde_json::to_vec(&otlp_json("search", 60, 10)).unwrap()).await;
    assert_eq!((status, kind.as_str()), (200, "application/json"), "{}", String::from_utf8_lossy(&body));
    assert_eq!(serde_json::from_slice::<Json>(&body).unwrap(), json!({}));

    // gRPC, gzip-compressed as the Collector sends it.
    let g = grpc(svc.clone()).await;
    let mut client = LogsServiceClient::connect(g).await.unwrap().send_compressed(tonic::codec::CompressionEncoding::Gzip);
    let a = client.export(request("auth", 70, 20)).await.unwrap().into_inner();
    assert_eq!(a, ExportLogsServiceResponse::default());

    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs").await, json!([[90]]));
    assert_eq!(sql(&url, "SELECT service, COUNT(*) FROM otel_logs GROUP BY service ORDER BY service").await, json!([["auth", 20], ["checkout", 60], ["search", 10]]));
    // Every third protobuf record an error with its status code (20 of
    // checkout's, 6 of auth's), found by severity, by its words and by its
    // attributes.
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs WHERE severity = 'ERROR'").await, json!([[26]]));
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs WHERE MATCH(body, 'declined')").await, json!([[26]]));
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs WHERE attributes['http.status_code'] = 402").await, json!([[26]]));
    assert_eq!(sql(&url, "SELECT service, COUNT(*) FROM otel_logs WHERE severity = 'INFO' GROUP BY service ORDER BY service").await, json!([["auth", 14], ["checkout", 40], ["search", 10]]));
    assert_eq!(
        sql(&url, "SELECT ts, severity, host, body, trace_id, scope, resource FROM otel_logs WHERE service = 'checkout' ORDER BY ts LIMIT 1").await,
        json!([["2025-11-08T00:00:00Z", "ERROR", "web-1", "line 0: payment declined", "000102030405060708090a0b0c0d0e0f", "app", "{\"deployment.environment\":\"prod\"}"]])
    );
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs WHERE ts >= '2025-11-08T00:01:00Z'").await, json!([[30]]));
    let t = svc.table("otel_logs").await.unwrap();
    assert_eq!((t.primary_key, t.shard_by.as_deref(), t.compression), (None, Some("batch"), strata::schema::Compression::Zstd));

    // A record the table refuses: counted, the others written.
    let mut req = request("checkout", 90, 3);
    let mut deep = s("x");
    for _ in 0..20 {
        deep = Some(AnyValue { value: Some(any_value::Value::KvlistValue(KeyValueList { values: vec![kv("k", deep)] })) });
    }
    req.resource_logs[0].scope_logs[0].log_records[1].attributes = vec![kv("deep", deep)];
    let (status, _, body) = post(&url, "application/x-protobuf", false, req.encode_to_vec()).await;
    assert_eq!(status, 200);
    let p = ExportLogsServiceResponse::decode(body.as_slice()).unwrap().partial_success.unwrap();
    assert_eq!(p.rejected_log_records, 1, "{p:?}");
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs").await, json!([[92]]));

    // Another table, by header; and what is not OTLP.
    let c = strata_server::http_client().build().unwrap();
    let a = c.post(format!("{url}/v1/logs")).header("content-type", "application/x-protobuf").header("x-strata-table", "audit_logs").body(request("auth", 0, 5).encode_to_vec()).send().await.unwrap();
    assert_eq!(a.status().as_u16(), 200);
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM audit_logs").await, json!([[5]]));
    let (status, ..) = post(&url, "text/plain", false, b"hello".to_vec()).await;
    assert_eq!(status, 415);
    let (status, kind, _) = post(&url, "application/x-protobuf", false, b"\xff\xff\xff".to_vec()).await;
    assert_eq!((status, kind.as_str()), (400, "application/x-protobuf"));
    let (status, ..) = post(&url, "application/json", false, br#"{"resourceLogs": [{"scopeLogs": [{"logRecords": [{"traceId": "xyz"}]}]}]}"#.to_vec()).await;
    assert_eq!(status, 400);
    let a = c.post(format!("{url}/v1/logs")).header("content-type", "application/x-protobuf").header("x-strata-table", "bad name").body(Vec::new()).send().await.unwrap();
    assert_eq!(a.status().as_u16(), 400);
    // The log store's own lines are still read at GET /v1/logs.
    assert_eq!(c.get(format!("{url}/v1/logs")).send().await.unwrap().status().as_u16(), 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_takes_otlp_logs_and_places_them_by_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (s0, s1) = (empty(dir.path(), "s0.strata"), empty(dir.path(), "s1.strata"));
    let shards = vec![grpc(s0.clone()).await, grpc(s1.clone()).await];
    let c = Coordinator::connect(shards.clone()).await.unwrap();
    let schema = c.schema.clone();
    let url = http(service(Backend::Cluster(Arc::new(c)), schema)).await;
    for i in 0..20 {
        let (status, _, body) = post(&url, "application/x-protobuf", false, request("checkout", i * 10, 10).encode_to_vec()).await;
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    }
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs").await, json!([[200]]));
    assert_eq!(sql(&url, "SELECT COUNT(*) FROM otel_logs WHERE MATCH(body, 'declined')").await, json!([[67]]));
    // Twenty requests, each on the shard its batch places it: both hold
    // some, and each request's rows are on one.
    let held = |svc: &Arc<Service>| {
        let svc = svc.clone();
        async move {
            let r = svc.query(strata_server::service::QueryReq { sql: "SELECT COUNT(*) FROM otel_logs".into(), ..Default::default() }).await.unwrap();
            strata_server::json::result(&r.rs, 0.0)["rows"][0][0].as_u64().unwrap()
        }
    };
    let (a, b) = (held(&s0).await, held(&s1).await);
    assert_eq!(a + b, 200);
    assert!(a > 0 && b > 0, "{a} {b}");
    assert_eq!((a % 10, b % 10), (0, 0), "{a} {b}");
    // A coordinator connects again to shards whose first table has no key.
    Coordinator::connect(shards).await.unwrap();
}

/// Sends `req` to `/v1/logs` as protobuf with `auth` as the bearer
/// credential: the status.
async fn post_as(url: &str, auth: Option<&str>, req: &ExportLogsServiceRequest) -> u16 {
    let c = strata_server::http_client().build().unwrap();
    let mut r = c.post(format!("{url}/v1/logs")).header("content-type", "application/x-protobuf").body(req.encode_to_vec());
    if let Some(a) = auth {
        r = r.bearer_auth(a);
    }
    r.send().await.unwrap().status().as_u16()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logs_take_a_key_that_may_write() {
    use strata_server::auth::Secret;
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("a.strata"), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(20));
    svc.keys = Arc::new(strata_server::keys::Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    let svc = Arc::new(svc);
    let access = Access { token: Some(Secret::new("root-token")), ..Default::default() };
    let (h, g) = (TcpListener::bind("127.0.0.1:0").await.unwrap(), TcpListener::bind("127.0.0.1:0").await.unwrap());
    let (url, grpc_url) = (format!("http://{}", h.local_addr().unwrap()), format!("http://{}", g.local_addr().unwrap()));
    tokio::spawn(server::serve_http(svc.clone(), h, access.clone()));
    tokio::spawn(server::serve_grpc(svc.clone(), g, access));
    let c = strata_server::http_client().build().unwrap();
    let mut keys = Vec::new();
    for scope in ["read", "write"] {
        let j: Json = c.post(format!("{url}/v1/admin/keys")).bearer_auth("root-token").json(&json!({"name": format!("otel-{scope}"), "scope": scope})).send().await.unwrap().json().await.unwrap();
        keys.push(j["secret"].as_str().unwrap().to_string());
    }
    let (read, write) = (keys[0].as_str(), keys[1].as_str());

    // None, a read key, then a write key, which may make the table.
    assert_eq!(post_as(&url, None, &request("api", 0, 5)).await, 401);
    assert_eq!(post_as(&url, Some(read), &request("api", 0, 5)).await, 403);
    assert_eq!(post_as(&url, Some(write), &request("api", 0, 5)).await, 200);
    let as_grpc = |key: &str| {
        let token: tonic::metadata::MetadataValue<_> = format!("Bearer {key}").parse().unwrap();
        move |mut r: tonic::Request<()>| {
            r.metadata_mut().insert("authorization", token.clone());
            Ok(r)
        }
    };
    let channel = tonic::transport::Endpoint::from_shared(grpc_url).unwrap().connect().await.unwrap();
    let e = LogsServiceClient::with_interceptor(channel.clone(), as_grpc(read)).export(request("api", 5, 5)).await.unwrap_err();
    assert_eq!(e.code(), tonic::Code::PermissionDenied, "{e:?}");
    LogsServiceClient::with_interceptor(channel, as_grpc(write)).export(request("api", 5, 5)).await.unwrap();
    let j: Json = c.post(format!("{url}/v1/query")).bearer_auth(read).json(&json!({"sql": "SELECT COUNT(*) FROM otel_logs"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(j["rows"], json!([[10]]), "{j}");
    // The refusals are audited, the gRPC one by its method.
    let j: Json = c.get(format!("{url}/v1/admin/audit?kind=refusal")).bearer_auth("root-token").send().await.unwrap().json().await.unwrap();
    let routes: Vec<&str> = j["entries"].as_array().unwrap().iter().filter(|e| e["status"] == 403).map(|e| e["route"].as_str().unwrap_or("")).collect();
    assert!(routes.contains(&"LogsExport") && routes.contains(&"/v1/logs"), "{j}");
}
