//! chloedb-sheets: spreadsheets kept in Strata, an app of its command center.
//!
//! Workbooks are IronCalc models ([`book`]) saved as rows of two tables
//! ([`store`]); the view edits them through operations ([`ops`]) sent to
//! this API ([`api`]), which keeps the books it opened in memory
//! ([`books`]). xlsx, CSV and SQLite files come in and go out ([`io`]).

pub mod api;
pub mod book;
pub mod books;
pub mod io;
pub mod ops;
pub mod store;
pub mod strata;

/// Copies every `CHLOEDB_X` setting into `STRATA_X`, the name the code and
/// clap read: ChloeDB was called Strata, and a setting may carry either
/// name (the new one wins). As `chloedb::env::adopt_new_names`, which this
/// crate does not depend on. Call it first in `main`, before any thread.
pub fn adopt_new_env_names() {
    let new: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let rest = k.to_str()?.strip_prefix("CHLOEDB_")?;
            (!rest.is_empty()).then(|| (format!("STRATA_{rest}").into(), v))
        })
        .collect();
    for (k, v) in new {
        std::env::set_var(k, v);
    }
}
