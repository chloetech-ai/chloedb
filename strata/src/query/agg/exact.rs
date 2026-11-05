//! Exact sums of doubles, for `SUM` and `AVG` of FLOAT values.
//!
//! Every value is added into one fixed-point integer wide enough for any
//! double, from 2^-1074 to past 2^1024, held as signed 32-bit chunks in
//! 64-bit words so that an addition never carries at once. The sum is
//! rounded to a double once, to nearest with ties to even, when it is read.
//! So it is the same whatever order the values come in and however they
//! are split up and merged: segments summed on several threads, or shards'
//! partial sums, give the bits one pass over the rows gives.
//!
//! A sum holds a window of six chunks in place (a column's values sit
//! within a few chunks of each other) and moves to all 67 chunks on the
//! heap once its values need more.

/// Bits per chunk: chunk `i` is worth 2^(32 i - 1074).
const CHUNK: u32 = 32;
/// Chunks for any double (bit 0 is 2^-1074, a double's highest bit is bit
/// 2097) or 128-bit integer, three apart from the lowest bit of the value.
const CHUNKS: usize = 67;
/// Chunks held in place before a sum moves to the heap.
const WINDOW: usize = 6;
/// Additions between carries: each adds less than 2^32 to a chunk, so a
/// chunk stays far within 63 bits.
const CARRY_EVERY: u32 = 1 << 30;

#[derive(Clone, Debug, Default)]
pub struct ExactSum {
    /// The first chunk the window holds, and how many.
    lo: u8,
    len: u8,
    window: [i64; WINDOW],
    /// Every chunk, once the window is too narrow.
    wide: Option<Box<[i64; CHUNKS]>>,
    /// Additions since the chunks were last carried.
    adds: u32,
    nan: bool,
    pos_inf: bool,
    neg_inf: bool,
}

impl ExactSum {
    pub fn new() -> ExactSum {
        ExactSum::default()
    }

    /// Adds `x` exactly.
    #[inline]
    pub fn add(&mut self, x: f64) {
        let bits = x.to_bits();
        let neg = bits >> 63 == 1;
        let exp = ((bits >> 52) & 0x7ff) as usize;
        let frac = bits & ((1 << 52) - 1);
        if exp == 0x7ff {
            match (frac != 0, neg) {
                (true, _) => self.nan = true,
                (false, false) => self.pos_inf = true,
                (false, true) => self.neg_inf = true,
            }
            return;
        }
        if exp == 0 && frac == 0 {
            return;
        }
        // The value is `mant` times 2^low, counted in units of 2^-1074.
        let (mant, low) = if exp == 0 { (frac, 0) } else { (frac | 1 << 52, exp - 1) };
        let v = (mant as u128) << (low % CHUNK as usize);
        let mut cs = [(v & 0xffff_ffff) as i64, ((v >> 32) & 0xffff_ffff) as i64, (v >> 64) as i64];
        if neg {
            cs.iter_mut().for_each(|c| *c = -*c);
        }
        self.add_chunks(low / CHUNK as usize, &cs);
        self.counted();
    }

    /// Adds the integer `x` exactly.
    pub fn add_int(&mut self, x: i128) {
        if x == 0 {
            return;
        }
        // 1 is bit 1074: chunk 33, bit 18.
        let m = x.unsigned_abs();
        let mut cs = [0i64; 5];
        for (k, c) in cs.iter_mut().enumerate() {
            let at = 32 * k as i32 - 18;
            let part = if at < 0 { m << -at } else { m >> at };
            *c = (part & 0xffff_ffff) as i64;
            if x < 0 {
                *c = -*c;
            }
        }
        self.add_chunks(33, &cs);
        self.counted();
    }

    /// Adds another sum exactly.
    pub fn merge(&mut self, other: &ExactSum) {
        self.nan |= other.nan;
        self.pos_inf |= other.pos_inf;
        self.neg_inf |= other.neg_inf;
        let mut o = other.clone();
        o.carry();
        self.carry();
        for (i, c) in o.chunks() {
            if c != 0 {
                self.add_chunks(i, &[c]);
            }
        }
        self.adds = 2;
    }

    /// The sum, rounded once to the nearest double (ties to even); an
    /// infinity past the largest double, NaN for a NaN or both infinities.
    pub fn value(&self) -> f64 {
        if self.nan || (self.pos_inf && self.neg_inf) {
            return f64::NAN;
        }
        if self.pos_inf {
            return f64::INFINITY;
        }
        if self.neg_inf {
            return f64::NEG_INFINITY;
        }
        // Every chunk, one more for what carries out of the top.
        let mut a = [0i64; CHUNKS + 1];
        for (i, c) in self.chunks() {
            a[i] = c;
        }
        carry_all(&mut a);
        // Below the top every chunk is now in [0, 2^32): the top's sign is
        // the sum's.
        let neg = a[CHUNKS] < 0;
        if neg {
            a.iter_mut().for_each(|c| *c = -*c);
            carry_all(&mut a);
        }
        let Some(h) = a.iter().rposition(|&c| c != 0) else { return 0.0 };
        // The highest bit, in units of 2^-1074.
        let top = 32 * h + 63 - a[h].leading_zeros() as usize;
        let mag = if top >= 2098 {
            f64::INFINITY
        } else if top <= 52 {
            // At most 53 bits from 2^-1074: a subnormal or the lowest
            // exponent's normals, which the bits spell as they are.
            f64::from_bits((a[0] as u64) | (a[1] as u64) << 32)
        } else {
            let mut m = bits_at(&a, top - 52, 53);
            let half = bits_at(&a, top - 53, 1) == 1;
            let rest = below(&a, top - 53);
            let mut top = top;
            if half && (rest || m & 1 == 1) {
                m += 1;
                if m == 1 << 53 {
                    m >>= 1;
                    top += 1;
                }
            }
            let e = (top - 51) as u64;
            if e >= 0x7ff {
                f64::INFINITY
            } else {
                f64::from_bits(e << 52 | (m & ((1 << 52) - 1)))
            }
        };
        if neg {
            -mag
        } else {
            mag
        }
    }

    /// The sum plus the integer `x`, rounded once.
    pub fn value_plus(&self, x: i128) -> f64 {
        if x == 0 {
            return self.value();
        }
        let mut s = self.clone();
        s.add_int(x);
        s.value()
    }

    #[inline]
    fn counted(&mut self) {
        self.adds += 1;
        if self.adds >= CARRY_EVERY {
            self.carry();
        }
    }

    /// The chunks held, by their number.
    fn chunks(&self) -> Vec<(usize, i64)> {
        match &self.wide {
            Some(w) => w.iter().copied().enumerate().collect(),
            None => self.window[..self.len as usize].iter().enumerate().map(|(k, &c)| (self.lo as usize + k, c)).collect(),
        }
    }

    /// Adds `cs` to chunks `i`, `i + 1` and on.
    #[inline]
    fn add_chunks(&mut self, i: usize, cs: &[i64]) {
        if let Some(w) = &mut self.wide {
            for (k, &c) in cs.iter().enumerate() {
                w[i + k] += c;
            }
            return;
        }
        let (lo, len) = (self.lo as usize, self.len as usize);
        let (new_lo, new_hi) = if len == 0 { (i, i + cs.len()) } else { (lo.min(i), (lo + len).max(i + cs.len())) };
        if new_hi - new_lo > WINDOW {
            let mut w = Box::new([0i64; CHUNKS]);
            w[lo..lo + len].copy_from_slice(&self.window[..len]);
            for (k, &c) in cs.iter().enumerate() {
                w[i + k] += c;
            }
            self.wide = Some(w);
            self.window = [0; WINDOW];
            self.len = 0;
            return;
        }
        if len > 0 && new_lo < lo {
            let d = lo - new_lo;
            self.window.copy_within(0..len, d);
            self.window[..d].fill(0);
        }
        self.lo = new_lo as u8;
        self.len = (new_hi - new_lo) as u8;
        for (k, &c) in cs.iter().enumerate() {
            self.window[i - new_lo + k] += c;
        }
    }

    /// Carries each chunk's excess into the next, below the highest held:
    /// the chunks then lie in [0, 2^32), the highest keeping the sign.
    fn carry(&mut self) {
        self.adds = 0;
        let cs: &mut [i64] = match &mut self.wide {
            Some(w) => &mut w[..],
            None => &mut self.window[..self.len as usize],
        };
        for k in 0..cs.len().saturating_sub(1) {
            let c = cs[k] >> CHUNK;
            cs[k] -= c << CHUNK;
            cs[k + 1] += c;
        }
    }
}

/// Carries every chunk into the next, up to the last.
fn carry_all(a: &mut [i64]) {
    for k in 0..a.len() - 1 {
        let c = a[k] >> CHUNK;
        a[k] -= c << CHUNK;
        a[k + 1] += c;
    }
}

/// `n` (at most 64) bits of the carried chunks from bit `at`.
fn bits_at(a: &[i64], at: usize, n: u32) -> u64 {
    let (q, off) = (at / 32, at % 32);
    let word = |k: usize| a.get(k).map_or(0u128, |&c| c as u64 as u128);
    let v = (word(q) | word(q + 1) << 32 | word(q + 2) << 64 | word(q + 3) << 96) >> off;
    (v & ((1u128 << n) - 1)) as u64
}

/// Whether any bit below bit `at` is set.
fn below(a: &[i64], at: usize) -> bool {
    let (q, off) = (at / 32, at % 32);
    a[..q].iter().any(|&c| c != 0) || (off > 0 && (a[q] as u64) & ((1u64 << off) - 1) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(xs: &[f64]) -> f64 {
        let mut s = ExactSum::new();
        xs.iter().for_each(|&x| s.add(x));
        s.value()
    }

    /// Python's `math.fsum` (Shewchuk's partials, then a correctly rounded
    /// total): an independent exact sum to check against, for values whose
    /// partial sums stay finite.
    fn fsum(xs: &[f64]) -> f64 {
        let mut partials: Vec<f64> = Vec::new();
        for &x in xs {
            let mut x = x;
            let mut i = 0;
            for j in 0..partials.len() {
                let mut y = partials[j];
                if x.abs() < y.abs() {
                    std::mem::swap(&mut x, &mut y);
                }
                let hi = x + y;
                let lo = y - (hi - x);
                if lo != 0.0 {
                    partials[i] = lo;
                    i += 1;
                }
                x = hi;
            }
            partials.truncate(i);
            partials.push(x);
        }
        let mut n = partials.len();
        if n == 0 {
            return 0.0;
        }
        n -= 1;
        let mut hi = partials[n];
        let mut lo = 0.0;
        while n > 0 {
            n -= 1;
            let x = hi;
            let y = partials[n];
            hi = x + y;
            let yr = hi - x;
            lo = y - yr;
            if lo != 0.0 {
                break;
            }
        }
        if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
            let y = lo * 2.0;
            let x = hi + y;
            if y == x - hi {
                hi = x;
            }
        }
        hi
    }

    /// A small deterministic generator (xorshift).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// A double of any sign and of exponents from 2^-1074 to 2^lim.
        fn double(&mut self, lim: i32) -> f64 {
            loop {
                let x = f64::from_bits(self.next());
                if x.is_finite() && x.abs() < 2f64.powi(lim) {
                    return x;
                }
            }
        }
    }

    #[test]
    fn cancels_and_rounds_once() {
        assert_eq!(sum(&[1e100, 1.0, -1e100]), 1.0);
        // Ten 0.1s are 1.0000000000000000555...: 1.0 once rounded.
        assert_eq!(sum(&[0.1; 10]), 1.0);
        assert_ne!([0.1; 10].iter().sum::<f64>(), 1.0);
        assert_eq!(sum(&[]), 0.0);
        assert_eq!(sum(&[-0.0]).to_bits(), 0.0f64.to_bits());
        assert_eq!(sum(&[2.5, -2.5]), 0.0);
        assert_eq!(sum(&[-3.25, 1.0]), -2.25);
    }

    #[test]
    fn extremes() {
        assert_eq!(sum(&[f64::MAX, f64::MAX]), f64::INFINITY);
        assert_eq!(sum(&[-f64::MAX, -f64::MAX]), f64::NEG_INFINITY);
        // The running sum passes the largest double, the total does not.
        assert_eq!(sum(&[f64::MAX, f64::MAX, -f64::MAX]), f64::MAX);
        let tiny = f64::from_bits(1);
        assert_eq!(sum(&[tiny, tiny]), f64::from_bits(2));
        assert_eq!(sum(&[f64::MIN_POSITIVE, -tiny]), f64::MIN_POSITIVE - tiny);
        assert_eq!(sum(&[1e308, 1e-308, -1e308]), 1e-308);
        assert!(sum(&[1.0, f64::NAN]).is_nan());
        assert!(sum(&[f64::INFINITY, f64::NEG_INFINITY]).is_nan());
        assert_eq!(sum(&[f64::INFINITY, 1.0]), f64::INFINITY);
        assert_eq!(sum(&[f64::NEG_INFINITY, f64::MAX]), f64::NEG_INFINITY);
        // Ties round to even: 2^53 + 1 is halfway between 2^53 and 2^53 + 2.
        let two53 = 2f64.powi(53);
        assert_eq!(sum(&[two53, 1.0]), two53);
        assert_eq!(sum(&[two53, 1.0, 1.0]), two53 + 2.0);
        assert_eq!(sum(&[two53 + 2.0, 1.0]), two53 + 4.0);
        // Just over a half rounds up.
        assert_eq!(sum(&[two53, 1.0, tiny]), two53 + 2.0);
    }

    #[test]
    fn integers_exactly() {
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..200 {
            let mut s = ExactSum::new();
            let mut exact: i128 = 0;
            for _ in 0..50 {
                let x = (r.next() as i64 >> (r.next() % 60)) as i128;
                if r.next() % 2 == 0 {
                    s.add_int(x);
                } else {
                    // An integer below 2^53 is a double exactly.
                    let x = x >> 11;
                    s.add(x as f64);
                    exact += x;
                    continue;
                }
                exact += x;
            }
            assert_eq!(s.value().to_bits(), (exact as f64).to_bits());
            let big = i128::MAX >> 3;
            assert_eq!(s.value_plus(big).to_bits(), ((exact + big) as f64).to_bits());
        }
    }

    #[test]
    fn as_fsum_in_any_order_and_split() {
        let mut r = Rng(42);
        for round in 0..300 {
            let n = 1 + (r.next() % 200) as usize;
            // Some rounds spread over every exponent (the window moves to
            // the heap), some keep near each other (it stays in place).
            let lim = if round % 3 == 0 { 1000 } else { 20 };
            let mut xs: Vec<f64> = (0..n).map(|_| r.double(lim)).collect();
            if round % 3 == 1 {
                xs.iter_mut().for_each(|x| *x = x.abs().max(1e-3).min(1e6).copysign(*x));
            }
            let want = fsum(&xs);
            assert_eq!(sum(&xs).to_bits(), want.to_bits(), "round {round}: {xs:?}");
            // Shuffled, then split into parts summed apart and merged in
            // another order.
            for k in (1..xs.len()).rev() {
                xs.swap(k, (r.next() % (k as u64 + 1)) as usize);
            }
            let parts = 1 + (r.next() % 5) as usize;
            let mut sums: Vec<ExactSum> = (0..parts).map(|_| ExactSum::new()).collect();
            for (i, &x) in xs.iter().enumerate() {
                sums[i % parts].add(x);
            }
            let mut total = ExactSum::new();
            for s in sums.iter().rev() {
                total.merge(s);
            }
            assert_eq!(total.value().to_bits(), want.to_bits(), "round {round} merged");
        }
    }

    #[test]
    fn carries_keep_the_sum() {
        // 2 - 2^-52 and -0.5, added many times, carried every thousand
        // additions in one sum and only when read in the other.
        let x = 1.9999999999999998;
        let (mut s, mut t) = (ExactSum::new(), ExactSum::new());
        let n: i128 = 300_000;
        for i in 0..n {
            for v in [x, -0.5] {
                s.add(v);
                t.add(v);
            }
            if i % 1000 == 999 {
                s.carry();
            }
        }
        // x is (2^53 - 1) 2^-52: the sum is (n (2^53 - 1) - n 2^51) 2^-52.
        let mut units = ExactSum::new();
        units.add_int(n * ((1i128 << 53) - 1) - n * (1 << 51));
        let want = units.value() / 2f64.powi(52);
        assert_eq!(s.value(), want);
        assert_eq!(t.value(), want);
    }
}
