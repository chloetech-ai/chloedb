//! Fetching: one HTTP client for a crawl (compressed answers, at most 5
//! redirects, a timeout, a cap on the size of a page), each host's
//! robots.txt read once before its first page, and sitemaps.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::OnceCell;
use url::Url;

use crate::robots::Robots;

pub const AGENT_TOKEN: &str = "strata-crawl";

pub struct Fetcher {
    http: reqwest::Client,
    max_bytes: usize,
    robots: Mutex<HashMap<String, Arc<OnceCell<Arc<Robots>>>>>,
    pub obey_robots: bool,
}

/// A page fetched.
pub struct Got {
    /// Where the redirects ended.
    pub url: Url,
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub bytes: usize,
    pub ms: u64,
}

/// Why a page was not fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    Robots,
    /// Not HTML: its content type.
    NotHtml(String),
    TooLarge,
    Status(u16),
    Timeout,
    Network(String),
}

impl Miss {
    /// A short name, for the counts.
    pub fn kind(&self) -> &'static str {
        match self {
            Miss::Robots => "robots",
            Miss::NotHtml(_) => "not_html",
            Miss::TooLarge => "too_large",
            Miss::Status(s) if *s >= 500 => "http_5xx",
            Miss::Status(_) => "http_4xx",
            Miss::Timeout => "timeout",
            Miss::Network(_) => "network",
        }
    }

    pub fn is_error(&self) -> bool {
        !matches!(self, Miss::Robots | Miss::NotHtml(_))
    }
}

impl std::fmt::Display for Miss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Miss::Robots => write!(f, "robots.txt does not allow it"),
            Miss::NotHtml(t) => write!(f, "not HTML ({t})"),
            Miss::TooLarge => write!(f, "larger than the most a page may be"),
            Miss::Status(s) => write!(f, "HTTP {s}"),
            Miss::Timeout => write!(f, "no answer in time"),
            Miss::Network(e) => write!(f, "{e}"),
        }
    }
}

/// The user agent: the crawler's product token, its version, and where
/// to read about it (or the crawl's own).
pub fn user_agent(custom: Option<&str>) -> String {
    match custom {
        Some(c) if !c.trim().is_empty() => c.trim().to_string(),
        _ => format!("{AGENT_TOKEN}/{} (+https://github.com/chloetech-ai/strata)", env!("CARGO_PKG_VERSION")),
    }
}

impl Fetcher {
    pub fn new(agent: &str, timeout: Duration, max_bytes: usize, per_host: usize, obey_robots: bool) -> Fetcher {
        let http = reqwest::Client::builder()
            .user_agent(agent)
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10).min(timeout))
            .redirect(reqwest::redirect::Policy::limited(5))
            .pool_max_idle_per_host(per_host.max(1))
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .build()
            .expect("an HTTP client");
        Fetcher { http, max_bytes, robots: Mutex::new(HashMap::new()), obey_robots }
    }

    /// The robots.txt of the URL's host (scheme and port too), read once.
    pub async fn robots(&self, u: &Url) -> Arc<Robots> {
        let origin = u.origin().ascii_serialization();
        let cell = {
            let mut g = self.robots.lock().unwrap_or_else(|e| e.into_inner());
            g.entry(origin.clone()).or_insert_with(|| Arc::new(OnceCell::new())).clone()
        };
        cell.get_or_init(|| async {
            let url = format!("{origin}/robots.txt");
            let r = match self.http.get(&url).timeout(Duration::from_secs(15)).send().await {
                Ok(r) => r,
                Err(_) => return Arc::new(Robots::allow_all()),
            };
            let status = r.status().as_u16();
            if (500..600).contains(&status) {
                return Arc::new(Robots::deny_all());
            }
            if status != 200 {
                return Arc::new(Robots::allow_all());
            }
            match r.text().await {
                Ok(t) => Arc::new(Robots::parse(&t[..t.len().min(500_000)], AGENT_TOKEN)),
                Err(_) => Arc::new(Robots::allow_all()),
            }
        })
        .await
        .clone()
    }

    /// Fetches a page: HTML only, at most `max_bytes`, as text.
    pub async fn page(&self, u: &Url) -> Result<Got, Miss> {
        if self.obey_robots {
            let robots = self.robots(u).await;
            let pq = match u.query() {
                Some(q) => format!("{}?{}", u.path(), q),
                None => u.path().to_string(),
            };
            if !robots.allowed(&pq) {
                return Err(Miss::Robots);
            }
        }
        let t0 = Instant::now();
        let r = self.http.get(u.as_str()).header("accept", "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5").send().await.map_err(|e| {
            if e.is_timeout() {
                Miss::Timeout
            } else {
                Miss::Network(short_error(&e))
            }
        })?;
        let status = r.status().as_u16();
        let final_url = r.url().clone();
        let ctype = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if !(200..300).contains(&status) {
            return Err(Miss::Status(status));
        }
        let main = ctype.split(';').next().unwrap_or("").trim().to_string();
        if !main.is_empty() && main != "text/html" && main != "application/xhtml+xml" {
            return Err(Miss::NotHtml(main));
        }
        if r.content_length().is_some_and(|n| n as usize > self.max_bytes) {
            return Err(Miss::TooLarge);
        }
        let mut bytes: Vec<u8> = Vec::new();
        let mut r = r;
        while let Some(chunk) = r.chunk().await.map_err(|e| if e.is_timeout() { Miss::Timeout } else { Miss::Network(short_error(&e)) })? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > self.max_bytes {
                return Err(Miss::TooLarge);
            }
        }
        let n = bytes.len();
        let body = decode(&bytes, &ctype);
        Ok(Got { url: final_url, status, content_type: if main.is_empty() { "text/html".into() } else { main }, body, bytes: n, ms: t0.elapsed().as_millis() as u64 })
    }

    /// The page URLs of a sitemap, following a sitemap index one level
    /// down, at most `limit`.
    pub async fn sitemap(&self, u: &str, limit: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut todo = vec![(u.to_string(), 0)];
        while let Some((u, level)) = todo.pop() {
            if out.len() >= limit {
                break;
            }
            let Ok(r) = self.http.get(&u).timeout(Duration::from_secs(20)).send().await else { continue };
            if !r.status().is_success() {
                continue;
            }
            let Ok(text) = r.text().await else { continue };
            let index = text.contains("<sitemapindex");
            for loc in locs(&text) {
                if index {
                    if level < 1 && todo.len() < 50 {
                        todo.push((loc, level + 1));
                    }
                } else if out.len() < limit {
                    out.push(loc);
                }
            }
        }
        out
    }
}

/// The `<loc>`s of a sitemap.
fn locs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<loc>") {
        rest = &rest[i + 5..];
        let Some(j) = rest.find("</loc>") else { break };
        let v = rest[..j].trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").replace("&amp;", "&");
        out.push(v);
        rest = &rest[j..];
    }
    out
}

fn short_error(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(c) = src {
        s = c.to_string();
        src = c.source();
    }
    s.chars().take(200).collect()
}

/// A page's bytes as text: the charset its content type or its own
/// `<meta charset>` names, UTF-8 by default.
fn decode(bytes: &[u8], ctype: &str) -> String {
    let named = ctype.split("charset=").nth(1).map(|c| c.trim_matches(|ch: char| ch == '"' || ch == '\'' || ch.is_whitespace()).to_string()).or_else(|| {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
        let i = head.find("charset=")?;
        let rest = head[i + 8..].trim_start_matches(['"', '\'']);
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    });
    match named.and_then(|n| encoding_rs::Encoding::for_label(n.as_bytes())) {
        Some(enc) if enc != encoding_rs::UTF_8 => enc.decode(bytes).0.into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sitemaps_and_charsets() {
        let xml = "<urlset><url><loc>https://a/1</loc></url><url><loc> https://a/2?x=1&amp;y=2 </loc></url></urlset>";
        assert_eq!(locs(xml), vec!["https://a/1", "https://a/2?x=1&y=2"]);
        let latin1 = b"<meta charset=\"iso-8859-1\"><p>caf\xe9</p>";
        assert!(decode(latin1, "text/html").contains("café"));
        assert!(decode("ñ".as_bytes(), "text/html; charset=utf-8").contains('ñ'));
    }
}
