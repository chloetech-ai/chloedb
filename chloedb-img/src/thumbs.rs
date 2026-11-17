//! Thumbnails, made from the originals in PD12M's public bucket the first
//! time a picture is shown, and kept on disk: one 400 pixels high for the
//! grids and one up to 1,600 pixels for the picture's page. The same pass
//! makes the picture's colors (`img_colors`) and, for a picture from
//! Wikimedia Commons, finds its page there by the file's SHA-1
//! (`img_links`), which PD12M's records do not give.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::colors;
use crate::strata::Strata;

/// The heights and widths thumbnails are made at.
pub const SMALL_H: u32 = 400;
pub const SMALL_MAX_W: u32 = 1200;
pub const LARGE: u32 = 1600;
/// Originals larger than this are not fetched (a few scans of maps and
/// manuscripts run to hundreds of megapixels).
pub const MAX_MP: f64 = 150.0;
/// Megapixels decoded at once: a decoded picture takes about 7 bytes a
/// pixel until its thumbnails are made, so a few large scans at once would
/// take gigabytes; one of up to `MAX_MP` waits for room.
pub const DECODE_MP: u32 = 200;
pub const MAX_BYTES: usize = 120 << 20;

/// Where a picture's file is and what to label its colors with.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub url: String,
    pub kind: String,
    pub source: String,
    pub mp: f64,
}

pub struct Thumbs {
    dir: PathBuf,
    http: reqwest::Client,
    db: Strata,
    /// The pictures being made now, so that two requests for one wait for
    /// the same download.
    making: Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
    /// Downloads at once.
    slots: Arc<tokio::sync::Semaphore>,
    /// Megapixels being decoded ([`DECODE_MP`]).
    decode: Arc<tokio::sync::Semaphore>,
    /// Whether to look pictures from Commons up there.
    pub commons: bool,
    /// When Commons was last asked.
    commons_last: tokio::sync::Mutex<std::time::Instant>,
    pub made: std::sync::atomic::AtomicU64,
    pub failed: std::sync::atomic::AtomicU64,
}

/// A thumbnail's size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Small,
    Large,
}

impl Size {
    fn dir(self) -> &'static str {
        match self {
            Size::Small => "t",
            Size::Large => "l",
        }
    }
}

/// Whether an id is one PD12M gives (hex), and so safe in a path.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Thumbs {
    pub fn new(dir: &Path, db: Strata, slots: usize, commons: bool) -> Thumbs {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("ImageExplorer/0.3 (https://img.chloetech.ai; built on ChloeDB)")
            .build()
            .expect("an HTTP client");
        Thumbs { dir: dir.to_path_buf(), http, db, making: Mutex::new(HashMap::new()), slots: Arc::new(tokio::sync::Semaphore::new(slots.max(1))), decode: Arc::new(tokio::sync::Semaphore::new(DECODE_MP as usize)), commons, commons_last: tokio::sync::Mutex::new(std::time::Instant::now()), made: 0.into(), failed: 0.into() }
    }

    pub fn path(&self, id: &str, size: Size) -> PathBuf {
        let shard = &id[..2.min(id.len())];
        self.dir.join(size.dir()).join(shard).join(format!("{id}.jpg"))
    }

    fn failed_path(&self, id: &str) -> PathBuf {
        let shard = &id[..2.min(id.len())];
        self.dir.join("x").join(shard).join(id)
    }

    pub fn have(&self, id: &str, size: Size) -> bool {
        self.path(id, size).exists()
    }

    /// The thumbnail's bytes, made first when there is none: `None` when
    /// the original could not be fetched or read (remembered, so that it
    /// is not fetched again).
    pub async fn get(self: &Arc<Self>, id: &str, size: Size, src: &Source) -> Option<Vec<u8>> {
        if !valid_id(id) {
            return None;
        }
        loop {
            if let Ok(b) = tokio::fs::read(self.path(id, size)).await {
                return Some(b);
            }
            if self.failed_path(id).exists() {
                return None;
            }
            // One download a picture: the others wait for it.
            let (wait, mine) = {
                let mut m = self.making.lock().unwrap_or_else(|e| e.into_inner());
                match m.get(id) {
                    Some(n) => (n.clone(), false),
                    None => {
                        let n = Arc::new(tokio::sync::Notify::new());
                        m.insert(id.to_string(), n.clone());
                        (n, true)
                    }
                }
            };
            if !mine {
                let notified = wait.notified();
                if !self.making.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id) {
                    continue;
                }
                let _ = tokio::time::timeout(Duration::from_secs(90), notified).await;
                continue;
            }
            let r = self.make(id, src, size).await;
            self.making.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
            wait.notify_waiters();
            if let Err(e) = r {
                tracing::warn!("thumbnail of {id}: {e}");
                self.failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let p = self.failed_path(id);
                let _ = std::fs::create_dir_all(p.parent().expect("a folder"));
                let _ = std::fs::write(p, e.as_bytes());
                return None;
            }
            self.made.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Fetches the original, writes the small thumbnail (and the large one
    /// when that is the one asked for: most pictures are only ever seen in
    /// a grid), and keeps its colors (and its page on Commons).
    async fn make(self: &Arc<Self>, id: &str, src: &Source, size: Size) -> Result<(), String> {
        let first = !self.have(id, Size::Small);
        if src.url.is_empty() {
            return Err("no file".into());
        }
        if !src.url.starts_with("https://pd12m.s3.us-west-2.amazonaws.com/") {
            return Err(format!("not a PD12M file: {}", src.url));
        }
        if src.mp > MAX_MP {
            return Err(format!("{:.0} megapixels: too large to fetch", src.mp));
        }
        let _slot = self.slots.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let r = self.http.get(&src.url).send().await.map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("HTTP {}", r.status()));
        }
        let bytes = r.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err(format!("{} MB", bytes.len() >> 20));
        }
        let sha1 = sha1_smol::Sha1::from(&bytes[..]).digest().to_string();
        let need = (src.mp.ceil() as u32).clamp(1, DECODE_MP);
        let _room = self.decode.clone().acquire_many_owned(need).await.map_err(|e| e.to_string())?;
        let (small, large, colors) = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgb8();
            let (w, h) = img.dimensions();
            let sh = SMALL_H.min(h);
            let mut sw = ((w as f64) * sh as f64 / h as f64).round().max(1.0) as u32;
            let mut sh2 = sh;
            if sw > SMALL_MAX_W {
                sw = SMALL_MAX_W;
                sh2 = ((h as f64) * sw as f64 / w as f64).round().max(1.0) as u32;
            }
            let small = image::imageops::resize(&img, sw, sh2, image::imageops::FilterType::Lanczos3);
            let scale = (LARGE as f64 / w.max(h) as f64).min(1.0);
            let large = match (size, scale < 1.0) {
                (Size::Small, _) => None,
                (Size::Large, true) => Some(image::imageops::resize(&img, ((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1), image::imageops::FilterType::Lanczos3)),
                (Size::Large, false) => Some(img),
            };
            let jpeg = |i: &image::RgbImage, q: u8| -> Result<Vec<u8>, String> {
                let mut out = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q).encode_image(i).map_err(|e| e.to_string())?;
                Ok(out)
            };
            let colors = colors::of_image(&small);
            let large = match large {
                Some(l) => Some(jpeg(&l, 84)?),
                None => None,
            };
            Ok((jpeg(&small, 82)?, large, colors))
        })
        .await
        .map_err(|e| e.to_string())??;
        for (size, b) in [(Size::Small, Some(&small)), (Size::Large, large.as_ref())] {
            let Some(b) = b else { continue };
            let p = self.path(id, size);
            tokio::fs::create_dir_all(p.parent().expect("a folder")).await.map_err(|e| e.to_string())?;
            let tmp = p.with_extension("part");
            tokio::fs::write(&tmp, b).await.map_err(|e| e.to_string())?;
            tokio::fs::rename(&tmp, &p).await.map_err(|e| e.to_string())?;
        }
        if !first {
            return Ok(());
        }
        let palette: Vec<_> = colors.palette.iter().map(|(c, s)| json!([colors::hex(*c), (s * 1000.0).round() / 1000.0])).collect();
        let doc = json!({"id": id, "kind": src.kind, "source": src.source, "palette": serde_json::to_string(&palette).unwrap_or_default(), "colors": colors.vector});
        if let Err(e) = self.db.upsert("img_colors", vec![doc]).await {
            tracing::warn!("colors of {id}: {e}");
        }
        if self.commons && src.source == "Wikimedia Commons" {
            let me = self.clone();
            let id = id.to_string();
            tokio::spawn(async move {
                if let Err(e) = me.commons_page(&id, &sha1).await {
                    tracing::warn!("Commons page of {id}: {e}");
                }
            });
        }
        Ok(())
    }

    /// The picture's page on Wikimedia Commons, found by its file's SHA-1:
    /// one question at a time, a second apart, as Wikimedia asks of
    /// programs, and asked again later when it says to slow down.
    async fn commons_page(&self, id: &str, sha1: &str) -> Result<(), String> {
        let url = format!("https://commons.wikimedia.org/w/api.php?action=query&list=allimages&aisha1={sha1}&ailimit=1&format=json&formatversion=2&maxlag=5");
        let mut tries = 0;
        let j: serde_json::Value = loop {
            let r = {
                let mut last = self.commons_last.lock().await;
                let wait = Duration::from_millis(1000).saturating_sub(last.elapsed());
                tokio::time::sleep(wait).await;
                let r = self.http.get(&url).send().await.map_err(|e| e.to_string());
                *last = std::time::Instant::now();
                r?
            };
            if r.status().as_u16() == 429 || r.status().is_server_error() {
                tries += 1;
                if tries > 4 {
                    return Err(format!("HTTP {} from Commons", r.status()));
                }
                tokio::time::sleep(Duration::from_secs(5 * tries)).await;
                continue;
            }
            break r.json().await.map_err(|e| e.to_string())?;
        };
        if j.get("error").is_some() {
            return Err(format!("Commons: {}", j["error"]));
        }
        let found = j["query"]["allimages"].as_array().and_then(|a| a.first()).cloned();
        let doc = match found {
            Some(f) => {
                let title = f["title"].as_str().unwrap_or("").to_string();
                let page = f["descriptionurl"].as_str().map(str::to_string).unwrap_or_else(|| format!("https://commons.wikimedia.org/wiki/{}", title.replace(' ', "_")));
                json!({"id": id, "page": page, "title": title.trim_start_matches("File:"), "found": true})
            }
            None => json!({"id": id, "page": "", "title": "", "found": false}),
        };
        self.db.upsert("img_links", vec![doc]).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
