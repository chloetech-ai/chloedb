//! Content understanding: what an item's HTML says as text, what kind of
//! post it is, where its link goes and what it is about. All of it is
//! deterministic and cheap (microseconds an item), so the archive's 49
//! million items go through it in minutes and a live item on arrival.

/// HN's HTML as the text HN's own editor takes: paragraphs apart by a
/// blank line, `*italics*`, code indented by two spaces, links as their
/// address. The page renders it back the way HN does.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut in_pre = false;
    while let Some(lt) = rest.find('<') {
        let (before, tag_on) = rest.split_at(lt);
        push_text(&mut out, before, in_pre);
        let Some(gt) = tag_on.find('>') else {
            push_text(&mut out, tag_on, in_pre);
            rest = "";
            break;
        };
        let tag = &tag_on[1..gt];
        rest = &tag_on[gt + 1..];
        let lower = tag.trim().to_ascii_lowercase();
        let name = lower.trim_start_matches('/').split(|c: char| c.is_whitespace()).next().unwrap_or("");
        let closing = lower.starts_with('/');
        match name {
            "p" if !closing => {
                trim_end_spaces(&mut out);
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
            }
            "i" | "em" => out.push('*'),
            "pre" if !closing => {
                trim_end_spaces(&mut out);
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str("  ");
                in_pre = true;
            }
            "pre" => {
                in_pre = false;
                trim_end_spaces(&mut out);
                out.push_str("\n\n");
            }
            "a" if !closing => {
                // The address in full: HN shows it cut, the href holds it whole.
                if let Some(href) = attr(tag, "href") {
                    let href = unescape(&href);
                    out.push_str(&href);
                    // Skip the link's own text, which is the address again (or a cut of it).
                    if let Some(end) = rest.to_ascii_lowercase().find("</a>") {
                        let inner = unescape(&rest[..end]);
                        let inner = inner.trim_end_matches("...");
                        if !(href.starts_with(inner) || inner.is_empty()) {
                            out.push_str(" (");
                            out.push_str(inner);
                            out.push(')');
                        }
                        rest = &rest[end + 4..];
                    }
                }
            }
            "br" => out.push('\n'),
            _ => {}
        }
    }
    push_text(&mut out, rest, in_pre);
    let t = out.trim();
    // Paragraph breaks at most one blank line.
    let mut s = String::with_capacity(t.len());
    let mut nl = 0;
    for c in t.chars() {
        if c == '\n' {
            nl += 1;
            if nl <= 2 {
                s.push(c);
            }
        } else {
            nl = 0;
            s.push(c);
        }
    }
    s
}

fn trim_end_spaces(s: &mut String) {
    while s.ends_with(' ') {
        s.pop();
    }
}

fn push_text(out: &mut String, raw: &str, in_pre: bool) {
    if raw.is_empty() {
        return;
    }
    let t = unescape(raw);
    if in_pre {
        // Code keeps its lines, each indented as HN's editor writes it.
        out.push_str(&t.replace('\n', "\n  "));
    } else {
        out.push_str(&t);
    }
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')? + at;
    Some(tag[at..end].to_string())
}

/// HTML's entities as their characters (`&#x27;`, `&quot;`, `&#8212;`).
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let semi = after.find(';').filter(|&i| i <= 10);
        let Some(semi) = semi else {
            out.push('&');
            rest = after;
            continue;
        };
        let ent = &after[..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The kind of post: `story`, `ask`, `show`, `launch`, `tell`, `job`,
/// `poll`, from the item's type and its title's prefix.
pub fn kind(item_type: i8, title: &str) -> &'static str {
    match item_type {
        5 => return "job",
        3 => return "poll",
        _ => {}
    }
    let t = title.trim_start().to_ascii_lowercase();
    if t.starts_with("ask hn") {
        "ask"
    } else if t.starts_with("show hn") {
        "show"
    } else if t.starts_with("launch hn") {
        "launch"
    } else if t.starts_with("tell hn") {
        "tell"
    } else {
        "story"
    }
}

/// A link's host without `www.`, lowercased (`github.com`), or "".
pub fn domain(url: &str) -> String {
    let u = url.trim();
    let Some(after) = u.split_once("://").map(|(_, r)| r) else { return String::new() };
    let host = after.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).trim_end_matches('.').to_string()
}

/// Hosts where the site is a person or a project on it, as HN's own
/// "from" link reads them (`github.com/rust-lang`).
const SITES_BY_PATH: &[&str] = &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org", "twitter.com", "x.com", "medium.com", "huggingface.co", "sr.ht", "git.sr.ht", "gist.github.com", "substack.com", "youtube.com/@", "bsky.app"];

/// The site HN shows beside a title: the host, or for a host of many
/// people's pages the host and the first part of the path.
pub fn site(url: &str) -> String {
    let d = domain(url);
    if d.is_empty() {
        return d;
    }
    if SITES_BY_PATH.contains(&d.as_str()) {
        let path = url.split_once("://").map(|(_, r)| r).unwrap_or("");
        let mut parts = path.split(['?', '#']).next().unwrap_or("").split('/').skip(1).filter(|p| !p.is_empty());
        if let Some(first) = parts.next() {
            if d == "bsky.app" {
                if let Some(who) = parts.nth(0) {
                    return format!("{d}/{}", who.to_ascii_lowercase());
                }
            }
            return format!("{d}/{}", first.to_ascii_lowercase());
        }
    }
    d
}

/// The words of a link worth searching: host and path parts
/// (`github.com/tokio-rs/axum` gives "github tokio rs axum").
pub fn url_words(url: &str) -> String {
    let path = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut words = Vec::new();
    for w in path.split(|c: char| !c.is_alphanumeric()) {
        let lw = w.to_ascii_lowercase();
        if lw.is_empty() || lw.len() > 30 || matches!(lw.as_str(), "www" | "com" | "org" | "net" | "io" | "html" | "htm" | "php" | "https" | "http" | "index") {
            continue;
        }
        // Ids and hashes are not words.
        if lw.chars().filter(|c| c.is_ascii_digit()).count() * 3 >= lw.len() && lw.len() > 4 {
            continue;
        }
        words.push(lw);
    }
    words.join(" ")
}

/// HN items a text links to (`news.ycombinator.com/item?id=123`).
pub fn item_links(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices("ycombinator.com/item?id=") {
        let digits: String = text[i + 24..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// A URL in one form for "the same link": no scheme, no `www.`, no
/// trailing slash, no fragment, no tracking parameters.
pub fn canonical_url(url: &str) -> String {
    let u = url.trim();
    let Some((_, rest)) = u.split_once("://") else { return String::new() };
    let rest = rest.split('#').next().unwrap_or("");
    let (hostpath, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut hp = hostpath.to_string();
    if let Some(h) = hp.strip_prefix("www.") {
        hp = h.to_string();
    }
    if let Some(i) = hp.find('/') {
        let (h, p) = hp.split_at(i);
        hp = format!("{}{}", h.to_ascii_lowercase(), p);
    } else {
        hp = hp.to_ascii_lowercase();
    }
    while hp.ends_with('/') {
        hp.pop();
    }
    for idx in ["/index.html", "/index.htm", "/index.php"] {
        if let Some(h) = hp.strip_suffix(idx) {
            hp = h.to_string();
        }
    }
    let keep: Vec<&str> = query.split('&').filter(|kv| !kv.is_empty()).filter(|kv| {
        let k = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
        !(k.starts_with("utm_") || matches!(k.as_str(), "ref" | "ref_src" | "source" | "fbclid" | "gclid" | "mc_cid" | "mc_eid" | "s" | "si" | "smid" | "cmpid" | "share"))
    }).collect();
    if keep.is_empty() {
        hp
    } else {
        format!("{hp}?{}", keep.join("&"))
    }
}

/// Topics: a curated list, each with the words or phrases that name it.
/// A title (and, for text posts, the first part of the text) names a topic
/// when it holds one of them as whole words. Short ambiguous words (`go`,
/// `c`, `r`) are left to their longer names.
pub const TOPICS: &[(&str, &[&str])] = &[
    // Languages
    ("rust", &["rust", "rustlang", "rustc", "cargo crate", "crates.io"]),
    ("go", &["golang", "go 1", "goroutine", "goroutines"]),
    ("python", &["python", "cpython", "pypy", "pip install", "django", "flask", "fastapi", "numpy", "pandas", "pytorch"]),
    ("javascript", &["javascript", "js", "node.js", "nodejs", "npm", "typescript", "deno", "bun", "react", "vue", "svelte", "angular", "next.js", "nextjs", "webpack", "esm"]),
    ("typescript", &["typescript", "tsc"]),
    ("c++", &["c++", "cpp", "c++20", "c++23", "c++26", "stl"]),
    ("c", &["c programming", "c language", "c99", "c11", "c23", "libc", "gcc", "clang"]),
    ("java", &["java", "jvm", "openjdk", "kotlin", "scala", "spring boot", "graalvm"]),
    ("kotlin", &["kotlin"]),
    ("swift", &["swift", "swiftui"]),
    ("haskell", &["haskell", "ghc"]),
    ("lisp", &["lisp", "common lisp", "clojure", "scheme", "racket", "emacs lisp", "elisp"]),
    ("elixir", &["elixir", "erlang", "beam", "phoenix liveview", "otp"]),
    ("zig", &["zig", "ziglang"]),
    ("ruby", &["ruby", "rails", "ruby on rails"]),
    ("php", &["php", "laravel", "wordpress"]),
    ("ocaml", &["ocaml", "f#", "fsharp"]),
    ("webassembly", &["webassembly", "wasm", "wasi"]),
    ("sql", &["sql", "sqlite", "postgres", "postgresql", "mysql", "mariadb"]),
    // Data
    ("databases", &["database", "databases", "postgres", "postgresql", "mysql", "sqlite", "mongodb", "redis", "cassandra", "dynamodb", "clickhouse", "duckdb", "cockroachdb", "tidb", "elasticsearch", "opensearch", "vector database", "oltp", "olap", "query planner"]),
    ("postgres", &["postgres", "postgresql", "pgvector", "psql"]),
    ("sqlite", &["sqlite", "libsql", "litestream"]),
    ("search", &["search engine", "full-text search", "full text search", "bm25", "elasticsearch", "lucene", "algolia", "meilisearch", "typesense", "vector search", "semantic search", "retrieval"]),
    ("data engineering", &["data pipeline", "etl", "spark", "kafka", "airflow", "dbt", "data warehouse", "snowflake", "bigquery", "parquet", "arrow", "data lake", "iceberg"]),
    // AI
    ("ai", &["ai", "artificial intelligence", "agi", "openai", "anthropic", "deepmind", "chatgpt", "gpt", "gpt-4", "gpt-5", "claude", "gemini", "llama", "mistral", "llm", "llms", "large language model", "language model", "language models", "machine learning", "deep learning", "neural network", "neural networks", "transformer", "transformers", "diffusion", "stable diffusion", "midjourney", "copilot", "ai agent", "ai agents", "agentic", "fine-tuning", "rag", "embeddings"]),
    ("llms", &["llm", "llms", "large language model", "large language models", "chatgpt", "gpt-3", "gpt-4", "gpt-5", "claude", "gemini", "llama", "mistral", "deepseek", "qwen", "prompt engineering", "fine-tuning", "rag", "context window", "hallucination", "hallucinations"]),
    ("machine learning", &["machine learning", "ml", "deep learning", "neural network", "neural networks", "pytorch", "tensorflow", "jax", "gradient descent", "reinforcement learning", "kaggle", "training data"]),
    ("ai coding", &["copilot", "cursor", "claude code", "codex", "ai coding", "coding agent", "coding agents", "vibe coding", "vibe-coding", "code generation", "ai-assisted", "ai assisted"]),
    ("robotics", &["robot", "robots", "robotics", "humanoid", "boston dynamics"]),
    // Systems and infrastructure
    ("linux", &["linux", "kernel", "ubuntu", "debian", "fedora", "arch linux", "systemd", "gnome", "kde", "wayland", "x11", "nixos", "nix"]),
    ("kubernetes", &["kubernetes", "k8s", "helm", "kubectl", "k3s"]),
    ("docker", &["docker", "containers", "container", "podman", "dockerfile"]),
    ("cloud", &["aws", "amazon web services", "azure", "gcp", "google cloud", "cloudflare", "digitalocean", "hetzner", "serverless", "lambda", "s3", "ec2", "heroku", "vercel", "fly.io", "cloud computing"]),
    ("devops", &["devops", "ci/cd", "terraform", "ansible", "observability", "monitoring", "prometheus", "grafana", "sre", "incident", "outage", "postmortem", "on-call"]),
    ("distributed systems", &["distributed systems", "distributed system", "consensus", "raft", "paxos", "crdt", "crdts", "eventual consistency", "replication", "sharding", "jepsen"]),
    ("operating systems", &["operating system", "operating systems", "kernel", "freebsd", "openbsd", "netbsd", "illumos", "plan 9", "unix", "posix"]),
    ("networking", &["tcp", "udp", "http/2", "http/3", "quic", "dns", "bgp", "ipv6", "ipv4", "wireguard", "vpn", "tailscale", "networking", "router", "cdn"]),
    ("compilers", &["compiler", "compilers", "llvm", "jit", "interpreter", "bytecode", "parser", "type system", "type inference", "garbage collector", "garbage collection"]),
    ("performance", &["latency", "benchmark", "benchmarks", "profiling", "optimization", "simd"]),
    ("hardware", &["cpu", "gpu", "gpus", "risc-v", "riscv", "x86", "fpga", "asic", "semiconductor", "semiconductors", "tsmc", "intel", "amd", "nvidia", "apple silicon", "m1", "m2", "m3", "m4", "raspberry pi", "arduino", "esp32", "soldering"]),
    ("apple", &["apple", "iphone", "ipad", "macos", "mac", "macbook", "ios", "app store", "xcode", "vision pro", "tim cook", "steve jobs"]),
    ("google", &["google", "alphabet", "android", "chrome", "chromium", "youtube", "gmail", "sundar pichai"]),
    ("microsoft", &["microsoft", "windows", "windows 10", "windows 11", "azure", "github", "vs code", "vscode", "satya nadella", "bill gates", "xbox"]),
    ("amazon", &["amazon", "aws", "jeff bezos", "kindle", "alexa"]),
    ("meta", &["facebook", "meta", "instagram", "whatsapp", "zuckerberg", "oculus", "threads"]),
    ("tesla", &["tesla", "elon musk", "spacex", "starlink", "twitter", "x.com", "neuralink"]),
    ("browsers", &["browser", "browsers", "firefox", "chrome", "chromium", "safari", "webkit", "gecko", "mozilla", "ladybird", "servo"]),
    ("web development", &["html", "css", "web development", "frontend", "front-end", "backend", "web app", "web apps", "htmx", "rest api", "graphql", "websocket", "websockets", "tailwind"]),
    ("mobile", &["android", "ios", "mobile app", "mobile apps", "smartphone", "smartphones", "flutter", "react native"]),
    ("editors", &["vim", "neovim", "emacs", "vs code", "vscode", "zed", "sublime text", "jetbrains", "intellij", "ide", "text editor"]),
    ("git", &["git", "github", "gitlab", "version control", "jujutsu", "mercurial", "pull request", "monorepo"]),
    ("open source", &["open source", "open-source", "foss", "free software", "gpl", "agpl", "mit license", "apache license", "license", "licensing", "maintainer", "maintainers", "fork"]),
    ("programming", &["programming", "programmer", "programmers", "software engineering", "coding", "code review", "refactoring", "testing", "unit tests", "tdd", "design patterns", "functional programming", "object-oriented", "oop", "type safety", "technical debt"]),
    ("security", &["vulnerability", "vulnerabilities", "exploit", "exploits", "cve", "malware", "ransomware", "hacked", "breach", "data breach", "phishing", "zero-day", "0day", "backdoor", "encryption", "cryptography", "tls", "ssl", "openssl", "passwords", "password", "2fa", "passkeys", "infosec"]),
    ("privacy", &["privacy", "surveillance", "tracking", "gdpr", "nsa", "snowden", "encryption", "end-to-end", "signal", "tor", "data protection", "facial recognition", "cookies", "ad tracking"]),
    ("crypto", &["bitcoin", "ethereum", "crypto", "cryptocurrency", "cryptocurrencies", "blockchain", "nft", "nfts", "web3", "defi", "stablecoin", "stablecoins", "coinbase", "ftx", "satoshi"]),
    ("gaming", &["game", "games", "gaming", "video game", "video games", "steam", "nintendo", "playstation", "xbox", "unity", "unreal engine", "godot", "game engine", "game dev", "gamedev"]),
    ("graphics", &["graphics", "opengl", "vulkan", "directx", "metal", "shader", "shaders", "ray tracing", "rendering", "3d", "webgl", "webgpu"]),
    // Work and business
    ("startups", &["startup", "startups", "founder", "founders", "y combinator", "yc", "seed round", "series a", "vc", "vcs", "venture capital", "fundraising", "unicorn", "bootstrapped", "bootstrapping", "saas", "product market fit", "pivot"]),
    ("careers", &["career", "careers", "hiring", "interview", "interviews", "salary", "salaries", "layoffs", "layoff", "resume", "recruiter", "recruiters", "senior engineer", "promotion", "tech jobs", "job market"]),
    ("remote work", &["remote work", "remote", "work from home", "wfh", "return to office", "rto", "hybrid work", "digital nomad", "coworking"]),
    ("management", &["leadership", "engineering manager", "ceo", "cto", "okrs", "agile", "scrum"]),
    ("productivity", &["productivity", "procrastination", "habits", "deep work", "note-taking", "obsidian", "notion", "time management"]),
    ("economics", &["economy", "economics", "inflation", "recession", "interest rates", "federal reserve", "the fed", "gdp", "stock market", "stocks", "tariff", "tariffs", "trade war", "housing market", "housing", "rent"]),
    ("finance", &["bank", "banks", "banking", "fintech", "stripe", "paypal", "payments", "credit card", "investing", "investors", "ipo", "hedge fund", "wall street", "venture debt"]),
    ("law", &["lawsuit", "lawsuits", "court", "supreme court", "judge", "copyright", "patent", "patents", "antitrust", "ftc", "doj", "regulation", "regulations", "dmca", "eu commission", "gdpr"]),
    ("politics", &["election", "elections", "congress", "senate", "president", "trump", "biden", "democrats", "republicans", "eu", "european union", "brexit"]),
    ("media", &["journalism", "newspaper", "podcast", "podcasts", "netflix", "spotify", "streaming", "social media", "reddit", "tiktok", "mastodon", "bluesky", "substack", "youtube"]),
    // Science and world
    ("science", &["scientists", "physics", "chemistry", "biology", "mathematics", "math", "quantum", "particle", "experiment"]),
    ("space", &["space", "nasa", "spacex", "rocket", "rockets", "mars", "moon", "lunar", "orbit", "satellite", "satellites", "telescope", "jwst", "james webb", "asteroid", "starship", "esa"]),
    ("physics", &["physics", "quantum", "relativity", "particle", "cern", "fusion", "dark matter", "black hole", "black holes", "gravitational waves"]),
    ("math", &["math", "mathematics", "mathematician", "theorem", "proof", "proofs", "prime", "primes", "algebra", "geometry", "topology", "calculus", "lean", "statistics"]),
    ("biology", &["biology", "dna", "rna", "gene", "genes", "genome", "crispr", "protein", "proteins", "cells", "evolution", "bacteria", "virus", "neuroscience", "brain"]),
    ("health", &["health", "medicine", "medical", "doctor", "doctors", "hospital", "fda", "drug", "drugs", "cancer", "covid", "covid-19", "vaccine", "vaccines", "sleep", "exercise", "diet", "nutrition", "obesity", "ozempic", "mental health", "depression", "adhd", "alzheimer"]),
    ("climate", &["climate", "climate change", "global warming", "carbon", "co2", "emissions", "renewable", "renewables", "solar", "wind power", "battery", "batteries", "nuclear", "nuclear power", "energy", "grid", "electric vehicles", "ev", "evs", "heat pump"]),
    ("energy", &["energy", "solar", "wind", "nuclear", "fusion", "battery", "batteries", "grid", "oil", "gas", "power plant", "electricity"]),
    ("transportation", &["car", "cars", "self-driving", "autonomous", "waymo", "cruise", "uber", "lyft", "tesla", "ev", "electric vehicles", "trains", "rail", "high-speed rail", "bike", "bikes", "cycling", "aviation", "airline", "airlines", "boeing", "airbus", "flight"]),
    ("history", &["history", "historical", "ancient", "medieval", "archaeology", "archaeologists", "wwii", "world war"]),
    ("education", &["education", "school", "schools", "university", "universities", "college", "students", "teacher", "teachers", "learning", "mooc", "phd", "academia"]),
    ("books", &["book", "books", "novel", "reading", "author", "library", "libraries", "kindle", "ebook", "sci-fi", "science fiction"]),
    ("design", &["ux", "typography", "font", "fonts", "user interface", "usability", "accessibility", "figma"]),
    ("hn meta", &["hacker news", "dang", "ycombinator"]),
];

/// The word sequence a text names topics by: lowercase words, `.`, `+`,
/// `#`, `/` and `-` kept inside them (`node.js`, `c++`, `ci/cd`).
pub fn topic_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || matches!(c, '+' | '#' | '-' | '.' | '/') && !cur.is_empty() {
            cur.extend(c.to_lowercase());
        } else {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur).trim_end_matches(['.', '-', '/']).to_string());
            }
        }
    }
    if !cur.is_empty() {
        words.push(cur.trim_end_matches(['.', '-', '/']).to_string());
    }
    words
}

/// The topics a text names (a title, with the start of a text post's text).
pub fn topics(text: &str) -> Vec<&'static str> {
    let words = topic_words(text);
    let mut out: Vec<&'static str> = Vec::new();
    for (topic, names) in TOPICS {
        if out.contains(topic) {
            continue;
        }
        let hit = names.iter().any(|name| {
            let nw: Vec<&str> = name.split(' ').collect();
            words.windows(nw.len()).any(|w| w.iter().zip(&nw).all(|(a, b)| a == b))
        });
        if hit {
            out.push(topic);
        }
    }
    out
}

/// The first `n` characters of a text, cut at a word.
pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    match cut.rfind(char::is_whitespace) {
        Some(i) if i > n / 2 => format!("{}…", &cut[..i]),
        _ => format!("{cut}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_hn_text() {
        let h = "I&#x27;d say <i>no</i>.<p>See <a href=\"https:&#x2F;&#x2F;example.com&#x2F;a?b=1\" rel=\"nofollow\">https:&#x2F;&#x2F;example.com&#x2F;a?b=1</a><p><pre><code>  fn main() {}\n  x\n</code></pre>after";
        let t = html_to_text(h);
        assert_eq!(t, "I'd say *no*.\n\nSee https://example.com/a?b=1\n\n    fn main() {}\n    x\n\nafter");
        assert_eq!(unescape("a &amp; b &#8212; c &bogus; d"), "a & b — c &bogus; d");
    }

    #[test]
    fn kinds_domains_sites() {
        assert_eq!(kind(1, "Show HN: A thing"), "show");
        assert_eq!(kind(1, "Ask HN: Who is hiring?"), "ask");
        assert_eq!(kind(5, "Acme is hiring"), "job");
        assert_eq!(kind(1, "Rust 2.0"), "story");
        assert_eq!(domain("https://www.Example.com:443/x?y"), "example.com");
        assert_eq!(site("https://github.com/Tokio-rs/axum/pull/1"), "github.com/tokio-rs");
        assert_eq!(site("https://blog.rust-lang.org/2024/x.html"), "blog.rust-lang.org");
        assert_eq!(site(""), "");
        assert_eq!(url_words("https://github.com/tokio-rs/axum/blob/a1b2c3d4e5f6/README.md"), "github tokio rs axum blob readme md");
        assert_eq!(canonical_url("https://www.Example.com/a/?utm_source=hn&id=3#top"), "example.com/a?id=3");
        assert_eq!(item_links("see https://news.ycombinator.com/item?id=123 and news.ycombinator.com/item?id=45"), vec![123, 45]);
    }

    #[test]
    fn topics_by_whole_words() {
        assert_eq!(topics("Show HN: A Rust crate for Postgres"), vec!["rust", "sql", "databases", "postgres"]);
        assert!(topics("Trusting the process").is_empty());
        assert!(topics("Node.js 22 released").contains(&"javascript"));
        assert!(topics("The C++26 committee").contains(&"c++"));
    }
}
