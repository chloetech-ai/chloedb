//! Percentiles (`PERCENTILE_CONT`, `PERCENTILE_DISC`, `MEDIAN`), exactly:
//! a group keeps every value it adds, 8 bytes each, and orders them once
//! when it is read. Shards send their values as runs of equal ones.
//!
//! The answers are PostgreSQL's: of n values in order, `PERCENTILE_DISC(p)`
//! is the value at position ⌈p·n⌉ (counting from 1, and the first for p =
//! 0), and `PERCENTILE_CONT(p)` the value at position p·(n − 1) (from 0),
//! interpolated between the values at the positions below and above it,
//! in doubles.

use std::cmp::Ordering;

use super::cmp_int_float;
use crate::value::Value;

/// One percentile's values: the integers and the doubles apart.
#[derive(Clone, Debug)]
pub struct Quantile {
    p: f64,
    cont: bool,
    ints: Vec<i64>,
    floats: Vec<f64>,
    /// Whether every integer was a TIMESTAMP: a value of one is one.
    time: bool,
    /// Whether both lists are in order ([`Quantile::settle`]).
    sorted: bool,
}

/// A value of either list.
#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn f64(self) -> f64 {
        match self {
            Num::Int(x) => x as f64,
            Num::Float(x) => x,
        }
    }
}

impl Quantile {
    pub fn new(p: f64, cont: bool) -> Quantile {
        Quantile { p, cont, ints: Vec::new(), floats: Vec::new(), time: true, sorted: true }
    }

    /// Adds one value (NULL, text and lists add nothing); returns the bytes
    /// it now holds more.
    pub fn add(&mut self, v: &Value) -> u64 {
        match v {
            Value::Int(x) | Value::Timestamp(x) => {
                self.time &= matches!(v, Value::Timestamp(_));
                self.ints.push(*x);
            }
            Value::Bool(b) => {
                self.time = false;
                self.ints.push(*b as i64);
            }
            Value::Float(x) => self.floats.push(*x),
            _ => return 0,
        }
        self.sorted = false;
        8
    }

    /// The bytes the values hold.
    pub fn bytes(&self) -> u64 {
        8 * (self.ints.len() + self.floats.len()) as u64
    }

    /// Adds the values of another group of the same percentile.
    pub fn absorb(&mut self, other: Quantile) {
        if other.ints.is_empty() && other.floats.is_empty() {
            return;
        }
        if !other.ints.is_empty() {
            self.time &= other.time;
        }
        self.ints.extend(other.ints);
        self.floats.extend(other.floats);
        self.sorted = false;
    }

    /// Puts the values in order, so that reading the value is a lookup.
    pub fn settle(&mut self) {
        if !self.sorted {
            self.ints.sort_unstable();
            self.floats.sort_unstable_by(f64::total_cmp);
            self.sorted = true;
        }
    }

    /// The `k`-th value in order (from 0) of the settled lists.
    fn nth(&self, k: usize) -> Num {
        if self.floats.is_empty() {
            return Num::Int(self.ints[k]);
        }
        if self.ints.is_empty() {
            return Num::Float(self.floats[k]);
        }
        // Both: walk the two in order (rare, as a column has one type).
        let (mut i, mut j) = (0, 0);
        loop {
            let int_first = j == self.floats.len() || (i < self.ints.len() && cmp_int_float(self.ints[i], self.floats[j]) != Ordering::Greater);
            let x = if int_first {
                i += 1;
                Num::Int(self.ints[i - 1])
            } else {
                j += 1;
                Num::Float(self.floats[j - 1])
            };
            if i + j == k + 1 {
                return x;
            }
        }
    }

    /// The percentile: NULL without values.
    pub fn value(&self) -> Value {
        if !self.sorted {
            let mut q = self.clone();
            q.settle();
            return q.value();
        }
        let n = self.ints.len() + self.floats.len();
        if n == 0 {
            return Value::Null;
        }
        if self.cont {
            let at = self.p * (n - 1) as f64;
            let (lo, hi) = (at.floor(), at.ceil());
            let a = self.nth(lo as usize).f64();
            if hi == lo {
                return Value::Float(a);
            }
            let b = self.nth(hi as usize).f64();
            return Value::Float(a + (b - a) * (at - lo));
        }
        let k = ((self.p * n as f64).ceil() as usize).clamp(1, n) - 1;
        match self.nth(k) {
            Num::Int(x) if self.time => Value::Timestamp(x),
            Num::Int(x) => Value::Int(x),
            Num::Float(x) => Value::Float(x),
        }
    }

    /// The values for a shard's answer: `[values, counts]`, each distinct
    /// value once and in order, and how many times it was added.
    pub fn partial(&self) -> Value {
        if !self.sorted {
            let mut q = self.clone();
            q.settle();
            return q.partial();
        }
        let mut values = Vec::new();
        let mut counts = Vec::new();
        let mut run = |v: Value| {
            if values.last() == Some(&v) {
                if let Some(Value::Int(c)) = counts.last_mut() {
                    *c += 1;
                }
            } else {
                values.push(v);
                counts.push(Value::Int(1));
            }
        };
        self.ints.iter().for_each(|&x| run(Value::Int(x)));
        self.floats.iter().for_each(|&x| run(Value::Float(x)));
        Value::List(vec![Value::List(values), Value::List(counts)])
    }

    /// Adds a [`partial`](Self::partial); None if it is not one.
    pub fn merge(&mut self, p: &Value) -> Option<()> {
        let Value::List(xs) = p else { return None };
        let [Value::List(values), Value::List(counts)] = xs.as_slice() else { return None };
        if values.len() != counts.len() {
            return None;
        }
        for (v, c) in values.iter().zip(counts) {
            let c = usize::try_from(c.as_i64()?).ok()?;
            match v {
                Value::Int(x) => {
                    self.time = false;
                    self.ints.extend(std::iter::repeat_n(*x, c));
                }
                Value::Float(x) => self.floats.extend(std::iter::repeat_n(*x, c)),
                _ => return None,
            }
        }
        if !values.is_empty() {
            self.sorted = false;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(p: f64, cont: bool, values: &[Value]) -> Value {
        let mut q = Quantile::new(p, cont);
        values.iter().for_each(|v| _ = q.add(v));
        q.value()
    }

    fn ints(xs: &[i64]) -> Vec<Value> {
        xs.iter().map(|&x| Value::Int(x)).collect()
    }

    #[test]
    fn postgres_answers() {
        // SELECT percentile_cont(p) WITHIN GROUP (ORDER BY x), percentile_disc(p) ..
        // FROM unnest(ARRAY[10, 1, 4, 2]) x, as PostgreSQL 16 answers (8.2
        // is 8.200000000000001 there too: p·(n − 1) is a double).
        let xs = ints(&[10, 1, 4, 2]);
        assert_eq!(of(0.5, true, &xs), Value::Float(3.0));
        assert_eq!(of(0.5, false, &xs), Value::Int(2));
        assert_eq!(of(0.0, true, &xs), Value::Float(1.0));
        assert_eq!(of(0.0, false, &xs), Value::Int(1));
        assert_eq!(of(1.0, true, &xs), Value::Float(10.0));
        assert_eq!(of(1.0, false, &xs), Value::Int(10));
        assert_eq!(of(0.9, true, &xs), Value::Float(8.200000000000001));
        assert_eq!(of(0.9, false, &xs), Value::Int(10));
        assert_eq!(of(0.25, true, &xs), Value::Float(1.75));
        assert_eq!(of(0.25, false, &xs), Value::Int(1));
        // One value, none, NULLs.
        assert_eq!(of(0.3, true, &ints(&[7])), Value::Float(7.0));
        assert_eq!(of(0.3, false, &[]), Value::Null);
        assert_eq!(of(0.5, true, &[Value::Null, Value::Int(3), Value::Null]), Value::Float(3.0));
        // Doubles, and integers among them in their numeric order.
        assert_eq!(of(0.5, true, &[Value::Float(2.5), Value::Int(1), Value::Float(-1.0), Value::Int(4)]), Value::Float(1.75));
        assert_eq!(of(0.75, false, &[Value::Float(2.5), Value::Int(1), Value::Float(-1.0), Value::Int(4)]), Value::Float(2.5));
        // Times are times.
        assert_eq!(of(0.5, false, &[Value::Timestamp(5), Value::Timestamp(1), Value::Timestamp(9)]), Value::Timestamp(5));
    }

    #[test]
    fn partials_merge_to_the_whole() {
        let all = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        for (p, cont) in [(0.5, true), (0.5, false), (0.1, true), (0.9, false), (1.0, true)] {
            let whole = of(p, cont, &ints(&all));
            let (mut a, mut b) = (Quantile::new(p, cont), Quantile::new(p, cont));
            all[..4].iter().for_each(|&x| _ = a.add(&Value::Int(x)));
            all[4..].iter().for_each(|&x| _ = b.add(&Value::Int(x)));
            let mut m = Quantile::new(p, cont);
            m.merge(&a.partial()).unwrap();
            m.merge(&b.partial()).unwrap();
            assert_eq!(m.value(), whole, "{p} {cont}");
            let mut s = a.clone();
            s.absorb(b);
            assert_eq!(s.value(), whole, "{p} {cont}");
        }
        // Equal values travel once, with their count.
        let mut q = Quantile::new(0.5, true);
        [2, 2, 2, 7].iter().for_each(|&x| _ = q.add(&Value::Int(x)));
        assert_eq!(q.partial(), Value::List(vec![Value::List(ints(&[2, 7])), Value::List(ints(&[3, 1]))]));
        assert!(Quantile::new(0.5, true).merge(&Value::Int(3)).is_none());
    }
}
