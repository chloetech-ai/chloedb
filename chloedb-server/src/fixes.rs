//! One-click fixes: a statement refused for a name it misspelt, rewritten
//! with each name the error says it may have meant.
//!
//! An error such as `no such column: titel in movies (did you mean
//! title?)` names the word and the names near it ([`chloedb::query::hint`]).
//! The answer of `POST /v1/sql` and `POST /v1/query` then carries `fixes`:
//! `[{"label": "title for titel", "sql": "SELECT title FROM movies"}]`,
//! the statement with the word replaced where it stands as a name (not in
//! a string), which the command center offers as a button and the MCP
//! tools as the statement to run. Nothing is run for the caller.

use serde_json::{json, Value as Json};

/// The fixes of `sql` that `error` suggests: none when it names no word
/// with names near it, or the word is not in the statement as a name.
pub fn fixes(sql: &str, error: &str) -> Vec<Json> {
    let mut out = Vec::new();
    for kind in ["no such column: ", "no such table: ", "no such function: "] {
        let Some(at) = error.find(kind) else { continue };
        let rest = &error[at + kind.len()..];
        let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect();
        let Some(m) = rest.find("(did you mean ") else { continue };
        let Some(end) = rest[m..].find("?)") else { continue };
        let names = &rest[m + "(did you mean ".len()..m + end];
        let word = word.rsplit('.').next().unwrap_or(&word).to_string();
        if word.is_empty() {
            continue;
        }
        for name in names.split(", ").flat_map(|n| n.split(" or ")).map(str::trim).filter(|n| !n.is_empty()).take(3) {
            if let Some(fixed) = replace_name(sql, &word, name) {
                out.push(json!({"label": format!("{name} for {word}"), "sql": fixed}));
            }
        }
    }
    out
}

/// `sql` with every `word` that stands as a name (any case, bare or
/// double-quoted, not inside a 'string' or a comment) replaced by `name`;
/// None when there is none.
fn replace_name(sql: &str, word: &str, name: &str) -> Option<String> {
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len() + 8);
    let mut i = 0;
    let mut n = 0;
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' {
            // A string, with '' inside it.
            let start = i;
            i += 1;
            while i < b.len() {
                if b[i] == b'\'' {
                    if b.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push_str(&sql[start..i]);
        } else if c == b'-' && b.get(i + 1) == Some(&b'-') {
            let start = i;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            out.push_str(&sql[start..i]);
        } else if c == b'"' {
            let start = i;
            let close = sql[i + 1..].find('"').map(|k| i + 1 + k);
            match close {
                Some(e) if sql[i + 1..e].eq_ignore_ascii_case(word) => {
                    // A path (`_source.x`) is not one quoted name.
                    if name.contains('.') {
                        out.push_str(name);
                    } else {
                        out.push('"');
                        out.push_str(name);
                        out.push('"');
                    }
                    n += 1;
                    i = e + 1;
                }
                Some(e) => {
                    out.push_str(&sql[start..=e]);
                    i = e + 1;
                }
                None => {
                    out.push_str(&sql[start..]);
                    i = b.len();
                }
            }
        } else if ident(c) {
            let start = i;
            while i < b.len() && ident(b[i]) {
                i += 1;
            }
            let w = &sql[start..i];
            if w.eq_ignore_ascii_case(word) {
                out.push_str(name);
                n += 1;
            } else {
                out.push_str(w);
            }
        } else {
            let ch = sql[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    (n > 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_misspelt_name_is_replaced_where_it_is_a_name() {
        let sql = "SELECT titel, 'titel' AS t FROM movies WHERE titel = 'x''titel' -- titel\nORDER BY \"titel\"";
        let f = fixes(sql, "query error: no such column: titel in movies (did you mean title?); its columns are id, title");
        assert_eq!(f, vec![json!({"label": "title for titel", "sql": "SELECT title, 'titel' AS t FROM movies WHERE title = 'x''titel' -- titel\nORDER BY \"title\""})]);
        let f = fixes("SELECT * FROM movie LIMIT 3", "no such table: movie (did you mean movies or moviez?); the tables are movies, moviez");
        assert_eq!(f.iter().map(|x| x["sql"].as_str().unwrap()).collect::<Vec<_>>(), ["SELECT * FROM movies LIMIT 3", "SELECT * FROM moviez LIMIT 3"]);
        // A qualified name: the word after the dot.
        let f = fixes("SELECT m.yaer FROM movies m", "no such column: m.yaer (did you mean year?)");
        assert_eq!(f[0]["sql"], "SELECT m.year FROM movies m");
        // A field an OpenSearch index keeps in its documents.
        let f = fixes("SELECT \"product\", count(*) FROM os_reviews GROUP BY product", "no such column: product in os_reviews (did you mean _source.product?); its columns are _id, _source");
        assert_eq!(f[0]["sql"], "SELECT _source.product, count(*) FROM os_reviews GROUP BY _source.product");
        // Nothing near, or the word not in the statement as a name.
        assert!(fixes("SELECT zz FROM movies", "no such column: zz in movies; its columns are id").is_empty());
        assert!(fixes("SELECT 'titel' FROM movies", "no such column: titel (did you mean title?)").is_empty());
        // Not a word inside a longer one, and text that is not ASCII stays.
        assert_eq!(fixes("SELECT titels, titel FROM é", "no such column: titel (did you mean title?)")[0]["sql"], "SELECT titels, title FROM é");
    }
}
