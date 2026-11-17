//! `prepare`: what the records do not say, made from the vectors: each
//! picture's kind and theme (`labels.bin`, two bytes a picture in PD12M's
//! order) and the themes themselves (`themes.json`: names, sizes, places
//! on the map, the pictures nearest their middle, their neighbours and
//! their centres). `load` puts them in the rows.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::clip::Clip;
use crate::pd12m::{self, dot, Vectors, DIM, PER_FILE, PER_VECTORS};
use crate::themes::{self, KINDS, NAMES};

pub struct Options {
    /// The folder with PD12M's Parquet and NumPy files.
    pub data: PathBuf,
    /// The pictures to prepare: the first `rows` of PD12M.
    pub rows: usize,
    pub models: PathBuf,
    pub out: PathBuf,
    pub themes: usize,
    /// Pictures k-means learns the themes from.
    pub sample: usize,
    pub iterations: usize,
    /// Names chosen for these themes (`{"rows": n, "themes": k, "names": {"0": "..."}}`),
    /// which replace the names made from the words in `NAMES`.
    pub names: Option<PathBuf>,
    /// An earlier prepare's themes.json whose themes (centres, names,
    /// places on the map) these pictures are placed in, in place of
    /// learning new ones.
    pub themes_from: Option<PathBuf>,
}

/// The themes of an earlier prepare: their centres, names and places.
struct Kept {
    centres: Vec<Vec<f32>>,
    names: Vec<String>,
    at: Vec<(usize, usize)>,
    cols: usize,
}

fn kept_themes(p: &Path) -> Result<Kept, String> {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?;
    let themes = j["themes"].as_array().ok_or_else(|| format!("{}: no themes", p.display()))?;
    let mut k = Kept { centres: Vec::new(), names: Vec::new(), at: Vec::new(), cols: j["cols"].as_u64().unwrap_or(1) as usize };
    for (i, t) in themes.iter().enumerate() {
        if t["id"].as_u64() != Some(i as u64) {
            return Err(format!("{}: theme {i} is out of order", p.display()));
        }
        let mut c: Vec<f32> = t["centre"].as_array().into_iter().flatten().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
        if c.len() != DIM {
            return Err(format!("{}: theme {i}'s centre has {} numbers, not {DIM}", p.display(), c.len()));
        }
        pd12m::unit(&mut c);
        k.centres.push(c);
        k.names.push(t["name"].as_str().unwrap_or("").to_string());
        k.at.push((t["row"].as_u64().unwrap_or(0) as usize, t["col"].as_u64().unwrap_or(0) as usize));
    }
    Ok(k)
}

/// How many pictures there are to read, and the vector files in order.
fn plan(o: &Options) -> Result<(usize, Vec<PathBuf>), String> {
    let files = pd12m::files(&o.data)?;
    let have = files.len() * PER_FILE;
    let rows = o.rows.min(have);
    let mut vfiles = Vec::new();
    for f in 0..rows.div_ceil(PER_VECTORS) {
        let p = pd12m::vector_file(&o.data, f * (PER_VECTORS / PER_FILE));
        if !p.exists() {
            return Err(format!("{} is missing: PD12M's vectors for rows {} to {}", p.display(), f * PER_VECTORS, (f + 1) * PER_VECTORS - 1));
        }
        vfiles.push(p);
    }
    Ok((rows, vfiles))
}

/// Reads the vectors of the first `rows` pictures, a chunk at a time.
fn each_chunk(vfiles: &[PathBuf], rows: usize, chunk: usize, mut f: impl FnMut(usize, &[f32]) -> Result<(), String>) -> Result<(), String> {
    let mut done = 0;
    for p in vfiles {
        let mut v = Vectors::open(p)?;
        while done < rows {
            let mut buf = Vec::with_capacity(chunk * DIM);
            let start = done;
            while buf.len() < chunk * DIM && done < rows {
                match v.next_row()? {
                    Some(r) => buf.extend(r),
                    None => break,
                }
                done += 1;
            }
            if buf.is_empty() {
                break;
            }
            f(start, &buf)?;
            if done % PER_VECTORS == 0 {
                break;
            }
        }
    }
    Ok(())
}

pub fn run(o: &Options) -> Result<(), String> {
    std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
    let (rows, vfiles) = plan(o)?;
    let clip = Clip::open(&o.models, 1)?;
    eprintln!("{rows} pictures; {} vector files; the model: {}", vfiles.len(), clip.name());
    let kind_vecs = clip.texts_now(&KINDS.iter().map(|k| k.1).collect::<Vec<_>>())?;
    let name_vecs = clip.texts_now(NAMES)?;

    // The themes, learnt from every `step`th picture, or kept from an
    // earlier prepare.
    let kept = match &o.themes_from {
        Some(p) => Some(kept_themes(p)?),
        None => None,
    };
    let centres = match &kept {
        Some(k) => {
            eprintln!("the {} themes of {}", k.centres.len(), o.themes_from.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
            k.centres.clone()
        }
        None => {
            let step = (rows / o.sample.max(1)).max(1);
            let mut sample: Vec<f32> = Vec::with_capacity(o.sample * DIM);
            each_chunk(&vfiles, rows, 50_000, |start, buf| {
                for (j, v) in buf.chunks_exact(DIM).enumerate() {
                    if (start + j) % step == 0 {
                        sample.extend_from_slice(v);
                    }
                }
                Ok(())
            })?;
            eprintln!("k-means of {} pictures into {} themes", sample.len() / DIM, o.themes);
            let c = themes::kmeans(&sample, o.themes, o.iterations);
            drop(sample);
            c
        }
    };

    // Every picture's kind and theme, and each theme's pictures nearest
    // its middle.
    const KEEP: usize = 96;
    let k = centres.len();
    let mut labels = vec![0u8; rows * 2];
    let mut counts = vec![0usize; k];
    let mut kinds = vec![vec![0usize; KINDS.len()]; k];
    let mut best: Vec<Vec<(f32, usize, Vec<f32>)>> = vec![Vec::new(); k];
    each_chunk(&vfiles, rows, 100_000, |start, buf| {
        let near = themes::nearest(buf, &centres);
        let kind = themes::nearest(buf, &kind_vecs);
        for (j, ((t, s), (kd, _))) in near.iter().zip(&kind).enumerate() {
            let (t, kd) = (*t as usize, *kd as usize);
            labels[(start + j) * 2] = kd as u8;
            labels[(start + j) * 2 + 1] = t as u8;
            counts[t] += 1;
            kinds[t][kd] += 1;
            let b = &mut best[t];
            if b.len() < KEEP || *s > b[b.len() - 1].0 {
                b.push((*s, start + j, buf[j * DIM..(j + 1) * DIM].to_vec()));
                b.sort_by(|x, y| y.0.total_cmp(&x.0));
                b.truncate(KEEP);
            }
        }
        eprintln!("  {} of {rows}", start + buf.len() / DIM);
        Ok(())
    })?;
    let mut f = std::fs::File::create(o.out.join("labels.bin")).map_err(|e| e.to_string())?;
    f.write_all(&labels).map_err(|e| e.to_string())?;

    let mut names = match &kept {
        Some(k) => k.names.clone(),
        None => themes::name_themes(&centres, &name_vecs, NAMES),
    };
    if let Some(p) = &o.names {
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?;
        if j["rows"].as_u64() != Some(rows as u64) || j["themes"].as_u64() != Some(k as u64) {
            return Err(format!("{} names the themes of {} pictures in {}, not {rows} in {k}", p.display(), j["rows"], j["themes"]));
        }
        for (t, n) in j["names"].as_object().into_iter().flatten() {
            if let (Ok(t), Some(n)) = (t.parse::<usize>(), n.as_str()) {
                if t < k {
                    names[t] = n.to_string();
                }
            }
        }
    }
    let (cols, at) = match &kept {
        Some(kp) => (kp.cols, kp.at.clone()),
        None => {
            let cols = ((k as f64) * 1.8).sqrt().ceil() as usize;
            (cols, themes::layout(&centres, cols.max(1)))
        }
    };
    let mut out = Vec::new();
    for t in 0..k {
        // Nine pictures near the middle that are not near-copies of each other.
        let mut reps: Vec<(usize, &Vec<f32>)> = Vec::new();
        for (_, n, v) in &best[t] {
            if reps.iter().all(|(_, w)| dot(v, w) < 0.93) {
                reps.push((*n, v));
            }
            if reps.len() == 24 {
                break;
            }
        }
        let mut near: Vec<(f32, usize)> = (0..k).filter(|&u| u != t).map(|u| (dot(&centres[t], &centres[u]), u)).collect();
        near.sort_by(|a, b| b.0.total_cmp(&a.0));
        let kinds_of: serde_json::Map<String, serde_json::Value> = KINDS.iter().enumerate().filter(|(i, _)| kinds[t][*i] > 0).map(|(i, (name, _))| (name.to_string(), json!(kinds[t][i]))).collect();
        let main_kind = (0..KINDS.len()).max_by_key(|&i| kinds[t][i]).map(|i| KINDS[i].0).unwrap_or("");
        out.push(json!({
            "id": t, "name": names[t], "n": counts[t], "row": at[t].0, "col": at[t].1,
            "group": themes::kind_group(main_kind),
            "reps": reps.iter().map(|r| r.0).collect::<Vec<_>>(),
            "kinds": kinds_of,
            "near": near.iter().take(4).map(|x| x.1).collect::<Vec<_>>(),
            "centre": centres[t].iter().map(|x| (x * 1e5).round() / 1e5).collect::<Vec<_>>(),
        }));
    }
    let kind_counts: Vec<usize> = (0..KINDS.len()).map(|i| kinds.iter().map(|t| t[i]).sum()).collect();
    let doc = json!({"rows": rows, "cols": cols, "themes": out, "kinds": KINDS.iter().zip(&kind_counts).map(|((n, w), c)| json!({"name": n, "words": w, "n": c})).collect::<Vec<_>>()});
    std::fs::write(o.out.join("themes.json"), serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for t in &out {
        eprintln!("  theme {:>2} {:<32} {:>7} pictures, {}", t["id"], t["name"].as_str().unwrap_or(""), t["n"], t["group"]);
    }
    eprintln!("wrote {} and {}", o.out.join("labels.bin").display(), o.out.join("themes.json").display());
    Ok(())
}

/// The kind and theme of picture `n`, from `labels.bin`.
pub fn label(labels: &[u8], n: usize) -> Option<(&'static str, usize)> {
    let k = *labels.get(n * 2)? as usize;
    let t = *labels.get(n * 2 + 1)? as usize;
    Some((KINDS.get(k)?.0, t))
}

pub fn read_labels(dir: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(dir.join("labels.bin")).map_err(|e| format!("{}: {e} (run prepare first)", dir.join("labels.bin").display()))
}
