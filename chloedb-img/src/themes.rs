//! What each picture is and what it is of, made from the vectors alone:
//! its kind (a painting, a print, an old photograph...), the nearest of a
//! few descriptions CLIP reads (zero-shot), and its theme, one of the
//! clusters k-means finds among the pictures' vectors, each named by the
//! description that best fits its middle and placed on a map so that
//! themes that look alike are next to each other.

use crate::pd12m::{dot, unit, DIM};

/// The kinds, and the words CLIP compares each picture with.
pub const KINDS: &[(&str, &str)] = &[
    ("Photo", "a photograph"),
    ("Old photo", "an old black and white photograph"),
    ("Painting", "an oil painting"),
    ("Drawing", "a pencil drawing or sketch"),
    ("Print", "an engraving or etching, a print"),
    ("Map", "an old map"),
    ("Illustration", "a book illustration or poster"),
    ("Object", "a museum object photographed on a plain background"),
    ("Text page", "a page of printed text or a document"),
    ("Nature", "a close-up photo of a plant, insect or animal in nature"),
];

/// The kinds of art, of photographs and of nature, for the map's filters.
pub fn kind_group(kind: &str) -> &'static str {
    match kind {
        "Painting" | "Drawing" | "Print" | "Illustration" | "Map" | "Text page" => "art",
        "Nature" => "nature",
        _ => "photos",
    }
}

/// Names a theme may get: the one whose words best fit the middle of its
/// pictures, each name used once.
pub const NAMES: &[&str] = &[
    "Molecules and models", "Buildings and monuments", "Trains, ships and factories", "Madonnas and saints", "Family photos",
    "Cartoons and posters", "Forests and streams", "Studio portraits", "Crowds and ceremonies", "Landscape paintings",
    "Wildflowers", "Interiors and ruins", "Towns in old paintings", "From the air", "Letters and documents",
    "Lace and textiles", "Engraved portraits", "Street signs and traffic", "Berries and leaves", "Plants up close",
    "Old city postcards", "Vases and ceramics", "Roads and streets", "Flowers", "Aircraft", "Maps",
    "Japanese woodblock prints", "Moths and beetles", "Butterflies and dragonflies", "Medieval manuscripts",
    "Speeches and protests", "Spiders and caterpillars", "Moss and lichen", "Birds", "Water birds",
    "Specimens with labels", "Shells and fossils", "Medals and banknotes", "Frogs, lizards and snakes", "Under water",
    "Ancient coins", "Mushrooms", "Portrait paintings", "Ships at sea", "Mountains", "Rivers and bridges",
    "Churches and cathedrals", "Castles", "Gardens and parks", "Farms and fields", "Horses", "Dogs and cats",
    "Cattle and farm animals", "Wild animals", "Insects", "Fish", "Trees", "Cacti and succulents", "Seaweed and coral",
    "Rocks and minerals", "Skulls and bones", "Anatomy drawings", "Botanical drawings", "Natural history plates",
    "Fashion plates", "Costumes and dress", "Jewellery", "Furniture", "Clocks and instruments", "Weapons and armour",
    "Tools and machines", "Cars", "Bicycles", "Railways", "Harbours and boats", "Beaches and coast", "Snow and winter",
    "Night scenes", "Sky and clouds", "Space and stars", "Sculpture", "Statues in parks", "Classical ruins",
    "Egyptian antiquities", "Greek vases", "Asian art", "Islamic art", "Calligraphy", "Book covers", "Title pages",
    "Sheet music", "Newspapers", "Advertisements", "Tickets and labels", "Stamps", "Playing cards", "Patterns and ornament",
    "Wallpaper and textiles", "Rugs and carpets", "Baskets and weaving", "Pottery", "Glass", "Silverware",
    "Soldiers and war", "Ships and naval battles", "Kings and queens", "Religious scenes", "Myths and allegories",
    "Children", "Women in portraits", "Men in uniform", "Workers", "Markets and shops", "Restaurants and food",
    "Fruit and vegetables", "Still lifes", "Sport", "Theatre and dance", "Music and musicians", "Schools and classrooms",
    "Hospitals and medicine", "Science and laboratories", "Diagrams and charts", "Architecture drawings", "Floor plans",
    "City views", "Villages", "Houses", "Bridges", "Lighthouses", "Windmills", "Waterfalls", "Lakes", "Deserts",
    "Jungle", "Volcanoes", "Caves", "Ice and glaciers", "Microscope images", "Cells and tissue", "X-rays",
    "Satellite images", "Aerial photos of towns", "Industrial sites", "Mines and quarries", "Power and electricity",
    "Telephones and radios", "Computers", "Typewriters", "Cameras", "Toys and dolls", "Games", "Masks", "Puppets",
    "Coins and tokens", "Seals and stamps", "Banknotes", "Badges and buttons", "Flags", "Coats of arms", "Signs and lettering",
];

/// A small, fixed pseudo-random sequence (xorshift), so that the themes
/// come out the same each time from the same pictures.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The index of the largest of `scores`.
pub fn argmax(scores: &[f32]) -> usize {
    let mut best = 0;
    for (i, s) in scores.iter().enumerate() {
        if *s > scores[best] {
            best = i;
        }
    }
    best
}

/// For each vector of `rows` (each [`DIM`] numbers, laid end to end), the
/// nearest of `centres` and its similarity, on every core.
pub fn nearest(rows: &[f32], centres: &[Vec<f32>]) -> Vec<(u16, f32)> {
    let n = rows.len() / DIM;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let per = n.div_ceil(threads).max(1);
    let mut out = vec![(0u16, 0f32); n];
    std::thread::scope(|s| {
        for (part, chunk) in out.chunks_mut(per).enumerate() {
            s.spawn(move || {
                for (j, o) in chunk.iter_mut().enumerate() {
                    let i = part * per + j;
                    let v = &rows[i * DIM..(i + 1) * DIM];
                    let mut best = (0u16, f32::MIN);
                    for (k, c) in centres.iter().enumerate() {
                        let s = dot(v, c);
                        if s > best.1 {
                            best = (k as u16, s);
                        }
                    }
                    *o = best;
                }
            });
        }
    });
    out
}

/// Spherical k-means (centres kept unit length, rows assigned by cosine)
/// over `rows`, seeded by k-means++ with a fixed seed: `k` centres. The
/// seed fixes a slice's themes, so that the names chosen for them
/// (`data/theme-names-*.json`, by theme number) stay theirs.
pub fn kmeans(rows: &[f32], k: usize, iterations: usize) -> Vec<Vec<f32>> {
    let n = rows.len() / DIM;
    let row = |i: usize| &rows[i * DIM..(i + 1) * DIM];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut centres: Vec<Vec<f32>> = vec![row(rng.below(n)).to_vec()];
    // k-means++: each next centre drawn by its distance to the nearest.
    let mut d: Vec<f32> = (0..n).map(|i| 1.0 - dot(row(i), &centres[0])).collect();
    while centres.len() < k {
        let total: f64 = d.iter().map(|x| x.max(0.0) as f64).sum();
        let mut t = rng.unit() * total;
        let mut pick = n - 1;
        for (i, x) in d.iter().enumerate() {
            t -= x.max(0.0) as f64;
            if t <= 0.0 {
                pick = i;
                break;
            }
        }
        let c = row(pick).to_vec();
        for (i, x) in d.iter_mut().enumerate() {
            *x = x.min(1.0 - dot(row(i), &c));
        }
        centres.push(c);
    }
    for _ in 0..iterations {
        let near = nearest(rows, &centres);
        let mut sums = vec![vec![0f32; DIM]; k];
        let mut counts = vec![0usize; k];
        for (i, (c, _)) in near.iter().enumerate() {
            let c = *c as usize;
            counts[c] += 1;
            for (s, x) in sums[c].iter_mut().zip(row(i)) {
                *s += x;
            }
        }
        for (c, s) in sums.iter_mut().enumerate() {
            if counts[c] == 0 {
                // An empty cluster starts again at a random row.
                *s = row(rng.below(n)).to_vec();
            }
            unit(s);
        }
        centres = sums;
    }
    centres
}

/// Each centre's name: the most similar of `names` (their vectors in
/// `name_vecs`), the best pairs first, each name given once.
pub fn name_themes(centres: &[Vec<f32>], name_vecs: &[Vec<f32>], names: &[&str]) -> Vec<String> {
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (c, cv) in centres.iter().enumerate() {
        for (n, nv) in name_vecs.iter().enumerate() {
            pairs.push((dot(cv, nv), c, n));
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = vec![String::new(); centres.len()];
    let mut used = vec![false; names.len()];
    for (_, c, n) in pairs {
        if out[c].is_empty() && !used[n] {
            out[c] = names[n].to_string();
            used[n] = true;
        }
    }
    out
}

/// A place on a plane for each centre, so that centres that look alike
/// are near: classical multidimensional scaling of their cosine distances
/// (the two largest eigenvectors of the centred Gram matrix, by power
/// iteration), then laid out in rows of a tidy grid, `cols` a row.
pub fn layout(centres: &[Vec<f32>], cols: usize) -> Vec<(usize, usize)> {
    let k = centres.len();
    if k == 0 {
        return Vec::new();
    }
    // Squared distances, double-centred: B = -1/2 J D J.
    let mut d = vec![vec![0f64; k]; k];
    for i in 0..k {
        for j in 0..k {
            let s = dot(&centres[i], &centres[j]) as f64;
            d[i][j] = 2.0 - 2.0 * s;
        }
    }
    let rm: Vec<f64> = d.iter().map(|r| r.iter().sum::<f64>() / k as f64).collect();
    let all = rm.iter().sum::<f64>() / k as f64;
    let mut b = vec![vec![0f64; k]; k];
    for i in 0..k {
        for j in 0..k {
            b[i][j] = -0.5 * (d[i][j] - rm[i] - rm[j] + all);
        }
    }
    let mut axes: Vec<Vec<f64>> = Vec::new();
    for a in 0..2 {
        let mut v: Vec<f64> = (0..k).map(|i| ((i * 7 + a * 3 + 1) % 11) as f64 - 5.0).collect();
        let mut lambda = 0.0;
        for _ in 0..300 {
            let mut w: Vec<f64> = (0..k).map(|i| (0..k).map(|j| b[i][j] * v[j]).sum()).collect();
            for u in &axes {
                let p: f64 = w.iter().zip(u).map(|(x, y)| x * y).sum();
                w.iter_mut().zip(u).for_each(|(x, y)| *x -= p * y);
            }
            let n = w.iter().map(|x| x * x).sum::<f64>().sqrt();
            if n == 0.0 {
                break;
            }
            lambda = n;
            v = w.into_iter().map(|x| x / n).collect();
        }
        let _ = lambda;
        axes.push(v);
    }
    let (x, y) = (&axes[0], &axes[1]);
    // Rows by y, each row by x.
    let rows = k.div_ceil(cols);
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&p, &q| y[p].total_cmp(&y[q]));
    let mut out = vec![(0usize, 0usize); k];
    let per = k.div_ceil(rows);
    for (r, chunk) in order.chunks(per).enumerate() {
        let mut row: Vec<usize> = chunk.to_vec();
        row.sort_by(|&p, &q| x[p].total_cmp(&x[q]));
        for (c, &t) in row.iter().enumerate() {
            out[t] = (r, c + (cols.saturating_sub(row.len())) / 2);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around(c: usize, noise: f32, rng: &mut Rng) -> Vec<f32> {
        let mut v = vec![0f32; DIM];
        v[c] = 1.0;
        for x in v.iter_mut() {
            *x += (rng.unit() as f32 - 0.5) * noise;
        }
        unit(&mut v);
        v
    }

    #[test]
    fn kmeans_finds_the_clusters_and_names_each_once() {
        let mut rng = Rng(7);
        let mut rows = Vec::new();
        for i in 0..600 {
            rows.extend(around(i % 3 * 10, 0.02, &mut rng));
        }
        let centres = kmeans(&rows, 3, 10);
        let near = nearest(&rows, &centres);
        for i in 0..600 {
            assert_eq!(near[i].0, near[i % 3].0, "row {i} joins its own cluster");
        }
        let names = ["zero", "ten", "twenty"];
        let vecs: Vec<Vec<f32>> = [0, 10, 20].iter().map(|&c| around(c, 0.0, &mut rng)).collect();
        let named = name_themes(&centres, &vecs, &names);
        let mut sorted = named.clone();
        sorted.sort();
        assert_eq!(sorted, ["ten", "twenty", "zero"]);
        assert_eq!(named[near[0].0 as usize], "zero");
        let at = layout(&centres, 2);
        assert_eq!(at.len(), 3);
        assert!(at.iter().all(|&(r, c)| r < 2 && c < 2));
    }
}
