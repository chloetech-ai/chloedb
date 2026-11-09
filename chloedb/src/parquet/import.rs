//! Reading Parquet files into a Strata table ([`super`]).

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use datafusion::arrow::array::{Array, AsArray};
use datafusion::arrow::datatypes::{DataType, Schema as ArrowSchema};
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use parquet::arrow::ProjectionMask;
use serde_json::{json, Map, Value as Json};

use super::{distinct, ident, META_KEY};
use crate::arrow_rows::{nonfinite_notes, write_docs, RowReader};
use crate::db::Db;
use crate::error::{Error, Result};
use crate::ingest;
use crate::schema::{Field, FieldType, Schema, DEFAULT_HNSW};
use crate::storage::DbOptions;
use crate::value::{Document, Value};

/// The rows a plan reads to choose new fields' types: the first of the
/// files'.
pub const SAMPLE: usize = 100_000;

/// Rows a batch read from a file holds.
const BATCH: usize = 4096;

/// Distinct texts a column's sample counts, past which it counts none.
const DISTINCT: usize = 10_000;

/// How an import goes.
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// The table the rows go to (any case). By default: the table an
    /// export kept in the file ([`META_KEY`]), else the name of the file
    /// (up to its first `.`) or directory, as a Strata identifier.
    pub table: Option<String>,
    /// The schema of the table the import creates, in place of the one it
    /// would make; the columns fill the fields of their names. A plan's
    /// JSON ([`Plan::to_json`]), edited or not, is one.
    pub schema: Option<Schema>,
    /// The column that is the key of the table the import creates, in
    /// place of a column named `id`; `""` for no key.
    pub key: Option<String>,
    /// Keep binary columns as base64 text, in a stored KEYWORD field that
    /// is not indexed, rather than leave them out.
    pub blobs_base64: bool,
}

/// What an import does ([`plan`]).
#[derive(Clone, Debug)]
pub struct Plan {
    /// The table's schema: the one the import creates, or the existing
    /// table's.
    pub schema: Schema,
    /// Whether the table existed when the plan was made. Rows go into an
    /// empty table in one bulk load, and are otherwise upserted by key
    /// (inserted, in a table without one).
    pub exists: bool,
    /// The Parquet files read, and the rows they hold.
    pub files: usize,
    pub rows: u64,
    /// The columns read, each with the field it fills.
    pub columns: Vec<(String, usize)>,
    /// The Hive partition keys read from the files' directories, each with
    /// the field it fills.
    pub partitions: Vec<(String, usize)>,
    /// The columns left out, each with the reason.
    pub skipped: Vec<(String, String)>,
    /// What does not carry over exactly, and names Strata spells otherwise.
    pub notes: Vec<String>,
}

impl Plan {
    /// The plan as JSON: the schema's short form ([`Schema::to_short_json`])
    /// with whether it `exists`, the `files` and `rows`, the `columns` and
    /// `partitions` (each: field), what was `skipped` (column: why) and
    /// `notes`. As [`ImportOptions::schema`] it gives the schema back.
    pub fn to_json(&self) -> Json {
        let mut o = Map::new();
        if let Json::Object(s) = self.schema.to_short_json() {
            o.extend(s);
        }
        o.insert("exists".into(), json!(self.exists));
        o.insert("files".into(), json!(self.files));
        o.insert("rows".into(), json!(self.rows));
        let named = |cols: &[(String, usize)]| -> Map<String, Json> { cols.iter().map(|(c, f)| (c.clone(), json!(self.schema.fields[*f].name))).collect() };
        o.insert("columns".into(), Json::Object(named(&self.columns)));
        if !self.partitions.is_empty() {
            o.insert("partitions".into(), Json::Object(named(&self.partitions)));
        }
        if !self.skipped.is_empty() {
            o.insert("skipped".into(), Json::Object(self.skipped.iter().map(|(c, why)| (c.clone(), json!(why))).collect()));
        }
        if !self.notes.is_empty() {
            o.insert("notes".into(), json!(self.notes));
        }
        Json::Object(o)
    }
}

/// What an import did.
#[derive(Clone, Debug, Default)]
pub struct Imported {
    /// The Strata table.
    pub table: String,
    pub rows: u64,
    pub files: usize,
    /// Whether the import created the table.
    pub created: bool,
    /// Whether the rows were bulk-loaded (the table was empty) rather than
    /// upserted.
    pub bulk: bool,
    /// The columns left out, each with the reason.
    pub skipped: Vec<(String, String)>,
    /// The plan's notes, and the NaN and infinite numbers read as NULL.
    pub notes: Vec<String>,
}

impl Imported {
    /// `{"table", "rows", "files", "created", "bulk", "skipped": {column: why}, "notes"}`.
    pub fn to_json(&self) -> Json {
        let skipped: Map<String, Json> = self.skipped.iter().map(|(c, why)| (c.clone(), json!(why))).collect();
        json!({"table": self.table, "rows": self.rows, "files": self.files, "created": self.created, "bulk": self.bulk, "skipped": skipped, "notes": self.notes})
    }
}

/// What [`Source::plan`] makes of the Parquet file or directory `path`,
/// for the database `db` (`None`: a new one).
pub fn plan(path: &Path, db: Option<&Db>, opts: &ImportOptions) -> Result<Plan> {
    Source::open(path)?.plan(db, opts)
}

/// Imports the Parquet file or directory `path` into `db` ([`Source::plan`],
/// then [`Source::load`]).
pub fn import(db: &Db, path: &Path, opts: &ImportOptions) -> Result<Imported> {
    let src = Source::open(path)?;
    let plan = src.plan(Some(db), opts)?;
    src.load(db, &plan)
}

/// Creates a database at `path` whose first table is the one the Parquet
/// file or directory `parquet` makes ([`Source::plan`]), and loads it.
pub fn import_new(parquet: &Path, path: &Path, db_opts: DbOptions, opts: &ImportOptions) -> Result<(Db, Imported)> {
    let src = Source::open(parquet)?;
    let plan = src.plan(None, opts)?;
    let db = Db::create(path, plan.schema.clone(), db_opts)?;
    let r = src.load(&db, &plan)?;
    Ok((db, r))
}

/// One Parquet file of a source.
#[derive(Clone, Debug)]
struct Part {
    path: PathBuf,
    /// Its name as messages give it: relative to the source's directory.
    name: String,
    /// Its Hive partition values (`key=value` directories), by key.
    partition: Vec<(String, Option<String>)>,
}

/// A Parquet file, or a directory of them, open for reading.
pub struct Source {
    parts: Vec<Part>,
    /// The files' columns: the first file's, then those later files add.
    arrow: Arc<ArrowSchema>,
    /// The schema an export kept in the first file.
    meta: Option<Schema>,
    /// The partition keys, in the order the directories give them.
    keys: Vec<String>,
    rows: u64,
    /// The name a table made from it takes by default.
    stem: String,
}

impl Source {
    /// Opens the Parquet file `path`, or every Parquet file under the
    /// directory `path` (named `.parquet`, or without an extension and
    /// starting as Parquet does; names starting with `_` or `.`, such as
    /// Spark's `_SUCCESS`, are passed over), in the order of their paths.
    pub fn open(path: &Path) -> Result<Source> {
        let not_found = || Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{}: no such file or directory", path.display())));
        let parts = if path.is_dir() {
            let files = files_under(path)?;
            if files.is_empty() {
                return Err(Error::Schema(format!("{}: no Parquet files in the directory", path.display())));
            }
            files
                .into_iter()
                .map(|f| {
                    let rel = f.strip_prefix(path).unwrap_or(&f).to_path_buf();
                    let partition = rel.parent().into_iter().flat_map(|p| p.components()).filter_map(|c| hive(&c.as_os_str().to_string_lossy())).collect();
                    Part { name: rel.to_string_lossy().into_owned(), path: f, partition }
                })
                .collect()
        } else if path.is_file() {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            vec![Part { path: path.to_path_buf(), name, partition: Vec::new() }]
        } else {
            return Err(not_found());
        };
        let mut keys: Vec<String> = Vec::new();
        for p in &parts {
            for (k, _) in &p.partition {
                if !keys.iter().any(|x| x == k) {
                    keys.push(k.clone());
                }
            }
        }
        let mut rows = 0u64;
        let mut first: Option<(Arc<ArrowSchema>, Option<Schema>)> = None;
        // Every file's columns: the first's, then those later files add.
        let mut columns: Vec<Arc<datafusion::arrow::datatypes::Field>> = Vec::new();
        for p in &parts {
            let b = builder(p)?;
            rows += b.metadata().file_metadata().num_rows().max(0) as u64;
            for c in b.schema().fields() {
                if !columns.iter().any(|x| x.name() == c.name()) {
                    columns.push(c.clone());
                }
            }
            let meta = b.metadata().file_metadata().key_value_metadata().and_then(|kv| kv.iter().find(|k| k.key == META_KEY).and_then(|k| k.value.clone()));
            let meta = meta.map(|m| Schema::from_json(&m).map_err(|e| Error::Schema(format!("{}: the ChloeDB schema it keeps: {e}", p.name)))).transpose()?;
            match &first {
                None => first = Some((b.schema().clone(), meta)),
                // An export of several tables to a directory: a file each.
                Some((_, Some(a))) if meta.as_ref().is_some_and(|m| !m.table.eq_ignore_ascii_case(&a.table)) => {
                    let b = meta.map(|m| m.table).unwrap_or_default();
                    return Err(Error::Schema(format!("{}: its files hold different ChloeDB tables ({}, {b}..): import one file at a time", path.display(), a.table)));
                }
                Some(_) => {}
            }
        }
        let (_, meta) = first.expect("a file");
        let arrow = Arc::new(ArrowSchema::new(columns));
        let base = path.file_name().map_or_else(|| "data".to_string(), |n| n.to_string_lossy().into_owned());
        let stem = base.split('.').next().filter(|s| !s.is_empty()).unwrap_or("data").to_string();
        Ok(Source { parts, arrow, meta, keys, rows, stem })
    }

    /// The files' columns: the first file's, then those later files add.
    pub fn arrow_schema(&self) -> &ArrowSchema {
        &self.arrow
    }

    /// The table whose schema an export kept in the files ([`META_KEY`]).
    pub fn kept_table(&self) -> Option<&str> {
        self.meta.as_ref().map(|m| m.table.as_str())
    }

    /// What an import into `db` does (see [`super`]): into `db`'s table of
    /// the source's name (in any case) when it has one, else into a new
    /// table with the schema given ([`ImportOptions::schema`]), the one an
    /// export kept ([`META_KEY`]), or one made from the columns.
    pub fn plan(&self, db: Option<&Db>, opts: &ImportOptions) -> Result<Plan> {
        self.plan_over(&db.map(Db::catalog).unwrap_or_default(), opts)
    }

    /// [`Source::plan`] for a database whose tables are `existing`, such as
    /// a server's, which may hold them on shards.
    pub fn plan_over(&self, existing: &[Arc<Schema>], opts: &ImportOptions) -> Result<Plan> {
        let name = match (&opts.table, &opts.schema, &self.meta) {
            (Some(t), _, _) => t.clone(),
            (None, Some(s), _) => s.table.clone(),
            (None, None, Some(m)) => m.table.clone(),
            (None, None, None) => ident(&self.stem),
        };
        let mut notes = Vec::new();
        let (schema, exists) = if let Some(s) = existing.iter().find(|s| s.table.eq_ignore_ascii_case(&name)) {
            ((**s).clone(), true)
        } else if let Some(s) = opts.schema.clone().or_else(|| self.meta.clone()) {
            let mut s = s;
            if s.table != name {
                s.table = ident(&name);
            }
            if let Some(k) = &opts.key {
                let whose = if opts.schema.is_some() { "given" } else { "kept in the file" };
                notes.push(format!("the schema {whose} names the key: key {k:?} is not used"));
            }
            (s, false)
        } else {
            let (s, n) = self.infer(&ident(&name), opts)?;
            notes = n;
            (s, false)
        };
        if exists && (opts.schema.is_some() || opts.key.is_some()) {
            notes.push(format!("table {} exists: its schema stands, and the schema or key given is not used", schema.table));
        }
        let mut plan = Plan { schema, exists, files: self.parts.len(), rows: self.rows, columns: Vec::new(), partitions: Vec::new(), skipped: Vec::new(), notes };
        let field_of = |s: &Schema, c: &str| s.field_index(c).or_else(|| s.fields.iter().position(|f| f.name.eq_ignore_ascii_case(c) || f.name.eq_ignore_ascii_case(&ident(c))));
        for c in self.arrow.fields() {
            match field_of(&plan.schema, c.name()) {
                Some(f) if plan.columns.iter().any(|(_, x)| *x == f) => plan.skipped.push((c.name().clone(), format!("field {} is filled by another column", plan.schema.fields[f].name))),
                Some(f) => plan.columns.push((c.name().clone(), f)),
                None if plan.skipped.iter().any(|(x, _)| x == c.name()) => {}
                None => plan.skipped.push((c.name().clone(), self.why_skipped(c.name(), &plan.schema, opts))),
            }
        }
        for k in &self.keys {
            match field_of(&plan.schema, k) {
                Some(f) if !plan.columns.iter().any(|(_, x)| *x == f) => plan.partitions.push((k.clone(), f)),
                _ => plan.skipped.push((format!("{k} (partition)"), format!("no field {k} in table {}", plan.schema.table))),
            }
        }
        for (c, f) in &plan.columns {
            let name = &plan.schema.fields[*f].name;
            if name != c && !plan.exists {
                plan.notes.push(format!("column {c:?} is {name}"));
            }
        }
        Ok(plan)
    }

    /// Why column `c` fills no field of `schema`.
    fn why_skipped(&self, c: &str, schema: &Schema, opts: &ImportOptions) -> String {
        if let Some(t) = self.arrow.field_with_name(c).ok().map(|f| f.data_type()) {
            if let Choice::Skip(why) = choose(c, t, &Stats::default(), false, opts) {
                if !schema.fields.iter().any(|f| f.name.eq_ignore_ascii_case(c)) {
                    return why;
                }
            }
        }
        format!("no field {c} in table {}", schema.table)
    }

    /// A schema made from the files' columns and the partition keys,
    /// by their types and a sample of their values, and the notes on it.
    fn infer(&self, table: &str, opts: &ImportOptions) -> Result<(Schema, Vec<String>)> {
        let stats = self.sample()?;
        let mut notes = Vec::new();
        let mut used = HashSet::new();
        let mut fields = Vec::new();
        let mut key: Option<String> = None;
        let wanted = opts.key.as_deref().filter(|k| !k.is_empty());
        if let Some(k) = wanted {
            if !self.arrow.fields().iter().any(|c| c.name().eq_ignore_ascii_case(k)) && !self.keys.iter().any(|p| p.eq_ignore_ascii_case(k)) {
                return Err(Error::Schema(format!("key {k}: no such column")));
            }
        }
        for (c, st) in self.arrow.fields().iter().zip(&stats) {
            let is_key = match wanted {
                Some(k) => c.name().eq_ignore_ascii_case(k),
                None => opts.key.is_none() && key.is_none() && c.name().eq_ignore_ascii_case("id") && keyable(c.data_type()) && st.nulls == 0 && st.rows > 0,
            };
            match choose(c.name(), c.data_type(), st, is_key, opts) {
                Choice::Skip(why) if is_key => return Err(Error::Schema(format!("key {}: {why}", c.name()))),
                Choice::Skip(_) => {}
                Choice::Field(mut f, note) => {
                    f.name = distinct(ident(c.name()), &mut used);
                    if let Some(n) = note {
                        notes.push(format!("{}: {n}", f.name));
                    }
                    if is_key {
                        if !matches!(f.ty, FieldType::Keyword | FieldType::Int) || f.logical.is_some() || f.multi {
                            return Err(Error::Schema(format!("key {}: a key is integers or text, not {}", c.name(), c.data_type())));
                        }
                        if wanted.is_none() {
                            notes.push(format!("{} is the key (key names another column, or \"\" for none)", f.name));
                        }
                        key = Some(f.name.clone());
                    }
                    fields.push(f);
                }
            }
        }
        for k in &self.keys {
            let ints = self.parts.iter().filter_map(|p| p.partition.iter().find(|(x, _)| x == k)).all(|(_, v)| v.as_deref().is_none_or(|v| v.parse::<i64>().is_ok()));
            let name = distinct(ident(k), &mut used);
            let f = Field::new(&name, if ints { FieldType::Int } else { FieldType::Keyword });
            if wanted.is_some_and(|w| w.eq_ignore_ascii_case(k)) {
                key = Some(name.clone());
            }
            notes.push(format!("{name}: the Hive partition {k}=… of each file's directory"));
            fields.push(f);
        }
        if fields.is_empty() {
            return Err(Error::Schema("no column ChloeDB can hold: see the plan's skipped columns".into()));
        }
        let s = Schema::new(table, fields, key.as_deref())?;
        Ok((s, notes))
    }

    /// Statistics of the first [`SAMPLE`] rows' values, a column each.
    fn sample(&self) -> Result<Vec<Stats>> {
        let mut stats: Vec<Stats> = self.arrow.fields().iter().map(|_| Stats::default()).collect();
        let mut n = 0usize;
        for p in &self.parts {
            if n >= SAMPLE {
                break;
            }
            let b = builder(p)?;
            let cols: Vec<Option<usize>> = self.arrow.fields().iter().map(|c| b.schema().index_of(c.name()).ok()).collect();
            for batch in b.with_batch_size(BATCH).build().map_err(|e| parquet_error(p, e))? {
                let batch = batch.map_err(|e| Error::Schema(format!("{}: {e}", p.name)))?;
                let take = batch.num_rows().min(SAMPLE - n);
                for (st, c) in stats.iter_mut().zip(&cols) {
                    match c {
                        Some(c) => st.add(batch.column(*c).slice(0, take).as_ref()),
                        // A column a file lacks is NULL in its rows.
                        None => {
                            st.rows += take as u64;
                            st.nulls += take as u64;
                        }
                    }
                }
                n += take;
                if n >= SAMPLE {
                    break;
                }
            }
        }
        Ok(stats)
    }

    /// Creates the plan's table when `db` lacks it, and writes the rows:
    /// into an empty table in one bulk load (all or nothing), into a table
    /// with rows upserted by key ([`ingest::IMPORT_BATCH`] rows a transaction).
    /// Embedded vectors the rows leave out are computed when `db` has the
    /// field's model ([`Db::open_embedders`]).
    pub fn load(&self, db: &Db, plan: &Plan) -> Result<Imported> {
        let name = plan.schema.table.clone();
        if db.storage().table(&name).is_none() {
            db.create_table(plan.schema.clone())?;
        }
        let empty = db.snapshot().table(&name).is_none_or(|s| s.n_live_docs() == 0);
        let (rows, nonfinite) = if empty {
            self.documents(plan, |docs| {
                let mut n = 0u64;
                db.try_load_into(&name, docs.inspect(|d| n += d.is_ok() as u64))?;
                Ok(n)
            })?
        } else {
            let keyed = db.storage().table(&name).is_some_and(|s| s.primary_key.is_some());
            self.documents(plan, |docs| write_docs(db, &name, keyed, docs))?
        };
        let mut notes = plan.notes.clone();
        notes.extend(nonfinite_notes(&plan.schema, &nonfinite));
        Ok(Imported { table: name, rows, files: self.parts.len(), created: !plan.exists, bulk: empty, skipped: plan.skipped.clone(), notes })
    }

    /// Calls `f` with the rows of every file as documents of `plan.schema`
    /// (fields without a column take their defaults, generated fields are
    /// filled in), or the first error naming the file and row; returns
    /// what `f` did, and the NaN and infinite numbers read as NULL by
    /// field.
    pub fn documents<R>(&self, plan: &Plan, f: impl FnOnce(&mut dyn Iterator<Item = Result<Document>>) -> Result<R>) -> Result<(R, Vec<u64>)> {
        let schema = &plan.schema;
        let mut nonfinite = vec![0u64; schema.fields.len()];
        let mut parts = self.parts.iter();
        let mut current: Option<(&Part, ParquetRecordBatchReader, RowReader, Vec<(usize, Value)>)> = None;
        let mut ready = Vec::new().into_iter();
        let mut row = 0u64;
        let mut failed = false;
        let mut docs = std::iter::from_fn(|| loop {
            if failed {
                return None;
            }
            if let Some(d) = ready.next() {
                return Some(Ok(d));
            }
            if current.is_none() {
                let p = parts.next()?;
                match self.open_part(p, plan) {
                    Ok((r, rows, consts)) => current = Some((p, r, rows, consts)),
                    Err(e) => {
                        failed = true;
                        return Some(Err(e));
                    }
                }
                row = 0;
            }
            let (p, reader, rows, consts) = current.as_mut().expect("a file open");
            let batch = match reader.next() {
                None => {
                    for (t, n) in nonfinite.iter_mut().zip(&rows.nonfinite) {
                        *t += n;
                    }
                    current = None;
                    continue;
                }
                Some(Err(e)) => {
                    failed = true;
                    return Some(Err(Error::Schema(format!("{}: {e}", p.name))));
                }
                Some(Ok(b)) => b,
            };
            let at = row;
            row += batch.num_rows() as u64;
            match rows.read(&batch, at) {
                Ok(mut docs) => {
                    for d in &mut docs {
                        for (f, v) in consts.iter() {
                            d[*f] = v.clone();
                        }
                        schema.fill_generated(d);
                    }
                    ready = docs.into_iter();
                }
                Err(e) => {
                    failed = true;
                    let e = match e {
                        Error::Schema(m) => Error::Schema(format!("{}: {m}", p.name)),
                        e => e,
                    };
                    return Some(Err(e));
                }
            }
        });
        let r = f(&mut docs)?;
        Ok((r, nonfinite))
    }

    /// A reader of the plan's columns of `p`, the rows' reader, and the
    /// values its partition gives.
    fn open_part(&self, p: &Part, plan: &Plan) -> Result<(ParquetRecordBatchReader, RowReader, Vec<(usize, Value)>)> {
        let b = builder(p)?;
        let mut wanted: Vec<(usize, usize)> = plan.columns.iter().filter_map(|(c, f)| b.schema().index_of(c).ok().map(|i| (i, *f))).collect();
        wanted.sort_unstable();
        let mask = ProjectionMask::roots(b.parquet_schema(), wanted.iter().map(|(i, _)| *i));
        let rows = RowReader::with_columns(&plan.schema, wanted.iter().map(|(_, f)| Some(*f)).collect())?;
        let reader = b.with_projection(mask).with_batch_size(BATCH).build().map_err(|e| parquet_error(p, e))?;
        let mut consts = Vec::new();
        for (k, f) in &plan.partitions {
            let field = &plan.schema.fields[*f];
            let v = match p.partition.iter().find(|(x, _)| x == k).and_then(|(_, v)| v.as_deref()) {
                None => Value::Null,
                Some(v) => {
                    // A directory's name is text: as the field's type, when it parses.
                    let j = match (field.logical, &field.ty) {
                        (Some(crate::schema::Logical::Bool), _) => v.parse::<bool>().map_or_else(|_| Json::String(v.into()), Json::Bool),
                        (Some(_), _) => v.parse::<i64>().map_or_else(|_| Json::String(v.into()), |x| json!(x)),
                        (None, FieldType::Int) => v.parse::<i64>().map_or_else(|_| Json::String(v.into()), |x| json!(x)),
                        (None, FieldType::Float) => v.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or_else(|| Json::String(v.into()), Json::Number),
                        _ => Json::String(v.into()),
                    };
                    ingest::field_value(field, plan.schema.primary_key == Some(*f), &j).map_err(|e| Error::Schema(format!("{}: partition {k}={v}: {e}", p.name)))?
                }
            };
            consts.push((*f, v));
        }
        Ok((reader, rows, consts))
    }
}

fn builder(p: &Part) -> Result<ParquetRecordBatchReaderBuilder<File>> {
    let f = File::open(&p.path).map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("{}: {e}", p.path.display()))))?;
    ParquetRecordBatchReaderBuilder::try_new(f).map_err(|e| parquet_error(p, e))
}

fn parquet_error(p: &Part, e: impl std::fmt::Display) -> Error {
    Error::Schema(format!("{}: not a Parquet file ChloeDB can read ({e})", p.name))
}

/// The Parquet files under `dir`, at any depth, in the order of their
/// paths ([`Source::open`]).
pub(super) fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('_') || name.starts_with('.') {
                continue;
            }
            let path = e.path();
            if e.file_type()?.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("parquet")) || (path.extension().is_none() && super::is_parquet_file(&path)) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A Hive partition directory's key and value (`key=value`, the value
/// percent-decoded; `__HIVE_DEFAULT_PARTITION__` is NULL).
fn hive(component: &str) -> Option<(String, Option<String>)> {
    let (k, v) = component.split_once('=')?;
    if k.is_empty() {
        return None;
    }
    let v = (v != "__HIVE_DEFAULT_PARTITION__").then(|| percent_decode(v));
    Some((percent_decode(k), v))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(x) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a column of type `t` can be a key: integers or text.
fn keyable(t: &DataType) -> bool {
    match t {
        DataType::Dictionary(_, v) => keyable(v),
        t => t.is_integer() || matches!(t, DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View),
    }
}

// ------------------------------------------------------------------ types

/// A column's sample.
#[derive(Debug, Default)]
struct Stats {
    rows: u64,
    nulls: u64,
    /// Texts, the longest in bytes, and the words of all of them (runs
    /// without spaces that hold a letter).
    texts: u64,
    text_max: usize,
    words: u64,
    /// Hashes of the distinct texts, up to [`DISTINCT`]; `None` past it.
    distinct: Option<HashSet<u64>>,
    /// The lengths of the lists (and of the lists in lists) seen.
    lens: Option<(usize, usize)>,
}

impl Stats {
    /// Whether the texts are labels rather than prose: short (2.5 words or
    /// fewer on average, none over 256 bytes), or few values repeated (at
    /// least 100 texts, each value ten times on average), as the SQLite
    /// import judges.
    fn labels(&self) -> bool {
        let short = self.words as f64 <= 2.5 * self.texts as f64;
        let repeated = self.texts >= 100 && self.distinct.as_ref().is_some_and(|d| d.len() as u64 * 10 <= self.texts);
        self.text_max <= 256 && (short || repeated)
    }

    fn add(&mut self, a: &dyn Array) {
        self.rows += a.len() as u64;
        self.nulls += a.null_count() as u64;
        if self.texts == 0 && self.distinct.is_none() {
            self.distinct = Some(HashSet::new());
        }
        match a.data_type() {
            DataType::Utf8 => self.texts_of((0..a.len()).filter(|&i| a.is_valid(i)).map(|i| a.as_string::<i32>().value(i))),
            DataType::LargeUtf8 => self.texts_of((0..a.len()).filter(|&i| a.is_valid(i)).map(|i| a.as_string::<i64>().value(i))),
            DataType::Utf8View => self.texts_of((0..a.len()).filter(|&i| a.is_valid(i)).map(|i| a.as_string_view().value(i))),
            DataType::List(_) => self.lists((0..a.len()).filter(|&i| a.is_valid(i)).map(|i| a.as_list::<i32>().value(i))),
            DataType::LargeList(_) => self.lists((0..a.len()).filter(|&i| a.is_valid(i)).map(|i| a.as_list::<i64>().value(i))),
            _ => {}
        }
    }

    fn texts_of<'a>(&mut self, texts: impl Iterator<Item = &'a str>) {
        for s in texts {
            self.texts += 1;
            self.text_max = self.text_max.max(s.len());
            self.words += s.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count() as u64;
            if let Some(d) = &mut self.distinct {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                s.hash(&mut h);
                d.insert(h.finish());
                if d.len() > DISTINCT {
                    self.distinct = None;
                }
            }
        }
    }

    /// The lengths of `lists` (of lists in them, for lists of lists).
    fn lists(&mut self, lists: impl Iterator<Item = datafusion::arrow::array::ArrayRef>) {
        for l in lists {
            let lens: Vec<usize> = match l.data_type() {
                DataType::List(_) => (0..l.len()).filter(|&j| l.is_valid(j)).map(|j| l.as_list::<i32>().value(j).len()).collect(),
                DataType::LargeList(_) => (0..l.len()).filter(|&j| l.is_valid(j)).map(|j| l.as_list::<i64>().value(j).len()).collect(),
                _ => vec![l.len()],
            };
            for n in lens {
                self.lens = Some(self.lens.map_or((n, n), |(lo, hi)| (lo.min(n), hi.max(n))));
            }
        }
    }

    /// The one length every list seen has, if they have one.
    fn one_len(&self) -> Option<usize> {
        self.lens.and_then(|(lo, hi)| (lo == hi && lo > 0).then_some(lo))
    }
}

/// What a column of a table being made becomes.
enum Choice {
    /// The field (its name is the caller's to set), and a note.
    Field(Field, Option<String>),
    Skip(String),
}

fn numeric(t: &DataType) -> bool {
    t.is_integer() || t.is_floating() || matches!(t, DataType::Decimal32(..) | DataType::Decimal64(..) | DataType::Decimal128(..) | DataType::Decimal256(..))
}

fn text(t: &DataType) -> bool {
    match t {
        DataType::Dictionary(_, v) => text(v),
        t => matches!(t, DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View),
    }
}

fn choose(name: &str, t: &DataType, st: &Stats, key: bool, opts: &ImportOptions) -> Choice {
    let field = |ty: FieldType| Choice::Field(Field::new(name, ty), None);
    let vector = |dim: usize| Field::new(name, FieldType::Vector { dim, index: DEFAULT_HNSW });
    match t {
        DataType::Null => Choice::Skip("no values (Arrow type Null)".into()),
        DataType::Boolean => Choice::Field(Field::boolean(name), None),
        t if t.is_integer() => field(FieldType::Int),
        t if t.is_floating() => field(FieldType::Float),
        DataType::Decimal32(p, 0) | DataType::Decimal64(p, 0) | DataType::Decimal128(p, 0) | DataType::Decimal256(p, 0) if *p <= 18 => field(FieldType::Int),
        DataType::Decimal32(p, s) | DataType::Decimal64(p, s) | DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => {
            Choice::Field(Field::new(name, FieldType::Float), Some(format!("decimal({p}, {s}) as FLOAT, a double: about 15 significant digits")))
        }
        DataType::Date32 | DataType::Date64 => Choice::Field(Field::timestamp(name), Some("dates as TIMESTAMP, at midnight UTC".into())),
        DataType::Timestamp(..) => Choice::Field(Field::timestamp(name), None),
        DataType::Dictionary(_, v) if text(v) => field(FieldType::Keyword),
        DataType::Dictionary(_, v) => choose(name, v, st, key, opts),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View if key || st.labels() => field(FieldType::Keyword),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => field(FieldType::Text { positions: true }),
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView | DataType::FixedSizeBinary(_) => match opts.blobs_base64 {
            true => Choice::Field(Field::new(name, FieldType::Keyword).stored_only(), Some("binary values as base64 text".into())),
            false => Choice::Skip("binary values: ChloeDB has no binary type (blobs_base64 keeps them as base64 text)".into()),
        },
        DataType::FixedSizeList(item, n) if numeric(item.data_type()) && *n > 0 => Choice::Field(vector(*n as usize), None),
        DataType::List(item) | DataType::LargeList(item) | DataType::ListView(item) | DataType::LargeListView(item) => match item.data_type() {
            t if text(t) => Choice::Field(Field::new(name, FieldType::Keyword).multi(), None),
            t if t.is_integer() => Choice::Field(Field::new(name, FieldType::Int).multi(), None),
            t if numeric(t) => match st.one_len() {
                Some(n) => Choice::Field(vector(n), Some(format!("lists of {n} numbers as VECTOR({n})"))),
                None if st.lens.is_none() => Choice::Skip("lists of numbers, all empty or NULL in the sample: their length is unknown".into()),
                None => Choice::Field(Field::new(name, FieldType::Json), Some("lists of numbers of several lengths as JSON".into())),
            },
            DataType::FixedSizeList(v, n) if numeric(v.data_type()) && *n > 0 => Choice::Field(vector(*n as usize).multi(), None),
            DataType::List(v) | DataType::LargeList(v) if numeric(v.data_type()) => match st.one_len() {
                Some(n) => Choice::Field(vector(n).multi(), Some(format!("lists of lists of {n} numbers as VECTOR({n})[]"))),
                None => Choice::Field(Field::new(name, FieldType::Json), Some("lists of lists of numbers as JSON".into())),
            },
            _ => Choice::Field(Field::new(name, FieldType::Json), Some(format!("{t} as JSON"))),
        },
        DataType::FixedSizeList(..) | DataType::Struct(_) | DataType::Map(..) => Choice::Field(Field::new(name, FieldType::Json), Some(format!("{t} as JSON"))),
        t => Choice::Skip(format!("ChloeDB has no type for {t}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hive_partitions() {
        assert_eq!(hive("year=2026"), Some(("year".into(), Some("2026".into()))));
        assert_eq!(hive("city=S%C3%A3o%20Paulo"), Some(("city".into(), Some("São Paulo".into()))));
        assert_eq!(hive("x=__HIVE_DEFAULT_PARTITION__"), Some(("x".into(), None)));
        assert_eq!(hive("part-0"), None);
        assert_eq!(hive("=a"), None);
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%éa"), "%éa");
    }
}
