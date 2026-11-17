//! PD12M as Spawning publishes it (huggingface.co/datasets/Spawning/PD12M):
//! 124 Parquet files of records (`pd12m.000.parquet` ...), 100,000 pictures
//! each, and the pictures' CLIP ViT-L/14 vectors in NumPy files of a
//! million rows each (`pd12m.00.npy` holds the rows of files 000 to 009), in
//! the same order. The pictures themselves are in a public bucket, at each
//! record's `url`.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

/// Rows a Parquet file holds, and a vector file.
pub const PER_FILE: usize = 100_000;
pub const PER_VECTORS: usize = 1_000_000;
pub const DIM: usize = 768;

/// One picture's record.
#[derive(Debug, Clone, Default)]
pub struct Record {
    pub id: String,
    pub url: String,
    pub caption: String,
    pub width: i64,
    pub height: i64,
    pub mime: String,
    pub hash: String,
    pub license: String,
    pub source: String,
}

impl Record {
    /// `CC0` (zero/1.0), `PD` (the Public Domain Mark), or empty when the
    /// record does not say (a few in a thousand).
    pub fn licence(&self) -> &'static str {
        licence_of(&self.license)
    }
}

pub fn licence_of(url: &str) -> &'static str {
    if url.contains("/zero/") {
        "CC0"
    } else if url.contains("/mark/") {
        "PD"
    } else {
        ""
    }
}

/// The Parquet files of a folder, in order.
pub fn files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "parquet") && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("pd12m.")))
        .collect();
    out.sort();
    Ok(out)
}

/// The vector file of Parquet file number `n` (`pd12m.00.npy` for 0 to 9).
pub fn vector_file(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("pd12m.{:02}.npy", n / (PER_VECTORS / PER_FILE)))
}

fn strs<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a StringArray, String> {
    let i = b.schema().index_of(name).map_err(|_| format!("no column {name}"))?;
    b.column(i).as_any().downcast_ref::<StringArray>().ok_or_else(|| format!("{name} is not text"))
}

fn ints<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a Int64Array, String> {
    let i = b.schema().index_of(name).map_err(|_| format!("no column {name}"))?;
    b.column(i).as_any().downcast_ref::<Int64Array>().ok_or_else(|| format!("{name} is not a whole number"))
}

/// The records of one Parquet file, in order.
pub fn records(path: &Path) -> Result<Vec<Record>, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r = ParquetRecordBatchReaderBuilder::try_new(f).map_err(|e| format!("{}: {e}", path.display()))?.with_batch_size(16_384).build().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::with_capacity(PER_FILE);
    for b in r {
        let b = b.map_err(|e| format!("{}: {e}", path.display()))?;
        let (id, url, caption, mime, hash, license, source) = (strs(&b, "id")?, strs(&b, "url")?, strs(&b, "caption")?, strs(&b, "mime_type")?, strs(&b, "hash")?, strs(&b, "license")?, strs(&b, "source")?);
        let (w, h) = (ints(&b, "width")?, ints(&b, "height")?);
        let s = |a: &StringArray, i: usize| if a.is_null(i) { String::new() } else { a.value(i).to_string() };
        for i in 0..b.num_rows() {
            out.push(Record {
                id: s(id, i),
                url: s(url, i),
                caption: s(caption, i),
                width: if w.is_null(i) { 0 } else { w.value(i) },
                height: if h.is_null(i) { 0 } else { h.value(i) },
                mime: s(mime, i),
                hash: s(hash, i),
                license: s(license, i),
                source: s(source, i),
            });
        }
    }
    Ok(out)
}

/// A NumPy file of float32 rows of [`DIM`] numbers, read in order.
pub struct Vectors {
    r: BufReader<File>,
    pub rows: usize,
    read: usize,
}

impl Vectors {
    pub fn open(path: &Path) -> Result<Vectors, String> {
        let e = |x: String| format!("{}: {x}", path.display());
        let mut f = File::open(path).map_err(|x| e(x.to_string()))?;
        let mut magic = [0u8; 10];
        f.read_exact(&mut magic).map_err(|x| e(x.to_string()))?;
        if &magic[..6] != b"\x93NUMPY" {
            return Err(e("not a NumPy file".into()));
        }
        let len = match magic[6] {
            1 => u16::from_le_bytes([magic[8], magic[9]]) as usize,
            _ => {
                let mut more = [0u8; 2];
                f.read_exact(&mut more).map_err(|x| e(x.to_string()))?;
                u32::from_le_bytes([magic[8], magic[9], more[0], more[1]]) as usize
            }
        };
        let mut head = vec![0u8; len];
        f.read_exact(&mut head).map_err(|x| e(x.to_string()))?;
        let head = String::from_utf8_lossy(&head).to_string();
        if !head.contains("'<f4'") || head.contains("'fortran_order': True") {
            return Err(e(format!("not little-endian float32 rows in C order: {head}")));
        }
        let shape = head.split("'shape':").nth(1).and_then(|s| s.split(')').next()).unwrap_or("").trim().trim_start_matches('(');
        let dims: Vec<usize> = shape.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if dims.len() != 2 || dims[1] != DIM {
            return Err(e(format!("shape {dims:?}, not (n, {DIM})")));
        }
        Ok(Vectors { r: BufReader::with_capacity(1 << 22, f), rows: dims[0], read: 0 })
    }

    /// The next row, unit length.
    pub fn next_row(&mut self) -> Result<Option<Vec<f32>>, String> {
        if self.read == self.rows {
            return Ok(None);
        }
        let mut b = vec![0u8; DIM * 4];
        self.r.read_exact(&mut b).map_err(|e| e.to_string())?;
        self.read += 1;
        let mut v: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        unit(&mut v);
        Ok(Some(v))
    }

    /// Skips `n` rows.
    pub fn skip(&mut self, n: usize) -> Result<(), String> {
        let n = n.min(self.rows - self.read);
        std::io::copy(&mut (&mut self.r).take((n * DIM * 4) as u64), &mut std::io::sink()).map_err(|e| e.to_string())?;
        self.read += n;
        Ok(())
    }
}

pub fn unit(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// A short title made from a caption, which the records have instead of
/// one: its first sentence without "The image shows", cut at a clause.
pub fn title_of(caption: &str) -> String {
    let first = caption.split(". ").next().unwrap_or(caption).trim().trim_end_matches('.');
    let lower = first.to_ascii_lowercase();
    let mut t = first;
    for p in ["the image shows ", "this image shows ", "the image is ", "this is ", "the picture shows ", "this picture shows ", "the photo shows ", "this photo shows ", "in this image we can see ", "in this picture we can see ", "the image depicts ", "this image depicts ", "an image of ", "a picture of "] {
        if lower.starts_with(p) {
            t = &first[p.len()..];
            break;
        }
    }
    // A clause after a comma, ", with ..." or ", which ..." is detail.
    let mut cut = t.len();
    for m in [", with ", ", which ", ", and ", ", while ", ", where ", "; "] {
        if let Some(i) = t.find(m) {
            cut = cut.min(i);
        }
    }
    let mut t = t[..cut].trim();
    // A long one keeps its first clause: "a portrait of a man in a blue
    // coat, framed against a white background".
    if t.chars().count() > 60 {
        if let Some(i) = t.match_indices(", ").map(|(i, _)| i).find(|&i| i >= 24) {
            t = &t[..i];
        }
    }
    let t: String = match t.char_indices().nth(90) {
        Some((i, _)) => {
            let head = &t[..i];
            match head.rfind(' ') {
                Some(j) if j > 40 => format!("{}…", head[..j].trim_end_matches([',', ';', ':'])),
                _ => format!("{head}…"),
            }
        }
        None => t.to_string(),
    };
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A picture's shape by its width over its height.
pub fn shape_of(w: i64, h: i64) -> &'static str {
    if w <= 0 || h <= 0 {
        return "";
    }
    let r = w as f64 / h as f64;
    if r < 0.8 {
        "tall"
    } else if r <= 1.25 {
        "square"
    } else if r <= 2.2 {
        "wide"
    } else {
        "panorama"
    }
}

/// A picture's size by its megapixels.
pub fn size_of(mp: f64) -> &'static str {
    if mp < 1.0 {
        "small"
    } else if mp < 4.0 {
        "medium"
    } else if mp < 16.0 {
        "large"
    } else {
        "huge"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_the_first_clause_of_the_caption() {
        assert_eq!(title_of("The image shows a miniature portrait of a man in a blue coat, framed in a photo frame against a white background."), "A miniature portrait of a man in a blue coat");
        assert_eq!(title_of("This is a painting of a ship in rough seas. The sky is dark."), "A painting of a ship in rough seas");
        assert_eq!(title_of("A red fox in the snow"), "A red fox in the snow");
        let long = title_of("The image shows an old photo of a group of people standing in front of a very large building in a city street on a sunny afternoon in the summer of a year");
        assert!(long.ends_with('…') && long.chars().count() <= 91, "{long}");
    }

    #[test]
    fn licences_shapes_and_sizes() {
        assert_eq!(licence_of("https://creativecommons.org/publicdomain/zero/1.0/"), "CC0");
        assert_eq!(licence_of("https://creativecommons.org/publicdomain/mark/1.0/"), "PD");
        assert_eq!(licence_of(""), "");
        assert_eq!(shape_of(2718, 3088), "square");
        assert_eq!(shape_of(1000, 2000), "tall");
        assert_eq!(shape_of(5000, 1000), "panorama");
        assert_eq!(size_of(0.5), "small");
        assert_eq!(size_of(42.8), "huge");
    }
}
