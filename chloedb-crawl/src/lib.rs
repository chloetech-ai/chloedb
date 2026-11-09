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
pub mod robots;
pub mod strata;
pub mod urls;

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
