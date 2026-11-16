//! A paper's own text, figures and tables, from its HTML page on arxiv.org
//! (arXiv renders most papers since late 2023 to HTML with LaTeXML): the
//! sections up to the appendices, each figure with its caption and the
//! images it holds, and each table's caption and cells. A feed's issue
//! explains a paper from this rather than from its abstract alone, and
//! shows the figures that explain it best.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Image {
    /// Where arXiv serves it (an absolute URL).
    pub src: String,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Figure {
    /// The page's id of it (`S3.F2`).
    pub id: String,
    /// "Figure 2".
    pub label: String,
    pub caption: String,
    pub images: Vec<Image>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Table {
    pub id: String,
    pub label: String,
    pub caption: String,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Full {
    pub sections: Vec<Section>,
    pub figures: Vec<Figure>,
    pub tables: Vec<Table>,
}

/// Characters of a section the model reads at most, and of all of them.
const SECTION_CHARS: usize = 9_000;
const TOTAL_CHARS: usize = 45_000;

fn re(p: &str) -> regex::Regex {
    regex::Regex::new(p).expect("a valid pattern")
}

/// Text without its tags, entities decoded, spaces collapsed.
pub fn text_of(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut inside = false;
    for ch in s.chars() {
        match ch {
            '<' => inside = true,
            '>' => {
                inside = false;
                out.push(' ');
            }
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // Spaces the tags left before punctuation.
    out.replace(" ,", ",").replace(" .", ".").replace(" ;", ";").replace(" :", ":").replace("( ", "(").replace(" )", ")")
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n).collect();
    if let Some(i) = t.rfind(". ") {
        if i > n / 2 {
            t.truncate(i + 1);
        }
    }
    t.push_str(" …");
    t
}

/// An image's address on arxiv.org, from the page's `src` (relative to
/// `https://arxiv.org/html/<id>`, so `2501.12948v2/x1.png`).
fn absolute(src: &str) -> String {
    if src.starts_with("http://") || src.starts_with("https://") {
        src.to_string()
    } else if let Some(p) = src.strip_prefix('/') {
        format!("https://arxiv.org/{p}")
    } else {
        format!("https://arxiv.org/html/{src}")
    }
}

/// The attributes of one tag.
fn attrs(tag: &str) -> std::collections::HashMap<String, String> {
    let a = re(r#"([a-zA-Z-]+)="([^"]*)""#);
    a.captures_iter(tag).map(|c| (c[1].to_lowercase(), c[2].to_string())).collect()
}

/// The images a stretch of the page holds (raster images and SVG
/// objects), but icons.
fn images_in(html: &str) -> Vec<Image> {
    let tags = re(r#"(?s)<(?:img|object)\s[^>]*>"#);
    let mut out = Vec::new();
    for m in tags.find_iter(html) {
        let a = attrs(m.as_str());
        let Some(src) = a.get("src").or_else(|| a.get("data")) else { continue };
        if src.is_empty() || src.contains("static.arxiv.org") || src.starts_with("data:") {
            continue;
        }
        let w = a.get("width").and_then(|x| x.parse().ok()).unwrap_or(0);
        let h = a.get("height").and_then(|x| x.parse().ok()).unwrap_or(0);
        if (w > 0 && w < 60) || (h > 0 && h < 40) {
            continue;
        }
        let img = Image { src: absolute(src), w, h };
        if !out.contains(&img) {
            out.push(img);
        }
    }
    out
}

/// The caption of a figure or table: its label ("Figure 2") and its text.
fn caption(html: &str, kind: &str) -> Option<(String, String)> {
    let c = re(&format!(r#"(?s)<figcaption[^>]*>\s*<span class="ltx_tag ltx_tag_{kind}">(.*?)</span>(.*?)</figcaption>"#));
    let m = c.captures(html)?;
    let label = text_of(&m[1]).trim_end_matches(':').trim().to_string();
    Some((label, text_of(&m[2])))
}

/// Reads a LaTeXML page.
pub fn parse(html: &str) -> Full {
    if html.is_empty() {
        return Full::default();
    }
    // Math as its LaTeX, and no citations (they are names and years the
    // model does not need).
    let html = re(r#"(?s)<math[^>]*?alttext="([^"]*)"[^>]*>.*?</math>"#).replace_all(html, |c: &regex::Captures| format!(" ${}$ ", &c[1]));
    let html = re(r#"(?s)<cite\b[^>]*>.*?</cite>"#).replace_all(&html, "");
    let html = html.as_ref();

    // Where the body ends: the appendices or the references.
    let end = re(r#"<section id="(?:A|Ax|bib)[0-9]*" class="ltx_(?:appendix|bibliography)""#).find(html).map(|m| m.start()).unwrap_or(html.len());

    // Sections: each top-level one, its paragraphs.
    let paras = re(r#"(?s)<p\b[^>]*class="ltx_p"[^>]*>(.*?)</p>"#);
    let starts: Vec<usize> = re(r#"<section id="S\d+" class="ltx_section">"#).find_iter(&html[..end]).map(|m| m.start()).collect();
    let title = re(r#"(?s)<h2 class="ltx_title ltx_title_section">(.*?)</h2>"#);
    let mut sections = Vec::new();
    let mut total = 0;
    for (k, &s) in starts.iter().enumerate() {
        let e = starts.get(k + 1).copied().unwrap_or(end);
        let part = &html[s..e];
        let t = title.captures(part).map(|c| text_of(&c[1])).unwrap_or_default();
        let mut body = String::new();
        for p in paras.captures_iter(part) {
            let x = text_of(&p[1]);
            if !x.is_empty() {
                body.push_str(&x);
                body.push('\n');
            }
            if body.len() > SECTION_CHARS + 500 {
                break;
            }
        }
        let body = cut(body.trim(), SECTION_CHARS.min(TOTAL_CHARS.saturating_sub(total)));
        total += body.len();
        if !body.is_empty() || !t.is_empty() {
            sections.push(Section { title: t, text: body });
        }
        if total >= TOTAL_CHARS {
            break;
        }
    }
    if sections.is_empty() {
        // A page without numbered sections: its paragraphs as one.
        let mut body = String::new();
        for p in paras.captures_iter(&html[..end]) {
            body.push_str(&text_of(&p[1]));
            body.push('\n');
            if body.len() > TOTAL_CHARS {
                break;
            }
        }
        if !body.trim().is_empty() {
            sections.push(Section { title: "Text".into(), text: cut(body.trim(), TOTAL_CHARS) });
        }
    }

    // Figures and tables: the top-level ones (not a figure's panels), in
    // the body and the appendices alike.
    let blocks: Vec<(usize, String, String)> = re(r#"<figure id="([^"]+)" class="(ltx_figure|ltx_table)[^"]*">"#)
        .captures_iter(html)
        .filter(|c| {
            let id = &c[1];
            let last = id.rsplit('.').next().unwrap_or("");
            (last.starts_with('F') || last.starts_with('T')) && last[1..].chars().all(|ch| ch.is_ascii_digit()) && !last[1..].is_empty()
        })
        .map(|c| (c.get(0).map(|m| m.start()).unwrap_or(0), c[1].to_string(), c[2].to_string()))
        .collect();
    let mut figures = Vec::new();
    let mut tables = Vec::new();
    let tr = re(r#"(?s)<tr\b[^>]*>(.*?)</tr>"#);
    let td = re(r#"(?s)<t[dh]\b[^>]*>(.*?)</t[dh]>"#);
    for (k, (s, id, class)) in blocks.iter().enumerate() {
        let e = blocks.get(k + 1).map(|b| b.0).unwrap_or(html.len()).min(s + 400_000);
        let part = &html[*s..e];
        if class == "ltx_figure" {
            let Some((label, cap)) = caption(part, "figure") else { continue };
            // The figure's own images: those before its caption ends.
            let upto = part.find("</figcaption>").unwrap_or(part.len());
            let images: Vec<Image> = images_in(&part[..upto]).into_iter().take(4).collect();
            if !images.is_empty() {
                figures.push(Figure { id: id.clone(), label, caption: cut(&cap, 1200), images });
            }
        } else {
            let Some((label, cap)) = caption(part, "table") else { continue };
            let mut rows = Vec::new();
            for r in tr.captures_iter(part).take(24) {
                let cells: Vec<String> = td.captures_iter(&r[1]).take(12).map(|c| cut(&text_of(&c[1]), 80)).collect();
                if !cells.is_empty() && cells.iter().any(|c| !c.is_empty()) {
                    rows.push(cells);
                }
            }
            // A table of numbers: two rows and two columns at least (not
            // a prompt set in a box).
            if rows.len() >= 2 && rows.iter().any(|r| r.len() >= 2) {
                tables.push(Table { id: id.clone(), label, caption: cut(&cap, 600), rows });
            }
        }
    }
    Full { sections, figures, tables }
}

impl Full {
    /// What the model reads of the paper: its sections, then the figures'
    /// and tables' captions, each with the number it is picked by.
    pub fn context(&self) -> String {
        let mut s = String::new();
        for x in &self.sections {
            s.push_str(&format!("\n## {}\n{}\n", x.title, x.text));
        }
        if !self.figures.is_empty() {
            s.push_str("\n## Figures (pick by number)\n");
            for (i, f) in self.figures.iter().enumerate() {
                s.push_str(&format!("[F{}] {}: {}\n", i + 1, f.label, cut(&f.caption, 500)));
            }
        }
        if !self.tables.is_empty() {
            s.push_str("\n## Tables (pick by number)\n");
            for (i, t) in self.tables.iter().enumerate() {
                s.push_str(&format!("[T{}] {}: {}\n", i + 1, t.label, cut(&t.caption, 300)));
                for r in t.rows.iter().take(8) {
                    s.push_str(&format!("  | {} |\n", r.join(" | ")));
                }
            }
        }
        s
    }
}

/// Base64 (standard, padded) of bytes: how an image is kept in a row.
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if ch.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if ch.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// An image's media type, from what the server said or its name.
pub fn mime(content_type: &str, src: &str) -> Option<&'static str> {
    let ct = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let by_name = |s: &str| {
        let s = s.to_ascii_lowercase();
        if s.ends_with(".png") {
            Some("image/png")
        } else if s.ends_with(".jpg") || s.ends_with(".jpeg") {
            Some("image/jpeg")
        } else if s.ends_with(".gif") {
            Some("image/gif")
        } else if s.ends_with(".svg") {
            Some("image/svg+xml")
        } else if s.ends_with(".webp") {
            Some("image/webp")
        } else {
            None
        }
    };
    match ct.as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/svg+xml" => Some("image/svg+xml"),
        "image/webp" => Some("image/webp"),
        _ => by_name(src),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r##"<html><body><article>
<h1 class="ltx_title ltx_title_document">A Paper</h1>
<section id="S1" class="ltx_section">
<h2 class="ltx_title ltx_title_section"><span class="ltx_tag ltx_tag_section">1 </span>Introduction</h2>
<div class="ltx_para"><p id="S1.p1.1" class="ltx_p">We study <math alttext="x^{2}" display="inline"><mi>x</mi></math> loss <cite class="ltx_cite">(<a href="#bib.bib1">Smith, 2020</a>)</cite> in recommenders &amp; search.</p></div>
<figure id="S1.F1" class="ltx_figure"><img src="2401.00001v1/x1.png" id="S1.F1.g1" class="ltx_graphics" width="476" height="300" alt="Refer to caption">
<figcaption class="ltx_caption ltx_centering"><span class="ltx_tag ltx_tag_figure">Figure 1: </span>The model, end to end.</figcaption>
</figure>
</section>
<section id="S2" class="ltx_section">
<h2 class="ltx_title ltx_title_section"><span class="ltx_tag ltx_tag_section">2 </span>Results</h2>
<div class="ltx_para"><p class="ltx_p">It is 12% better than the baseline.</p></div>
<figure id="S2.F2" class="ltx_figure"><div class="ltx_flex_figure">
<figure id="S2.F2.fig1" class="ltx_figure ltx_figure_panel"><object type="image/svg+xml" data="2401.00001v1/a.svg" width="224" height="166"></object></figure>
<figure id="S2.F2.fig2" class="ltx_figure ltx_figure_panel"><img src="2401.00001v1/b.png" width="229" height="165"></figure></div>
<figcaption class="ltx_caption"><span class="ltx_tag ltx_tag_figure">Figure 2: </span>(a) Recall. (b) Length.</figcaption></figure>
<figure id="S2.T1" class="ltx_table"><figcaption class="ltx_caption"><span class="ltx_tag ltx_tag_table">Table 1: </span>Main results.</figcaption>
<table class="ltx_tabular"><tr class="ltx_tr"><th class="ltx_th">Model</th><th class="ltx_th">NDCG</th></tr>
<tr class="ltx_tr"><td class="ltx_td">Ours</td><td class="ltx_td"><span class="ltx_text ltx_font_bold">0.42</span></td></tr></table></figure>
</section>
<section id="A1" class="ltx_appendix"><h2 class="ltx_title ltx_title_appendix">Appendix A</h2><p class="ltx_p">Proofs.</p>
<figure id="A1.F3" class="ltx_figure"><img src="2401.00001v1/c.png" width="10" height="10"><figcaption class="ltx_caption"><span class="ltx_tag ltx_tag_figure">Figure 3: </span>An icon only.</figcaption></figure>
</section>
<section id="bib" class="ltx_bibliography"><p class="ltx_p">Smith 2020.</p></section>
</article></body></html>"##;

    #[test]
    fn sections_figures_and_tables() {
        let f = parse(PAGE);
        assert_eq!(f.sections.len(), 2);
        assert_eq!(f.sections[0].title, "1 Introduction");
        assert_eq!(f.sections[0].text, "We study $x^{2}$ loss in recommenders & search.");
        assert!(!f.sections.iter().any(|s| s.text.contains("Proofs") || s.text.contains("Smith")));
        assert_eq!(f.figures.len(), 2, "{:?}", f.figures);
        assert_eq!(f.figures[0].label, "Figure 1");
        assert_eq!(f.figures[0].caption, "The model, end to end.");
        assert_eq!(f.figures[0].images, vec![Image { src: "https://arxiv.org/html/2401.00001v1/x1.png".into(), w: 476, h: 300 }]);
        assert_eq!(f.figures[1].images.len(), 2);
        assert_eq!(f.figures[1].images[0].src, "https://arxiv.org/html/2401.00001v1/a.svg");
        assert_eq!(f.tables.len(), 1);
        assert_eq!(f.tables[0].label, "Table 1");
        assert_eq!(f.tables[0].rows, vec![vec!["Model".to_string(), "NDCG".into()], vec!["Ours".into(), "0.42".into()]]);
        let ctx = f.context();
        assert!(ctx.contains("[F2] Figure 2: (a) Recall. (b) Length."), "{ctx}");
        assert!(ctx.contains("[T1] Table 1: Main results."), "{ctx}");
        assert_eq!(parse(""), Full::default());
    }

    #[test]
    fn base64_and_types() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(mime("image/png; charset=binary", "x"), Some("image/png"));
        assert_eq!(mime("application/octet-stream", "a/b.SVG"), Some("image/svg+xml"));
        assert_eq!(mime("text/html", "a/b"), None);
    }
}
