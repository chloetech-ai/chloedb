//! Fetching: one HTTP client for a crawl (compressed answers, at most 5
//! redirects, a timeout, a cap on the size of a page), each host's
//! robots.txt read before its first page and again after a day, and
//! sitemaps.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::OnceCell;
use url::Url;

use crate::robots::Robots;

pub const AGENT_TOKEN: &str = "strata-crawl";
/// The page for site owners the user agent links to: what the crawler is,
/// how it behaves, and how to slow it down or keep it out.
pub const ABOUT: &str = "https://chloetech-ai.github.io/strata/chloedb/crawler-bot.html";

/// How long a robots.txt read is kept before it is read again (RFC 9309
/// asks for at most a day).
pub const ROBOTS_TTL: Duration = Duration::from_secs(24 * 3600);
/// How long a site whose robots.txt did not answer waits before it is
/// asked again, unless its Retry-After says.
pub const ROBOTS_RETRY: Duration = Duration::from_secs(60);

pub struct Fetcher {
    http: reqwest::Client,
    max_bytes: usize,
    robots: Mutex<HashMap<String, Arc<OnceCell<Read>>>>,
    pub obey_robots: bool,
}

/// A site's robots.txt as read, and until when it holds.
struct Read {
    robots: Arc<Robots>,
    until: Instant,
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
    /// The site's robots.txt could not be read (why), so nothing is
    /// fetched from it until it is read again (in how long).
    RobotsUnreachable(String, Duration),
    /// Not HTML: its content type.
    NotHtml(String),
    TooLarge,
    /// 429, 502, 503 or 504: the site is busy; its Retry-After, if any.
    Busy(u16, Option<Duration>),
    Status(u16),
    Timeout,
    Network(String),
}

impl Miss {
    /// A short name, for the counts.
    pub fn kind(&self) -> &'static str {
        match self {
            Miss::Robots => "robots",
            Miss::RobotsUnreachable(..) => "robots_unreachable",
            Miss::NotHtml(_) => "not_html",
            Miss::TooLarge => "too_large",
            Miss::Busy(..) => "busy",
            Miss::Status(s) if *s >= 500 => "http_5xx",
            Miss::Status(_) => "http_4xx",
            Miss::Timeout => "timeout",
            Miss::Network(_) => "network",
        }
    }

    pub fn is_error(&self) -> bool {
        !matches!(self, Miss::Robots | Miss::NotHtml(_))
    }

    /// Whether a request went out for it (robots.txt kept it from going).
    pub fn fetched(&self) -> bool {
        !matches!(self, Miss::Robots | Miss::RobotsUnreachable(..))
    }

    /// Whether it is the site pushing back, so that the crawler waits
    /// before its next request there: whether the page is asked again
    /// later, and how long the site asked to wait.
    pub fn refusal(&self) -> Option<(bool, Option<Duration>)> {
        match self {
            Miss::Busy(_, after) => Some((true, *after)),
            Miss::RobotsUnreachable(_, after) => Some((true, Some(*after))),
            // A site that refuses crawlers, or one that does not answer.
            Miss::Status(403) | Miss::Timeout | Miss::Network(_) => Some((false, None)),
            _ => None,
        }
    }
}

impl std::fmt::Display for Miss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Miss::Robots => write!(f, "robots.txt does not allow it"),
            Miss::RobotsUnreachable(why, _) => write!(f, "{why}: nothing is fetched from a site whose robots.txt cannot be read"),
            Miss::NotHtml(t) => write!(f, "not HTML ({t})"),
            Miss::TooLarge => write!(f, "larger than the most a page may be"),
            Miss::Busy(s, Some(d)) => write!(f, "HTTP {s}: the site is busy and asks to wait {}", secs(*d)),
            Miss::Busy(s, None) => write!(f, "HTTP {s}: the site is busy"),
            Miss::Status(s) => write!(f, "HTTP {s}"),
            Miss::Timeout => write!(f, "no answer in time"),
            Miss::Network(e) => write!(f, "{e}"),
        }
    }
}

/// A duration as people read it: `750 ms`, `2.5 s`, `3 min`.
pub fn secs(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 1.0 {
        format!("{} ms", d.as_millis())
    } else if s < 120.0 {
        format!("{} s", (s * 10.0).round() / 10.0)
    } else {
        format!("{} min", (s / 60.0).round())
    }
}

/// The user agent: the crawler's product token, its version, the page for
/// site owners, and whom to reach about this crawl (`contact`). A crawl's
/// own user agent keeps the token after it, unless it names the crawler
/// already, so that a site can always tell it is this crawler (and its
/// robots.txt group `strata-crawl` applies).
pub fn user_agent(custom: Option<&str>, contact: Option<&str>) -> String {
    let ours = match contact.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => format!("{AGENT_TOKEN}/{} (+{ABOUT}; {c})", env!("CARGO_PKG_VERSION")),
        None => format!("{AGENT_TOKEN}/{} (+{ABOUT})", env!("CARGO_PKG_VERSION")),
    };
    match custom.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) if c.to_ascii_lowercase().contains(AGENT_TOKEN) => c.to_string(),
        Some(c) => format!("{c} {ours}"),
        None => ours,
    }
}

/// Whether text may go in a request header: printable ASCII, at most 300
/// characters.
pub fn header_text(s: &str) -> bool {
    s.len() <= 300 && s.chars().all(|c| (' '..='~').contains(&c))
}

/// An answer's Retry-After: seconds, or an HTTP date.
pub fn retry_after(h: &reqwest::header::HeaderMap) -> Option<Duration> {
    let v = h.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(s) = v.parse::<u64>() {
        return Some(Duration::from_secs(s));
    }
    let at = httpdate::parse_http_date(v).ok()?;
    Some(at.duration_since(SystemTime::now()).unwrap_or(Duration::ZERO))
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

    /// The robots.txt of the URL's host (scheme and port too), read once
    /// and again once its read is a day old.
    pub async fn robots(&self, u: &Url) -> Arc<Robots> {
        self.robots_read(u).await.0
    }

    /// The robots.txt of the URL's host, and until when it holds.
    async fn robots_read(&self, u: &Url) -> (Arc<Robots>, Instant) {
        let origin = u.origin().ascii_serialization();
        let cell = {
            let mut g = self.robots.lock().unwrap_or_else(|e| e.into_inner());
            let cell = g.entry(origin.clone()).or_insert_with(|| Arc::new(OnceCell::new()));
            if cell.get().is_some_and(|r| r.until <= Instant::now()) {
                *cell = Arc::new(OnceCell::new());
            }
            cell.clone()
        };
        let r = cell
            .get_or_init(|| async {
                let (robots, ttl) = self.read_robots(&origin).await;
                Read { robots: Arc::new(robots), until: Instant::now() + ttl }
            })
            .await;
        (r.robots.clone(), r.until)
    }

    /// Reads a site's robots.txt (RFC 9309): a 4xx allows everything; a
    /// 5xx, a 429 or no answer refuses everything until it is read again
    /// in a minute (or when its Retry-After says, up to 10 minutes).
    async fn read_robots(&self, origin: &str) -> (Robots, Duration) {
        let url = format!("{origin}/robots.txt");
        let r = match self.http.get(&url).timeout(Duration::from_secs(15)).send().await {
            Ok(r) => r,
            // No such host: no site to keep rules for (its pages fail too).
            Err(e) if e.is_connect() && chain(&e).contains("dns") => return (Robots::allow_all(), ROBOTS_RETRY),
            Err(e) if e.is_timeout() => return (Robots::unreachable("robots.txt did not answer in time".into()), ROBOTS_RETRY),
            Err(e) => return (Robots::unreachable(format!("robots.txt could not be read ({})", short_error(&e))), ROBOTS_RETRY),
        };
        let status = r.status().as_u16();
        if status == 429 || (500..600).contains(&status) {
            let wait = retry_after(r.headers()).unwrap_or(ROBOTS_RETRY).clamp(Duration::from_secs(10), Duration::from_secs(600));
            return (Robots::unreachable(format!("robots.txt answered HTTP {status}")), wait);
        }
        if status != 200 {
            return (Robots::allow_all(), ROBOTS_TTL);
        }
        match r.text().await {
            Ok(t) => (Robots::parse(&t[..t.floor_char_boundary(500_000)], AGENT_TOKEN), ROBOTS_TTL),
            Err(e) => (Robots::unreachable(format!("robots.txt could not be read ({})", short_error(&e))), ROBOTS_RETRY),
        }
    }

    /// Fetches a page: HTML only, at most `max_bytes`, as text.
    pub async fn page(&self, u: &Url) -> Result<Got, Miss> {
        if self.obey_robots {
            let (robots, until) = self.robots_read(u).await;
            if let Some(why) = &robots.unreachable {
                return Err(Miss::RobotsUnreachable(why.clone(), until.saturating_duration_since(Instant::now())));
            }
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
        if matches!(status, 429 | 502 | 503 | 504) {
            return Err(Miss::Busy(status, retry_after(r.headers())));
        }
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

/// An error and its causes, as one line.
fn chain(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(c) = src {
        s.push_str(": ");
        s.push_str(&c.to_string());
        src = c.source();
    }
    s.to_ascii_lowercase()
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
    fn the_user_agent_names_the_crawler_and_its_page() {
        let v = env!("CARGO_PKG_VERSION");
        assert_eq!(user_agent(None, None), format!("strata-crawl/{v} (+{ABOUT})"));
        assert_eq!(user_agent(Some(" "), Some("ops@example.com")), format!("strata-crawl/{v} (+{ABOUT}; ops@example.com)"));
        assert_eq!(user_agent(Some("AcmeBot/2.0"), None), format!("AcmeBot/2.0 strata-crawl/{v} (+{ABOUT})"), "the token stays");
        assert_eq!(user_agent(Some("Strata-Crawl/9 (team x)"), Some("x")), "Strata-Crawl/9 (team x)", "it names the crawler already");
        assert!(header_text("ops@example.com") && !header_text("a\nb") && !header_text("café") && !header_text(&"x".repeat(301)));
    }

    #[test]
    fn retry_after_and_what_counts_as_pushing_back() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&h), None);
        h.insert("retry-after", "120".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::from_secs(120)));
        let at = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(90));
        h.insert("retry-after", at.parse().unwrap());
        let d = retry_after(&h).unwrap();
        assert!(d > Duration::from_secs(85) && d <= Duration::from_secs(90), "{d:?}");
        h.insert("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::ZERO), "a date gone by");
        h.insert("retry-after", "soon".parse().unwrap());
        assert_eq!(retry_after(&h), None);

        assert_eq!(Miss::Busy(429, Some(Duration::from_secs(3))).refusal(), Some((true, Some(Duration::from_secs(3)))));
        assert_eq!(Miss::RobotsUnreachable("x".into(), Duration::from_secs(9)).refusal(), Some((true, Some(Duration::from_secs(9)))));
        assert_eq!(Miss::Status(403).refusal(), Some((false, None)));
        assert_eq!(Miss::Timeout.refusal(), Some((false, None)));
        assert_eq!(Miss::Status(404).refusal(), None);
        assert_eq!(Miss::Robots.refusal(), None);
        assert!(!Miss::RobotsUnreachable("x".into(), Duration::ZERO).fetched() && Miss::Busy(503, None).fetched());
        assert_eq!(Miss::Busy(503, Some(Duration::from_secs(150))).to_string(), "HTTP 503: the site is busy and asks to wait 3 min");
        assert_eq!(secs(Duration::from_millis(250)), "250 ms");
        assert_eq!(secs(Duration::from_millis(2500)), "2.5 s");
    }

    #[test]
    fn sitemaps_and_charsets() {
        let xml = "<urlset><url><loc>https://a/1</loc></url><url><loc> https://a/2?x=1&amp;y=2 </loc></url></urlset>";
        assert_eq!(locs(xml), vec!["https://a/1", "https://a/2?x=1&y=2"]);
        let latin1 = b"<meta charset=\"iso-8859-1\"><p>caf\xe9</p>";
        assert!(decode(latin1, "text/html").contains("café"));
        assert!(decode("ñ".as_bytes(), "text/html; charset=utf-8").contains('ñ'));
    }
}
