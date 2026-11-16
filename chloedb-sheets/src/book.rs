//! A workbook open in memory: IronCalc's model of it, what of it Strata
//! holds already, and the cells as the command center draws them.
//!
//! A workbook is kept in Strata as rows of two tables ([`crate::store`]):
//! one row in `sheets_books` with its *frame* (the workbook without its
//! cells: sheets, styles, column widths, merges, names, conditional formats,
//! as IronCalc encodes them) and one row in `sheets_cells` for each cell that
//! holds something (a value, a formula, a style), with the value as shown,
//! as a number, and as typed. After every change the model's cells are
//! fingerprinted and compared with what was saved: the cells that differ are
//! written, those gone are deleted, in one write ([`Book::changes`]).

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hasher;

use ironcalc::base::cell::CellValue;
use ironcalc::base::expressions::token::{get_error_by_english_name, Error as CellError};
use ironcalc::base::sheet_data::SheetData;
use ironcalc::base::types::{ArrayKind, Cell, Color, FormulaValue, SpillValue, Style, Workbook, Worksheet};
use ironcalc::base::{Model, UserModel};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// IronCalc's language for formulas and errors (function names in English).
pub const LANG: &str = "en";
/// What wrote a workbook's frame: another version may not read it, and
/// rebuilds the cells from their inputs instead.
pub const ENGINE: &str = "ironcalc 0.9.1";

pub type Engine = UserModel<'static>;

/// A cell as shown, the one formatter of the grid, CSV, search, filters
/// and the values charts read: IronCalc's formats, except a number in the
/// General format, which IronCalc writes with nine fixed decimals
/// (`226159459.830000013`, `4.127118644`); here it is [`general`].
pub trait Shown {
    fn shown(&self, sheet: u32, row: i32, col: i32) -> Result<String, String>;
}

impl Shown for Model<'_> {
    fn shown(&self, sheet: u32, row: i32, col: i32) -> Result<String, String> {
        if let Ok(CellValue::Number(v)) = self.get_cell_value_by_index(sheet, row, col) {
            if self.get_style_for_cell(sheet, row, col)?.num_fmt.eq_ignore_ascii_case("general") {
                return Ok(general(v));
            }
        }
        self.get_formatted_cell_value(sheet, row, col)
    }
}

impl Shown for UserModel<'_> {
    fn shown(&self, sheet: u32, row: i32, col: i32) -> Result<String, String> {
        self.get_model().shown(sheet, row, col)
    }
}

/// A number in the General format, as Excel shows it: rounded to 15
/// significant digits (what a double holds for sure), then the shortest
/// text that reads back as that (`226159459.83`, `4.12711864406780`
/// as `4.1271186440678`); an exponent past 10^15 and under 10^-9.
pub fn general(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() { "#NUM!".into() } else { "#DIV/0!".into() };
    }
    if v == 0.0 {
        return "0".into();
    }
    let r: f64 = format!("{v:.14e}").parse().unwrap_or(v);
    if (1e-9..1e15).contains(&r.abs()) {
        return format!("{r}");
    }
    let e = format!("{r:e}");
    match e.split_once('e') {
        Some((m, x)) if x.starts_with('-') => format!("{m}E-{:02}", x[1..].parse::<i32>().unwrap_or(0)),
        Some((m, x)) => format!("{m}E+{:02}", x.parse::<i32>().unwrap_or(0)),
        None => e,
    }
}

/// A new workbook of one empty sheet.
pub fn new_engine(name: &str) -> Engine {
    let mut m = Model::new_empty("Book", "en", "UTC", LANG).expect("an empty workbook");
    m.workbook.name = name.to_string();
    UserModel::from_model(m)
}

/// A model from a workbook (an import, or a frame filled with its cells).
pub fn engine_of(wb: Workbook) -> Result<Engine, String> {
    let model = Model::from_workbook(wb, LANG)?;
    Ok(UserModel::from_model(model))
}

/// What to type for a cell to hold `s` as text: as it is when a
/// spreadsheet reads it as text anyway, and after a `'` when it would read
/// it as something else (a number, a date, a formula, TRUE).
pub fn as_text(s: &str) -> String {
    let t = s.trim();
    let first = t.chars().next();
    let risky = match first {
        None => false,
        Some(c) => c.is_ascii_digit() || "=+-'#$.%(".contains(c) || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false"),
    };
    if risky || (s != t && !s.is_empty()) {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

/// A cell of a sheet, by the sheet's id (stable while sheets move).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub sheet: u32,
    pub row: i32,
    pub col: i32,
}

/// The column's letters: 1 is A, 27 is AA.
pub fn col_name(mut c: i32) -> String {
    let mut s = Vec::new();
    while c > 0 {
        let r = ((c - 1) % 26) as u8;
        s.push(b'A' + r);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

pub fn cell_ref(row: i32, col: i32) -> String {
    format!("{}{}", col_name(col), row)
}

/// The workbook without its cells and their strings: what `sheets_books.frame` holds.
pub fn frame(model: &Model) -> Vec<u8> {
    let wb = &model.workbook;
    let worksheets = wb
        .worksheets
        .iter()
        .map(|w| Worksheet {
            dimension: w.dimension.clone(),
            cols: w.cols.clone(),
            rows: w.rows.clone(),
            name: w.name.clone(),
            sheet_data: SheetData::new(),
            shared_formulas: w.shared_formulas.clone(),
            sheet_id: w.sheet_id,
            state: w.state.clone(),
            color: w.color.clone(),
            merged_cells: w.merged_cells.clone(),
            comments: w.comments.clone(),
            frozen_rows: w.frozen_rows,
            frozen_columns: w.frozen_columns,
            show_grid_lines: w.show_grid_lines,
            conditional_formatting: w.conditional_formatting.clone(),
            links: w.links.clone(),
        })
        .collect();
    let f = Workbook {
        shared_strings: Vec::new(),
        defined_names: wb.defined_names.clone(),
        worksheets,
        styles: wb.styles.clone(),
        name: wb.name.clone(),
        settings: wb.settings.clone(),
        metadata: wb.metadata.clone(),
        tables: wb.tables.clone(),
        theme: wb.theme.clone(),
    };
    bitcode::encode(&f)
}

pub fn decode_frame(bytes: &[u8]) -> Result<Workbook, String> {
    bitcode::decode(bytes).map_err(|e| format!("the workbook's frame does not decode ({e}): it was written by another version"))
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    h.write(b);
    h.finish()
}

fn put_value(h: &mut std::hash::DefaultHasher, v: &FormulaValue) {
    match v {
        FormulaValue::Unevaluated => h.write_u8(0),
        FormulaValue::Boolean(b) => {
            h.write_u8(1);
            h.write_u8(*b as u8)
        }
        FormulaValue::Number(n) => {
            h.write_u8(2);
            h.write_u64(n.to_bits())
        }
        FormulaValue::Text(t) => {
            h.write_u8(3);
            h.write(t.as_bytes())
        }
        FormulaValue::Error { ei, .. } => {
            h.write_u8(4);
            h.write(ei.to_string().as_bytes())
        }
    }
}

fn put_spill(h: &mut std::hash::DefaultHasher, v: &SpillValue) {
    match v {
        SpillValue::Boolean(b) => {
            h.write_u8(1);
            h.write_u8(*b as u8)
        }
        SpillValue::Number(n) => {
            h.write_u8(2);
            h.write_u64(n.to_bits())
        }
        SpillValue::Text(t) => {
            h.write_u8(3);
            h.write(t.as_bytes())
        }
        SpillValue::Error(e) => {
            h.write_u8(4);
            h.write(e.to_string().as_bytes())
        }
    }
}

/// What a cell is, to compare with what was saved: its value, style and
/// formula, and the name of its sheet (a row keeps it).
pub fn fingerprint(cell: &Cell, formulas: &[String], strings: &[String], sheet_name: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    h.write(sheet_name.as_bytes());
    h.write_u8(0xff);
    match cell {
        Cell::EmptyCell { s } => {
            h.write_u8(0);
            h.write_i32(*s);
        }
        Cell::BooleanCell { v, s } => {
            h.write_u8(1);
            h.write_u8(*v as u8);
            h.write_i32(*s);
        }
        Cell::NumberCell { v, s } => {
            h.write_u8(2);
            h.write_u64(v.to_bits());
            h.write_i32(*s);
        }
        Cell::ErrorCell { ei, s } => {
            h.write_u8(3);
            h.write(ei.to_string().as_bytes());
            h.write_i32(*s);
        }
        Cell::SharedString { si, s } => {
            h.write_u8(4);
            h.write(strings.get(*si as usize).map(|x| x.as_bytes()).unwrap_or(b""));
            h.write_i32(*s);
        }
        Cell::CellFormula { f, s, v } => {
            h.write_u8(5);
            h.write(formulas.get(*f as usize).map(|x| x.as_bytes()).unwrap_or(b""));
            h.write_i32(*f);
            h.write_i32(*s);
            put_value(&mut h, v);
        }
        Cell::ArrayFormula { f, s, r, kind, v } => {
            h.write_u8(6);
            h.write(formulas.get(*f as usize).map(|x| x.as_bytes()).unwrap_or(b""));
            h.write_i32(*f);
            h.write_i32(*s);
            h.write_i32(r.0);
            h.write_i32(r.1);
            h.write_u8(matches!(kind, ArrayKind::Dynamic) as u8);
            put_value(&mut h, v);
        }
        Cell::SpillCell { s, a, v } => {
            h.write_u8(7);
            h.write_i32(*s);
            h.write_i32(a.0);
            h.write_i32(a.1);
            put_spill(&mut h, v);
        }
    }
    h.finish()
}

/// A cell as `sheets_cells` holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub kind: &'static str,
    /// As shown, formatted.
    pub text: String,
    /// The value as a number (a number, a date, a boolean as 1 or 0).
    pub num: Option<f64>,
    /// What was typed: the formula, the text, the number as the formula bar shows it.
    pub input: String,
    pub style: i32,
    /// The formula's index in its sheet's shared formulas (in the frame).
    pub f: Option<i32>,
    /// What else the kind needs: a formula's value type (n, s, b, e), an
    /// array's size and kind, a spill cell's anchor.
    pub extra: Option<String>,
}

fn value_type(v: &FormulaValue) -> (&'static str, Option<f64>) {
    match v {
        FormulaValue::Number(n) => ("n", Some(*n)),
        FormulaValue::Boolean(b) => ("b", Some(*b as u8 as f64)),
        FormulaValue::Text(_) => ("s", None),
        FormulaValue::Error { .. } => ("e", None),
        FormulaValue::Unevaluated => ("u", None),
    }
}

fn spill_type(v: &SpillValue) -> (&'static str, Option<f64>) {
    match v {
        SpillValue::Number(n) => ("n", Some(*n)),
        SpillValue::Boolean(b) => ("b", Some(*b as u8 as f64)),
        SpillValue::Text(_) => ("s", None),
        SpillValue::Error(_) => ("e", None),
    }
}

/// The row of one cell of sheet (by index) `sheet`.
pub fn stored(model: &Model, sheet: u32, row: i32, col: i32, cell: &Cell) -> Stored {
    let wb = &model.workbook;
    let text = model.shown(sheet, row, col).unwrap_or_default();
    let content = || model.get_localized_cell_content(sheet, row, col).unwrap_or_default();
    let formula = || model.get_cell_formula(sheet, row, col).ok().flatten().unwrap_or_default();
    match cell {
        Cell::EmptyCell { s } => Stored { kind: "empty", text, num: None, input: String::new(), style: *s, f: None, extra: None },
        Cell::BooleanCell { v, s } => Stored { kind: "bool", text, num: Some(*v as u8 as f64), input: content(), style: *s, f: None, extra: None },
        Cell::NumberCell { v, s } => Stored { kind: "number", text, num: Some(*v), input: content(), style: *s, f: None, extra: None },
        Cell::ErrorCell { ei, s } => Stored { kind: "error", text, num: None, input: ei.to_string(), style: *s, f: None, extra: None },
        Cell::SharedString { si, s } => Stored { kind: "text", text, num: None, input: wb.shared_strings.get(*si as usize).cloned().unwrap_or_default(), style: *s, f: None, extra: None },
        Cell::CellFormula { f, s, v } => {
            let (t, num) = value_type(v);
            Stored { kind: "formula", text, num, input: formula(), style: *s, f: Some(*f), extra: Some(t.to_string()) }
        }
        Cell::ArrayFormula { f, s, r, kind, v } => {
            let (t, num) = value_type(v);
            let k = if matches!(kind, ArrayKind::Dynamic) { "dynamic" } else { "cse" };
            Stored { kind: "array", text, num, input: formula(), style: *s, f: Some(*f), extra: Some(format!("{t},{},{},{k}", r.0, r.1)) }
        }
        Cell::SpillCell { s, a, v } => {
            let (t, num) = spill_type(v);
            Stored { kind: "spill", text, num, input: String::new(), style: *s, f: None, extra: Some(format!("{t},{},{}", a.0, a.1)) }
        }
    }
}

/// A saved cell read back into a sheet: the cell, or (when its formula is
/// not in the frame) the input to type into it once the model is made.
pub enum Restored {
    Cell(Cell),
    Input(String),
    Skip,
}

/// Interns a sheet's strings into the workbook's shared strings.
#[derive(Default)]
pub struct Strings {
    pub list: Vec<String>,
    index: HashMap<String, i32>,
}

impl Strings {
    pub fn intern(&mut self, s: &str) -> i32 {
        if let Some(i) = self.index.get(s) {
            return *i;
        }
        let i = self.list.len() as i32;
        self.list.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
}

/// A row of `sheets_cells` (as `{kind, input, text, num, style, f, extra}`)
/// as the cell it was, in a sheet with `formulas` shared formulas.
pub fn restore(row: &Value, formulas: usize, strings: &mut Strings) -> Restored {
    let s = row["style"].as_i64().unwrap_or(0) as i32;
    let num = row["num"].as_f64();
    let input = row["input"].as_str().unwrap_or("");
    let text = row["text"].as_str().unwrap_or("");
    let extra = row["extra"].as_str().unwrap_or("");
    let f = row["f"].as_i64().map(|f| f as i32);
    let cached = |t: &str| -> FormulaValue {
        match t {
            "n" => num.map(FormulaValue::Number).unwrap_or(FormulaValue::Unevaluated),
            "b" => FormulaValue::Boolean(num.unwrap_or(0.0) != 0.0),
            "s" => FormulaValue::Text(text.to_string()),
            _ => FormulaValue::Unevaluated,
        }
    };
    match row["kind"].as_str().unwrap_or("") {
        "empty" => Restored::Cell(Cell::EmptyCell { s }),
        "bool" => Restored::Cell(Cell::BooleanCell { v: num.unwrap_or(0.0) != 0.0, s }),
        "number" => match num {
            Some(v) => Restored::Cell(Cell::NumberCell { v, s }),
            None => Restored::Input(input.to_string()),
        },
        "error" => match get_error_by_english_name(input) {
            Some(ei) => Restored::Cell(Cell::ErrorCell { ei, s }),
            None => Restored::Cell(Cell::ErrorCell { ei: CellError::ERROR, s }),
        },
        "text" => Restored::Cell(Cell::SharedString { si: strings.intern(input), s }),
        "formula" => match f {
            Some(f) if (f as usize) < formulas => Restored::Cell(Cell::CellFormula { f, s, v: cached(extra) }),
            _ => Restored::Input(input.to_string()),
        },
        "array" => {
            let parts: Vec<&str> = extra.split(',').collect();
            let (w, h) = (parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(1), parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(1));
            let kind = if parts.get(3) == Some(&"cse") { ArrayKind::Cse } else { ArrayKind::Dynamic };
            match f {
                Some(f) if (f as usize) < formulas => Restored::Cell(Cell::ArrayFormula { f, s, r: (w, h), kind, v: cached(parts.first().copied().unwrap_or("")) }),
                _ => Restored::Input(input.to_string()),
            }
        }
        // Spill cells are their anchor's to write, at the next evaluation.
        _ => Restored::Skip,
    }
}

// ---------------------------------------------------------------- extensions

/// What a workbook holds beside IronCalc's model: charts, and the SQL
/// queries whose answers fill a block of cells.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Ext {
    #[serde(default)]
    pub charts: Vec<Chart>,
    #[serde(default)]
    pub queries: Vec<Query>,
    /// The filter of each sheet that has one.
    #[serde(default)]
    pub filters: Vec<SheetFilter>,
    #[serde(default)]
    pub next: u64,
    /// The key that made it (its name), which may change it
    /// ([`crate::caller`]); `None` for a workbook from before owners.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Whether keys limited to tables other than its owner's may open it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

/// A sheet's filter: its area (the first row names the columns) and what
/// each filtered column shows.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SheetFilter {
    /// The sheet's id.
    pub sheet: u32,
    pub row: i32,
    pub col: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub criteria: Vec<crate::ops::Crit>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Chart {
    pub id: String,
    /// The sheet's id.
    pub sheet: u32,
    /// The data, as A1:D20 (first row: series names, first column: categories).
    pub range: String,
    /// bar, column, line, area, pie, scatter.
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// Where it is drawn: its top left cell, and its size in pixels.
    pub row: i32,
    pub col: i32,
    #[serde(default = "chart_w")]
    pub width: f64,
    #[serde(default = "chart_h")]
    pub height: f64,
    #[serde(default)]
    pub stacked: bool,
}

fn chart_w() -> f64 {
    480.0
}
fn chart_h() -> f64 {
    300.0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Query {
    pub id: String,
    pub sheet: u32,
    /// Its top left cell: the column names, then the rows.
    pub row: i32,
    pub col: i32,
    pub sql: String,
    /// The block it filled last time: rows (with the names) and columns.
    #[serde(default)]
    pub rows: i32,
    #[serde(default)]
    pub cols: i32,
    #[serde(default)]
    pub refreshed_ms: i64,
    #[serde(default)]
    pub took_ms: f64,
    #[serde(default)]
    pub error: Option<String>,
}

// ---------------------------------------------------------------- the book

/// One change, for the other viewers of the book: the cells it touched, or
/// `None` when its layout changed (they draw the sheet again).
#[derive(Clone, Debug)]
pub struct Change {
    pub version: u64,
    pub cells: Option<Vec<Key>>,
}

pub struct Book {
    pub id: String,
    pub source: String,
    pub created_ms: i64,
    pub engine: Engine,
    pub ext: Ext,
    /// What Strata holds: each saved cell's fingerprint.
    pub saved: HashMap<Key, u64>,
    pub saved_frame: u64,
    pub saved_head: u64,
    /// Bumped by every change; viewers ask for the changes since theirs.
    pub version: u64,
    pub log: VecDeque<Change>,
    /// The last save failed (Strata did not answer): the changes are kept
    /// in memory and saved with the next one.
    pub unsaved: Option<String>,
    pub last_used: std::time::Instant,
}

/// What a save writes: rows to upsert (cells, then the book), keys to delete,
/// and the fingerprints to keep once it is written.
pub struct Save {
    pub cells: Vec<(Key, String, Stored, u64)>,
    pub deleted: Vec<Key>,
    pub frame: Option<(Vec<u8>, u64)>,
    pub head: Option<u64>,
    /// Every cell's key and fingerprint now.
    pub now: HashMap<Key, u64>,
    pub layout_changed: bool,
    pub cell_count: usize,
}

impl Save {
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.deleted.is_empty() && self.frame.is_none() && self.head.is_none()
    }
}

impl Book {
    pub fn new(id: String, source: &str, engine: Engine, ext: Ext, created_ms: i64) -> Book {
        Book { id, source: source.to_string(), created_ms, engine, ext, saved: HashMap::new(), saved_frame: 0, saved_head: 0, version: 1, log: VecDeque::new(), unsaved: None, last_used: std::time::Instant::now() }
    }

    pub fn model(&self) -> &Model<'_> {
        self.engine.get_model()
    }

    pub fn name(&self) -> String {
        self.engine.get_name()
    }

    /// The fingerprint of what the book row holds beside the frame.
    pub fn head_hash(&self) -> u64 {
        let j = json!({"name": self.name(), "ext": self.ext, "source": self.source, "sheets": self.sheet_names()});
        hash_bytes(j.to_string().as_bytes())
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.model().workbook.worksheets.iter().map(|w| w.name.clone()).collect()
    }

    /// The index of the sheet with id `id`.
    pub fn sheet_index(&self, id: u32) -> Option<u32> {
        self.model().workbook.worksheets.iter().position(|w| w.sheet_id == id).map(|i| i as u32)
    }

    pub fn sheet_id(&self, index: u32) -> Option<u32> {
        self.model().workbook.worksheets.get(index as usize).map(|w| w.sheet_id)
    }

    /// What differs from what Strata holds.
    pub fn changes(&self) -> Save {
        let model = self.model();
        let wb = &model.workbook;
        let mut now = HashMap::with_capacity(self.saved.len() + 16);
        let mut cells = Vec::new();
        for (si, ws) in wb.worksheets.iter().enumerate() {
            for (row, col, cell) in ws.sheet_data.cells() {
                // An empty cell of the default style is no cell at all.
                if matches!(&*cell, Cell::EmptyCell { s: 0 }) {
                    continue;
                }
                let key = Key { sheet: ws.sheet_id, row, col };
                let fp = fingerprint(&cell, &ws.shared_formulas, &wb.shared_strings, &ws.name);
                now.insert(key, fp);
                if self.saved.get(&key) != Some(&fp) {
                    cells.push((key, ws.name.clone(), stored(model, si as u32, row, col, &cell), fp));
                }
            }
        }
        let mut deleted: Vec<Key> = self.saved.keys().filter(|k| !now.contains_key(k)).copied().collect();
        deleted.sort();
        let fr = frame(model);
        let fh = hash_bytes(&fr);
        let hh = self.head_hash();
        let layout_changed = fh != self.saved_frame;
        Save {
            cell_count: now.len(),
            frame: (fh != self.saved_frame).then_some((fr, fh)),
            head: (hh != self.saved_head || fh != self.saved_frame).then_some(hh),
            cells,
            deleted,
            now,
            layout_changed,
        }
    }

    /// Takes a save as written.
    pub fn saved(&mut self, s: Save) {
        if let Some((_, fh)) = &s.frame {
            self.saved_frame = *fh;
        }
        if let Some(hh) = s.head {
            self.saved_head = hh;
        }
        self.saved = s.now;
        self.unsaved = None;
    }

    /// Notes a change for the other viewers.
    pub fn note(&mut self, cells: Option<Vec<Key>>) {
        self.version += 1;
        self.log.push_back(Change { version: self.version, cells });
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    // ------------------------------------------------------------ for the UI

    /// The workbook as the view lists it: its sheets, names, queries, charts.
    pub fn summary(&self) -> Value {
        let model = self.model();
        let sheets: Vec<Value> = model
            .workbook
            .worksheets
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let d = w.dimension();
                let empty = w.sheet_data.is_empty();
                json!({
                    "index": i, "id": w.sheet_id, "name": w.name,
                    "state": format!("{:?}", w.state).to_lowercase(),
                    "color": color_hex(&self.engine, &w.color),
                    "rows": if empty { 0 } else { d.max_row }, "cols": if empty { 0 } else { d.max_column },
                    "frozen_rows": w.frozen_rows, "frozen_cols": w.frozen_columns,
                    "grid_lines": w.show_grid_lines,
                    "layout": layout_hash(w),
                    "cf": w.conditional_formatting.len(),
                })
            })
            .collect();
        let names: Vec<Value> = self.engine.get_defined_name_list().into_iter().map(|(name, scope, formula)| json!({"name": name, "scope": scope, "formula": formula})).collect();
        json!({
            "id": self.id, "name": self.name(), "source": self.source, "created_ms": self.created_ms,
            "version": self.version, "sheets": sheets, "names": names,
            "charts": self.ext.charts, "queries": self.ext.queries, "filters": self.ext.filters,
            "owner": self.ext.owner, "shared": self.ext.shared,
            "can_undo": self.engine.can_undo(), "can_redo": self.engine.can_redo(),
            "unsaved": self.unsaved,
        })
    }

    /// A sheet's layout: column widths and styles, row heights, hidden rows
    /// and columns, merged cells, frozen panes, conditional formats.
    pub fn layout(&self, sheet: u32) -> Result<Value, String> {
        let model = self.model();
        let ws = model.workbook.worksheet(sheet)?;
        let mut styles = StyleMap::default();
        let cols: Vec<Value> = ws
            .cols
            .iter()
            .map(|c| {
                let s = c.style.map(|s| styles.of_column(&self.engine, sheet, c.min, s));
                json!({"min": c.min, "max": c.max, "width": if c.custom_width { c.width * ironcalc::base::COLUMN_WIDTH_FACTOR } else { 90.0 }, "hidden": c.hidden, "style": s})
            })
            .collect();
        let rows: Vec<Value> = ws
            .rows
            .iter()
            .filter(|r| r.custom_height || r.hidden || r.custom_format)
            .map(|r| {
                let s = r.custom_format.then(|| styles.of_row(&self.engine, sheet, r.r, r.s));
                json!([r.r, if r.custom_height { r.height * ironcalc::base::ROW_HEIGHT_FACTOR } else { 0.0 }, r.hidden, s])
            })
            .collect();
        let merges: Vec<Value> = ws.merged_cells.iter().map(|m| json!([m.row, m.column, m.width, m.height])).collect();
        let cf = self.engine.get_conditional_formatting_list(sheet).ok().map(|l| serde_json::to_value(l).unwrap_or(Value::Null)).unwrap_or(Value::Null);
        let d = ws.dimension();
        let empty = ws.sheet_data.is_empty();
        Ok(json!({
            "sheet": sheet, "id": ws.sheet_id, "name": ws.name,
            "rows_used": if empty { 0 } else { d.max_row }, "cols_used": if empty { 0 } else { d.max_column },
            "default_width": 90.0, "default_height": 25.0,
            "cols": cols, "rows": rows, "merges": merges,
            "frozen_rows": ws.frozen_rows, "frozen_cols": ws.frozen_columns, "grid_lines": ws.show_grid_lines,
            "cf": cf, "styles": styles.out, "layout": layout_hash(ws),
        }))
    }

    /// The cells of rows `r0..=r1`, columns `c0..=c1` of sheet `sheet`: each
    /// `[row, col, shown, type, style, input, extra]`, with the styles they
    /// use. `type` is n (number), s (text), b (boolean), e (error), with an f
    /// after it for a formula (nf, sf ...) and x for a spilled value; `input`
    /// is what the formula bar shows when it is not what the cell shows.
    pub fn cells(&self, sheet: u32, r0: i32, r1: i32, c0: i32, c1: i32) -> Result<Value, String> {
        let model = self.model();
        let ws = model.workbook.worksheet(sheet)?;
        let mut styles = StyleMap::default();
        let has_cf = !ws.conditional_formatting.is_empty();
        let mut out = Vec::new();
        for row in r0.max(1)..=r1.min(1_048_576) {
            for (col, cell) in ws.sheet_data.cells_in_row_between(row, c0.max(1), c1.min(16_384)) {
                out.push(self.cell_json(sheet, row, col, &cell, has_cf, &mut styles));
            }
        }
        Ok(json!({"sheet": sheet, "r0": r0, "r1": r1, "c0": c0, "c1": c1, "cells": out, "styles": styles.out}))
    }

    /// The cells `keys` (those of sheet index `sheet`), as [`Book::cells`] gives them;
    /// a key with no cell any more comes as `[row, col, null]`.
    pub fn cells_at(&self, keys: &[Key]) -> Value {
        let model = self.model();
        let mut styles = StyleMap::default();
        let mut by_sheet: HashMap<u32, Vec<Value>> = HashMap::new();
        for k in keys {
            let Some(si) = self.sheet_index(k.sheet) else { continue };
            let Ok(ws) = model.workbook.worksheet(si) else { continue };
            let has_cf = !ws.conditional_formatting.is_empty();
            let v = match ws.sheet_data.cell(k.row, k.col) {
                Some(cell) => self.cell_json(si, k.row, k.col, &cell, has_cf, &mut styles),
                None => json!([k.row, k.col, null]),
            };
            by_sheet.entry(si).or_default().push(v);
        }
        let sheets: Vec<Value> = by_sheet.into_iter().map(|(s, cells)| json!({"sheet": s, "cells": cells})).collect();
        json!({"sheets": sheets, "styles": styles.out})
    }

    fn cell_json(&self, sheet: u32, row: i32, col: i32, cell: &Cell, has_cf: bool, styles: &mut StyleMap) -> Value {
        let model = self.model();
        let shown = model.shown(sheet, row, col).unwrap_or_default();
        let s = cell.get_style();
        styles.of_cell(&self.engine, sheet, row, col, s);
        let (t, formula, spill) = match cell {
            Cell::EmptyCell { .. } => ("", false, false),
            Cell::BooleanCell { .. } => ("b", false, false),
            Cell::NumberCell { .. } => ("n", false, false),
            Cell::ErrorCell { .. } => ("e", false, false),
            Cell::SharedString { .. } => ("s", false, false),
            Cell::CellFormula { v, .. } | Cell::ArrayFormula { v, .. } => (value_type(v).0, true, false),
            Cell::SpillCell { v, .. } => (spill_type(v).0, false, true),
        };
        let mut t = t.to_string();
        if formula {
            t.push('f');
        }
        if spill {
            t.push('x');
        }
        let input = if spill || matches!(cell, Cell::EmptyCell { .. }) {
            None
        } else {
            let c = self.engine.get_cell_content(sheet, row, col).unwrap_or_default();
            (c != shown).then_some(c)
        };
        let mut extra = serde_json::Map::new();
        // A number format's color ([Red] for the negatives).
        if let (Cell::NumberCell { v, .. }, Some(fmt)) = (cell, styles.num_fmt(s)) {
            if fmt.contains('[') {
                let f = ironcalc::base::formatter::format::format_number(*v, &fmt, &ironcalc::base::locale::get_locale("en").expect("en"));
                if let Some(c) = f.color.and_then(format_color) {
                    extra.insert("color".into(), json!(c));
                }
            }
        } else if let (Cell::CellFormula { v: FormulaValue::Number(v), .. }, Some(fmt)) = (cell, styles.num_fmt(s)) {
            if fmt.contains('[') {
                let f = ironcalc::base::formatter::format::format_number(*v, &fmt, &ironcalc::base::locale::get_locale("en").expect("en"));
                if let Some(c) = f.color.and_then(format_color) {
                    extra.insert("color".into(), json!(c));
                }
            }
        }
        if has_cf {
            if let Ok(x) = self.engine.get_extended_cell_style(sheet, row, col) {
                let base = model.get_style_for_cell(sheet, row, col).ok();
                if base.as_ref() != Some(&x.style) {
                    extra.insert("cf".into(), style_json(&self.engine, &x.style));
                }
                if let Some(b) = &x.data_bar {
                    extra.insert("bar".into(), serde_json::to_value(b).unwrap_or(Value::Null));
                }
                if let Some(i) = &x.icon {
                    extra.insert("icon".into(), serde_json::to_value(i).unwrap_or(Value::Null));
                }
            }
        }
        let x = if extra.is_empty() { Value::Null } else { Value::Object(extra) };
        json!([row, col, shown, t, s, input, x])
    }

    /// The cells whose value as shown (or, with `formulas`, whose input)
    /// holds `q`: `[sheet, row, col, shown]`, in sheet, row, column order.
    pub fn find(&self, q: &str, sheet: Option<u32>, case: bool, whole: bool, formulas: bool, max: usize) -> Vec<Value> {
        let model = self.model();
        let ql = if case { q.to_string() } else { q.to_lowercase() };
        let mut out = Vec::new();
        for (si, ws) in model.workbook.worksheets.iter().enumerate() {
            let si = si as u32;
            if sheet.is_some_and(|s| s != si) {
                continue;
            }
            let mut cells: Vec<(i32, i32)> = ws.sheet_data.cells().map(|(r, c, _)| (r, c)).collect();
            cells.sort();
            for (r, c) in cells {
                let shown = model.shown(si, r, c).unwrap_or_default();
                let text = if formulas { self.engine.get_cell_content(si, r, c).unwrap_or_default() } else { shown.clone() };
                if text.is_empty() {
                    continue;
                }
                let t = if case { text } else { text.to_lowercase() };
                let hit = if whole { t == ql } else { t.contains(&ql) };
                if hit {
                    out.push(json!([si, r, c, shown]));
                    if out.len() >= max {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// The values of an area, as numbers where they are (for charts and
    /// summaries): rows of `[shown, number or null]`.
    pub fn values(&self, sheet: u32, r0: i32, c0: i32, r1: i32, c1: i32) -> Vec<Vec<(String, Option<f64>)>> {
        let model = self.model();
        let mut out = Vec::new();
        for r in r0..=r1 {
            let mut row = Vec::new();
            for c in c0..=c1 {
                let shown = model.shown(sheet, r, c).unwrap_or_default();
                let n = match model.get_cell_value_by_index(sheet, r, c) {
                    Ok(CellValue::Number(n)) => Some(n),
                    Ok(CellValue::Boolean(b)) => Some(b as u8 as f64),
                    _ => None,
                };
                row.push((shown, n));
            }
            out.push(row);
        }
        out
    }
}

/// Excel's number-format colors ([Red], [Color10]) as hex.
fn format_color(i: i32) -> Option<String> {
    const NAMED: [&str; 8] = ["#000000", "#0000ff", "#00ffff", "#00ff00", "#ff00ff", "#ff0000", "#ffffff", "#ffff00"];
    match i {
        0..=7 => Some(NAMED[i as usize].to_string()),
        // [Color n]: the 56-color palette, of which the first eight are above.
        _ => Some("#c00000".to_string()),
    }
}

fn layout_hash(w: &Worksheet) -> String {
    let mut h = std::hash::DefaultHasher::new();
    for c in &w.cols {
        h.write_i32(c.min);
        h.write_i32(c.max);
        h.write_u64(c.width.to_bits());
        h.write_u8(c.hidden as u8 | (c.custom_width as u8) << 1);
        h.write_i32(c.style.unwrap_or(-1));
    }
    for r in &w.rows {
        h.write_i32(r.r);
        h.write_u64(r.height.to_bits());
        h.write_u8(r.hidden as u8 | (r.custom_height as u8) << 1 | (r.custom_format as u8) << 2);
        h.write_i32(r.s);
    }
    for m in &w.merged_cells {
        h.write_i32(m.row);
        h.write_i32(m.column);
        h.write_i32(m.width);
        h.write_i32(m.height);
    }
    h.write_i32(w.frozen_rows);
    h.write_i32(w.frozen_columns);
    h.write_u8(w.show_grid_lines as u8);
    h.write_usize(w.conditional_formatting.len());
    for c in &w.conditional_formatting {
        h.write(c.range.as_bytes());
        h.write_u32(c.priority);
    }
    format!("{:x}", h.finish())
}

pub fn color_hex(engine: &Engine, c: &Color) -> Option<String> {
    match c {
        Color::None => None,
        _ => {
            let s = engine.resolve_color(c);
            (!s.is_empty()).then_some(s)
        }
    }
}

/// A style as the view draws it: font (b, i, u, strike, size, name,
/// color), fill, alignment (h, v, wrap), borders (`[style, color]`) and the
/// number format.
pub fn style_json(engine: &Engine, s: &Style) -> Value {
    let mut o = serde_json::Map::new();
    if s.font.b {
        o.insert("b".into(), json!(1));
    }
    if s.font.i {
        o.insert("i".into(), json!(1));
    }
    if s.font.u {
        o.insert("u".into(), json!(1));
    }
    if s.font.strike {
        o.insert("st".into(), json!(1));
    }
    o.insert("sz".into(), json!(s.font.sz));
    if !s.font.name.is_empty() {
        o.insert("ff".into(), json!(s.font.name));
    }
    if let Some(c) = color_hex(engine, &s.font.color) {
        o.insert("c".into(), json!(c));
    }
    if let Some(c) = color_hex(engine, &s.fill.color) {
        o.insert("bg".into(), json!(c));
    }
    if let Some(a) = &s.alignment {
        let ha = a.horizontal.to_string();
        if ha != "general" {
            o.insert("ha".into(), json!(ha));
        }
        let va = a.vertical.to_string();
        if va != "bottom" {
            o.insert("va".into(), json!(va));
        }
        if a.wrap_text {
            o.insert("wr".into(), json!(1));
        }
    }
    let side = |b: &Option<ironcalc::base::types::BorderItem>| b.as_ref().map(|b| json!([b.style.to_string(), color_hex(engine, &b.color).unwrap_or_else(|| "#000000".into())]));
    for (k, b) in [("bt", &s.border.top), ("br", &s.border.right), ("bb", &s.border.bottom), ("bl", &s.border.left)] {
        if let Some(v) = side(b) {
            o.insert(k.into(), v);
        }
    }
    if s.num_fmt != "general" {
        o.insert("nf".into(), json!(s.num_fmt));
    }
    if s.quote_prefix {
        o.insert("qp".into(), json!(1));
    }
    Value::Object(o)
}

/// The styles a response uses, by index, each read once.
#[derive(Default)]
pub struct StyleMap {
    pub out: serde_json::Map<String, Value>,
    fmts: HashMap<i32, String>,
    seen: HashSet<i32>,
}

impl StyleMap {
    fn of_cell(&mut self, engine: &Engine, sheet: u32, row: i32, col: i32, s: i32) {
        if !self.seen.insert(s) {
            return;
        }
        if let Ok(st) = engine.get_model().get_style_for_cell(sheet, row, col) {
            self.fmts.insert(s, st.num_fmt.clone());
            self.out.insert(s.to_string(), style_json(engine, &st));
        }
    }

    fn of_column(&mut self, engine: &Engine, sheet: u32, col: i32, s: i32) -> i32 {
        if self.seen.insert(s) {
            if let Ok(Some(st)) = engine.get_model().get_column_style(sheet, col) {
                self.out.insert(s.to_string(), style_json(engine, &st));
            }
        }
        s
    }

    fn of_row(&mut self, engine: &Engine, sheet: u32, row: i32, s: i32) -> i32 {
        if self.seen.insert(s) {
            if let Ok(Some(st)) = engine.get_model().get_row_style(sheet, row) {
                self.out.insert(s.to_string(), style_json(engine, &st));
            }
        }
        s
    }

    fn num_fmt(&self, s: i32) -> Option<String> {
        self.fmts.get(&s).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names() {
        assert_eq!(col_name(1), "A");
        assert_eq!(col_name(26), "Z");
        assert_eq!(col_name(27), "AA");
        assert_eq!(col_name(702), "ZZ");
        assert_eq!(col_name(703), "AAA");
        assert_eq!(cell_ref(5, 3), "C5");
    }

    /// A workbook taken apart into its frame and cells as Strata holds them,
    /// and put back, is the same workbook: values, formulas, styles, sheets.
    #[test]
    fn frame_and_cells_round_trip() {
        let mut m = new_engine("Budget");
        m.set_user_input(0, 1, 1, "Item").unwrap();
        m.set_user_input(0, 1, 2, "Cost").unwrap();
        m.set_user_input(0, 2, 1, "Rent").unwrap();
        m.set_user_input(0, 2, 2, "1200").unwrap();
        m.set_user_input(0, 3, 1, "Food").unwrap();
        m.set_user_input(0, 3, 2, "$350.50").unwrap();
        m.set_user_input(0, 4, 2, "=SUM(B2:B3)").unwrap();
        m.set_user_input(0, 5, 2, "=SEQUENCE(2,2)").unwrap();
        m.set_user_input(0, 8, 1, "=1/0").unwrap();
        m.set_user_input(0, 9, 1, "TRUE").unwrap();
        m.update_range_style(&ironcalc::base::expressions::types::Area { sheet: 0, row: 1, column: 1, width: 2, height: 1 }, "font.b", "true").unwrap();
        m.new_sheet().unwrap();
        m.set_user_input(1, 1, 1, "=Sheet1!B4*2").unwrap();
        m.set_columns_width(0, 1, 1, 200.0).unwrap();
        m.set_frozen_rows_count(0, 1).unwrap();

        let model = m.get_model();
        let fr = frame(model);
        let mut rows = Vec::new();
        for (si, ws) in model.workbook.worksheets.iter().enumerate() {
            for (r, c, cell) in ws.sheet_data.cells() {
                let s = stored(model, si as u32, r, c, &cell);
                rows.push((ws.sheet_id, r, c, json!({"kind": s.kind, "input": s.input, "text": s.text, "num": s.num, "style": s.style, "f": s.f, "extra": s.extra})));
            }
        }
        let mut wb = decode_frame(&fr).unwrap();
        let mut strings = Strings::default();
        let mut later = Vec::new();
        for (sid, r, c, j) in &rows {
            let ws = wb.worksheets.iter_mut().find(|w| w.sheet_id == *sid).unwrap();
            match restore(j, ws.shared_formulas.len(), &mut strings) {
                Restored::Cell(cell) => {
                    ws.sheet_data.set_cell(*r, *c, cell);
                }
                Restored::Input(i) => later.push((*sid, *r, *c, i)),
                Restored::Skip => {}
            }
        }
        assert!(later.is_empty());
        wb.shared_strings = strings.list;
        let mut back = engine_of(wb).unwrap();
        back.evaluate();
        for (s, r, c) in [(0, 1, 1), (0, 2, 2), (0, 3, 2), (0, 4, 2), (0, 5, 2), (0, 6, 3), (0, 8, 1), (0, 9, 1), (1, 1, 1)] {
            assert_eq!(back.shown(s, r, c).unwrap(), m.shown(s, r, c).unwrap(), "{s}!{r},{c}");
            assert_eq!(back.get_cell_content(s, r, c).unwrap(), m.get_cell_content(s, r, c).unwrap(), "{s}!{r},{c}");
        }
        assert_eq!(back.shown(0, 4, 2).unwrap(), "1550.5");
        assert_eq!(back.shown(1, 1, 1).unwrap(), "3101");
        assert!(back.get_cell_style(0, 1, 1).unwrap().font.b);
        assert_eq!(back.get_column_width(0, 1).unwrap(), m.get_column_width(0, 1).unwrap());
        assert_eq!(back.get_frozen_rows_count(0).unwrap(), 1);
        assert_eq!(back.get_name(), "Budget");
        // The same cells fingerprint the same.
        let fp = |e: &Engine| -> Vec<u64> {
            let wb = &e.get_model().workbook;
            let mut v: Vec<u64> = wb.worksheets.iter().flat_map(|ws| ws.sheet_data.cells().map(|(_, _, c)| fingerprint(&c, &ws.shared_formulas, &wb.shared_strings, &ws.name)).collect::<Vec<_>>()).collect();
            v.sort();
            v
        };
        assert_eq!(fp(&back), fp(&m));
    }

    #[test]
    fn a_formula_missing_from_the_frame_is_typed_again() {
        let mut strings = Strings::default();
        let j = json!({"kind": "formula", "input": "=1+2", "f": 9, "extra": "n", "num": 3.0});
        assert!(matches!(restore(&j, 2, &mut strings), Restored::Input(ref s) if s == "=1+2"));
        let j = json!({"kind": "spill", "num": 3.0, "extra": "n,1,1"});
        assert!(matches!(restore(&j, 2, &mut strings), Restored::Skip));
    }
}
