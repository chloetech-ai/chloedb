//! Downloads the embedding models ChloeDB knows by name
//! ([`crate::embed::KNOWN_MODELS`]) from Hugging Face, at the commit each
//! is pinned at and checked against the SHA-256 of each file
//! ([`crate::model_pins`]), as `chloedb-server fetch-model` does.
//!
//! The Python, JavaScript and Java packages turn [`set_auto_fetch`] on, so
//! that a schema naming `potion-retrieval-32M` works on a new machine as
//! written: the first open (or `CREATE TABLE`) that needs the model
//! downloads it once into the models directory, saying so on stderr. A
//! server fetches the models of its tables itself, in the background, and
//! leaves this off. `CHLOEDB_FETCH_MODELS=0` (or `off`) turns it off where
//! it is on, for a machine that must not reach the network; `HF_ENDPOINT`
//! names a mirror of Hugging Face, as it does for Hugging Face's own tools.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

static AUTO: AtomicBool = AtomicBool::new(false);

/// Turns downloading a missing model on first use on or off for this
/// process ([`ensure`]).
pub fn set_auto_fetch(on: bool) {
    AUTO.store(on, Ordering::Relaxed);
}

/// Whether [`ensure`] downloads: [`set_auto_fetch`] turned it on, and
/// `CHLOEDB_FETCH_MODELS` does not turn it off.
pub fn auto_fetch() -> bool {
    AUTO.load(Ordering::Relaxed) && !crate::env::var("CHLOEDB_FETCH_MODELS").ok().is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"))
}

/// Downloads model `model` into `models` when downloads on first use are
/// on ([`auto_fetch`]), it is a model ChloeDB knows that this build runs,
/// and its files are not all there yet; does nothing otherwise. A download
/// that fails leaves the model missing, and the error says what failed
/// and the command that fetches it by hand.
pub fn ensure(model: &str, models: &Path) -> Result<()> {
    if !auto_fetch() {
        return Ok(());
    }
    let Some(name) = crate::embed::to_fetch(model, models) else { return Ok(()) };
    // One download at a time in a process: a second open of the same
    // model waits for the first and then finds it there.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if crate::embed::to_fetch(name, models).is_none() {
        return Ok(());
    }
    let size = approximate_mb(name);
    eprintln!("chloedb: downloading the embedding model {name} ({size} MB, once) into {}", models.display());
    let t0 = std::time::Instant::now();
    match fetch(name, models) {
        Ok(_) => {
            eprintln!("chloedb: {name} is ready ({:.1} s; CHLOEDB_FETCH_MODELS=0 turns these downloads off)", t0.elapsed().as_secs_f64());
            Ok(())
        }
        Err(e) => Err(Error::Schema(format!(
            "model {name} is not under {} and could not be downloaded: {e}. Download it on a machine that can (`chloedb-server fetch-model {name}`, or `python -m chloedb fetch-model {name}`) and copy the directory {name} there, or set HF_ENDPOINT to a mirror of Hugging Face",
            models.display()
        ))),
    }
}

/// About how many megabytes [`fetch`] downloads for `name`.
fn approximate_mb(name: &str) -> u64 {
    match name {
        n if n.starts_with("potion-retrieval-32M") => 130,
        n if n.starts_with("potion-base-8M") => 30,
        n if n.starts_with("bge-small") => 130,
        n if n.starts_with("snowflake") => 440,
        _ => 500,
    }
}

/// Downloads model `name` of [`crate::embed::KNOWN_MODELS`] into
/// `models/<name>` and opens it to check it; returns its directory. Each
/// file comes from the commit the model is pinned at and is refused when
/// its SHA-256 is not the pinned one. A file already there that is the
/// pinned one is kept. A model of a table in fewer bits
/// (`potion-retrieval-32M-int8`) is its original fetched aside, then
/// written into its own directory.
#[cfg(all(feature = "hosted", feature = "embed"))]
pub fn fetch(name: &str, models: &Path) -> Result<PathBuf> {
    use std::io::Read;
    let &(local, repo, _) = crate::embed::KNOWN_MODELS.iter().find(|m| m.0.eq_ignore_ascii_case(name)).ok_or_else(|| Error::Schema(format!("unknown model {name}")))?;
    let pin = crate::model_pins::pin(repo).ok_or_else(|| Error::Schema(format!("model {name} has no pinned revision")))?;
    let quantized = crate::embed::quantized_of(local);
    let model_dir = models.join(local);
    let out = match quantized {
        Some(_) => models.join(format!(".{local}.fetch")),
        None => model_dir.clone(),
    };
    std::fs::create_dir_all(&out).map_err(|e| Error::Schema(format!("{}: {e}", out.display())))?;
    let base = crate::env::var("HF_ENDPOINT").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "https://huggingface.co".into());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .timeout_recv_body(None)
        .user_agent(format!("chloedb/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    for file in crate::embed::model_files(quantized.map_or(local, |q| q.0)) {
        let want = pin.sha256(file).ok_or_else(|| Error::Schema(format!("no checksum is pinned for {file} of {repo}")))?;
        if std::fs::read(out.join(file)).is_ok_and(|have| crate::model_pins::sha256_hex(&have) == want) {
            continue;
        }
        let url = format!("{}/{repo}/resolve/{}/{file}", base.trim_end_matches('/'), pin.revision);
        let resp = agent.get(&url).call().map_err(|e| Error::Schema(format!("{url}: {e}")))?;
        let mut bytes = Vec::new();
        resp.into_body().into_with_config().limit(4 << 30).reader().read_to_end(&mut bytes).map_err(|e| Error::Schema(format!("{url}: {e}")))?;
        let got = crate::model_pins::sha256_hex(&bytes);
        if got != want {
            return Err(Error::Schema(format!("{url}: SHA-256 {got}, where {want} is pinned for {repo} at {}: the file is not the one ChloeDB was built for, and is not kept", pin.revision)));
        }
        // Written aside and renamed, so that a model is never half there.
        let tmp = out.join(format!("{file}.part"));
        std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, out.join(file))).map_err(|e| Error::Schema(format!("{}: {e}", out.join(file).display())))?;
    }
    if let Some((_, elem)) = quantized {
        crate::embed::quantize_static(&out, &model_dir, elem)?;
        let _ = std::fs::remove_dir_all(&out);
    }
    crate::embed::open(local, models)?;
    Ok(model_dir)
}

#[cfg(not(all(feature = "hosted", feature = "embed")))]
pub fn fetch(name: &str, _models: &Path) -> Result<PathBuf> {
    Err(Error::Schema(format!("this build downloads no models (features hosted and embed): fetch {name} with chloedb-server fetch-model")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_unless_turned_on_and_not_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        // Off by default: nothing is fetched, nothing fails.
        assert!(!auto_fetch());
        ensure("potion-base-8M", dir.path()).unwrap();
        assert!(!dir.path().join("potion-base-8M").exists());
        // A model it does not know, or one there already, is left alone
        // even when on.
        set_auto_fetch(true);
        ensure("hash:16", dir.path()).unwrap();
        ensure("openai/text-embedding-3-small", dir.path()).unwrap();
        set_auto_fetch(false);
    }

    #[test]
    fn sizes_are_said_before_a_download() {
        assert_eq!(approximate_mb("potion-retrieval-32M"), 130);
        assert_eq!(approximate_mb("potion-base-8M-int8"), 30);
    }
}
