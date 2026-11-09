//! The API's header names under both spellings.
//!
//! ChloeDB was called Strata, and the headers a client sets are named
//! `x-strata-*`: `x-strata-client`, `x-strata-app`, `x-strata-reason` and
//! `x-strata-confirm` ([`ALIASED`]). A request may now name each one
//! `x-chloedb-*` as well: [`alias_headers`] and [`alias_metadata`] copy it
//! to its `x-strata-*` name, unless the request sets that one too, before
//! anything reads them. The code goes on reading the old names, and
//! answers still carry only the old names.
//!
//! Only these four: a credential (`x-strata-authorization`) and the headers
//! servers send each other (`x-strata-via`, `x-strata-peer`, the
//! coordinator's metadata) keep their one name, so that renaming adds no
//! new way in and no header that a log would print unredacted.
//!
//! [`http_layer`] puts the copy in front of every HTTP router a server
//! serves (the API, the UI and the OpenSearch API); the gRPC services do
//! it in their check ([`crate::server::Check`]).

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName};

/// The prefix headers were named by before the rename.
pub const OLD: &str = "x-strata-";
/// The prefix headers may be named by now.
pub const NEW: &str = "x-chloedb-";

/// The headers a client sets, which may be named either way (after the
/// prefix).
pub const ALIASED: [&str; 4] = ["client", "app", "reason", "confirm"];

/// The old name of a header named the new way: `x-strata-client` for
/// `x-chloedb-client`; `None` for any name not in [`ALIASED`].
pub fn old_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix(NEW)?;
    ALIASED.contains(&rest).then(|| format!("{OLD}{rest}"))
}

/// Copies each `x-chloedb-*` header to its `x-strata-*` name, unless that
/// one is set already (then the request's own `x-strata-*` value stands).
pub fn alias_headers(headers: &mut HeaderMap) {
    let new: Vec<(HeaderName, Vec<axum::http::HeaderValue>)> = headers
        .keys()
        .filter_map(|k| {
            let old = HeaderName::from_bytes(old_name(k.as_str())?.as_bytes()).ok()?;
            (!headers.contains_key(&old)).then(|| (old, headers.get_all(k).iter().cloned().collect()))
        })
        .collect();
    for (old, values) in new {
        for v in values {
            headers.append(old.clone(), v);
        }
    }
}

/// [`alias_headers`] over a gRPC call's metadata (its ASCII entries; no
/// header of ours is binary).
pub fn alias_metadata(md: &mut tonic::metadata::MetadataMap) {
    let new: Vec<(tonic::metadata::MetadataKey<tonic::metadata::Ascii>, tonic::metadata::MetadataValue<tonic::metadata::Ascii>)> = md
        .keys()
        .filter_map(|k| match k {
            tonic::metadata::KeyRef::Ascii(k) => {
                let old = tonic::metadata::MetadataKey::from_bytes(old_name(k.as_str())?.as_bytes()).ok()?;
                (!md.contains_key(&old)).then(|| md.get(k).cloned().map(|v| (old, v)))?
            }
            tonic::metadata::KeyRef::Binary(_) => None,
        })
        .collect();
    for (old, v) in new {
        md.insert(old, v);
    }
}

async fn alias_request(mut req: Request) -> Request {
    alias_headers(req.headers_mut());
    req
}

/// [`alias_headers`] on every request a router takes, before its own
/// layers (credentials among them) read a header.
pub fn http_layer(router: axum::Router) -> axum::Router {
    router.layer(axum::middleware::map_request(alias_request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_names_are_copied_to_old_ones_unless_set() {
        assert_eq!(old_name("x-chloedb-client").as_deref(), Some("x-strata-client"));
        assert_eq!(old_name("x-chloedb-"), None);
        // A credential and the servers' own headers have one name only.
        assert_eq!(old_name("x-chloedb-authorization"), None);
        assert_eq!(old_name("x-chloedb-peer"), None);
        assert_eq!(old_name("x-strata-client"), None);
        assert_eq!(old_name("authorization"), None);

        let mut h = HeaderMap::new();
        h.insert("x-chloedb-client", "command-center".parse().unwrap());
        h.insert("x-chloedb-app", "new".parse().unwrap());
        h.insert("x-chloedb-authorization", "Bearer k".parse().unwrap());
        h.insert("x-strata-app", "old".parse().unwrap());
        h.append("x-chloedb-reason", "a".parse().unwrap());
        h.append("x-chloedb-reason", "b".parse().unwrap());
        alias_headers(&mut h);
        assert_eq!(h.get("x-strata-client").unwrap(), "command-center");
        // A request that sets both keeps its x-strata-* value.
        assert_eq!(h.get_all("x-strata-app").iter().collect::<Vec<_>>(), ["old"]);
        assert_eq!(h.get_all("x-strata-reason").iter().collect::<Vec<_>>(), ["a", "b"]);
        assert!(h.get("x-strata-authorization").is_none());
        // The new names stay as they came.
        assert_eq!(h.get("x-chloedb-client").unwrap(), "command-center");

        let mut md = tonic::metadata::MetadataMap::new();
        md.insert("x-chloedb-app", "grpc-app".parse().unwrap());
        md.insert("x-chloedb-client", "new".parse().unwrap());
        md.insert("x-strata-client", "old".parse().unwrap());
        alias_metadata(&mut md);
        assert_eq!(md.get("x-strata-app").unwrap(), "grpc-app");
        assert_eq!(md.get("x-strata-client").unwrap(), "old");
    }
}
