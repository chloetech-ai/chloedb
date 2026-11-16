use std::fmt;

/// Errors of every Strata layer.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// On-disk data failed a check (bad magic, checksum, bounds).
    Corrupt(String),
    /// A schema or document does not fit the table.
    Schema(String),
    /// A query failed to parse, bind or plan.
    Query(String),
    /// Another writer holds the database, or a transaction was misused.
    Busy(String),
    /// A query ran past its deadline.
    Timeout(String),
    /// A query needed more memory than its budget had left
    /// ([`crate::query::memory`]).
    Memory(String),
    /// A service Strata calls refused or did not answer: a hosted
    /// embedding model ([`crate::hosted`]), after its retries.
    Unavailable(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Corrupt(m) => write!(f, "corrupt database: {m}"),
            Error::Schema(m) => write!(f, "schema error: {m}"),
            Error::Query(m) => write!(f, "query error: {m}"),
            Error::Busy(m) => write!(f, "busy: {m}"),
            Error::Timeout(m) => write!(f, "timeout: {m}"),
            Error::Memory(m) => write!(f, "memory: {m}"),
            Error::Unavailable(m) => write!(f, "unavailable: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// The message without the kind the error displays it after ("schema
    /// error: "), to put in another message without saying the kind twice.
    pub fn message(&self) -> String {
        match self {
            Error::Io(e) => e.to_string(),
            Error::Corrupt(m) | Error::Schema(m) | Error::Query(m) | Error::Busy(m) | Error::Timeout(m) | Error::Memory(m) | Error::Unavailable(m) => m.clone(),
        }
    }

    /// This error of the same kind, its message after `context`: "schema
    /// error: row 3: no field nope", not "schema error: row 3: schema
    /// error: no field nope".
    pub fn within(self, context: &str) -> Error {
        match self {
            Error::Io(e) => Error::Io(std::io::Error::new(e.kind(), format!("{context}: {e}"))),
            Error::Corrupt(m) => Error::Corrupt(format!("{context}: {m}")),
            Error::Schema(m) => Error::Schema(format!("{context}: {m}")),
            Error::Query(m) => Error::Query(format!("{context}: {m}")),
            Error::Busy(m) => Error::Busy(format!("{context}: {m}")),
            Error::Timeout(m) => Error::Timeout(format!("{context}: {m}")),
            Error::Memory(m) => Error::Memory(format!("{context}: {m}")),
            Error::Unavailable(m) => Error::Unavailable(format!("{context}: {m}")),
        }
    }

    /// The error's stable code, as the server's answers and the Python and
    /// JavaScript clients name it: `conflict` for a row whose primary key
    /// the table holds already (a `Schema` error whose message starts with
    /// "duplicate primary key"), `bad_request` for another schema or query
    /// error, and `busy`, `timeout`, `memory`, `unavailable`, `corrupt` or
    /// `io` by the kind.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Schema(m) if m.starts_with("duplicate primary key") => "conflict",
            Error::Schema(_) | Error::Query(_) => "bad_request",
            Error::Busy(_) => "busy",
            Error::Timeout(_) => "timeout",
            Error::Memory(_) => "memory",
            Error::Unavailable(_) => "unavailable",
            Error::Corrupt(_) => "corrupt",
            Error::Io(_) => "io",
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(any(feature = "sqlite", feature = "baselines"))]
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Query(format!("sqlite: {e}"))
    }
}

/// Shorthand for `Error::Corrupt` from anything printable.
pub fn corrupt(m: impl fmt::Display) -> Error {
    Error::Corrupt(m.to_string())
}
