//! Parley's tables, as ChloeDB SQL (schema.sql), and creating them.

use serde_json::json;

use crate::db::Db;
use crate::util::Result;

const SCHEMA: &str = include_str!("schema.sql");
const IMAGE_COLS: &str = include_str!("image_cols.sql");
const MIGRATIONS: &str = include_str!("migrations.sql");
pub const RECOMMENDER: &str = include_str!("recommender.sql");

/// The reactions a post can get: (kind, emoji, its counter column on posts).
pub const REACTIONS: [(&str, &str, &str); 6] = [
    ("like", "👍", "likes"),
    ("love", "❤️", "r_love"),
    ("laugh", "😂", "r_laugh"),
    ("insight", "💡", "r_insight"),
    ("wow", "😮", "r_wow"),
    ("hmm", "🤔", "r_hmm"),
];

pub const POST_KINDS: [&str; 6] = ["text", "link", "picture", "meme", "drawing", "app"];

pub fn reaction_col(kind: &str) -> Option<&'static str> {
    REACTIONS.iter().find(|r| r.0 == kind).map(|r| r.2)
}

pub fn reaction_cols() -> String {
    REACTIONS.iter().map(|r| r.2).collect::<Vec<_>>().join(", ")
}

/// The CREATE TABLE statements, in order.
pub fn statements(vision: bool) -> Vec<String> {
    SCHEMA
        .split("\n;;\n")
        .map(|s| {
            s.lines()
                .skip_while(|l| l.trim().is_empty() || l.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|s| s.starts_with("CREATE"))
        .map(|s| {
            s.replace(
                "{IMAGE_COLS}",
                if vision { IMAGE_COLS.trim_end() } else { "" },
            )
        })
        .collect()
}

fn table_of(stmt: &str) -> String {
    stmt.split("EXISTS")
        .nth(1)
        .unwrap_or("")
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Creates the tables that are missing, runs the migrations, and makes the
/// feed's recommender where the database can keep one. Returns what it made.
pub async fn ensure(db: &Db) -> Result<Vec<String>> {
    let have = db.tables().await?;
    let mut made = Vec::new();
    if !have.iter().any(|t| t == "meta") && have.is_empty() {
        db.sql("CREATE TABLE IF NOT EXISTS meta (k KEYWORD PRIMARY KEY, v TEXT OPTIONS(indexed = false))", json!({})).await?;
        made.push("meta".to_string());
    }
    for stmt in statements(db.vision) {
        let name = table_of(&stmt);
        if !have.contains(&name) {
            db.sql(&stmt, json!({})).await?;
            made.push(name);
        }
    }
    for m in MIGRATIONS
        .split("\n;;\n")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let _ = db.sql(m, json!({})).await;
    }
    if db.graph_in_db() {
        if have.iter().any(|t| t == "taste_counts") {
            db.recommender
                .store(true, std::sync::atomic::Ordering::Relaxed);
        } else if db.sql(RECOMMENDER.trim(), json!({})).await.is_ok() {
            db.recommender
                .store(true, std::sync::atomic::Ordering::Relaxed);
            made.push("recommender taste".into());
        }
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_is_created_and_vision_adds_columns() {
        let plain = statements(false);
        let vis = statements(true);
        assert_eq!(plain.len(), vis.len());
        assert!(plain.len() >= 27);
        let posts = plain.iter().find(|s| table_of(s) == "posts").unwrap();
        assert!(!posts.contains("{IMAGE_COLS}") && !posts.contains("img_emb"));
        assert!(vis.iter().any(|s| s.contains("img_emb")));
        for t in [
            "agents",
            "posts",
            "comments",
            "techniques",
            "trend_topics",
            "experiments",
        ] {
            assert!(plain.iter().any(|s| table_of(s) == t), "{t}");
        }
    }
}
