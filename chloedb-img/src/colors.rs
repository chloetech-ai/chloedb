//! A picture's colors, made from its thumbnail: its six main colors and
//! how much of it each covers (the palette shown under it), and a
//! histogram of its pixels over bins of CIE Lab, the space in which equal
//! distances look equally different, kept as a vector so that a search by
//! colors is `ORDER BY similarity(colors, :c)`: the square roots of the
//! shares make the dot product of two histograms their Bhattacharyya
//! coefficient, how much the two overlap.

use crate::schema::COLOR_DIM;

const L_BINS: usize = 4;
const AB_BINS: usize = 6;
/// The range of a and b the bins cover; colors beyond fall in the outer bins.
const AB: f32 = 64.0;

/// sRGB (0..255) to CIE Lab (D65).
pub fn lab(rgb: [u8; 3]) -> [f32; 3] {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE Lab back to sRGB.
pub fn rgb(lab: [f32; 3]) -> [u8; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inv = |t: f32| if t.powi(3) > 0.008856 { t.powi(3) } else { (t - 16.0 / 116.0) / 7.787 };
    let (x, y, z) = (inv(fx) * 0.95047, inv(fy), inv(fz) * 1.08883);
    let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
    let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
    let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;
    let gamma = |c: f32| {
        let c = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    [gamma(r), gamma(g), gamma(b)]
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// The bin a Lab color falls in, and how far it is into the next one on
/// each axis (for spreading a color over neighbouring bins).
fn coords(c: [f32; 3]) -> [f32; 3] {
    let l = (c[0] / 100.0 * L_BINS as f32 - 0.5).clamp(0.0, (L_BINS - 1) as f32);
    let ab = |x: f32| ((x + AB) / (2.0 * AB) * AB_BINS as f32 - 0.5).clamp(0.0, (AB_BINS - 1) as f32);
    [l, ab(c[1]), ab(c[2])]
}

/// Adds `w` of color `c` to histogram `h`, spread over the eight bins
/// around it (trilinearly), so that nearly equal colors share bins.
fn add(h: &mut [f32], c: [f32; 3], w: f32) {
    let p = coords(c);
    let base = [p[0].floor() as usize, p[1].floor() as usize, p[2].floor() as usize];
    let frac = [p[0] - base[0] as f32, p[1] - base[1] as f32, p[2] - base[2] as f32];
    let max = [L_BINS - 1, AB_BINS - 1, AB_BINS - 1];
    for dl in 0..2 {
        for da in 0..2 {
            for db in 0..2 {
                let i = [(base[0] + dl).min(max[0]), (base[1] + da).min(max[1]), (base[2] + db).min(max[2])];
                let f = |d: usize, k: usize| if d == 1 { frac[k] } else { 1.0 - frac[k] };
                h[(i[0] * AB_BINS + i[1]) * AB_BINS + i[2]] += w * f(dl, 0) * f(da, 1) * f(db, 2);
            }
        }
    }
}

/// A histogram as a vector: the square root of each share, unit length.
fn finish(mut h: Vec<f32>) -> Vec<f32> {
    let total: f32 = h.iter().sum();
    if total > 0.0 {
        h.iter_mut().for_each(|x| *x = (*x / total).sqrt());
    }
    crate::pd12m::unit(&mut h);
    h
}

/// A picture's six main colors (Lab k-means of its pixels, by share) and
/// its histogram vector.
pub struct Colors {
    pub palette: Vec<([u8; 3], f32)>,
    pub vector: Vec<f32>,
}

pub fn of_image(img: &image::RgbImage) -> Colors {
    let small = image::imageops::resize(img, 64, 64, image::imageops::FilterType::Triangle);
    let px: Vec<[f32; 3]> = small.pixels().map(|p| lab([p[0], p[1], p[2]])).collect();
    let mut h = vec![0f32; COLOR_DIM];
    for c in &px {
        add(&mut h, *c, 1.0);
    }
    Colors { palette: palette(&px, 6), vector: finish(h) }
}

/// The query of a search by colors: each picked color (sRGB) with its
/// weight, as the histogram of a picture made only of them, each spread a
/// little, so that near colors match too.
pub fn query(picks: &[([u8; 3], f32)]) -> Vec<f32> {
    let mut h = vec![0f32; COLOR_DIM];
    for (c, w) in picks {
        let l = lab(*c);
        for (dl, da, db) in [(0.0, 0.0, 0.0), (6.0, 0.0, 0.0), (-6.0, 0.0, 0.0), (0.0, 8.0, 0.0), (0.0, -8.0, 0.0), (0.0, 0.0, 8.0), (0.0, 0.0, -8.0)] {
            add(&mut h, [l[0] + dl, l[1] + da, l[2] + db], *w);
        }
    }
    finish(h)
}

/// Lab k-means of `px` into `k` colors (seeded at spread-out pixels),
/// largest first, as sRGB with their shares; a color of under 2% of the
/// picture (most often the blend along an edge) is left out.
fn palette(px: &[[f32; 3]], k: usize) -> Vec<([u8; 3], f32)> {
    if px.is_empty() {
        return Vec::new();
    }
    let d2 = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    // Farthest-point seeds from the mean.
    let mean = px.iter().fold([0f32; 3], |m, c| [m[0] + c[0], m[1] + c[1], m[2] + c[2]]).map(|x| x / px.len() as f32);
    let mut centres: Vec<[f32; 3]> = vec![mean];
    while centres.len() < k {
        let far = px.iter().max_by(|a, b| {
            let da = centres.iter().map(|c| d2(a, c)).fold(f32::MAX, f32::min);
            let db = centres.iter().map(|c| d2(b, c)).fold(f32::MAX, f32::min);
            da.total_cmp(&db)
        });
        match far {
            Some(f) => centres.push(*f),
            None => break,
        }
    }
    let mut assign = vec![0usize; px.len()];
    for _ in 0..12 {
        for (i, p) in px.iter().enumerate() {
            assign[i] = (0..centres.len()).min_by(|&a, &b| d2(p, &centres[a]).total_cmp(&d2(p, &centres[b]))).unwrap_or(0);
        }
        let mut sums = vec![[0f32; 4]; centres.len()];
        for (i, p) in px.iter().enumerate() {
            let s = &mut sums[assign[i]];
            s[0] += p[0];
            s[1] += p[1];
            s[2] += p[2];
            s[3] += 1.0;
        }
        for (c, s) in centres.iter_mut().zip(&sums) {
            if s[3] > 0.0 {
                *c = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
            }
        }
    }
    let mut counts = vec![0usize; centres.len()];
    for a in &assign {
        counts[*a] += 1;
    }
    let mut out: Vec<([u8; 3], f32)> = centres.iter().zip(&counts).filter(|(_, n)| **n as f32 >= 0.02 * px.len() as f32).map(|(c, n)| (rgb(*c), *n as f32 / px.len() as f32)).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_goes_there_and_back() {
        for c in [[0u8, 0, 0], [255, 255, 255], [31, 58, 110], [232, 121, 43], [12, 200, 90]] {
            let back = rgb(lab(c));
            for i in 0..3 {
                assert!((back[i] as i32 - c[i] as i32).abs() <= 1, "{c:?} came back as {back:?}");
            }
        }
        assert_eq!(parse_hex("#1f3a6e"), Some([31, 58, 110]));
        assert_eq!(hex([31, 58, 110]), "#1f3a6e");
    }

    #[test]
    fn a_navy_and_orange_picture_matches_navy_and_orange_best() {
        let mut img = image::RgbImage::new(40, 40);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 24 { image::Rgb([31, 58, 110]) } else { image::Rgb([232, 121, 43]) };
        }
        let c = of_image(&img);
        assert_eq!(c.palette.len(), 2, "{:?}", c.palette);
        assert!((c.palette[0].1 - 0.6).abs() < 0.05);
        let navy_orange = crate::pd12m::dot(&c.vector, &query(&[([31, 58, 110], 1.0), ([232, 121, 43], 1.0)]));
        let green = crate::pd12m::dot(&c.vector, &query(&[([20, 160, 60], 1.0)]));
        let navy = crate::pd12m::dot(&c.vector, &query(&[([31, 58, 110], 1.0)]));
        assert!(navy_orange > navy && navy > green, "{navy_orange} {navy} {green}");
    }
}
