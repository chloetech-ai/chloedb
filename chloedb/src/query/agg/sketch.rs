//! Approximate percentiles (`approx_percentile(x, p)`): a DDSketch
//! (Masson, Rim and Lee, VLDB 2019) of a group's values, whose answer is
//! within 1% of the true value at that rank (relative error, which is what
//! a latency's tail needs), in a few KB per group however many rows it
//! adds, and which sketches of the same values in any split add up to
//! exactly ([`Sketch::absorb`]): segments and shards merge without error.
//!
//! A value x > 0 counts in bucket ⌈log_γ x⌉, γ = (1 + α) / (1 − α), α =
//! 0.01: the bucket of index i holds (γ^(i−1), γ^i], and its value,
//! 2γ^i / (γ + 1), is within α of every value in it. Negative values count
//! by their magnitude in buckets of their own, and values too near 0 for
//! a bucket in one of zero. A group keeps at most [`MAX_BINS`] buckets a
//! sign: past that its least magnitudes are folded into one bucket, so the
//! bound holds for the values at least γ^MAX_BINS (about 10^8.8) times the
//! least one's, as a latency's spread is. The least and greatest values
//! are kept exactly, and the answer stays between them.
//!
//! Shards send a sketch as bytes, in [`Sketch::to_bytes`]'s versioned
//! form, base64 in a text value; [`Sketch::from_bytes`] refuses a version
//! it does not read rather than misread it.

use base64::Engine;

use crate::value::Value;

/// The relative error of a sketch's answers.
pub const ALPHA: f64 = 0.01;

/// The most buckets a sketch keeps of each sign: 1024 counts of 4 bytes,
/// 4 KB, unless one counts past 2^32 − 1 rows.
pub const MAX_BINS: usize = 1024;

/// The version of [`Sketch::to_bytes`]'s form this version writes and reads.
pub const VERSION: u8 = 1;

/// A text partial's prefix: the form and its version, so that a reader
/// tells it from other text before decoding.
const PREFIX: &str = "ddsketch1:";

/// Counts of consecutive buckets: 4 bytes each until one would pass
/// `u32::MAX`, then 8.
#[derive(Clone, Debug, PartialEq)]
enum Bins {
    Narrow(Vec<u32>),
    Wide(Vec<u64>),
}

impl Bins {
    fn len(&self) -> usize {
        match self {
            Bins::Narrow(b) => b.len(),
            Bins::Wide(b) => b.len(),
        }
    }

    fn get(&self, i: usize) -> u64 {
        match self {
            Bins::Narrow(b) => b[i] as u64,
            Bins::Wide(b) => b[i],
        }
    }

    fn add(&mut self, i: usize, n: u64) {
        if let Bins::Narrow(b) = self {
            match (b[i] as u64).checked_add(n).filter(|&s| s <= u32::MAX as u64) {
                Some(s) => {
                    b[i] = s as u32;
                    return;
                }
                None => *self = Bins::Wide(b.iter().map(|&c| c as u64).collect()),
            }
        }
        if let Bins::Wide(b) = self {
            b[i] += n;
        }
    }

    /// `n` empty buckets before the first.
    fn prepend(&mut self, n: usize) {
        match self {
            Bins::Narrow(b) => {
                b.splice(0..0, std::iter::repeat_n(0, n));
            }
            Bins::Wide(b) => {
                b.splice(0..0, std::iter::repeat_n(0, n));
            }
        }
    }

    /// `n` empty buckets after the last: room for twice as many, up to
    /// [`MAX_BINS`], so that a sketch's bytes stay within its bound.
    fn append(&mut self, n: usize) {
        fn grow<T: Clone + Default>(b: &mut Vec<T>, n: usize) {
            let want = b.len() + n;
            if want > b.capacity() {
                let room = (2 * b.len()).clamp(8, MAX_BINS).max(want);
                b.reserve_exact(room - b.len());
            }
            b.resize(want, T::default());
        }
        match self {
            Bins::Narrow(b) => grow(b, n),
            Bins::Wide(b) => grow(b, n),
        }
    }

    /// Folds the first `n` buckets into the one after them.
    fn fold_first(&mut self, n: usize) {
        let folded: u64 = (0..=n).map(|i| self.get(i)).sum();
        match self {
            Bins::Narrow(b) => {
                b.drain(..n);
            }
            Bins::Wide(b) => {
                b.drain(..n);
            }
        }
        // The first bucket's count is now the sum, which may need 8 bytes.
        let now = self.get(0);
        self.add(0, folded - now);
    }

    fn bytes(&self) -> u64 {
        match self {
            Bins::Narrow(b) => 4 * b.capacity() as u64,
            Bins::Wide(b) => 8 * b.capacity() as u64,
        }
    }
}

/// The buckets of one sign: counts of the buckets from index `first` on.
#[derive(Clone, Debug, PartialEq)]
struct Store {
    first: i32,
    bins: Bins,
}

impl Store {
    fn new() -> Store {
        Store { first: 0, bins: Bins::Narrow(Vec::new()) }
    }

    fn is_empty(&self) -> bool {
        self.bins.len() == 0
    }

    fn total(&self) -> u64 {
        (0..self.bins.len()).map(|i| self.bins.get(i)).sum()
    }

    /// Counts `n` in bucket `i`: below the least bucket kept, in it once
    /// [`MAX_BINS`] are kept; above the greatest, after folding the least
    /// into one, so that at most [`MAX_BINS`] are kept.
    fn add(&mut self, i: i32, n: u64) {
        if self.is_empty() {
            self.first = i;
            self.bins.append(1);
        }
        let last = self.first + self.bins.len() as i32 - 1;
        if i > last {
            let need = i as i64 - self.first as i64 + 1;
            if need > MAX_BINS as i64 {
                self.fold_below(i - MAX_BINS as i32 + 1);
            }
            let last = self.first + self.bins.len() as i32 - 1;
            self.bins.append((i - last) as usize);
        } else if i < self.first {
            let room = MAX_BINS.saturating_sub(self.bins.len());
            let grow = ((self.first as i64 - i as i64) as usize).min(room);
            self.bins.prepend(grow);
            self.first -= grow as i32;
        }
        let at = (i.max(self.first) - self.first) as usize;
        self.bins.add(at, n);
    }

    /// Folds the buckets below index `to` into bucket `to`, which becomes
    /// the least kept.
    fn fold_below(&mut self, to: i32) {
        if to <= self.first {
            return;
        }
        let k = (to as i64 - self.first as i64) as usize;
        if k < self.bins.len() {
            self.bins.fold_first(k);
        } else {
            let total = self.total();
            self.bins = Bins::Narrow(vec![0]);
            self.bins.add(0, total);
        }
        self.first = to;
    }

    /// Each bucket with values: its index and count, least index first.
    fn each(&self) -> impl DoubleEndedIterator<Item = (i32, u64)> + '_ {
        (0..self.bins.len()).map(|j| (self.first + j as i32, self.bins.get(j))).filter(|&(_, n)| n > 0)
    }
}

/// One approximate percentile's sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    p: f64,
    pos: Store,
    neg: Store,
    zeros: u64,
    count: u64,
    min: f64,
    max: f64,
}

/// γ of [`ALPHA`] and its log.
fn gamma() -> (f64, f64) {
    let g = (1.0 + ALPHA) / (1.0 - ALPHA);
    (g, g.ln())
}

/// The least magnitude with a bucket: below it, a value counts as 0.
const MIN_MAGNITUDE: f64 = 1e-300;

/// The greatest bucket index's magnitude: of f64::MAX, about 35,800, and
/// of [`MIN_MAGNITUDE`] about −34,900. A sketch's bytes with an index
/// beyond it are damaged.
const MAX_INDEX: i32 = 40_000;

impl Sketch {
    pub fn new(p: f64) -> Sketch {
        Sketch { p, pos: Store::new(), neg: Store::new(), zeros: 0, count: 0, min: f64::INFINITY, max: f64::NEG_INFINITY }
    }

    /// The bucket of a magnitude at least [`MIN_MAGNITUDE`].
    #[inline]
    fn index(x: f64) -> i32 {
        (x.ln() / gamma().1).ceil() as i32
    }

    /// The value a bucket answers.
    fn bucket_value(i: i32) -> f64 {
        let (g, ln_g) = gamma();
        2.0 * (i as f64 * ln_g).exp() / (g + 1.0)
    }

    /// Adds one number; NaN and the infinities add nothing.
    #[inline]
    pub fn add_f64(&mut self, x: f64) {
        if !x.is_finite() {
            return;
        }
        self.count += 1;
        self.min = self.min.min(x);
        self.max = self.max.max(x);
        if x >= MIN_MAGNITUDE {
            self.pos.add(Sketch::index(x), 1);
        } else if x <= -MIN_MAGNITUDE {
            self.neg.add(Sketch::index(-x), 1);
        } else {
            self.zeros += 1;
        }
    }

    /// Adds one value (numbers; NULL and the rest add nothing); returns the
    /// bytes it now holds more.
    pub fn add(&mut self, v: &Value) -> u64 {
        let x = match v {
            Value::Int(x) => *x as f64,
            Value::Float(x) => *x,
            Value::Bool(b) => *b as i64 as f64,
            _ => return 0,
        };
        let before = self.bytes();
        self.add_f64(x);
        self.bytes().saturating_sub(before)
    }

    /// About the bytes the sketch holds.
    pub fn bytes(&self) -> u64 {
        self.pos.bins.bytes() + self.neg.bins.bytes()
    }

    /// Adds another sketch of the same percentile: the sketch of both's
    /// values.
    pub fn absorb(&mut self, other: Sketch) {
        if other.count == 0 {
            return;
        }
        for (i, n) in other.pos.each() {
            self.pos.add(i, n);
        }
        for (i, n) in other.neg.each() {
            self.neg.add(i, n);
        }
        self.zeros += other.zeros;
        self.count += other.count;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    /// The value at fraction `p` of the values' ranks (FLOAT), NULL for no
    /// values: the bucket of the value at rank ⌊p·(n − 1)⌋ from 0, within
    /// [`ALPHA`] of it, and between the least and greatest values.
    pub fn value(&self) -> Value {
        if self.count == 0 {
            return Value::Null;
        }
        if self.p <= 0.0 {
            return Value::Float(self.min);
        }
        if self.p >= 1.0 {
            return Value::Float(self.max);
        }
        let rank = (self.p * (self.count - 1) as f64).floor() as u64;
        let mut seen = 0u64;
        let x = 'found: {
            // Negatives from the greatest magnitude, then zeros, then positives.
            for (i, n) in self.neg.each().rev() {
                seen += n;
                if seen > rank {
                    break 'found -Sketch::bucket_value(i);
                }
            }
            seen += self.zeros;
            if seen > rank {
                break 'found 0.0;
            }
            for (i, n) in self.pos.each() {
                seen += n;
                if seen > rank {
                    break 'found Sketch::bucket_value(i);
                }
            }
            self.max
        };
        Value::Float(x.clamp(self.min, self.max))
    }

    /// The sketch in its versioned form: a version byte ([`VERSION`]), α
    /// (f64, little-endian), then as LEB128 varints the count of zeros,
    /// the least and greatest values (f64 bits), and each sign's store,
    /// positive then negative: its first index (zigzag), its number of
    /// buckets, and their counts. The fraction is the query's, not sent.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.pos.bins.len() + self.neg.bins.len());
        out.push(VERSION);
        out.extend_from_slice(&ALPHA.to_le_bytes());
        varint(&mut out, self.zeros);
        out.extend_from_slice(&self.min.to_le_bytes());
        out.extend_from_slice(&self.max.to_le_bytes());
        for s in [&self.pos, &self.neg] {
            varint(&mut out, zigzag(s.first));
            varint(&mut out, s.bins.len() as u64);
            for j in 0..s.bins.len() {
                varint(&mut out, s.bins.get(j));
            }
        }
        out
    }

    /// A sketch of percentile `p` from [`Sketch::to_bytes`]'s form, or why
    /// it is not one this version reads.
    pub fn from_bytes(p: f64, b: &[u8]) -> Result<Sketch, String> {
        let mut r = Reader { b, at: 0 };
        let bad = || "a sketch's bytes are cut short or damaged".to_string();
        let version = r.byte().ok_or_else(bad)?;
        if version != VERSION {
            return Err(format!("a sketch of version {version}, which this version of ChloeDB does not read (it reads version {VERSION}): it may be newer"));
        }
        let alpha = f64::from_le_bytes(r.take(8).ok_or_else(bad)?.try_into().expect("8 bytes"));
        if alpha != ALPHA {
            return Err(format!("a sketch of relative error {alpha}, which this version of ChloeDB does not merge with its own ({ALPHA})"));
        }
        let zeros = r.varint().ok_or_else(bad)?;
        let min = f64::from_le_bytes(r.take(8).ok_or_else(bad)?.try_into().expect("8 bytes"));
        let max = f64::from_le_bytes(r.take(8).ok_or_else(bad)?.try_into().expect("8 bytes"));
        let mut s = Sketch::new(p);
        s.zeros = zeros;
        let mut count = zeros;
        for neg in [false, true] {
            let first = unzigzag(r.varint().ok_or_else(bad)?).ok_or_else(bad)?;
            let n = r.varint().ok_or_else(bad)?;
            if n > MAX_BINS as u64 || first.unsigned_abs() > MAX_INDEX as u32 || (first + n as i32).unsigned_abs() > MAX_INDEX as u32 {
                return Err(bad());
            }
            let store = if neg { &mut s.neg } else { &mut s.pos };
            for j in 0..n as i32 {
                let c = r.varint().ok_or_else(bad)?;
                if c > 0 {
                    store.add(first + j, c);
                    count = count.checked_add(c).ok_or_else(bad)?;
                }
            }
        }
        if r.at != b.len() || (count > 0 && !(min <= max && min.is_finite() && max.is_finite())) {
            return Err(bad());
        }
        s.count = count;
        if count > 0 {
            (s.min, s.max) = (min, max);
        }
        Ok(s)
    }

    /// The sketch as a shard sends it: [`Sketch::to_bytes`] in base64, as
    /// text that names its form.
    pub fn partial(&self) -> Value {
        Value::Str(format!("{PREFIX}{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(self.to_bytes())))
    }

    /// Adds a shard's [`Sketch::partial`]; why not, if it is not one.
    pub fn merge(&mut self, v: &Value) -> Result<(), String> {
        let Value::Str(s) = v else { return Err("a shard's approximate percentile is not a sketch".into()) };
        let Some(b64) = s.strip_prefix(PREFIX) else {
            return Err(match s.split_once(':') {
                Some((form, _)) if form.starts_with("ddsketch") => format!("a shard's sketch is of form {form}, which this version of ChloeDB does not read: it may be newer"),
                _ => "a shard's approximate percentile is not a sketch".into(),
            });
        };
        let b = base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64).map_err(|_| "a shard's sketch is not base64".to_string())?;
        self.absorb(Sketch::from_bytes(self.p, &b)?);
        Ok(())
    }
}

fn varint(out: &mut Vec<u8>, mut x: u64) {
    while x >= 0x80 {
        out.push(x as u8 | 0x80);
        x >>= 7;
    }
    out.push(x as u8);
}

fn zigzag(x: i32) -> u64 {
    ((x << 1) ^ (x >> 31)) as u32 as u64
}

fn unzigzag(x: u64) -> Option<i32> {
    let x = u32::try_from(x).ok()?;
    Some(((x >> 1) as i32) ^ -((x & 1) as i32))
}

struct Reader<'b> {
    b: &'b [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let x = *self.b.get(self.at)?;
        self.at += 1;
        Some(x)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut x = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            x |= ((b & 0x7f) as u64).checked_shl(shift)?;
            if b & 0x80 == 0 {
                return Some(x);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    use super::*;

    /// The exact value at rank ⌊p·(n − 1)⌋ of `xs`, in order.
    fn exact(xs: &mut [f64], p: f64) -> f64 {
        xs.sort_by(f64::total_cmp);
        xs[(p * (xs.len() - 1) as f64).floor() as usize]
    }

    fn of(p: f64, xs: &[f64]) -> Sketch {
        let mut s = Sketch::new(p);
        xs.iter().for_each(|&x| s.add_f64(x));
        s
    }

    fn near(got: f64, want: f64) -> bool {
        (got - want).abs() <= ALPHA * want.abs() + 1e-12
    }

    #[test]
    fn answers_are_within_the_relative_error() {
        let mut rng = StdRng::seed_from_u64(7);
        // Latencies with a long tail, values across signs, a narrow band, and ints.
        let sets: Vec<Vec<f64>> = vec![
            (0..50_000).map(|_| (rng.gen::<f64>() * 6.0).exp() * 100.0 + 50.0).collect(),
            (0..20_000).map(|_| rng.gen_range(-1e6..1e6)).collect(),
            (0..10_000).map(|_| 1000.0 + rng.gen::<f64>()).collect(),
            (0..10_000).map(|_| rng.gen_range(0..50) as f64).collect(),
            vec![42.0],
        ];
        for xs in sets {
            for p in [0.0, 0.01, 0.25, 0.5, 0.9, 0.95, 0.99, 0.999, 1.0] {
                let s = of(p, &xs);
                let Value::Float(got) = s.value() else { panic!("a value") };
                let want = exact(&mut xs.clone(), p);
                assert!(near(got, want), "p {p}: {got} for {want}");
                assert!(s.bytes() <= 4 * MAX_BINS as u64 * 2);
            }
        }
        assert_eq!(Sketch::new(0.5).value(), Value::Null);
    }

    #[test]
    fn merged_sketches_are_the_sketch_of_all_the_values() {
        let mut rng = StdRng::seed_from_u64(9);
        let xs: Vec<f64> = (0..30_000).map(|_| (rng.gen::<f64>() * 10.0).exp() - 3.0).collect();
        let whole = of(0.99, &xs);
        for parts in [2, 3, 17] {
            let mut merged = Sketch::new(0.99);
            for chunk in xs.chunks(xs.len() / parts + 1) {
                merged.absorb(of(0.99, chunk));
            }
            assert_eq!(merged, whole, "{parts} parts");
            // And through a shard's bytes.
            let mut by_bytes = Sketch::new(0.99);
            for chunk in xs.chunks(xs.len() / parts + 1) {
                by_bytes.merge(&of(0.99, chunk).partial()).unwrap();
            }
            assert_eq!(by_bytes.value(), whole.value());
        }
    }

    #[test]
    fn a_sketch_keeps_at_most_max_bins_a_sign() {
        let mut s = Sketch::new(0.99);
        // Magnitudes from 1e-200 to 1e200: far more buckets than kept.
        for e in -200..=200 {
            for k in 1..10 {
                s.add_f64(k as f64 * 10f64.powi(e));
            }
        }
        assert!(s.pos.bins.len() <= MAX_BINS);
        assert!(s.bytes() <= 4 * MAX_BINS as u64 + 64);
        // The tail is still within the bound; the least values were folded.
        let Value::Float(got) = s.value() else { panic!() };
        let mut xs: Vec<f64> = (-200..=200).flat_map(|e| (1..10).map(move |k| k as f64 * 10f64.powi(e))).collect();
        assert!(near(got, exact(&mut xs, 0.99)));
    }

    #[test]
    fn counts_past_four_bytes_widen() {
        let mut s = Sketch::new(0.5);
        s.pos.add(5, u32::MAX as u64);
        s.pos.add(5, 2);
        s.pos.add(7, 1);
        assert_eq!(s.pos.bins, Bins::Wide(vec![u32::MAX as u64 + 2, 0, 1]));
    }

    #[test]
    fn bytes_are_versioned_and_checked() {
        let s = of(0.5, &[1.0, -2.5, 0.0, 1e9]);
        let b = s.to_bytes();
        assert_eq!(b[0], VERSION);
        assert_eq!(Sketch::from_bytes(0.5, &b).unwrap(), s);
        let mut newer = b.clone();
        newer[0] = 2;
        assert!(Sketch::from_bytes(0.5, &newer).unwrap_err().contains("version 2"));
        for cut in 0..b.len() {
            assert!(Sketch::from_bytes(0.5, &b[..cut]).is_err(), "cut at {cut}");
        }
        let mut longer = b.clone();
        longer.push(0);
        assert!(Sketch::from_bytes(0.5, &longer).is_err());
        let mut m = Sketch::new(0.5);
        assert!(m.merge(&Value::Str("ddsketch2:AAAA".into())).unwrap_err().contains("ddsketch2"));
        assert!(m.merge(&Value::Int(3)).is_err());
        // Arbitrary bytes never panic.
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..2000 {
            let n = rng.gen_range(0..64);
            let mut junk: Vec<u8> = (0..n).map(|_| rng.gen()).collect();
            if !junk.is_empty() {
                junk[0] = VERSION;
            }
            let _ = Sketch::from_bytes(0.5, &junk);
        }
    }
}
