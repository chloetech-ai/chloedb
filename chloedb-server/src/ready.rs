//! What `chloedb-server` prints once it serves, when it was started in a
//! terminal: where to open it, a first query and a first import that work
//! as pasted, and how to stop it. A service's log (a file, journald, a
//! pod's) gets only the log lines before it.

/// The server as a person who just started it needs it.
pub struct Ready {
    /// The command center's address, as a browser on this machine opens it.
    pub url: String,
    /// The database file served (`None` for a coordinator).
    pub db: Option<String>,
    /// Shards behind this coordinator (0 when it serves a file).
    pub shards: usize,
    /// The tables served, the apps' own left out.
    pub tables: Vec<String>,
    /// Rows in them all.
    pub rows: u64,
    /// Whether requests need the token (`--token`, `$CHLOEDB_TOKEN`).
    pub token: bool,
}

impl Ready {
    pub fn text(&self) -> String {
        let what = match (&self.db, self.shards) {
            (Some(db), _) => db.clone(),
            (None, n) => format!("a cluster of {n} shard{}", if n == 1 { "" } else { "s" }),
        };
        let tables = match self.tables.len() {
            0 => "no tables yet".to_string(),
            1 => format!("1 table ({}), {} rows", self.tables[0], thousands(self.rows)),
            n => format!("{n} tables, {} rows", thousands(self.rows)),
        };
        let auth = if self.token { " -H \"authorization: Bearer $CHLOEDB_TOKEN\"" } else { "" };
        let sql = match self.tables.first() {
            Some(t) => format!("SELECT * FROM {} LIMIT 3", quoted(t)),
            None => "SHOW TABLES".to_string(),
        };
        let query = format!("curl -s {}/v1/sql{auth} -H 'content-type: application/json' -d '{{\"sql\": \"{sql}\"}}'", self.url);
        let import = format!("curl -s '{}/v1/import?table=NAME'{auth} -H 'content-type: text/csv' --data-binary @FILE.csv", self.url);
        let open = if self.token { format!("{}  (it asks for the token)", self.url) } else { self.url.clone() };
        let mut lines = vec![
            String::new(),
            format!("  ChloeDB {} is ready: {what}, {tables}", env!("CARGO_PKG_VERSION")),
            String::new(),
            format!("    Open        {open}"),
        ];
        // An empty database: loading comes first.
        if self.tables.is_empty() {
            lines.push(format!("    Load a file {import}"));
            lines.push("                (CSV, TSV, JSON or JSON Lines: the table is made from its columns)".into());
            lines.push(format!("    Query       {query}"));
        } else {
            lines.push(format!("    Query       {query}"));
            lines.push(format!("    Load a file {import}"));
        }
        lines.push(if self.db.is_some() { "    Stop        Ctrl+C (the next start opens it as it was)".into() } else { "    Stop        Ctrl+C".into() });
        lines.push(String::new());
        lines.join("\n")
    }
}

/// `addr` as a browser on this machine reaches it: a server listening on
/// every interface (`0.0.0.0`, `[::]`) is reached at loopback.
pub fn browsable(addr: &str) -> String {
    if let Some(port) = addr.strip_prefix("0.0.0.0:") {
        return format!("127.0.0.1:{port}");
    }
    if let Some(port) = addr.strip_prefix("[::]:") {
        return format!("localhost:{port}");
    }
    addr.to_string()
}

/// A table's name as SQL reads it.
fn quoted(t: &str) -> String {
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !t.starts_with(|c: char| c.is_ascii_digit()) {
        t.to_string()
    } else {
        format!("\\\"{}\\\"", t.replace('"', "\"\""))
    }
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Ready {
        Ready { url: "http://127.0.0.1:7700".into(), db: Some("demo.chloedb".into()), shards: 0, tables: vec!["products".into()], rows: 20000, token: false }
    }

    #[test]
    fn says_where_and_what_to_do_next() {
        let t = demo().text();
        assert!(t.contains("is ready: demo.chloedb, 1 table (products), 20,000 rows"), "{t}");
        assert!(t.contains("    Open        http://127.0.0.1:7700\n"), "{t}");
        assert!(t.contains("curl -s http://127.0.0.1:7700/v1/sql -H 'content-type: application/json' -d '{\"sql\": \"SELECT * FROM products LIMIT 3\"}'"), "{t}");
        assert!(t.contains("/v1/import?table=NAME' -H 'content-type: text/csv' --data-binary @FILE.csv"), "{t}");
        assert!(!t.contains("Bearer"), "{t}");
    }

    #[test]
    fn an_empty_database_loads_first_and_a_token_is_sent() {
        let r = Ready { tables: vec![], rows: 0, token: true, db: Some("chloedb.chloedb".into()), ..demo() };
        let t = r.text();
        assert!(t.contains("chloedb.chloedb, no tables yet"), "{t}");
        assert!(t.find("Load a file").unwrap() < t.find("Query").unwrap(), "{t}");
        assert!(t.contains("SHOW TABLES"), "{t}");
        assert!(t.contains("-H \"authorization: Bearer $CHLOEDB_TOKEN\""), "{t}");
        assert!(t.contains("(it asks for the token)"), "{t}");
    }

    #[test]
    fn a_coordinator_and_odd_names() {
        let r = Ready { db: None, shards: 2, tables: vec!["my table".into(), "b".into()], rows: 1234567, ..demo() };
        let t = r.text();
        assert!(t.contains("a cluster of 2 shards, 2 tables, 1,234,567 rows"), "{t}");
        assert!(t.contains("SELECT * FROM \\\"my table\\\" LIMIT 3"), "{t}");
        assert_eq!(browsable("0.0.0.0:7700"), "127.0.0.1:7700");
        assert_eq!(browsable("127.0.0.1:7710"), "127.0.0.1:7710");
    }
}
