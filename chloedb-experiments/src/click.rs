//! Click models: how a simulated user reads a list. Each model samples
//! what one user does with one list (which rows they click, which clicks
//! satisfy them, how long they stay) and gives, exactly, the chance of each
//! of those at every position, so the true value of any ranking under the
//! model is a sum, not an estimate.
//!
//! | model | the user | reference |
//! |---|---|---|
//! | `pbm` | looks at position k with chance θₖ = (k + 1)^−η, whatever else is in the list, and clicks a row seen with its attractiveness α | Richardson et al., WWW 2007 |
//! | `cascade` | reads down the list and clicks the first attractive row, then stops | Craswell et al., WSDM 2008 |
//! | `dcm` | reads down, clicks attractive rows, and after a click reads on with chance λ | Guo et al., WSDM 2009 |
//! | `dbn` | reads down, clicks attractive rows, stops when a click satisfies, and otherwise reads on with chance γ | Chapelle and Zhang, WWW 2009 |
//! | `ubm` | looks at position r with a chance that depends on r and on the distance d to the last click: r^(−η/2) · d^(−η/2), so θ with no click above | Dupret and Piwowarski, SIGIR 2008 |
//! | `trust` | as `pbm`, but trusts the top: clicks a row seen that is not relevant with chance ε⁻/(k + 1) | Agarwal et al., WWW 2019 |
//!
//! A click satisfies with the row's chance s (from its grade), and then the
//! user stays longer: dwell is lognormal, above 30 seconds for a satisfied
//! click and below for one that is not.

use crate::policy::Rng;
use crate::stats;

/// What a user sees at one position: the chance the row attracts a click
/// (for `trust`, that it is relevant) and the chance a click on it
/// satisfies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub attract: f64,
    pub satisfy: f64,
}

/// How one user reads (from the population: [`crate::users`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Behaviour {
    /// Position bias: θₖ = (k + 1)^−η.
    pub eta: f64,
    /// DBN's chance of reading on after a row that did not satisfy.
    pub gamma: f64,
    /// DCM's chance of reading on after a click.
    pub lambda: f64,
    /// Trust bias: the chance of clicking a row seen that is not relevant,
    /// at the top (divided by k + 1 below it).
    pub trust: f64,
    /// Dwell's lognormal μ (log seconds); σ is [`DWELL_SIGMA`].
    pub dwell_mu: f64,
}

/// Dwell's σ, in log seconds.
pub const DWELL_SIGMA: f64 = 1.0;
/// A click is satisfied when the user stays longer than this.
pub const SATISFIED_SECONDS: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Model {
    Pbm,
    Cascade,
    Dcm,
    Dbn,
    Ubm,
    Trust,
}

pub const MODELS: [Model; 6] = [Model::Pbm, Model::Cascade, Model::Dcm, Model::Dbn, Model::Ubm, Model::Trust];

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Model::Pbm => "pbm",
            Model::Cascade => "cascade",
            Model::Dcm => "dcm",
            Model::Dbn => "dbn",
            Model::Ubm => "ubm",
            Model::Trust => "trust",
        }
    }

    pub fn parse(s: &str) -> Result<Model, String> {
        MODELS.iter().copied().find(|m| m.name() == s).ok_or_else(|| format!("no click model {s:?}: pbm, cascade, dcm, dbn, ubm, trust"))
    }

    /// Whether a click at position k depends only on the row there and k
    /// (the item-position model): what item-position IPS assumes.
    pub fn item_position(self) -> bool {
        matches!(self, Model::Pbm | Model::Trust)
    }
}

/// What one user did with one list, per position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub click: Vec<bool>,
    pub satisfied: Vec<bool>,
    /// Seconds on the row clicked (0 where not clicked).
    pub dwell: Vec<f64>,
}

impl Outcome {
    pub fn clicks(&self) -> usize {
        self.click.iter().filter(|&&c| c).count()
    }

    pub fn satisfied_clicks(&self) -> usize {
        self.satisfied.iter().filter(|&&c| c).count()
    }
}

/// The chances, per position, of a click and of a satisfied click, and of
/// the list as a whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Expected {
    pub click: Vec<f64>,
    pub satisfied: Vec<f64>,
    /// The chance of at least one click (1 − abandonment).
    pub any_click: f64,
    /// The chance of at least one satisfied click (the query's success).
    pub any_satisfied: f64,
    /// Expected seconds of dwell, over all clicks.
    pub dwell: f64,
}

/// PBM's chance of looking at position k (from 0).
pub fn theta(eta: f64, k: usize) -> f64 {
    ((k + 1) as f64).powf(-eta)
}

/// UBM's examination at 1-based rank r with the last click d ranks above.
fn ubm_gamma(eta: f64, r: usize, d: usize) -> f64 {
    (r as f64).powf(-eta / 2.0) * (d as f64).powf(-eta / 2.0)
}

fn bern(rng: &mut Rng, p: f64) -> bool {
    rng.uniform() < p
}

/// Dwell, given a click and whether it satisfied: lognormal(μ, σ) above or
/// below the cut, by its inverse CDF on that side.
pub fn dwell(rng: &mut Rng, mu: f64, satisfied: bool) -> f64 {
    let f = stats::norm_cdf((SATISFIED_SECONDS.ln() - mu) / DWELL_SIGMA);
    let u = rng.uniform();
    let p = if satisfied { f + (1.0 - f) * u } else { f * u };
    let p = p.clamp(1e-300, 1.0 - 1e-16);
    (mu + DWELL_SIGMA * stats::norm_quantile(p)).exp()
}

/// The expected dwell of a click, satisfied or not.
pub fn dwell_mean(mu: f64, satisfied: bool) -> f64 {
    stats::lognormal_mean_beyond(mu, DWELL_SIGMA, SATISFIED_SECONDS, satisfied)
}

impl Model {
    /// What a user who reads as `b` does with the list `slots`.
    pub fn sample(self, slots: &[Slot], b: &Behaviour, rng: &mut Rng) -> Outcome {
        let n = slots.len();
        let mut o = Outcome { click: vec![false; n], satisfied: vec![false; n], dwell: vec![0.0; n] };
        let mut last = 0usize; // UBM: the 1-based rank of the last click, 0 for none
        for (k, s) in slots.iter().enumerate() {
            let clicked = match self {
                Model::Pbm => bern(rng, theta(b.eta, k)) && bern(rng, s.attract),
                Model::Trust => {
                    if bern(rng, theta(b.eta, k)) {
                        if bern(rng, s.attract) {
                            true
                        } else {
                            // Not relevant: trusted into a click, and it never satisfies.
                            if bern(rng, b.trust / (k + 1) as f64) {
                                o.click[k] = true;
                                o.dwell[k] = dwell(rng, b.dwell_mu, false);
                            }
                            continue;
                        }
                    } else {
                        false
                    }
                }
                Model::Cascade | Model::Dcm | Model::Dbn => bern(rng, s.attract),
                Model::Ubm => {
                    let r = k + 1;
                    bern(rng, ubm_gamma(b.eta, r, r - last)) && bern(rng, s.attract)
                }
            };
            if clicked {
                o.click[k] = true;
                o.satisfied[k] = bern(rng, s.satisfy);
                o.dwell[k] = dwell(rng, b.dwell_mu, o.satisfied[k]);
                last = k + 1;
            }
            // Whether the user reads on to position k + 1.
            let on = match self {
                Model::Pbm | Model::Trust | Model::Ubm => true,
                Model::Cascade => !clicked,
                Model::Dcm => !clicked || bern(rng, b.lambda),
                Model::Dbn => !o.satisfied[k] && bern(rng, b.gamma),
            };
            if !on {
                break;
            }
        }
        o
    }

    /// The exact chances of what [`Model::sample`] draws.
    pub fn expected(self, slots: &[Slot], b: &Behaviour) -> Expected {
        let n = slots.len();
        let mut e = Expected { click: vec![0.0; n], satisfied: vec![0.0; n], ..Default::default() };
        match self {
            Model::Pbm | Model::Trust => {
                let (mut none, mut none_sat) = (1.0, 1.0);
                for (k, s) in slots.iter().enumerate() {
                    let t = theta(b.eta, k);
                    let relevant = t * s.attract;
                    let c = match self {
                        Model::Trust => relevant + t * (1.0 - s.attract) * b.trust / (k + 1) as f64,
                        _ => relevant,
                    };
                    e.click[k] = c;
                    e.satisfied[k] = relevant * s.satisfy;
                    none *= 1.0 - c;
                    none_sat *= 1.0 - e.satisfied[k];
                }
                e.any_click = 1.0 - none;
                e.any_satisfied = 1.0 - none_sat;
            }
            Model::Cascade => {
                let mut reach = 1.0;
                for (k, s) in slots.iter().enumerate() {
                    e.click[k] = reach * s.attract;
                    e.satisfied[k] = e.click[k] * s.satisfy;
                    reach *= 1.0 - s.attract;
                }
                e.any_click = 1.0 - reach;
                e.any_satisfied = e.satisfied.iter().sum();
            }
            Model::Dcm => {
                // reach: examined k; quiet: examined k with no satisfied click above.
                let (mut reach, mut quiet, mut stopped_quiet) = (1.0, 1.0, 0.0);
                let mut no_click = 1.0;
                for (k, s) in slots.iter().enumerate() {
                    e.click[k] = reach * s.attract;
                    e.satisfied[k] = e.click[k] * s.satisfy;
                    reach *= 1.0 - s.attract + s.attract * b.lambda;
                    stopped_quiet += quiet * s.attract * (1.0 - s.satisfy) * (1.0 - b.lambda);
                    quiet *= 1.0 - s.attract + s.attract * (1.0 - s.satisfy) * b.lambda;
                    no_click *= 1.0 - s.attract;
                }
                e.any_click = 1.0 - no_click;
                e.any_satisfied = 1.0 - (stopped_quiet + quiet);
            }
            Model::Dbn => {
                let (mut reach, mut quiet, mut no_click) = (1.0, 1.0, 0.0);
                for (k, s) in slots.iter().enumerate() {
                    e.click[k] = reach * s.attract;
                    e.satisfied[k] = e.click[k] * s.satisfy;
                    reach *= b.gamma * (1.0 - s.attract * s.satisfy);
                    // No click yet: stop here without one, or read on.
                    no_click += quiet * (1.0 - s.attract) * (1.0 - b.gamma);
                    quiet *= (1.0 - s.attract) * b.gamma;
                }
                e.any_click = 1.0 - (no_click + quiet);
                e.any_satisfied = e.satisfied.iter().sum();
            }
            Model::Ubm => {
                // last[j]: the chance the last click above is at 1-based rank j (0: none),
                // and with no satisfied click so far (quiet).
                let mut last = vec![0.0; n + 1];
                let mut quiet = vec![0.0; n + 1];
                last[0] = 1.0;
                quiet[0] = 1.0;
                for (k, s) in slots.iter().enumerate() {
                    let r = k + 1;
                    let (mut c, mut cq) = (0.0, 0.0);
                    for j in 0..r {
                        let p = s.attract * ubm_gamma(b.eta, r, r - j);
                        c += last[j] * p;
                        cq += quiet[j] * p;
                        last[j] *= 1.0 - p;
                        quiet[j] *= 1.0 - p;
                    }
                    last[r] = c;
                    quiet[r] = cq * (1.0 - s.satisfy);
                    e.click[k] = c;
                    e.satisfied[k] = c * s.satisfy;
                }
                e.any_click = 1.0 - last[0];
                e.any_satisfied = 1.0 - quiet.iter().sum::<f64>();
            }
        }
        let (ds, du) = (dwell_mean(b.dwell_mu, true), dwell_mean(b.dwell_mu, false));
        e.dwell = e.click.iter().zip(&e.satisfied).map(|(c, s)| s * ds + (c - s) * du).sum();
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots() -> Vec<Slot> {
        [(0.8, 0.7), (0.1, 0.2), (0.5, 0.9), (0.05, 0.1), (0.6, 0.4), (0.3, 0.5)].iter().map(|&(a, s)| Slot { attract: a, satisfy: s }).collect()
    }

    fn behaviour() -> Behaviour {
        Behaviour { eta: 1.1, gamma: 0.85, lambda: 0.55, trust: 0.35, dwell_mu: 3.4 }
    }

    /// Each model's expected chances are what its sampler draws, to within
    /// four standard errors over 400,000 users.
    #[test]
    fn expected_is_what_is_sampled() {
        let (s, b) = (slots(), behaviour());
        let n = 400_000;
        for m in MODELS {
            let e = m.expected(&s, &b);
            let mut rng = Rng::seeded(m as u64 + 1);
            let (mut click, mut sat) = (vec![0.0; s.len()], vec![0.0; s.len()]);
            let (mut any, mut any_sat, mut dwell) = (0.0, 0.0, 0.0);
            for _ in 0..n {
                let o = m.sample(&s, &b, &mut rng);
                for k in 0..s.len() {
                    click[k] += o.click[k] as u8 as f64;
                    sat[k] += o.satisfied[k] as u8 as f64;
                }
                any += (o.clicks() > 0) as u8 as f64;
                any_sat += (o.satisfied_clicks() > 0) as u8 as f64;
                dwell += o.dwell.iter().sum::<f64>();
            }
            let close = |got: f64, p: f64, what: &str| {
                let se = (p * (1.0 - p) / n as f64).sqrt().max(1e-6);
                assert!((got / n as f64 - p).abs() < 4.0 * se, "{}: {what} {} vs {p}", m.name(), got / n as f64);
            };
            for k in 0..s.len() {
                close(click[k], e.click[k], &format!("click {k}"));
                close(sat[k], e.satisfied[k], &format!("satisfied {k}"));
            }
            close(any, e.any_click, "any click");
            close(any_sat, e.any_satisfied, "any satisfied");
            assert!((dwell / n as f64 / e.dwell - 1.0).abs() < 0.02, "{}: dwell {} vs {}", m.name(), dwell / n as f64, e.dwell);
        }
    }

    #[test]
    fn dwell_falls_on_its_side() {
        let mut rng = Rng::seeded(3);
        for _ in 0..10_000 {
            assert!(dwell(&mut rng, 3.4, true) > SATISFIED_SECONDS);
            assert!(dwell(&mut rng, 3.4, false) < SATISFIED_SECONDS);
        }
    }

    /// UBM with no click above is PBM; the item-position models are the
    /// two whose clicks at k do not move when the rows above change.
    #[test]
    fn what_each_model_assumes() {
        let (s, b) = (slots(), behaviour());
        let pbm = Model::Pbm.expected(&s, &b);
        let ubm = Model::Ubm.expected(&s, &b);
        assert!((pbm.click[0] - ubm.click[0]).abs() < 1e-15);
        let mut t = s.clone();
        t[0] = Slot { attract: 0.01, satisfy: 0.5 };
        for m in MODELS {
            let (a, c) = (m.expected(&s, &b), m.expected(&t, &b));
            assert_eq!((a.click[2] - c.click[2]).abs() < 1e-15, m.item_position(), "{}", m.name());
        }
    }
}
