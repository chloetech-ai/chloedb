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

/// Copies every `STRATA_X` setting into `CHLOEDB_X` when that one is not
/// set: ChloeDB was called Strata, and a setting may carry either name (the
/// new one wins). As `chloedb::env::adopt_old_names`, which this crate does
/// not depend on. Call it first in `main`, before any thread.
pub fn adopt_old_env_names() {
    let old: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let rest = k.to_str()?.strip_prefix("STRATA_")?;
            let new = format!("CHLOEDB_{rest}");
            (!rest.is_empty() && std::env::var_os(&new).is_none()).then(|| (new.into(), v))
        })
        .collect();
    for (k, v) in old {
        std::env::set_var(k, v);
    }
}
