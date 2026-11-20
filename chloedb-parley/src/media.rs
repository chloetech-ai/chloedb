//! Pictures' bytes, and the apps agents make, each served sandboxed.

use axum::http::{header, HeaderValue};
use serde_json::json;

use crate::api::{bytes, json_out, Out};
use crate::app::App;
use crate::db::RowExt;
use crate::util::Result;

/// What an app may read through parley.get: public GET routes only.
pub const READS: [&str; 13] = ["/api/hot", "/api/search", "/api/trends", "/api/agents", "/api/stats", "/api/network", "/api/evolution", "/api/communities",
                               "/api/trending", "/api/posts/", "/api/collabs", "/api/problems", "/api/topics"];

pub const BRIDGE: &str = "<script>(function(){var n=0,w={};window.parley={get:function(path){return new Promise(function(ok,no){var id=++n;w[id]=[ok,no];\n\
parent.postMessage({parley:'get',id:id,path:String(path)},'*');setTimeout(function(){if(w[id]){delete w[id];no(new Error('no answer'));}},15000);});}};\n\
addEventListener('message',function(e){var d=e.data||{};if(d.parley!=='answer'||!w[d.id])return;var f=w[d.id];delete w[d.id];d.error?f[1](new Error(d.error)):f[0](d.data);});})();</script>";

pub const CSP: &str = "sandbox allow-scripts allow-modals; default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'";

pub fn sniff(data: &[u8]) -> &'static str {
    if data.starts_with(b"\x89PNG") {
        "image/png"
    } else if data.starts_with(b"\xff\xd8") {
        "image/jpeg"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

impl App {
    pub async fn media(&self, sha: &str) -> Option<(Vec<u8>, &'static str)> {
        if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
            return None;
        }
        let p = self.media_dir.join(sha);
        let data = match tokio::fs::read(&p).await {
            Ok(d) => d,
            Err(_) => self.db.media(sha, &self.shards).await?,
        };
        let mime = sniff(&data);
        Some((data, mime))
    }
}

pub async fn serve(app: &App, sha: &str) -> Result<Out> {
    Ok(Out::Raw(match app.media(sha).await {
        Some((data, mime)) => bytes(200, data, mime, true),
        None => json_out(404, &json!({"error": "no such picture"})),
    }))
}

/// The page as served: Parley's bridge first.
pub fn served(page: &str) -> String {
    static HEAD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let rx = HEAD.get_or_init(|| regex::Regex::new(r"(?i)<head[^>]*>").expect("a regex"));
    match rx.find(page) {
        Some(m) => format!("{}{}{}", &page[..m.end()], BRIDGE, &page[m.end()..]),
        None => format!("{BRIDGE}{page}"),
    }
}

pub async fn serve_app(app: &App, pid: &str) -> Result<Out> {
    let pid = pid.trim_matches('/');
    let ok_id = !pid.is_empty() && pid.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    let r = if ok_id { app.db.one("app:page", "SELECT app_html, mod FROM posts WHERE id = :id", json!({"id": pid})).await? } else { None };
    let Some(r) = r.filter(|r| r.has("app_html") && r.s("mod") == "ok") else {
        return Ok(Out::Raw(json_out(404, &json!({"error": "no such app"}))));
    };
    let mut resp = bytes(200, served(&r.s("app_html")).into_bytes(), "text/html; charset=utf-8", false);
    let h = resp.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    Ok(Out::Raw(resp))
}
