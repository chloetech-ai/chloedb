//! One decode step of attention over a long KV cache on CPU: dense against an
//! IVF over the keys (the `cluster_cent` method of `kvsel.py`), for the shape
//! of an 8B model's layer (8 KV heads, 4 query heads each, head dim 128, bf16
//! K and V).
//!
//! The IVF follows `chloedb/src/vector/ivf.rs`: Lloyd's k-means on a sample of
//! the rows, then every row assigned to its nearest centroid, and the rows
//! copied into cluster order so that probing a cluster is one sequential read.
//! The differences are the ones attention needs: Euclidean k-means (a key's
//! norm matters to q·k, so centroids are means, not normalised), clusters of
//! about 16 keys (`ivf.rs` aims at thousands of rows per list), the attention
//! sink (the first 16 tokens) and the tokens appended since the index was
//! built (the tail) read densely, and whole clusters taken by `q·centroid`
//! until a token budget is spent. `ivf::build` is private to the crate and
//! works on segment rows, so this is a standalone copy of the method.
//!
//!   cargo run --release -- [--tokens 32768,131072] [--budget 0.0625] [--iters 8]
//!                          [--sample 1.0] [--tail 256] [--threads 4] [--steps 50]
//!                          [--keys FILE --values FILE]   (bf16, H x N x 128, from dump_kv.py)
use std::time::Instant;

const D: usize = 128; // head dim
const G: usize = 4; // query heads per KV head
const H: usize = 8; // KV heads
const SINK: usize = 16;
const LAYERS_8B: f64 = 32.0;

#[inline(always)]
fn bf(x: u16) -> f32 {
    f32::from_bits((x as u32) << 16)
}

fn to_bf(x: f32) -> u16 {
    let b = x.to_bits();
    ((b + 0x7fff + ((b >> 16) & 1)) >> 16) as u16
}

type Q = [[f32; D]; G];

/// `q_g · k` for the G query heads against one bf16 row, 16 lanes at a time
/// (each lane its own accumulator, so the loop vectorises without reassociating).
#[inline(always)]
fn dot_g(q: &Q, k: &[u16]) -> [f32; G] {
    let mut acc = [[0f32; 16]; G];
    for c in 0..D / 16 {
        let kk: [f32; 16] = std::array::from_fn(|l| bf(k[c * 16 + l]));
        for g in 0..G {
            for l in 0..16 {
                acc[g][l] += q[g][c * 16 + l] * kk[l];
            }
        }
    }
    acc.map(|a| a.iter().sum())
}

/// Online-softmax state of one KV head's G query heads.
struct State {
    m: [f32; G],
    l: [f32; G],
    acc: Box<Q>,
}

impl State {
    fn new() -> State {
        State { m: [f32::NEG_INFINITY; G], l: [0.0; G], acc: Box::new([[0.0; D]; G]) }
    }

    /// Folds in `rows` contiguous K and V rows.
    fn attend(&mut self, q: &Q, k: &[u16], v: &[u16], rows: usize) {
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx512f") {
            assert!(k.len() >= rows * D && v.len() >= rows * D);
            // SAFETY: AVX-512 is present; k and v hold `rows` rows of D.
            unsafe { avx::attend(self, q, k.as_ptr(), v.as_ptr(), rows) };
            return;
        }
        self.attend_scalar(q, k, v, rows)
    }

    fn attend_scalar(&mut self, q: &Q, k: &[u16], v: &[u16], rows: usize) {
        const B: usize = 32;
        let mut s = [[0f32; G]; B];
        let mut r0 = 0;
        while r0 < rows {
            let n = B.min(rows - r0);
            for r in 0..n {
                s[r] = dot_g(q, &k[(r0 + r) * D..(r0 + r + 1) * D]);
            }
            for g in 0..G {
                let bm = s[..n].iter().fold(f32::NEG_INFINITY, |a, x| a.max(x[g]));
                let m = self.m[g].max(bm);
                let c = (self.m[g] - m).exp();
                if c != 1.0 {
                    self.l[g] *= c;
                    for x in self.acc[g].iter_mut() {
                        *x *= c;
                    }
                }
                self.m[g] = m;
                for r in 0..n {
                    s[r][g] = (s[r][g] - m).exp();
                    self.l[g] += s[r][g];
                }
            }
            for r in 0..n {
                let vr = &v[(r0 + r) * D..(r0 + r + 1) * D];
                let vv: [f32; D] = std::array::from_fn(|i| bf(vr[i]));
                for g in 0..G {
                    let p = s[r][g];
                    for i in 0..D {
                        self.acc[g][i] += p * vv[i];
                    }
                }
            }
            r0 += n;
        }
    }

    fn out(&self) -> Q {
        let mut o = [[0f32; D]; G];
        for g in 0..G {
            for i in 0..D {
                o[g][i] = self.acc[g][i] / self.l[g];
            }
        }
        o
    }
}

#[cfg(target_arch = "x86_64")]
mod avx {
    //! The same online softmax with AVX-512: bf16 rows widened to f32 in
    //! registers, a polynomial exp, and the V sum with its 4×16 accumulators
    //! held in registers across a block of rows.
    use super::{State, D, G, Q};
    use std::arch::x86_64::*;

    const B: usize = 32;

    #[inline(always)]
    unsafe fn load_bf(p: *const u16) -> __m512 {
        let x = _mm256_loadu_si256(p as *const __m256i);
        _mm512_castsi512_ps(_mm512_slli_epi32::<16>(_mm512_cvtepu16_epi32(x)))
    }

    /// exp(x) for x <= 0 (rel. error ~2e-7): 2^(x·log2 e), split into integer and
    /// fraction, a degree-6 polynomial for the fraction, scalef for the integer.
    #[inline(always)]
    unsafe fn exp(x: __m512) -> __m512 {
        let y = _mm512_mul_ps(_mm512_max_ps(x, _mm512_set1_ps(-87.0)), _mm512_set1_ps(std::f32::consts::LOG2_E));
        let n = _mm512_roundscale_ps::<{ _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC }>(y);
        let f = _mm512_mul_ps(_mm512_sub_ps(y, n), _mm512_set1_ps(std::f32::consts::LN_2));
        // e^f, |f| <= ln2/2, Taylor to degree 6
        let mut p = _mm512_set1_ps(1.0 / 720.0);
        for c in [1.0 / 120.0, 1.0 / 24.0, 1.0 / 6.0, 0.5, 1.0, 1.0] {
            p = _mm512_fmadd_ps(p, f, _mm512_set1_ps(c));
        }
        _mm512_scalef_ps(p, n)
    }

    /// `max_g q_g · row` for `rows` bf16 rows (the centroids).
    #[target_feature(enable = "avx512f")]
    pub(super) unsafe fn max_scores(q: &Q, k: *const u16, rows: usize, out: &mut [f32]) {
        for r in 0..rows {
            let kr = k.add(r * D);
            if r + 4 < rows {
                super::prefetch(k.add((r + 4) * D) as *const u8, D * 2);
            }
            let mut acc = [_mm512_setzero_ps(); G];
            for c in 0..D / 16 {
                let kk = load_bf(kr.add(c * 16));
                for g in 0..G {
                    acc[g] = _mm512_fmadd_ps(_mm512_loadu_ps(q[g].as_ptr().add(c * 16)), kk, acc[g]);
                }
            }
            let mut best = f32::NEG_INFINITY;
            for g in 0..G {
                best = best.max(_mm512_reduce_add_ps(acc[g]));
            }
            out[r] = best;
        }
    }

    #[target_feature(enable = "avx512f")]
    pub(super) unsafe fn attend(st: &mut State, q: &Q, k: *const u16, v: *const u16, rows: usize) {
        let mut s = [[0f32; B]; G];
        let mut r0 = 0;
        while r0 < rows {
            let n = B.min(rows - r0);
            // scores
            for r in 0..n {
                let kr = k.add((r0 + r) * D);
                let mut acc = [_mm512_setzero_ps(); G];
                for c in 0..D / 16 {
                    let kk = load_bf(kr.add(c * 16));
                    for g in 0..G {
                        acc[g] = _mm512_fmadd_ps(_mm512_loadu_ps(q[g].as_ptr().add(c * 16)), kk, acc[g]);
                    }
                }
                for g in 0..G {
                    s[g][r] = _mm512_reduce_add_ps(acc[g]);
                }
            }
            for g in 0..G {
                for r in n..B {
                    s[g][r] = f32::NEG_INFINITY;
                }
                let a = _mm512_loadu_ps(s[g].as_ptr());
                let b = _mm512_loadu_ps(s[g].as_ptr().add(16));
                let bm = _mm512_reduce_max_ps(_mm512_max_ps(a, b));
                let m = st.m[g].max(bm);
                let c = (st.m[g] - m).exp();
                if c != 1.0 {
                    st.l[g] *= c;
                    for x in st.acc[g].iter_mut() {
                        *x *= c;
                    }
                }
                st.m[g] = m;
                let mv = _mm512_set1_ps(m);
                let ea = exp(_mm512_sub_ps(a, mv));
                let eb = exp(_mm512_sub_ps(b, mv));
                _mm512_storeu_ps(s[g].as_mut_ptr(), ea);
                _mm512_storeu_ps(s[g].as_mut_ptr().add(16), eb);
                st.l[g] += _mm512_reduce_add_ps(_mm512_add_ps(ea, eb));
            }
            // V: one 16-dim chunk at a time, the G accumulators in registers
            for c in 0..D / 16 {
                let mut acc = [_mm512_setzero_ps(); G];
                for g in 0..G {
                    acc[g] = _mm512_loadu_ps(st.acc[g].as_ptr().add(c * 16));
                }
                for r in 0..n {
                    let vv = load_bf(v.add((r0 + r) * D + c * 16));
                    for g in 0..G {
                        acc[g] = _mm512_fmadd_ps(_mm512_set1_ps(s[g][r]), vv, acc[g]);
                    }
                }
                for g in 0..G {
                    _mm512_storeu_ps(st.acc[g].as_mut_ptr().add(c * 16), acc[g]);
                }
            }
            r0 += n;
        }
    }
}

/// One KV head of one layer, held both ways.
struct Head {
    // dense, in position order
    k: Vec<u16>,
    v: Vec<u16>,
    // IVF over positions SINK..n-tail: rows in cluster order, cluster c is rows
    // off[c]..off[c+1]; the sink and the tail are the dense arrays' first and last rows
    ck: Vec<u16>,
    cv: Vec<u16>,
    off: Vec<u32>,
    cent: Vec<u16>,
}

struct Layer {
    heads: Vec<Head>,
    n: usize,
    tail: usize,
}

fn dense_step(h: &Head, q: &Q, n: usize) -> Q {
    let mut st = State::new();
    st.attend(q, &h.k, &h.v, n);
    st.out()
}

/// Clusters by `max_g q_g·centroid`, whole clusters in that order while they fit
/// in `budget` tokens; then sink, chosen clusters (in memory order) and tail.
fn ivf_step(h: &Head, q: &Q, n: usize, tail: usize, budget: usize, read: &mut usize) -> Q {
    let c = h.off.len() - 1;
    let mut raw = vec![0f32; c];
    #[cfg(target_arch = "x86_64")]
    let fast = std::is_x86_feature_detected!("avx512f");
    #[cfg(not(target_arch = "x86_64"))]
    let fast = false;
    if fast {
        // SAFETY: AVX-512 is present; cent holds c rows of D.
        #[cfg(target_arch = "x86_64")]
        unsafe {
            avx::max_scores(q, h.cent.as_ptr(), c, &mut raw)
        };
    } else {
        for j in 0..c {
            raw[j] = dot_g(q, &h.cent[j * D..(j + 1) * D]).iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        }
    }
    let mut sc: Vec<(f32, u32)> = raw.iter().enumerate().map(|(j, &s)| (s, j as u32)).collect();
    let avg = ((n - SINK - tail) as f64 / c as f64).max(1.0);
    let want = ((2.0 * budget as f64 / avg) as usize + 8).min(c);
    if want < c {
        sc.select_nth_unstable_by(want, |a, b| b.0.total_cmp(&a.0));
        sc.truncate(want);
    }
    sc.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    let mut chosen = Vec::new();
    let mut used = 0usize;
    for &(_, j) in &sc {
        let sz = (h.off[j as usize + 1] - h.off[j as usize]) as usize;
        if used + sz > budget {
            if used + sz > budget + budget / 8 {
                continue;
            }
        }
        chosen.push(j);
        used += sz;
        if used >= budget {
            break;
        }
    }
    chosen.sort_unstable();
    let mut st = State::new();
    st.attend(q, &h.k[..SINK * D], &h.v[..SINK * D], SINK);
    for (ci, &j) in chosen.iter().enumerate() {
        if let Some(&nx) = chosen.get(ci + 2) {
            // the cluster after next: its K and V rows, ahead of the gather
            let (a, b) = (h.off[nx as usize] as usize, h.off[nx as usize + 1] as usize);
            prefetch(h.ck[a * D..].as_ptr() as *const u8, (b - a) * D * 2);
            prefetch(h.cv[a * D..].as_ptr() as *const u8, (b - a) * D * 2);
        }
        let (a, b) = (h.off[j as usize] as usize, h.off[j as usize + 1] as usize);
        st.attend(q, &h.ck[a * D..b * D], &h.cv[a * D..b * D], b - a);
    }
    let t0 = n - tail;
    st.attend(q, &h.k[t0 * D..n * D], &h.v[t0 * D..n * D], tail);
    *read += SINK + used + tail;
    st.out()
}

/// One prefetch hint per 64-byte line (as `kernels::prefetch` in chloedb).
#[inline(always)]
fn prefetch(p: *const u8, bytes: usize) {
    #[cfg(target_arch = "x86_64")]
    {
        let mut off = 0;
        while off < bytes {
            // SAFETY: a prefetch never faults.
            unsafe { std::arch::x86_64::_mm_prefetch::<{ std::arch::x86_64::_MM_HINT_T0 }>(p.wrapping_add(off) as *const i8) };
            off += 64;
        }
    }
}

// ------------------------------------------------------------------ k-means

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn normal(&mut self) -> f32 {
        let (a, b) = (self.unit().max(1e-7), self.unit());
        (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
    }
}

/// The 16 dot products of four points by four centroids (as `dot_f32_4x4` in
/// `chloedb/src/vector/kernels.rs`, which `ivf.rs` assigns with).
#[inline(always)]
fn dot4x4(p: [&[f32]; 4], c: [&[f32]; 4]) -> [[f32; 4]; 4] {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx512f") {
        // SAFETY: AVX-512 is present; all eight rows hold D floats.
        return unsafe { dot4x4_avx(p.map(|x| x.as_ptr()), c.map(|x| x.as_ptr())) };
    }
    p.map(|a| c.map(|b| a.iter().zip(b).map(|(x, y)| x * y).sum()))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn dot4x4_avx(p: [*const f32; 4], c: [*const f32; 4]) -> [[f32; 4]; 4] {
    use std::arch::x86_64::*;
    let mut acc = [[_mm512_setzero_ps(); 4]; 4];
    for b in 0..D / 16 {
        let cv = c.map(|x| _mm512_loadu_ps(x.add(b * 16)));
        for a in 0..4 {
            let pv = _mm512_loadu_ps(p[a].add(b * 16));
            for e in 0..4 {
                acc[a][e] = _mm512_fmadd_ps(pv, cv[e], acc[a][e]);
            }
        }
    }
    acc.map(|r| r.map(|x| _mm512_reduce_add_ps(x)))
}

/// Nearest centroid of every point (max of x·c − |c|²/2), four points by four
/// centroids at a time.
fn assign(x: &[f32], pts: &[usize], cent: &[f32], c: usize, flops: &mut f64) -> Vec<u32> {
    let half: Vec<f32> = (0..c).map(|j| 0.5 * cent[j * D..(j + 1) * D].iter().map(|v| v * v).sum::<f32>()).collect();
    let mut out = vec![0u32; pts.len()];
    for (pi, chunk) in pts.chunks(4).enumerate() {
        let mut best = [(f32::NEG_INFINITY, 0u32); 4];
        let p: [&[f32]; 4] = std::array::from_fn(|i| {
            let r = chunk[i.min(chunk.len() - 1)];
            &x[r * D..(r + 1) * D]
        });
        let mut j = 0;
        while j < c {
            let cs: [&[f32]; 4] = std::array::from_fn(|i| {
                let r = (j + i).min(c - 1);
                &cent[r * D..(r + 1) * D]
            });
            let dots = dot4x4(p, cs);
            for a in 0..4 {
                for e in 0..4 {
                    let jj = (j + e).min(c - 1);
                    let s = dots[a][e] - half[jj];
                    if s > best[a].0 {
                        best[a] = (s, jj as u32);
                    }
                }
            }
            j += 4;
        }
        for (i, _) in chunk.iter().enumerate() {
            out[pi * 4 + i] = best[i].1;
        }
    }
    *flops += 2.0 * pts.len() as f64 * c as f64 * D as f64;
    out
}

/// Builds one head's IVF: k-means on a `sample` share of rows SINK..n-tail
/// (`iters` Lloyd steps), every row assigned, rows copied into cluster order.
fn build_ivf(h: &mut Head, n: usize, tail: usize, per: usize, iters: usize, sample: f64, seed: u64, flops: &mut f64) {
    let lo = SINK;
    let hi = n - tail;
    let x: Vec<f32> = h.k.iter().map(|&b| bf(b)).collect();
    let all: Vec<usize> = (lo..hi).collect();
    let c = ((hi - lo) / per).max(1);
    let mut rng = Rng(seed | 1);
    let mut samp: Vec<usize> = if sample < 1.0 {
        all.iter().cloned().filter(|_| (rng.unit() as f64) < sample).collect()
    } else {
        all.clone()
    };
    if samp.len() < c {
        samp = all.clone();
    }
    let mut cent = vec![0f32; c * D];
    for j in 0..c {
        let r = samp[(rng.next() as usize) % samp.len()];
        cent[j * D..(j + 1) * D].copy_from_slice(&x[r * D..(r + 1) * D]);
    }
    for _ in 0..iters {
        let a = assign(&x, &samp, &cent, c, flops);
        let mut sum = vec![0f32; c * D];
        let mut cnt = vec![0u32; c];
        for (&r, &j) in samp.iter().zip(&a) {
            cnt[j as usize] += 1;
            for i in 0..D {
                sum[j as usize * D + i] += x[r * D + i];
            }
        }
        for j in 0..c {
            if cnt[j] == 0 {
                let r = samp[(rng.next() as usize) % samp.len()];
                cent[j * D..(j + 1) * D].copy_from_slice(&x[r * D..(r + 1) * D]);
            } else {
                for i in 0..D {
                    cent[j * D + i] = sum[j * D + i] / cnt[j] as f32;
                }
            }
        }
    }
    let a = assign(&x, &all, &cent, c, flops);
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|&i| a[i]);
    let mut off = vec![0u32; c + 1];
    for &j in &a {
        off[j as usize + 1] += 1;
    }
    for j in 0..c {
        off[j + 1] += off[j];
    }
    h.ck = Vec::with_capacity(all.len() * D);
    h.cv = Vec::with_capacity(all.len() * D);
    for &i in &order {
        let r = all[i];
        h.ck.extend_from_slice(&h.k[r * D..(r + 1) * D]);
        h.cv.extend_from_slice(&h.v[r * D..(r + 1) * D]);
    }
    h.off = off;
    h.cent = cent.iter().map(|&f| to_bf(f)).collect();
}

// ------------------------------------------------------------------ data

/// Synthetic keys with the traits that matter here: a few large channels shared
/// by every key (as RoPE'd keys of real models have), topics (keys of related
/// tokens near each other) and noise.
fn synth_head(n: usize, seed: u64) -> (Vec<u16>, Vec<u16>) {
    let mut rng = Rng(seed * 2654435761 + 7);
    let topics = 256;
    let tvec: Vec<f32> = (0..topics * D).map(|_| rng.normal()).collect();
    let bias: Vec<f32> = (0..D).map(|i| if i % 16 == 0 { 8.0 * rng.normal() } else { 0.0 }).collect();
    let mut k = Vec::with_capacity(n * D);
    let mut v = Vec::with_capacity(n * D);
    let mut t = 0usize;
    for _ in 0..n {
        if rng.unit() < 0.02 {
            t = (rng.next() as usize) % topics;
        }
        for i in 0..D {
            k.push(to_bf(bias[i] + 1.5 * tvec[t * D + i] + rng.normal()));
            v.push(to_bf(rng.normal()));
        }
    }
    (k, v)
}

fn read_bf16(path: &str) -> Vec<u16> {
    let b = std::fs::read(path).expect("read");
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// A layer of n tokens: synthetic, or the first n of a dumped layer (H x N x D).
fn make_layer(n: usize, tail: usize, li: usize, real: Option<&(Vec<u16>, Vec<u16>, usize)>) -> Layer {
    let heads = (0..H)
        .map(|hi| {
            let (k, v) = match real {
                Some((k, v, nn)) => {
                    // tile the dump if n is longer; layers li > 0 rotate the heads
                    let src = (hi + li) % H;
                    let mut kk = Vec::with_capacity(n * D);
                    let mut vv = Vec::with_capacity(n * D);
                    for t in 0..n {
                        let r = (src * nn + t % nn) * D;
                        kk.extend_from_slice(&k[r..r + D]);
                        vv.extend_from_slice(&v[r..r + D]);
                    }
                    (kk, vv)
                }
                None => synth_head(n, (li * H + hi) as u64 + 1),
            };
            Head { k, v, ck: vec![], cv: vec![], off: vec![], cent: vec![] }
        })
        .collect();
    Layer { heads, n, tail }
}

/// Queries that look like the model's: a key of the layer (or a mix of two) plus noise.
fn make_query(l: &Layer, hi: usize, rng: &mut Rng) -> Q {
    let n = l.n;
    let h = &l.heads[hi];
    std::array::from_fn(|_| {
        let a = (rng.next() as usize) % n;
        let b = (rng.next() as usize) % n;
        std::array::from_fn(|i| 0.09 * (bf(h.k[a * D + i]) + bf(h.k[b * D + i]) + rng.normal()))
    })
}

fn par_heads<T: Send>(threads: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let mut out: Vec<Option<T>> = (0..H).map(|_| None).collect();
    std::thread::scope(|s| {
        let f = &f;
        let hs: Vec<_> = (0..threads)
            .map(|t| s.spawn(move || (t..H).step_by(threads).map(|h| (h, f(h))).collect::<Vec<_>>()))
            .collect();
        for j in hs {
            for (h, x) in j.join().unwrap() {
                out[h] = Some(x);
            }
        }
    });
    out.into_iter().map(|x| x.unwrap()).collect()
}

/// Streaming read bandwidth over `bytes`, on `threads` threads (GB/s).
fn bandwidth(threads: usize, bytes: usize) -> f64 {
    let buf: Vec<u64> = (0..bytes / 8).map(|i| i as u64).collect();
    let chunk = buf.len() / threads;
    let mut best = 0f64;
    for _ in 0..5 {
        let t = Instant::now();
        let s: u64 = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..threads)
                .map(|i| {
                    let part = &buf[i * chunk..(i + 1) * chunk];
                    sc.spawn(move || {
                        let mut acc = [0u64; 8];
                        for c in part.chunks_exact(8) {
                            for l in 0..8 {
                                acc[l] = acc[l].wrapping_add(c[l]);
                            }
                        }
                        acc.iter().fold(0u64, |a, b| a.wrapping_add(*b))
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).fold(0u64, |a, b| a.wrapping_add(b))
        });
        std::hint::black_box(s);
        best = best.max(bytes as f64 / t.elapsed().as_secs_f64() / 1e9);
    }
    best
}

fn median(mut x: Vec<f64>) -> f64 {
    x.sort_by(|a, b| a.total_cmp(b));
    x[x.len() / 2]
}

/// FLOPs of an 8B (Llama-3-8B-shaped) prefill of n tokens: 2·params·n for the
/// matrices, causal attention 2·2·(n²/2)·128 per query head, 32 heads, 32 layers.
fn prefill_flops(n: usize) -> f64 {
    let n = n as f64;
    2.0 * 7.0e9 * n + LAYERS_8B * 32.0 * 2.0 * 2.0 * 0.5 * n * n * D as f64
}

fn arg<T: std::str::FromStr>(args: &[String], name: &str, def: T) -> T {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(def)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tokens: String = arg(&args, "--tokens", "32768,131072".to_string());
    let budget_frac: f64 = arg(&args, "--budget", 0.0625);
    let iters: usize = arg(&args, "--iters", 8);
    let sample: f64 = arg(&args, "--sample", 1.0);
    let tail: usize = arg(&args, "--tail", 256);
    let per: usize = arg(&args, "--per", 16);
    let threads: usize = arg(&args, "--threads", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
    let steps: usize = arg(&args, "--steps", 50);
    let weights_gb: f64 = arg(&args, "--weights-gb", 4.9);
    let keys: String = arg(&args, "--keys", String::new());
    let values: String = arg(&args, "--values", String::new());
    let check = args.iter().any(|a| a == "--check");

    let real = if keys.is_empty() {
        None
    } else {
        let k = read_bf16(&keys);
        let v = read_bf16(&values);
        let nn = k.len() / (H * D);
        eprintln!("real K/V: {nn} tokens per head");
        Some((k, v, nn))
    };
    let bw = bandwidth(threads, 1 << 30);
    let w_ms = weights_gb / bw * 1e3;
    println!("# {threads} threads, streaming read {bw:.1} GB/s; {weights_gb} GB of 4-bit 8B weights read once per token = {w_ms:.0} ms");
    println!("# budget {:.2}% of the indexed tokens, clusters of ~{per}, tail {tail}, sink {SINK}, k-means {iters} iterations on {:.0}% of rows; keys {}",
        100.0 * budget_frac, 100.0 * sample, if real.is_some() { "real (dump)" } else { "synthetic" });
    println!();
    println!("| tokens | dense attention, one layer | IVF, one layer | attention speedup | tokens read | 8B decode step, dense (32 layers + weights) | 8B, IVF | 8B speedup | index build, one layer | build FLOPs / prefill FLOPs |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for n in tokens.split(',').map(|s| s.trim().parse::<usize>().unwrap()) {
        let layer_bytes = n * H * D * 2 * 2;
        // L3 here is 260 MiB: cycle through enough layers that a step finds its layer out of cache
        let nl = ((2usize << 30) / layer_bytes).clamp(2, 16);
        let mut layers: Vec<Layer> = (0..nl).map(|li| make_layer(n, tail, li, real.as_ref())).collect();
        let budget = ((budget_frac * (n - SINK - tail) as f64) as usize).max(16);
        // build (every layer; time the first)
        let mut build_s = 0.0;
        let mut flops = 0.0;
        for (li, l) in layers.iter_mut().enumerate() {
            let t = Instant::now();
            let f: Vec<f64> = {
                let ln = l.n;
                let lt = l.tail;
                let heads = std::sync::Mutex::new(std::mem::take(&mut l.heads).into_iter().map(Some).collect::<Vec<_>>());
                let res = par_heads(threads, |hi| {
                    let mut h = heads.lock().unwrap()[hi].take().unwrap();
                    let mut fl = 0.0;
                    build_ivf(&mut h, ln, lt, per, if li == 0 { iters } else { 0 }, sample, (li * H + hi) as u64 + 11, &mut fl);
                    (h, fl)
                });
                l.heads = Vec::with_capacity(H);
                let mut fs = vec![];
                for (h, fl) in res {
                    l.heads.push(h);
                    fs.push(fl);
                }
                fs
            };
            if li == 0 {
                build_s = t.elapsed().as_secs_f64();
                flops = f.iter().sum();
            }
        }
        let mut rng = Rng(12345 + n as u64);
        if check {
            // with the whole index read, IVF must equal dense
            let l = &layers[0];
            let mut err = 0f32;
            for hi in 0..H {
                let q = make_query(l, hi, &mut rng);
                let a = dense_step(&l.heads[hi], &q, n);
                let mut r = 0;
                let b = ivf_step(&l.heads[hi], &q, n, tail, n, &mut r);
                for g in 0..G {
                    for i in 0..D {
                        err = err.max((a[g][i] - b[g][i]).abs());
                    }
                }
            }
            println!("check n={n}: max |dense - ivf(all)| = {err:e}");
        }
        let qs: Vec<Vec<Q>> = (0..steps).map(|s| (0..H).map(|hi| make_query(&layers[s % nl], hi, &mut rng)).collect()).collect();
        let mut td = vec![];
        let mut ti = vec![];
        let mut read = 0usize;
        for s in 0..steps {
            let l = &layers[s % nl];
            let t = Instant::now();
            let o = par_heads(threads, |hi| dense_step(&l.heads[hi], &qs[s][hi], n));
            td.push(t.elapsed().as_secs_f64());
            std::hint::black_box(o);
            let t = Instant::now();
            let o = par_heads(threads, |hi| {
                let mut r = 0;
                let x = ivf_step(&l.heads[hi], &qs[s][hi], n, tail, budget, &mut r);
                (x, r)
            });
            ti.push(t.elapsed().as_secs_f64());
            read += o.iter().map(|x| x.1).sum::<usize>();
        }
        let (d, i) = (median(td) * 1e3, median(ti) * 1e3);
        let frac = read as f64 / (steps * H * n) as f64;
        let (md, mi) = (w_ms + LAYERS_8B * d, w_ms + LAYERS_8B * i);
        let ratio = flops * LAYERS_8B / prefill_flops(n);
        println!("| {n} | {d:.2} ms | {i:.2} ms | {:.1}x | {:.1}% | {md:.0} ms | {mi:.0} ms | {:.2}x | {build_s:.2} s | {:.1}% |",
            d / i, 100.0 * frac, md / mi, 100.0 * ratio);
    }
}
