//! Embedders: the models that turn a row's text or image, or a query,
//! into the vector of a VECTOR field declared with
//! [`crate::schema::Embed`]. Every vector is unit length, so Strata's dot
//! product is cosine similarity. Every model embeds text
//! ([`Embedder::embed`]); a model of images and text, such as SigLIP,
//! also embeds decoded images ([`Embedder::embed_inputs`]) into the same
//! space, so a text query finds images and an image finds images.
//!
//! * [`StaticEmbedder`] (feature `embed`, on by default): a Model2Vec model
//!   (token lookup and mean), such as `potion-retrieval-32M`. Tens of
//!   microseconds a row on one core.
//! * [`BertEmbedder`] (feature `bert`): a BERT encoder with CLS pooling
//!   through candle, such as `bge-small-en-v1.5` or
//!   `snowflake-arctic-embed-m-v1.5`. Better vectors, milliseconds a row.
//! * [`ColbertEmbedder`] (feature `bert`): ColBERT, a vector a token of
//!   the text, such as `answerai-colbert-small-v1`, for a multi-vector
//!   field (`VECTOR(d)[]`) ranked by `maxsim` ([`Embedder::embed_tokens`]).
//! * [`SiglipEmbedder`] (feature `vision`): SigLIP's image and text
//!   towers through candle, `siglip-base-patch16-224`. A tenth of a second
//!   an image on a few cores.
//! * [`SparseEncoder`] (feature `onnx`): term weights for SPARSE fields,
//!   a transformer's for documents and an IDF lookup for queries.
//! * [`OnnxEmbedder`] (feature `onnx`): any encoder exported to ONNX
//!   (`model.onnx`), run by ONNX Runtime, such as a ModernBERT embedder
//!   (`granite-embedding-small-english-r2`) quantised to int8: CLS or mean
//!   pooling as its sentence-transformers configuration says.
//! * [`HashEmbedder`]: feature hashing of words (`hash:<dim>`), and of an
//!   image's colours, for tests and demos without model files; it matches
//!   words and colours, not meanings. For a multi-vector field each word is
//!   a vector of its own.
//!
//! [`open`] finds a model by the name a schema gives it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};

pub trait Embedder: Send + Sync {
    fn name(&self) -> String;
    fn dim(&self) -> usize;
    /// Embeddings of documents, or why the model made none (a model behind
    /// an API that refused, or did not answer).
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    /// Embedding of a search query (some models prefix an instruction).
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed(&[text])?.pop().unwrap_or_default())
    }
    /// What it embeds: text, and images for a model of both.
    fn modalities(&self) -> &'static [Modality] {
        &[Modality::Text]
    }
    /// Embeddings of inputs of the kinds it takes ([`Embedder::modalities`]),
    /// in the space of its text vectors: text as documents
    /// ([`Embedder::embed`]), images with the model's image tower.
    fn embed_inputs(&self, inputs: &[Input<'_>]) -> Result<Vec<Vec<f32>>> {
        let texts: Vec<&str> = inputs.iter().filter_map(|i| if let Input::Text(t) = i { Some(*t) } else { None }).collect();
        match inputs.iter().find(|i| !matches!(i, Input::Text(_))) {
            Some(i) => Err(model_err(format!("model {} does not embed {}", self.name(), modality_name(i.modality())))),
            None => self.embed(&texts),
        }
    }
    /// Whether many texts are best split across the cores, as a model
    /// computed here is; a model behind an API sends its own requests at
    /// once instead.
    fn parallel(&self) -> bool {
        true
    }
    /// Whether it embeds a text into one vector ([`Embedder::embed`]): all
    /// do but a model of tokens only, such as ColBERT.
    fn single(&self) -> bool {
        true
    }
    /// Whether it embeds a text into a vector a token, for a multi-vector
    /// field (`VECTOR(d)[]`) that `maxsim` ranks: late interaction, as
    /// ColBERT scores it ([`Embedder::embed_tokens`]).
    fn tokens(&self) -> bool {
        false
    }
    /// The vectors of documents' tokens, a list a text, for a model whose
    /// [`Embedder::tokens`] is true.
    fn embed_tokens(&self, texts: &[&str]) -> Result<Vec<Vec<Vec<f32>>>> {
        let _ = texts;
        Err(model_err(format!("model {} embeds a text into one vector, not one a token: a multi-vector field (VECTOR(d)[]) needs a model of late interaction, such as ColBERT", self.name())))
    }
    /// The vectors of a search query's tokens ([`Embedder::embed_tokens`]).
    fn embed_query_tokens(&self, text: &str) -> Result<Vec<Vec<f32>>> {
        Ok(self.embed_tokens(&[text])?.pop().unwrap_or_default())
    }
    /// What a model behind an API has asked of it since it opened.
    fn usage(&self) -> Option<Usage> {
        None
    }
}

/// The model of an embedded field that is not open here, as a write sees
/// it: every call fails with what to do ([`crate::db::Db::set_embed_later`]).
/// A row written without its vector would be one `similarity()` never
/// finds.
pub struct NotOpen {
    pub table: String,
    pub field: String,
    pub model: String,
    pub dim: usize,
}

impl NotOpen {
    fn refuse(&self) -> Error {
        Error::Schema(format!(
            "{}.{} is embedded by model {}, which is not open: the row would be written without its vector, which similarity() never finds. Open the model (Db::open_embedders with its models directory; chloedb.open(..., models=DIR) in Python; chloedb-server fetches it), give the vector in the write, or, to write rows without vectors on purpose and fill them in later with embed_backfill, call Db::set_embed_later(true) (db.set_embed_later(True) in Python)",
            self.table, self.field, self.model
        ))
    }
}

impl Embedder for NotOpen {
    fn name(&self) -> String {
        self.model.clone()
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        Err(self.refuse())
    }
    fn embed_inputs(&self, inputs: &[Input<'_>]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        Err(self.refuse())
    }
    fn embed_tokens(&self, texts: &[&str]) -> Result<Vec<Vec<Vec<f32>>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        Err(self.refuse())
    }
    fn parallel(&self) -> bool {
        false
    }
    fn tokens(&self) -> bool {
        true
    }
}

/// The requests a model behind an API sent ([`Embedder::usage`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub requests: u64,
    /// Requests sent again after a rate limit or a failure.
    pub retries: u64,
    /// Requests that failed for good, failing their write or query.
    pub failures: u64,
    /// Texts embedded.
    pub texts: u64,
    /// Tokens the API counted, where its answers say.
    pub tokens: u64,
}

/// A kind of input a model embeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modality {
    Text,
    Image,
    Audio,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Text => "text",
            Modality::Image => "image",
            Modality::Audio => "audio",
        }
    }
}

/// An image decoded to 8-bit RGB, row by row
/// ([`crate::media::decode_image`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Sound decoded to one channel of samples, `rate` a second
/// ([`crate::media::decode_audio`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// What [`Embedder::embed_inputs`] embeds.
#[derive(Clone, Copy, Debug)]
pub enum Input<'a> {
    Text(&'a str),
    Image(&'a Image),
    Audio(&'a Audio),
}

impl Input<'_> {
    pub fn modality(&self) -> Modality {
        match self {
            Input::Text(_) => Modality::Text,
            Input::Image(_) => Modality::Image,
            Input::Audio(_) => Modality::Audio,
        }
    }
}

/// What a media file is to a model: sound if its bytes are of an audio
/// file, else an image.
pub fn file_modality(data: &[u8]) -> Modality {
    match crate::media::describe(data).mime.is_some_and(|m| m.starts_with("audio/")) {
        true => Modality::Audio,
        false => Modality::Image,
    }
}

/// The vector of an image file (JPEG, PNG, WebP, GIF, BMP) or a sound
/// file (WAV, FLAC, MP3, Ogg Vorbis, AAC) by `e`, a model of images or of
/// sound ([`file_modality`]): a query by a file, as `{"embed": {"image":
/// ...}}` or `{"embed": {"audio": ...}}` gives it.
pub fn embed_file(e: &dyn Embedder, data: &[u8]) -> Result<Vec<f32>> {
    let m = file_modality(data);
    if !e.modalities().contains(&m) {
        return Err(model_err(format!("model {} does not embed {}", e.name(), modality_name(m))));
    }
    let v = match m {
        Modality::Audio => e.embed_inputs(&[Input::Audio(&crate::media::decode_audio(data)?)])?,
        _ => e.embed_inputs(&[Input::Image(&crate::media::decode_image(data)?)])?,
    };
    Ok(v.into_iter().next().unwrap_or_default())
}

/// A query's file, the `embed` of a parameter `{"embed": {"image":
/// "<base64>"}}` or `{"embed": {"audio": "<base64>"}}`: its modality and
/// bytes, which have to be of that kind ([`file_is`]).
pub fn query_file(kind: &str, base64: &str) -> Result<(Modality, Vec<u8>)> {
    let want = match kind {
        "image" => Modality::Image,
        "audio" => Modality::Audio,
        k => return Err(Error::Query(format!("\"embed\" is a text, {{\"image\": \"<base64>\"}} or {{\"audio\": \"<base64>\"}}, not {{{k:?}: ...}}"))),
    };
    let data = crate::media::base64_bytes(base64).map_err(|m| Error::Query(format!("{{\"embed\": {{\"{kind}\": ...}}}} is a file in base64: {m}")))?;
    file_is(want, &data)?;
    Ok((want, data))
}

/// Refuses a query's file given as an image that is a sound file, or given
/// as a sound that is not one.
pub fn file_is(want: Modality, data: &[u8]) -> Result<()> {
    match (want, file_modality(data)) {
        (Modality::Audio, Modality::Audio) | (Modality::Image, Modality::Image) => Ok(()),
        (Modality::Audio, _) => Err(Error::Query("the file given as audio is not a sound file (WAV, FLAC, MP3, Ogg Vorbis, AAC)".into())),
        _ => Err(Error::Query("the file given as an image is a sound file".into())),
    }
}

/// What a model takes, as a query's file names it: "images", "sound".
pub fn modality_name(m: Modality) -> &'static str {
    match m {
        Modality::Text => "text",
        Modality::Image => "images",
        Modality::Audio => "sound",
    }
}

pub(crate) fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

fn model_err(what: impl std::fmt::Display) -> Error {
    Error::Schema(what.to_string())
}

/// Where [`open`] looks for models by default: `$CHLOEDB_MODELS` (or
/// `$CHLOEDB_MODELS`), else
/// `~/.local/share/strata/models`.
pub fn models_dir() -> PathBuf {
    if let Some(d) = crate::env::var_os("CHLOEDB_MODELS") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/share/strata/models")
}

/// Models [`open`] knows how to fetch: name, Hugging Face repository, and
/// what it is.
pub const KNOWN_MODELS: &[(&str, &str, &str)] = &[
    ("potion-retrieval-32M", "minishlab/potion-retrieval-32M", "Model2Vec, 512 dimensions, fast (the model ChloeCtx uses)"),
    ("potion-base-8M", "minishlab/potion-base-8M", "Model2Vec, 256 dimensions, smallest"),
    ("bge-small-en-v1.5", "BAAI/bge-small-en-v1.5", "BERT, 384 dimensions (feature bert)"),
    ("snowflake-arctic-embed-m-v1.5", "Snowflake/snowflake-arctic-embed-m-v1.5", "BERT, 768 dimensions (feature bert)"),
    ("siglip-base-patch16-224", "google/siglip-base-patch16-224", "SigLIP, images and text, 768 dimensions (feature vision)"),
    ("clap-htsat-unfused", "laion/clap-htsat-unfused", "CLAP, sound and text, 512 dimensions (feature audio)"),
    ("answerai-colbert-small-v1", "answerdotai/answerai-colbert-small-v1", "ColBERT, a 96-dimension vector a token, for a multi-vector field (feature bert)"),
];

/// The dimensions of a model [`open`] knows by name (`hash:N` and hosted
/// models included, [`crate::hosted::KNOWN`]), without opening it.
pub fn known_dim(model: &str) -> Option<usize> {
    if let Some(n) = model.strip_prefix("hash:") {
        return n.parse().ok();
    }
    if let Some(k) = crate::hosted::known_model(model) {
        return Some(k.dim);
    }
    let dims = [("potion-retrieval-32M", 512), ("potion-base-8M", 256), ("bge-small-en-v1.5", 384), ("snowflake-arctic-embed-m-v1.5", 768), ("siglip-base-patch16-224", 768), ("clap-htsat-unfused", 512), ("answerai-colbert-small-v1", 96)];
    dims.iter().find(|(m, _)| m.eq_ignore_ascii_case(model)).map(|&(_, d)| d)
}

/// A model [`catalog`] lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub dim: Option<usize>,
    /// Behind an API ([`crate::hosted`]), rather than computed here.
    pub hosted: bool,
    pub what: String,
    /// Whether a field naming it would get its model here, or why not (the
    /// files not fetched, the key not set, the feature not built).
    pub ready: std::result::Result<(), String>,
}

/// The feature this build lacks to run model `name` of [`KNOWN_MODELS`].
fn missing_feature(name: &str) -> Option<&'static str> {
    match name {
        n if n.starts_with("siglip") => (!cfg!(feature = "vision")).then_some("vision"),
        n if n.starts_with("clap") => (!cfg!(feature = "audio")).then_some("audio"),
        n if n.starts_with("bge") || n.starts_with("snowflake") || n.starts_with("answerai-colbert") => (!cfg!(feature = "bert")).then_some("bert"),
        _ => None,
    }
}

/// The name in [`KNOWN_MODELS`] of model `name` when this build runs it
/// and its files are not all under `models` yet: what a server fetches
/// for the fields that name it (unless `chloedb-server --no-fetch-models`).
pub fn to_fetch(name: &str, models: &Path) -> Option<&'static str> {
    let &(known, _, _) = KNOWN_MODELS.iter().find(|m| m.0.eq_ignore_ascii_case(name))?;
    let dir = models.join(known);
    (missing_feature(known).is_none() && !model_files(known).iter().all(|f| dir.join(f).exists())).then_some(known)
}

/// The models a schema can name, and whether each would open here: those
/// Strata fetches ([`KNOWN_MODELS`]), the hosted models it knows
/// ([`crate::hosted::KNOWN`]), and the other directories of `models` that
/// hold a model or an `endpoint.json`. Nothing is sent or loaded.
pub fn catalog(models: &Path) -> Vec<Listed> {
    let mut out: Vec<Listed> = KNOWN_MODELS
        .iter()
        .map(|&(name, _, what)| {
            let dir = models.join(name);
            let fetched = model_files(name).iter().all(|f| dir.join(f).exists());
            let ready = match (missing_feature(name), fetched) {
                (Some(f), _) => Err(format!("built without the `{f}` feature")),
                (None, false) => Err(format!("not fetched: chloedb-server fetch-model {name}")),
                (None, true) => Ok(()),
            };
            Listed { name: name.into(), dim: known_dim(name), hosted: false, what: what.into(), ready }
        })
        .collect();
    let ready_hosted = |name: &str, config: Option<&serde_json::Value>| -> std::result::Result<(), String> {
        let ep = crate::hosted::Endpoint::from_config(name, config, &|k| std::env::var(k).ok()).map_err(|e| e.to_string())?;
        match cfg!(feature = "hosted") {
            true => Ok(()),
            false => Err(format!("a hosted model ({}): build with the `hosted` feature", ep.api.as_str())),
        }
    };
    for k in crate::hosted::KNOWN {
        let provider = k.name.split('/').next().unwrap_or_default();
        let config = std::fs::read_to_string(models.join(k.name).join("endpoint.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
        let what = format!("{provider}, {} dimensions{}", k.dim, if k.shortens { " (or fewer)" } else { "" });
        out.push(Listed { name: k.name.into(), dim: Some(k.dim), hosted: true, what, ready: ready_hosted(k.name, config.as_ref()) });
    }
    // Directories of models, and of endpoints, not listed above.
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(models).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        if out.iter().any(|l| l.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let endpoint = dir.join("endpoint.json");
        if endpoint.exists() {
            let config = std::fs::read_to_string(&endpoint).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let dim = config.as_ref().and_then(|c| c.get("dim")).and_then(serde_json::Value::as_u64).map(|d| d as usize);
            let api = config.as_ref().and_then(|c| c.get("api")).and_then(serde_json::Value::as_str).unwrap_or("?").to_string();
            let ready = match &config {
                Some(c) => ready_hosted(&name, Some(c)),
                None => Err(format!("{}: not JSON", endpoint.display())),
            };
            out.push(Listed { name, dim, hosted: true, what: format!("{api} endpoint ({})", endpoint.display()), ready });
        } else if dir.join("model.safetensors").exists() || dir.join(crate::query::cross_encoder::ONNX).exists() {
            let config: serde_json::Value = std::fs::read_to_string(dir.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            // Ranking models and cross-encoders live here too; only embedders are listed.
            let onnx = !dir.join("model.safetensors").exists();
            let kind = format!("{}{}", config["model_type"].as_str().unwrap_or("model2vec"), if onnx { " (ONNX)" } else { "" });
            if config["architectures"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str().is_some_and(|s| s.contains("SequenceClassification")))) {
                continue;
            }
            let dim = config["hidden_size"].as_u64().map(|d| d as usize);
            let ready = match onnx && !cfg!(feature = "onnx") {
                true => Err("built without the `onnx` feature".to_string()),
                false => Ok(()),
            };
            out.push(Listed { name, dim, hosted: false, what: format!("{kind} model in {}", dir.display()), ready });
        }
    }
    out
}

/// The files the model directory of `name` needs: CLAP's repository
/// ships its weights as a PyTorch file only, and a ColBERT checkpoint (a
/// name with `colbert` in it) has its settings in `artifact.metadata`.
pub fn model_files(name: &str) -> &'static [&'static str] {
    match name {
        n if n.starts_with("clap") => &["config.json", "tokenizer.json", "pytorch_model.bin"],
        // A colbert-ai checkpoint: its settings are in artifact.metadata.
        n if n.to_ascii_lowercase().contains("colbert") => &["config.json", "tokenizer.json", "model.safetensors", "artifact.metadata"],
        _ => &["config.json", "tokenizer.json", "model.safetensors"],
    }
}

/// Opens the model a schema names: `hash:<dim>`, or a directory under
/// `models` (or an absolute path) holding a Model2Vec, BERT, SigLIP or
/// CLAP model ([`model_files`]), or a hosted model ([`crate::hosted`]):
/// `<provider>/<model>` (`openai/text-embedding-3-small`), or a directory
/// holding an `endpoint.json`. [`open_for`] opens one for a field's
/// dimensions.
pub fn open(model: &str, models: &Path) -> Result<Arc<dyn Embedder>> {
    open_for(model, None, models)
}

/// The fingerprint of model `model` under `models`: `sha256:` and the
/// SHA-256 of its weights file (`model.safetensors`, `pytorch_model.bin` or
/// `model.onnx`), or `None` for a model without one (`hash:N`, a hosted
/// model) or that is not there. The hash is kept beside the file
/// (`<file>.sha256`, with the file's length and time of change) so that
/// a model is read whole once, not at every open.
pub fn fingerprint(model: &str, models: &Path) -> Option<String> {
    if model.starts_with("hash:") {
        return None;
    }
    let dir = if Path::new(model).is_absolute() { PathBuf::from(model) } else { models.join(model) };
    let file = ["model.safetensors", "pytorch_model.bin", "model.onnx"].iter().map(|f| dir.join(f)).find(|p| p.is_file())?;
    let meta = std::fs::metadata(&file).ok()?;
    let stamp = format!("{} {}", meta.len(), meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos());
    let side = file.with_extension(format!("{}.sha256", file.extension().and_then(|e| e.to_str()).unwrap_or("")));
    if let Ok(kept) = std::fs::read_to_string(&side) {
        if let Some((s, h)) = kept.trim().rsplit_once(' ') {
            if s == stamp && h.len() == 64 {
                return Some(format!("sha256:{h}"));
            }
        }
    }
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(&file).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let h: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    // A models directory may be read-only: the hash is then worked out again.
    let _ = std::fs::write(&side, format!("{stamp} {h}\n"));
    Some(format!("sha256:{h}"))
}

/// Gives each embedded field of `schema` whose model is under `models` the
/// model's [`fingerprint`], where it has none yet (a table being created).
pub fn stamp_fingerprints(schema: &mut crate::schema::Schema, models: &Path) {
    for f in schema.fields.iter_mut() {
        if let Some(e) = f.embed.as_mut().filter(|e| e.fingerprint.is_none()) {
            e.fingerprint = fingerprint(&e.model, models);
        }
    }
}

/// Refuses model `model` for a field whose vectors were made by a model
/// of fingerprint `want`, when the one under `models` differs.
pub fn check_fingerprint(field: &str, model: &str, want: Option<&str>, models: &Path) -> Result<()> {
    let Some(want) = want else { return Ok(()) };
    match fingerprint(model, models) {
        Some(got) if got == want => Ok(()),
        got => Err(model_err(format!(
            "model {model} under {} is not the one field {field}'s vectors came from: its weights are {}, the table recorded {want}. Vectors of the two would not compare. Put back the model the table was made with (chloedb-server fetch-model downloads the pinned revision of a model ChloeDB knows), or make a new table with the new model and copy the rows into it, whose vectors it computes again",
            models.display(),
            got.as_deref().unwrap_or("missing"),
        ))),
    }
}

/// [`open`], for a field of `dim` dimensions: a hosted model that makes
/// more, and can make fewer, is asked for `dim`, and one Strata does not
/// know makes `dim`. A model computed here makes what it makes, which the
/// field must hold.
pub fn open_for(model: &str, dim: Option<usize>, models: &Path) -> Result<Arc<dyn Embedder>> {
    if let Some(d) = model.strip_prefix("hash:") {
        let d = d.parse().map_err(|_| model_err(format!("model {model}: hash:<dimensions>")))?;
        return Ok(Arc::new(HashEmbedder { dim: d }));
    }
    let dir = if Path::new(model).is_absolute() { PathBuf::from(model) } else { models.join(model) };
    let endpoint = dir.join("endpoint.json");
    let has_weights = dir.join("model.safetensors").exists() || dir.join("pytorch_model.bin").exists();
    if endpoint.exists() || (!has_weights && crate::hosted::is_hosted_name(model)) {
        let config = match endpoint.exists() {
            true => {
                let text = std::fs::read_to_string(&endpoint).map_err(|e| model_err(format!("{}: {e}", endpoint.display())))?;
                Some(serde_json::from_str::<serde_json::Value>(&text).map_err(|e| model_err(format!("{}: {e}", endpoint.display())))?)
            }
            false => None,
        };
        let ep = crate::hosted::Endpoint::from_config(model, config.as_ref(), &|k| std::env::var(k).ok())?;
        #[cfg(feature = "hosted")]
        return Ok(Arc::new(crate::hosted::HostedEmbedder::new(ep, dim)?));
        #[cfg(not(feature = "hosted"))]
        return Err(model_err(format!("model {model} is a hosted model ({}): build with the `hosted` feature", ep.api.as_str())));
    }
    let _ = dim;
    if !has_weights && dir.join(crate::query::cross_encoder::ONNX).exists() {
        #[cfg(feature = "onnx")]
        return Ok(Arc::new(OnnxEmbedder::open(&dir)?));
        #[cfg(not(feature = "onnx"))]
        return Err(model_err(format!("model {model} is an ONNX model: build with the `onnx` feature")));
    }
    if !has_weights {
        return Err(model_err(format!("model {model}: no {} (fetch it first)", dir.join("model.safetensors").display())));
    }
    let config: serde_json::Value = std::fs::read_to_string(dir.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if config["model_type"] == "bert" && dir.join("artifact.metadata").exists() {
        #[cfg(feature = "bert")]
        return Ok(Arc::new(ColbertEmbedder::open(&dir)?));
        #[cfg(not(feature = "bert"))]
        return Err(model_err(format!("model {model} is a ColBERT model: build with the `bert` feature")));
    }
    if config["model_type"] == "bert" {
        #[cfg(feature = "bert")]
        return Ok(Arc::new(BertEmbedder::open(&dir)?));
        #[cfg(not(feature = "bert"))]
        return Err(model_err(format!("model {model} is a BERT model: build with the `bert` feature")));
    }
    if config["model_type"] == "siglip" {
        #[cfg(feature = "vision")]
        return Ok(Arc::new(SiglipEmbedder::open(&dir)?));
        #[cfg(not(feature = "vision"))]
        return Err(model_err(format!("model {model} is a SigLIP model: build with the `vision` feature")));
    }
    if config["model_type"] == "clap" {
        #[cfg(feature = "audio")]
        return Ok(Arc::new(crate::clap::ClapEmbedder::open(&dir)?));
        #[cfg(not(feature = "audio"))]
        return Err(model_err(format!("model {model} is a CLAP model: build with the `audio` feature")));
    }
    #[cfg(feature = "embed")]
    return Ok(Arc::new(StaticEmbedder::open(&dir)?));
    #[cfg(not(feature = "embed"))]
    Err(model_err(format!("model {model}: built without the `embed` feature")))
}

// ---------------------------------------------------------------- static

/// Model2Vec: the embedding of a text is the mean of its tokens' rows.
///
/// The table is read as Model2Vec writes it: `embeddings` in F32, F16,
/// BF16 or I8 (an int8 table has one scale for the whole table, which the
/// unit length the vector is scaled to cancels: 4 times smaller than F32
/// and, on BEIR, within 0.002 nDCG@10 of it), and, when the model has them,
/// `weights` (a factor a token) and `mapping` (the row of each token, for a
/// model whose vocabulary was quantised to fewer rows than tokens).
#[cfg(feature = "embed")]
pub struct StaticEmbedder {
    tokenizer: tokenizers::Tokenizer,
    table: Table,
    /// A factor a token id (Model2Vec's `weights`).
    weights: Option<Vec<f32>>,
    /// The row of a token id (Model2Vec's `mapping`).
    mapping: Option<Vec<u32>>,
    dim: usize,
    unk: Option<u32>,
    name: String,
    max_tokens: usize,
}

#[cfg(feature = "embed")]
impl StaticEmbedder {
    pub fn open(dir: &Path) -> Result<StaticEmbedder> {
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| model_err(format!("{}: {e}", dir.join("tokenizer.json").display())))?;
        let path = dir.join("model.safetensors");
        let table = Table::open(&path, "embeddings")?;
        let (rows, dim) = (table.rows, table.dim);
        let vocab = tokenizer.get_vocab_size(true);
        let weights = Table::open_vector(&path, "weights")?.map(|w| w.into_iter().map(|x| x as f32).collect::<Vec<f32>>());
        let mapping = Table::open_vector(&path, "mapping")?.map(|m| m.into_iter().map(|x| x as u32).collect::<Vec<u32>>());
        let bad = |m: String| Err(model_err(format!("{}: {m}", dir.display())));
        match &mapping {
            Some(m) if m.len() < vocab => return bad(format!("mapping has {} tokens, tokenizer {vocab}", m.len())),
            Some(m) if m.iter().any(|&r| r as usize >= rows) => return bad(format!("mapping names a row past the table's {rows}")),
            None if rows < vocab => return bad(format!("embedding table has {rows} rows, tokenizer {vocab} tokens")),
            _ => {}
        }
        if weights.as_ref().is_some_and(|w| w.len() < vocab) {
            return bad(format!("weights has fewer than the tokenizer's {vocab} tokens"));
        }
        let unk = tokenizer.token_to_id("[UNK]");
        let name = format!("static:{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        Ok(StaticEmbedder { tokenizer, table, weights, mapping, dim, unk, name, max_tokens: 512 })
    }

    fn embed_ids(&self, ids: &[u32]) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let mut n = 0usize;
        for &id in ids.iter().filter(|&&id| Some(id) != self.unk).take(self.max_tokens) {
            let row = match &self.mapping {
                Some(m) => match m.get(id as usize) {
                    Some(&r) => r as usize,
                    None => continue,
                },
                None => id as usize,
            };
            let w = self.weights.as_ref().map_or(Some(1.0), |w| w.get(id as usize).copied());
            if let Some(w) = w {
                if self.table.add_row(row, w, &mut v) {
                    n += 1;
                }
            }
        }
        if n > 0 {
            v.iter_mut().for_each(|x| *x /= n as f32);
            normalize(&mut v);
        }
        v
    }
}

#[cfg(feature = "embed")]
impl Embedder for StaticEmbedder {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| match self.tokenizer.encode(*t, false) {
                Ok(e) => self.embed_ids(e.get_ids()),
                Err(_) => vec![0f32; self.dim],
            })
            .collect())
    }
}

/// The element types a static table can hold.
#[cfg(feature = "embed")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Elem {
    F32,
    F16,
    BF16,
    I8,
}

#[cfg(feature = "embed")]
impl Elem {
    fn parse(s: &str) -> Option<Elem> {
        Some(match s {
            "F32" => Elem::F32,
            "F16" => Elem::F16,
            "BF16" => Elem::BF16,
            "I8" => Elem::I8,
            _ => return None,
        })
    }
    fn bytes(self) -> usize {
        match self {
            Elem::F32 => 4,
            Elem::F16 | Elem::BF16 => 2,
            Elem::I8 => 1,
        }
    }
}

/// IEEE half precision to single.
#[cfg(feature = "embed")]
fn f16_to_f32(h: u16) -> f32 {
    let (sign, exp, frac) = ((h as u32 & 0x8000) << 16, (h as u32 >> 10) & 0x1f, h as u32 & 0x3ff);
    let bits = match (exp, frac) {
        (0, 0) => sign,
        (0, f) => {
            // subnormal: normalise it
            let shift = f.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | ((f << shift) & 0x3ff) << 13
        }
        (0x1f, f) => sign | 0x7f80_0000 | f << 13,
        (e, f) => sign | (e + 112) << 23 | f << 13,
    };
    f32::from_bits(bits)
}

/// One tensor of a safetensors file, mapped rather than read: a process
/// starts without loading the table, and only the rows of tokens it meets
/// are paged in.
#[cfg(feature = "embed")]
struct Table {
    map: memmap2::Mmap,
    /// Byte offset of row 0 in the file.
    start: usize,
    rows: usize,
    dim: usize,
    elem: Elem,
}

#[cfg(feature = "embed")]
impl Table {
    /// The mapped file, its JSON header, and the offset of its data.
    fn header(path: &Path) -> Result<(memmap2::Mmap, serde_json::Value, usize)> {
        let file = std::fs::File::open(path).map_err(|e| model_err(format!("{}: {e}", path.display())))?;
        // SAFETY: the model file is not modified while mapped (fetching a
        // model writes a new file).
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| model_err(format!("{}: {e}", path.display())))?;
        let bad = |m: String| model_err(format!("{}: {m}", path.display()));
        if map.len() < 8 {
            return Err(bad("not a safetensors file".into()));
        }
        let hlen = u64::from_le_bytes(map[..8].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(map.get(8..8 + hlen).ok_or_else(|| bad("truncated header".into()))?).map_err(|e| bad(e.to_string()))?;
        Ok((map, header, 8 + hlen))
    }

    /// A tensor's type, shape and byte range, checked against the file.
    fn locate(path: &Path, map: &memmap2::Mmap, t: &serde_json::Value, base: usize, tensor: &str) -> Result<(String, Vec<usize>, usize, usize)> {
        let bad = |m: String| model_err(format!("{}: {m}", path.display()));
        let dtype = t["dtype"].as_str().unwrap_or_default().to_string();
        let shape: Vec<usize> = t["shape"].as_array().into_iter().flatten().filter_map(|x| x.as_u64()).map(|x| x as usize).collect();
        let offs: Vec<usize> = t["data_offsets"].as_array().into_iter().flatten().filter_map(|x| x.as_u64()).map(|x| x as usize).collect();
        let (a, b) = (base + offs.first().copied().unwrap_or(0), base + offs.get(1).copied().unwrap_or(0));
        if b > map.len() || a > b {
            return Err(bad(format!("{tensor}: bytes {a}..{b} past the file's {}", map.len())));
        }
        Ok((dtype, shape, a, b))
    }

    fn open(path: &Path, tensor: &str) -> Result<Table> {
        let (map, header, base) = Self::header(path)?;
        let bad = |m: String| model_err(format!("{}: {m}", path.display()));
        let t = header.get(tensor).ok_or_else(|| bad(format!("no tensor {tensor}")))?;
        let (dtype, shape, a, b) = Self::locate(path, &map, t, base, tensor)?;
        let elem = Elem::parse(&dtype).ok_or_else(|| bad(format!("{tensor}: dtype {dtype} (need F32, F16, BF16 or I8)")))?;
        let [rows, dim] = shape[..] else { return Err(bad(format!("{tensor}: shape {shape:?}"))) };
        if b - a != rows * dim * elem.bytes() {
            return Err(bad(format!("{tensor}: {} bytes for shape {rows}x{dim} of {dtype}", b - a)));
        }
        Ok(Table { map, start: a, rows, dim, elem })
    }

    /// A one-dimensional tensor read whole (Model2Vec's `weights` and
    /// `mapping`), or None if the file has no tensor of that name.
    fn open_vector(path: &Path, tensor: &str) -> Result<Option<Vec<f64>>> {
        let (map, header, base) = Self::header(path)?;
        let Some(t) = header.get(tensor) else { return Ok(None) };
        let bad = |m: String| model_err(format!("{}: {m}", path.display()));
        let (dtype, shape, a, b) = Self::locate(path, &map, t, base, tensor)?;
        let [n] = shape[..] else { return Err(bad(format!("{tensor}: shape {shape:?}"))) };
        let bytes = &map[a..b];
        let width = match dtype.as_str() {
            "F64" | "I64" => 8,
            "F32" | "I32" => 4,
            "F16" | "BF16" => 2,
            _ => return Err(bad(format!("{tensor}: dtype {dtype}"))),
        };
        if bytes.len() != n * width {
            return Err(bad(format!("{tensor}: {} bytes for {n} values of {dtype}", bytes.len())));
        }
        let v = bytes
            .chunks_exact(width)
            .map(|c| match dtype.as_str() {
                "F64" => f64::from_le_bytes(c.try_into().unwrap()),
                "I64" => i64::from_le_bytes(c.try_into().unwrap()) as f64,
                "F32" => f32::from_le_bytes(c.try_into().unwrap()) as f64,
                "I32" => i32::from_le_bytes(c.try_into().unwrap()) as f64,
                "F16" => f16_to_f32(u16::from_le_bytes(c.try_into().unwrap())) as f64,
                _ => f32::from_bits((u16::from_le_bytes(c.try_into().unwrap()) as u32) << 16) as f64,
            })
            .collect();
        Ok(Some(v))
    }

    /// Adds `w` times row `id` to `acc`; false if there is no such row.
    fn add_row(&self, id: usize, w: f32, acc: &mut [f32]) -> bool {
        if id >= self.rows {
            return false;
        }
        let width = self.elem.bytes();
        let off = self.start + id * self.dim * width;
        let row = &self.map[off..off + self.dim * width];
        match self.elem {
            Elem::F32 => acc.iter_mut().zip(row.chunks_exact(4)).for_each(|(a, c)| *a += w * f32::from_le_bytes(c.try_into().unwrap())),
            Elem::F16 => acc.iter_mut().zip(row.chunks_exact(2)).for_each(|(a, c)| *a += w * f16_to_f32(u16::from_le_bytes([c[0], c[1]]))),
            Elem::BF16 => acc.iter_mut().zip(row.chunks_exact(2)).for_each(|(a, c)| *a += w * f32::from_bits((u16::from_le_bytes([c[0], c[1]]) as u32) << 16)),
            Elem::I8 => acc.iter_mut().zip(row).for_each(|(a, &c)| *a += w * (c as i8) as f32),
        }
        true
    }
}


#[cfg(all(test, feature = "embed"))]
mod static_table_tests {
    use super::*;

    /// A safetensors file of the given tensors: (name, dtype, shape, bytes).
    fn write(path: &Path, tensors: &[(&str, &str, Vec<usize>, Vec<u8>)]) {
        let mut header = serde_json::Map::new();
        let mut data = Vec::new();
        for (name, dtype, shape, bytes) in tensors {
            header.insert(name.to_string(), serde_json::json!({"dtype": dtype, "shape": shape, "data_offsets": [data.len(), data.len() + bytes.len()]}));
            data.extend_from_slice(bytes);
        }
        let h = serde_json::Value::Object(header).to_string().into_bytes();
        let mut out = (h.len() as u64).to_le_bytes().to_vec();
        out.extend(h);
        out.extend(data);
        std::fs::write(path, out).unwrap();
    }

    fn f16_bits(x: f32) -> u16 {
        // round to nearest for zero and normal values, which is all the test uses
        if x == 0.0 {
            return 0;
        }
        let b = x.to_bits();
        let sign = ((b >> 16) & 0x8000) as u16;
        let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
        let frac = ((b & 0x7f_ffff) + 0x1000) >> 13;
        sign | ((exp as u16) << 10) + frac as u16
    }

    #[test]
    fn half_precision_converts() {
        for x in [0.0f32, 1.0, -2.5, 0.333, 65504.0, -1e-3] {
            assert!((f16_to_f32(f16_bits(x)) - x).abs() <= x.abs() * 1e-3, "{x}");
        }
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24)); // smallest subnormal
        assert_eq!(f16_to_f32(0x03ff), 1023.0 * 2f32.powi(-24)); // largest subnormal
        assert!(f16_to_f32(0x7c00).is_infinite());
        assert!(f16_to_f32(0x7e00).is_nan());
    }

    #[test]
    fn every_element_type_reads_the_same_rows() {
        let dir = std::env::temp_dir().join(format!("chloedb-static-table-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rows: Vec<f32> = (0..12).map(|i| (i as f32 - 5.5) / 4.0).collect(); // 3 rows of 4
        let scale = rows.iter().fold(0f32, |m, x| m.max(x.abs())) / 127.0;
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("F32", rows.iter().flat_map(|x| x.to_le_bytes()).collect()),
            ("F16", rows.iter().flat_map(|&x| f16_bits(x).to_le_bytes()).collect()),
            ("BF16", rows.iter().flat_map(|x| ((x.to_bits() >> 16) as u16).to_le_bytes()).collect()),
            ("I8", rows.iter().map(|x| (x / scale).round() as i8 as u8).collect()),
        ];
        for (dtype, bytes) in cases {
            let path = dir.join(format!("{dtype}.safetensors"));
            write(&path, &[("embeddings", dtype, vec![3, 4], bytes)]);
            let t = Table::open(&path, "embeddings").unwrap();
            assert_eq!((t.rows, t.dim, t.elem), (3, 4, Elem::parse(dtype).unwrap()));
            let unit = if dtype == "I8" { scale } else { 1.0 };
            for r in 0..3 {
                let mut acc = vec![0f32; 4];
                assert!(t.add_row(r, 2.0, &mut acc));
                for (c, a) in acc.iter().enumerate() {
                    let want = 2.0 * rows[r * 4 + c];
                    assert!((a * unit - want).abs() < 0.02, "{dtype} row {r} col {c}: {} vs {want}", a * unit);
                }
            }
            assert!(!t.add_row(3, 1.0, &mut vec![0f32; 4]));
        }
        // a table of the wrong size, and a type it does not read
        let path = dir.join("bad.safetensors");
        write(&path, &[("embeddings", "I8", vec![3, 5], vec![0; 12])]);
        assert!(Table::open(&path, "embeddings").is_err());
        write(&path, &[("embeddings", "I16", vec![3, 2], vec![0; 12])]);
        assert!(Table::open(&path, "embeddings").is_err());
        // weights and a mapping, as Model2Vec writes them
        let path = dir.join("vectors.safetensors");
        write(&path, &[("weights", "F32", vec![2], [0.5f32, 2.0].iter().flat_map(|x| x.to_le_bytes()).collect()), ("mapping", "I64", vec![2], [1i64, 0].iter().flat_map(|x| x.to_le_bytes()).collect())]);
        assert_eq!(Table::open_vector(&path, "weights").unwrap(), Some(vec![0.5, 2.0]));
        assert_eq!(Table::open_vector(&path, "mapping").unwrap(), Some(vec![1.0, 0.0]));
        assert_eq!(Table::open_vector(&path, "nothing").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---------------------------------------------------------------- hash

/// Lowercased words hashed into `dim` buckets (signed), for tests. An
/// image is the words of its colours: each cell of a 4 by 4 grid is a word
/// of its mean colour, a few levels a channel, so equal images have equal
/// vectors and images of like colours are near.
pub struct HashEmbedder {
    pub dim: usize,
}

impl HashEmbedder {
    fn of_words<'a>(&self, words: impl Iterator<Item = &'a str>) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        for w in words {
            let h = fnv(&w.to_lowercase());
            v[(h % self.dim as u64) as usize] += if h >> 63 == 0 { 1.0 } else { -1.0 };
        }
        normalize(&mut v);
        v
    }

    /// The words of an image's colours.
    fn colour_words(image: &Image) -> Vec<String> {
        let (w, h) = (image.width.max(1) as usize, image.height.max(1) as usize);
        let mut sums = [[0u64; 4]; 16];
        for y in 0..image.height as usize {
            for x in 0..image.width as usize {
                let (cell, at) = ((y * 4 / h) * 4 + x * 4 / w, (y * w + x) * 3);
                let Some(p) = image.rgb.get(at..at + 3) else { continue };
                for c in 0..3 {
                    sums[cell][c] += p[c] as u64;
                }
                sums[cell][3] += 1;
            }
        }
        sums.iter().enumerate().filter(|(_, s)| s[3] > 0).map(|(i, s)| format!("c{i}r{}g{}b{}", s[0] / s[3] / 64, s[1] / s[3] / 64, s[2] / s[3] / 64)).collect()
    }
}

impl Embedder for HashEmbedder {
    fn name(&self) -> String {
        format!("hash:{}", self.dim)
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.of_words(t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()))).collect())
    }
    fn tokens(&self) -> bool {
        true
    }
    /// A vector a word, so that `maxsim` counts the query's words a row has.
    fn embed_tokens(&self, texts: &[&str]) -> Result<Vec<Vec<Vec<f32>>>> {
        Ok(texts.iter().map(|t| t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| self.of_words(std::iter::once(w))).collect()).collect())
    }
    fn modalities(&self) -> &'static [Modality] {
        &[Modality::Text, Modality::Image]
    }
    fn embed_inputs(&self, inputs: &[Input<'_>]) -> Result<Vec<Vec<f32>>> {
        inputs
            .iter()
            .map(|i| match i {
                Input::Text(t) => Ok(self.of_words(t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()))),
                Input::Image(image) => Ok(self.of_words(Self::colour_words(image).iter().map(String::as_str))),
                Input::Audio(_) => Err(model_err(format!("model {} does not embed sound", self.name()))),
            })
            .collect()
    }
}

/// 64-bit FNV-1a.
pub fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------- onnx

/// An encoder exported to ONNX (`model.onnx`, taking `input_ids` and
/// `attention_mask`, and `token_type_ids` if a BERT's), run by ONNX
/// Runtime on every core, its vector the first token's output (CLS) or the
/// mean of its tokens', as the directory's sentence-transformers files
/// say (`1_Pooling/config.json`; CLS if there are none). A query is
/// prefixed with the model's query prompt
/// (`config_sentence_transformers.json`), if it has one, and a text is cut
/// to `max_seq_length` tokens (`sentence_bert_config.json`; at most 512).
/// An int8 export
/// (ONNX Runtime's dynamic quantisation) runs a ModernBERT about twice as
/// fast as float32, within 0.01 nDCG@10 of it on BEIR.
#[cfg(feature = "onnx")]
pub struct OnnxEmbedder {
    tokenizer: tokenizers::Tokenizer,
    session: std::sync::Mutex<ort::session::Session>,
    types: bool,
    mean: bool,
    pad: u32,
    dim: usize,
    query_prefix: String,
    name: String,
}

#[cfg(feature = "onnx")]
impl OnnxEmbedder {
    /// Texts in one run, sorted by length so that little of it is padding.
    const BATCH: usize = 32;

    pub fn open(dir: &Path) -> Result<OnnxEmbedder> {
        use ort::session::builder::GraphOptimizationLevel;
        use ort::session::Session;
        let e = |x: String| model_err(format!("{}: {x}", dir.display()));
        let json = |f: &str| -> Option<serde_json::Value> { std::fs::read_to_string(dir.join(f)).ok().and_then(|t| serde_json::from_str(&t).ok()) };
        let config = json("config.json").unwrap_or_default();
        let pooling = json("1_Pooling/config.json").unwrap_or_default();
        let mean = pooling["pooling_mode_mean_tokens"].as_bool().unwrap_or(false) && !pooling["pooling_mode_cls_token"].as_bool().unwrap_or(false);
        let query_prefix = json("config_sentence_transformers.json").and_then(|c| c["prompts"]["query"].as_str().map(String::from)).unwrap_or_default();
        let pad = config["pad_token_id"].as_u64().unwrap_or(0) as u32;
        // sentence-transformers' max_seq_length, if set, else the model's
        // positions, at most 512: fewer tokens run faster (128 tokens embed
        // about 7 times faster than 512, for 0.01 to 0.04 nDCG@10 on BEIR).
        let max_tokens = json("sentence_bert_config.json")
            .and_then(|c| c["max_seq_length"].as_u64())
            .or_else(|| config["max_position_embeddings"].as_u64())
            .map_or(512, |n| n as usize)
            .min(512);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let session = Session::builder(crate::query::cross_encoder::onnx_environment().map_err(|x| e(x.to_string()))?)
            .map_err(|x| e(x.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|x| e(x.to_string()))?
            .with_intra_threads(threads)
            .map_err(|x| e(x.to_string()))?
            .commit_from_file(dir.join(crate::query::cross_encoder::ONNX))
            .map_err(|x| e(format!("model.onnx: {x}")))?;
        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        if !names.iter().any(|n| n == "input_ids") || !names.iter().any(|n| n == "attention_mask") {
            return Err(e("model.onnx does not take input_ids and attention_mask".into()));
        }
        let types = names.iter().any(|n| n == "token_type_ids");
        let dim = config["hidden_size"].as_u64().or_else(|| config["dim"].as_u64()).ok_or_else(|| e("config.json has no hidden_size".into()))? as usize;
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|x| e(x.to_string()))?;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams { max_length: max_tokens, ..Default::default() })).map_err(|x| e(x.to_string()))?;
        tokenizer.with_padding(None);
        let name = format!("onnx:{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        Ok(OnnxEmbedder { tokenizer, session: std::sync::Mutex::new(session), types, mean, pad, dim, query_prefix, name })
    }

    fn run(&self, enc: &[&tokenizers::Encoding]) -> Result<Vec<Vec<f32>>> {
        use ort::value::Tensor;
        let attempt = || -> std::result::Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
            let (n, len) = (enc.len(), enc.iter().map(|e| e.len()).max().unwrap_or(0).max(1));
            let (mut ids, mut mask) = (vec![self.pad as i64; n * len], vec![0i64; n * len]);
            for (k, e) in enc.iter().enumerate() {
                for (j, &id) in e.get_ids().iter().enumerate() {
                    ids[k * len + j] = id as i64;
                    mask[k * len + j] = 1;
                }
            }
            let shape = vec![n as i64, len as i64];
            let tensor = |v: Vec<i64>| Tensor::from_array((shape.clone(), v));
            let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
            let outputs = match self.types {
                true => session.run(ort::inputs!["input_ids" => tensor(ids)?, "attention_mask" => tensor(mask.clone())?, "token_type_ids" => tensor(vec![0i64; n * len])?])?,
                false => session.run(ort::inputs!["input_ids" => tensor(ids)?, "attention_mask" => tensor(mask.clone())?])?,
            };
            let (shape, h) = outputs[0].try_extract_tensor::<f32>()?;
            let d = *shape.last().ok_or("the model's output has no dimensions")? as usize;
            if d != self.dim || h.len() != n * len * d {
                return Err(format!("the model's output is {shape:?}, not [{n}, {len}, {}]", self.dim).into());
            }
            Ok((0..n)
                .map(|k| {
                    let mut v = vec![0f32; d];
                    let rows = if self.mean { 0..enc[k].len() } else { 0..1 };
                    let count = rows.len().max(1) as f32;
                    for j in rows {
                        v.iter_mut().zip(&h[(k * len + j) * d..(k * len + j + 1) * d]).for_each(|(a, &x)| *a += x / count);
                    }
                    normalize(&mut v);
                    v
                })
                .collect())
        };
        attempt().map_err(|e| model_err(format!("{}: {e}", self.name)))
    }
}

#[cfg(feature = "onnx")]
impl Embedder for OnnxEmbedder {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn dim(&self) -> usize {
        self.dim
    }
    /// ONNX Runtime runs a batch on every core itself.
    fn parallel(&self) -> bool {
        false
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let enc = self.tokenizer.encode_batch(texts.to_vec(), true).map_err(|e| model_err(format!("{}: {e}", self.name)))?;
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| enc[i].len());
        let mut out = vec![Vec::new(); texts.len()];
        for chunk in order.chunks(Self::BATCH) {
            let batch: Vec<&tokenizers::Encoding> = chunk.iter().map(|&i| &enc[i]).collect();
            for (&i, v) in chunk.iter().zip(self.run(&batch)?) {
                out[i] = v;
            }
        }
        Ok(out)
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed(&[&format!("{}{text}", self.query_prefix)])?.pop().unwrap_or_default())
    }
}

// ---------------------------------------------------------------- learned sparse

/// A learned sparse encoder for SPARSE fields, "inference-free" on the
/// query side (OpenSearch's neural sparse doc-v3-distill and its kind): a
/// document's term weights come from a transformer with its masked-language
/// head, exported to ONNX with the pooling in the graph (its one output is
/// `[batch, vocabulary]`, the max over tokens of `log(1 + relu(logits))`;
/// `research/cpu-encoders/sparse_onnx_export.py` writes it), and a query's
/// are its tokens weighted by the model's IDF table (`idf.json`), a lookup
/// that runs no model. Terms are the tokenizer's vocabulary entries, so a
/// document's and a query's weights meet in a SPARSE field's index, which
/// ranks their dot product with the text index's top-k code.
///
/// On BEIR (SciFact, NFCorpus, ArguAna; `examples/sparse_beir.rs`, int8,
/// 4 cores) its documents fused with BM25 by `rrf` come within 1.2 to 1.8
/// points of nDCG@10 of granite-small-r2's dense vectors, with a query's
/// p50 at 0.2 to 2.6 ms against 6 to 50 ms and 50 to 80 times the queries
/// a second, and documents encode faster than granite's (19 to 34 a
/// second).
#[cfg(feature = "onnx")]
pub struct SparseEncoder {
    tokenizer: tokenizers::Tokenizer,
    session: std::sync::Mutex<ort::session::Session>,
    /// The vocabulary entry of each token id.
    terms: Vec<String>,
    /// The query side's weight of each token id (1 where `idf.json` has none).
    idf: Vec<f32>,
    /// Token ids that are never terms: the tokenizer's special tokens.
    special: Vec<bool>,
    pad: u32,
    name: String,
}

#[cfg(feature = "onnx")]
impl SparseEncoder {
    /// Tokens in one run, padding included: the head's logits are
    /// `[batch, tokens, vocabulary]` floats, 2 GB for 32 documents of 512
    /// tokens, so a run is sized by tokens (4,096: 0.5 GB), not documents.
    const TOKENS: usize = 4096;

    pub fn open(dir: &Path) -> Result<SparseEncoder> {
        use ort::session::builder::GraphOptimizationLevel;
        use ort::session::Session;
        let e = |x: String| model_err(format!("{}: {x}", dir.display()));
        let config: serde_json::Value = std::fs::read_to_string(dir.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        // sentence-transformers' max_seq_length, if set, else the model's
        // positions, at most 512, as OnnxEmbedder reads it
        let max_tokens = std::fs::read_to_string(dir.join("sentence_bert_config.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|c| c["max_seq_length"].as_u64())
            .or_else(|| config["max_position_embeddings"].as_u64())
            .map_or(512, |n| n as usize)
            .min(512);
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|x| e(x.to_string()))?;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams { max_length: max_tokens, ..Default::default() })).map_err(|x| e(x.to_string()))?;
        // tokenizer.json may pad a batch to its longest, and the mask would
        // then cover the padding: a run pads itself
        tokenizer.with_padding(None);
        let vocab = tokenizer.get_vocab_size(true);
        let terms: Vec<String> = (0..vocab as u32).map(|i| tokenizer.id_to_token(i).unwrap_or_default()).collect();
        let mut special = vec![false; vocab];
        for (&id, t) in tokenizer.get_added_tokens_decoder().iter() {
            if t.special && (id as usize) < vocab {
                special[id as usize] = true;
            }
        }
        let idf_text = std::fs::read_to_string(dir.join("idf.json")).map_err(|x| e(format!("idf.json: {x}")))?;
        let idf_map: std::collections::HashMap<String, f64> = serde_json::from_str(&idf_text).map_err(|x| e(format!("idf.json: {x}")))?;
        let mut idf = vec![1f32; vocab];
        for (t, w) in idf_map {
            if let Some(id) = tokenizer.token_to_id(&t) {
                if let Some(slot) = idf.get_mut(id as usize) {
                    *slot = w as f32;
                }
            }
        }
        let pad = config["pad_token_id"].as_u64().unwrap_or(0) as u32;
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let session = Session::builder(crate::query::cross_encoder::onnx_environment().map_err(|x| e(x.to_string()))?)
            .map_err(|x| e(x.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|x| e(x.to_string()))?
            .with_intra_threads(threads)
            .map_err(|x| e(x.to_string()))?
            .commit_from_file(dir.join(crate::query::cross_encoder::ONNX))
            .map_err(|x| e(format!("model.onnx: {x}")))?;
        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        if !names.iter().any(|n| n == "input_ids") || !names.iter().any(|n| n == "attention_mask") {
            return Err(e("model.onnx does not take input_ids and attention_mask".into()));
        }
        let name = format!("sparse:{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        Ok(SparseEncoder { tokenizer, session: std::sync::Mutex::new(session), terms, idf, special, pad, name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The documents' term weights, the zero weights left out.
    pub fn encode_documents(&self, texts: &[&str]) -> Result<Vec<Vec<(String, f32)>>> {
        let enc = self.tokenizer.encode_batch(texts.to_vec(), true).map_err(|e| model_err(format!("{}: {e}", self.name)))?;
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| enc[i].len());
        let mut out = vec![Vec::new(); texts.len()];
        // runs of documents sorted by length, so that little is padding,
        // each of at most TOKENS tokens padded
        let mut start = 0;
        while start < order.len() {
            let mut end = start + 1;
            while end < order.len() && (end - start + 1) * enc[order[end]].len().max(1) <= Self::TOKENS {
                end += 1;
            }
            let chunk = &order[start..end];
            let batch: Vec<&tokenizers::Encoding> = chunk.iter().map(|&i| &enc[i]).collect();
            for (&i, v) in chunk.iter().zip(self.run(&batch)?) {
                out[i] = v;
            }
            start = end;
        }
        Ok(out)
    }

    /// A query's term weights: its distinct tokens, each weighted by the
    /// IDF table. No model runs.
    pub fn encode_query(&self, text: &str) -> Result<Vec<(String, f32)>> {
        let enc = self.tokenizer.encode(text, false).map_err(|e| model_err(format!("{}: {e}", self.name)))?;
        let mut ids: Vec<u32> = enc.get_ids().iter().copied().filter(|&i| !self.special.get(i as usize).copied().unwrap_or(true)).collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids.into_iter().map(|i| (self.terms[i as usize].clone(), self.idf[i as usize])).collect())
    }

    /// Term weights as a SPARSE value: a JSON object of term to weight.
    pub fn to_json(terms: &[(String, f32)]) -> String {
        let m: serde_json::Map<String, serde_json::Value> = terms.iter().map(|(t, w)| (t.clone(), serde_json::Value::from(*w as f64))).collect();
        serde_json::Value::Object(m).to_string()
    }

    fn run(&self, enc: &[&tokenizers::Encoding]) -> Result<Vec<Vec<(String, f32)>>> {
        use ort::value::Tensor;
        let attempt = || -> std::result::Result<Vec<Vec<(String, f32)>>, Box<dyn std::error::Error>> {
            let (n, len) = (enc.len(), enc.iter().map(|e| e.len()).max().unwrap_or(0).max(1));
            let (mut ids, mut mask) = (vec![self.pad as i64; n * len], vec![0i64; n * len]);
            for (k, e) in enc.iter().enumerate() {
                for (j, &id) in e.get_ids().iter().enumerate() {
                    ids[k * len + j] = id as i64;
                    mask[k * len + j] = 1;
                }
            }
            let shape = vec![n as i64, len as i64];
            let tensor = |v: Vec<i64>| Tensor::from_array((shape.clone(), v));
            let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
            let outputs = session.run(ort::inputs!["input_ids" => tensor(ids)?, "attention_mask" => tensor(mask)?])?;
            let (shape, w) = outputs[0].try_extract_tensor::<f32>()?;
            let v = self.terms.len();
            if shape.len() != 2 || w.len() != n * v {
                return Err(format!("the model's output is {shape:?}, not [{n}, {v}]: export it with its pooling (sparse_onnx_export.py)").into());
            }
            Ok((0..n)
                .map(|k| {
                    w[k * v..(k + 1) * v]
                        .iter()
                        .enumerate()
                        .filter(|&(i, &x)| x > 0.0 && !self.special[i])
                        .map(|(i, &x)| (self.terms[i].clone(), x))
                        .collect()
                })
                .collect())
        };
        attempt().map_err(|e| model_err(format!("{}: {e}", self.name)))
    }
}

// ---------------------------------------------------------------- bert

#[cfg(feature = "bert")]
pub struct BertEmbedder {
    model: candle_transformers::models::bert::BertModel,
    tokenizer: tokenizers::Tokenizer,
    dim: usize,
    name: String,
}

#[cfg(feature = "bert")]
impl BertEmbedder {
    /// The query instruction of BGE v1.5 and Arctic Embed v1.5.
    const QUERY_PREFIX: &'static str = "Represent this sentence for searching relevant passages: ";

    pub fn open(dir: &Path) -> Result<BertEmbedder> {
        use candle_nn::VarBuilder;
        use candle_transformers::models::bert::{BertModel, Config};
        let e = |x: String| model_err(format!("{}: {x}", dir.display()));
        let text = std::fs::read_to_string(dir.join("config.json")).map_err(|x| e(x.to_string()))?;
        let cfg: Config = serde_json::from_str(&text).map_err(|x| e(x.to_string()))?;
        let dim = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["hidden_size"].as_u64()).unwrap_or(384) as usize;
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|x| e(x.to_string()))?;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams { max_length: 512, ..Default::default() })).map_err(|x| e(x.to_string()))?;
        tokenizer.with_padding(Some(tokenizers::PaddingParams::default()));
        let dev = candle_core::Device::Cpu;
        // SAFETY: the weights file is not modified while mapped.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], candle_core::DType::F32, &dev) }.map_err(|x| e(x.to_string()))?;
        let model = BertModel::load(vb, &cfg).map_err(|x| e(x.to_string()))?;
        let name = format!("bert:{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        Ok(BertEmbedder { model, tokenizer, dim, name })
    }

    fn run(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        use candle_core::Tensor;
        let attempt = || -> std::result::Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
            let enc = self.tokenizer.encode_batch(texts.to_vec(), true).map_err(|e| e.to_string())?;
            let ids: Vec<Vec<u32>> = enc.iter().map(|e| e.get_ids().to_vec()).collect();
            let mask: Vec<Vec<u32>> = enc.iter().map(|e| e.get_attention_mask().to_vec()).collect();
            let dev = candle_core::Device::Cpu;
            let ids = Tensor::new(ids, &dev)?;
            let mask = Tensor::new(mask, &dev)?;
            let types = ids.zeros_like()?;
            let out = self.model.forward(&ids, &types, Some(&mask))?;
            let cls = out.narrow(1, 0, 1)?.squeeze(1)?;
            let mut v = cls.to_vec2::<f32>()?;
            v.iter_mut().for_each(|x| normalize(x));
            Ok(v)
        };
        attempt().map_err(|e| model_err(format!("{}: {e}", self.name)))
    }
}

#[cfg(feature = "bert")]
impl Embedder for BertEmbedder {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        // Batches of similar length waste less on padding.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| texts[i].len());
        let mut out = vec![Vec::new(); texts.len()];
        for chunk in order.chunks(16) {
            let batch: Vec<String> = chunk.iter().map(|&i| texts[i].to_string()).collect();
            for (&i, v) in chunk.iter().zip(self.run(&batch)?) {
                out[i] = v;
            }
        }
        Ok(out)
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.run(&[format!("{}{text}", Self::QUERY_PREFIX)])?.pop().unwrap_or_default())
    }
}

// ---------------------------------------------------------------- colbert

/// ColBERT: a BERT encoder whose every token's output, projected to `dim`
/// dimensions and scaled to unit length, is a vector of the text, for a
/// multi-vector field that `maxsim` ranks (late interaction). It reads a
/// checkpoint of the `colbert-ai` library (`artifact.metadata` beside
/// `config.json`, `tokenizer.json` and `model.safetensors`, whose
/// `linear.weight` is the projection), such as
/// `answerdotai/answerai-colbert-small-v1` (96 dimensions) or
/// `colbert-ir/colbertv2.0` (128), as the library reads it:
///
/// * a document is `[CLS] [D] text [SEP]`, cut to `doc_maxlen` tokens,
///   and its punctuation's vectors are left out (`mask_punctuation`);
/// * a query is `[CLS] [Q] text [SEP]`, cut or padded with `[MASK]` to
///   `query_maxlen` tokens, each of them a vector (query augmentation); the
///   masks are attended to only if `attend_to_mask_tokens`.
#[cfg(feature = "bert")]
pub struct ColbertEmbedder {
    model: candle_transformers::models::bert::BertModel,
    /// The projection: `dim` rows of the hidden size.
    linear: candle_core::Tensor,
    tokenizer: tokenizers::Tokenizer,
    cls: u32,
    sep: u32,
    mask: u32,
    pad: u32,
    query_marker: u32,
    doc_marker: u32,
    query_len: usize,
    doc_len: usize,
    attend_masks: bool,
    /// The tokens of ASCII punctuation, whose vectors documents leave out.
    punctuation: std::collections::HashSet<u32>,
    dim: usize,
    name: String,
}

#[cfg(feature = "bert")]
impl ColbertEmbedder {
    pub fn open(dir: &Path) -> Result<ColbertEmbedder> {
        use candle_nn::VarBuilder;
        use candle_transformers::models::bert::{BertModel, Config};
        let e = |x: String| model_err(format!("{}: {x}", dir.display()));
        let text = std::fs::read_to_string(dir.join("config.json")).map_err(|x| e(x.to_string()))?;
        let cfg: Config = serde_json::from_str(&text).map_err(|x| e(x.to_string()))?;
        let config: serde_json::Value = serde_json::from_str(&text).map_err(|x| e(x.to_string()))?;
        let hidden = config["hidden_size"].as_u64().ok_or_else(|| e("config.json has no hidden_size".into()))? as usize;
        let positions = config["max_position_embeddings"].as_u64().unwrap_or(512) as usize;
        let meta = std::fs::read_to_string(dir.join("artifact.metadata")).map_err(|x| e(x.to_string()))?;
        let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|x| e(format!("artifact.metadata: {x}")))?;
        let num = |k: &str, default: u64| meta[k].as_u64().unwrap_or(default) as usize;
        let (dim, query_len, doc_len) = (num("dim", 128), num("query_maxlen", 32), num("doc_maxlen", 180));
        if let Some(s) = meta["similarity"].as_str().filter(|s| *s != "cosine") {
            return Err(e(format!("artifact.metadata: similarity {s}: ChloeDB scores ColBERT's cosine")));
        }
        if query_len < 4 || doc_len < 4 || query_len > positions || doc_len > positions {
            return Err(e(format!("artifact.metadata: query_maxlen {query_len} and doc_maxlen {doc_len} are 4 to {positions} tokens")));
        }
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|x| e(x.to_string()))?;
        // Texts are cut and padded here, as colbert-ai does.
        tokenizer.with_truncation(None).map_err(|x| e(x.to_string()))?;
        tokenizer.with_padding(None);
        let id = |t: &str| tokenizer.token_to_id(t).ok_or_else(|| e(format!("tokenizer.json has no token {t}")));
        let (cls, sep, mask, pad) = (id("[CLS]")?, id("[SEP]")?, id("[MASK]")?, id("[PAD]")?);
        let query_marker = id(meta["query_token_id"].as_str().unwrap_or("[unused0]"))?;
        let doc_marker = id(meta["doc_token_id"].as_str().unwrap_or("[unused1]"))?;
        // As colbert-ai: the first token of each ASCII punctuation mark.
        let punctuation = match meta["mask_punctuation"].as_bool().unwrap_or(true) {
            true => "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".chars().filter_map(|c| tokenizer.encode(c.to_string(), false).ok()?.get_ids().first().copied()).collect(),
            false => Default::default(),
        };
        let dev = candle_core::Device::Cpu;
        // SAFETY: the weights file is not modified while mapped.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], candle_core::DType::F32, &dev) }.map_err(|x| e(x.to_string()))?;
        let model = BertModel::load(vb.pp("bert"), &cfg).map_err(|x| e(x.to_string()))?;
        let linear = vb.get((dim, hidden), "linear.weight").map_err(|x| e(format!("linear.weight: {x}")))?;
        let name = format!("colbert:{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        Ok(ColbertEmbedder {
            model,
            linear,
            tokenizer,
            cls,
            sep,
            mask,
            pad,
            query_marker,
            doc_marker,
            query_len,
            doc_len,
            attend_masks: meta["attend_to_mask_tokens"].as_bool().unwrap_or(false),
            punctuation,
            dim,
            name,
        })
    }

    /// `[CLS] marker text [SEP]`, the text cut so that all of it is at most
    /// `len` tokens: colbert-ai's `". " + text` with its `.` replaced by
    /// the marker.
    fn ids(&self, text: &str, len: usize, marker: u32) -> Result<Vec<u32>> {
        let enc = self.tokenizer.encode(text, false).map_err(|x| model_err(format!("{}: {x}", self.name)))?;
        let mut ids = Vec::with_capacity(len);
        ids.extend([self.cls, marker]);
        ids.extend(enc.get_ids().iter().take(len - 3));
        ids.push(self.sep);
        Ok(ids)
    }

    /// The unit vectors of every position of `seqs` (padded to the
    /// longest, `attend` saying which positions are attended to).
    fn run(&self, seqs: &[Vec<u32>], attend: &[Vec<u32>]) -> Result<Vec<Vec<Vec<f32>>>> {
        use candle_core::Tensor;
        let attempt = || -> std::result::Result<Vec<Vec<Vec<f32>>>, Box<dyn std::error::Error>> {
            let len = seqs.iter().map(Vec::len).max().unwrap_or(0);
            let pad = |v: &Vec<u32>, with: u32| v.iter().copied().chain(std::iter::repeat(with)).take(len).collect::<Vec<u32>>();
            let dev = candle_core::Device::Cpu;
            let ids = Tensor::new(seqs.iter().map(|s| pad(s, self.pad)).collect::<Vec<_>>(), &dev)?;
            let mask = Tensor::new(attend.iter().map(|a| pad(a, 0)).collect::<Vec<_>>(), &dev)?;
            let types = ids.zeros_like()?;
            let out = self.model.forward(&ids, &types, Some(&mask))?;
            let mut v = out.broadcast_matmul(&self.linear.t()?)?.to_vec3::<f32>()?;
            v.iter_mut().flatten().for_each(|x| normalize(x));
            Ok(v)
        };
        attempt().map_err(|x| model_err(format!("{}: {x}", self.name)))
    }
}

#[cfg(feature = "bert")]
impl Embedder for ColbertEmbedder {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Err(model_err(format!("model {} embeds a text into a vector a token: it is the model of a multi-vector field (VECTOR({})[])", self.name, self.dim)))
    }
    fn single(&self) -> bool {
        false
    }
    fn tokens(&self) -> bool {
        true
    }
    fn embed_tokens(&self, texts: &[&str]) -> Result<Vec<Vec<Vec<f32>>>> {
        let seqs = texts.iter().map(|t| self.ids(t, self.doc_len, self.doc_marker)).collect::<Result<Vec<_>>>()?;
        // Batches of similar length waste less on padding.
        let mut order: Vec<usize> = (0..seqs.len()).collect();
        order.sort_by_key(|&i| seqs[i].len());
        let mut out = vec![Vec::new(); seqs.len()];
        for chunk in order.chunks(16) {
            let batch: Vec<Vec<u32>> = chunk.iter().map(|&i| seqs[i].clone()).collect();
            let attend: Vec<Vec<u32>> = batch.iter().map(|s| vec![1; s.len()]).collect();
            for (&i, vs) in chunk.iter().zip(self.run(&batch, &attend)?) {
                let ids = &seqs[i];
                out[i] = vs.into_iter().zip(ids).filter(|(_, id)| !self.punctuation.contains(*id)).map(|(v, _)| v).collect();
            }
        }
        Ok(out)
    }
    fn embed_query_tokens(&self, text: &str) -> Result<Vec<Vec<f32>>> {
        let mut ids = self.ids(text, self.query_len, self.query_marker)?;
        let mut attend = vec![1u32; ids.len()];
        ids.resize(self.query_len, self.mask);
        attend.resize(self.query_len, u32::from(self.attend_masks));
        Ok(self.run(&[ids], &[attend])?.pop().unwrap_or_default())
    }
}

// ---------------------------------------------------------------- siglip

/// SigLIP: a vision transformer and a text transformer trained so that an
/// image and its caption meet (`google/siglip-base-patch16-224`). Images
/// are resized to the model's square (bicubic, without keeping the aspect
/// ratio, as the model was trained) and scaled to [-1, 1]; texts are
/// lowercased without punctuation by the tokenizer, and padded to the 64
/// tokens the text tower pools the last of. Text is embedded alike for
/// documents and queries.
#[cfg(feature = "vision")]
pub struct SiglipEmbedder {
    model: candle_transformers::models::siglip::Model,
    tokenizer: tokenizers::Tokenizer,
    /// The side of the square images are resized to.
    side: usize,
    dim: usize,
    name: String,
}

#[cfg(feature = "vision")]
impl SiglipEmbedder {
    /// Images a forward pass takes at once.
    const BATCH: usize = 8;

    pub fn open(dir: &Path) -> Result<SiglipEmbedder> {
        use candle_nn::VarBuilder;
        use candle_transformers::models::siglip::{Config, Model};
        let e = |x: String| model_err(format!("{}: {x}", dir.display()));
        let text = std::fs::read_to_string(dir.join("config.json")).map_err(|x| e(x.to_string()))?;
        let cfg: Config = serde_json::from_str(&text).map_err(|x| e(x.to_string()))?;
        let len = cfg.text_config.max_position_embeddings;
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|x| e(x.to_string()))?;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams { max_length: len, ..Default::default() })).map_err(|x| e(x.to_string()))?;
        let pad = cfg.text_config.pad_token_id;
        tokenizer.with_padding(Some(tokenizers::PaddingParams { strategy: tokenizers::PaddingStrategy::Fixed(len), pad_id: pad, pad_token: "</s>".into(), ..Default::default() }));
        let dev = candle_core::Device::Cpu;
        // SAFETY: the weights file is not modified while mapped.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], candle_core::DType::F32, &dev) }.map_err(|x| e(x.to_string()))?;
        let model = Model::new(&cfg, vb).map_err(|x| e(x.to_string()))?;
        let name = format!("siglip:{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        Ok(SiglipEmbedder { model, tokenizer, side: cfg.vision_config.image_size, dim: cfg.text_config.hidden_size, name })
    }

    fn texts(&self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, Box<dyn std::error::Error + Send + Sync>> {
        let enc = self.tokenizer.encode_batch(texts.to_vec(), true)?;
        let ids: Vec<Vec<u32>> = enc.iter().map(|e| e.get_ids().to_vec()).collect();
        let ids = candle_core::Tensor::new(ids, &candle_core::Device::Cpu)?;
        let mut v = self.model.get_text_features(&ids)?.to_vec2::<f32>()?;
        v.iter_mut().for_each(|x| normalize(x));
        Ok(v)
    }

    /// The pixels the image tower takes: `side` by `side`, channels first,
    /// in [-1, 1].
    fn pixels(&self, image: &Image) -> Result<Vec<f32>> {
        let rgb = image::RgbImage::from_raw(image.width, image.height, image.rgb.clone()).ok_or_else(|| model_err("an image's pixels do not fill its size"))?;
        let side = self.side as u32;
        let small = image::imageops::resize(&rgb, side, side, image::imageops::FilterType::CatmullRom);
        let n = self.side * self.side;
        let mut out = vec![0f32; 3 * n];
        for (i, p) in small.pixels().enumerate() {
            for c in 0..3 {
                out[c * n + i] = p[c] as f32 / 127.5 - 1.0;
            }
        }
        Ok(out)
    }

    fn images(&self, images: &[&Image]) -> Result<Vec<Vec<f32>>> {
        let e = |x: candle_core::Error| model_err(format!("{}: {x}", self.name));
        let mut out = Vec::with_capacity(images.len());
        for batch in images.chunks(Self::BATCH) {
            let pixels = batch.iter().map(|i| self.pixels(i)).collect::<Result<Vec<_>>>()?.concat();
            let t = candle_core::Tensor::from_vec(pixels, (batch.len(), 3, self.side, self.side), &candle_core::Device::Cpu).map_err(e)?;
            let mut v = self.model.get_image_features(&t).and_then(|f| f.to_vec2::<f32>()).map_err(e)?;
            v.iter_mut().for_each(|x| normalize(x));
            out.extend(v);
        }
        Ok(out)
    }
}

#[cfg(feature = "vision")]
impl Embedder for SiglipEmbedder {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for c in texts.chunks(32) {
            out.extend(self.texts(c).map_err(|e| model_err(format!("{}: {e}", self.name)))?);
        }
        Ok(out)
    }
    fn modalities(&self) -> &'static [Modality] {
        &[Modality::Text, Modality::Image]
    }
    fn embed_inputs(&self, inputs: &[Input<'_>]) -> Result<Vec<Vec<f32>>> {
        if inputs.iter().any(|i| matches!(i, Input::Audio(_))) {
            return Err(model_err(format!("model {} does not embed sound", self.name)));
        }
        let texts: Vec<&str> = inputs.iter().filter_map(|i| if let Input::Text(t) = i { Some(*t) } else { None }).collect();
        let images: Vec<&Image> = inputs.iter().filter_map(|i| if let Input::Image(m) = i { Some(*m) } else { None }).collect();
        let (mut texts, mut images) = (self.embed(&texts)?.into_iter(), self.images(&images)?.into_iter());
        Ok(inputs.iter().map(|i| if let Input::Text(_) = i { texts.next() } else { images.next() }.unwrap_or_default()).collect())
    }
}
