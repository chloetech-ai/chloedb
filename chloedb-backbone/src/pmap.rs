//! A map from prefixes to values that answers "which prefixes cover this
//! one", longest first: a hash map per address family and length, and the
//! lengths that hold any. RPKI's ROAs and the usual origins of each prefix
//! are kept in these, a million or so each, to check every update as it
//! arrives.

use rustc_hash::FxHashMap;

use crate::mrt::Prefix;

pub struct PrefixMap<T> {
    /// [v4 lengths 0..=32, v6 lengths 0..=128]
    by_len: Vec<FxHashMap<u128, T>>,
    lens4: Vec<u8>,
    lens6: Vec<u8>,
    n: usize,
}

impl<T> Default for PrefixMap<T> {
    fn default() -> Self {
        PrefixMap { by_len: (0..(33 + 129)).map(|_| FxHashMap::default()).collect(), lens4: Vec::new(), lens6: Vec::new(), n: 0 }
    }
}

fn slot(p: &Prefix) -> usize {
    if p.v6 {
        33 + p.len as usize
    } else {
        p.len as usize
    }
}

impl<T> PrefixMap<T> {
    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The value at `p`, made with `make` the first time.
    pub fn entry(&mut self, p: Prefix, make: impl FnOnce() -> T) -> &mut T {
        let p = p.masked();
        let lens = if p.v6 { &mut self.lens6 } else { &mut self.lens4 };
        if let Err(i) = lens.binary_search_by(|l| p.len.cmp(l)) {
            lens.insert(i, p.len);
        }
        let m = &mut self.by_len[slot(&p)];
        if !m.contains_key(&p.addr) {
            self.n += 1;
        }
        m.entry(p.addr).or_insert_with(make)
    }

    pub fn get(&self, p: &Prefix) -> Option<&T> {
        self.by_len[slot(p)].get(&p.masked().addr)
    }

    /// The prefixes that cover `p` (itself included), longest first.
    pub fn covering<'a>(&'a self, p: &Prefix) -> impl Iterator<Item = (Prefix, &'a T)> + 'a {
        let p = *p;
        let lens = if p.v6 { &self.lens6 } else { &self.lens4 };
        lens.iter().filter(move |l| **l <= p.len).filter_map(move |&l| {
            let q = Prefix { v6: p.v6, addr: p.addr, len: l }.masked();
            self.by_len[slot(&q)].get(&q.addr).map(|v| (q, v))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covering_is_longest_first() {
        let mut m = PrefixMap::default();
        *m.entry(Prefix::parse("84.255.184.0/21").unwrap(), || 0) = 21;
        *m.entry(Prefix::parse("84.255.184.0/22").unwrap(), || 0) = 22;
        *m.entry(Prefix::parse("10.0.0.0/8").unwrap(), || 0) = 8;
        let got: Vec<i32> = m.covering(&Prefix::parse("84.255.184.0/24").unwrap()).map(|(_, v)| *v).collect();
        assert_eq!(got, vec![22, 21]);
        assert_eq!(m.len(), 3);
        assert!(m.covering(&Prefix::parse("84.255.192.0/24").unwrap()).next().is_none());
    }
}
