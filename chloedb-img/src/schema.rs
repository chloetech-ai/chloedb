//! The app's tables, ordinary ChloeDB tables that any SQL reads: the
//! pictures, each with its record and its CLIP vector; the themes, with
//! their centres; and the colors of each picture whose thumbnail was made,
//! as a histogram vector, so that a color search is a vector search too.

use crate::strata::{Result, Strata};

/// Dimensions of a color histogram: 4 lightness bins by 6 by 6 bins of
/// Lab's a and b ([`crate::colors`]).
pub const COLOR_DIM: usize = 4 * 6 * 6;

pub fn tables() -> Vec<(&'static str, String)> {
    vec![
        ("img_pictures", "CREATE TABLE IF NOT EXISTS img_pictures (
            id KEYWORD PRIMARY KEY,
            n INT,
            url KEYWORD OPTIONS(indexed = false),
            title TEXT,
            caption TEXT,
            source KEYWORD,
            licence KEYWORD,
            width INT, height INT, mp FLOAT,
            shape KEYWORD, size KEYWORD,
            kind KEYWORD, theme INT,
            hash KEYWORD, mime KEYWORD OPTIONS(indexed = false),
            clip VECTOR(768) OPTIONS(index = 'hnsw')
        )".to_string()),
        ("img_themes", "CREATE TABLE IF NOT EXISTS img_themes (
            id INT PRIMARY KEY,
            name TEXT, n INT, grp KEYWORD, row INT, col INT,
            reps INT[], near INT[], kinds TEXT OPTIONS(indexed = false),
            centre VECTOR(768) OPTIONS(index = 'flat')
        )".to_string()),
        ("img_colors", format!("CREATE TABLE IF NOT EXISTS img_colors (
            id KEYWORD PRIMARY KEY,
            kind KEYWORD, source KEYWORD,
            palette TEXT OPTIONS(indexed = false),
            colors VECTOR({COLOR_DIM}) OPTIONS(index = 'flat')
        )")),
        // Where each picture is on the collection's own site, when it was
        // found (Wikimedia Commons, by the file's SHA-1).
        ("img_links", "CREATE TABLE IF NOT EXISTS img_links (
            id KEYWORD PRIMARY KEY,
            page KEYWORD OPTIONS(indexed = false), title TEXT, found BOOL
        )".to_string()),
    ]
}

pub async fn create(db: &Strata) -> Result<()> {
    for (_, sql) in tables() {
        db.sql(&sql).await?;
    }
    Ok(())
}
