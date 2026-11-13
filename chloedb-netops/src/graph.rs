//! The topology walked in the app: the same breadth-first search and
//! personalized PageRank that ChloeDB runs as `REACHABLE` and `ppr` over the
//! links table. The app uses these to group alerts every minute, and as the
//! fallback when the server is a coordinator, which does not walk graphs
//! yet. An agent's step says which one answered.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::world::World;

pub struct Graph {
    pub ids: Vec<String>,
    index: HashMap<String, usize>,
    adj: Vec<Vec<usize>>,
}

impl Graph {
    /// The inventory's links (what `netops_links` holds), live or not.
    pub fn of(w: &World) -> Graph {
        let ids: Vec<String> = w.devices.iter().map(|d| d.id.clone()).collect();
        let index = ids.iter().enumerate().map(|(i, d)| (d.clone(), i)).collect();
        let mut adj = vec![Vec::new(); ids.len()];
        for l in w.links.iter().filter(|l| l.documented) {
            adj[l.a].push(l.b);
            adj[l.b].push(l.a);
        }
        Graph { ids, index, adj }
    }

    /// The devices within `hops` of the seeds, with their distance; the
    /// seeds themselves are left out, as `REACHABLE` leaves them.
    pub fn reach(&self, seeds: &[String], hops: usize) -> Vec<(String, usize)> {
        let mut dist: HashMap<usize, usize> = HashMap::new();
        let mut queue = VecDeque::new();
        for s in seeds {
            if let Some(&i) = self.index.get(s) {
                dist.insert(i, 0);
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let d = dist[&i];
            if d == hops {
                continue;
            }
            for &j in &self.adj[i] {
                if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(j) {
                    e.insert(d + 1);
                    queue.push_back(j);
                }
            }
        }
        let mut out: Vec<(String, usize)> = dist.into_iter().filter(|&(_, d)| d > 0).map(|(i, d)| (self.ids[i].clone(), d)).collect();
        out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    /// The seeds and everything within `hops` of them.
    pub fn ball(&self, seeds: &[String], hops: usize) -> HashSet<String> {
        let mut out: HashSet<String> = seeds.iter().cloned().collect();
        out.extend(self.reach(seeds, hops).into_iter().map(|x| x.0));
        out
    }

    /// Personalized PageRank from the seeds by power iteration (restart
    /// 0.15), best first.
    pub fn ppr(&self, seeds: &[String]) -> Vec<(String, f64)> {
        let n = self.ids.len();
        let s: Vec<usize> = seeds.iter().filter_map(|x| self.index.get(x).copied()).collect();
        if s.is_empty() {
            return Vec::new();
        }
        let mut restart = vec![0.0; n];
        for &i in &s {
            restart[i] += 1.0 / s.len() as f64;
        }
        let mut p = restart.clone();
        for _ in 0..40 {
            let mut next: Vec<f64> = restart.iter().map(|r| 0.15 * r).collect();
            for i in 0..n {
                if self.adj[i].is_empty() {
                    next[i] += 0.85 * p[i];
                    continue;
                }
                let share = 0.85 * p[i] / self.adj[i].len() as f64;
                for &j in &self.adj[i] {
                    next[j] += share;
                }
            }
            p = next;
        }
        let mut out: Vec<(String, f64)> = p.into_iter().enumerate().filter(|x| x.1 > 1e-6).map(|(i, v)| (self.ids[i].clone(), v)).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// One shortest path between two devices.
    pub fn path(&self, from: &str, to: &str) -> Vec<String> {
        let (Some(&a), Some(&b)) = (self.index.get(from), self.index.get(to)) else { return Vec::new() };
        let mut prev: HashMap<usize, usize> = HashMap::new();
        let mut queue = VecDeque::from([a]);
        prev.insert(a, a);
        while let Some(i) = queue.pop_front() {
            if i == b {
                break;
            }
            for &j in &self.adj[i] {
                if let std::collections::hash_map::Entry::Vacant(e) = prev.entry(j) {
                    e.insert(i);
                    queue.push_back(j);
                }
            }
        }
        if !prev.contains_key(&b) {
            return Vec::new();
        }
        let mut out = vec![b];
        while *out.last().unwrap() != a {
            out.push(prev[out.last().unwrap()]);
        }
        out.reverse();
        out.into_iter().map(|i| self.ids[i].clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_the_inventory() {
        let w = World::build();
        let g = Graph::of(&w);
        let near = g.reach(&["cr1-ams".to_string()], 1);
        assert!(near.iter().any(|x| x.0 == "cr2-ams"));
        assert!(near.iter().any(|x| x.0 == "cr1-fra"));
        assert!(near.iter().all(|x| x.1 == 1));
        let rank = g.ppr(&["cr1-ams".to_string(), "cr1-fra".to_string()]);
        assert!(rank[0].0 == "cr1-ams" || rank[0].0 == "cr1-fra");
        let path = g.path("lf1-ams", "lf1-fra");
        assert_eq!(path.first().map(String::as_str), Some("lf1-ams"));
        assert_eq!(path.last().map(String::as_str), Some("lf1-fra"));
        assert!(path.len() >= 4);
    }
}
