//! Files in and out: xlsx (IronCalc reads and writes Excel's format), CSV,
//! and SQLite databases, each of whose tables becomes a sheet.

use std::io::Cursor;

use ironcalc::base::types::Workbook;
use ironcalc::base::{Model, UserModel};
use serde_json::{json, Value};

use crate::book::{engine_of, Engine, LANG};

/// Cells an import fills at most (a sheet holds the first rows of a table
/// that has more, and says so).
pub const MAX_IMPORT_CELLS: usize = 2_000_000;

/// The kind of a file, by its first bytes, or by its name.
pub fn sniff(bytes: &[u8], name: &str) -> &'static str {
    if bytes.starts_with(b"SQLite format 3\0") {
        return "sqlite";
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return "xlsx";
    }
    let lower = name.to_lowercase();
    if lower.ends_with(".tsv") || lower.ends_with(".tab") {
        return "tsv";
    }
    "csv"
}

/// A workbook from the bytes of an xlsx file.
pub fn from_xlsx(bytes: &[u8], name: &str) -> Result<Engine, String> {
    let wb: Workbook = ironcalc::import::load_from_xlsx_bytes(bytes, name, "en", "UTC").map_err(|e| format!("not an xlsx file IronCalc reads: {e}"))?;
    let mut e = engine_of(wb)?;
    e.set_name(name);
    Ok(e)
}

/// The workbook as an xlsx file.
pub fn to_xlsx(model: &Model) -> Result<Vec<u8>, String> {
    let w = ironcalc::export::save_xlsx_to_writer(model, Cursor::new(Vec::new())).map_err(|e| format!("writing xlsx: {e}"))?;
    Ok(w.into_inner())
}

fn empty_model() -> Model<'static> {
    Model::new_empty("Book", "en", "UTC", LANG).expect("an empty workbook")
}

/// A sheet's name from a file's or a table's: at most 31 characters, none
/// of `[]:*?/\`, and none another sheet has.
pub fn sheet_name(want: &str, taken: &[String]) -> String {
    let clean: String = want.chars().map(|c| if "[]:*?/\\".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_matches('\'');
    let base: String = if clean.is_empty() { "Sheet".into() } else { clean.chars().take(31).collect() };
    let mut name = base.clone();
    let mut i = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        let suffix = format!(" ({i})");
        name = format!("{}{suffix}", base.chars().take(31 - suffix.chars().count()).collect::<String>());
        i += 1;
    }
    name
}

/// A value read from a file, typed as it is (not parsed again).
pub enum Val {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
}

/// Fills sheet `s` of `m` from `rows`, its first row (the names) bold and
/// frozen. Returns the cells filled.
fn fill(m: &mut Model, s: u32, header: Option<&[String]>, rows: impl Iterator<Item = Vec<Val>>, budget: &mut usize) -> Result<(usize, bool), String> {
    let mut r = 1;
    let mut n = 0;
    if let Some(h) = header {
        for (j, name) in h.iter().enumerate() {
            let c = j as i32 + 1;
            m.update_cell_with_text(s, 1, c, name)?;
            let mut st = m.get_style_for_cell(s, 1, c)?;
            st.font.b = true;
            m.set_cell_style(s, 1, c, &st)?;
            let w = (name.chars().count() as f64 * 8.0 + 24.0).clamp(70.0, 220.0);
            m.set_column_width(s, c, w)?;
        }
        m.set_frozen_rows(s, 1)?;
        r = 2;
    }
    let mut cut = false;
    for row in rows {
        if *budget < row.len() {
            cut = true;
            break;
        }
        for (j, v) in row.iter().enumerate() {
            let c = j as i32 + 1;
            match v {
                Val::Empty => continue,
                Val::Num(x) if x.is_finite() => m.update_cell_with_number(s, r, c, *x)?,
                Val::Num(_) => continue,
                Val::Text(t) if t.is_empty() => continue,
                Val::Text(t) => m.update_cell_with_text(s, r, c, t)?,
                Val::Bool(b) => m.update_cell_with_bool(s, r, c, *b)?,
            }
            n += 1;
            *budget -= 1;
        }
        r += 1;
        if r > 1_048_576 {
            cut = true;
            break;
        }
    }
    Ok((n, cut))
}

/// A value of a CSV field: a number when it reads as one, else text.
fn csv_val(s: &str) -> Val {
    let t = s.trim();
    if t.is_empty() {
        return Val::Empty;
    }
    // Numbers as a spreadsheet would read them, but not codes with leading zeros.
    let leading_zero = t.len() > 1 && t.starts_with('0') && !t.starts_with("0.");
    if !leading_zero {
        if let Ok(x) = t.replace(',', "").parse::<f64>() {
            if !t.contains(',') || t.split('.').next().is_some_and(|i| i.split(',').skip(1).all(|g| g.len() == 3)) {
                return Val::Num(x);
            }
        }
    }
    match t.to_ascii_lowercase().as_str() {
        "true" => Val::Bool(true),
        "false" => Val::Bool(false),
        _ => Val::Text(s.to_string()),
    }
}

/// The delimiter of a CSV file: the one of `,;\t|` its first lines split by most evenly.
fn delimiter(text: &str) -> u8 {
    let lines: Vec<&str> = text.lines().take(20).collect();
    let mut best = (b',', 0usize);
    for d in [b',', b'\t', b';', b'|'] {
        let counts: Vec<usize> = lines.iter().map(|l| l.bytes().filter(|b| *b == d).count()).collect();
        let Some(&first) = counts.first() else { continue };
        if first == 0 {
            continue;
        }
        let even = counts.iter().filter(|c| **c == first).count();
        if even * first > best.1 {
            best = (d, even * first);
        }
    }
    best.0
}

/// A workbook of one sheet from a CSV (or tab-separated) file. Its first
/// row is taken as the names when `header` says so (by default, when it
/// holds only text and the next one does not).
pub fn from_csv(bytes: &[u8], name: &str, header: Option<bool>) -> Result<(Engine, Value), String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let d = if sniff(bytes, name) == "tsv" { b'\t' } else { delimiter(text) };
    let mut rd = csv::ReaderBuilder::new().delimiter(d).has_headers(false).flexible(true).from_reader(text.as_bytes());
    let mut rows: Vec<Vec<String>> = Vec::new();
    for rec in rd.records() {
        let rec = rec.map_err(|e| format!("CSV: {e}"))?;
        rows.push(rec.iter().map(str::to_string).collect());
    }
    let header = header.unwrap_or_else(|| {
        rows.len() > 1 && rows[0].iter().all(|v| matches!(csv_val(v), Val::Text(_))) && rows[1].iter().any(|v| !matches!(csv_val(v), Val::Text(_) | Val::Empty))
    });
    let mut m = empty_model();
    let stem = name.rsplit_once('.').map(|(a, _)| a).unwrap_or(name);
    m.rename_sheet_by_index(0, &sheet_name(stem, &[]))?;
    let mut budget = MAX_IMPORT_CELLS;
    let (head, body) = if header && !rows.is_empty() { (Some(rows.remove(0)), rows) } else { (None, rows) };
    let n_rows = body.len();
    let (cells, cut) = fill(&mut m, 0, head.as_deref(), body.into_iter().map(|r| r.iter().map(|v| csv_val(v)).collect()), &mut budget)?;
    m.workbook.name = stem.to_string();
    m.evaluate();
    let report = json!({"format": "csv", "delimiter": (d as char).to_string(), "header": header, "rows": n_rows, "cells": cells, "truncated": cut});
    Ok((UserModel::from_model(m), report))
}

/// A sheet as CSV: the values as shown.
pub fn to_csv(model: &Model, sheet: u32) -> Result<String, String> {
    let ws = model.workbook.worksheet(sheet)?;
    let mut w = csv::Writer::from_writer(Vec::new());
    if !ws.sheet_data.is_empty() {
        let d = ws.dimension();
        for r in 1..=d.max_row {
            let rec: Vec<String> = (1..=d.max_column).map(|c| model.get_formatted_cell_value(sheet, r, c).unwrap_or_default()).collect();
            w.write_record(&rec).map_err(|e| e.to_string())?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// The tables of a SQLite database: name, columns and rows.
pub fn sqlite_tables(conn: &rusqlite::Connection) -> Result<Vec<Value>, String> {
    let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY type, name").map_err(|e| e.to_string())?;
    let names: Vec<String> = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
    let mut out = Vec::new();
    for t in names {
        let q = crate::strata::ident(&t);
        let cols: Vec<Value> = match conn.prepare(&format!("PRAGMA table_info({q})")) {
            Ok(mut s) => s.query_map([], |r| Ok(json!({"name": r.get::<_, String>(1)?, "type": r.get::<_, String>(2).unwrap_or_default()}))).map_err(|e| e.to_string())?.filter_map(Result::ok).collect(),
            Err(e) => return Err(e.to_string()),
        };
        let rows: i64 = conn.query_row(&format!("SELECT count(*) FROM {q}"), [], |r| r.get(0)).unwrap_or(0);
        out.push(json!({"name": t, "columns": cols, "rows": rows}));
    }
    Ok(out)
}

/// Opens SQLite bytes (written to a temporary file, read only).
pub fn open_sqlite(bytes: &[u8]) -> Result<(rusqlite::Connection, tempfile::NamedTempFile), String> {
    let f = tempfile::Builder::new().prefix("strata-sheets-").suffix(".sqlite").tempfile().map_err(|e| e.to_string())?;
    std::fs::write(f.path(), bytes).map_err(|e| e.to_string())?;
    let conn = rusqlite::Connection::open_with_flags(f.path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| format!("not a SQLite database: {e}"))?;
    Ok((conn, f))
}

/// A workbook of a SQLite database: a sheet for each table (those of
/// `only`, if given), its columns' names in the first row.
pub fn from_sqlite(bytes: &[u8], name: &str, only: Option<&[String]>) -> Result<(Engine, Value), String> {
    let (conn, _file) = open_sqlite(bytes)?;
    let tables = sqlite_tables(&conn)?;
    let mut m = empty_model();
    let mut budget = MAX_IMPORT_CELLS;
    let mut taken: Vec<String> = Vec::new();
    let mut report = Vec::new();
    for t in &tables {
        let tname = t["name"].as_str().unwrap_or_default().to_string();
        if only.is_some_and(|o| !o.iter().any(|x| x == &tname)) {
            continue;
        }
        let sname = sheet_name(&tname, &taken);
        let s = if taken.is_empty() {
            m.rename_sheet_by_index(0, &sname)?;
            0
        } else {
            m.add_sheet(&sname)?;
            taken.len() as u32
        };
        taken.push(sname.clone());
        let q = crate::strata::ident(&tname);
        let mut st = conn.prepare(&format!("SELECT * FROM {q}")).map_err(|e| e.to_string())?;
        let head: Vec<String> = st.column_names().into_iter().map(str::to_string).collect();
        let width = head.len();
        let mut rows = st.query([]).map_err(|e| e.to_string())?;
        let mut vals: Vec<Vec<Val>> = Vec::new();
        let mut more = false;
        let room = budget / width.max(1);
        while let Some(r) = rows.next().map_err(|e| e.to_string())? {
            if vals.len() >= room {
                more = true;
                break;
            }
            let mut line = Vec::with_capacity(width);
            for i in 0..width {
                use rusqlite::types::ValueRef;
                line.push(match r.get_ref(i).map_err(|e| e.to_string())? {
                    ValueRef::Null => Val::Empty,
                    ValueRef::Integer(x) => Val::Num(x as f64),
                    ValueRef::Real(x) => Val::Num(x),
                    ValueRef::Text(t) => Val::Text(String::from_utf8_lossy(t).into_owned()),
                    ValueRef::Blob(b) => Val::Text(format!("({} bytes)", b.len())),
                });
            }
            vals.push(line);
        }
        let n = vals.len();
        let (cells, cut) = fill(&mut m, s, Some(&head), vals.into_iter(), &mut budget)?;
        report.push(json!({"table": tname, "sheet": sname, "rows": n, "of": t["rows"], "cells": cells, "truncated": cut || more}));
    }
    if taken.is_empty() {
        return Err(if tables.is_empty() { "the database has no tables".into() } else { "none of those tables is in the database".into() });
    }
    let stem = name.rsplit_once('.').map(|(a, _)| a).unwrap_or(name);
    m.workbook.name = stem.to_string();
    m.evaluate();
    Ok((UserModel::from_model(m), json!({"format": "sqlite", "tables": report})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_values_and_delimiters() {
        assert!(matches!(csv_val("12.5"), Val::Num(x) if x == 12.5));
        assert!(matches!(csv_val("1,234"), Val::Num(x) if x == 1234.0));
        assert!(matches!(csv_val("007"), Val::Text(_)));
        assert!(matches!(csv_val("1,2"), Val::Text(_)));
        assert!(matches!(csv_val("TRUE"), Val::Bool(true)));
        assert_eq!(delimiter("a;b;c\n1;2;3\n"), b';');
        assert_eq!(delimiter("a,b\n1,2\n"), b',');
        let (e, r) = from_csv(b"name,qty,price\npear,3,1.5\n\"fig, dried\",10,0.25\n", "fruit.csv", None).unwrap();
        assert_eq!(r["header"], true);
        assert_eq!(e.get_formatted_cell_value(0, 3, 1).unwrap(), "fig, dried");
        assert_eq!(e.get_formatted_cell_value(0, 2, 2).unwrap(), "3");
        assert_eq!(e.get_worksheets_properties()[0].name, "fruit");
        let csv = to_csv(e.get_model(), 0).unwrap();
        assert!(csv.starts_with("name,qty,price\npear,3,1.5\n\"fig, dried\""), "{csv}");
    }

    #[test]
    fn sheet_names_are_valid_and_distinct() {
        let taken = vec!["Sales".to_string()];
        assert_eq!(sheet_name("sales", &taken), "sales (2)");
        assert_eq!(sheet_name("a/b:c", &[]), "a_b_c");
        assert_eq!(sheet_name(&"x".repeat(40), &[]).chars().count(), 31);
    }

    #[test]
    fn sqlite_tables_become_sheets() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("shop.db");
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("CREATE TABLE orders (id INTEGER PRIMARY KEY, item TEXT, qty INTEGER, price REAL, note BLOB); INSERT INTO orders VALUES (1, 'pear', 3, 1.5, x'00ff'), (2, '007', NULL, 2.25, NULL); CREATE TABLE people (name TEXT);").unwrap();
        drop(c);
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(sniff(&bytes, "x"), "sqlite");
        let (e, r) = from_sqlite(&bytes, "shop.db", None).unwrap();
        let props = e.get_worksheets_properties();
        assert_eq!(props.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["orders", "people"]);
        assert_eq!(e.get_formatted_cell_value(0, 1, 2).unwrap(), "item");
        assert_eq!(e.get_formatted_cell_value(0, 3, 2).unwrap(), "007");
        assert_eq!(e.get_formatted_cell_value(0, 2, 4).unwrap(), "1.5");
        assert_eq!(e.get_formatted_cell_value(0, 2, 5).unwrap(), "(2 bytes)");
        assert_eq!(r["tables"][0]["rows"], 2);
        let x = to_xlsx(e.get_model()).unwrap();
        let back = from_xlsx(&x, "shop").unwrap();
        assert_eq!(back.get_formatted_cell_value(0, 3, 2).unwrap(), "007");
        assert_eq!(back.get_formatted_cell_value(0, 2, 3).unwrap(), "3");
    }
}
