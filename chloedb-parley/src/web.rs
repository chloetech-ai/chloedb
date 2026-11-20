//! Web search and page reading for agents (examples/parley's web.py).
//!
//! Parley runs no web index of its own: it asks public APIs that welcome
//! programs (Hacker News through Algolia, Wikipedia's page summaries, arXiv,
//! Open Library), plus Brave Search or Tavily when a key is set, and keeps
//! what an agent found in `web_pages`. Requests say who they are, read
//! robots.txt before a page, stay small, and reach only public addresses.

use std::collections::HashMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

use crate::social::UA;
use crate::util::re;

pub const TIMEOUT: u64 = 12;
pub const DEFAULT: [&str; 5] = ["brave", "tavily", "wikipedia", "hackernews", "arxiv"];

pub type WebResult<T> = std::result::Result<T, String>;

/// True when the address is on the public internet.
pub fn global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_unspecified()
                || v.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                || o[0] >= 240)
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return global(IpAddr::V4(v4));
            }
            let s = v.segments();
            // Global unicast is 2000::/3, less documentation (2001:db8::/32) and the IETF block (2001::/23).
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && s[1] == 0x0db8)
                && !(s[0] == 0x2001 && s[1] < 0x0200)
        }
    }
}

/// True when every address the name resolves to is public: agents choose the
/// URLs they read, so a page must not be a way into this machine's network.
pub fn public_host_blocking(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    match (host, 0u16).to_socket_addrs() {
        Ok(it) => {
            let ips: Vec<IpAddr> = it.map(|a| a.ip()).collect();
            !ips.is_empty() && ips.into_iter().all(global)
        }
        Err(_) => false,
    }
}

pub async fn public_host(host: &str) -> bool {
    let h = host.to_string();
    tokio::task::spawn_blocking(move || public_host_blocking(&h))
        .await
        .unwrap_or(false)
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        let policy = reqwest::redirect::Policy::custom(|a| {
            if a.previous().len() >= 10 {
                return a.error("too many redirects");
            }
            let host = a.url().host_str().unwrap_or("").to_string();
            if !public_host_blocking(&host) {
                return a.error(format!("redirect to a private address refused: {host}"));
            }
            a.follow()
        });
        reqwest::Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(TIMEOUT))
            .redirect(policy)
            .build()
            .expect("an HTTP client")
    })
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()))
        .unwrap_or_default()
}

/// A GET of a public URL: its first `limit` bytes and its content type.
pub async fn get(
    url: &str,
    headers: &[(&str, &str)],
    limit: usize,
) -> WebResult<(Vec<u8>, String)> {
    let host = host_of(url);
    if !public_host(&host).await {
        return Err(format!("PermissionError: not a public address: {host}"));
    }
    let mut req = client().get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut r = req.send().await.map_err(|e| format!("URLError: {e}"))?;
    if !r.status().is_success() {
        return Err(format!(
            "HTTPError: HTTP Error {}: {}",
            r.status().as_u16(),
            r.status().canonical_reason().unwrap_or("")
        ));
    }
    let ctype = r
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut out = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(|e| format!("URLError: {e}"))? {
        out.extend_from_slice(&chunk);
        if out.len() >= limit {
            out.truncate(limit);
            break;
        }
    }
    Ok((out, ctype))
}

async fn get_json(url: &str, headers: &[(&str, &str)]) -> WebResult<Value> {
    let (b, _) = get(url, headers, 2_000_000).await?;
    serde_json::from_slice(&b).map_err(|e| format!("JSONDecodeError: {e}"))
}

fn qs(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn quote(s: &str) -> String {
    // Python's urllib.parse.quote: letters, digits, _.-~ and / kept.
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn res(url: &str, title: &str, source: &str, snippet: &str) -> Value {
    json!({"url": url, "title": title, "source": source, "snippet": snippet})
}

pub async fn hackernews(q: &str, n: usize) -> WebResult<Vec<Value>> {
    let d = get_json(
        &format!(
            "https://hn.algolia.com/api/v1/search?{}",
            qs(&[
                ("query", q),
                ("tags", "story"),
                ("hitsPerPage", &n.to_string())
            ])
        ),
        &[],
    )
    .await?;
    Ok(d["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|h| {
            let url = h["url"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "https://news.ycombinator.com/item?id={}",
                        h["objectID"].as_str().unwrap_or("")
                    )
                });
            let day: String = h["created_at"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(10)
                .collect();
            res(
                &url,
                h["title"].as_str().unwrap_or(""),
                "hackernews",
                &format!(
                    "{} points, {} comments on Hacker News ({day})",
                    h["points"].as_i64().unwrap_or(0),
                    h["num_comments"].as_i64().unwrap_or(0)
                ),
            )
        })
        .collect())
}

fn title_case(s: &str) -> String {
    // Python's str.title: each run of letters starts upper, the rest lower.
    let mut o = String::new();
    let mut prev = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev {
                o.extend(c.to_lowercase())
            } else {
                o.extend(c.to_uppercase())
            }
            prev = true;
        } else {
            o.push(c);
            prev = false;
        }
    }
    o
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f
            .to_uppercase()
            .chain(c.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// Wikipedia's page summary of the query as a title, then of its words' capitalised forms.
pub async fn wikipedia(q: &str, n: usize) -> WebResult<Vec<Value>> {
    let mut out: Vec<Value> = Vec::new();
    let mut tried = Vec::new();
    let first: String = q
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .chain(q.chars().skip(1))
        .collect();
    let third = if q.contains(' ') {
        Some(capitalize(q.split(' ').next().unwrap_or("")))
    } else {
        None
    };
    for title in [Some(first), Some(title_case(q)), third]
        .into_iter()
        .flatten()
    {
        if title.is_empty() || tried.contains(&title) {
            continue;
        }
        tried.push(title.clone());
        let Ok(d) = get_json(
            &format!(
                "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
                quote(&title.replace(' ', "_"))
            ),
            &[],
        )
        .await
        else {
            continue;
        };
        let extract = d["extract"].as_str().unwrap_or("");
        if d["type"] == "disambiguation" || extract.is_empty() {
            continue;
        }
        let url = d["content_urls"]["desktop"]["page"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if out.iter().any(|o| o["url"] == url.as_str()) {
            continue;
        }
        let mut r = res(
            &url,
            d["title"].as_str().unwrap_or(&title),
            "wikipedia",
            &extract.chars().take(600).collect::<String>(),
        );
        r["body"] = json!(extract);
        r["image_url"] = d["thumbnail"]["source"].clone();
        out.push(r);
        if out.len() >= n {
            break;
        }
    }
    Ok(out)
}

fn spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub async fn arxiv(q: &str, n: usize) -> WebResult<Vec<Value>> {
    let (body, _) = get(
        &format!(
            "https://export.arxiv.org/api/query?{}",
            qs(&[
                ("search_query", &format!("all:{q}")),
                ("max_results", &n.to_string()),
                ("sortBy", "relevance")
            ])
        ),
        &[],
        2_000_000,
    )
    .await?;
    let text = String::from_utf8_lossy(&body).to_string();
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("ParseError: {e}"))?;
    let atom = "http://www.w3.org/2005/Atom";
    let mut out = Vec::new();
    for e in doc
        .root_element()
        .children()
        .filter(|c| c.has_tag_name((atom, "entry")))
    {
        let field = |name: &str| {
            e.children()
                .find(|c| c.has_tag_name((atom, name)))
                .map(|c| c.descendants().filter_map(|d| d.text()).collect::<String>())
                .unwrap_or_default()
        };
        let summary = spaces(&field("summary"));
        let mut r = res(
            &field("id"),
            &spaces(&field("title")),
            "arxiv",
            &summary.chars().take(600).collect::<String>(),
        );
        r["body"] = json!(summary);
        out.push(r);
    }
    Ok(out)
}

pub async fn openlibrary(q: &str, n: usize) -> WebResult<Vec<Value>> {
    let d = get_json(
        &format!(
            "https://openlibrary.org/search.json?{}",
            qs(&[
                ("q", q),
                ("limit", &n.to_string()),
                ("fields", "key,title,author_name,first_publish_year,cover_i")
            ])
        ),
        &[],
    )
    .await?;
    Ok(d["docs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|doc| {
            let authors: Vec<&str> = doc["author_name"]
                .as_array()
                .into_iter()
                .flatten()
                .take(3)
                .filter_map(Value::as_str)
                .collect();
            let authors = if authors.is_empty() {
                "unknown".to_string()
            } else {
                authors.join(", ")
            };
            let year = match &doc["first_publish_year"] {
                Value::Null => "?".to_string(),
                v => v.to_string(),
            };
            let mut r = res(
                &format!(
                    "https://openlibrary.org{}",
                    doc["key"].as_str().unwrap_or("")
                ),
                doc["title"].as_str().unwrap_or(""),
                "openlibrary",
                &format!("Book by {authors}, first published {year}."),
            );
            r["image_url"] = match &doc["cover_i"] {
                Value::Null => Value::Null,
                c => json!(format!("https://covers.openlibrary.org/b/id/{c}-M.jpg")),
            };
            r
        })
        .collect())
}

pub async fn brave(q: &str, n: usize) -> WebResult<Vec<Value>> {
    let Some(key) = std::env::var("BRAVE_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    else {
        return Ok(vec![]);
    };
    let d = get_json(
        &format!(
            "https://api.search.brave.com/res/v1/web/search?{}",
            qs(&[("q", q), ("count", &n.to_string())])
        ),
        &[
            ("X-Subscription-Token", &key),
            ("Accept", "application/json"),
        ],
    )
    .await?;
    Ok(d["web"]["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            res(
                r["url"].as_str().unwrap_or(""),
                r["title"].as_str().unwrap_or(""),
                "brave",
                &strip(r["description"].as_str().unwrap_or("")),
            )
        })
        .collect())
}

pub async fn tavily(q: &str, n: usize) -> WebResult<Vec<Value>> {
    let Some(key) = std::env::var("TAVILY_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    else {
        return Ok(vec![]);
    };
    let r = client()
        .post("https://api.tavily.com/search")
        .bearer_auth(key)
        .json(&json!({"query": q, "max_results": n}))
        .send()
        .await
        .map_err(|e| format!("URLError: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("HTTPError: HTTP Error {}", r.status().as_u16()));
    }
    let d: Value = r
        .json()
        .await
        .map_err(|e| format!("JSONDecodeError: {e}"))?;
    Ok(d["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|x| {
            res(
                x["url"].as_str().unwrap_or(""),
                x["title"].as_str().unwrap_or(""),
                "tavily",
                &x["content"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(600)
                    .collect::<String>(),
            )
        })
        .collect())
}

async fn provider(name: &str, q: &str) -> Option<WebResult<Vec<Value>>> {
    Some(match name {
        "brave" => brave(q, 6).await,
        "tavily" => tavily(q, 6).await,
        "hackernews" => hackernews(q, 5).await,
        "wikipedia" => wikipedia(q, 3).await,
        "arxiv" => arxiv(q, 4).await,
        "openlibrary" => openlibrary(q, 3).await,
        _ => return None,
    })
}

/// Results from each source in turn, deduplicated by URL, and each source's error.
pub async fn search(
    q: &str,
    sources: Option<&[String]>,
    n: usize,
) -> (Vec<Value>, serde_json::Map<String, Value>) {
    let names: Vec<String> = match sources {
        Some(s) if !s.is_empty() => s.to_vec(),
        _ => DEFAULT.iter().map(|s| s.to_string()).collect(),
    };
    let mut results: Vec<Value> = Vec::new();
    let mut errors = serde_json::Map::new();
    let mut seen = std::collections::HashSet::new();
    for name in names {
        match provider(&name, q).await {
            None => continue,
            Some(Ok(rs)) => {
                for r in rs {
                    let u = r["url"].as_str().unwrap_or("").to_string();
                    if !u.is_empty() && seen.insert(u) {
                        results.push(r);
                    }
                }
            }
            Some(Err(e)) => {
                errors.insert(name.clone(), json!(e.chars().take(200).collect::<String>()));
            }
        }
        if results.len() >= n {
            break;
        }
    }
    results.truncate(n);
    (results, errors)
}

// ---------------------------------------------------------------- robots.txt

/// One robots.txt: its groups of agents and rules, as Python's robotparser reads them.
#[derive(Clone, Default)]
pub struct Robots {
    groups: Vec<(Vec<String>, Vec<(bool, String)>)>,
    default: Option<Vec<(bool, String)>>,
}

impl Robots {
    pub fn parse(text: &str) -> Robots {
        let mut r = Robots::default();
        let mut agents: Vec<String> = Vec::new();
        let mut rules: Vec<(bool, String)> = Vec::new();
        let mut in_rules = false;
        let flush = |agents: &mut Vec<String>, rules: &mut Vec<(bool, String)>, r: &mut Robots| {
            if !agents.is_empty() {
                if agents.iter().any(|a| a == "*") {
                    if r.default.is_none() {
                        r.default = Some(rules.clone());
                    }
                } else {
                    r.groups.push((agents.clone(), rules.clone()));
                }
            }
            agents.clear();
            rules.clear();
        };
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let (k, v) = (k.trim().to_lowercase(), v.trim());
            match k.as_str() {
                "user-agent" => {
                    if in_rules {
                        flush(&mut agents, &mut rules, &mut r);
                        in_rules = false;
                    }
                    agents.push(v.to_lowercase());
                }
                "allow" | "disallow" if !agents.is_empty() => {
                    in_rules = true;
                    let allow = k == "allow" || v.is_empty();
                    rules.push((allow, norm_path(v)));
                }
                _ => {}
            }
        }
        flush(&mut agents, &mut rules, &mut r);
        r
    }

    pub fn can_fetch(&self, agent: &str, url: &str) -> bool {
        let path = url::Url::parse(url)
            .map(|u| {
                let mut p = u.path().to_string();
                if let Some(q) = u.query() {
                    p = format!("{p}?{q}");
                }
                p
            })
            .unwrap_or_else(|_| "/".into());
        let path = norm_path(if path.is_empty() { "/" } else { &path });
        let token = agent.split('/').next().unwrap_or("").to_lowercase();
        let rules = self
            .groups
            .iter()
            .find(|(agents, _)| agents.iter().any(|a| token.contains(a.as_str())))
            .map(|g| &g.1)
            .or(self.default.as_ref());
        let Some(rules) = rules else { return true };
        for (allow, p) in rules {
            if p.is_empty() || path.starts_with(p.as_str()) {
                return *allow;
            }
        }
        true
    }
}

fn norm_path(p: &str) -> String {
    let dec = url::form_urlencoded::parse(format!("x={}", p.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();
    quote(&dec)
        .replace("%3F", "?")
        .replace("%3D", "=")
        .replace("%26", "&")
}

fn robots_cache() -> &'static Mutex<HashMap<String, Option<Robots>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<Robots>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn allowed(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    let origin = format!(
        "{}://{}",
        u.scheme(),
        u.host_str()
            .map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_string(),
            })
            .unwrap_or_default()
    );
    let known = robots_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&origin)
        .cloned();
    let rp = match known {
        Some(r) => r,
        None => {
            let r = match get(&format!("{origin}/robots.txt"), &[], 200_000).await {
                Ok((b, _)) => Some(Robots::parse(&String::from_utf8_lossy(&b))),
                Err(_) => None, // no robots.txt: allowed
            };
            let mut c = robots_cache().lock().unwrap_or_else(|e| e.into_inner());
            if c.len() >= 256 {
                c.clear();
            }
            c.insert(origin, r.clone());
            r
        }
    };
    rp.map_or(true, |r| r.can_fetch(UA, url))
}

// ---------------------------------------------------------------- pages

/// A page's title, its text (no scripts, menus or forms), and its picture.
pub fn page_text(html: &str) -> (String, String, Option<String>) {
    const SKIP: [&str; 8] = [
        "script", "style", "noscript", "svg", "nav", "footer", "header", "form",
    ];
    const BREAK: [&str; 8] = ["p", "br", "li", "h1", "h2", "h3", "tr", "div"];
    let mut parts = String::new();
    let mut title = String::new();
    let mut skip = 0usize;
    let mut in_title = false;
    let mut og: Option<String> = None;
    let b = html.as_bytes();
    let mut i = 0;
    let mut data_from = 0;
    let push = |s: &str, in_title: bool, skip: usize, title: &mut String, parts: &mut String| {
        if s.is_empty() {
            return;
        }
        let t = unescape(s);
        if in_title {
            title.push_str(&t);
        } else if skip == 0 {
            parts.push_str(&t);
        }
    };
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        // A comment, a declaration, an end tag or a start tag.
        if html[i..].starts_with("<!--") {
            push(&html[data_from..i], in_title, skip, &mut title, &mut parts);
            let end = html[i + 4..]
                .find("-->")
                .map(|e| i + 4 + e + 3)
                .unwrap_or(b.len());
            i = end;
            data_from = i;
            continue;
        }
        let rest = &html[i + 1..];
        let is_end = rest.starts_with('/');
        let name_start = if is_end { i + 2 } else { i + 1 };
        let mut j = name_start;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b':') {
            j += 1;
        }
        if j == name_start && !rest.starts_with('!') && !rest.starts_with('?') {
            i += 1;
            continue; // a bare '<' is text
        }
        push(&html[data_from..i], in_title, skip, &mut title, &mut parts);
        let Some(close) = html[j..].find('>').map(|e| j + e) else {
            data_from = i;
            break;
        };
        let tag = html[name_start..j].to_ascii_lowercase();
        let attrs = &html[j..close];
        i = close + 1;
        data_from = i;
        if rest.starts_with('!') || rest.starts_with('?') {
            continue;
        }
        if is_end {
            if SKIP.contains(&tag.as_str()) && skip > 0 {
                skip -= 1;
            }
            if tag == "title" {
                in_title = false;
            }
            continue;
        }
        if SKIP.contains(&tag.as_str()) {
            skip += 1;
        }
        if tag == "title" {
            in_title = true;
        }
        if tag == "meta" && og.is_none() {
            let a = attr_map(attrs);
            if matches!(
                a.get("property").map(String::as_str),
                Some("og:image" | "twitter:image")
            ) {
                if let Some(c) = a.get("content").filter(|c| !c.is_empty()) {
                    og = Some(c.clone());
                }
            }
        }
        if BREAK.contains(&tag.as_str()) {
            parts.push('\n');
        }
        // A script's or a style's content is not markup: skip to its end tag.
        if (tag == "script" || tag == "style") && !attrs.trim_end().ends_with('/') {
            let low = html[i..].to_ascii_lowercase();
            let end = low
                .find(&format!("</{tag}"))
                .map(|e| i + e)
                .unwrap_or(b.len());
            i = end;
            data_from = i;
        }
    }
    if data_from < b.len() {
        push(&html[data_from..], in_title, skip, &mut title, &mut parts);
    }
    (spaces(&title), parts, og)
}

fn attr_map(s: &str) -> HashMap<String, String> {
    static A: OnceLock<regex::Regex> = OnceLock::new();
    re(
        &A,
        r#"([A-Za-z_:][-A-Za-z0-9_:.]*)\s*(?:=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?"#,
    )
    .captures_iter(s)
    .map(|c| {
        (
            c[1].to_lowercase(),
            unescape(
                c.get(2)
                    .or(c.get(3))
                    .or(c.get(4))
                    .map_or("", |m| m.as_str()),
            ),
        )
    })
    .collect()
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    html_escape::decode_html_entities(s).to_string()
}

pub fn strip(s: &str) -> String {
    static T: OnceLock<regex::Regex> = OnceLock::new();
    unescape(&re(&T, r"<[^>]+>").replace_all(s, ""))
}

pub async fn read(url: &str, max_chars: usize) -> WebResult<Value> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) URLs".into());
    }
    if !allowed(url).await {
        return Err(format!(
            "robots.txt of {} disallows {url}",
            url::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default()
        ));
    }
    let (body, ctype) = get(url, &[], 2_000_000).await?;
    if !ctype.contains("html") && !ctype.contains("text") && !ctype.contains("xml") {
        return Err(format!("not a page: {ctype}"));
    }
    let (title, parts, og) = page_text(&String::from_utf8_lossy(&body));
    static WS: OnceLock<regex::Regex> = OnceLock::new();
    static NL: OnceLock<regex::Regex> = OnceLock::new();
    let t = re(&WS, r"[ \t\r\x0c\x0b]+").replace_all(&parts, " ");
    let text = re(&NL, r"\n\s*\n+")
        .replace_all(&t, "\n\n")
        .trim()
        .to_string();
    Ok(
        json!({"url": url, "title": title, "body": text.chars().take(max_chars).collect::<String>(), "image_url": og, "source": "page"}),
    )
}

/// An error as Python's str() of it reads: without the class's name.
pub fn bare(e: &str) -> String {
    static P: OnceLock<regex::Regex> = OnceLock::new();
    re(&P, r"^[A-Za-z]+Error: ").replace(e, "").into_owned()
}

/// The picture types ChloeDB decodes.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

pub async fn fetch_image(url: &str, max_bytes: usize) -> WebResult<(Vec<u8>, &'static str)> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) URLs".into());
    }
    let (body, ctype) = get(url, &[], max_bytes + 1).await?;
    if body.len() > max_bytes {
        return Err("image larger than 5 MB".into());
    }
    match sniff(&body) {
        Some(m) => Ok((body, m)),
        None => Err(format!(
            "not an image ChloeDB decodes (JPEG, PNG, WebP, GIF, BMP): {ctype}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "192.168.1.1",
            "172.16.0.1",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "0.0.0.0",
        ] {
            assert!(!global(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700::1111"] {
            assert!(global(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn robots_rules() {
        let r = Robots::parse("User-agent: *\nDisallow: /private\nAllow: /\n\nUser-agent: ParleyPrototype\nDisallow: /no-parley\n");
        assert!(!r.can_fetch(UA, "https://x.org/no-parley/a"));
        assert!(r.can_fetch(UA, "https://x.org/private"));
        let r = Robots::parse("User-agent: *\nDisallow: /private\n");
        assert!(!r.can_fetch(UA, "https://x.org/private/x"));
        assert!(r.can_fetch(UA, "https://x.org/public"));
        let r = Robots::parse("User-agent: *\nDisallow:\n");
        assert!(r.can_fetch(UA, "https://x.org/anything"));
    }

    #[test]
    fn page_text_skips_scripts_and_menus() {
        let (t, body, og) = page_text("<html><head><title> A  page </title><meta property=\"og:image\" content=\"https://x/i.jpg\"><script>var a = '<p>no</p>';</script></head>\
            <body><nav>menu</nav><p>Hello &amp; welcome</p><div>second</div><!-- c --></body></html>");
        assert_eq!(t, "A page");
        assert_eq!(og.as_deref(), Some("https://x/i.jpg"));
        assert!(
            body.contains("Hello & welcome")
                && body.contains("second")
                && !body.contains("menu")
                && !body.contains("no")
        );
    }
}
