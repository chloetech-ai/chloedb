//! A table's rows out, read by a scroll ([`crate::service::scroll`]): what
//! `GET /v1/export?format=jsonl|csv|arrow|parquet&tables=T` answers, on a
//! coordinator from the scroll it serves over its shards, and on one
//! server from its own. JSON lines and CSV stream a page at a time; Arrow
//! and Parquet are written once every page is read (Arrow as one batch,
//! in memory; Parquet to a temporary file, a page at a time), since each
//! needs one schema for the whole file. A key with read on the table may
//! export it.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use chloedb::query::sql::ident;
use chloedb::query::Params;
use chloedb::value::Value;
use serde_json::Value as Json;

use crate::json as js;
use crate::service::ScrollAsk;
use crate::service::{QueryReq, Service};
use crate::ApiError;

/// The rows a page of the export's scroll holds (cut to the server's
/// `--max-result-window`).
const PAGE: usize = 5000;

/// The most rows an Arrow export holds, built in memory.
pub const ARROW_ROWS: usize = 1_000_000;

/// The formats a table's rows go out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jsonl,
    Csv,
    Arrow,
    Parquet,
}

impl Format {
    pub fn parse(f: &str) -> Option<Format> {
        match f.to_ascii_lowercase().as_str() {
            "jsonl" | "ndjson" => Some(Format::Jsonl),
            "csv" => Some(Format::Csv),
            "arrow" => Some(Format::Arrow),
            "parquet" => Some(Format::Parquet),
            _ => None,
        }
    }
}

/// The scroll's pages: the first answered, then each next one.
struct Pages {
    svc: Arc<Service>,
    owner: String,
    cursor: Option<String>,
    first: Option<chloedb::query::exec::ResultSet>,
}

impl Pages {
    async fn open(svc: &Arc<Service>, owner: &str, sql: String, params: Params) -> Result<Pages, ApiError> {
        let a = svc.query(QueryReq { sql, params, scroll: Some(ScrollAsk { page: PAGE, keep_alive: None, owner: owner.to_string() }), ..Default::default() }).await?;
        Ok(Pages { svc: svc.clone(), owner: owner.to_string(), cursor: a.cursor, first: Some(a.rs) })
    }

    /// The next page, none once every row was read.
    async fn next(&mut self) -> Result<Option<chloedb::query::exec::ResultSet>, ApiError> {
        if let Some(rs) = self.first.take() {
            return Ok(Some(rs));
        }
        let Some(c) = self.cursor.take() else { return Ok(None) };
        let a = self.svc.scroll_next(&c, &self.owner, None, 0, None).await?;
        self.cursor = a.cursor;
        Ok(Some(a.rs))
    }
}

/// `table`'s rows in `format`, read by a scroll that `owner` holds
/// ([`crate::service::scroll::owner`]).
pub async fn rows(svc: &Arc<Service>, owner: &str, table: &str, format: Format) -> Result<Response, ApiError> {
    let schema = svc.table(table).await?;
    let mut pages = Pages::open(svc, owner, format!("SELECT * FROM {}", ident(&schema.table)), Params::new()).await?;
    let name = schema.table.clone();
    match format {
        Format::Jsonl | Format::Csv => {
            let csv = format == Format::Csv;
            let stream = futures::stream::unfold((pages, true), move |(mut pages, first)| async move {
                match pages.next().await {
                    Ok(Some(rs)) => {
                        let mut out = String::new();
                        if csv && first {
                            out.push_str(&rs.columns.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
                            out.push('\n');
                        }
                        for row in &rs.rows {
                            match csv {
                                true => {
                                    out.push_str(&row.iter().map(csv_value).collect::<Vec<_>>().join(","));
                                    out.push('\n');
                                }
                                false => {
                                    let o: serde_json::Map<String, Json> = rs.columns.iter().zip(row).map(|(c, v)| (c.clone(), js::to_json(v))).collect();
                                    out.push_str(&Json::Object(o).to_string());
                                    out.push('\n');
                                }
                            }
                        }
                        Some((Ok::<_, std::io::Error>(Bytes::from(out)), (pages, false)))
                    }
                    Ok(None) => None,
                    // The answer has begun: the error ends it, as a line the
                    // reader sees.
                    Err(e) => {
                        let line = match csv {
                            true => format!("# export failed: {}\n", e.message),
                            false => format!("{}\n", serde_json::json!({"error": e.message})),
                        };
                        pages.cursor = None;
                        Some((Ok(Bytes::from(line)), (pages, false)))
                    }
                }
            });
            let (ct, ext) = if csv { ("text/csv; charset=utf-8", "csv") } else { ("application/x-ndjson", "jsonl") };
            Ok((StatusCode::OK, [(header::CONTENT_TYPE, ct.to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}.{ext}\""))], Body::from_stream(stream)).into_response())
        }
        Format::Arrow => arrow(&mut pages, &name).await,
        Format::Parquet => parquet(&mut pages, &schema).await,
    }
}

/// Every row `sql` answers, read by a scroll, so that no window cuts it:
/// what a coordinator runs a rank log's export by ([`rank_log`]).
pub async fn all_rows(svc: &Arc<Service>, owner: &str, sql: &str, params: &Params) -> Result<chloedb::query::exec::ResultSet, ApiError> {
    let mut pages = Pages::open(svc, owner, sql.to_string(), params.clone()).await?;
    let mut all: Option<chloedb::query::exec::ResultSet> = None;
    while let Some(rs) = pages.next().await? {
        match &mut all {
            None => all = Some(rs),
            Some(a) => {
                a.rows.extend(rs.rows);
                a.keys.extend(rs.keys);
                a.scores.extend(rs.scores);
            }
        }
    }
    all.ok_or_else(|| ApiError::internal("a scroll without a first page"))
}

/// A rank log's export through a coordinator ([`chloedb::rank_log::export`]):
/// its queries run over every shard, each read whole by a scroll, and its
/// records given to `out` as they come. Each query reads the shards as
/// they are then, not one snapshot of them all, as a server's export does.
pub async fn rank_log(svc: &Arc<Service>, owner: &str, x: chloedb::rank_log::Export, mut out: impl FnMut(Json) -> chloedb::Result<()> + Send + 'static) -> Result<u64, ApiError> {
    let tables = svc.catalog();
    let (svc, owner) = (svc.clone(), owner.to_string());
    let rt = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let refs: Vec<&chloedb::Schema> = tables.iter().map(|t| &**t).collect();
        let mut run = |sql: &str, params: &Params| rt.block_on(all_rows(&svc, &owner, sql, params)).map_err(|e| chloedb::Error::Query(e.message));
        chloedb::rank_log::export(&mut run, &refs, &x, &mut out).map_err(ApiError::from)
    })
    .await
    .map_err(ApiError::internal)?
}

#[cfg(feature = "datafusion")]
async fn arrow(pages: &mut Pages, name: &str) -> Result<Response, ApiError> {
    let mut all: Option<chloedb::query::exec::ResultSet> = None;
    while let Some(rs) = pages.next().await? {
        match &mut all {
            None => all = Some(rs),
            Some(a) => a.rows.extend(rs.rows),
        }
        if all.as_ref().is_some_and(|a| a.rows.len() > ARROW_ROWS) {
            return Err(ApiError::bad(format!("an Arrow export holds at most {ARROW_ROWS} rows, built in memory: export the table as JSON lines, CSV or Parquet, or read it with a scroll (page_size, format arrow)")));
        }
    }
    let rs = all.ok_or_else(|| ApiError::internal("a scroll without a first page"))?;
    let bytes = tokio::task::spawn_blocking(move || {
        let (batch, _) = chloedb::query::arrow::answer_batch(chloedb::query::exec::Leaf::Rows(rs))?;
        chloedb::query::arrow::ipc_of(&batch)
    })
    .await
    .map_err(ApiError::internal)??;
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/vnd.apache.arrow.stream".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}.arrow\""))], bytes).into_response())
}

#[cfg(not(feature = "datafusion"))]
async fn arrow(_: &mut Pages, _: &str) -> Result<Response, ApiError> {
    Err(ApiError::bad("this server was built without Arrow answers (feature datafusion)"))
}

#[cfg(feature = "parquet")]
async fn parquet(pages: &mut Pages, schema: &chloedb::Schema) -> Result<Response, ApiError> {
    let dir = tempfile::tempdir().map_err(ApiError::internal)?;
    let path = dir.path().join("export.parquet");
    let (p, s) = (path.clone(), schema.clone());
    let mut w = tokio::task::spawn_blocking(move || chloedb::parquet::Writer::create(&p, &s, true)).await.map_err(ApiError::internal)??;
    while let Some(rs) = pages.next().await? {
        // A page's columns by the table's fields: one the table does not
        // store (generated) is NULL.
        let at: Vec<Option<usize>> = rs.columns.iter().map(|c| schema.field_index(c)).collect();
        let docs: Vec<chloedb::value::Document> = rs
            .rows
            .into_iter()
            .map(|row| {
                let mut d = vec![Value::Null; schema.fields.len()];
                for (v, i) in row.into_iter().zip(&at) {
                    if let Some(i) = i {
                        d[*i] = v;
                    }
                }
                d
            })
            .collect();
        w = tokio::task::spawn_blocking(move || w.write(&docs).map(|()| w)).await.map_err(ApiError::internal)??;
    }
    tokio::task::spawn_blocking(move || w.finish()).await.map_err(ApiError::internal)??;
    let bytes = tokio::fs::read(&path).await.map_err(ApiError::internal)?;
    drop(dir);
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/vnd.apache.parquet".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}.parquet\"", schema.table))], bytes).into_response())
}

#[cfg(not(feature = "parquet"))]
async fn parquet(_: &mut Pages, _: &chloedb::Schema) -> Result<Response, ApiError> {
    Err(ApiError::bad("this server was built without Parquet (feature parquet)"))
}

/// A CSV cell of text: quoted when it holds a comma, a quote or a line
/// break.
fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A value as a CSV cell, as `/v1/import` reads one back: NULL empty, a
/// list's values joined by `|`, a JSON value as its JSON text.
fn csv_value(v: &Value) -> String {
    match js::to_json(v) {
        Json::Null => String::new(),
        Json::String(s) => csv_cell(&s),
        Json::Array(items) if items.iter().all(|i| !i.is_object() && !i.is_array()) => csv_cell(
            &items
                .iter()
                .map(|i| match i {
                    Json::String(s) => s.clone(),
                    o => o.to_string(),
                })
                .collect::<Vec<_>>()
                .join("|"),
        ),
        o => csv_cell(&o.to_string()),
    }
}
