//! The app's tables. Every one is an ordinary ChloeDB table that any SQL
//! reads: the stories and comments with their text indexed for words and
//! for meaning, the people, and two graphs as edge tables (who replied to
//! whom, and which discussion links to which).

use serde_json::Value;

use crate::strata::{Result, Strata};

// Titles, links and comments keep a substring index too (an FM-index per
// segment): `LIKE '%std::move%'` finds what the word index cannot, the
// code, paths, versions and punctuation HN writes, and counts a phrase's
// uses by the phrase's length, not the table's size.

/// The embedding model of titles and comments: a static model, tens of
/// microseconds a text on one core, so the whole archive embeds on a CPU.
pub const MODEL: &str = "potion-retrieval-32M";

/// The tables, the stories' titles and texts embedded by `model`, the
/// comments by `comment_model` (`none`: the comments have no vectors, and
/// are searched by their words; vectors take most of a comment's bytes).
pub fn tables(model: &str, comment_model: &str) -> Vec<(&'static str, String)> {
    let comment_emb = if comment_model == "none" || comment_model.is_empty() { String::new() } else { format!(",\n            emb VECTOR OPTIONS(embed = 'body', model = '{comment_model}')") };
    vec![
        ("hn_users", "CREATE TABLE IF NOT EXISTS hn_users (
            id KEYWORD PRIMARY KEY,
            stories INT, comments INT, points INT,
            first TIMESTAMP, last TIMESTAMP,
            replies_got INT, talked_to INT,
            about TEXT, karma INT
        ) OPTIONS(cluster_by = 'id')".to_string()),
        ("hn_stories", format!("CREATE TABLE IF NOT EXISTS hn_stories (
            id INT PRIMARY KEY,
            title TEXT OPTIONS(substring = true),
            body TEXT,
            url KEYWORD OPTIONS(substring = true), curl KEYWORD, domain KEYWORD, site KEYWORD,
            urlwords TEXT OPTIONS(analyzer = 'simple', positions = false),
            author KEYWORD REFERENCES hn_users,
            created TIMESTAMP, year INT,
            points INT, comments INT,
            kind KEYWORD, topics KEYWORD[],
            pop FLOAT,
            emb VECTOR OPTIONS(embed = 'title, body', model = '{model}')
        )")),
        ("hn_comments", format!("CREATE TABLE IF NOT EXISTS hn_comments (
            id INT PRIMARY KEY,
            body TEXT OPTIONS(substring = true),
            author KEYWORD REFERENCES hn_users,
            created TIMESTAMP, year INT,
            parent INT, parent_author KEYWORD,
            story INT REFERENCES hn_stories, story_title TEXT,
            depth INT, pos INT, replies INT, thread INT, chars INT,
            pop FLOAT{comment_emb}
        ) OPTIONS(cluster_by = 'story')")),
        // Who replied to whom: an edge from the one who replied to the one
        // they answered, weighted by how often.
        ("hn_replies", "CREATE TABLE IF NOT EXISTS hn_replies (
            id KEYWORD PRIMARY KEY,
            src KEYWORD NOT NULL REFERENCES hn_users,
            dst KEYWORD NOT NULL REFERENCES hn_users,
            w FLOAT
        ) OPTIONS(edges = 'src, dst, w')".to_string()),
        // Which discussion links to which (a story or one of its comments
        // naming news.ycombinator.com/item?id=...).
        ("hn_links", "CREATE TABLE IF NOT EXISTS hn_links (
            id KEYWORD PRIMARY KEY,
            src INT NOT NULL REFERENCES hn_stories,
            dst INT NOT NULL REFERENCES hn_stories,
            w FLOAT
        ) OPTIONS(edges = 'src, dst, w')".to_string()),
        // What the app keeps for itself: the live feed's place, answers it
        // made, searches people ran (their words and counts only).
        ("hn_state", "CREATE TABLE IF NOT EXISTS hn_state (
            id KEYWORD PRIMARY KEY, val TEXT OPTIONS(indexed = false), n INT, at TIMESTAMP
        )".to_string()),
        ("hn_answers", "CREATE TABLE IF NOT EXISTS hn_answers (
            id KEYWORD PRIMARY KEY, kind KEYWORD, question TEXT, answer TEXT OPTIONS(indexed = false),
            sources TEXT OPTIONS(indexed = false), model KEYWORD, at TIMESTAMP, ms FLOAT
        )".to_string()),
        ("hn_searches", "CREATE TABLE IF NOT EXISTS hn_searches (
            id KEYWORD PRIMARY KEY, n INT, last TIMESTAMP
        ) OPTIONS(cluster_by = 'id')".to_string()),
        // Autocomplete: phrases of titles, sites, topics and people, keyed
        // `p:`, `s:`, `t:`, `u:` and their text, completed by a prefix of
        // the key (crate::complete).
        ("hn_complete", "CREATE TABLE IF NOT EXISTS hn_complete (
            id KEYWORD PRIMARY KEY, kind KEYWORD, text KEYWORD, n INT, weight FLOAT
        ) OPTIONS(cluster_by = 'id')".to_string()),
        // Trending: each month's terms of titles with their counts (term
        // `*` is the month's count of titles), and each month's spikes
        // against the twelve before it (crate::trending).
        ("hn_terms", "CREATE TABLE IF NOT EXISTS hn_terms (
            id KEYWORD PRIMARY KEY, month KEYWORD, term KEYWORD, n INT, points INT, top INT
        )".to_string()),
        ("hn_spikes", "CREATE TABLE IF NOT EXISTS hn_spikes (
            id KEYWORD PRIMARY KEY, month KEYWORD, year INT, rank INT, term KEYWORD, n INT,
            expected FLOAT, ratio FLOAT, score FLOAT, points INT, top INT, top_title TEXT
        ) OPTIONS(cluster_by = 'id')".to_string()),
        // The pages the stories link to, as `crawl` read them (crate::crawl):
        // keyed by the link's canonical form (the stories' `curl`), with
        // what came of asking (`status`: ok, noindex, robots, http_4xx,
        // timeout, ...), the page's text and its fields, and a vector of
        // its title, description and text. A page that asks not to be
        // indexed keeps no text.
        ("hn_pages", format!("CREATE TABLE IF NOT EXISTS hn_pages (
            id KEYWORD PRIMARY KEY,
            url KEYWORD, final KEYWORD, site KEYWORD,
            story INT REFERENCES hn_stories, stories INT, points INT, year INT,
            status KEYWORD, code INT, fetched TIMESTAMP, ms INT, bytes INT,
            title TEXT, description TEXT, body TEXT,
            words INT, lang KEYWORD, published TIMESTAMP, author KEYWORD,
            image KEYWORD, site_name KEYWORD, kind KEYWORD,
            emb VECTOR OPTIONS(embed = 'title, description, body', model = '{model}')
        )")),
    ]
}

/// The words HN people write for the same thing, as a synonym list in
/// Solr's format: kept with the stories' and comments' tables, used by
/// searches with `synonyms => 1`.
pub const SYNONYMS: &str = "k8s, kubernetes
postgres, postgresql
js, javascript
ts, typescript
golang, go language
ml, machine learning
llm, large language model
llms, large language models
ai, artificial intelligence
nn, neural network
rl, reinforcement learning
nlp, natural language processing
cv, computer vision
os, operating system
db, database
dbs, databases
fp, functional programming
oop, object oriented
wasm, webassembly
gc, garbage collection
vm, virtual machine
vms, virtual machines
yc, y combinator
hn, hacker news
wfh, work from home
rto, return to office
ev, electric vehicle
evs, electric vehicles
ui, user interface
ux, user experience
cli, command line
tui, terminal ui
ci, continuous integration
gpu, graphics card
oss, open source
foss, free software
vc, venture capital
vcs, venture capitalists
saas, software as a service
ipo, initial public offering
mac, macos
osx, macos
fb, facebook
ms, microsoft
msft, microsoft
aws, amazon web services
gcp, google cloud
rpi, raspberry pi
cpp, c++
nyc, new york
sf, san francisco
uk, united kingdom
eu, european union
gpt4, gpt-4
chatgpt, chat gpt
e2e, end to end
2fa, two factor
mfa, multi factor
vpn, virtual private network
dns, domain name system
tdd, test driven development
ddd, domain driven design
orm, object relational mapping
regex, regular expression
regexes, regular expressions
repo, repository
repos, repositories
k8, kubernetes
nix, nixos";

/// Makes the tables that are not there yet, and the synonym lists.
pub async fn ensure(db: &Strata, model: &str, comment_model: &str) -> Result<Vec<String>> {
    let have = db.tables().await?;
    let mut made = Vec::new();
    for (name, sql) in tables(model, comment_model) {
        if !have.iter().any(|t| t == name) {
            db.sql(&sql).await?;
            made.push(name.to_string());
        }
    }
    // A column added since the table was first made.
    if have.iter().any(|t| t == "hn_pages") {
        db.sql("ALTER TABLE hn_pages ADD COLUMN IF NOT EXISTS year INT").await?;
    }
    for t in ["hn_stories", "hn_comments", "hn_pages"] {
        let _ = db.sql(&format!("CREATE OR REPLACE SYNONYMS ON {t} AS '{}'", SYNONYMS.replace('\'', "''"))).await;
    }
    Ok(made)
}

/// How many rows each table holds.
pub async fn counts(db: &Strata) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (name, _) in tables(MODEL, MODEL) {
        let n = db.query(&format!("SELECT COUNT(*) AS n FROM {name}"), Value::Null).await.ok().and_then(|r| r.first().and_then(|r| r.get("n").cloned())).unwrap_or(Value::Null);
        out.insert(name.to_string(), n);
    }
    Ok(Value::Object(out))
}
