//! robots.txt: which paths a site lets crawlers fetch, how long to wait
//! between requests, and its sitemaps (RFC 9309, with `Crawl-delay` and
//! `Sitemap`, which most sites use and the RFC leaves out).

/// The rules of one site for this crawler.
#[derive(Clone, Debug, Default)]
pub struct Robots {
    /// `(allow, pattern)`, the group that names this crawler, or `*`'s.
    rules: Vec<(bool, String)>,
    /// Seconds between requests the site asks for.
    pub crawl_delay: Option<f64>,
    pub sitemaps: Vec<String>,
    /// Why it could not be read, when the site did not answer it (5xx,
    /// 429, no answer): nothing is fetched from the site until it is read.
    pub unreachable: Option<String>,
}

impl Robots {
    /// Everything allowed: a site with no robots.txt (404) or one that
    /// could not be read.
    pub fn allow_all() -> Robots {
        Robots::default()
    }

    /// Everything refused.
    pub fn deny_all() -> Robots {
        Robots { rules: vec![(false, "/".into())], ..Default::default() }
    }

    /// Everything refused for now: what RFC 9309 asks of a site whose
    /// robots.txt is unreachable (5xx, or no answer; 429 too, as a site
    /// that is busy), until it is read again.
    pub fn unreachable(why: String) -> Robots {
        Robots { unreachable: Some(why), ..Robots::deny_all() }
    }

    /// The rules for `agent` (the product token, as `strata-crawl`) in a
    /// robots.txt: the groups that name it, or else those for `*`.
    pub fn parse(text: &str, agent: &str) -> Robots {
        let agent = agent.to_ascii_lowercase();
        let mut mine: Vec<(bool, String)> = Vec::new();
        let mut star: Vec<(bool, String)> = Vec::new();
        let (mut mine_delay, mut star_delay) = (None, None);
        let mut sitemaps = Vec::new();
        // The agents of the group being read, and whether its rules began
        // (a User-agent line after rules starts a new group).
        let mut agents: Vec<String> = Vec::new();
        let mut in_rules = false;
        let mut matched_mine = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once(':') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "user-agent" => {
                    if in_rules {
                        agents.clear();
                        in_rules = false;
                    }
                    agents.push(v.to_ascii_lowercase());
                }
                "allow" | "disallow" => {
                    in_rules = true;
                    // An empty Disallow allows everything: no rule.
                    if v.is_empty() {
                        continue;
                    }
                    let rule = (k == "allow", v.to_string());
                    if agents.iter().any(|a| a != "*" && agent.contains(a.as_str())) {
                        matched_mine = true;
                        mine.push(rule);
                    } else if agents.iter().any(|a| a == "*") {
                        star.push(rule);
                    }
                }
                "crawl-delay" => {
                    in_rules = true;
                    let d = v.parse::<f64>().ok().filter(|d| d.is_finite() && *d >= 0.0);
                    if agents.iter().any(|a| a != "*" && agent.contains(a.as_str())) {
                        matched_mine = true;
                        mine_delay = d;
                    } else if agents.iter().any(|a| a == "*") {
                        star_delay = d;
                    }
                }
                "sitemap" => sitemaps.push(line[line.find(':').map_or(0, |i| i + 1)..].trim().to_string()),
                _ => {}
            }
        }
        if matched_mine {
            Robots { rules: mine, crawl_delay: mine_delay, sitemaps, unreachable: None }
        } else {
            Robots { rules: star, crawl_delay: star_delay, sitemaps, unreachable: None }
        }
    }

    /// Whether a path (with its query) may be fetched: the longest rule
    /// that matches decides, an allow winning a tie; no rule, allowed.
    pub fn allowed(&self, path_and_query: &str) -> bool {
        let mut best: Option<(usize, bool)> = None;
        for (allow, pat) in &self.rules {
            if matches(pat, path_and_query) {
                let len = pat.len();
                best = match best {
                    Some((l, a)) if l > len || (l == len && a) => Some((l, a)),
                    _ => Some((len, *allow)),
                };
            }
        }
        best.is_none_or(|(_, a)| a)
    }
}

/// A robots pattern on a path: a prefix, where `*` is any run of
/// characters and a final `$` anchors the end.
fn matches(pat: &str, path: &str) -> bool {
    let (pat, anchored) = match pat.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pat, false),
    };
    let parts: Vec<&str> = pat.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(part) {
                return false;
            }
            pos = part.len();
            continue;
        }
        if i == parts.len() - 1 && anchored {
            return path.len() >= pos + part.len() && path.ends_with(part);
        }
        match path[pos..].find(part) {
            Some(j) => pos += j + part.len(),
            None => return false,
        }
    }
    !anchored || pos == path.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TXT: &str = "
# comment
User-agent: *
Disallow: /private/
Disallow: /*.pdf$
Allow: /private/open
Crawl-delay: 2

User-agent: strata-crawl
User-agent: other
Disallow: /mine/
Crawl-delay: 0.5

Sitemap: https://example.com/sitemap.xml
";

    #[test]
    fn groups_and_rules() {
        let r = Robots::parse(TXT, "strata-crawl/0.1");
        assert!(!r.allowed("/mine/x"));
        assert!(r.allowed("/private/x"), "our own group replaces *'s");
        assert_eq!(r.crawl_delay, Some(0.5));
        assert_eq!(r.sitemaps, vec!["https://example.com/sitemap.xml"]);
        let s = Robots::parse(TXT, "somebot");
        assert!(!s.allowed("/private/x"));
        assert!(s.allowed("/private/open/page"), "the longer allow wins");
        assert!(!s.allowed("/files/a.pdf"));
        assert!(s.allowed("/files/a.pdf?x=1"));
        assert!(s.allowed("/"));
        assert_eq!(s.crawl_delay, Some(2.0));
    }

    #[test]
    fn empty_and_deny() {
        assert!(Robots::parse("User-agent: *\nDisallow:\n", "x").allowed("/anything"));
        assert!(!Robots::deny_all().allowed("/"));
        assert!(Robots::allow_all().allowed("/"));
        assert!(matches("/a*b$", "/a/x/b"));
        assert!(!matches("/a*b$", "/a/x/bc"));
    }
}
