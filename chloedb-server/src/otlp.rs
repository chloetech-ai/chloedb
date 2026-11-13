//! OpenTelemetry logs in (OTLP): the logs OpenTelemetry's SDKs and its
//! Collector export, taken as the protocol has them and written as rows of
//! a table.
//!
//! - **OTLP/HTTP**: `POST /v1/logs`, the path an exporter adds to its
//!   endpoint, with an `ExportLogsServiceRequest` in protobuf
//!   (`application/x-protobuf`) or in OTLP's JSON (`application/json`),
//!   gzip-compressed or not (`Content-Encoding: gzip`). It answers in the
//!   request's encoding.
//! - **OTLP/gRPC**: `opentelemetry.proto.collector.logs.v1.LogsService/Export`
//!   on the server's gRPC port, gzip-compressed or not.
//!
//! Both take a key with the write scope (`Authorization: Bearer ...`, as
//! an exporter's `headers` set it), as a write does.
//!
//! **The table** is `otel_logs`, or the one the request's `x-strata-table`
//! header (or `?table=` on HTTP) names; it is created on the first request
//! if it is not there ([`create_sql`]), with a column for each part of a
//! log record: `ts` (the record's time, or when it was observed if it has
//! none), `observed_ts`, `severity` (its text, or its number's name),
//! `severity_number`, `service` and `host` (the resource's `service.name`
//! and `host.name`), `body` (text: a string body as it is, another kind
//! as JSON), `trace_id` and `span_id` (hex), `scope` (the instrumentation
//! scope's name), `attributes` (the record's, and its `event_name`, as a
//! JSON object) and `resource` (the resource's other attributes, JSON),
//! and `batch`, a random number per request, which places a request's
//! rows on one shard of a cluster (`shard_by`). The table has no key, as a
//! log has none, and keeps its rows with zstd (`compression = 'zstd'`). A
//! table made by hand takes the columns of these it has, by name.
//!
//! A record the table refuses (a value its column cannot hold, such as
//! attributes nested deeper than a JSON column takes) is left out and
//! counted in the answer's `partial_success` with the first reason; the
//! others are written. A failure to write (the server overloaded, holding
//! its writes, or a shard down) fails the request with the status OTLP
//! clients retry on (429, 503), so that they send it again.

use std::sync::Arc;

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use prost::Message;
use serde_json::{json, Map, Value as Json};
use chloedb::schema::Schema;

use crate::keys::{Principal, Scope};
use crate::service::{After, Service, IMPORT_OPS};
use crate::ApiError;

/// The OpenTelemetry protocol's types for logs, from `otlp/` (`build.rs`).
pub mod pb {
    pub mod opentelemetry {
        pub mod proto {
            pub mod common {
                pub mod v1 {
                    tonic::include_proto!("opentelemetry.proto.common.v1");
                }
            }
            pub mod resource {
                pub mod v1 {
                    tonic::include_proto!("opentelemetry.proto.resource.v1");
                }
            }
            pub mod logs {
                pub mod v1 {
                    tonic::include_proto!("opentelemetry.proto.logs.v1");
                }
            }
            pub mod collector {
                pub mod logs {
                    pub mod v1 {
                        tonic::include_proto!("opentelemetry.proto.collector.logs.v1");
                    }
                }
            }
        }
    }
}

pub use pb::opentelemetry::proto::collector::logs::v1::{logs_service_client, logs_service_server, ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse};
use pb::opentelemetry::proto::common::v1::{any_value, AnyValue, KeyValue};
use pb::opentelemetry::proto::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use pb::opentelemetry::proto::resource::v1::Resource;

/// The table logs go to when a request names none.
pub const TABLE: &str = "otel_logs";

/// The header (and HTTP query parameter `table`) that names the table.
pub const TABLE_HEADER: &str = "x-strata-table";

/// The statement that makes table `table` for logs, if it is not there.
pub fn create_sql(table: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {table} (ts TIMESTAMP NOT NULL, observed_ts TIMESTAMP, severity KEYWORD, severity_number INT, \
         service KEYWORD, host KEYWORD, body TEXT, trace_id KEYWORD, span_id KEYWORD, scope KEYWORD, attributes JSON, resource JSON, \
         batch INT NOT NULL) OPTIONS(shard_by = 'batch', compression = 'zstd')"
    )
}

/// Table `name` if it can be one: letters, digits and `_`, not first a
/// digit.
fn table_name(name: &str) -> Result<&str, ApiError> {
    let ok = !name.is_empty() && name.len() <= 128 && !name.starts_with(|c: char| c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(name).ok_or_else(|| ApiError::bad(format!("{TABLE_HEADER} names a table: letters, digits and _, not {name:?}")))
}

/// The severity a number names (1 to 24, in steps of four: TRACE, DEBUG,
/// INFO, WARN, ERROR, FATAL), for a record without a text of its own.
fn severity_name(n: i32) -> Option<&'static str> {
    const NAMES: [&str; 6] = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL"];
    (1..=24).contains(&n).then(|| NAMES[(n as usize - 1) / 4])
}

/// A value as JSON: bytes in base64, a float that is not a number as
/// text.
fn any_json(v: &AnyValue) -> Json {
    use base64::Engine;
    match &v.value {
        None => Json::Null,
        Some(any_value::Value::StringValue(s)) => json!(s),
        Some(any_value::Value::BoolValue(b)) => json!(b),
        Some(any_value::Value::IntValue(i)) => json!(i),
        Some(any_value::Value::DoubleValue(d)) => serde_json::Number::from_f64(*d).map_or_else(|| json!(d.to_string()), Json::Number),
        Some(any_value::Value::ArrayValue(a)) => Json::Array(a.values.iter().map(any_json).collect()),
        Some(any_value::Value::KvlistValue(kv)) => Json::Object(attributes(&kv.values)),
        Some(any_value::Value::BytesValue(b)) => json!(base64::engine::general_purpose::STANDARD.encode(b)),
    }
}

/// Attributes as a JSON object (a key given twice keeps its last value).
fn attributes(kvs: &[KeyValue]) -> Map<String, Json> {
    kvs.iter().filter(|kv| !kv.key.is_empty()).map(|kv| (kv.key.clone(), kv.value.as_ref().map_or(Json::Null, any_json))).collect()
}

fn hex(b: &[u8]) -> Option<String> {
    (!b.is_empty() && b.iter().any(|&x| x != 0)).then(|| b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Nanoseconds since 1970 as a TIMESTAMP's text; `None` for 0, unset.
fn time(nanos: u64) -> Option<String> {
    (nanos > 0).then(|| chloedb::timestamp::format((nanos / 1000) as i64))
}

/// The rows a request's records make, as the table's documents (with only
/// the columns it has), and the records the table refuses: how many, and
/// the first's reason.
pub struct Rows {
    pub docs: Vec<Json>,
    pub rejected: u64,
    pub error: Option<String>,
}

/// The row of each record of `req` in table `schema`, every row with
/// `batch` in its column of that name.
pub fn rows(req: &ExportLogsServiceRequest, schema: &Schema, batch: i64, now_micros: i64) -> Rows {
    let has = |c: &str| schema.field_index(c).is_some();
    let mut out = Rows { docs: Vec::new(), rejected: 0, error: None };
    for ResourceLogs { resource, scope_logs, .. } in &req.resource_logs {
        let mut res = resource.as_ref().map_or_else(Map::new, |r: &Resource| attributes(&r.attributes));
        // The resource's name and host are columns of their own.
        let mut promoted = |key: &str| match res.remove(key) {
            Some(Json::String(s)) => Some(s),
            Some(other) => {
                res.insert(key.to_string(), other);
                None
            }
            None => None,
        };
        let (service, host) = (promoted("service.name"), promoted("host.name"));
        for ScopeLogs { scope, log_records, .. } in scope_logs {
            let scope = scope.as_ref().map(|s| s.name.clone()).filter(|n| !n.is_empty());
            for r in log_records {
                let doc = record(r, (&service, &host, &scope, &res), batch);
                let doc: Map<String, Json> = doc.into_iter().filter(|(k, v)| !v.is_null() && has(k)).collect();
                let doc = Json::Object(doc);
                match crate::json::document(schema, &doc, now_micros) {
                    Ok(_) => out.docs.push(doc),
                    Err(e) => {
                        out.rejected += 1;
                        out.error.get_or_insert(e.message);
                    }
                }
            }
        }
    }
    out
}

/// One record's row, every column of [`create_sql`]'s.
fn record(r: &LogRecord, (service, host, scope, resource): (&Option<String>, &Option<String>, &Option<String>, &Map<String, Json>), batch: i64) -> Map<String, Json> {
    let mut attrs = attributes(&r.attributes);
    if !r.event_name.is_empty() {
        attrs.insert("event_name".into(), json!(r.event_name));
    }
    let body = r.body.as_ref().and_then(|b| match &b.value {
        Some(any_value::Value::StringValue(s)) => Some(s.clone()),
        None => None,
        Some(_) => Some(any_json(b).to_string()),
    });
    let observed = time(r.observed_time_unix_nano);
    let ts = time(r.time_unix_nano).or_else(|| observed.clone()).unwrap_or_else(|| chloedb::timestamp::format(crate::telemetry::now_ms() as i64 * 1000));
    let severity = Some(r.severity_text.clone()).filter(|s| !s.is_empty()).or_else(|| severity_name(r.severity_number).map(String::from));
    let mut m = Map::new();
    m.insert("ts".into(), json!(ts));
    m.insert("observed_ts".into(), json!(observed));
    m.insert("severity".into(), json!(severity));
    m.insert("severity_number".into(), if r.severity_number > 0 { json!(r.severity_number) } else { Json::Null });
    m.insert("service".into(), json!(service));
    m.insert("host".into(), json!(host));
    m.insert("body".into(), json!(body));
    m.insert("trace_id".into(), json!(hex(&r.trace_id)));
    m.insert("span_id".into(), json!(hex(&r.span_id)));
    m.insert("scope".into(), json!(scope));
    m.insert("attributes".into(), if attrs.is_empty() { Json::Null } else { Json::Object(attrs) });
    m.insert("resource".into(), if resource.is_empty() { Json::Null } else { Json::Object(resource.clone()) });
    m.insert("batch".into(), json!(batch));
    m
}

/// What a request wrote: rows, and those refused ([`Rows`]).
#[derive(Debug, Default)]
pub struct Exported {
    pub written: u64,
    pub rejected: u64,
    pub error: Option<String>,
}

impl Exported {
    /// The OTLP answer: a partial success if any record was refused.
    pub fn response(&self) -> ExportLogsServiceResponse {
        ExportLogsServiceResponse {
            partial_success: (self.rejected > 0).then(|| ExportLogsPartialSuccess { rejected_log_records: self.rejected as i64, error_message: self.error.clone().unwrap_or_default() }),
        }
    }
}

/// The table `name`, made by [`create_sql`] if it is not there.
async fn table(svc: &Arc<Service>, name: &str) -> Result<Arc<Schema>, ApiError> {
    if let Ok(t) = svc.table(name).await {
        return Ok(t);
    }
    // The server makes it, whoever writes the first logs: a key that may
    // write may not make tables, but this one is the logs' own.
    svc.sql(&Principal::open(), &create_sql(name), &chloedb::query::Params::new(), After::Any, 0).await?;
    svc.table(name).await
}

/// Writes the records of `req` as rows of table `name` (default
/// [`TABLE`]), for caller `who`.
pub async fn export(svc: &Arc<Service>, who: &Principal, name: Option<&str>, req: ExportLogsServiceRequest) -> Result<Exported, ApiError> {
    who.may(Scope::Write, "writing logs")?;
    let name = table_name(name.unwrap_or(TABLE))?;
    // A key limited to tables writes logs into a table it may write.
    who.may_table(Scope::Write, name, "OTLP logs")?;
    let schema = table(svc, name).await?;
    let batch: i64 = rand::random::<u32>() as i64 >> 1;
    let now = crate::telemetry::now_ms() as i64 * 1000;
    let (schema_, n) = (schema.clone(), req.resource_logs.len());
    let rows = if n > 0 { tokio::task::spawn_blocking(move || rows(&req, &schema_, batch, now)).await.map_err(ApiError::internal)? } else { Rows { docs: Vec::new(), rejected: 0, error: None } };
    let mut out = Exported { written: 0, rejected: rows.rejected, error: rows.error };
    let table = schema.table.clone();
    let mut docs = rows.docs.into_iter().peekable();
    while docs.peek().is_some() {
        let ops: Vec<Json> = docs.by_ref().take(IMPORT_OPS).map(|d| json!({"op": "insert", "table": table, "doc": d})).collect();
        out.written += svc.write_request(ops, None).await?.rows;
    }
    Ok(out)
}

// ----- OTLP/HTTP

/// How a request's body is encoded, and so its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Protobuf,
    Json,
}

/// An OTLP/HTTP failure: a status, and the reason as OTLP's `Status`
/// message, in the request's encoding.
fn failed(status: u16, message: &str, enc: Encoding) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut r = match enc {
        // google.rpc.Status: code (field 1), message (field 2).
        Encoding::Protobuf => {
            let mut b = Vec::new();
            prost::encoding::int32::encode(1, &grpc_code(status), &mut b);
            prost::encoding::string::encode(2, &message.to_string(), &mut b);
            (code, [(header::CONTENT_TYPE, "application/x-protobuf")], b).into_response()
        }
        Encoding::Json => (code, axum::Json(json!({"code": grpc_code(status), "message": message}))).into_response(),
    };
    if status == 429 || status == 503 {
        r.headers_mut().insert(header::RETRY_AFTER, header::HeaderValue::from(crate::service::RETRY_AFTER.as_secs()));
    }
    r
}

/// The gRPC code an HTTP status stands for, as OTLP's `Status` gives it.
fn grpc_code(status: u16) -> i32 {
    match status {
        400 | 413 | 415 | 422 => 3,
        401 => 16,
        403 => 7,
        404 => 5,
        409 => 10,
        429 => 8,
        503 => 14,
        504 => 4,
        _ => 13,
    }
}

/// The answer to `POST /v1/logs`.
pub async fn http(svc: &Arc<Service>, who: &Principal, headers: &HeaderMap, table: Option<&str>, body: axum::body::Bytes) -> Response {
    let kind = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let enc = match kind.split(';').next().unwrap_or("").trim() {
        "application/x-protobuf" | "application/protobuf" => Encoding::Protobuf,
        "application/json" => Encoding::Json,
        other => return failed(415, &format!("OTLP logs come as application/x-protobuf or application/json, not {other:?}"), Encoding::Json),
    };
    let table = table.or_else(|| headers.get(TABLE_HEADER).and_then(|v| v.to_str().ok()));
    let gzip = headers.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).map(str::trim);
    let body = match gzip {
        None | Some("identity") => body.to_vec(),
        Some("gzip") => match gunzip(&body, svc.max_body) {
            Ok(b) => b,
            Err(e) => return failed(e.status, &e.message, enc),
        },
        Some(other) => return failed(415, &format!("OTLP bodies are gzip-compressed or not, not {other:?}"), enc),
    };
    let req = match enc {
        Encoding::Protobuf => ExportLogsServiceRequest::decode(body.as_slice()).map_err(|e| ApiError::bad(format!("not an OTLP ExportLogsServiceRequest: {e}"))),
        Encoding::Json => serde_json::from_slice::<Json>(&body).map_err(|e| ApiError::bad(format!("not JSON: {e}"))).and_then(|j| from_json(&j)),
    };
    let req = match req {
        Ok(r) => r,
        Err(e) => return failed(e.status, &e.message, enc),
    };
    match export(svc, who, table, req).await {
        Ok(done) => {
            let r = done.response();
            match enc {
                Encoding::Protobuf => ([(header::CONTENT_TYPE, "application/x-protobuf")], r.encode_to_vec()).into_response(),
                Encoding::Json => axum::Json(match r.partial_success {
                    Some(p) => json!({"partialSuccess": {"rejectedLogRecords": p.rejected_log_records, "errorMessage": p.error_message}}),
                    None => json!({}),
                })
                .into_response(),
            }
        }
        Err(e) => failed(e.status, &e.message, enc),
    }
}

/// `body` gunzipped, if it holds at most `max` bytes so.
fn gunzip(body: &[u8], max: usize) -> Result<Vec<u8>, ApiError> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(body).take(max as u64 + 1).read_to_end(&mut out).map_err(|e| ApiError::bad(format!("the body is not gzip: {e}")))?;
    if out.len() > max {
        return Err(ApiError { status: 413, message: format!("the body is more than {max} bytes uncompressed") });
    }
    Ok(out)
}

// ----- OTLP's JSON

/// A request from OTLP's JSON encoding: the protobuf's fields in
/// lowerCamelCase (their own names are taken too), 64-bit integers as
/// numbers or strings, trace and span ids in hex, bytes in base64, and
/// enums as numbers or names.
pub fn from_json(j: &Json) -> Result<ExportLogsServiceRequest, ApiError> {
    let bad = |m: &str| ApiError::bad(format!("OTLP JSON: {m}"));
    let get = |o: &Json, camel: &str, snake: &str| -> Option<Json> { o.get(camel).or_else(|| o.get(snake)).filter(|v| !v.is_null()).cloned() };
    let list = |o: &Json, camel: &str, snake: &str| -> Result<Vec<Json>, ApiError> {
        match get(o, camel, snake) {
            None => Ok(Vec::new()),
            Some(Json::Array(a)) => Ok(a),
            Some(_) => Err(bad(&format!("{camel} is a list"))),
        }
    };
    let text = |o: &Json, camel: &str, snake: &str| get(o, camel, snake).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let int = |o: &Json, camel: &str, snake: &str| -> Result<u64, ApiError> {
        match get(o, camel, snake) {
            None => Ok(0),
            Some(Json::Number(n)) => n.as_u64().ok_or_else(|| bad(&format!("{camel} is a whole number"))),
            Some(Json::String(s)) => s.parse().map_err(|_| bad(&format!("{camel} is a whole number, not {s:?}"))),
            Some(_) => Err(bad(&format!("{camel} is a whole number"))),
        }
    };
    let unhex = |s: &str, what: &str| -> Result<Vec<u8>, ApiError> {
        if s.len() % 2 != 0 {
            return Err(bad(&format!("{what} is hex")));
        }
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2).unwrap_or(""), 16).map_err(|_| bad(&format!("{what} is hex")))).collect()
    };
    let mut req = ExportLogsServiceRequest::default();
    for rl in list(j, "resourceLogs", "resource_logs")? {
        let resource = get(&rl, "resource", "resource").map(|r| Ok::<_, ApiError>(Resource { attributes: key_values(&list(&r, "attributes", "attributes")?)?, ..Default::default() })).transpose()?;
        let mut scope_logs = Vec::new();
        for sl in list(&rl, "scopeLogs", "scope_logs")? {
            let scope = get(&sl, "scope", "scope").map(|s| pb::opentelemetry::proto::common::v1::InstrumentationScope { name: text(&s, "name", "name"), version: text(&s, "version", "version"), ..Default::default() });
            let mut log_records = Vec::new();
            for r in list(&sl, "logRecords", "log_records")? {
                let severity_number = match get(&r, "severityNumber", "severity_number") {
                    None => 0,
                    Some(Json::Number(n)) => n.as_i64().unwrap_or(0) as i32,
                    Some(Json::String(s)) => pb::opentelemetry::proto::logs::v1::SeverityNumber::from_str_name(&s).map_or(0, |n| n as i32),
                    Some(_) => return Err(bad("severityNumber is a number or a name")),
                };
                log_records.push(LogRecord {
                    time_unix_nano: int(&r, "timeUnixNano", "time_unix_nano")?,
                    observed_time_unix_nano: int(&r, "observedTimeUnixNano", "observed_time_unix_nano")?,
                    severity_number,
                    severity_text: text(&r, "severityText", "severity_text"),
                    body: get(&r, "body", "body").map(|b| any_value(&b)).transpose()?,
                    attributes: key_values(&list(&r, "attributes", "attributes")?)?,
                    flags: int(&r, "flags", "flags")? as u32,
                    trace_id: unhex(&text(&r, "traceId", "trace_id"), "traceId")?,
                    span_id: unhex(&text(&r, "spanId", "span_id"), "spanId")?,
                    event_name: text(&r, "eventName", "event_name"),
                    ..Default::default()
                });
            }
            scope_logs.push(ScopeLogs { scope, log_records, ..Default::default() });
        }
        req.resource_logs.push(ResourceLogs { resource, scope_logs, ..Default::default() });
    }
    Ok(req)
}

fn key_values(kvs: &[Json]) -> Result<Vec<KeyValue>, ApiError> {
    kvs.iter()
        .map(|kv| {
            let key = kv.get("key").and_then(Json::as_str).ok_or_else(|| ApiError::bad("OTLP JSON: an attribute has a key"))?;
            Ok(KeyValue { key: key.to_string(), value: kv.get("value").filter(|v| !v.is_null()).map(any_value).transpose()? })
        })
        .collect()
}

fn any_value(v: &Json) -> Result<AnyValue, ApiError> {
    use any_value::Value as V;
    use base64::Engine;
    let bad = |m: &str| ApiError::bad(format!("OTLP JSON: {m}"));
    let field = |camel: &str, snake: &str| v.get(camel).or_else(|| v.get(snake)).filter(|x| !x.is_null());
    let value = if let Some(s) = field("stringValue", "string_value") {
        Some(V::StringValue(s.as_str().ok_or_else(|| bad("stringValue is text"))?.to_string()))
    } else if let Some(b) = field("boolValue", "bool_value") {
        Some(V::BoolValue(b.as_bool().ok_or_else(|| bad("boolValue is true or false"))?))
    } else if let Some(i) = field("intValue", "int_value") {
        Some(V::IntValue(match i {
            Json::Number(n) => n.as_i64().ok_or_else(|| bad("intValue is a whole number"))?,
            Json::String(s) => s.parse().map_err(|_| bad("intValue is a whole number"))?,
            _ => return Err(bad("intValue is a whole number")),
        }))
    } else if let Some(d) = field("doubleValue", "double_value") {
        Some(V::DoubleValue(match d {
            Json::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Json::String(s) => s.parse().map_err(|_| bad("doubleValue is a number"))?,
            _ => return Err(bad("doubleValue is a number")),
        }))
    } else if let Some(a) = field("arrayValue", "array_value") {
        let values = a.get("values").and_then(Json::as_array).map(|xs| xs.iter().map(any_value).collect::<Result<Vec<_>, _>>()).transpose()?.unwrap_or_default();
        Some(V::ArrayValue(pb::opentelemetry::proto::common::v1::ArrayValue { values }))
    } else if let Some(kv) = field("kvlistValue", "kvlist_value") {
        let values = kv.get("values").and_then(Json::as_array).map(|xs| key_values(xs)).transpose()?.unwrap_or_default();
        Some(V::KvlistValue(pb::opentelemetry::proto::common::v1::KeyValueList { values }))
    } else if let Some(b) = field("bytesValue", "bytes_value") {
        let s = b.as_str().ok_or_else(|| bad("bytesValue is base64"))?;
        Some(V::BytesValue(base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| bad("bytesValue is base64"))?))
    } else {
        None
    };
    Ok(AnyValue { value })
}

// ----- OTLP/gRPC

/// The collector's `LogsService`, on the server's gRPC port: a call
/// counted, audited and timed as Strata's own are, as method
/// `LogsExport`, which needs a key that may write.
pub struct Logs(pub crate::grpc::Grpc);

#[tonic::async_trait]
impl logs_service_server::LogsService for Logs {
    async fn export(&self, req: tonic::Request<ExportLogsServiceRequest>) -> Result<tonic::Response<ExportLogsServiceResponse>, tonic::Status> {
        let table = req.metadata().get(TABLE_HEADER).and_then(|v| v.to_str().ok()).map(String::from);
        self.0.serve("LogsExport", req, |svc, who, r| async move { Ok(export(&svc, &who, table.as_deref(), r).await?.response()) }).await
    }
}

/// The service, as the gRPC server adds it: messages up to `max_body`
/// bytes, gzip-compressed or not.
pub fn grpc_service(svc: Arc<Service>, max_body: usize) -> logs_service_server::LogsServiceServer<Logs> {
    logs_service_server::LogsServiceServer::new(Logs(crate::grpc::Grpc(svc))).max_decoding_message_size(max_body).accept_compressed(tonic::codec::CompressionEncoding::Gzip).send_compressed(tonic::codec::CompressionEncoding::Gzip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pb::opentelemetry::proto::common::v1::InstrumentationScope;

    fn s(v: &str) -> Option<AnyValue> {
        Some(AnyValue { value: Some(any_value::Value::StringValue(v.into())) })
    }

    fn kv(k: &str, v: Option<AnyValue>) -> KeyValue {
        KeyValue { key: k.into(), value: v }
    }

    /// The table [`create_sql`] makes.
    fn schema() -> Schema {
        let dir = tempfile::tempdir().unwrap();
        let db = chloedb::db::Db::create_empty(&dir.path().join("logs.strata"), Default::default()).unwrap();
        db.sql(&create_sql("otel_logs"), &chloedb::query::Params::new()).unwrap();
        let s = db.catalog().into_iter().find(|t| t.table == "otel_logs").unwrap().as_ref().clone();
        s
    }

    fn request() -> ExportLogsServiceRequest {
        ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: Some(Resource { attributes: vec![kv("service.name", s("checkout")), kv("host.name", s("web-1")), kv("k8s.pod.name", s("checkout-7f9"))], ..Default::default() }),
                scope_logs: vec![ScopeLogs {
                    scope: Some(InstrumentationScope { name: "app.payments".into(), ..Default::default() }),
                    log_records: vec![
                        LogRecord {
                            time_unix_nano: 1_762_560_000_123_456_789,
                            severity_number: 17,
                            body: s("card declined"),
                            attributes: vec![kv("http.status_code", Some(AnyValue { value: Some(any_value::Value::IntValue(402)) }))],
                            trace_id: vec![0xab; 16],
                            span_id: vec![0x01; 8],
                            ..Default::default()
                        },
                        LogRecord { observed_time_unix_nano: 1_762_560_001_000_000_000, severity_text: "Warning".into(), body: Some(AnyValue { value: Some(any_value::Value::KvlistValue(pb::opentelemetry::proto::common::v1::KeyValueList { values: vec![kv("a", s("b"))] })) }), event_name: "retry".into(), ..Default::default() },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn records_become_rows() {
        let rows = rows(&request(), &schema(), 7, 0);
        assert_eq!((rows.docs.len(), rows.rejected), (2, 0));
        assert_eq!(
            rows.docs[0],
            json!({"ts": "2025-11-08T00:00:00.123456Z", "severity": "ERROR", "severity_number": 17, "service": "checkout", "host": "web-1", "body": "card declined",
                   "trace_id": "abababababababababababababababab", "span_id": "0101010101010101", "scope": "app.payments",
                   "attributes": {"http.status_code": 402}, "resource": {"k8s.pod.name": "checkout-7f9"}, "batch": 7})
        );
        // No time of its own: when it was observed. Another kind of body
        // is its JSON; an event's name is an attribute.
        assert_eq!(rows.docs[1]["ts"], json!("2025-11-08T00:00:01Z"));
        assert_eq!(rows.docs[1]["observed_ts"], json!("2025-11-08T00:00:01Z"));
        assert_eq!(rows.docs[1]["severity"], json!("Warning"));
        assert_eq!(rows.docs[1]["body"], json!(r#"{"a":"b"}"#));
        assert_eq!(rows.docs[1]["attributes"], json!({"event_name": "retry"}));
        assert!(rows.docs[1].get("trace_id").is_none() && rows.docs[1].get("severity_number").is_none());
    }

    #[test]
    fn a_record_the_table_refuses_is_counted_and_the_others_kept() {
        // Attributes nested deeper than a JSON column takes.
        let mut deep = s("x");
        for _ in 0..20 {
            deep = Some(AnyValue { value: Some(any_value::Value::KvlistValue(pb::opentelemetry::proto::common::v1::KeyValueList { values: vec![kv("k", deep)] })) });
        }
        let mut req = request();
        req.resource_logs[0].scope_logs[0].log_records[1].attributes = vec![kv("deep", deep)];
        let rows = rows(&req, &schema(), 1, 0);
        assert_eq!((rows.docs.len(), rows.rejected), (1, 1));
        assert!(rows.error.as_deref().is_some_and(|e| e.contains("16")), "{:?}", rows.error);
        let r = Exported { written: 1, rejected: rows.rejected, error: rows.error }.response();
        assert_eq!(r.partial_success.map(|p| p.rejected_log_records), Some(1));
    }

    #[test]
    fn otlp_json_reads_as_protobuf_does() {
        let j = json!({"resourceLogs": [{
            "resource": {"attributes": [{"key": "service.name", "value": {"stringValue": "checkout"}}, {"key": "host.name", "value": {"stringValue": "web-1"}}, {"key": "k8s.pod.name", "value": {"stringValue": "checkout-7f9"}}]},
            "scopeLogs": [{"scope": {"name": "app.payments"}, "logRecords": [
                {"timeUnixNano": "1762560000123456789", "severityNumber": "SEVERITY_NUMBER_ERROR", "body": {"stringValue": "card declined"},
                 "attributes": [{"key": "http.status_code", "value": {"intValue": "402"}}], "traceId": "abababababababababababababababab", "spanId": "0101010101010101"},
                {"observed_time_unix_nano": 1762560001000000000u64, "severityText": "Warning", "body": {"kvlistValue": {"values": [{"key": "a", "value": {"stringValue": "b"}}]}}, "eventName": "retry"}
            ]}]
        }]});
        assert_eq!(from_json(&j).unwrap(), request());
        assert!(from_json(&json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [{"traceId": "xyz"}]}]}]})).is_err());
    }

    #[test]
    fn names_and_severities() {
        assert!(table_name("otel_logs").is_ok() && table_name("Logs2").is_ok());
        for bad in ["", "1logs", "logs; DROP TABLE x", "a-b"] {
            assert!(table_name(bad).is_err(), "{bad}");
        }
        assert_eq!([1, 4, 5, 9, 13, 17, 21, 24].map(severity_name), [Some("TRACE"), Some("TRACE"), Some("DEBUG"), Some("INFO"), Some("WARN"), Some("ERROR"), Some("FATAL"), Some("FATAL")]);
        assert_eq!(severity_name(0), None);
    }
}
