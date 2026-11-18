//! Simulated users: a population with the attributes a report segments by
//! (age, gender, country, device, tenure), how each reads a list (its
//! [`Behaviour`], moved by its attributes and by itself), what it finds
//! attractive (its cohort's taste moves each row's attractiveness a
//! little, so a ranking can win for one cohort and lose for another), how
//! active it is, and how its sessions go on.
//!
//! Everything is drawn from a seed: the same seed, the same population.

use crate::click::Behaviour;
use crate::policy::Rng;
use crate::stats;

pub const AGES: [&str; 6] = ["18-24", "25-34", "35-44", "45-54", "55-64", "65+"];
const AGE_SHARE: [f64; 6] = [0.14, 0.26, 0.21, 0.17, 0.13, 0.09];
pub const GENDERS: [&str; 3] = ["female", "male", "other"];
const GENDER_SHARE: [f64; 3] = [0.48, 0.49, 0.03];
pub const COUNTRIES: [&str; 9] = ["US", "IN", "GB", "DE", "BR", "CA", "FR", "JP", "other"];
const COUNTRY_SHARE: [f64; 9] = [0.32, 0.14, 0.08, 0.07, 0.07, 0.05, 0.05, 0.04, 0.18];
pub const DEVICES: [&str; 3] = ["desktop", "phone", "tablet"];
const DEVICE_SHARE: [f64; 3] = [0.44, 0.49, 0.07];
pub const TENURES: [&str; 3] = ["new", "regular", "veteran"];
const TENURE_SHARE: [f64; 3] = [0.25, 0.5, 0.25];

/// The attributes a report segments by.
pub const ATTRIBUTES: [&str; 5] = ["age", "gender", "country", "device", "tenure"];

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: u32,
    pub age: u8,
    pub gender: u8,
    pub country: u8,
    pub device: u8,
    pub tenure: u8,
    pub behaviour: Behaviour,
    /// Relative activity: impressions go to users in proportion.
    pub activity: f64,
    /// The chance a session goes on after a query that satisfied, and
    /// after one that did not.
    pub go_on: (f64, f64),
    /// How readily this user clicks: every row's attractiveness is
    /// multiplied by it (1 for all when [`Shape::appetite`] is 0).
    pub appetite: f64,
}

impl User {
    /// The user's value of attribute `a` ([`ATTRIBUTES`]), as text.
    pub fn attribute(&self, a: &str) -> &'static str {
        match a {
            "age" => AGES[self.age as usize],
            "gender" => GENDERS[self.gender as usize],
            "country" => COUNTRIES[self.country as usize],
            "device" => DEVICES[self.device as usize],
            "tenure" => TENURES[self.tenure as usize],
            _ => "",
        }
    }

    /// The taste group: rows' attractiveness moves by (age band, gender).
    pub fn taste_group(&self) -> u32 {
        self.age as u32 * 8 + self.gender as u32
    }
}

/// How much the cohorts' tastes move a row's attractiveness: the log of its
/// multiplier has this standard deviation.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub taste: f64,
    /// How much users differ from their cohort (the log of η's multiplier).
    pub spread: f64,
    /// How much users differ in how readily they click: the log of each
    /// user's [`User::appetite`] has this standard deviation (0: none).
    pub appetite: f64,
}

impl Default for Shape {
    fn default() -> Shape {
        Shape { taste: 0.35, spread: 0.15, appetite: 0.0 }
    }
}

pub struct Population {
    pub users: Vec<User>,
    pub shape: Shape,
    pub seed: u64,
    pick: Alias,
}

fn pick(rng: &mut Rng, shares: &[f64]) -> u8 {
    let mut u = rng.uniform() * shares.iter().sum::<f64>();
    for (i, s) in shares.iter().enumerate() {
        if u < *s {
            return i as u8;
        }
        u -= s;
    }
    (shares.len() - 1) as u8
}

fn normal(rng: &mut Rng) -> f64 {
    stats::norm_quantile(rng.uniform())
}

impl Population {
    pub fn new(n: usize, seed: u64, shape: Shape) -> Population {
        let mut rng = Rng::seeded(seed ^ 0x5eed_0f05_e7e5_0001u64);
        let users: Vec<User> = (0..n as u32)
            .map(|id| {
                let age = pick(&mut rng, &AGE_SHARE);
                let gender = pick(&mut rng, &GENDER_SHARE);
                let country = pick(&mut rng, &COUNTRY_SHARE);
                let device = pick(&mut rng, &DEVICE_SHARE);
                let tenure = pick(&mut rng, &TENURE_SHARE);
                // A phone shows fewer rows a screen: position matters more.
                let eta = (1.0 + [0.0, 0.45, 0.2][device as usize] + if age >= 4 { 0.1 } else { 0.0 }) * (shape.spread * normal(&mut rng)).exp();
                let gamma = (0.88 - [0.0, 0.06, 0.03][device as usize] - if tenure == 0 { 0.04 } else { 0.0 } + 0.03 * normal(&mut rng)).clamp(0.5, 0.98);
                let lambda = (0.6 + 0.05 * normal(&mut rng)).clamp(0.2, 0.95);
                let trust = (0.3 + if tenure == 0 { 0.1 } else { 0.0 } + 0.05 * normal(&mut rng)).clamp(0.0, 0.8);
                let dwell_mu = 3.3 + [0.15, -0.2, 0.0][device as usize] + if age >= 4 { 0.2 } else { 0.0 } + 0.2 * normal(&mut rng);
                let activity = (0.8 * normal(&mut rng)).exp() * [0.6, 1.0, 1.6][tenure as usize];
                let go_on = ((0.45f64 + [0.0, 0.05, 0.1][tenure as usize]).min(0.9), (0.6 + 0.05 * normal(&mut rng)).clamp(0.3, 0.85));
                // From a stream of its own, so that the rest is the same
                // whatever the appetite's spread.
                let appetite = if shape.appetite > 0.0 { (shape.appetite * normal(&mut Rng::of(&format!("{seed}/appetite/{id}"))) - shape.appetite * shape.appetite / 2.0).exp() } else { 1.0 };
                User { id, age, gender, country, device, tenure, behaviour: Behaviour { eta, gamma, lambda, trust, dwell_mu }, activity, go_on, appetite }
            })
            .collect();
        let pick = Alias::new(&users.iter().map(|u| u.activity).collect::<Vec<_>>());
        Population { users, shape, seed, pick }
    }

    /// A user drawn by activity.
    pub fn draw(&self, rng: &mut Rng) -> &User {
        &self.users[self.pick.draw(rng)]
    }

    /// Each user's share of impressions.
    pub fn shares(&self) -> Vec<f64> {
        let total: f64 = self.users.iter().map(|u| u.activity).sum();
        self.users.iter().map(|u| u.activity / total).collect()
    }

    /// The multiplier taste group `group` puts on row `row`'s
    /// attractiveness for query `query`: lognormal with mean 1.
    pub fn taste(&self, group: u32, query: u32, row: u32) -> f64 {
        if self.shape.taste == 0.0 {
            return 1.0;
        }
        let mut h = Rng::seeded(self.seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ ((group as u64) << 48) ^ ((query as u64) << 24) ^ row as u64);
        let z = normal(&mut h);
        let s = self.shape.taste;
        (s * z - s * s / 2.0).exp()
    }
}

/// Walker's alias table: a draw in O(1) from fixed weights.
pub struct Alias {
    prob: Vec<f64>,
    alias: Vec<u32>,
}

impl Alias {
    pub fn new(w: &[f64]) -> Alias {
        let n = w.len();
        let total: f64 = w.iter().sum();
        let mut prob: Vec<f64> = w.iter().map(|x| x * n as f64 / total).collect();
        let mut alias = vec![0u32; n];
        let (mut small, mut large): (Vec<usize>, Vec<usize>) = (0..n).partition(|&i| prob[i] < 1.0);
        while let (Some(s), Some(&l)) = (small.pop(), large.last()) {
            alias[s] = l as u32;
            prob[l] -= 1.0 - prob[s];
            if prob[l] < 1.0 {
                large.pop();
                small.push(l);
            }
        }
        for i in large.into_iter().chain(small) {
            prob[i] = 1.0;
        }
        Alias { prob, alias }
    }

    pub fn draw(&self, rng: &mut Rng) -> usize {
        let i = rng.below(self.prob.len());
        if rng.uniform() < self.prob[i] {
            i
        } else {
            self.alias[i] as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_population_is_its_seed() {
        let a = Population::new(2000, 7, Shape::default());
        let b = Population::new(2000, 7, Shape::default());
        assert_eq!(a.users, b.users);
        assert_ne!(a.users, Population::new(2000, 8, Shape::default()).users);
        // Phones read with more position bias than desktops.
        let mean = |d: u8| {
            let xs: Vec<f64> = a.users.iter().filter(|u| u.device == d).map(|u| u.behaviour.eta).collect();
            xs.iter().sum::<f64>() / xs.len() as f64
        };
        assert!(mean(1) > mean(0) + 0.3);
        // A taste is the same each time, with mean 1.
        assert_eq!(a.taste(3, 9, 11), b.taste(3, 9, 11));
        let m: f64 = (0..20_000).map(|r| a.taste(5, 1, r)).sum::<f64>() / 20_000.0;
        assert!((m - 1.0).abs() < 0.02, "{m}");
    }

    #[test]
    fn the_alias_table_draws_its_weights() {
        let w = [1.0, 0.0, 3.0, 0.5, 5.5];
        let t = Alias::new(&w);
        let mut rng = Rng::seeded(1);
        let mut n = [0.0; 5];
        for _ in 0..200_000 {
            n[t.draw(&mut rng)] += 1.0;
        }
        for i in 0..5 {
            assert!((n[i] / 200_000.0 - w[i] / 10.0).abs() < 0.005, "{i}: {n:?}");
        }
    }
}
