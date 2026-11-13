//! chloedb-crawl: a crawler on Strata. Given a URL, it fetches the site's
//! pages (politely: robots.txt, a delay per host), reads each page's fields
//! (text, metadata, JSON-LD, Open Graph, microdata, key-value tables, fields
//! learned from the site's templates, CSS selectors), infers a table's
//! schema from a sample of them, and writes every page as a row of that
//! table, adding columns as new fields turn common. As the table grows, the
//! cluster gets more shards and its slots move onto them ([`grow`]).
//!
//! It is a client of chloedb-server's HTTP API, a program of its own: it
//! runs on its own machine (or in its own pod), against one server, a
//! cluster anywhere, or a cluster it starts on its machine (`--local`).

pub mod api;
pub mod extract;
pub mod fetch;
pub mod frontier;
pub mod grow;
pub mod infer;
pub mod job;
pub mod reach;
pub mod robots;
pub mod strata;
pub mod urls;

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
