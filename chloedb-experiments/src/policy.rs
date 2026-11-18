//! ChloeDB's logging policies (`chloedb/src/explore.rs`), ported line for
//! line so that the fast mode draws, for a request, the list and the
//! propensities a chloedb-server draws for it: the same seeded stream
//! ([`Rng::of`]), the same order of operations on the same floats. A change
//! there is a change here; `tests/served.rs` checks the two agree, bit for
//! bit, against a real server.
//!
//! | policy | what it shows | propensity |
//! |---|---|---|
//! | `none` | the ranking as it is | 1 |
//! | `epsilon` | with chance ε the top n in a uniform random order, else the ranking | exact |
//! | `shuffle` | the top n in a uniform random order | exact, 1/n |
//! | `swap` | position 1 swapped with a uniform position up to n (itself included) | exact |
//! | `softmax` | the top n drawn by Plackett–Luce, row i with weight exp(sᵢ/τ) | exact: list and prefix in closed form, per position by quadrature |

use serde_json::{json, Map, Value as Json};

type Result<T> = std::result::Result<T, String>;

fn bad(m: impl Into<String>) -> String {
    m.into()
}

/// A seeded random stream (splitmix64), from a request's id (FNV-1a):
/// the same stream for the same request, independent ones across
/// requests. Interleaving's coin is its low bit.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// The stream of request `request`.
    pub fn of(request: &str) -> Rng {
        Rng(request.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)))
    }

    /// A stream from a number.
    pub fn seeded(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in (0, 1), never 0 or 1.
    pub fn uniform(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    /// Uniform in `0..n` (n > 0), without modulo bias worth noting for n
    /// far below 2^64.
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next_u64() as u128 * n as u128) >> 64) as usize
    }

    /// A standard Gumbel draw.
    pub fn gumbel(&mut self) -> f64 {
        -(-self.uniform().ln()).ln()
    }
}

/// What a Plackett–Luce policy's scores are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum By {
    /// The rows' `score()`.
    Score,
    /// Their ranks: sᵢ = −ln(i + 1).
    Rank,
}

/// A logging policy over a ranked list's top `top` rows; rows below them
/// keep their places.
#[derive(Clone, Debug, PartialEq)]
pub enum Policy {
    None,
    Epsilon { epsilon: f64, top: usize },
    Shuffle { top: usize },
    Swap { top: usize },
    Softmax { tau: f64, top: usize, by: By },
}

/// A list drawn by a policy ([`Policy::draw`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    /// The rows in the order shown, as indexes into the ranked list: the
    /// top permuted, then the rest in their ranked order.
    pub order: Vec<usize>,
    /// Per position, the chance that the row there is there.
    pub propensity: Vec<f64>,
    /// Per position, the chance of the rows down to it, in this order. On
    /// the last row it is the list's chance.
    pub prefix: Vec<f64>,
}

impl Policy {
    /// A policy from JSON (a logged query's `"explore"`): `{"policy":
    /// "epsilon", "epsilon": 0.1, "top": 10}`, `{"policy": "shuffle",
    /// "top": 10}`, `{"policy": "swap", "top": 10}`, `{"policy":
    /// "softmax", "tau": 0.5, "top": 20, "by": "score"}`, or `{"policy":
    /// "none"}`. `top` defaults to 10 (20 for softmax), `by` to `score`.
    pub fn from_json(j: &Json) -> Result<Policy> {
        let m = j.as_object().ok_or_else(|| bad(format!("explore is an object with a policy (none, epsilon, shuffle, swap, softmax), not {j}")))?;
        let name = m.get("policy").and_then(|v| v.as_str()).ok_or_else(|| bad("explore names its policy: none, epsilon, shuffle, swap or softmax"))?;
        let allowed: &[&str] = match name {
            "none" => &["policy"],
            "epsilon" => &["policy", "epsilon", "top"],
            "shuffle" | "swap" => &["policy", "top"],
            "softmax" => &["policy", "tau", "top", "by"],
            _ => return Err(bad(format!("explore has no policy {name:?}: none, epsilon, shuffle, swap, softmax"))),
        };
        if let Some(k) = m.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(bad(format!("explore policy {name} has no option {k:?}: {}", allowed[1..].join(", "))));
        }
        let top = |d: usize| -> Result<usize> {
            match m.get("top") {
                None | Some(Json::Null) => Ok(d),
                Some(v) => v.as_u64().filter(|&n| (2..=1000).contains(&n)).map(|n| n as usize).ok_or_else(|| bad(format!("explore top is a number of rows from 2 to 1000, not {v}"))),
            }
        };
        let num = |k: &str, ok: &dyn Fn(f64) -> bool, what: &str| -> Result<f64> {
            let v = m.get(k).ok_or_else(|| bad(format!("explore policy {name} needs {k}, {what}")))?;
            v.as_f64().filter(|x| x.is_finite() && ok(*x)).ok_or_else(|| bad(format!("explore {k} is {what}, not {v}")))
        };
        Ok(match name {
            "none" => Policy::None,
            "epsilon" => Policy::Epsilon { epsilon: num("epsilon", &|x| (0.0..=1.0).contains(&x), "a chance from 0 to 1")?, top: top(10)? },
            "shuffle" => Policy::Shuffle { top: top(10)? },
            "swap" => Policy::Swap { top: top(10)? },
            _ => Policy::Softmax {
                tau: num("tau", &|x| x > 0.0, "a temperature above 0")?,
                top: top(20)?,
                by: match m.get("by").and_then(|v| v.as_str()) {
                    None | Some("score") => By::Score,
                    Some("rank") => By::Rank,
                    Some(o) => return Err(bad(format!("explore by is score or rank, not {o:?}"))),
                },
            },
        })
    }

    /// The policy as JSON, as the log keeps it (column `policy`).
    pub fn to_json(&self) -> Json {
        match self {
            Policy::None => json!({"policy": "none"}),
            Policy::Epsilon { epsilon, top } => json!({"policy": "epsilon", "epsilon": epsilon, "top": top}),
            Policy::Shuffle { top } => json!({"policy": "shuffle", "top": top}),
            Policy::Swap { top } => json!({"policy": "swap", "top": top}),
            Policy::Softmax { tau, top, by } => {
                let mut m = Map::new();
                m.insert("policy".into(), json!("softmax"));
                m.insert("tau".into(), json!(tau));
                m.insert("top".into(), json!(top));
                m.insert("by".into(), json!(if *by == By::Rank { "rank" } else { "score" }));
                Json::Object(m)
            }
        }
    }

    /// The ranked rows it permutes.
    pub fn top(&self) -> usize {
        match self {
            Policy::None => 0,
            Policy::Epsilon { top, .. } | Policy::Shuffle { top } | Policy::Swap { top } | Policy::Softmax { top, .. } => *top,
        }
    }

    /// Whether it needs the rows' scores.
    pub fn needs_scores(&self) -> bool {
        matches!(self, Policy::Softmax { by: By::Score, .. })
    }

    /// The Plackett–Luce weights of the top `n` rows: exp((sᵢ − max)/τ),
    /// the largest 1.
    pub fn weights(tau: f64, by: By, scores: &[Option<f64>], n: usize) -> Result<Vec<f64>> {
        let s: Vec<f64> = match by {
            By::Rank => (0..n).map(|i| -((i + 1) as f64).ln()).collect(),
            By::Score => scores[..n].iter().map(|s| s.filter(|x| x.is_finite()).ok_or_else(|| bad("softmax exploration by score needs every row ranked to have a score: rank by an ORDER BY that scores them, or explore by rank"))).collect::<Result<_>>()?,
        };
        let max = s.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Ok(s.iter().map(|x| ((x - max) / tau).exp()).collect())
    }

    /// Draws the list a request shows from rows ranked with `scores` (in
    /// ranked order), with the stream `rng`: the top `min(top, len)` rows
    /// permuted as the policy says and the rest kept, with each position's
    /// propensity and prefix chance.
    pub fn draw(&self, scores: &[Option<f64>], rng: &mut Rng) -> Result<Draw> {
        self.draw_with(scores, rng, None)
    }

    /// [`Policy::draw`] with a softmax's position chances given (those
    /// [`Policy::marginals_of`] gives for these scores): the same draw,
    /// for a simulator that asks the same query many times.
    pub fn draw_with(&self, scores: &[Option<f64>], rng: &mut Rng, kept: Option<&[Vec<f64>]>) -> Result<Draw> {
        let len = scores.len();
        let n = self.top().min(len);
        let mut order: Vec<usize> = (0..len).collect();
        let mut propensity = vec![1.0; len];
        let mut prefix = vec![1.0; len];
        if n < 2 {
            return Ok(Draw { order, propensity, prefix });
        }
        let nf = n as f64;
        // The chance of a uniform order's first k + 1 rows: Π 1/(n − t).
        let uniform_prefix = |k: usize| (0..=k.min(n - 1)).map(|t| 1.0 / (nf - t as f64)).product::<f64>();
        match self {
            Policy::None => {}
            Policy::Shuffle { .. } => {
                shuffle(&mut order[..n], rng);
                for k in 0..len {
                    propensity[k] = if k < n { 1.0 / nf } else { 1.0 };
                    prefix[k] = uniform_prefix(k);
                }
            }
            Policy::Epsilon { epsilon, .. } => {
                let e = *epsilon;
                if rng.uniform() < e {
                    shuffle(&mut order[..n], rng);
                }
                let mut same = true;
                for k in 0..len {
                    same &= order[k] == k;
                    propensity[k] = if k < n { e / nf + (1.0 - e) * (order[k] == k) as u8 as f64 } else { 1.0 };
                    prefix[k] = e * uniform_prefix(k) + (1.0 - e) * same as u8 as f64;
                }
            }
            Policy::Swap { .. } => {
                let j = rng.below(n);
                order.swap(0, j);
                for k in 0..len {
                    propensity[k] = match k {
                        0 => 1.0 / nf,
                        k if k < n && order[k] == 0 => 1.0 / nf,
                        k if k < n => (nf - 1.0) / nf,
                        _ => 1.0,
                    };
                    prefix[k] = 1.0 / nf;
                }
            }
            Policy::Softmax { tau, by, .. } => {
                let w = Self::weights(*tau, *by, scores, n)?;
                let mut keyed: Vec<(f64, usize)> = (0..n).map(|i| (w[i].ln() + rng.gumbel(), i)).collect();
                keyed.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                for (k, (_, i)) in keyed.iter().enumerate() {
                    order[k] = *i;
                }
                let fresh;
                let m: &[Vec<f64>] = match kept {
                    Some(m) => m,
                    None => {
                        fresh = Self::marginals_of(&w, n);
                        &fresh
                    }
                };
                let mut left: f64 = w.iter().sum();
                let mut p = 1.0;
                for k in 0..len {
                    if k < n {
                        let i = order[k];
                        p *= w[i] / left;
                        left -= w[i];
                        propensity[k] = m[i][k];
                    } else {
                        propensity[k] = 1.0;
                    }
                    prefix[k] = p;
                }
            }
        }
        Ok(Draw { order, propensity, prefix })
    }

    /// The chance that ranked row `i` is shown at position `k`, for every
    /// row and position of the top `min(top, len)` (rows below it keep
    /// their places, with chance 1): in closed form, and for Plackett–Luce
    /// by quadrature ([`Policy::marginals_of`]).
    pub fn marginals(&self, scores: &[Option<f64>]) -> Result<Vec<Vec<f64>>> {
        let len = scores.len();
        let n = self.top().min(len);
        let mut m = vec![vec![0.0; len]; len];
        for (i, row) in m.iter_mut().enumerate().skip(n) {
            row[i] = 1.0;
        }
        let nf = n as f64;
        match self {
            _ if n < 2 => {
                for (i, row) in m.iter_mut().enumerate() {
                    row[i] = 1.0;
                }
            }
            Policy::None => {
                for (i, row) in m.iter_mut().enumerate() {
                    row[i] = 1.0;
                }
            }
            Policy::Shuffle { .. } => {
                for row in m.iter_mut().take(n) {
                    row[..n].iter_mut().for_each(|x| *x = 1.0 / nf);
                }
            }
            Policy::Epsilon { epsilon, .. } => {
                for (i, row) in m.iter_mut().enumerate().take(n) {
                    row[..n].iter_mut().for_each(|x| *x = epsilon / nf);
                    row[i] += 1.0 - epsilon;
                }
            }
            Policy::Swap { .. } => {
                m[0][..n].iter_mut().for_each(|x| *x = 1.0 / nf);
                for (i, row) in m.iter_mut().enumerate().take(n).skip(1) {
                    row[0] = 1.0 / nf;
                    row[i] = (nf - 1.0) / nf;
                }
            }
            Policy::Softmax { tau, by, .. } => {
                let w = Self::weights(*tau, *by, scores, n)?;
                let pm = Self::marginals_of(&w, n);
                for i in 0..n {
                    m[i][..n].copy_from_slice(&pm[i]);
                }
            }
        }
        Ok(m)
    }

    /// Plackett–Luce position chances of `n` rows weighted `w`, exact to
    /// within 1e-6 (1e-9 for weights less than a million times apart). In the Gumbel form of Plackett–Luce (each row's key is
    /// ln wᵢ plus a Gumbel draw, and the rows are shown by key), row i is
    /// at position k when exactly k other rows have a higher key; given
    /// V = the Gumbel CDF of row i's draw, uniform on (0, 1), row j is above
    /// it with chance 1 − V^(wⱼ/wᵢ), independently. So P(i at k) is the
    /// integral over V of the chance that k of those happen (a
    /// Poisson-binomial), one dimension, taken by tanh-sinh quadrature,
    /// which the integrand's steep ends do not upset. Position 0 is
    /// wᵢ / Σw, as it must be.
    pub fn marginals_of(w: &[f64], n: usize) -> Vec<Vec<f64>> {
        // The same query asked again ranks the same rows with the same
        // scores: its chances are kept, for the last few lists.
        thread_local! {
            static KEPT: std::cell::RefCell<Vec<(Vec<u64>, std::rc::Rc<Vec<Vec<f64>>>)>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let key: Vec<u64> = w[..n].iter().map(|x| x.to_bits()).collect();
        if let Some(m) = KEPT.with(|k| k.borrow().iter().find(|(kw, _)| *kw == key).map(|(_, m)| m.clone())) {
            return (*m).clone();
        }
        let m = Self::marginals_integrated(&w[..n]);
        KEPT.with(|k| {
            let mut k = k.borrow_mut();
            if k.len() >= 16 {
                k.remove(0);
            }
            k.push((key, std::rc::Rc::new(m.clone())));
        });
        m
    }

    fn marginals_integrated(w: &[f64]) -> Vec<Vec<f64>> {
        let n = w.len();
        // Tanh-sinh nodes on (0, 1): v = 1 / (1 + e^(−2a)), a = (π/2)
        // sinh t, with ln v and the weight dv/dt = v (1 − v) π cosh t.
        const H: f64 = 1.0 / 16.0;
        let nodes: Vec<(f64, f64)> = (-72..=72)
            .map(|i| {
                let t = i as f64 * H;
                let a = std::f64::consts::FRAC_PI_2 * t.sinh();
                let ln_v = -(-2.0 * a).exp().ln_1p();
                let one_minus_v = 1.0 / (1.0 + (2.0 * a).exp());
                let v = ln_v.exp();
                (ln_v, H * v * one_minus_v * std::f64::consts::PI * t.cosh())
            })
            .filter(|&(_, wt)| wt > 0.0 && wt.is_finite())
            .collect();
        let mut m = vec![vec![0.0; n]; n];
        let mut q = vec![0.0; n];
        for i in 0..n {
            for &(ln_v, wt) in &nodes {
                q.iter_mut().for_each(|x| *x = 0.0);
                q[0] = 1.0;
                let mut c = 0;
                for (j, wj) in w.iter().enumerate() {
                    if j == i {
                        continue;
                    }
                    // The chance row j is above row i.
                    let above = -(ln_v * wj / w[i]).exp_m1();
                    c += 1;
                    for x in (1..=c).rev() {
                        q[x] = q[x] * (1.0 - above) + q[x - 1] * above;
                    }
                    q[0] *= 1.0 - above;
                }
                for (k, qk) in q.iter().enumerate() {
                    m[i][k] += wt * qk;
                }
            }
        }
        // The quadrature's last digits: each row and each position sums
        // to 1.
        for row in m.iter_mut() {
            let s: f64 = row.iter().sum();
            if s > 0.0 {
                row.iter_mut().for_each(|x| *x /= s);
            }
        }
        m
    }

    /// The chance the policy shows exactly the rows `order` (indexes into
    /// the ranked list, a full permutation of its first `order.len()`
    /// rows or more) down to each position: the prefix chances an
    /// estimator needs to value a list it reads from the log.
    pub fn prefix_chances(&self, scores: &[Option<f64>], order: &[usize]) -> Result<Vec<f64>> {
        let len = scores.len();
        let n = self.top().min(len);
        let nf = n as f64;
        let mut out = Vec::with_capacity(order.len());
        let uniform_prefix = |k: usize| (0..=k.min(n.max(1) - 1)).map(|t| 1.0 / (nf - t as f64)).product::<f64>();
        let rest_kept = |k: usize| (n..=k).all(|t| order[t] == t);
        match self {
            _ if n < 2 => {
                let mut same = true;
                for (k, &o) in order.iter().enumerate() {
                    same &= o == k;
                    out.push(same as u8 as f64);
                }
            }
            Policy::None => {
                let mut same = true;
                for (k, &o) in order.iter().enumerate() {
                    same &= o == k;
                    out.push(same as u8 as f64);
                }
            }
            Policy::Shuffle { .. } => {
                for k in 0..order.len() {
                    let inside = order[..=k.min(n - 1)].iter().all(|&o| o < n);
                    out.push(if inside && rest_kept(k) { uniform_prefix(k) } else { 0.0 });
                }
            }
            Policy::Epsilon { epsilon, .. } => {
                let mut same = true;
                for k in 0..order.len() {
                    same &= order[k] == k;
                    let inside = order[..=k.min(n - 1)].iter().all(|&o| o < n);
                    let shuffled = if inside && rest_kept(k) { uniform_prefix(k) } else { 0.0 };
                    out.push(epsilon * shuffled + (1.0 - epsilon) * same as u8 as f64);
                }
            }
            Policy::Swap { .. } => {
                // The list is fixed by the row at the top: ranked row j,
                // swapped with row 0.
                let j = order.first().copied().unwrap_or(0);
                for k in 0..order.len() {
                    let ok = j < n && (0..=k).all(|t| order[t] == if t == 0 { j } else if t == j { 0 } else { t });
                    out.push(if ok { 1.0 / nf } else { 0.0 });
                }
            }
            Policy::Softmax { tau, by, .. } => {
                let w = Self::weights(*tau, *by, scores, n)?;
                let mut left: f64 = w.iter().sum();
                let mut used = vec![false; n];
                let mut p = 1.0;
                for k in 0..order.len() {
                    let i = order[k];
                    if k < n {
                        if i >= n || used[i] {
                            p = 0.0;
                        } else {
                            p *= w[i] / left;
                            left -= w[i];
                            used[i] = true;
                        }
                    } else if i != k {
                        p = 0.0;
                    }
                    out.push(p);
                }
            }
        }
        Ok(out)
    }
}

/// A uniform permutation of `xs` (Fisher–Yates).
fn shuffle(xs: &mut [usize], rng: &mut Rng) {
    for i in (1..xs.len()).rev() {
        let j = rng.below(i + 1);
        xs.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(n: usize) -> Vec<Option<f64>> {
        (0..n).map(|i| Some(10.0 - i as f64 * 0.7)).collect()
    }

    fn policies() -> Vec<Policy> {
        vec![
            Policy::None,
            Policy::Epsilon { epsilon: 0.3, top: 5 },
            Policy::Shuffle { top: 5 },
            Policy::Swap { top: 5 },
            Policy::Softmax { tau: 1.5, top: 6, by: By::Score },
            Policy::Softmax { tau: 0.7, top: 6, by: By::Rank },
        ]
    }

    #[test]
    fn policies_read_and_write_json() {
        for p in policies() {
            assert_eq!(Policy::from_json(&p.to_json()).unwrap(), p);
        }
        assert_eq!(Policy::from_json(&json!({"policy": "softmax", "tau": 0.5})).unwrap(), Policy::Softmax { tau: 0.5, top: 20, by: By::Score });
        for bad in [json!({"policy": "greedy"}), json!({"policy": "softmax"}), json!({"policy": "epsilon", "epsilon": 2}), json!({"policy": "swap", "top": 1}), json!({"policy": "shuffle", "tau": 1}), json!(true)] {
            assert!(Policy::from_json(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_draw_is_the_requests() {
        let s = scores(12);
        for p in policies() {
            let a = p.draw(&s, &mut Rng::of("r1")).unwrap();
            assert_eq!(a, p.draw(&s, &mut Rng::of("r1")).unwrap());
            let mut sorted = a.order.clone();
            sorted.sort();
            assert_eq!(sorted, (0..12).collect::<Vec<_>>(), "{p:?} draws a permutation");
            assert!(a.order[p.top().max(1)..].iter().enumerate().all(|(k, &o)| o == k + p.top().max(1)), "{p:?} keeps the rows below its top");
        }
    }

    /// The propensity check: drawn 100,000 times for one ranked list,
    /// every logged propensity is within 0.005 of how often the row was
    /// at its position, and every logged prefix chance within 0.005 of
    /// how often that prefix came; the marginals sum to 1 by row and by
    /// position.
    #[test]
    fn propensities_match_how_often_rows_land() {
        let s = scores(8);
        let runs = 100_000;
        for p in policies() {
            let m = p.marginals(&s).unwrap();
            for i in 0..8 {
                let (r, c) = (m[i].iter().sum::<f64>(), (0..8).map(|j| m[j][i]).sum::<f64>());
                assert!((r - 1.0).abs() < 1e-9 && (c - 1.0).abs() < 1e-9, "{p:?} marginals of {i} sum to {r} and {c}");
            }
            let mut at = vec![vec![0u32; 8]; 8];
            let mut prefixes: std::collections::HashMap<Vec<usize>, u32> = std::collections::HashMap::new();
            let mut logged: std::collections::HashMap<(usize, usize), f64> = std::collections::HashMap::new();
            let mut logged_prefix: std::collections::HashMap<Vec<usize>, f64> = std::collections::HashMap::new();
            for r in 0..runs {
                let d = p.draw(&s, &mut Rng::of(&format!("request-{r}"))).unwrap();
                for (k, &i) in d.order.iter().enumerate() {
                    at[i][k] += 1;
                    let prior = logged.insert((i, k), d.propensity[k]);
                    assert!(prior.is_none_or(|x| (x - d.propensity[k]).abs() < 1e-12), "{p:?}: row {i} at {k} logs one propensity");
                    assert!((d.propensity[k] - m[i][k]).abs() < 1e-12, "{p:?}: the logged propensity is the marginal");
                }
                let first3 = d.order[..3].to_vec();
                *prefixes.entry(first3.clone()).or_default() += 1;
                logged_prefix.insert(first3.clone(), d.prefix[2]);
                let again = p.prefix_chances(&s, &d.order).unwrap();
                for k in 0..8 {
                    assert!((again[k] - d.prefix[k]).abs() < 1e-12, "{p:?}: prefix chances recomputed from the list");
                }
            }
            for ((i, k), prop) in &logged {
                let seen = at[*i][*k] as f64 / runs as f64;
                assert!((seen - prop).abs() < 0.005, "{p:?}: row {i} at {k} logged {prop}, seen {seen}");
            }
            for (pre, prop) in &logged_prefix {
                let seen = prefixes[pre] as f64 / runs as f64;
                assert!((seen - prop).abs() < 0.005, "{p:?}: prefix {pre:?} logged {prop}, seen {seen}");
            }
        }
    }

    #[test]
    fn exact_policies_log_exact_numbers() {
        let s = scores(6);
        let d = Policy::Shuffle { top: 4 }.draw(&s, &mut Rng::of("x")).unwrap();
        assert_eq!(&d.propensity[..4], &[0.25; 4]);
        assert_eq!(&d.propensity[4..], &[1.0, 1.0]);
        assert_eq!(d.prefix[3], 1.0 / 24.0);
        let d = Policy::Swap { top: 4 }.draw(&s, &mut Rng::of("y")).unwrap();
        assert!(d.prefix.iter().all(|&x| x == 0.25));
        let d = Policy::None.draw(&s, &mut Rng::of("z")).unwrap();
        assert_eq!(d.order, (0..6).collect::<Vec<_>>());
        assert!(d.propensity.iter().chain(&d.prefix).all(|&x| x == 1.0));
        // A list the policy cannot show has chance 0.
        assert_eq!(Policy::Shuffle { top: 3 }.prefix_chances(&s, &[0, 1, 2, 4, 3, 5]).unwrap()[3], 0.0);
        assert_eq!(Policy::None.prefix_chances(&s, &[1, 0]).unwrap(), vec![0.0, 0.0]);
    }

    /// The quadrature against every permutation's chance, summed, for
    /// weights from even to steep.
    #[test]
    fn plackett_luce_marginals_are_exact() {
        fn permutations(n: usize) -> Vec<Vec<usize>> {
            if n == 1 {
                return vec![vec![0]];
            }
            let mut out = Vec::new();
            for p in permutations(n - 1) {
                for at in 0..n {
                    let mut q = p.clone();
                    q.insert(at, n - 1);
                    out.push(q);
                }
            }
            out
        }
        for w in [vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0], vec![1.0, 0.6, 0.3, 0.1, 0.05, 0.01], vec![1.0, 1e-3, 0.5, 1e-6, 0.9, 0.2], vec![1.0, 0.999, 1e-9, 0.5, 0.25, 0.125]] {
            let n = w.len();
            let mut exact = vec![vec![0.0; n]; n];
            for p in permutations(n) {
                let (mut left, mut chance) = (w.iter().sum::<f64>(), 1.0);
                for &i in &p {
                    chance *= w[i] / left;
                    left -= w[i];
                }
                for (k, &i) in p.iter().enumerate() {
                    exact[i][k] += chance;
                }
            }
            let m = Policy::marginals_integrated(&w);
            for i in 0..n {
                for k in 0..n {
                    assert!((m[i][k] - exact[i][k]).abs() < 1e-6, "w {w:?}: row {i} at {k}: {} against {}", m[i][k], exact[i][k]);
                }
            }
        }
    }

    #[test]
    fn softmax_by_score_needs_scores() {
        let s = vec![Some(1.0), None, Some(0.5)];
        assert!(Policy::Softmax { tau: 1.0, top: 3, by: By::Score }.draw(&s, &mut Rng::of("a")).is_err());
        assert!(Policy::Softmax { tau: 1.0, top: 3, by: By::Rank }.draw(&s, &mut Rng::of("a")).is_ok());
    }

    #[test]
    fn the_coin_is_the_streams_low_bit() {
        let mut r = Rng::of("abc");
        let first: Vec<bool> = (0..8).map(|_| r.next_u64() & 1 == 1).collect();
        let mut r2 = Rng::of("abc");
        assert_eq!(first, (0..8).map(|_| r2.next_u64() & 1 == 1).collect::<Vec<_>>());
    }
}
