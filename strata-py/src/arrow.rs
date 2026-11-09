//! Answers as Arrow (feature `datafusion`): a query's rows as one Arrow
//! record batch, which Python libraries take through the Arrow PyCapsule
//! interface (`__arrow_c_stream__`) without copying it: pyarrow, polars,
//! DuckDB, and pandas through pyarrow.
//!
//! A column has the Arrow type of its values, as Strata's DataFusion
//! engine types them ([`strata::query::arrow`]): INT as Int64, FLOAT as
//! Float64, text and keywords as Utf8, BOOL as Boolean, TIMESTAMP as
//! microseconds in UTC, a vector as a list of Float32 (a multi-vector
//! field's as a list of those), a list field as a list of Int64 or Utf8,
//! and a column of NULLs as Null. A column of both
//! integers and floats is Float64; values of other types mixed in one
//! column are an error, as Arrow holds one type a column.

use std::ffi::CStr;

use arrow_array::ffi_stream::FFI_ArrowArrayStream;
use arrow_array::{RecordBatch, RecordBatchIterator};
use arrow_schema::ffi::FFI_ArrowSchema;
use pyo3::prelude::*;
use pyo3::types::PyCapsule;
use strata::query::arrow::answer_batch;
use strata::query::exec::Leaf;

use crate::StrataError;

const STREAM: &CStr = c"arrow_array_stream";
const SCHEMA: &CStr = c"arrow_schema";

/// A query's answer as Arrow columns.
#[pyclass(frozen, module = "strata._strata")]
pub struct Arrow {
    batch: RecordBatch,
    /// What the engine did, as JSON text.
    #[pyo3(get)]
    stats: String,
}

impl Arrow {
    /// A query's answer ([`strata::db::Db::query_leaf`]) as Arrow columns,
    /// each named as the query names it ([`strata::query::arrow::answer_batch`]).
    pub fn of(leaf: Leaf) -> strata::Result<Arrow> {
        let (batch, stats) = answer_batch(leaf)?;
        Ok(Arrow { batch, stats: serde_json::to_string(&stats).unwrap_or_default() })
    }
}

#[pymethods]
impl Arrow {
    /// The columns' names, in order.
    #[getter]
    fn columns(&self) -> Vec<String> {
        self.batch.schema().fields().iter().map(|f| f.name().clone()).collect()
    }

    #[getter]
    fn num_rows(&self) -> usize {
        self.batch.num_rows()
    }

    fn __len__(&self) -> usize {
        self.batch.num_rows()
    }

    /// The rows as an Arrow C stream of one record batch, in a capsule
    /// (the Arrow PyCapsule interface). The batch's buffers are shared,
    /// not copied: each call gives a new stream of the same batch. A
    /// requested schema is not followed.
    #[pyo3(signature = (requested_schema = None))]
    fn __arrow_c_stream__<'py>(&self, py: Python<'py>, requested_schema: Option<Bound<'py, PyAny>>) -> PyResult<Bound<'py, PyCapsule>> {
        let _ = requested_schema;
        let reader = RecordBatchIterator::new([Ok(self.batch.clone())], self.batch.schema());
        PyCapsule::new_with_value(py, FFI_ArrowArrayStream::new(Box::new(reader)), STREAM)
    }

    /// The columns' Arrow schema, in a capsule.
    fn __arrow_c_schema__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyCapsule>> {
        let schema = FFI_ArrowSchema::try_from(self.batch.schema().as_ref()).map_err(|e| StrataError::new_err(e.to_string()))?;
        PyCapsule::new_with_value(py, schema, SCHEMA)
    }

    fn __repr__(&self) -> String {
        let n = self.batch.num_rows();
        format!("<strata.ArrowResult {n} row{}: {}>", if n == 1 { "" } else { "s" }, self.columns().join(", "))
    }
}

/// The Arrow stream `data` gives through the Arrow PyCapsule interface
/// (`__arrow_c_stream__`): a pyarrow Table, RecordBatch or
/// RecordBatchReader, a polars DataFrame, a DuckDB relation, a
/// `query_arrow` answer..
#[cfg(feature = "parquet")]
pub fn stream_of(data: &Bound<'_, PyAny>) -> PyResult<arrow_array::ffi_stream::ArrowArrayStreamReader> {
    use pyo3::exceptions::PyTypeError;
    if !data.hasattr("__arrow_c_stream__")? {
        return Err(PyTypeError::new_err(format!("{} gives no Arrow stream (__arrow_c_stream__): pass a pyarrow Table, RecordBatch or RecordBatchReader, a polars DataFrame or a DuckDB relation", data.get_type().name()?)));
    }
    let capsule = data.call_method1("__arrow_c_stream__", (data.py().None(),))?;
    let capsule = capsule.cast::<PyCapsule>().map_err(|_| PyTypeError::new_err("__arrow_c_stream__ gave no capsule"))?;
    let ptr = capsule.pointer_checked(Some(STREAM))?.as_ptr().cast::<FFI_ArrowArrayStream>();
    // The stream is moved out, and the capsule left a released one, which
    // its destructor does nothing with (as arrow's pyarrow module does).
    let stream = unsafe { FFI_ArrowArrayStream::from_raw(ptr) };
    arrow_array::ffi_stream::ArrowArrayStreamReader::try_new(stream).map_err(|e| StrataError::new_err(format!("the Arrow stream: {e}")))
}
