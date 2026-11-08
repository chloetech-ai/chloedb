//! strata-crawl: a crawler on Strata. Given a URL, it fetches the site's
//! pages (politely: robots.txt, a delay per host), reads each page's fields
//! (text, metadata, JSON-LD, Open Graph, microdata, key-value tables, fields
//! learned from the site's templates, CSS selectors), infers a table's
//! schema from a sample of them, and writes every page as a row of that
//! table, adding columns as new fields turn common. As the table grows, the
//! cluster gets more shards and its slots move onto them ([`grow`]).
//!
//! It is a client of strata-server's HTTP API, a program of its own: it
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
