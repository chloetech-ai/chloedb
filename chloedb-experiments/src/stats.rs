//! The few distributions the simulator and the checks need: the normal's
//! CDF and its inverse, a lognormal's mean above or below a cut, and a
//! chi-square's tail.

/// Φ(x), the standard normal CDF, by [`erfc`].
pub fn norm_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

/// erfc(x), by the Chebyshev fit of Numerical Recipes (3rd edition,
/// §6.2.2), to about 1e-15 relative.
pub fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 2.0 / (2.0 + z);
    let ty = 4.0 * t - 2.0;
    const C: [f64; 28] = [
        -1.3026537197817094,
        6.419_697_923_564_902e-1,
        1.9476473204185836e-2,
        -9.561_514_786_808_631e-3,
        -9.46595344482036e-4,
        3.66839497852761e-4,
        4.2523324806907e-5,
        -2.0278578112534e-5,
        -1.624290004647e-6,
        1.303655835580e-6,
        1.5626441722e-8,
        -8.5238095915e-8,
        6.529054439e-9,
        5.059343495e-9,
        -9.91364156e-10,
        -2.27365122e-10,
        9.6467911e-11,
        2.394038e-12,
        -6.886027e-12,
        8.94487e-13,
        3.13092e-13,
        -1.12708e-13,
        3.81e-16,
        7.106e-15,
        -1.523e-15,
        -9.4e-17,
        1.21e-16,
        -2.8e-17,
    ];
    let (mut d, mut dd) = (0.0, 0.0);
    for &c in C.iter().skip(1).rev() {
        let tmp = d;
        d = ty * d - dd + c;
        dd = tmp;
    }
    let r = t * (-z * z + 0.5 * (C[0] + ty * d) - dd).exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}

/// Φ⁻¹(p) for p in (0, 1): Acklam's rational approximation (relative
/// error 1.15e-9) and one Halley step, which takes it to full precision.
pub fn norm_quantile(p: f64) -> f64 {
    assert!(p > 0.0 && p < 1.0, "a quantile of {p}");
    const A: [f64; 6] = [-3.969683028665376e1, 2.209460984245205e2, -2.759285104469687e2, 1.383577518672690e2, -3.066479806614716e1, 2.506628277459239];
    const B: [f64; 5] = [-5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2, 6.680131188771972e1, -1.328068155288572e1];
    const C: [f64; 6] = [-7.784894002430293e-3, -3.223964580411365e-1, -2.400758277161838, -2.549732539343734, 4.374664141464968, 2.938163982698783];
    const D: [f64; 4] = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996, 3.754408661907416];
    let lo = 0.02425;
    let x = if p < lo {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - lo {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // Halley: e = Φ(x) − p, u = e √(2π) e^(x²/2).
    let e = norm_cdf(x) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (x * x / 2.0).exp();
    x - u / (1.0 + x * u / 2.0)
}

/// A lognormal(μ, σ)'s mean above `cut` (`above`) or below it: E[X | X > c]
/// = e^(μ + σ²/2) Φ((μ + σ² − ln c)/σ) / (1 − Φ((ln c − μ)/σ)), and below
/// with Φ's other sides.
pub fn lognormal_mean_beyond(mu: f64, sigma: f64, cut: f64, above: bool) -> f64 {
    let m = (mu + sigma * sigma / 2.0).exp();
    let a = (cut.ln() - mu) / sigma;
    if above {
        m * norm_cdf(sigma - a) / (1.0 - norm_cdf(a))
    } else {
        m * norm_cdf(a - sigma) / norm_cdf(a)
    }
}

/// The upper tail of a chi-square with `k` degrees of freedom at `x`, by
/// Wilson–Hilferty's normal approximation (good to a few per cent of the
/// tail for k above 30, which the checks' tables are).
pub fn chi2_upper(x: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return 1.0;
    }
    let z = ((x / k).powf(1.0 / 3.0) - (1.0 - 2.0 / (9.0 * k))) / (2.0 / (9.0 * k)).sqrt();
    1.0 - norm_cdf(z)
}

/// P(X ≤ k) for X binomial(n, p), summed exactly (n up to a few thousand).
pub fn binom_cdf(k: u64, n: u64, p: f64) -> f64 {
    if k >= n {
        return 1.0;
    }
    // Terms by the ratio of each to the one before, from P(X = 0).
    let (q, mut term) = (1.0 - p, (1.0 - p).powi(n as i32));
    let mut sum = term;
    for x in 1..=k {
        term *= (n - x + 1) as f64 / x as f64 * p / q;
        sum += term;
    }
    sum.min(1.0)
}

/// The mean and the sample standard deviation.
pub fn mean_sd(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (m, f64::NAN);
    }
    let v = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0);
    (m, v.sqrt())
}

/// A running mean and variance (Welford), for terms too many to keep.
#[derive(Clone, Debug, Default)]
pub struct Running {
    pub n: f64,
    pub mean: f64,
    m2: f64,
}

impl Running {
    pub fn add(&mut self, x: f64) {
        self.n += 1.0;
        let d = x - self.mean;
        self.mean += d / self.n;
        self.m2 += d * (x - self.mean);
    }

    pub fn merge(&mut self, o: &Running) {
        if o.n == 0.0 {
            return;
        }
        let n = self.n + o.n;
        let d = o.mean - self.mean;
        self.mean += d * o.n / n;
        self.m2 += o.m2 + d * d * self.n * o.n / n;
        self.n = n;
    }

    pub fn var(&self) -> f64 {
        if self.n < 2.0 {
            f64::NAN
        } else {
            self.m2 / (self.n - 1.0)
        }
    }

    /// The standard error of the mean.
    pub fn se(&self) -> f64 {
        (self.var() / self.n).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_normal() {
        // Values from R's pnorm and qnorm.
        for (x, p) in [(0.0, 0.5), (1.0, 0.8413447460685429), (-1.96, 0.024997895148220435), (3.0, 0.9986501019683699), (-6.0, 9.865876450376946e-10)] {
            assert!((norm_cdf(x) - p).abs() <= 1e-13 + 1e-9 * p, "{x}: {} vs {p}", norm_cdf(x));
        }
        for p in [1e-10, 0.001, 0.025, 0.3, 0.5, 0.77, 0.975, 0.999999] {
            let x = norm_quantile(p);
            assert!((norm_cdf(x) - p).abs() < 1e-12 * p.max(1e-3), "{p}: {x}");
        }
        assert!((norm_quantile(0.975) - 1.959963984540054).abs() < 1e-12);
    }

    #[test]
    fn a_lognormal_cut() {
        // By the definition, numerically.
        let (mu, s, c) = (3.6, 0.9, 30.0);
        let mut rng = crate::policy::Rng::seeded(9);
        let (mut above, mut na, mut below, mut nb) = (0.0, 0.0, 0.0, 0.0);
        for _ in 0..400_000 {
            let x = (mu + s * norm_quantile(rng.uniform())).exp();
            if x > c {
                above += x;
                na += 1.0;
            } else {
                below += x;
                nb += 1.0;
            }
        }
        assert!((above / na / lognormal_mean_beyond(mu, s, c, true) - 1.0).abs() < 0.01);
        assert!((below / nb / lognormal_mean_beyond(mu, s, c, false) - 1.0).abs() < 0.01);
    }

    #[test]
    fn the_binomial() {
        // Summed with Python's exact integers.
        assert!((binom_cdf(87, 100, 0.95) - 0.0014643480080344517).abs() < 1e-15, "{}", binom_cdf(87, 100, 0.95));
        assert!((binom_cdf(3, 10, 0.5) - 0.171875).abs() < 1e-12);
        assert_eq!(binom_cdf(10, 10, 0.3), 1.0);
    }

    #[test]
    fn running_matches_two_pass() {
        let xs: Vec<f64> = (0..1000).map(|i| ((i * 37 % 101) as f64).sqrt()).collect();
        let (m, sd) = mean_sd(&xs);
        let (mut a, mut b) = (Running::default(), Running::default());
        xs[..400].iter().for_each(|&x| a.add(x));
        xs[400..].iter().for_each(|&x| b.add(x));
        a.merge(&b);
        assert!((a.mean - m).abs() < 1e-12 && (a.var().sqrt() - sd).abs() < 1e-12);
    }
}
