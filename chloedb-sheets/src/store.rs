//! Workbooks in Strata: the tables they are kept in, and reading, saving,
//! listing, searching and deleting them.
//!
//! `sheets_books` has a row a workbook: its name, when it was made and
//! changed, its sheets, and its frame (IronCalc's encoding of the workbook
//! without its cells, base64). `sheets_cells` has a row a cell that holds
//! something, keyed `<book>:<sheet id>:<row>:<column>`, placed by the
//! workbook (`shard_by = 'book'`), so that a workbook's cells sit on one
//! shard and a save of them is one transaction there. Any SQL reads them:
//!
//! ```sql
//! SELECT sheet_name, ref, text FROM sheets_cells WHERE book = 'b1x9' AND num > 1000;
//! SELECT book, ref, text FROM sheets_cells WHERE MATCH(text, 'invoice') ORDER BY bm25(text, 'invoice') DESC LIMIT 20;
//! ```

use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

use crate::book::{self, Book, Ext, Key, Restored, Save, Strings};
use crate::strata::{lit, Strata};

pub const BOOKS: &str = "sheets_books";
pub const CELLS: &str = "sheets_cells";

pub const CREATE_BOOKS: &str = "CREATE TABLE IF NOT EXISTS sheets_books (
  id KEYWORD PRIMARY KEY,
  name TEXT,
  created TIMESTAMP,
  updated TIMESTAMP,
  source KEYWORD,
  sheets INT,
  cells INT,
  sheet_names TEXT,
  engine KEYWORD,
  ext TEXT OPTIONS(indexed = false),
  frame TEXT OPTIONS(indexed = false)
)";

pub const CREATE_CELLS: &str = "CREATE TABLE IF NOT EXISTS sheets_cells (
  id KEYWORD PRIMARY KEY,
  book KEYWORD NOT NULL,
  sheet INT,
  sheet_name KEYWORD,
  row INT,
  col INT,
  ref KEYWORD,
  kind KEYWORD,
  input TEXT OPTIONS(indexed = false),
  text TEXT,
  num FLOAT,
  style INT,
  f INT,
  extra KEYWORD
) OPTIONS(shard_by = 'book', cluster_by = 'book')";

/// Operations in one write; a larger save (an import) takes several, the
/// cells first and the book's row last.
pub const WRITE_OPS: usize = 20_000;
pub const PATIENCE: Duration = Duration::from_secs(60);

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn parse_ms(v: &Value) -> i64 {
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis()).unwrap_or(0)
}

/// A new id: `b` and ten letters or digits.
pub fn new_id(prefix: &str) -> String {
    let mut b = [0u8; 10];
    let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b)).is_ok();
    if !ok {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t >> (i * 7)) as u8 ^ (std::process::id() >> i) as u8;
        }
    }
    const A: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    format!("{prefix}{}", b.iter().map(|x| A[*x as usize % 36] as char).collect::<String>())
}

pub fn cell_id(book: &str, k: &Key) -> String {
    format!("{book}:{}:{}:{}", k.sheet, k.row, k.col)
}

/// Makes the tables when they are not there yet.
pub async fn ensure_tables(strata: &Strata) -> Result<(), String> {
    let have: Vec<String> = strata.tables().await.map_err(|e| e.to_string())?.iter().filter_map(|t| t["table"].as_str().map(str::to_string)).collect();
    for (name, sql) in [(BOOKS, CREATE_BOOKS), (CELLS, CREATE_CELLS)] {
        if !have.iter().any(|t| t == name) {
            strata.sql(sql).await.map_err(|e| format!("making table {name}: {e}"))?;
        }
    }
    Ok(())
}

/// The workbooks, the last changed first.
pub async fn list(strata: &Strata) -> Result<Vec<Value>, String> {
    let j = strata
        .query(&format!("SELECT id, name, created, updated, source, sheets, cells, sheet_names FROM {BOOKS} ORDER BY updated DESC LIMIT 1000"), json!({}))
        .await
        .map_err(|e| e.to_string())?;
    let cols: Vec<String> = j["columns"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
    Ok(j["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let mut o = serde_json::Map::new();
            for (i, c) in cols.iter().enumerate() {
                o.insert(c.clone(), r[i].clone());
            }
            o.insert("updated_ms".into(), json!(parse_ms(&r[3])));
            o.insert("created_ms".into(), json!(parse_ms(&r[2])));
            Value::Object(o)
        })
        .collect())
}

/// Reads a workbook: its row, and its cells a page at a time.
pub async fn load(strata: &Strata, id: &str) -> Result<Book, String> {
    let j = strata.get(&format!("/v1/kv/{BOOKS}/{}", urlencode(id))).await.map_err(|e| if e.status == 404 { format!("no workbook {id}") } else { e.to_string() })?;
    let row = &j["row"];
    if j["found"] == false || row.is_null() {
        return Err(format!("no workbook {id}"));
    }
    let frame = base64::engine::general_purpose::STANDARD.decode(row["frame"].as_str().unwrap_or("")).map_err(|e| format!("the workbook's frame is not base64: {e}"))?;
    let mut wb = book::decode_frame(&frame)?;
    let ext: Ext = row["ext"].as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    let mut strings = Strings::default();
    let mut later: Vec<(u32, i32, i32, String)> = Vec::new();
    let mut problem: Option<String> = None;
    let sql = format!("SELECT sheet, row, col, kind, input, text, num, style, f, extra FROM {CELLS} WHERE book = :b");
    strata
        .scroll(&sql, json!({"b": id}), usize::MAX, |cols, rows| {
            let at = |name: &str| cols.iter().position(|c| c == name);
            let (si, ri, ci) = (at("sheet"), at("row"), at("col"));
            for r in rows {
                let obj: serde_json::Map<String, Value> = cols.iter().enumerate().map(|(i, c)| (c.clone(), r[i].clone())).collect();
                let sheet = si.and_then(|i| r[i].as_u64()).unwrap_or(0) as u32;
                let (row, col) = (ri.and_then(|i| r[i].as_i64()).unwrap_or(0) as i32, ci.and_then(|i| r[i].as_i64()).unwrap_or(0) as i32);
                let Some(ws) = wb.worksheets.iter_mut().find(|w| w.sheet_id == sheet) else {
                    problem.get_or_insert_with(|| format!("cells of a sheet {sheet} the workbook does not have"));
                    continue;
                };
                if row < 1 || col < 1 {
                    continue;
                }
                match book::restore(&Value::Object(obj), ws.shared_formulas.len(), &mut strings) {
                    Restored::Cell(c) => {
                        ws.sheet_data.set_cell(row, col, c);
                    }
                    Restored::Input(i) => later.push((sheet, row, col, i)),
                    Restored::Skip => {}
                }
            }
        })
        .await
        .map_err(|e| e.to_string())?;
    wb.shared_strings = strings.list;
    let mut engine = tokio::task::block_in_place(|| book::engine_of(wb))?;
    let created_ms = parse_ms(&row["created"]);
    let source = row["source"].as_str().unwrap_or("new").to_string();
    let mut b = Book::new(id.to_string(), &source, book::new_engine("x"), ext, created_ms);
    // What Strata holds, before anything is typed again or computed: a
    // later save writes what differs.
    {
        std::mem::swap(&mut b.engine, &mut engine);
        let s = b.changes();
        b.saved(s);
        if problem.is_some() {
            tracing::warn!("workbook {id}: {}", problem.unwrap_or_default());
        }
    }
    tokio::task::block_in_place(|| {
        if !later.is_empty() {
            b.engine.pause_evaluation();
            for (sheet, row, col, input) in &later {
                if let Some(si) = b.sheet_index(*sheet) {
                    let _ = b.engine.set_user_input(si, *row, *col, input);
                }
            }
            b.engine.resume_evaluation();
        }
        b.engine.evaluate();
    });
    Ok(b)
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect()
}

/// The book's row as written.
fn book_doc(b: &Book, frame: Option<&[u8]>, cells: usize) -> Value {
    let names = b.sheet_names();
    let mut doc = json!({
        "id": b.id, "name": b.name(), "created": iso(b.created_ms), "updated": iso(now_ms()),
        "source": b.source, "sheets": names.len(), "cells": cells,
        "sheet_names": names.join(" · "), "engine": book::ENGINE,
        "ext": serde_json::to_string(&b.ext).unwrap_or_default(),
    });
    if let Some(f) = frame {
        doc["frame"] = json!(base64::engine::general_purpose::STANDARD.encode(f));
    }
    doc
}

/// Writes a save: the cells that changed, the ones gone, and the book's row.
/// Returns the rows written.
pub async fn write(strata: &Strata, b: &Book, s: &Save) -> Result<usize, String> {
    let mut ops: Vec<Value> = Vec::with_capacity(s.cells.len() + s.deleted.len() + 1);
    for (k, sheet_name, c, _) in &s.cells {
        let mut doc = json!({
            "id": cell_id(&b.id, k), "book": b.id, "sheet": k.sheet, "sheet_name": sheet_name,
            "row": k.row, "col": k.col, "ref": book::cell_ref(k.row, k.col),
            "kind": c.kind, "input": c.input, "text": c.text, "style": c.style,
        });
        if let Some(n) = c.num.filter(|n| n.is_finite()) {
            doc["num"] = json!(n);
        }
        if let Some(f) = c.f {
            doc["f"] = json!(f);
        }
        if let Some(x) = &c.extra {
            doc["extra"] = json!(x);
        }
        ops.push(json!({"op": "upsert", "table": CELLS, "doc": doc}));
    }
    for k in &s.deleted {
        ops.push(json!({"op": "delete", "table": CELLS, "key": cell_id(&b.id, k), "routing": b.id}));
    }
    let head = s.head.is_some() || s.frame.is_some() || !ops.is_empty();
    if head {
        // The frame goes with every row of the book: a row upserted whole.
        let frame = match &s.frame {
            Some((f, _)) => f.clone(),
            None => book::frame(b.model()),
        };
        ops.push(json!({"op": "upsert", "table": BOOKS, "doc": book_doc(b, Some(&frame), s.cell_count)}));
    }
    if ops.is_empty() {
        return Ok(0);
    }
    let mut written = 0;
    let n = ops.len();
    let base = new_id("w");
    let mut i = 0;
    let mut part = 0;
    while i < n {
        let end = (i + WRITE_OPS).min(n);
        let chunk: Vec<Value> = ops[i..end].to_vec();
        written += strata.write(chunk, &format!("{base}-{part}"), PATIENCE).await.map_err(|e| e.to_string())?;
        i = end;
        part += 1;
    }
    Ok(written)
}

/// Deletes a workbook: its cells, then its row.
pub async fn delete(strata: &Strata, id: &str) -> Result<(), String> {
    strata.post("/v1/sql", &json!({"sql": format!("DELETE FROM {CELLS} WHERE book = :b"), "params": {"b": id}})).await.map_err(|e| e.to_string())?;
    strata.write(vec![json!({"op": "delete", "table": BOOKS, "key": id})], &new_id("d"), PATIENCE).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Cells of every workbook whose text matches, best first, and workbooks
/// whose name or sheets' names do.
pub async fn search(strata: &Strata, q: &str, limit: usize) -> Result<Value, String> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(json!({"cells": [], "books": []}));
    }
    let cells = strata
        .query(&format!("SELECT book, sheet, sheet_name, row, col, ref, text, input, score() FROM {CELLS} WHERE MATCH(text, :q) ORDER BY bm25(text, :q) DESC LIMIT {}", limit.clamp(1, 500)), json!({"q": q}))
        .await
        .map_err(|e| e.to_string())?;
    let books = strata
        .query(&format!("SELECT id, name, sheet_names, updated FROM {BOOKS} WHERE MATCH(name, :q) OR MATCH(sheet_names, :q) LIMIT 50"), json!({"q": q}))
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({"cells": cells, "books": books}))
}

/// A literal for the SQL of a sheet's query (as [`lit`]).
pub fn quote(s: &str) -> String {
    lit(s)
}
