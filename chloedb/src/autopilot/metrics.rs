//! The measures a proposal is judged by: nDCG@k of each query (Järvelin
//! and Kekäläinen, TOIS 2002) with gains 2^grade − 1, as TREC and the
//! relevance lab compute it; the exact sign test of the queries a change
//! made better against those it made worse; a paired bootstrap interval of
//! the mean change (Efron and Tibshirani 1993; Sakai, SIGIR 2006, for its
//! use on IR measures); and latency percentiles.

use std::collections::HashMap;

/// nDCG@k of a ranking of `keys` against `grades` (a row not graded gains
/// nothing), the ideal ranking being the query's graded rows best first.
/// `None` when no row of the query has a grade above 0: nothing to rank
/// first, so the query says nothing about the ranking.
pub fn ndcg_at(keys: &[String], grades: &HashMap<String, f64>, k: usize) -> Option<f64> {
    let gain = |g: f64| 2f64.powf(g.max(0.0)) - 1.0;
    let mut ideal: Vec<f64> = grades.values().copied().filter(|g| *g > 0.0).collect();
    if ideal.is_empty() {
        return None;
    }
    ideal.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal.iter().take(k).enumerate().map(|(i, g)| gain(*g) / ((i + 2) as f64).log2()).sum();
    let dcg: f64 = keys.iter().take(k).enumerate().map(|(i, key)| grades.get(key).map_or(0.0, |g| gain(*g)) / ((i + 2) as f64).log2()).sum();
    Some(dcg / idcg)
}

/// How many of the first `k` keys have a grade.
pub fn judged_at(keys: &[String], grades: &HashMap<String, f64>, k: usize) -> usize {
    keys.iter().take(k).filter(|key| grades.contains_key(*key)).count()
}

/// The mean of `xs`, 0 for none.
pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

/// The `q` quantile (0 to 1) of `xs` by the nearest rank.
pub fn percentile(xs: &[f64], q: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

/// A seeded generator (splitmix64), so that a report is the same each
/// time it is made from the same data.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// The 95% interval of the mean of paired differences `d`, by the
/// percentile bootstrap over `rounds` resamples; `(0, 0)` for none.
pub fn bootstrap_ci(d: &[f64], rounds: usize, seed: u64) -> (f64, f64) {
    if d.is_empty() {
        return (0.0, 0.0);
    }
    let mut rng = Rng::new(seed);
    let mut means: Vec<f64> = (0..rounds.max(1)).map(|_| (0..d.len()).map(|_| d[rng.below(d.len())]).sum::<f64>() / d.len() as f64).collect();
    means.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| means[((q * (means.len() - 1) as f64).round() as usize).min(means.len() - 1)];
    (at(0.025), at(0.975))
}

/// The queries a change made better and worse, by more than `eps`.
pub fn wins(d: &[f64], eps: f64) -> (u64, u64) {
    (d.iter().filter(|x| **x > eps).count() as u64, d.iter().filter(|x| **x < -eps).count() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }
    fn k(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ndcg_is_one_for_the_ideal_ranking_and_less_otherwise() {
        let grades = g(&[("a", 3.0), ("b", 1.0), ("c", 0.0)]);
        assert_eq!(ndcg_at(&k(&["a", "b", "c"]), &grades, 10), Some(1.0));
        let swapped = ndcg_at(&k(&["b", "a"]), &grades, 10).unwrap();
        // (1/1 + 7/log2(3)) / (7 + 1/log2(3))
        let want = (1.0 + 7.0 / 3f64.log2()) / (7.0 + 1.0 / 3f64.log2());
        assert!((swapped - want).abs() < 1e-12, "{swapped} vs {want}");
        assert_eq!(ndcg_at(&k(&["x", "y"]), &grades, 10), Some(0.0));
        assert_eq!(ndcg_at(&k(&["a"]), &g(&[("a", 0.0)]), 10), None);
        // Only the top k count, and the ideal is cut at k too.
        assert_eq!(ndcg_at(&k(&["b", "a"]), &grades, 1), Some(1.0 / 7.0));
        assert_eq!(judged_at(&k(&["a", "x", "c"]), &grades, 2), 1);
    }

    #[test]
    fn bootstrap_brackets_the_mean_and_is_seeded() {
        let d: Vec<f64> = (0..50).map(|i| 0.02 + (i % 5) as f64 * 0.01 - 0.02).collect();
        let (lo, hi) = bootstrap_ci(&d, 1000, 7);
        assert!(lo <= mean(&d) && mean(&d) <= hi, "{lo} {hi}");
        assert_eq!((lo, hi), bootstrap_ci(&d, 1000, 7));
        assert_eq!(percentile(&[5.0, 1.0, 3.0, 2.0, 4.0], 0.95), 5.0);
        assert_eq!(percentile(&[5.0, 1.0, 3.0, 2.0, 4.0], 0.5), 3.0);
        assert_eq!(wins(&[0.1, -0.2, 0.0, 0.3], 1e-9), (2, 1));
    }
}
