//! strata-sheets: spreadsheets kept in Strata, an app of its command center.
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
