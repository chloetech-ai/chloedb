//! `GROUP BY` and aggregates read from the columns: each segment's rows
//! find their group by a keyword column's value code or a numeric
//! column's value, without making a group's values for every row, and add
//! into the group's accumulators. Segments are grouped on their own
//! threads ([`Executor::per_segment`]) and their groups merged in segment
//! order, so the groups come out in the order one pass first sees them;
//! FLOAT sums are exact ([`agg::ExactSum`]), so merging them gives the
//! bits one pass gives. A field's values written in place are added beside
//! its column's, the column's value of those rows left out, so a counter
//! or rating set in place keeps an aggregate of it on the columns.
//!
//! It answers a query whose groups are single-valued keyword, INT or
//! FLOAT fields, or a part of a single-valued TIMESTAMP's times
//! (`date_trunc('year', at)`, `date_bin('5 minutes', at)`, `EXTRACT(dow
//! FROM at)`), and whose
//! aggregates are `COUNT(*)`, or `COUNT`, `SUM`,
//! `AVG`, `MIN`, `MAX` and percentiles of single-valued INT or FLOAT
//! fields; anything
//! else (paths, multi-valued fields, `COUNT(DISTINCT)`, aggregates of
//! keyword fields) goes row by row ([`Executor::aggregate`]).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use super::{Executor, Local, Seg};
use crate::error::Result;
use crate::forward::{KeywordColumn, NumericColumn, NumericKind, BLOCK};
use crate::query::agg::{self, Acc, ExactSum, Part, Sketch};
use crate::query::logical::{AggFn, Col, Projection, Select, TimeCol, TimeOf};
use crate::schema::FieldType;
use crate::segment::{DocId, DocSet};
use crate::storage::Updates;
use crate::value::Value;

/// The fields a query's groups and aggregates read: each group's field
/// (and the part of its times, [`Select::times`], it groups by), and each
/// needed aggregate's (None for `COUNT(*)`).
pub(super) struct Plan {
    groups: Vec<usize>,
    times: Vec<Option<usize>>,
    aggs: Vec<Option<usize>>,
}

/// Whether the columns answer `q`'s groups and `needed` aggregates.
pub(super) fn plan(schema: &crate::schema::Schema, q: &Select, needed: &[Projection]) -> Option<Plan> {
    let single = |f: usize, kinds: &[FieldType]| {
        let fd = &schema.fields[f];
        !fd.multi && kinds.contains(&fd.ty)
    };
    let mut groups = Vec::with_capacity(q.group_by.len());
    let mut times = Vec::with_capacity(q.group_by.len());
    for c in &q.group_by {
        match *c {
            Col::Field(f) if single(f, &[FieldType::Keyword, FieldType::Int, FieldType::Float]) => {
                groups.push(f);
                times.push(None);
            }
            // A part of a time is a number; text (`strftime`) goes row by row.
            Col::Time(t) if q.times.get(t).is_some_and(|tc| matches!(tc.of, TimeOf::Trunc(_) | TimeOf::Part(_) | TimeOf::Bin { .. }) && single(tc.field, &[FieldType::Int])) => {
                groups.push(q.times[t].field);
                times.push(Some(t));
            }
            _ => return None,
        }
    }
    // Group keys of up to four fields hash as they are.
    if groups.len() > 4 {
        return None;
    }
    let mut aggs = Vec::with_capacity(needed.len());
    for p in needed {
        match p {
            Projection::Count => aggs.push(None),
            Projection::Agg { func: AggFn::Count | AggFn::Sum | AggFn::Avg | AggFn::Min | AggFn::Max | AggFn::PercentileCont(_) | AggFn::PercentileDisc(_) | AggFn::ApproxPercentile(_), of: Col::Field(f) } if single(*f, &[FieldType::Int, FieldType::Float]) => {
                aggs.push(Some(*f))
            }
            _ => return None,
        }
    }
    Some(Plan { groups, times, aggs })
}

/// A group field's values in one segment: a numeric field's, or the part
/// of its times the group takes.
enum Key<'s> {
    Codes(&'s KeywordColumn),
    Num(&'s NumericColumn, Option<&'s Updates>, Option<&'s TimeCol>),
}

/// A group's value of a numeric field's value `v`: the part of the time
/// it is when the group takes one ([`TimeCol::value`], as the row-by-row
/// aggregate reads it).
#[inline]
fn timed(t: Option<&TimeCol>, v: Value) -> Value {
    match (t, v) {
        (Some(tc), Value::Int(m) | Value::Timestamp(m)) => tc.value(m),
        (_, v) => v,
    }
}

/// A numeric field's value of document `d`, whose key in the column is
/// `key` (None without one): written in place, else the column's (as
/// [`Executor::agg_values`] reads it).
#[inline]
fn value(col: &NumericColumn, updates: Option<&Updates>, field: usize, d: DocId, key: Option<i64>) -> Value {
    if let Some(v) = updates.and_then(|u| u.get(field, d)) {
        return v.clone();
    }
    match key {
        None => Value::Null,
        Some(k) if col.kind() == NumericKind::Int => Value::Int(k),
        Some(k) => Value::Float(col.kind().decode(k)),
    }
}

/// Whether every value written in place of `field` (of type `ty`) is NULL
/// or of that type.
fn own_type(u: &Updates, field: usize, ty: &FieldType) -> bool {
    let mut own = true;
    u.each_oldest_first(field, |_, v| {
        own &= matches!((ty, v), (_, Value::Null) | (FieldType::Int, Value::Int(_)) | (FieldType::Float, Value::Float(_)));
    });
    own
}

/// Calls `f` with each block of 128 rows of `set` that holds any: the
/// block's number and its rows (bit `j % 64` of word `j / 64` for row
/// `BLOCK * b + j`), until `f` answers false.
fn each_block(set: &DocSet, mut f: impl FnMut(usize, [u64; 2]) -> bool) {
    match set {
        DocSet::Bits { words, .. } => {
            for (b, w) in words.chunks(2).enumerate() {
                let rows = [w[0], w.get(1).copied().unwrap_or(0)];
                if rows != [0, 0] && !f(b, rows) {
                    return;
                }
            }
        }
        DocSet::Sorted { docs, .. } => {
            let (mut at, mut rows) = (usize::MAX, [0u64; 2]);
            for &d in docs {
                let (b, j) = (d as usize / BLOCK, d as usize % BLOCK);
                if b != at {
                    if at != usize::MAX && !f(at, rows) {
                        return;
                    }
                    (at, rows) = (b, [0, 0]);
                }
                rows[j / 64] |= 1 << (j % 64);
            }
            if at != usize::MAX {
                f(at, rows);
            }
        }
    }
}

/// A value as the bits of a group key part: values that make the same
/// [`Part`] may differ here (they meet again at [`Part`]), and values that
/// make different parts never agree here.
#[inline]
fn bits(v: &Value) -> (u8, u64) {
    match v {
        Value::Null => (0, 0),
        Value::Int(x) | Value::Timestamp(x) => (1, *x as u64),
        Value::Float(x) => (2, x.to_bits()),
        Value::Bool(b) => (3, *b as u64),
        // Not a numeric column's value: one slot per segment, keyed again
        // by its part.
        _ => (4, 0),
    }
}

/// FxHash's multiply and rotate: the keys are a few integers.
#[derive(Default)]
struct Fx(u64);

impl Hasher for Fx {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(*b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, x: u8) {
        self.write_u64(x as u64);
    }
    #[inline]
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_usize(&mut self, x: usize) {
        self.write_u64(x as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

/// The fixed width (microseconds) and origin of a part of a time that is
/// a bucket of one: `date_bin`, and `date_trunc` to a second, minute,
/// hour, day or week (from Monday 1970-01-05). Rows of a segment then find
/// their group by the bucket's number from the least one, without making
/// the bucket's value first.
fn fixed_width(t: &TimeCol) -> Option<(i64, i64)> {
    use crate::timestamp::Unit;
    const DAY: i64 = 86_400_000_000;
    match t.of {
        TimeOf::Bin { width, origin } => Some((width, origin)),
        TimeOf::Trunc(Unit::Second) => Some((1_000_000, 0)),
        TimeOf::Trunc(Unit::Minute) => Some((60_000_000, 0)),
        TimeOf::Trunc(Unit::Hour) => Some((3_600_000_000, 0)),
        TimeOf::Trunc(Unit::Day) => Some((DAY, 0)),
        TimeOf::Trunc(Unit::Week) => Some((7 * DAY, 4 * DAY)),
        _ => None,
    }
}

/// A numeric group field's keys in a segment that groups find by a table
/// (of this many slots at most) rather than by hashing.
const DENSE_KEYS: i64 = 1 << 16;

/// Each needed aggregate's states, one per group (by its index in
/// [`All`]).
enum Sink {
    /// `COUNT(*)` (rows) or `COUNT(f)` (rows with a value).
    Count(Vec<i64>),
    /// `SUM` or `AVG` of an INT field: the exact sum and the values.
    Int { avg: bool, isum: Vec<i128>, n: Vec<i64> },
    /// `SUM` or `AVG` of a FLOAT field: the exact sum and the values.
    /// Values between 2^-12 and 2^42 are added as integers of 2^-64 apart
    /// from it ([`agg::near_units`]), `pending` of them since they were
    /// last moved into it.
    Float { avg: bool, sum: Vec<ExactSum>, near: Vec<i128>, pending: u32, n: Vec<i64> },
    /// `approx_percentile` of p: each group's sketch, which adds the
    /// column's numbers without making values of them.
    Sketch(f64, Vec<Sketch>),
    /// An `approx_percentile` of the field an earlier one's sketch (of
    /// this index in the needed aggregates) is of: it adds nothing, and
    /// answers from that sketch.
    SketchOf(usize),
    /// `MIN`, `MAX`, percentiles, and any aggregate of a field with values
    /// written in place of another type than its own: the row-by-row
    /// aggregate's accumulators.
    Accs(Vec<Acc>),
}

impl Sink {
    fn new(p: &Projection, field: Option<usize>, schema: &crate::schema::Schema, typed: bool) -> Sink {
        let float = field.is_some_and(|f| schema.fields[f].ty == FieldType::Float);
        match p {
            Projection::Count => Sink::Count(Vec::new()),
            Projection::Agg { func: AggFn::Count, .. } if typed => Sink::Count(Vec::new()),
            Projection::Agg { func: f @ (AggFn::Sum | AggFn::Avg), .. } if typed && float => Sink::Float { avg: *f == AggFn::Avg, sum: Vec::new(), near: Vec::new(), pending: 0, n: Vec::new() },
            Projection::Agg { func: f @ (AggFn::Sum | AggFn::Avg), .. } if typed => Sink::Int { avg: *f == AggFn::Avg, isum: Vec::new(), n: Vec::new() },
            Projection::Agg { func: AggFn::ApproxPercentile(p), .. } if typed => Sink::Sketch(p.get(), Vec::new()),
            _ => Sink::Accs(Vec::new()),
        }
    }

    fn push(&mut self, p: &Projection) {
        match self {
            Sink::Count(c) => c.push(0),
            Sink::Int { isum, n, .. } => {
                isum.push(0);
                n.push(0);
            }
            Sink::Float { sum, near, n, .. } => {
                sum.push(ExactSum::new());
                near.push(0);
                n.push(0);
            }
            Sink::Sketch(p, s) => s.push(Sketch::new(*p)),
            Sink::SketchOf(_) => {}
            Sink::Accs(a) => a.push(Acc::new(p)),
        }
    }

    /// Moves a FLOAT sink's integers into its sums.
    fn flush(&mut self) {
        if let Sink::Float { sum, near, pending, .. } = self {
            for (s, v) in sum.iter_mut().zip(near.iter_mut()) {
                s.add_near(std::mem::take(v));
            }
            *pending = 0;
        }
    }

    /// Adds group `h` of `other` (a later segment's) into group `g`.
    fn absorb(&mut self, g: usize, other: &mut Sink, h: usize) {
        match (self, other) {
            (Sink::Count(c), Sink::Count(d)) => c[g] += d[h],
            (Sink::Int { isum, n, .. }, Sink::Int { isum: i2, n: n2, .. }) => {
                isum[g] += i2[h];
                n[g] += n2[h];
            }
            (Sink::Float { sum, n, .. }, Sink::Float { sum: s2, n: n2, .. }) => {
                sum[g].merge(&s2[h]);
                n[g] += n2[h];
            }
            (Sink::Sketch(p, s), Sink::Sketch(_, t)) => s[g].absorb(std::mem::replace(&mut t[h], Sketch::new(*p))),
            (Sink::SketchOf(_), Sink::SketchOf(_)) => {}
            (Sink::Accs(a), Sink::Accs(b)) => a[g].absorb(std::mem::replace(&mut b[h], Acc::Count(0))),
            _ => unreachable!("segments' sinks differ"),
        }
    }

    /// Group `g`'s state as the row-by-row aggregate's accumulator.
    fn acc(&mut self, g: usize) -> Acc {
        match self {
            Sink::Count(c) => Acc::Count(c[g]),
            Sink::Int { avg: true, isum, n } => Acc::Avg { sum: ExactSum::new(), isum: isum[g], n: n[g] },
            Sink::Int { avg: false, isum, n } => Acc::Sum { sum: ExactSum::new(), isum: isum[g], int: true, any: n[g] > 0 },
            Sink::Float { avg: true, sum, n, .. } => Acc::Avg { sum: std::mem::take(&mut sum[g]), isum: 0, n: n[g] },
            Sink::Float { avg: false, n, .. } if n[g] == 0 => Acc::Sum { sum: ExactSum::new(), isum: 0, int: true, any: false },
            Sink::Float { avg: false, sum, .. } => Acc::Sum { sum: std::mem::take(&mut sum[g]), isum: 0, int: false, any: true },
            Sink::Sketch(p, s) => Acc::Sketch(Box::new(std::mem::replace(&mut s[g], Sketch::new(*p)))),
            // Filled in from the sketch it shares (`aggregate_columns`).
            Sink::SketchOf(_) => Acc::Count(0),
            Sink::Accs(a) => std::mem::replace(&mut a[g], Acc::Count(0)),
        }
    }
}

/// Groups in the order first seen, where each group's key is, and the
/// aggregates' states: one segment's, or the query's.
struct All {
    keys: Vec<Vec<Part>>,
    index: HashMap<Vec<Part>, u32>,
    sinks: Vec<Sink>,
}

impl<'a> Executor<'a> {
    /// The groups and aggregates of [`plan`]'s query, read from the columns:
    /// each segment on its own thread, merged in segment order.
    pub(super) fn aggregate_columns(&mut self, q: &Select, needed: &[Projection], plan: &Plan) -> Result<agg::Groups> {
        // Values written in place are added beside the column's
        // (`aggregate_segment`) when each is NULL or of the field's type, as
        // a write keeps them (`WriteTxn::set`). Should one be of another
        // type, the aggregates add every value as a value, as the row-by-row
        // one does.
        let typed = self.segs.iter().all(|s| plan.aggs.iter().flatten().all(|&f| self.updates(s, f).is_none_or(|u| own_type(u, f, &self.schema.fields[f].ty))));
        let schema = self.schema;
        // Approximate percentiles of one field share the first one's sketch
        // (p50, p95 and p99 of a latency): each answers from it.
        let sketch_of = |k: usize| -> Option<usize> {
            let Projection::Agg { func: AggFn::ApproxPercentile(_), .. } = needed[k] else { return None };
            (0..k).find(|&j| matches!(needed[j], Projection::Agg { func: AggFn::ApproxPercentile(_), .. }) && plan.aggs[j] == plan.aggs[k] && sketch_of_first(needed, plan, j))
        };
        let new = || All {
            keys: Vec::new(),
            index: HashMap::new(),
            sinks: needed
                .iter()
                .zip(&plan.aggs)
                .enumerate()
                .map(|(k, (p, &f))| match (typed, sketch_of(k)) {
                    (true, Some(of)) => Sink::SketchOf(of),
                    _ => Sink::new(p, f, schema, typed),
                })
                .collect(),
        };
        let per = self.per_segment(|ex, s, l| {
            let mut part = new();
            ex.aggregate_segment(s, q, needed, plan, &mut part, l)?;
            Ok(part)
        })?;
        let mut all = new();
        for mut part in per {
            part.sinks.iter_mut().for_each(Sink::flush);
            for (h, key) in std::mem::take(&mut part.keys).into_iter().enumerate() {
                let g = match all.index.get(&key) {
                    Some(&g) => g,
                    None => {
                        if let Some(max) = self.opts.max_groups.filter(|&m| all.keys.len() >= m) {
                            return Err(agg::too_many_groups(max));
                        }
                        let g = all.keys.len() as u32;
                        all.index.insert(key.clone(), g);
                        all.keys.push(key);
                        for (s, p) in all.sinks.iter_mut().zip(needed) {
                            s.push(p);
                        }
                        g
                    }
                };
                for (s, o) in all.sinks.iter_mut().zip(part.sinks.iter_mut()) {
                    s.absorb(g as usize, o, h);
                }
            }
        }
        let mut groups = agg::Groups::limited(self.opts.max_groups);
        groups.charge = self.charge.clone();
        let mut sinks = all.sinks;
        groups.map = all
            .keys
            .into_iter()
            .enumerate()
            .map(|(g, k)| {
                let mut accs: Vec<Acc> = sinks.iter_mut().map(|s| s.acc(g)).collect();
                for (i, s) in sinks.iter().enumerate() {
                    if let (Sink::SketchOf(of), Projection::Agg { func: AggFn::ApproxPercentile(p), .. }) = (s, &needed[i]) {
                        let Acc::Sketch(first) = &accs[*of] else { unreachable!("a shared sketch is a sketch") };
                        accs[i] = Acc::Sketch(Box::new(first.with_fraction(p.get())));
                    }
                }
                (k, accs)
            })
            .collect();
        Ok(groups)
    }

    /// Segment `s`'s groups and aggregates, into `all`.
    fn aggregate_segment(&self, s: &Seg<'a>, q: &Select, needed: &[Projection], plan: &Plan, all: &mut All, l: &mut Local) -> Result<()> {
        let mut tick = self.ticker();
        let set = self.filter_set_l(s, &q.filter, l)?;
        l.plan.push(format!("seg {}: aggregate {} rows into groups of {} from the columns", s.ord, set.len(), q.group_by.len()));
        let keys: Vec<Key> = plan
            .groups
            .iter()
            .zip(&plan.times)
            .map(|(&f, t)| match self.schema.fields[f].ty {
                FieldType::Keyword => self.keyword(s, f).map(Key::Codes),
                _ => self.numeric(s, f).map(|n| Key::Num(n.col(), self.updates(s, f), t.map(|t| &q.times[t]))),
            })
            .collect::<Result<_>>()?;
        let aggs: Vec<Option<(usize, &NumericColumn, Option<&Updates>)>> = plan
            .aggs
            .iter()
            .map(|a| match *a {
                None => Ok(None),
                Some(f) => self.numeric(s, f).map(|n| Some((f, n.col(), self.updates(s, f)))),
            })
            .collect::<Result<_>>()?;
        // The values written in place of each aggregate's field that its
        // typed sink adds itself, by docID: the column's value of those rows
        // is not read. (A row-by-row sink reads each row's through `value`.)
        let over: Vec<Vec<(DocId, &Value)>> = aggs
            .iter()
            .zip(&all.sinks)
            .map(|(a, sink)| match (a, sink) {
                (Some((f, _, Some(u))), Sink::Count(_) | Sink::Int { .. } | Sink::Float { .. } | Sink::Sketch(..)) => u.field(*f).collect(),
                _ => Vec::new(),
            })
            .collect();
        let n_over: usize = over.iter().map(Vec::len).sum();
        if n_over > 0 {
            l.plan.push(format!("seg {}: {n_over} values written in place added as they are", s.ord));
        }
        // Each list's first entry not in a block before the current one, and
        // its entries in the current one.
        let mut at = vec![0usize; over.len()];
        let mut here = vec![0..0; over.len()];
        // One keyword field: the group of each value code, and of rows
        // without a value; one numeric field of a few thousand keys (none
        // written in place): of each key from the smallest; else by the
        // key's bits.
        // One fixed-width part of a time (none written in place): of each
        // bucket from the least.
        let mut width: Option<(i64, i64, i128)> = None;
        let (mut by_code, base): (Vec<u32>, Option<i64>) = match keys.as_slice() {
            [Key::Codes(kw)] => (vec![u32::MAX; kw.n_values() + 1], None),
            [Key::Num(col, None, Some(t))] if fixed_width(t).is_some() => {
                let (w, o) = fixed_width(t).expect("fixed");
                let bucket = |k: i64| (k as i128 - o as i128).div_euclid(w as i128);
                match col.key_bounds() {
                    Some((lo, hi)) if bucket(hi) - bucket(lo) < DENSE_KEYS as i128 => {
                        width = Some((w, o, bucket(lo)));
                        (vec![u32::MAX; (bucket(hi) - bucket(lo)) as usize + 2], Some(lo))
                    }
                    None => (vec![u32::MAX; 1], Some(0)),
                    _ => (Vec::new(), None),
                }
            }
            [Key::Num(col, None, None)] => match col.key_bounds() {
                Some((lo, hi)) if hi.checked_sub(lo).is_some_and(|w| w < DENSE_KEYS) => (vec![u32::MAX; (hi - lo) as usize + 2], Some(lo)),
                None => (vec![u32::MAX; 1], Some(0)),
                _ => (Vec::new(), None),
            },
            _ => (Vec::new(), None),
        };
        let mut by_bits: FxMap<[(u8, u64); 4], u32> = FxMap::default();
        // Block by block of 128 rows: each column's block is unpacked
        // once, then each aggregate adds the block's rows.
        let n_cols = keys.len() + aggs.len();
        let mut block = vec![[0i64; BLOCK]; n_cols];
        let mut masks = vec![[0u64; 2]; n_cols];
        // Each picked row's place among the block's picked rows, by its row
        // in the block.
        let (mut pos, mut gs, mut slot) = ([0u8; BLOCK], [0u32; BLOCK], [0u8; BLOCK]);
        let mut err = None;
        each_block(&set, |b, picked| {
            if let Err(e) = tick.tick() {
                err = Some(e);
                return false;
            }
            for (i, k) in keys.iter().enumerate() {
                masks[i] = match k {
                    Key::Codes(kw) => kw.block_codes_of(b, &mut block[i]),
                    Key::Num(col, _, _) => col.block_keys_of(b, &mut block[i]),
                };
            }
            for (i, a) in aggs.iter().enumerate() {
                if let Some((_, col, _)) = a {
                    masks[keys.len() + i] = col.block_keys_of(b, &mut block[keys.len() + i]);
                }
            }
            // The rows of this block written in place, which the column's
            // values then leave out.
            let lo = (b * BLOCK) as DocId;
            for (i, list) in over.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
                while at[i] < list.len() && list[at[i]].0 < lo {
                    at[i] += 1;
                }
                let start = at[i];
                while at[i] < list.len() && list[at[i]].0 < lo + BLOCK as DocId {
                    let j = (list[at[i]].0 - lo) as usize;
                    masks[keys.len() + i][j / 64] &= !(1 << (j % 64));
                    at[i] += 1;
                }
                here[i] = start..at[i];
            }
            let mut k = 0;
            for (w, &bits) in picked.iter().enumerate() {
                let mut x = bits;
                while x != 0 {
                    let j = w * 64 + x.trailing_zeros() as usize;
                    (pos[k], slot[j]) = (j as u8, k as u8);
                    k += 1;
                    x &= x - 1;
                }
            }
            let has = |i: usize, j: usize| masks[i][j / 64] >> (j % 64) & 1 == 1;
            // Each row's group.
            for t in 0..k {
                let j = pos[t] as usize;
                let d = (b * BLOCK + j) as DocId;
                let g = match keys.as_slice() {
                    [_] if !by_code.is_empty() => {
                        let c = match (has(0, j), base, width) {
                            (false, _, _) => by_code.len() - 1,
                            (true, None, _) => block[0][j] as usize,
                            (true, Some(lo), None) => (block[0][j] - lo) as usize,
                            (true, Some(_), Some((w, o, first))) => ((block[0][j] as i128 - o as i128).div_euclid(w as i128) - first) as usize,
                        };
                        match by_code[c] {
                            u32::MAX => match self.group_of(all, needed, &keys, d, plan) {
                                Ok(g) => {
                                    by_code[c] = g;
                                    g
                                }
                                Err(e) => {
                                    err = Some(e);
                                    return false;
                                }
                            },
                            g => g,
                        }
                    }
                    _ => {
                        let mut key = [(0u8, 0u64); 4];
                        for (i, kc) in keys.iter().enumerate() {
                            key[i] = match kc {
                                Key::Codes(_) => if has(i, j) { (5, block[i][j] as u64) } else { (0, 0) },
                                Key::Num(col, u, t) => bits(&timed(*t, value(col, *u, plan.groups[i], d, has(i, j).then_some(block[i][j])))),
                            };
                        }
                        match by_bits.get(&key) {
                            Some(&g) => g,
                            None => match self.group_of(all, needed, &keys, d, plan) {
                                Ok(g) => {
                                    by_bits.insert(key, g);
                                    g
                                }
                                Err(e) => {
                                    err = Some(e);
                                    return false;
                                }
                            },
                        }
                    }
                };
                gs[t] = g;
            }
            // Each aggregate over the block's rows.
            for (a_i, (sink, a)) in all.sinks.iter_mut().zip(&aggs).enumerate() {
                let i = keys.len() + a_i;
                let rows = pos[..k].iter().zip(&gs[..k]).map(|(&j, &g)| (j as usize, g as usize));
                // The picked rows' values written in place, with their groups.
                let written = over[a_i][here[a_i].clone()].iter().filter_map(|&(d, v)| {
                    let j = (d - lo) as usize;
                    (picked[j / 64] >> (j % 64) & 1 == 1).then(|| (gs[slot[j] as usize] as usize, v))
                });
                match (sink, a) {
                    (Sink::Count(c), None) => rows.for_each(|(_, g)| c[g] += 1),
                    (Sink::Count(c), Some(_)) => {
                        rows.for_each(|(j, g)| c[g] += has(i, j) as i64);
                        written.for_each(|(g, v)| c[g] += !matches!(v, Value::Null) as i64);
                    }
                    (Sink::Int { isum, n, .. }, Some(_)) => {
                        for (j, g) in rows.filter(|&(j, _)| has(i, j)) {
                            isum[g] += block[i][j] as i128;
                            n[g] += 1;
                        }
                        for (g, v) in written {
                            if let Value::Int(x) = v {
                                isum[g] += *x as i128;
                                n[g] += 1;
                            }
                        }
                    }
                    (Sink::Float { sum, near, pending, n, .. }, Some((_, col, _))) => {
                        let (vals, rows) = (&block[i], rows.filter(|&(j, _)| has(i, j)));
                        match col.kind() {
                            NumericKind::Decimal(s) => add_floats(|k| NumericKind::Decimal(s).decode(k), vals, rows, sum, near, n),
                            NumericKind::FloatBits => add_floats(|k| NumericKind::FloatBits.decode(k), vals, rows, sum, near, n),
                            NumericKind::Int => add_floats(|k| k as f64, vals, rows, sum, near, n),
                        }
                        for (g, v) in written {
                            if let Value::Float(x) = v {
                                match agg::near_units(*x) {
                                    Some(u) => near[g] += u,
                                    None => sum[g].add(*x),
                                }
                                n[g] += 1;
                            }
                        }
                        // A block adds at most BLOCK to any group's integer.
                        *pending += k as u32;
                        if *pending > agg::NEAR_EVERY - BLOCK as u32 {
                            for (s, v) in sum.iter_mut().zip(near.iter_mut()) {
                                s.add_near(std::mem::take(v));
                            }
                            *pending = 0;
                        }
                    }
                    (Sink::Sketch(_, sk), Some((_, col, _))) => {
                        let (vals, rows) = (&block[i], rows.filter(|&(j, _)| has(i, j)));
                        let mut more = match col.kind() {
                            NumericKind::Decimal(s) => add_sketches(|k| NumericKind::Decimal(s).decode(k), vals, rows, sk),
                            NumericKind::FloatBits => add_sketches(|k| NumericKind::FloatBits.decode(k), vals, rows, sk),
                            NumericKind::Int => add_sketches(|k| k as f64, vals, rows, sk),
                        };
                        for (g, v) in written {
                            more += sk[g].add(v);
                        }
                        if more > 0 {
                            if let Err(e) = self.charge.add(more) {
                                err = Some(e);
                                return false;
                            }
                        }
                    }
                    (Sink::Accs(accs), Some((f, col, u))) => {
                        // A percentile holds its values.
                        let mut more = 0;
                        for (j, g) in rows {
                            more += accs[g].add(&value(col, *u, *f, (b * BLOCK + j) as DocId, has(i, j).then_some(block[i][j])));
                        }
                        if more > 0 {
                            if let Err(e) = self.charge.add(more) {
                                err = Some(e);
                                return false;
                            }
                        }
                    }
                    (Sink::Accs(accs), None) => rows.for_each(|(_, g)| accs[g].add_row()),
                    (Sink::Int { .. } | Sink::Float { .. } | Sink::Sketch(..), None) => {}
                    (Sink::SketchOf(_), _) => {}
                }
            }
            true
        });
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// The group of document `d` (its first row in this segment with these
    /// group values): its index in `all`, a new group's if the query has
    /// room for one more.
    fn group_of(&self, all: &mut All, needed: &[Projection], keys: &[Key], d: DocId, plan: &Plan) -> Result<u32> {
        let key: Vec<Part> = keys
            .iter()
            .zip(&plan.groups)
            .map(|(k, &f)| match k {
                Key::Codes(kw) => kw.value_of_doc(d).map_or(Part::Null, |v| Part::Str(v.to_string())),
                Key::Num(col, u, t) => Part::group(&timed(*t, value(col, *u, f, d, col.sort_key(d)))),
            })
            .collect();
        if let Some(&g) = all.index.get(&key) {
            return Ok(g);
        }
        if let Some(max) = self.opts.max_groups.filter(|&m| all.keys.len() >= m) {
            return Err(agg::too_many_groups(max));
        }
        let bytes: u64 = key.iter().map(Part::bytes).sum();
        self.charge.add(64 + bytes + (needed.len() * std::mem::size_of::<Acc>()) as u64)?;
        let g = all.keys.len() as u32;
        all.index.insert(key.clone(), g);
        all.keys.push(key);
        for (s, p) in all.sinks.iter_mut().zip(needed) {
            s.push(p);
        }
        Ok(g)
    }
}

/// Adds a block's values of a FLOAT column to their groups' sums: those in
/// [`agg::near_units`]'s range as integers, the rest to the exact sums. A
/// function of its own, once for each kind of column, so that the slices are
/// known apart and stay in registers and the decoding is not a match per row.
#[inline(never)]
fn add_floats(decode: impl Fn(i64) -> f64, vals: &[i64; BLOCK], rows: impl Iterator<Item = (usize, usize)>, sum: &mut [ExactSum], near: &mut [i128], n: &mut [i64]) {
    for (j, g) in rows {
        let x = decode(vals[j]);
        match agg::near_units(x) {
            Some(v) => near[g] += v,
            None => sum[g].add(x),
        }
        n[g] += 1;
    }
}

/// Whether aggregate `j` of `needed` is the first `approx_percentile` of its
/// field, whose sketch later ones of the field share.
fn sketch_of_first(needed: &[Projection], plan: &Plan, j: usize) -> bool {
    !(0..j).any(|i| matches!(needed[i], Projection::Agg { func: AggFn::ApproxPercentile(_), .. }) && plan.aggs[i] == plan.aggs[j])
}

/// Adds a block's values of a numeric column to their groups' sketches;
/// returns the bytes they now hold more. A function of its own per kind of
/// column, as [`add_floats`].
#[inline(never)]
fn add_sketches(decode: impl Fn(i64) -> f64, vals: &[i64; BLOCK], rows: impl Iterator<Item = (usize, usize)>, sk: &mut [Sketch]) -> u64 {
    let mut more = 0;
    for (j, g) in rows {
        more += sk[g].add_f64(decode(vals[j]));
    }
    more
}
