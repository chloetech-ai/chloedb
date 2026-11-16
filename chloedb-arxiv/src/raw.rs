//! A paper as arXiv describes it, from either source: the metadata
//! snapshot (arXiv's Kaggle dump, as Parquet) or arXiv's OAI-PMH interface
//! (`arXivRaw` records), which the live feed reads and `prepare` reads for
//! the weeks after the snapshot.

use std::fs::File;
use std::path::Path;

use arrow_array::{Array, ArrayRef, LargeListArray, LargeStringArray, ListArray, RecordBatch, StringArray, StringViewArray, StructArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;

#[derive(Debug, Clone, Default)]
pub struct Raw {
    pub id: String,
    pub title: String,
    pub abstract_: String,
    /// (last name, forenames, suffix), in order.
    pub authors: Vec<(String, String, String)>,
    pub cats: Vec<String>,
    pub comments: String,
    pub journal_ref: String,
    pub doi: String,
    pub license: String,
    /// Each version's time, Unix seconds, in order (v1 first).
    pub versions: Vec<i64>,
}

/// A string column of any of Arrow's three layouts.
pub enum Strs<'a> {
    S(&'a StringArray),
    L(&'a LargeStringArray),
    V(&'a StringViewArray),
    None,
}

impl<'a> Strs<'a> {
    pub fn of(a: Option<&'a ArrayRef>) -> Strs<'a> {
        let Some(a) = a else { return Strs::None };
        Strs::of_dyn(a.as_ref())
    }
    pub fn of_dyn(a: &'a dyn Array) -> Strs<'a> {
        if let Some(s) = a.as_any().downcast_ref::<StringArray>() {
            Strs::S(s)
        } else if let Some(s) = a.as_any().downcast_ref::<LargeStringArray>() {
            Strs::L(s)
        } else if let Some(s) = a.as_any().downcast_ref::<StringViewArray>() {
            Strs::V(s)
        } else {
            Strs::None
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Strs::S(s) => s.len(),
            Strs::L(s) => s.len(),
            Strs::V(s) => s.len(),
            Strs::None => 0,
        }
    }
    pub fn get(&self, i: usize) -> &'a str {
        match self {
            Strs::S(s) if s.is_valid(i) => s.value(i),
            Strs::L(s) if s.is_valid(i) => s.value(i),
            Strs::V(s) if s.is_valid(i) => s.value(i),
            _ => "",
        }
    }
}

pub fn col<'a>(b: &'a RecordBatch, name: &str) -> Option<&'a ArrayRef> {
    b.schema().index_of(name).ok().map(|i| b.column(i))
}

/// The values of row `i` of a list column (a list or a large list).
pub fn list_at(a: &dyn Array, i: usize) -> Option<ArrayRef> {
    if let Some(l) = a.as_any().downcast_ref::<ListArray>() {
        return l.is_valid(i).then(|| l.value(i));
    }
    if let Some(l) = a.as_any().downcast_ref::<LargeListArray>() {
        return l.is_valid(i).then(|| l.value(i));
    }
    None
}

/// Reads `cols` of a Parquet file, a batch at a time.
pub fn read_parquet(path: &Path, cols: &[&str], mut each: impl FnMut(&RecordBatch) -> Result<(), String>) -> Result<(), String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let b = ParquetRecordBatchReaderBuilder::try_new(f).map_err(|e| format!("{}: {e}", path.display()))?;
    let schema = b.parquet_schema();
    let fields = schema.root_schema().get_fields();
    let idx: Vec<usize> = (0..fields.len()).filter(|&i| cols.contains(&fields[i].name())).collect();
    let mask = ProjectionMask::roots(schema, idx);
    let r = b.with_projection(mask).with_batch_size(16_384).build().map_err(|e| format!("{}: {e}", path.display()))?;
    for batch in r {
        let batch = batch.map_err(|e| format!("{}: {e}", path.display()))?;
        each(&batch)?;
    }
    Ok(())
}

/// The snapshot's Parquet files in `dir`, in name order.
pub fn parquet_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut v: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "parquet")).collect();
    v.sort();
    Ok(v)
}

/// A time as the snapshot writes a version's (`Fri, 01 May 2009 01:52:14 GMT`).
pub fn rfc2822(s: &str) -> Option<i64> {
    let s = s.trim();
    chrono::DateTime::parse_from_rfc2822(s).ok().map(|d| d.timestamp()).or_else(|| chrono::DateTime::parse_from_rfc2822(&s.replace(" GMT", " +0000")).ok().map(|d| d.timestamp()))
}

/// The snapshot's papers, `full` with their texts, else only the id, the
/// authors, the categories, the DOI and the versions.
pub fn snapshot(dir: &Path, full: bool, mut each: impl FnMut(Vec<Raw>) -> Result<(), String>) -> Result<(), String> {
    let cols_full = ["id", "authors_parsed", "title", "abstract", "categories", "comments", "journal-ref", "doi", "license", "versions"];
    let cols_lite = ["id", "authors_parsed", "categories", "doi", "versions"];
    let cols: &[&str] = if full { &cols_full } else { &cols_lite };
    for path in parquet_files(dir)? {
        read_parquet(&path, cols, |b| {
            let id = Strs::of(col(b, "id"));
            let title = Strs::of(col(b, "title"));
            let abs = Strs::of(col(b, "abstract"));
            let cats = Strs::of(col(b, "categories"));
            let comments = Strs::of(col(b, "comments"));
            let jref = Strs::of(col(b, "journal-ref"));
            let doi = Strs::of(col(b, "doi"));
            let lic = Strs::of(col(b, "license"));
            let authors = col(b, "authors_parsed");
            let versions = col(b, "versions");
            let mut out = Vec::with_capacity(b.num_rows());
            for i in 0..b.num_rows() {
                let mut r = Raw { id: id.get(i).trim().to_string(), ..Default::default() };
                if r.id.is_empty() {
                    continue;
                }
                if full {
                    r.title = title.get(i).to_string();
                    r.abstract_ = abs.get(i).to_string();
                    r.comments = comments.get(i).to_string();
                    r.journal_ref = jref.get(i).to_string();
                    r.license = lic.get(i).to_string();
                }
                r.cats = cats.get(i).split_whitespace().map(str::to_string).collect();
                r.doi = doi.get(i).trim().to_string();
                if let Some(a) = authors.and_then(|a| list_at(a.as_ref(), i)) {
                    for j in 0..a.len() {
                        if let Some(parts) = list_at(a.as_ref(), j) {
                            let s = Strs::of_dyn(parts.as_ref());
                            let g = |k: usize| if k < s.len() { s.get(k).to_string() } else { String::new() };
                            r.authors.push((g(0), g(1), g(2)));
                        }
                    }
                }
                if let Some(v) = versions.and_then(|a| list_at(a.as_ref(), i)) {
                    if let Some(st) = v.as_any().downcast_ref::<StructArray>() {
                        if let Some(c) = st.column_by_name("created") {
                            let s = Strs::of_dyn(c.as_ref());
                            for k in 0..s.len() {
                                if let Some(t) = rfc2822(s.get(k)) {
                                    r.versions.push(t);
                                }
                            }
                        }
                    }
                }
                out.push(r);
            }
            each(out)
        })?;
    }
    Ok(())
}

/// The records of an OAI-PMH `ListRecords` page in the `arXivRaw` format,
/// and its resumption token (empty on the last page). Deleted records are
/// left out.
pub fn oai_page(xml: &str) -> (Vec<Raw>, String) {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    let mut rd = Reader::from_str(xml);
    rd.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut token = String::new();
    let mut cur: Option<Raw> = None;
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut deleted = false;
    loop {
        match rd.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "header" {
                    deleted = e.attributes().flatten().any(|a| a.key.as_ref() == b"status" && a.value.as_ref() == b"deleted");
                }
                if name == "arXivRaw" {
                    cur = Some(Raw::default());
                }
                path.push(name);
                text.clear();
            }
            Ok(Event::Text(t)) => {
                if let Ok(s) = t.unescape() {
                    text.push_str(&s);
                }
            }
            Ok(Event::CData(t)) => text.push_str(&String::from_utf8_lossy(&t)),
            Ok(Event::End(_)) => {
                let name = path.pop().unwrap_or_default();
                let parent = path.last().cloned().unwrap_or_default();
                if name == "resumptionToken" {
                    token = text.trim().to_string();
                }
                if let Some(r) = cur.as_mut() {
                    let v = text.trim().to_string();
                    match (name.as_str(), parent.as_str()) {
                        ("id", "arXivRaw") => r.id = v,
                        ("title", _) => r.title = v,
                        ("abstract", _) => r.abstract_ = v,
                        ("authors", _) => r.authors = split_authors(&v),
                        ("categories", _) => r.cats = v.split_whitespace().map(str::to_string).collect(),
                        ("comments", _) => r.comments = v,
                        ("journal-ref", _) => r.journal_ref = v,
                        ("doi", _) => r.doi = v,
                        ("license", _) => r.license = v,
                        ("date", "version") => {
                            if let Some(t) = rfc2822(&v) {
                                r.versions.push(t);
                            }
                        }
                        ("arXivRaw", _) => {
                            if let Some(r) = cur.take() {
                                if !deleted && !r.id.is_empty() {
                                    out.push(r);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    (out, token)
}

/// An author line as OAI-PMH gives it (`A. Smith, B. Jones (MIT) and C. Wu`)
/// as names: affiliations in brackets dropped, the last word the last name
/// (with its particles: `van der`, `de la`).
pub fn split_authors(line: &str) -> Vec<(String, String, String)> {
    let mut s = String::with_capacity(line.len());
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let s = s.replace('\n', " ").replace(" and ", ", ").replace(",and ", ", ");
    let mut out = Vec::new();
    for part in s.split(',') {
        let p = part.split_whitespace().collect::<Vec<_>>();
        if p.is_empty() {
            continue;
        }
        let suffix = if p.len() > 2 && matches!(p[p.len() - 1], "Jr" | "Jr." | "Sr" | "Sr." | "II" | "III" | "IV") { p[p.len() - 1].to_string() } else { String::new() };
        let p = if suffix.is_empty() { &p[..] } else { &p[..p.len() - 1] };
        if p.len() == 1 {
            out.push((p[0].to_string(), String::new(), suffix));
            continue;
        }
        // Particles go with the last name.
        let mut cut = p.len() - 1;
        while cut > 1 && matches!(p[cut - 1].to_lowercase().as_str(), "van" | "von" | "der" | "de" | "la" | "le" | "del" | "della" | "di" | "da" | "dos" | "du" | "ter" | "ten" | "den" | "bin" | "al") {
            cut -= 1;
        }
        out.push((p[cut..].join(" "), p[..cut].join(" "), suffix));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oai_records() {
        let xml = r#"<OAI-PMH><ListRecords><record><header><identifier>oai:arXiv.org:2611.00001</identifier></header><metadata><arXivRaw><id>2611.00001</id><submitter>A</submitter><version version="v1"><date>Mon, 2 Nov 2026 19:18:42 GMT</date><size>10kb</size></version><version version="v2"><date>Tue, 10 Nov 2026 10:00:00 GMT</date></version><title>A &amp; B
  title</title><authors>Ana Lima (MIT), Jan van der Berg and Wei Chen</authors><categories>cs.LG stat.ML</categories><abstract>  We study X. </abstract></arXivRaw></metadata></record>
<record><header status="deleted"><identifier>oai:arXiv.org:2611.00002</identifier></header></record>
<resumptionToken cursor="0">abc|1001</resumptionToken></ListRecords></OAI-PMH>"#;
        let (rs, tok) = oai_page(xml);
        assert_eq!(tok, "abc|1001");
        assert_eq!(rs.len(), 1);
        let r = &rs[0];
        assert_eq!(r.id, "2611.00001");
        assert_eq!(r.title, "A & B\n  title");
        assert_eq!(r.cats, vec!["cs.LG", "stat.ML"]);
        assert_eq!(r.versions.len(), 2);
        assert_eq!(r.authors[0], ("Lima".into(), "Ana".into(), String::new()));
        assert_eq!(r.authors[1], ("van der Berg".into(), "Jan".into(), String::new()));
        assert_eq!(r.authors[2].0, "Chen");
    }
}
