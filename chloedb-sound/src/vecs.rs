//! Vectors: what the server's CLAP makes of a text or a clip (asked once
//! and kept), the labels' vectors, and the arithmetic of steering and
//! drifting between sounds.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::strata::{Error, Result, RowExt, Strata, Trace};

/// A label the site listens for, with its vector.
#[derive(Clone, Debug)]
pub struct Label {
    pub id: String,
    pub name: String,
    pub grp: String,
    pub v: Vec<f32>,
}

/// Texts' vectors, kept (a text is embedded once), and the labels.
pub struct Embedder {
    db: Strata,
    texts: Mutex<HashMap<String, Arc<Vec<f32>>>>,
    labels: Mutex<Option<Arc<Vec<Label>>>>,
}

impl Embedder {
    pub fn new(db: Strata) -> Embedder {
        Embedder { db, texts: Mutex::new(HashMap::new()), labels: Mutex::new(None) }
    }

    async fn embed(&self, what: Value) -> Result<Vec<f32>> {
        let mut body = json!({"table": "au_labels", "field": "emb"});
        for (k, v) in what.as_object().into_iter().flatten() {
            body[k] = v.clone();
        }
        let j = self.db.post("/v1/embed", &body).await?;
        let v = floats(&j["vector"]);
        if v.is_empty() {
            return Err(Error { status: 0, message: "the server gave no vector".into() });
        }
        Ok(v)
    }

    /// A text's vector by CLAP's text tower, kept for next time.
    pub async fn text(&self, t: &str) -> Result<Arc<Vec<f32>>> {
        let key = t.trim().to_lowercase();
        if let Some(v) = self.texts.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(v.clone());
        }
        let v = Arc::new(self.embed(json!({"text": key})).await?);
        let mut m = self.texts.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, v.clone());
        Ok(v)
    }

    /// A sound file's vector by CLAP's audio tower (base64 of a WAV, FLAC,
    /// MP3 or Ogg file): the mean of its ten-second windows.
    pub async fn audio(&self, b64: &str) -> Result<Vec<f32>> {
        self.embed(json!({"audio": b64})).await
    }

    /// The labels with their vectors, read once.
    pub async fn labels(&self, trace: Option<&Trace>) -> Result<Arc<Vec<Label>>> {
        if let Some(l) = self.labels.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(l);
        }
        let sql = "SELECT id, name, grp, emb FROM au_labels LIMIT 1000";
        let rows = match trace {
            Some(t) => self.db.traced(t, "the labels, once", sql, Value::Null).await?,
            None => self.db.query(sql, Value::Null).await?,
        };
        let labels: Vec<Label> = rows.iter().map(|r| Label { id: r.s("id"), name: r.s("name"), grp: r.s("grp"), v: floats(r.get("emb").unwrap_or(&Value::Null)) }).filter(|l| l.v.len() == crate::schema::DIM).collect();
        let labels = Arc::new(labels);
        if !labels.is_empty() {
            *self.labels.lock().unwrap_or_else(|e| e.into_inner()) = Some(labels.clone());
        }
        Ok(labels)
    }
}

pub fn floats(v: &Value) -> Vec<f32> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default()
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn norm(v: &mut [f32]) {
    let n = dot(v, v).sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

/// `a + w b`, in place.
pub fn add(a: &mut [f32], b: &[f32], w: f32) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += w * y;
    }
}

/// The point a fraction `f` of the way from `a` to `b` on the sphere.
pub fn slerp(a: &[f32], b: &[f32], f: f32) -> Vec<f32> {
    let c = dot(a, b).clamp(-1.0, 1.0);
    let th = c.acos();
    let mut out: Vec<f32> = if th.abs() < 1e-4 {
        a.iter().zip(b).map(|(x, y)| x + f * (y - x)).collect()
    } else {
        let (wa, wb) = (((1.0 - f) * th).sin() / th.sin(), (f * th).sin() / th.sin());
        a.iter().zip(b).map(|(x, y)| wa * x + wb * y).collect()
    };
    norm(&mut out);
    out
}

/// CLAP's own scale of its similarities into odds (its logit scale, about
/// 100 after training): the labels' chances for one moment, summing to 1.
pub fn softmax(scores: &[f32], scale: f32) -> Vec<f32> {
    let m = scores.iter().cloned().fold(f32::MIN, f32::max);
    let e: Vec<f32> = scores.iter().map(|s| ((s - m) * scale).exp()).collect();
    let z: f32 = e.iter().sum();
    e.into_iter().map(|x| x / z.max(1e-12)).collect()
}

/// A vector as a parameter of a query.
pub fn param(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|x| json!(((*x as f64) * 1e6).round() / 1e6)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slerp_ends_and_middle() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let m = slerp(&a, &b, 0.5);
        assert!((m[0] - m[1]).abs() < 1e-6 && (dot(&m, &m) - 1.0).abs() < 1e-5);
        assert!((slerp(&a, &b, 0.0)[0] - 1.0).abs() < 1e-6);
        assert!((slerp(&a, &b, 1.0)[1] - 1.0).abs() < 1e-6);
        // The same point: no division by zero.
        assert!((slerp(&a, &a, 0.3)[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn softmax_sums_to_one() {
        let p = softmax(&[0.3, 0.2, 0.1], 100.0);
        assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(p[0] > 0.99);
    }
}
