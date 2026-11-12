//! Strata rows as Arrow record batches and back, by a table's schema
//! (feature `parquet`): what Parquet files hold ([`crate::parquet`]), and
//! what Python hands over as Arrow tables.
//!
//! A field is written as one Arrow type ([`arrow_type`]):
//!
//! | Strata field | Arrow |
//! |---|---|
//! | `INT` | `Int64` |
//! | `FLOAT` | `Float64` |
//! | `BOOL` | `Boolean` |
//! | `TIMESTAMP` | `Timestamp(Microsecond, "UTC")` |
//! | `TEXT`, `KEYWORD` | `Utf8` |
//! | `VECTOR(n)` | `FixedSizeList(Float32, n)` |
//! | `VECTOR(n)[]` (multi-vector) | `List(FixedSizeList(Float32, n))` |
//! | multi-valued `KEYWORD`, `INT` | `List(Utf8)`, `List(Int64)` |
//! | `JSON`, `SPARSE`, `EDGES`, `MEDIA` | `Utf8`, the value's JSON text |
//!
//! and read from whatever Arrow type holds its values ([`RowReader`]):
//! any integer, float or decimal for a number, dates and timestamps of any
//! unit (or ISO 8601 text) for a `TIMESTAMP`, a list of any length-`n`
//! numbers or `4n` little-endian float32 bytes for a `VECTOR(n)`, a list
//! for a multi-valued field, and structs, maps and lists (as JSON) for a
//! `JSON`, `SPARSE`, `EDGES` or `MEDIA` field. Dictionary-encoded columns
//! are read as their values. Binary values in a `TEXT` or `KEYWORD` field
//! become base64 text. A NaN or infinite number in a `FLOAT` field is read
//! as NULL ([`RowReader::nonfinite`] counts them).

use std::sync::Arc;

use datafusion::arrow::array::{
    Array, ArrayRef, AsArray, BooleanBuilder, FixedSizeListBuilder, Float32Builder, Float64Builder, Int64Builder, ListBuilder, StringBuilder, TimestampMicrosecondBuilder,
};
use datafusion::arrow::compute::cast;
use datafusion::arrow::datatypes::{
    DataType, Date32Type, Date64Type, Field as ArrowField, Float16Type, Float32Type, Float64Type, Int16Type, Int32Type, Int64Type, Int8Type, Schema as ArrowSchema, TimeUnit,
    TimestampMicrosecondType, TimestampMillisecondType, TimestampNanosecondType, TimestampSecondType, UInt16Type, UInt32Type, UInt64Type, UInt8Type,
};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::arrow::util::display::{ArrayFormatter, FormatOptions};
use serde_json::{Map, Value as Json};

use crate::error::{Error, Result};
use crate::ingest;
use crate::schema::{Field, FieldType, Logical, Schema};
use crate::value::{Document, Value};

const DAY_MICROS: i64 = 86_400_000_000;

/// The Arrow type field `f` is written as (see [`self`]).
pub fn arrow_type(f: &Field) -> DataType {
    let item = |t: DataType| Arc::new(ArrowField::new("item", t, true));
    match &f.ty {
        FieldType::Vector { dim, .. } => {
            let v = DataType::FixedSizeList(item(DataType::Float32), *dim as i32);
            if f.multi {
                DataType::List(item(v))
            } else {
                v
            }
        }
        FieldType::Int if f.multi => DataType::List(item(DataType::Int64)),
        FieldType::Keyword if f.multi => DataType::List(item(DataType::Utf8)),
        FieldType::Int => match f.logical {
            Some(Logical::Bool) => DataType::Boolean,
            Some(Logical::Timestamp) => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            None => DataType::Int64,
        },
        FieldType::Float => DataType::Float64,
        FieldType::Text { .. } | FieldType::Keyword | FieldType::Edges | FieldType::Sparse | FieldType::Json => DataType::Utf8,
    }
}

/// The Arrow schema of `s`'s rows: a column per field, named as the field,
/// of its [`arrow_type`]. The primary key is the only column that is not
/// nullable.
pub fn arrow_schema(s: &Schema) -> ArrowSchema {
    ArrowSchema::new(s.fields.iter().enumerate().map(|(i, f)| ArrowField::new(&f.name, arrow_type(f), s.primary_key != Some(i))).collect::<Vec<_>>())
}

/// `docs`, rows of `schema` in the stored form (as [`crate::db::Db::documents`]
/// reads them, or [`crate::value::conform`] leaves them), as one record
/// batch of [`arrow_schema`].
pub fn batch_of(schema: &Schema, arrow: &Arc<ArrowSchema>, docs: &[Document]) -> Result<RecordBatch> {
    let columns = schema.fields.iter().enumerate().map(|(i, f)| column(f, docs.iter().map(|d| &d[i]), docs.len())).collect::<Result<Vec<_>>>()?;
    RecordBatch::try_new(arrow.clone(), columns).map_err(|e| Error::Schema(format!("Arrow batch: {e}")))
}

fn column<'a>(f: &Field, values: impl Iterator<Item = &'a Value>, n: usize) -> Result<ArrayRef> {
    let bad = |v: &Value| Error::Schema(format!("field {}: {:?} cannot be written as {}", f.name, short(v), arrow_type(f)));
    Ok(match arrow_type(f) {
        DataType::Int64 => {
            let mut b = Int64Builder::with_capacity(n);
            for v in values {
                match v {
                    Value::Null => b.append_null(),
                    Value::Int(x) => b.append_value(*x),
                    Value::Bool(x) => b.append_value(*x as i64),
                    v => return Err(bad(v)),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Float64 => {
            let mut b = Float64Builder::with_capacity(n);
            for v in values {
                match v {
                    Value::Null => b.append_null(),
                    Value::Float(x) => b.append_value(*x),
                    Value::Int(x) => b.append_value(*x as f64),
                    v => return Err(bad(v)),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Boolean => {
            let mut b = BooleanBuilder::with_capacity(n);
            for v in values {
                match v {
                    Value::Null => b.append_null(),
                    Value::Bool(x) => b.append_value(*x),
                    Value::Int(x) => b.append_value(*x != 0),
                    v => return Err(bad(v)),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Timestamp(..) => {
            let mut b = TimestampMicrosecondBuilder::with_capacity(n).with_timezone("UTC");
            for v in values {
                match v {
                    Value::Null => b.append_null(),
                    Value::Int(x) | Value::Timestamp(x) => b.append_value(*x),
                    v => return Err(bad(v)),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Utf8 => {
            let mut b = StringBuilder::with_capacity(n, n * 16);
            for v in values {
                match v {
                    Value::Null => b.append_null(),
                    Value::Str(s) => b.append_value(s),
                    v => return Err(bad(v)),
                }
            }
            Arc::new(b.finish())
        }
        DataType::FixedSizeList(_, dim) => {
            let mut b = FixedSizeListBuilder::with_capacity(Float32Builder::with_capacity(n * dim as usize), dim, n);
            for v in values {
                append_vector(&mut b, v, dim as usize).map_err(|()| bad(v))?;
            }
            Arc::new(b.finish())
        }
        DataType::List(item) => match item.data_type() {
            DataType::Int64 => {
                let mut b = ListBuilder::with_capacity(Int64Builder::new(), n);
                for v in values {
                    match v {
                        Value::Null => b.append(false),
                        Value::List(xs) => {
                            for x in xs {
                                match x {
                                    Value::Int(i) => b.values().append_value(*i),
                                    _ => return Err(bad(v)),
                                }
                            }
                            b.append(true);
                        }
                        Value::Int(i) => {
                            b.values().append_value(*i);
                            b.append(true);
                        }
                        v => return Err(bad(v)),
                    }
                }
                Arc::new(b.finish())
            }
            DataType::Utf8 => {
                let mut b = ListBuilder::with_capacity(StringBuilder::new(), n);
                for v in values {
                    match v {
                        Value::Null => b.append(false),
                        Value::List(xs) => {
                            for x in xs {
                                match x {
                                    Value::Str(s) => b.values().append_value(s),
                                    _ => return Err(bad(v)),
                                }
                            }
                            b.append(true);
                        }
                        Value::Str(s) => {
                            b.values().append_value(s);
                            b.append(true);
                        }
                        v => return Err(bad(v)),
                    }
                }
                Arc::new(b.finish())
            }
            DataType::FixedSizeList(_, dim) => {
                let dim = *dim;
                let mut b = ListBuilder::with_capacity(FixedSizeListBuilder::new(Float32Builder::new(), dim), n);
                for v in values {
                    match v {
                        Value::Null => b.append(false),
                        Value::List(xs) => {
                            for x in xs {
                                append_vector(b.values(), x, dim as usize).map_err(|()| bad(v))?;
                            }
                            b.append(true);
                        }
                        v => return Err(bad(v)),
                    }
                }
                Arc::new(b.finish())
            }
            t => return Err(Error::Schema(format!("field {}: no Arrow list of {t}", f.name))),
        },
        t => return Err(Error::Schema(format!("field {}: no Arrow column of {t}", f.name))),
    })
}

fn append_vector(b: &mut FixedSizeListBuilder<Float32Builder>, v: &Value, dim: usize) -> std::result::Result<(), ()> {
    match v {
        Value::Null => {
            for _ in 0..dim {
                b.values().append_null();
            }
            b.append(false);
        }
        Value::Vector(x) if x.len() == dim => {
            b.values().append_slice(x);
            b.append(true);
        }
        _ => return Err(()),
    }
    Ok(())
}

/// Reads record batches as rows of a table: each column into the field it
/// fills, fields without a column taking their defaults. Generated fields
/// are left for the caller to fill in ([`Schema::fill_generated`]), after
/// any values it sets itself.
pub struct RowReader {
    schema: Schema,
    /// For each column of the batches, the field it fills (`None`: none).
    columns: Vec<Option<usize>>,
    start: Document,
    /// By field: the NaN and infinite numbers read as NULL.
    pub nonfinite: Vec<u64>,
}

impl RowReader {
    /// A reader of batches of `arrow` into rows of `schema`, a column
    /// filling the field of its name (in any case).
    pub fn new(schema: &Schema, arrow: &ArrowSchema) -> Result<RowReader> {
        let columns = arrow.fields().iter().map(|c| schema.field_index(c.name()).or_else(|| schema.fields.iter().position(|f| f.name.eq_ignore_ascii_case(c.name())))).collect();
        RowReader::with_columns(schema, columns)
    }

    /// A reader of batches whose column `i` fills field `columns[i]`.
    pub fn with_columns(schema: &Schema, columns: Vec<Option<usize>>) -> Result<RowReader> {
        let start = if schema.has_defaults() { schema.defaults(crate::timestamp::now())? } else { vec![Value::Null; schema.fields.len()] };
        Ok(RowReader { schema: schema.clone(), columns, start, nonfinite: vec![0; schema.fields.len()] })
    }

    /// The columns of `arrow` that fill no field.
    pub fn unknown(&self, arrow: &ArrowSchema) -> Vec<String> {
        arrow.fields().iter().zip(&self.columns).filter(|(_, f)| f.is_none()).map(|(c, _)| c.name().clone()).collect()
    }

    /// The rows of `batch`, each the reader's start (defaults or NULLs)
    /// with the values of its columns. An error names the row, counting
    /// from `first`, and the field.
    pub fn read(&mut self, batch: &RecordBatch, first: u64) -> Result<Vec<Document>> {
        let n = batch.num_rows();
        let mut docs = vec![self.start.clone(); n];
        for (c, f) in self.columns.iter().enumerate() {
            let Some(f) = *f else { continue };
            let Some(a) = batch.columns().get(c) else { continue };
            let field = &self.schema.fields[f];
            let key = self.schema.primary_key == Some(f);
            let (values, nonfinite) = values(field, key, a.as_ref()).map_err(|(row, e)| Error::Schema(format!("row {}: {}", first + row as u64 + 1, message(e))))?;
            self.nonfinite[f] += nonfinite;
            for (d, v) in docs.iter_mut().zip(values) {
                d[f] = v;
            }
        }
        Ok(docs)
    }
}

/// What [`import_batches`] wrote.
#[derive(Clone, Debug, Default)]
pub struct Written {
    pub rows: u64,
    /// Whether the rows were bulk-loaded (the table was empty) rather than
    /// upserted.
    pub bulk: bool,
    /// The columns that fill no field, left out.
    pub skipped: Vec<String>,
    /// The NaN and infinite numbers read as NULL ([`nonfinite_notes`]).
    pub notes: Vec<String>,
}

/// Writes record batches of columns `arrow` into the table `table` of
/// `db`, each column filling the field of its name (in any case; the
/// others are left out): into an empty table in one bulk load (all or
/// nothing), unless the database numbers its writes (a bulk load would
/// reach no replica); else upserted by key (inserted, in a table without one),
/// [`ingest::IMPORT_BATCH`] rows a transaction. Embedded vectors the rows
/// leave out are computed when `db` has the field's model.
pub fn import_batches(db: &crate::db::Db, table: &str, arrow: &ArrowSchema, batches: &mut dyn Iterator<Item = Result<RecordBatch>>) -> Result<Written> {
    let schema = db.storage().table(table).ok_or_else(|| Error::Query(format!("no such table: {table}")))?;
    let mut reader = RowReader::new(&schema, arrow)?;
    let skipped = reader.unknown(arrow);
    let bulk = db.storage().seq_no().is_none() && db.snapshot().table(&schema.table).is_none_or(|s| s.n_live_docs() == 0);
    let mut first = 0u64;
    let mut ready = Vec::new().into_iter();
    let mut docs = std::iter::from_fn(|| loop {
        if let Some(d) = ready.next() {
            return Some(Ok(d));
        }
        let b = match batches.next()? {
            Ok(b) => b,
            Err(e) => return Some(Err(e)),
        };
        let at = first;
        first += b.num_rows() as u64;
        match reader.read(&b, at) {
            Ok(mut d) => {
                d.iter_mut().for_each(|d| schema.fill_generated(d));
                ready = d.into_iter();
            }
            Err(e) => return Some(Err(e)),
        }
    });
    let rows = if bulk {
        let mut n = 0u64;
        db.try_load_into(&schema.table, docs.by_ref().inspect(|d| n += d.is_ok() as u64))?;
        n
    } else {
        write_docs(db, &schema.table, schema.primary_key.is_some(), &mut docs)?
    };
    let notes = nonfinite_notes(&schema, &reader.nonfinite);
    Ok(Written { rows, bulk, skipped, notes })
}

/// Writes `docs` into a table that has rows, [`ingest::IMPORT_BATCH`] a
/// transaction: upserted by key, or inserted in a table without one.
pub(crate) fn write_docs(db: &crate::db::Db, table: &str, keyed: bool, docs: &mut dyn Iterator<Item = Result<Document>>) -> Result<u64> {
    let mut n = 0;
    loop {
        let mut batch = (&mut *docs).take(ingest::IMPORT_BATCH).collect::<Result<Vec<_>>>()?;
        if batch.is_empty() {
            return Ok(n);
        }
        db.fill_in(table, &mut batch)?;
        n += batch.len() as u64;
        let mut txn = db.storage().begin()?;
        let mut t = txn.table(table)?;
        for d in batch {
            if keyed {
                t.upsert(d)?;
            } else {
                t.insert(d)?;
            }
        }
        txn.commit()?;
    }
}

/// The notes on NaN and infinite numbers read as NULL, given their counts
/// by field ([`RowReader::nonfinite`]).
pub fn nonfinite_notes(schema: &Schema, counts: &[u64]) -> Vec<String> {
    schema.fields.iter().zip(counts).filter(|(_, &n)| n > 0).map(|(f, n)| format!("{}: {n} NaN or infinite values read as NULL", f.name)).collect()
}

/// The values of column `a` for `field`, and how many NaN or infinite
/// numbers were read as NULL; or the row of the first value that does not
/// fit, with the error.
pub fn values(field: &Field, key: bool, a: &dyn Array) -> std::result::Result<(Vec<Value>, u64), (usize, Error)> {
    let whole = |e: Error| (0usize, e);
    // Dictionaries as their values, decimals as doubles (or integers).
    if let DataType::Dictionary(_, v) = a.data_type() {
        let plain = cast(a, v).map_err(|e| whole(Error::Schema(format!("field {}: {e}", field.name))))?;
        return values(field, key, plain.as_ref());
    }
    if matches!(a.data_type(), DataType::Decimal32(..) | DataType::Decimal64(..) | DataType::Decimal128(..) | DataType::Decimal256(..)) {
        let integral = matches!(a.data_type(), DataType::Decimal32(_, 0) | DataType::Decimal64(_, 0) | DataType::Decimal128(_, 0) | DataType::Decimal256(_, 0));
        let to = if integral && field.ty == FieldType::Int && !field.multi { DataType::Int64 } else { DataType::Float64 };
        let plain = cast(a, &to).map_err(|e| whole(Error::Schema(format!("field {}: {e}", field.name))))?;
        return values(field, key, plain.as_ref());
    }
    let mut out = Vec::with_capacity(a.len());
    let mut nonfinite = 0u64;
    for i in 0..a.len() {
        let v = if a.is_null(i) { Ok(Value::Null) } else { value(field, key, a, i, &mut nonfinite) };
        out.push(v.map_err(|e| (i, e))?);
    }
    Ok((out, nonfinite))
}

/// The value at row `i` of `a`, which is not NULL, for `field`.
fn value(field: &Field, key: bool, a: &dyn Array, i: usize, nonfinite: &mut u64) -> Result<Value> {
    let bad = || Error::Schema(format!("bad value for {}: {} ({})", field.name, short_json(&json_of(a, i)), a.data_type()));
    let by_json = || ingest::field_value(field, key, &json_of(a, i));
    if field.is_multi_vector() {
        let FieldType::Vector { dim, .. } = field.ty else { unreachable!("a vector field") };
        return match list_at(a, i) {
            Some(items) => Ok(Value::List((0..items.len()).map(|j| vector_at(items.as_ref(), j, dim).ok_or_else(bad).map(|v| Value::Vector(v.into()))).collect::<Result<_>>()?)),
            None => by_json(),
        };
    }
    if let FieldType::Vector { dim, .. } = field.ty {
        return match vector_at(a, i, dim) {
            Some(v) => Ok(Value::Vector(v.into())),
            None if is_text(a.data_type()) => by_json_text(field, key, a, i),
            None => Err(bad()),
        };
    }
    if field.multi {
        let items = match list_at(a, i) {
            Some(items) => items,
            None if is_text(a.data_type()) => return by_json_text(field, key, a, i),
            None => return Ok(Value::List(vec![item(field, a, i).ok_or_else(bad)?])),
        };
        let mut xs = Vec::with_capacity(items.len());
        for j in 0..items.len() {
            if !items.is_null(j) {
                xs.push(item(field, items.as_ref(), j).ok_or_else(bad)?);
            }
        }
        return Ok(Value::List(xs));
    }
    match field.logical {
        Some(Logical::Bool) => {
            return match a.data_type() {
                DataType::Boolean => Ok(Value::Bool(a.as_boolean().value(i))),
                t if t.is_integer() => int_at(a, i).map(Value::Int).ok_or_else(bad),
                t if is_text(t) => Ok(Value::Str(str_at(a, i).unwrap_or_default().to_string())),
                _ => Err(bad()),
            };
        }
        Some(Logical::Timestamp) => return micros_at(a, i).map(Value::Timestamp).or_else(|| is_text(a.data_type()).then(|| Value::Str(str_at(a, i).unwrap_or_default().to_string()))).ok_or_else(bad),
        None => {}
    }
    if field.media.is_some() {
        return by_json();
    }
    Ok(match &field.ty {
        FieldType::Int => match a.data_type() {
            t if t.is_integer() => Value::Int(int_at(a, i).ok_or_else(|| Error::Schema(format!("bad value for {}: {} is past the largest INT", field.name, short_json(&json_of(a, i)))))?),
            t if t.is_floating() => {
                let x = float_at(a, i).unwrap_or(f64::NAN);
                if x.fract() != 0.0 || !x.is_finite() || x.abs() >= 9.2e18 {
                    return Err(bad());
                }
                Value::Int(x as i64)
            }
            DataType::Boolean => Value::Int(a.as_boolean().value(i) as i64),
            t if is_text(t) => Value::Int(str_at(a, i).unwrap_or_default().trim().parse().map_err(|_| bad())?),
            _ => return Err(bad()),
        },
        FieldType::Float => match a.data_type() {
            t if t.is_integer() || t.is_floating() => {
                let x = float_at(a, i).ok_or_else(bad)?;
                if !x.is_finite() {
                    *nonfinite += 1;
                    return Ok(Value::Null);
                }
                Value::Float(x)
            }
            DataType::Boolean => Value::Float(a.as_boolean().value(i) as i64 as f64),
            t if is_text(t) => Value::Float(str_at(a, i).unwrap_or_default().trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(bad)?),
            _ => return Err(bad()),
        },
        FieldType::Text { .. } | FieldType::Keyword => match a.data_type() {
            t if is_text(t) => Value::Str(str_at(a, i).unwrap_or_default().to_string()),
            t if is_binary(t) => Value::Str(base64(binary_at(a, i).unwrap_or_default())),
            _ => match json_of(a, i) {
                Json::String(s) => Value::Str(s),
                Json::Null => Value::Null,
                j @ (Json::Array(_) | Json::Object(_)) => Value::Str(j.to_string()),
                j => Value::Str(j.to_string()),
            },
        },
        FieldType::Json | FieldType::Sparse | FieldType::Edges | FieldType::Vector { .. } => by_json()?,
    })
}

/// A value of text that holds JSON (a vector's or a list's) for `field`.
fn by_json_text(field: &Field, key: bool, a: &dyn Array, i: usize) -> Result<Value> {
    let s = str_at(a, i).unwrap_or_default();
    let j = match serde_json::from_str::<Json>(s.trim()) {
        Ok(j @ Json::Array(_)) => j,
        _ => Json::String(s.to_string()),
    };
    ingest::field_value(field, key, &j)
}

/// One value of a multi-valued KEYWORD or INT field from row `i` of `a`.
fn item(field: &Field, a: &dyn Array, i: usize) -> Option<Value> {
    match field.ty {
        FieldType::Int => match a.data_type() {
            t if t.is_integer() => int_at(a, i).map(Value::Int),
            t if t.is_floating() => float_at(a, i).filter(|x| x.fract() == 0.0 && x.abs() < 9.2e18).map(|x| Value::Int(x as i64)),
            t if is_text(t) => str_at(a, i)?.trim().parse().ok().map(Value::Int),
            _ => None,
        },
        _ => match json_of(a, i) {
            Json::String(s) => Some(Value::Str(s)),
            Json::Number(n) => Some(Value::Str(n.to_string())),
            Json::Bool(b) => Some(Value::Str(b.to_string())),
            _ => None,
        },
    }
}

fn is_text(t: &DataType) -> bool {
    matches!(t, DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View)
}

fn is_binary(t: &DataType) -> bool {
    matches!(t, DataType::Binary | DataType::LargeBinary | DataType::BinaryView | DataType::FixedSizeBinary(_))
}

fn str_at(a: &dyn Array, i: usize) -> Option<&str> {
    match a.data_type() {
        DataType::Utf8 => Some(a.as_string::<i32>().value(i)),
        DataType::LargeUtf8 => Some(a.as_string::<i64>().value(i)),
        DataType::Utf8View => Some(a.as_string_view().value(i)),
        _ => None,
    }
}

fn binary_at(a: &dyn Array, i: usize) -> Option<&[u8]> {
    match a.data_type() {
        DataType::Binary => Some(a.as_binary::<i32>().value(i)),
        DataType::LargeBinary => Some(a.as_binary::<i64>().value(i)),
        DataType::BinaryView => Some(a.as_binary_view().value(i)),
        DataType::FixedSizeBinary(_) => Some(a.as_fixed_size_binary().value(i)),
        _ => None,
    }
}

/// An integer at row `i`; `None` past `i64`.
fn int_at(a: &dyn Array, i: usize) -> Option<i64> {
    Some(match a.data_type() {
        DataType::Int8 => a.as_primitive::<Int8Type>().value(i) as i64,
        DataType::Int16 => a.as_primitive::<Int16Type>().value(i) as i64,
        DataType::Int32 => a.as_primitive::<Int32Type>().value(i) as i64,
        DataType::Int64 => a.as_primitive::<Int64Type>().value(i),
        DataType::UInt8 => a.as_primitive::<UInt8Type>().value(i) as i64,
        DataType::UInt16 => a.as_primitive::<UInt16Type>().value(i) as i64,
        DataType::UInt32 => a.as_primitive::<UInt32Type>().value(i) as i64,
        DataType::UInt64 => i64::try_from(a.as_primitive::<UInt64Type>().value(i)).ok()?,
        _ => return None,
    })
}

fn float_at(a: &dyn Array, i: usize) -> Option<f64> {
    Some(match a.data_type() {
        DataType::Float16 => a.as_primitive::<Float16Type>().value(i).to_f64(),
        DataType::Float32 => a.as_primitive::<Float32Type>().value(i) as f64,
        DataType::Float64 => a.as_primitive::<Float64Type>().value(i),
        DataType::UInt64 => a.as_primitive::<UInt64Type>().value(i) as f64,
        t if t.is_integer() => int_at(a, i)? as f64,
        _ => return None,
    })
}

/// A date or timestamp at row `i` as microseconds since the epoch (a
/// timestamp's time zone, if it has one, already counted); an integer as
/// Unix time in seconds up to 10^11 (the year 5138), else in milliseconds
/// up to 10^14, else microseconds, as the SQLite import reads them.
fn micros_at(a: &dyn Array, i: usize) -> Option<i64> {
    match a.data_type() {
        DataType::Timestamp(TimeUnit::Second, _) => a.as_primitive::<TimestampSecondType>().value(i).checked_mul(1_000_000),
        DataType::Timestamp(TimeUnit::Millisecond, _) => a.as_primitive::<TimestampMillisecondType>().value(i).checked_mul(1_000),
        DataType::Timestamp(TimeUnit::Microsecond, _) => Some(a.as_primitive::<TimestampMicrosecondType>().value(i)),
        DataType::Timestamp(TimeUnit::Nanosecond, _) => Some(a.as_primitive::<TimestampNanosecondType>().value(i).div_euclid(1_000)),
        DataType::Date32 => (a.as_primitive::<Date32Type>().value(i) as i64).checked_mul(DAY_MICROS),
        DataType::Date64 => a.as_primitive::<Date64Type>().value(i).checked_mul(1_000),
        t if t.is_integer() => {
            let x = int_at(a, i)?;
            match x.unsigned_abs() {
                0..100_000_000_000 => x.checked_mul(1_000_000),
                100_000_000_000..100_000_000_000_000 => x.checked_mul(1_000),
                _ => Some(x),
            }
        }
        _ => None,
    }
}

/// The values of the list at row `i`, if `a` is a list.
fn list_at(a: &dyn Array, i: usize) -> Option<ArrayRef> {
    match a.data_type() {
        DataType::List(_) => Some(a.as_list::<i32>().value(i)),
        DataType::LargeList(_) => Some(a.as_list::<i64>().value(i)),
        DataType::FixedSizeList(..) => Some(a.as_fixed_size_list().value(i)),
        DataType::ListView(_) => Some(a.as_list_view::<i32>().value(i)),
        DataType::LargeListView(_) => Some(a.as_list_view::<i64>().value(i)),
        _ => None,
    }
}

/// The vector of `dim` numbers at row `i`: a list of numbers, or `4 dim`
/// bytes of little-endian float32.
fn vector_at(a: &dyn Array, i: usize, dim: usize) -> Option<Vec<f32>> {
    if let Some(b) = binary_at(a, i) {
        return (b.len() == 4 * dim).then(|| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect());
    }
    let items = list_at(a, i)?;
    if items.len() != dim || items.null_count() > 0 {
        return None;
    }
    if let DataType::Float32 = items.data_type() {
        return Some(items.as_primitive::<Float32Type>().values().to_vec());
    }
    (0..dim).map(|j| float_at(items.as_ref(), j).map(|x| x as f32)).collect()
}

/// Row `i` of `a` as JSON: numbers, text, booleans, lists as arrays,
/// structs and maps as objects (a map's keys as text), dates and
/// timestamps as ISO 8601 text, binary values as base64 text, and other
/// values as Arrow prints them.
pub fn json_of(a: &dyn Array, i: usize) -> Json {
    if a.is_null(i) {
        return Json::Null;
    }
    let t = a.data_type();
    if t.is_integer() {
        return match t {
            DataType::UInt64 => Json::from(a.as_primitive::<UInt64Type>().value(i)),
            _ => int_at(a, i).map_or(Json::Null, Json::from),
        };
    }
    if t.is_floating() {
        return float_at(a, i).and_then(serde_json::Number::from_f64).map_or(Json::Null, Json::Number);
    }
    if let Some(s) = str_at(a, i) {
        return Json::String(s.to_string());
    }
    if let Some(b) = binary_at(a, i) {
        return Json::String(base64(b));
    }
    if let Some(items) = list_at(a, i) {
        return Json::Array((0..items.len()).map(|j| json_of(items.as_ref(), j)).collect());
    }
    match t {
        DataType::Boolean => Json::Bool(a.as_boolean().value(i)),
        DataType::Timestamp(..) | DataType::Date32 | DataType::Date64 => micros_at(a, i).map_or(Json::Null, |m| Json::String(crate::timestamp::format(m))),
        DataType::Struct(fields) => {
            let s = a.as_struct();
            Json::Object(fields.iter().zip(s.columns()).map(|(f, c)| (f.name().clone(), json_of(c.as_ref(), i))).collect())
        }
        DataType::Map(..) => {
            let m = a.as_map();
            let entries = m.value(i);
            let (keys, vals) = (entries.column(0), entries.column(1));
            let mut o = Map::new();
            for j in 0..entries.len() {
                let k = match json_of(keys.as_ref(), j) {
                    Json::String(s) => s,
                    k => k.to_string(),
                };
                o.insert(k, json_of(vals.as_ref(), j));
            }
            Json::Object(o)
        }
        DataType::Dictionary(_, v) => match cast(&a.slice(i, 1), v) {
            Ok(plain) => json_of(plain.as_ref(), 0),
            Err(_) => Json::Null,
        },
        DataType::Decimal32(..) | DataType::Decimal64(..) | DataType::Decimal128(..) | DataType::Decimal256(..) => match cast(&a.slice(i, 1), &DataType::Float64) {
            Ok(x) => json_of(x.as_ref(), 0),
            Err(_) => Json::Null,
        },
        _ => match ArrayFormatter::try_new(a, &FormatOptions::default()) {
            Ok(f) => Json::String(f.value(i).to_string()),
            Err(_) => Json::Null,
        },
    }
}

fn base64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn message(e: Error) -> String {
    match e {
        Error::Schema(m) | Error::Query(m) => m,
        e => e.to_string(),
    }
}

fn short(v: &Value) -> String {
    let s = format!("{v:?}");
    match s.char_indices().nth(60) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s,
    }
}

fn short_json(j: &Json) -> String {
    let s = j.to_string();
    match s.char_indices().nth(60) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s,
    }
}
