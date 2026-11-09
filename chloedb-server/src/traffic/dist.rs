//! The simulator's random draws: seeded streams, one for each part of a
//! schedule, and the distributions its profiles name.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, LogNormal, Zipf};

pub type Rand = ChaCha8Rng;

/// splitmix64: a seed spread over all 64 bits.
pub fn splitmix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The stream of draws of one part of a schedule (`name`): the same seed and
/// name give the same draws, whatever the other parts draw.
pub fn stream(seed: u64, name: &str) -> Rand {
    // FNV-1a of the name.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    Rand::seed_from_u64(splitmix(seed ^ h))
}

/// Zipf's law over ranks 0..n, rank 0 the most frequent: the share of rank
/// r is proportional to 1 / (r + 1)^s (s 0: every rank alike).
pub struct Ranks {
    n: usize,
    z: Option<Zipf<f64>>,
}

impl Ranks {
    pub fn new(n: usize, s: f64) -> Ranks {
        assert!(n > 0, "Zipf over no ranks");
        let z = (s > 0.0 && n > 1).then(|| Zipf::new(n as u64, s).expect("Zipf's exponent is checked by the profile"));
        Ranks { n, z }
    }

    pub fn draw(&self, rng: &mut Rand) -> usize {
        match &self.z {
            Some(z) => (z.sample(rng) as usize).clamp(1, self.n) - 1,
            None => rng.gen_range(0..self.n),
        }
    }
}

/// The share of draws of Zipf's law over n ranks with exponent s that go to
/// the first `top` ranks.
pub fn zipf_head_share(n: usize, s: f64, top: usize) -> f64 {
    let (mut all, mut head) = (0.0, 0.0);
    for r in 1..=n {
        let x = (r as f64).powf(-s);
        all += x;
        if r <= top {
            head += x;
        }
    }
    head / all
}

/// A log-normal by its median and sigma (sigma 0: always the median).
pub struct LogNorm {
    median: f64,
    d: Option<LogNormal<f64>>,
}

impl LogNorm {
    pub fn new(median: f64, sigma: f64) -> LogNorm {
        let d = (sigma > 0.0).then(|| LogNormal::new(median.ln(), sigma).expect("a log-normal's sigma is above 0"));
        LogNorm { median, d }
    }

    pub fn draw(&self, rng: &mut Rand) -> f64 {
        match &self.d {
            Some(d) => d.sample(rng),
            None => self.median,
        }
    }
}

/// A number between lo and hi whose logarithm is uniform.
pub fn log_uniform(rng: &mut Rand, lo: f64, hi: f64) -> f64 {
    if lo >= hi {
        return lo;
    }
    (lo.ln() + rng.gen::<f64>() * (hi.ln() - lo.ln())).exp()
}

/// The wait before the next event of a Poisson process at `rate` a second.
pub fn exp_gap(rng: &mut Rand, rate: f64) -> f64 {
    -(1.0 - rng.gen::<f64>()).ln() / rate
}

/// A pseudo-random order of 0..n without a table: rank r is row
/// (a r + b) mod n, a coprime to n, both from the seed.
pub struct Scatter {
    n: u64,
    a: u64,
    b: u64,
}

impl Scatter {
    pub fn new(n: usize, seed: u64) -> Scatter {
        let n = n.max(1) as u64;
        let gcd = |mut x: u64, mut y: u64| {
            while y != 0 {
                (x, y) = (y, x % y);
            }
            x
        };
        let mut a = splitmix(seed) % n;
        // An a near n / phi spreads neighbouring ranks far apart.
        a = (a / 2 + (n as f64 * 0.618_033_988_749_895) as u64 / 2) % n;
        while n > 1 && (a == 0 || gcd(a, n) != 1) {
            a = (a + 1) % n;
        }
        Scatter { n, a: a.max(1), b: splitmix(seed.wrapping_add(1)) % n }
    }

    pub fn map(&self, r: usize) -> usize {
        ((self.a as u128 * r as u128 + self.b as u128) % self.n as u128) as usize
    }
}

/// The day's shape at `x`, a share of the day: 1 at its peaks,
/// 1 / peak_to_trough at its troughs, `humps` peaks a day, the first at
/// `peak_at` (a raised cosine).
pub fn day_shape(x: f64, peak_to_trough: f64, humps: u32, peak_at: f64) -> f64 {
    let low = 1.0 / peak_to_trough;
    let c = (std::f64::consts::TAU * humps.max(1) as f64 * (x - peak_at)).cos();
    low + (1.0 - low) * (1.0 + c) / 2.0
}

/// The busy spells of a two-state Markov-modulated process over
/// [0, length): exponential stays in each state, starting calm.
pub fn busy_spells(rng: &mut Rand, busy_s: f64, calm_s: f64, length: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut t = exp_gap(rng, 1.0 / calm_s);
    while t < length {
        let end = t + exp_gap(rng, 1.0 / busy_s);
        out.push((t, end.min(length)));
        t = end + exp_gap(rng, 1.0 / calm_s);
    }
    out
}

/// Whether t falls in one of the (sorted) spells.
pub fn in_spells(spells: &[(f64, f64)], t: f64) -> bool {
    let i = spells.partition_point(|s| s.1 <= t);
    i < spells.len() && spells[i].0 <= t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_are_seeded_and_apart() {
        let a: Vec<u64> = (0..4).map(|_| stream(7, "x").gen()).collect();
        let b: Vec<u64> = {
            let mut r = stream(7, "x");
            (0..4).map(|_| r.gen()).collect()
        };
        assert_ne!(a[0], b[1]);
        let mut x1 = stream(7, "x");
        let mut x2 = stream(7, "x");
        let mut y = stream(7, "y");
        let (p, q, o): (u64, u64, u64) = (x1.gen(), x2.gen(), y.gen());
        assert_eq!(p, q);
        assert_ne!(p, o);
        let mut z = stream(8, "x");
        assert_ne!(p, z.gen::<u64>());
    }

    #[test]
    fn zipf_ranks_follow_the_law() {
        let n = 1000;
        let r = Ranks::new(n, 1.0);
        let mut rng = stream(1, "zipf");
        let mut counts = vec![0usize; n];
        let draws = 400_000;
        for _ in 0..draws {
            counts[r.draw(&mut rng)] += 1;
        }
        // Rank 0 against its expected share 1 / H(1000) = 0.1336, rank 9 against a tenth of that.
        let h: f64 = (1..=n).map(|x| 1.0 / x as f64).sum();
        for rank in [0usize, 1, 9, 99] {
            let want = draws as f64 / (rank as f64 + 1.0) / h;
            let got = counts[rank] as f64;
            assert!((got - want).abs() < 5.0 * want.sqrt() + 0.02 * want, "rank {rank}: {got} draws, {want:.0} expected");
        }
        let head = counts[..10].iter().sum::<usize>() as f64 / draws as f64;
        assert!((head - zipf_head_share(n, 1.0, 10)).abs() < 0.005, "head share {head}");
        // s 0: uniform.
        let u = Ranks::new(10, 0.0);
        let mut c = [0usize; 10];
        for _ in 0..100_000 {
            c[u.draw(&mut rng)] += 1;
        }
        assert!(c.iter().all(|&x| (9_000..11_000).contains(&x)), "{c:?}");
    }

    #[test]
    fn log_normal_has_its_median() {
        let d = LogNorm::new(5.0, 1.0);
        let mut rng = stream(2, "ln");
        let mut x: Vec<f64> = (0..50_001).map(|_| d.draw(&mut rng)).collect();
        x.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((x[25_000] - 5.0).abs() < 0.15, "median {}", x[25_000]);
        assert_eq!(LogNorm::new(3.0, 0.0).draw(&mut rng), 3.0);
    }

    #[test]
    fn scatter_is_a_permutation() {
        for n in [1usize, 2, 10, 97, 1000, 20_000] {
            let s = Scatter::new(n, 42);
            let mut seen = vec![false; n];
            for r in 0..n {
                let x = s.map(r);
                assert!(!seen[x], "n {n}: {x} twice");
                seen[x] = true;
            }
        }
        // Neighbouring ranks land far apart.
        let s = Scatter::new(1_000_000, 3);
        assert!(s.map(0).abs_diff(s.map(1)) > 1000);
    }

    #[test]
    fn day_shape_peaks_and_troughs() {
        assert!((day_shape(0.67, 3.0, 1, 0.67) - 1.0).abs() < 1e-12);
        assert!((day_shape(0.17, 3.0, 1, 0.67) - 1.0 / 3.0).abs() < 1e-12);
        assert!((day_shape(0.3, 1.0, 1, 0.67) - 1.0).abs() < 1e-12);
        let mean: f64 = (0..10_000).map(|i| day_shape(i as f64 / 10_000.0, 3.0, 2, 0.25)).sum::<f64>() / 10_000.0;
        assert!((mean - 2.0 / 3.0).abs() < 1e-3, "{mean}");
    }

    #[test]
    fn bursts_take_their_share() {
        let mut rng = stream(5, "b");
        let spells = busy_spells(&mut rng, 10.0, 60.0, 1_000_000.0);
        let busy: f64 = spells.iter().map(|(a, b)| b - a).sum();
        let share = busy / 1_000_000.0;
        assert!((share - 10.0 / 70.0).abs() < 0.01, "{share}");
        assert!(in_spells(&spells, (spells[3].0 + spells[3].1) / 2.0));
        assert!(!in_spells(&spells, (spells[3].1 + spells[4].0) / 2.0));
    }
}
