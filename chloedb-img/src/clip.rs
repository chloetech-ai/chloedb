//! Words and pictures made into CLIP ViT-L/14 vectors, the model PD12M's
//! vectors were made with, by ChloeDB's own embedder
//! (`chloedb::embed::ClipEmbedder`, model `clip-vit-large-patch14`). It
//! runs in the app, so that the servers it searches need no model: a text
//! in about a tenth of a second, a picture in about a second, on a CPU.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chloedb::embed::{Embedder, Image, Input};

pub const MODEL: &str = "clip-vit-large-patch14";

pub struct Clip {
    e: Arc<dyn Embedder>,
    /// Texts embedded lately, by their words (a search is asked again as
    /// its page is steered, filtered and paged).
    texts: Mutex<HashMap<String, Arc<Vec<f32>>>>,
    /// At most this many embeddings run at once: each takes a core or more.
    slots: Arc<tokio::sync::Semaphore>,
    /// The times of the last embeddings, for How it's built.
    pub times: Mutex<Times>,
}

#[derive(Default)]
pub struct Times {
    pub text: Vec<f64>,
    pub image: Vec<f64>,
}

impl Times {
    fn push(v: &mut Vec<f64>, ms: f64) {
        if v.len() >= 500 {
            v.remove(0);
        }
        v.push(ms);
    }
}

impl Clip {
    /// The model under `models` (`clip-vit-large-patch14/`, as
    /// `chloedb-server fetch-model clip-vit-large-patch14` puts it).
    pub fn open(models: &Path, slots: usize) -> Result<Clip, String> {
        let e = chloedb::embed::open(MODEL, models).map_err(|e| format!("{e} (fetch it with: chloedb-server fetch-model {MODEL} --models {})", models.display()))?;
        Ok(Clip { e, texts: Mutex::new(HashMap::new()), slots: Arc::new(tokio::sync::Semaphore::new(slots.max(1))), times: Mutex::new(Times::default()) })
    }

    pub fn name(&self) -> String {
        self.e.name()
    }

    /// The vector of a text, unit length (blocking).
    pub fn text_now(&self, text: &str) -> Result<Vec<f32>, String> {
        self.e.embed_query(text).map_err(|e| e.to_string())
    }

    /// The vectors of several texts (blocking): the kinds' and the themes'
    /// names when the rows are prepared.
    pub fn texts_now(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        self.e.embed(texts).map_err(|e| e.to_string())
    }

    /// The vector of a text, unit length, kept for the next time.
    pub async fn text(self: &Arc<Self>, text: &str) -> Result<Arc<Vec<f32>>, String> {
        let key = text.trim().to_lowercase();
        if let Some(v) = self.texts.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(v.clone());
        }
        let _slot = self.slots.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let me = self.clone();
        let words = key.clone();
        let t0 = Instant::now();
        let v = tokio::task::spawn_blocking(move || me.text_now(&words)).await.map_err(|e| e.to_string())??;
        Times::push(&mut self.times.lock().unwrap_or_else(|e| e.into_inner()).text, t0.elapsed().as_secs_f64() * 1e3);
        let v = Arc::new(v);
        let mut m = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, v.clone());
        Ok(v)
    }

    /// The vector of a decoded picture, unit length.
    pub async fn image(self: &Arc<Self>, img: image::RgbImage) -> Result<Vec<f32>, String> {
        let _slot = self.slots.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let me = self.clone();
        let t0 = Instant::now();
        let v = tokio::task::spawn_blocking(move || {
            let (width, height) = img.dimensions();
            let i = Image { width, height, rgb: img.into_raw() };
            me.e.embed_inputs(&[Input::Image(&i)]).map_err(|e| e.to_string()).map(|mut v| v.pop().unwrap_or_default())
        })
        .await
        .map_err(|e| e.to_string())??;
        Times::push(&mut self.times.lock().unwrap_or_else(|e| e.into_inner()).image, t0.elapsed().as_secs_f64() * 1e3);
        Ok(v)
    }
}
