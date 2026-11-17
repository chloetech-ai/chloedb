//! CLIP ViT-L/14 (feature `vision`, model `clip-vit-large-patch14`): the
//! vectors ChloeDB gives words and a picture are those of the reference
//! implementation (transformers' CLIPModel and CLIPProcessor, in
//! tests/fixtures/clip/reference.json), so that a query meets vectors made
//! elsewhere, such as PD12M's and LAION's. The model is 1.7 GB: the test
//! runs where it was fetched (`chloedb-server fetch-model
//! clip-vit-large-patch14`, or a folder named by `CHLOEDB_CLIP_DIR`), and
//! says it skipped otherwise.
#![cfg(feature = "vision")]

use std::path::PathBuf;

use chloedb::embed::embed_file;

fn model_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("CHLOEDB_CLIP_DIR").map(PathBuf::from).unwrap_or_else(|| chloedb::embed::models_dir().join("clip-vit-large-patch14"));
    match ["config.json", "tokenizer.json", "model.safetensors"].iter().all(|f| dir.join(f).exists()) {
        true => Some(dir),
        false => {
            eprintln!("skipped: no CLIP model at {} (set CHLOEDB_CLIP_DIR)", dir.display());
            None
        }
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip")
}

fn reference() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join("reference.json")).unwrap()).unwrap()
}

fn vector(v: &serde_json::Value) -> Vec<f32> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let n = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (n(a) * n(b))
}

#[test]
fn clip_gives_words_and_a_picture_the_reference_vectors() {
    let Some(dir) = model_dir() else { return };
    let models = dir.parent().unwrap().to_path_buf();
    let e = chloedb::embed::open(dir.file_name().unwrap().to_str().unwrap(), &models).unwrap();
    assert_eq!(e.dim(), 768);
    assert!(e.name().starts_with("clip:"), "{}", e.name());
    let want = reference();
    for (text, v) in want["texts"].as_object().unwrap() {
        let got = e.embed_query(text).unwrap();
        let unit = got.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((unit - 1.0).abs() < 1e-4, "unit length: {unit}");
        let c = cosine(&got, &vector(v));
        // The same weights and tokens: equal but for float order.
        assert!(c > 0.9999, "{text:?}: cosine {c} with the reference");
    }
    for (file, v) in want["images"].as_object().unwrap() {
        let data = std::fs::read(fixtures().join(file)).unwrap();
        let got = embed_file(e.as_ref(), &data).unwrap();
        let c = cosine(&got, &vector(v));
        // Bicubic resampling differs a little from Pillow's.
        assert!(c > 0.995, "{file}: cosine {c} with the reference");
    }
    // Words find the picture they describe over words that do not.
    let picture = embed_file(e.as_ref(), &std::fs::read(fixtures().join("portrait.jpg")).unwrap()).unwrap();
    let near = cosine(&picture, &e.embed_query("a drawing of a man in a hat").unwrap());
    let far = cosine(&picture, &e.embed_query("a photo of a red sports car").unwrap());
    assert!(near > far + 0.05, "near {near}, far {far}");
}

#[test]
fn clip_is_known_by_name_and_listed() {
    assert_eq!(chloedb::embed::known_dim("clip-vit-large-patch14"), Some(768));
    let listed = chloedb::embed::catalog(&std::env::temp_dir().join("no-models-here"));
    let clip = listed.iter().find(|m| m.name == "clip-vit-large-patch14").expect("listed");
    assert_eq!(clip.dim, Some(768));
    assert!(chloedb::model_pins::pin("openai/clip-vit-large-patch14").is_some_and(|p| p.sha256("model.safetensors").is_some()));
}
