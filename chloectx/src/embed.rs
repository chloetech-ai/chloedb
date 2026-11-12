//! Text embedders: ChloeDB's ([`chloedb::embed`]), opened from ChloeCtx's
//! model directory. Every vector is unit length, so ChloeDB's dot product is
//! cosine similarity.
//!
//! * [`StaticEmbedder`]: a Model2Vec model (token lookup and mean). Fast
//!   enough to embed every hook payload inline.
//! * `BertEmbedder` (feature `bert`): BGE-small through candle.
//! * [`HashEmbedder`]: feature hashing of words, for tests without model files.

use std::path::{Path, PathBuf};

#[cfg(feature = "bert")]
pub use chloedb::embed::BertEmbedder;
pub use chloedb::embed::{fnv, Embedder, HashEmbedder, StaticEmbedder};

use crate::{err, Result};

/// Where models live: `$CHLOECTX_MODELS`, else `models` in [`crate::paths::home`].
pub fn models_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CHLOECTX_MODELS") {
        return PathBuf::from(d);
    }
    crate::paths::home().join("models")
}

/// The models `chloectx fetch-model` downloads: its argument, the Hugging
/// Face repository, the folder under [`models_dir`], and the files.
pub const FETCHED: &[(&str, &str, &str, &[&str])] = &[
    ("static", "minishlab/potion-retrieval-32M", "potion-retrieval-32M", &["config.json", "tokenizer.json", "model.safetensors"]),
    ("bert", "BAAI/bge-small-en-v1.5", "bge-small-en-v1.5", &["config.json", "tokenizer.json", "model.safetensors"]),
];

/// What [`fetch_model`] did with each file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// It was there already, with the pinned bytes.
    Kept(String),
    Downloaded(String),
}

/// Downloads `files` of Hugging Face repository `repo` into `dir`, from
/// `base` (`https://huggingface.co`), at the commit ChloeDB pins for it
/// ([`chloedb::model_pins`]), each checked against its pinned SHA-256: a
/// model changed upstream under the same name, or a download cut short,
/// is refused instead of embedding differently from the store's vectors.
/// A file is fetched to a name of its own and renamed into place once it
/// checks, so a partial one never stands where the model is read; one
/// already in place with the pinned bytes is kept. A repository with no
/// pin comes from `main`, unchecked.
pub fn fetch_model(base: &str, repo: &str, dir: &Path, files: &[&str]) -> Result<Vec<Fetched>> {
    fetch_pinned(base, repo, chloedb::model_pins::pin(repo), dir, files)
}

/// [`fetch_model`] with the pin given.
pub fn fetch_pinned(base: &str, repo: &str, pin: Option<&chloedb::model_pins::Pin>, dir: &Path, files: &[&str]) -> Result<Vec<Fetched>> {
    std::fs::create_dir_all(dir)?;
    let revision = pin.map_or("main", |p| p.revision);
    let mut out = Vec::new();
    for f in files {
        let dest = dir.join(f);
        let want = match pin {
            Some(p) => Some(p.sha256(f).ok_or_else(|| err(format!("no SHA-256 is pinned for {f} of {repo}")))?),
            None => None,
        };
        if let Some(w) = want {
            if dest.is_file() && file_sha256(&dest)? == w {
                out.push(Fetched::Kept(f.to_string()));
                continue;
            }
        }
        let url = format!("{}/{repo}/resolve/{revision}/{f}", base.trim_end_matches('/'));
        let part = dir.join(format!("{f}.part"));
        let _ = std::fs::remove_file(&part);
        let ok = std::process::Command::new("curl").args(["-fsSL", "--retry", "2", "-o"]).arg(&part).arg(&url).status().map_or(false, |s| s.success());
        if !ok {
            let _ = std::fs::remove_file(&part);
            return Err(err(format!("failed to download {url}")));
        }
        if let Some(w) = want {
            let got = file_sha256(&part)?;
            if got != w {
                let _ = std::fs::remove_file(&part);
                return Err(err(format!("{url}: SHA-256 {got}, where {w} is pinned for {repo} at {revision}: not the file ChloeCtx was built for, so it was not kept")));
            }
        }
        std::fs::rename(&part, &dest)?;
        out.push(Fetched::Downloaded(f.to_string()));
    }
    Ok(out)
}

/// The SHA-256 of a file, hex, read a block at a time.
pub fn file_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Opens an embedder from a spec: `static[:<dir>]`, `bert[:<dir>]` or `hash[:<dim>]`.
pub fn open(spec: &str) -> Result<Box<dyn Embedder>> {
    let (kind, arg) = spec.split_once(':').unwrap_or((spec, ""));
    match kind {
        "static" => {
            let dir = if arg.is_empty() { models_dir().join("potion-retrieval-32M") } else { PathBuf::from(arg) };
            Ok(Box::new(StaticEmbedder::open(&dir).map_err(|e| err(format!("{e} (run `chloectx fetch-model`)")))?))
        }
        "hash" => Ok(Box::new(HashEmbedder { dim: arg.parse().unwrap_or(256) })),
        #[cfg(feature = "bert")]
        "bert" => {
            let dir = if arg.is_empty() { models_dir().join("bge-small-en-v1.5") } else { PathBuf::from(arg) };
            Ok(Box::new(BertEmbedder::open(&dir).map_err(|e| err(format!("{e} (run `chloectx fetch-model bert`)")))?))
        }
        _ => Err(err(format!("unknown embedder {spec} (static, hash{})", if cfg!(feature = "bert") { ", bert" } else { "" }))),
    }
}
