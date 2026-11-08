//! URLs as the crawler keeps them: one spelling per page (normalised), the
//! hash the seen set holds, and whether a link is in a crawl's scope.

use serde::{Deserialize, Serialize};
use url::Url;

/// Query parameters that only track where a click came from: dropped, so
/// that a page linked with them is the page without them.
const TRACKING: &[&str] = &["fbclid", "gclid", "dclid", "msclkid", "mc_cid", "mc_eid", "_ga", "_gl", "igshid", "yclid", "ref_src"];

/// Paths that end in these are never HTML: not fetched.
const NOT_PAGES: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "svg", "ico", "bmp", "tif", "tiff", "avif", "mp3", "mp4", "m4a", "m4v", "mov", "avi", "webm", "ogg", "wav", "flac", "pdf",
    "zip", "gz", "tgz", "bz2", "xz", "7z", "rar", "tar", "dmg", "exe", "msi", "deb", "rpm", "apk", "iso", "bin", "css", "js", "mjs", "json", "xml", "rss", "atom",
    "woff", "woff2", "ttf", "otf", "eot", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "txt", "wasm", "map",
];

/// One spelling of a URL: http(s) only, no fragment, no default port, no
/// tracking parameters, the query's parameters sorted, and `/` for an
/// empty path. `None` for anything that is not a web page's address.
pub fn normalize(u: &Url) -> Option<Url> {
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    u.host_str()?;
    // (The url crate drops a scheme's default port as it parses.)
    let mut u = u.clone();
    u.set_fragment(None);
    if u.query().is_some() {
        let mut pairs: Vec<(String, String)> =
            u.query_pairs().filter(|(k, _)| !k.starts_with("utm_") && !TRACKING.contains(&k.as_ref())).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        pairs.sort();
        if pairs.is_empty() {
            u.set_query(None);
        } else {
            u.query_pairs_mut().clear().extend_pairs(pairs);
        }
    }
    if u.path().is_empty() {
        u.set_path("/");
    }
    Some(u)
}

/// `href` read against the page at `base`, normalised.
pub fn join(base: &Url, href: &str) -> Option<Url> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") || href.starts_with("tel:") || href.starts_with("data:") {
        return None;
    }
    normalize(&base.join(href).ok()?)
}

/// Whether the path names a file that is not a page (an image, an archive).
pub fn not_a_page(u: &Url) -> bool {
    let p = u.path();
    let last = p.rsplit('/').next().unwrap_or("");
    match last.rsplit_once('.') {
        Some((_, ext)) => NOT_PAGES.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// The hash the seen set keeps of a URL.
pub fn hash(u: &str) -> u64 {
    twox_hash::XxHash3_64::oneshot(u.as_bytes())
}

/// Which links a crawl follows, from its seeds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The seeds' hosts only.
    #[default]
    Host,
    /// The seeds' sites: their hosts and every subdomain of the site
    /// (`www.example.com` and `shop.example.com` for `example.com`).
    Domain,
    /// Under the seeds' paths: `https://site/docs/` and below.
    Prefix,
    /// Anywhere.
    Any,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "host" => Some(Scope::Host),
            "domain" | "site" => Some(Scope::Domain),
            "prefix" | "path" => Some(Scope::Prefix),
            "any" | "web" => Some(Scope::Any),
            _ => None,
        }
    }
}

/// The site a host belongs to, as far as the crawler tells: its last two
/// labels, or three when the second last is short (`example.co.uk`).
/// Without the public suffix list, so an approximation.
pub fn site(host: &str) -> String {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host.to_string();
    }
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return host.to_string();
    }
    let keep = if labels[n - 2].len() <= 3 && labels[n - 1].len() == 2 { 3 } else { 2 };
    labels[n.saturating_sub(keep)..].join(".")
}

/// Whether `u` is in the scope of the seeds.
pub fn in_scope(scope: Scope, seeds: &[Url], u: &Url) -> bool {
    let host = u.host_str().unwrap_or("");
    match scope {
        Scope::Any => true,
        Scope::Host => seeds.iter().any(|s| s.host_str() == Some(host)),
        Scope::Domain => {
            let site_of = site(host);
            seeds.iter().any(|s| s.host_str().map(site) == Some(site_of.clone()))
        }
        Scope::Prefix => seeds.iter().any(|s| {
            if s.host_str() != Some(host) || s.scheme() != u.scheme() {
                return false;
            }
            let dir = match s.path().rfind('/') {
                Some(i) => &s.path()[..=i],
                None => "/",
            };
            u.path().starts_with(dir)
        }),
    }
}

/// A name for a table from a seed: its host, as a SQL name
/// (`books.toscrape.com` is `books_toscrape_com`).
pub fn table_name(u: &Url) -> String {
    let host = u.host_str().unwrap_or("pages").trim_start_matches("www.");
    let mut s: String = host.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert_str(0, "site_");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize(&u("HTTPS://Example.COM:443/a?b=2&a=1&utm_source=x#frag")).unwrap().as_str(), "https://example.com/a?a=1&b=2");
        assert_eq!(normalize(&u("http://example.com")).unwrap().as_str(), "http://example.com/");
        assert_eq!(normalize(&u("http://example.com:8080/x?fbclid=1")).unwrap().as_str(), "http://example.com:8080/x");
        assert!(normalize(&u("ftp://example.com/x")).is_none());
        assert!(join(&u("https://example.com/a/b.html"), "javascript:void(0)").is_none());
        assert_eq!(join(&u("https://example.com/a/b.html"), "../c.html#x").unwrap().as_str(), "https://example.com/c.html");
    }

    #[test]
    fn scopes() {
        let seeds = [u("https://www.example.com/docs/index.html")];
        assert!(in_scope(Scope::Host, &seeds, &u("https://www.example.com/other")));
        assert!(!in_scope(Scope::Host, &seeds, &u("https://shop.example.com/")));
        assert!(in_scope(Scope::Domain, &seeds, &u("https://shop.example.com/")));
        assert!(!in_scope(Scope::Domain, &seeds, &u("https://example.org/")));
        assert!(in_scope(Scope::Prefix, &seeds, &u("https://www.example.com/docs/a/b")));
        assert!(!in_scope(Scope::Prefix, &seeds, &u("https://www.example.com/blog/")));
        assert_eq!(site("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(site("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn pages_and_names() {
        assert!(not_a_page(&u("https://x.com/a/photo.JPG")));
        assert!(!not_a_page(&u("https://x.com/a/page.html")));
        assert!(!not_a_page(&u("https://x.com/a/")));
        assert_eq!(table_name(&u("https://www.books.toscrape.com/")), "books_toscrape_com");
        assert_eq!(table_name(&u("http://127.0.0.1:9000/")), "site_127_0_0_1");
    }
}
