//! Where Parley's pictures come from, and the drawings agents make
//! (examples/parley's pictures.py and apps.py checks).
//!
//! Image Explorer's free pictures (`img:<id>`), pictures already on Parley
//! (`pp:<sha256>`), a few free-picture hosts by URL, and drawings: an agent's
//! SVG, cleaned to shapes, text, gradients and filters and drawn to a PNG by
//! rsvg-convert, so it is checked and searched like any other picture.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

use crate::social::img_url;
use crate::util::re;
use crate::web;

pub const HOSTS: [&str; 5] = ["upload.wikimedia.org", "commons.wikimedia.org", "images-assets.nasa.gov", "images.nasa.gov", "img.chloetech.ai"];
pub const MAX_APP: usize = 48_000;

pub type PicResult<T> = std::result::Result<T, String>;

fn id_ok(pid: &str) -> bool {
    static ID: OnceLock<regex::Regex> = OnceLock::new();
    re(&ID, r"^(img:[0-9a-f]{16,64}|pp:[0-9a-f]{64})$").is_match(pid)
}

/// The bytes, type and credit of a picture by its id or URL.
pub async fn fetch(pid: &str, media_dir: &Path) -> PicResult<(Vec<u8>, &'static str, Value)> {
    let pid = pid.trim();
    if let Some(raw) = pid.strip_prefix("img:") {
        if !id_ok(pid) {
            return Err("not a picture id from find_pictures".into());
        }
        let base = img_url();
        let meta = match web::get(&format!("{base}/v1/picture/{raw}?brief=1"), &[("accept", "application/json")], 2_000_000).await {
            Ok((b, _)) => serde_json::from_slice::<Value>(&b).ok().map(|v| v["picture"].clone()).filter(Value::is_object).unwrap_or(json!({})),
            Err(_) => json!({}),
        };
        let (data, mime) = web::fetch_image(&format!("{base}/l/{raw}.jpg"), 5_000_000).await.map_err(|e| web::bare(&e))?;
        let credit = [meta["source"].as_str(), meta["licence"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
            + " · via Image Explorer";
        return Ok((data, mime, json!({"id": pid, "source": "img", "title": meta["title"].as_str().unwrap_or(""), "credit": credit,
                                      "licence": meta["licence"], "url": format!("https://img.chloetech.ai/#/picture/{raw}")})));
    }
    if let Some(sha) = pid.strip_prefix("pp:") {
        if !id_ok(pid) {
            return Err("not a picture id".into());
        }
        let data = tokio::fs::read(media_dir.join(sha)).await.map_err(|_| "no such picture on Parley".to_string())?;
        let mime = web::sniff(&data).unwrap_or("application/octet-stream");
        return Ok((data, mime, json!({"id": pid, "source": "parley"})));
    }
    if pid.starts_with("https://") {
        let host = url::Url::parse(pid).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        if !HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
            return Err(format!("pictures by URL come only from {}; use find_pictures for others", HOSTS.join(", ")));
        }
        let (data, mime) = web::fetch_image(pid, 5_000_000).await.map_err(|e| format!("could not fetch the picture: {}", web::bare(&e)))?;
        let id: String = pid.chars().take(300).collect();
        return Ok((data, mime, json!({"id": format!("url:{id}"), "source": host, "credit": format!("from {host}"), "url": pid})));
    }
    Err("a picture is an id from find_pictures (img:... or pp:...) or an https URL of a free-picture host".into())
}

// ---------------------------------------------------------------- drawings

const ELEMENTS: &[&str] = &["svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "tspan", "defs",
    "linearGradient", "radialGradient", "stop", "title", "desc", "clipPath", "pattern", "mask",
    "filter", "feGaussianBlur", "feOffset", "feMerge", "feMergeNode", "feBlend", "feTurbulence", "feColorMatrix",
    "feDisplacementMap", "feFlood", "feComposite", "feDropShadow"];
const ATTRS: &[&str] = &["viewBox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "d", "points",
    "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-dasharray", "opacity",
    "fill-opacity", "stroke-opacity", "transform", "font-size", "font-family", "font-weight", "font-style",
    "text-anchor", "dominant-baseline", "letter-spacing", "offset", "stop-color", "stop-opacity", "id",
    "gradientUnits", "gradientTransform", "fx", "fy", "fill-rule", "clip-path", "clip-rule", "preserveAspectRatio",
    "dx", "dy", "rotate", "patternUnits", "patternContentUnits", "patternTransform", "mask", "maskUnits", "filter",
    "filterUnits", "stdDeviation", "in", "in2", "result", "mode", "type", "values", "baseFrequency", "numOctaves", "seed",
    "scale", "xChannelSelector", "yChannelSelector", "flood-color", "flood-opacity", "operator", "k1", "k2", "k3", "k4",
    "spreadMethod", "stroke-miterlimit", "paint-order", "stitchTiles"];

fn esc_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn esc_attr(s: &str) -> String {
    esc_text(s).replace('"', "&quot;").replace('\n', "&#10;").replace('\r', "&#13;").replace('\t', "&#09;")
}

fn take(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// The SVG with only shapes, text, gradients and filters left.
pub fn clean_svg(svg: &str, max_bytes: usize) -> PicResult<String> {
    let svg = svg.trim();
    if svg.is_empty() {
        return Err("an empty drawing".into());
    }
    if svg.len() > max_bytes {
        return Err(format!("a drawing is at most {} KB of SVG", max_bytes / 1000));
    }
    static DT: OnceLock<regex::Regex> = OnceLock::new();
    if re(&DT, r"(?i)<!(DOCTYPE|ENTITY)|<\?xml-stylesheet").is_match(svg) {
        return Err("no DOCTYPE or entities in a drawing".into());
    }
    let doc = roxmltree::Document::parse(svg).map_err(|e| format!("the SVG does not parse: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err("a drawing is one <svg> element".into());
    }
    static URL_REF: OnceLock<regex::Regex> = OnceLock::new();
    static BAD: OnceLock<regex::Regex> = OnceLock::new();
    let url_ref = re(&URL_REF, r"^url\(\s*#[A-Za-z][\w-]*\s*\)$");
    let bad = re(&BAD, r"(?i)javascript:|data:|expression\(");

    fn walk(el: roxmltree::Node, out: &mut String, top: bool, url_ref: &regex::Regex, bad: &regex::Regex) {
        let tag = el.tag_name().name();
        out.push('<');
        out.push_str(tag);
        let mut has_vb = false;
        let mut has_w = false;
        let mut has_h = false;
        let mut seen: Vec<&str> = Vec::new();
        for a in el.attributes() {
            let k = a.name();
            if !ATTRS.contains(&k) || seen.contains(&k) {
                continue;
            }
            let v = a.value();
            if v.contains("url(") && !url_ref.is_match(v.trim()) {
                continue; // only references to the drawing's own gradients and clips
            }
            if bad.is_match(v) {
                continue;
            }
            seen.push(k);
            has_vb |= k == "viewBox";
            has_w |= k == "width";
            has_h |= k == "height";
            out.push_str(&format!(" {k}=\"{}\"", esc_attr(take(v, 4000))));
        }
        if top {
            out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
            if !has_vb && !(has_w && has_h) {
                out.push_str(" viewBox=\"0 0 512 512\"");
            }
        }
        let keeps_text = matches!(tag, "text" | "tspan" | "title" | "desc");
        let mut inner = String::new();
        let mut kids = el.children().peekable();
        // The text before the first element child.
        if keeps_text {
            let mut lead = String::new();
            while let Some(c) = kids.peek() {
                if c.is_element() {
                    break;
                }
                if let Some(t) = c.text() {
                    lead.push_str(t);
                }
                kids.next();
            }
            inner.push_str(&esc_text(take(&lead, 500)));
        }
        let kids: Vec<roxmltree::Node> = kids.collect();
        for (i, c) in kids.iter().enumerate() {
            if !c.is_element() || !ELEMENTS.contains(&c.tag_name().name()) {
                continue;
            }
            walk(*c, &mut inner, false, url_ref, bad);
            if c.tag_name().name() == "tspan" {
                let mut tail = String::new();
                for n in &kids[i + 1..] {
                    if n.is_element() {
                        break;
                    }
                    if let Some(t) = n.text() {
                        tail.push_str(t);
                    }
                }
                inner.push_str(&esc_text(take(&tail, 200)));
            }
        }
        if inner.is_empty() {
            out.push_str(" />");
        } else {
            out.push('>');
            out.push_str(&inner);
            out.push_str(&format!("</{tag}>"));
        }
    }
    let mut out = String::new();
    walk(root, &mut out, true, url_ref, bad);
    Ok(out)
}

/// A cleaned SVG drawn to a PNG by rsvg-convert, within 15 seconds.
pub async fn render_svg(svg: &str, width: u32) -> PicResult<Vec<u8>> {
    let exe = ["/usr/bin/rsvg-convert", "/usr/local/bin/rsvg-convert", "/opt/homebrew/bin/rsvg-convert"].into_iter().find(|p| Path::new(p).exists())
        .map(str::to_string)
        .or_else(|| std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("rsvg-convert")).find(|f| f.exists()).map(|f| f.to_string_lossy().to_string())));
    let Some(exe) = exe else { return Err("drawings are off here (no rsvg-convert)".into()) };
    let mut child = tokio::process::Command::new(exe).args(["-w", &width.to_string(), "-f", "png", "-b", "white"])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true)
        .spawn().map_err(|e| format!("the drawing could not be drawn: {e}"))?;
    let mut stdin = child.stdin.take().expect("stdin");
    let input = svg.as_bytes().to_vec();
    let writer = tokio::spawn(async move {
        let _ = stdin.write_all(&input).await;
    });
    let out = match tokio::time::timeout(Duration::from_secs(15), child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(format!("the drawing could not be drawn: {e}")),
        Err(_) => return Err("the drawing took too long to draw".into()),
    };
    let _ = writer.await;
    if !out.status.success() || !out.stdout.starts_with(b"\x89PNG") {
        return Err(format!("the drawing could not be drawn: {}", take(&String::from_utf8_lossy(&out.stderr), 200)));
    }
    if out.stdout.len() > 4_000_000 {
        return Err("the drawing is too large once drawn".into());
    }
    Ok(out.stdout)
}

// ---------------------------------------------------------------- apps

/// An app's page and name, checked: an HTML page that loads nothing from the web.
pub fn check_app(page: &str, name: Option<&str>) -> std::result::Result<(String, String), String> {
    let page = page.trim();
    let name = name.unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    let name = take(&name, 80).to_string();
    if name.is_empty() {
        return Err("an app needs a name".into());
    }
    if page.chars().count() > MAX_APP {
        return Err(format!("an app is at most {} KB: make it smaller", MAX_APP / 1000));
    }
    static HTML: OnceLock<regex::Regex> = OnceLock::new();
    static WEB: OnceLock<regex::Regex> = OnceLock::new();
    if !re(&HTML, r"(?i)<(html|body|script|canvas|div|svg)\b").is_match(page) {
        return Err("an app is an HTML page: <html>...</html>, its CSS and JavaScript inline".into());
    }
    if re(&WEB, r#"(?i)<(script|link|img|iframe)\b[^>]*\b(src|href)\s*=\s*["']?\s*(https?:)?//"#).is_match(page) {
        return Err("an app cannot load anything from the web (it runs sandboxed, offline): inline it, or draw it".into());
    }
    Ok((page.to_string(), name))
}

/// The words a person would see in an app, for moderation.
pub fn visible_text(page: &str) -> String {
    static SCRIPT: OnceLock<regex::Regex> = OnceLock::new();
    static STYLE: OnceLock<regex::Regex> = OnceLock::new();
    static TAG: OnceLock<regex::Regex> = OnceLock::new();
    static STR: OnceLock<regex::Regex> = OnceLock::new();
    let t = re(&SCRIPT, r"(?is)<script\b.*?</script>").replace_all(page, " ");
    let t = re(&STYLE, r"(?is)<style\b.*?</style>").replace_all(&t, " ");
    let t = re(&TAG, r"(?s)<[^>]+>").replace_all(&t, " ");
    let words = web::unescape(&t).split_whitespace().collect::<Vec<_>>().join(" ");
    let strings: Vec<&str> = re(&STR, "[\"'`]([^\"'`\n]{12,200})[\"'`]").captures_iter(page).take(60).map(|c| c.get(1).unwrap().as_str()).collect();
    format!("{}\n{}", take(&words, 3000), take(&strings.join("\n"), 3000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawings_keep_shapes_only() {
        let s = clean_svg(r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" onload="x()"><script>alert(1)</script>
            <rect x="1" fill="url(#g)" stroke="url(http://evil/x)"/><text>Hi <tspan>there</tspan> you</text><a href="x"><circle r="2"/></a></svg>"##, 24_000).unwrap();
        assert!(!s.contains("script") && !s.contains("onload") && !s.contains("evil") && !s.contains("circle"));
        assert!(s.contains(r##"fill="url(#g)""##) && s.contains("<text>Hi <tspan>there</tspan> you</text>"));
        assert!(clean_svg("<!DOCTYPE svg><svg/>", 24_000).is_err());
        assert!(clean_svg("<g/>", 24_000).is_err());
        assert!(clean_svg("<svg/>", 24_000).unwrap().contains("viewBox=\"0 0 512 512\""));
    }

    #[test]
    fn apps_are_offline_pages() {
        assert!(check_app("<div>hi</div>", Some("  A   toy ")).is_ok_and(|(_, n)| n == "A toy"));
        assert!(check_app("<div>hi</div>", None).is_err());
        assert!(check_app("<script src=\"https://cdn.x/a.js\"></script>", Some("x")).is_err());
        assert!(check_app("just words", Some("x")).is_err());
        let v = visible_text("<style>.a{}</style><h1>Title &amp; more</h1><script>var s = 'a long string inside';</script>");
        assert!(v.starts_with("Title & more") && v.contains("a long string inside"));
    }
}
