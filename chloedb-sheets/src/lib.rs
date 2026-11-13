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

/// The listening socket chloedb-server handed this program when it
/// started it as an app (`CHLOEDB_LISTEN_FD`, file descriptor 3), so that
/// the port the server chose and holds is never free for another process
/// to take. `Ok(None)` when not started so: it binds its own. The setting
/// is taken out of the environment and the socket marked close-on-exec, so
/// that no program this one starts gets it.
pub fn inherited_listener() -> Result<Option<std::net::TcpListener>, String> {
    let Some(v) = std::env::var_os("CHLOEDB_LISTEN_FD") else { return Ok(None) };
    std::env::remove_var("CHLOEDB_LISTEN_FD");
    let fd: i32 = v.to_str().and_then(|s| s.parse().ok()).filter(|fd| *fd >= 3).ok_or_else(|| format!("CHLOEDB_LISTEN_FD={v:?} is not a file descriptor of 3 or more"))?;
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: fcntl on a descriptor number; it fails if none is open.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(format!("CHLOEDB_LISTEN_FD={fd}: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: the descriptor is open, was handed to this process to
        // own, and nothing else here holds it.
        let l = unsafe { std::net::TcpListener::from_raw_fd(fd) };
        l.local_addr().map_err(|e| format!("CHLOEDB_LISTEN_FD={fd} is not a listening socket: {e}"))?;
        Ok(Some(l))
    }
    #[cfg(not(unix))]
    Err(format!("CHLOEDB_LISTEN_FD={fd}: a socket is handed over on Unix only"))
}
