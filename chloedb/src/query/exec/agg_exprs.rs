//! Expressions of a row's fields computed from the columns a block of 128
//! rows at a time, for the groups and aggregates of [`super::agg_columns`]:
//! `SUM(a * b)`, `COUNT(*) FILTER (WHERE c)` (by then `COUNT(CASE WHEN c
//! THEN 1 END)`), `GROUP BY CASE .. END`, `AVG(EXTRACT(epoch FROM b) -
//! EXTRACT(epoch FROM a))`, and the sums `STDDEV` and `CORR` are made of.
//!
//! An expression is compiled into steps over a block of values of one type
//! ([`Ty`]), which give what the row evaluator ([`eval::eval`]) gives row by
//! row. A step is an operation that cannot fail on values of its types, so
//! computing it on every row of a block, where the row evaluator computes a
//! `CASE`'s branch or an `AND`'s later conditions on some rows only, changes
//! nothing. Where an integer would overflow, which the row evaluator makes
//! a FLOAT, the block's rows go to the row evaluator instead. An expression
//! with anything else in it (text functions, `ROUND`, `date_trunc`, a
//! comparison the row evaluator refuses) goes to the row evaluator for
//! every row, over the values the columns give, as do the rows of a block
//! where a value written in place is of another type than its field's.

use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::error::{Error, Result};
use crate::forward::{NumericColumn, NumericKind, BLOCK};
use crate::query::agg::cmp_int_float;
use crate::query::ir::eval::{self, Ref};
use crate::query::ir::{ArithOp, CmpOp, Expr, Func};
use crate::schema::{Field, FieldType, Logical, Schema};
use crate::segment::DocId;
use crate::timestamp;
use crate::value::Value;

/// The type of a step's values. `Pick` is one of a few texts (a `CASE`'s
/// branches): each row's is the index of its branch's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Null,
    Int,
    Float,
    Bool,
    Time,
    Pick,
}

/// A block's values of a step, an input or an output: the rows with one
/// (bit `j % 64` of word `j / 64`, as the columns' masks), and the values;
/// a row without one holds anything.
struct Lane {
    valid: [u64; 2],
    vals: Vals,
}

enum Vals {
    /// No row has a value (`Ty::Null`).
    None,
    /// INT values, or TIMESTAMPs' microseconds.
    Int(Box<[i64; BLOCK]>),
    Float(Box<[f64; BLOCK]>),
    /// Each row's truth.
    Bool([u64; 2]),
    /// Each row's text, by its index in the step's [`Step::pick`].
    Pick(Box<[u16; BLOCK]>),
    /// Each row's value as the row evaluator gives it.
    Any(Vec<Value>),
}

impl Vals {
    fn of(ty: Ty) -> Vals {
        match ty {
            Ty::Null => Vals::None,
            Ty::Int | Ty::Time => Vals::Int(Box::new([0; BLOCK])),
            Ty::Float => Vals::Float(Box::new([0.0; BLOCK])),
            Ty::Bool => Vals::Bool([0; 2]),
            Ty::Pick => Vals::Pick(Box::new([0; BLOCK])),
        }
    }
}

#[inline]
fn bit(m: [u64; 2], j: usize) -> bool {
    m[j / 64] >> (j % 64) & 1 == 1
}

#[inline]
fn set(m: &mut [u64; 2], j: usize, on: bool) {
    if on {
        m[j / 64] |= 1 << (j % 64);
    } else {
        m[j / 64] &= !(1 << (j % 64));
    }
}

const ALL: [u64; 2] = [u64::MAX; 2];

/// The rows `j` of a block where `f(j)`, a word at a time without a branch
/// per row.
#[inline(always)]
fn mask(f: impl Fn(usize) -> bool) -> [u64; 2] {
    let mut m = [0u64; 2];
    for (w, word) in m.iter_mut().enumerate() {
        let mut x = 0u64;
        for b in 0..64 {
            x |= (f(w * 64 + b) as u64) << b;
        }
        *word = x;
    }
    m
}

fn and(a: [u64; 2], b: [u64; 2]) -> [u64; 2] {
    [a[0] & b[0], a[1] & b[1]]
}

fn not(a: [u64; 2]) -> [u64; 2] {
    [!a[0], !a[1]]
}

fn or(a: [u64; 2], b: [u64; 2]) -> [u64; 2] {
    [a[0] | b[0], a[1] | b[1]]
}

/// What a step computes, of the steps before it (by index).
#[derive(Clone, Debug, PartialEq)]
enum Op {
    /// A field's values ([`Exprs::inputs`]).
    Input(usize),
    Lit(Value),
    Arith(ArithOp, usize, usize),
    Neg(usize),
    Abs(usize),
    Cmp(CmpOp, usize, usize),
    IsNull(usize),
    Not(usize),
    And(Vec<usize>),
    Or(Vec<usize>),
    /// A searched `CASE`: each condition's step and its value, then the
    /// `ELSE`'s.
    Case(Vec<(usize, Then)>, Then),
    Coalesce(Vec<usize>),
    Part(timestamp::Part, usize),
    /// A function of numbers ([`Func::eval_f64`]), NULL as NaN.
    Math(Func, Vec<usize>),
}

/// A `CASE` branch's value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Then {
    Null,
    Step(usize),
    /// A text of the step's [`Step::pick`].
    Pick(u16),
}

struct Step {
    op: Op,
    ty: Ty,
    /// The texts of a `Ty::Pick` step.
    pick: Vec<Value>,
}

/// An expression the groups or aggregates read: its last step, or None
/// when it goes to the row evaluator for every row.
struct Out {
    expr: Expr,
    step: Option<usize>,
}

/// The expressions of a query's groups and aggregates, compiled.
#[derive(Default)]
pub(super) struct Exprs {
    /// The fields they read, each with its type as queries give it.
    inputs: Vec<(usize, Ty)>,
    steps: Vec<Step>,
    outs: Vec<Out>,
}

/// One block's values of every step, input and output of [`Exprs`].
pub(super) struct Scratch {
    steps: Vec<Lane>,
    inputs: Vec<Lane>,
    /// Each output's values from the row evaluator, when it gave them.
    rows: Vec<Lane>,
    /// Whether the steps gave this block's values (else the row evaluator
    /// gave every output's).
    typed: bool,
    keys: Box<[i64; BLOCK]>,
}

/// A field's type as queries give its values: a BOOL or TIMESTAMP as one,
/// not as the integer stored.
fn input_ty(f: &Field) -> Option<Ty> {
    if f.multi {
        return None;
    }
    match (&f.ty, f.logical) {
        (FieldType::Int, None) => Some(Ty::Int),
        (FieldType::Int, Some(Logical::Bool)) => Some(Ty::Bool),
        (FieldType::Int, Some(Logical::Timestamp)) => Some(Ty::Time),
        (FieldType::Float, None) => Some(Ty::Float),
        _ => None,
    }
}

fn lit_ty(v: &Value) -> Option<Ty> {
    Some(match v {
        Value::Null => Ty::Null,
        Value::Int(_) => Ty::Int,
        Value::Float(_) => Ty::Float,
        Value::Bool(_) => Ty::Bool,
        Value::Timestamp(_) => Ty::Time,
        _ => return None,
    })
}

/// Whether `ty`'s values compare with each other's by [`eval::compare`]
/// without an error: numbers, BOOLs and times.
fn ordered(ty: Ty) -> bool {
    matches!(ty, Ty::Int | Ty::Float | Ty::Bool | Ty::Time)
}

impl Exprs {
    /// Adds `e`, an expression of `schema`'s single-valued INT and FLOAT
    /// fields (BOOL and TIMESTAMP ones too): its index as an output, or
    /// None if it reads another kind of field.
    pub(super) fn add(&mut self, e: &Expr, schema: &Schema) -> Option<usize> {
        if let Some(o) = self.outs.iter().position(|o| o.expr == *e) {
            return Some(o);
        }
        let mut fields = Vec::new();
        e.visit(&mut |x| {
            if let Expr::Field(f) = x {
                fields.push(*f as usize);
            }
        });
        for f in fields {
            self.input(f, schema)?;
        }
        let step = self.step(e, schema);
        self.outs.push(Out { expr: e.clone(), step });
        Some(self.outs.len() - 1)
    }

    /// Whether output `o`'s values are all FLOAT or NULL, as its steps make
    /// them, and as the row evaluator does too.
    pub(super) fn float(&self, o: usize) -> bool {
        self.outs[o].step.is_some_and(|s| self.steps[s].ty == Ty::Float)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.outs.is_empty()
    }

    /// How many outputs the steps compute, and how many the row evaluator.
    pub(super) fn counts(&self) -> (usize, usize) {
        let typed = self.outs.iter().filter(|o| o.step.is_some()).count();
        (typed, self.outs.len() - typed)
    }

    /// The fields the expressions read, in the order of
    /// [`Scratch::read`]'s inputs.
    pub(super) fn fields(&self) -> impl Iterator<Item = usize> + '_ {
        self.inputs.iter().map(|&(f, _)| f)
    }

    fn input(&mut self, f: usize, schema: &Schema) -> Option<usize> {
        if let Some(i) = self.inputs.iter().position(|&(g, _)| g == f) {
            return Some(i);
        }
        let ty = input_ty(schema.fields.get(f)?)?;
        self.inputs.push((f, ty));
        Some(self.inputs.len() - 1)
    }

    fn ty(&self, s: usize) -> Ty {
        self.steps[s].ty
    }

    fn push(&mut self, op: Op, ty: Ty, pick: Vec<Value>) -> usize {
        // The same step twice is computed once (STDDEV's sums share theirs).
        if let Some(i) = self.steps.iter().position(|s| s.op == op && s.ty == ty && s.pick == pick) {
            return i;
        }
        self.steps.push(Step { op, ty, pick });
        self.steps.len() - 1
    }

    fn lit(&mut self, v: Value) -> Option<usize> {
        let ty = lit_ty(&v)?;
        Some(self.push(Op::Lit(v), ty, Vec::new()))
    }

    fn null(&mut self) -> Option<usize> {
        self.lit(Value::Null)
    }

    /// A step of a condition: a BOOL or NULL.
    fn cond(&mut self, e: &Expr, schema: &Schema) -> Option<usize> {
        let s = self.step(e, schema)?;
        matches!(self.ty(s), Ty::Bool | Ty::Null).then_some(s)
    }

    /// `left op right`, as [`eval::compare`] orders them: text against a
    /// time names one.
    fn cmp(&mut self, op: CmpOp, left: &Expr, right: &Expr, schema: &Schema) -> Option<usize> {
        let text = |e: &Expr| match e {
            Expr::Lit(Value::Str(s)) => Some(s.clone()),
            _ => None,
        };
        let time = |this: &mut Self, s: &str, other: &Expr| -> Option<(usize, usize)> {
            let o = this.step(other, schema)?;
            if this.ty(o) != Ty::Time {
                return None;
            }
            let t = this.lit(Value::Timestamp(timestamp::parse(s)?))?;
            Some((t, o))
        };
        let (l, r) = match (text(left), text(right)) {
            (None, None) => (self.step(left, schema)?, self.step(right, schema)?),
            (Some(s), None) => time(self, &s, right)?,
            (None, Some(s)) => {
                let (t, o) = time(self, &s, left)?;
                (o, t)
            }
            _ => return None,
        };
        match (self.ty(l), self.ty(r)) {
            (Ty::Null, _) | (_, Ty::Null) => self.null(),
            (a, b) if ordered(a) && ordered(b) => Some(self.push(Op::Cmp(op, l, r), Ty::Bool, Vec::new())),
            _ => None,
        }
    }

    /// The steps of `e`, or None when they would not give the row
    /// evaluator's values.
    fn step(&mut self, e: &Expr, schema: &Schema) -> Option<usize> {
        let one = |ty: Ty| matches!(ty, Ty::Int | Ty::Float);
        match e {
            Expr::Field(f) => {
                let i = self.input(*f as usize, schema)?;
                let ty = self.inputs[i].1;
                Some(self.push(Op::Input(i), ty, Vec::new()))
            }
            Expr::Lit(v) => self.lit(v.clone()),
            Expr::Arith { op, left, right } => {
                let (l, r) = (self.step(left, schema)?, self.step(right, schema)?);
                let ty = match (self.ty(l), self.ty(r), op) {
                    (Ty::Null, _, _) | (_, Ty::Null, _) => return self.null(),
                    (Ty::Int, Ty::Int, ArithOp::Div) => Ty::Float,
                    (Ty::Int, Ty::Int, _) => Ty::Int,
                    (a, b, _) if one(a) && one(b) => Ty::Float,
                    (Ty::Time, Ty::Int, ArithOp::Add | ArithOp::Sub) | (Ty::Int, Ty::Time, ArithOp::Add) => Ty::Time,
                    _ => return None,
                };
                Some(self.push(Op::Arith(*op, l, r), ty, Vec::new()))
            }
            Expr::Neg(x) => {
                let x = self.step(x, schema)?;
                match self.ty(x) {
                    Ty::Null => self.null(),
                    ty if one(ty) => Some(self.push(Op::Neg(x), ty, Vec::new())),
                    _ => None,
                }
            }
            Expr::Cmp { op, left, right } => self.cmp(*op, left, right, schema),
            Expr::Between { expr, lo, hi } => {
                // TRUE unless a bound is on the wrong side: the comparisons
                // with the bounds there are, as `AND` takes them.
                let mut sides = Vec::new();
                if let Some(lo) = lo {
                    sides.push(self.cmp(CmpOp::Ge, expr, lo, schema)?);
                }
                if let Some(hi) = hi {
                    sides.push(self.cmp(CmpOp::Le, expr, hi, schema)?);
                }
                Some(self.push(Op::And(sides), Ty::Bool, Vec::new()))
            }
            Expr::In { expr, list } => {
                // TRUE if one is equal, else UNKNOWN if one is unknown: as
                // `OR` of the comparisons.
                let mut eqs = Vec::with_capacity(list.len());
                for y in list {
                    if matches!(y, Expr::Lit(Value::List(_))) {
                        return None;
                    }
                    eqs.push(self.cmp(CmpOp::Eq, expr, y, schema)?);
                }
                Some(self.push(Op::Or(eqs), Ty::Bool, Vec::new()))
            }
            Expr::IsNull(x) => {
                let x = self.step(x, schema)?;
                Some(self.push(Op::IsNull(x), Ty::Bool, Vec::new()))
            }
            Expr::Not(x) => {
                let x = self.cond(x, schema)?;
                match self.ty(x) {
                    Ty::Null => self.null(),
                    _ => Some(self.push(Op::Not(x), Ty::Bool, Vec::new())),
                }
            }
            Expr::And(xs) | Expr::Or(xs) => {
                let xs = xs.iter().map(|x| self.cond(x, schema)).collect::<Option<Vec<_>>>()?;
                let op = if matches!(e, Expr::And(_)) { Op::And(xs) } else { Op::Or(xs) };
                Some(self.push(op, Ty::Bool, Vec::new()))
            }
            Expr::Case { operand, whens, otherwise } => {
                let mut conds = Vec::with_capacity(whens.len());
                for w in whens {
                    conds.push(match operand {
                        // `CASE x WHEN y`: where x = y.
                        Some(x) => self.cmp(CmpOp::Eq, x, &w.when, schema)?,
                        None => self.cond(&w.when, schema)?,
                    });
                }
                let values: Vec<Option<&Expr>> = whens.iter().map(|w| Some(&w.then)).chain([otherwise.as_deref()]).collect();
                let is_text = |v: &Option<&Expr>| matches!(v, Some(Expr::Lit(Value::Str(_))));
                let is_null = |v: &Option<&Expr>| matches!(v, None | Some(Expr::Lit(Value::Null)));
                let (thens, ty, pick) = if values.iter().any(is_text) && values.iter().all(|v| is_text(v) || is_null(v)) {
                    // Texts of their own: each row's branch.
                    let mut pick: Vec<Value> = Vec::new();
                    let thens = values
                        .iter()
                        .map(|v| match v {
                            Some(Expr::Lit(t @ Value::Str(_))) => Then::Pick(match pick.iter().position(|p| p == t) {
                                Some(i) => i as u16,
                                None => {
                                    pick.push(t.clone());
                                    (pick.len() - 1) as u16
                                }
                            }),
                            _ => Then::Null,
                        })
                        .collect::<Vec<_>>();
                    (thens, Ty::Pick, pick)
                } else {
                    let mut ty = Ty::Null;
                    let mut thens = Vec::with_capacity(values.len());
                    for v in &values {
                        let t = match v {
                            None => Then::Null,
                            Some(v) => {
                                let s = self.step(v, schema)?;
                                match self.ty(s) {
                                    Ty::Null => Then::Null,
                                    // One type in every branch, or the
                                    // values' types vary by row.
                                    t if ordered(t) && (ty == Ty::Null || ty == t) => {
                                        ty = t;
                                        Then::Step(s)
                                    }
                                    _ => return None,
                                }
                            }
                        };
                        thens.push(t);
                    }
                    if ty == Ty::Null {
                        return self.null();
                    }
                    (thens, ty, Vec::new())
                };
                let otherwise = *thens.last().expect("the ELSE");
                let arms = conds.into_iter().zip(thens).collect();
                Some(self.push(Op::Case(arms, otherwise), ty, pick))
            }
            Expr::Call { func, args } => match func {
                Func::Coalesce => {
                    let mut ty = Ty::Null;
                    let mut xs = Vec::with_capacity(args.len());
                    for a in args {
                        let s = self.step(a, schema)?;
                        match self.ty(s) {
                            Ty::Null => {}
                            t if ordered(t) && (ty == Ty::Null || ty == t) => {
                                ty = t;
                                xs.push(s);
                            }
                            _ => return None,
                        }
                    }
                    if ty == Ty::Null {
                        return self.null();
                    }
                    Some(self.push(Op::Coalesce(xs), ty, Vec::new()))
                }
                Func::DatePart => {
                    let [Expr::Lit(Value::Str(p)), t] = args.as_slice() else { return None };
                    let part = timestamp::Part::parse(p)?;
                    let t = self.step(t, schema)?;
                    match self.ty(t) {
                        Ty::Null => self.null(),
                        Ty::Time => {
                            let ty = match timestamp::part(0, part) {
                                timestamp::PartValue::Int(_) => Ty::Int,
                                timestamp::PartValue::Float(_) => Ty::Float,
                            };
                            Some(self.push(Op::Part(part, t), ty, Vec::new()))
                        }
                        _ => None,
                    }
                }
                Func::Abs => {
                    let [x] = args.as_slice() else { return None };
                    let x = self.step(x, schema)?;
                    match self.ty(x) {
                        Ty::Null => self.null(),
                        ty if one(ty) => Some(self.push(Op::Abs(x), ty, Vec::new())),
                        _ => None,
                    }
                }
                Func::Ln | Func::Log | Func::Exp | Func::Sqrt | Func::Pow | Func::Least | Func::Greatest | Func::Sigmoid | Func::Clamp => {
                    let mut xs = Vec::with_capacity(args.len());
                    for a in args {
                        let s = self.step(a, schema)?;
                        if !matches!(self.ty(s), Ty::Null | Ty::Int | Ty::Float | Ty::Bool) {
                            return None;
                        }
                        xs.push(s);
                    }
                    Some(self.push(Op::Math(*func, xs), Ty::Float, Vec::new()))
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Room for one block's values, the literals' filled in.
    pub(super) fn scratch(&self) -> Scratch {
        let steps = self
            .steps
            .iter()
            .map(|s| {
                let mut lane = Lane { valid: [0; 2], vals: Vals::of(s.ty) };
                if let Op::Lit(v) = &s.op {
                    lane.valid = if matches!(v, Value::Null) { [0; 2] } else { ALL };
                    match (&mut lane.vals, v) {
                        (Vals::Int(x), Value::Int(v) | Value::Timestamp(v)) => x.fill(*v),
                        (Vals::Float(x), Value::Float(v)) => x.fill(*v),
                        (Vals::Bool(b), Value::Bool(v)) => *b = if *v { ALL } else { [0; 2] },
                        _ => {}
                    }
                }
                lane
            })
            .collect();
        Scratch {
            steps,
            inputs: self.inputs.iter().map(|&(_, ty)| Lane { valid: [0; 2], vals: Vals::of(ty) }).collect(),
            rows: self.outs.iter().map(|_| Lane { valid: [0; 2], vals: Vals::Any(vec![Value::Null; BLOCK]) }).collect(),
            typed: false,
            keys: Box::new([0; BLOCK]),
        }
    }

    /// Reads block `b` of input `i` from its field's column `col`, then
    /// the values written in place of the block's rows (`written`, by
    /// docID), as [`super::agg_columns`]' `value` reads a row's.
    pub(super) fn read(&self, sc: &mut Scratch, i: usize, schema: &Schema, col: &NumericColumn, b: usize, written: &[(DocId, &Value)]) {
        let (f, ty) = self.inputs[i];
        let lane = &mut sc.inputs[i];
        if matches!(lane.vals, Vals::Any(_)) {
            lane.vals = Vals::of(ty);
        }
        let keys = &mut sc.keys;
        let valid = col.block_keys_of(b, keys);
        lane.valid = valid;
        // A column of another kind than its field's (never written so):
        // its values as they are.
        let own = (col.kind() == NumericKind::Int) == (ty != Ty::Float);
        if !own {
            lane.vals = Vals::Any((0..BLOCK).map(|j| if bit(valid, j) { schema.fields[f].output(col_value(col, keys[j])) } else { Value::Null }).collect());
        }
        match &mut lane.vals {
            Vals::Int(x) => x.copy_from_slice(&keys[..]),
            Vals::Float(x) => {
                let kind = col.kind();
                for j in 0..BLOCK {
                    x[j] = kind.decode(keys[j]);
                }
            }
            Vals::Bool(t) => *t = mask(|j| keys[j] != 0),
            _ => {}
        }
        let lo = (b * BLOCK) as DocId;
        for &(d, v) in written {
            let j = (d - lo) as usize;
            let v = schema.fields[f].output(v.clone());
            let fits = match (&mut lane.vals, &v) {
                (_, Value::Null) => true,
                (Vals::Int(x), Value::Int(v)) if ty == Ty::Int => {
                    x[j] = *v;
                    true
                }
                (Vals::Int(x), Value::Timestamp(v)) if ty == Ty::Time => {
                    x[j] = *v;
                    true
                }
                (Vals::Float(x), Value::Float(v)) => {
                    x[j] = *v;
                    true
                }
                (Vals::Bool(t), Value::Bool(v)) => {
                    set(t, j, *v);
                    true
                }
                (Vals::Any(xs), v) => {
                    xs[j] = v.clone();
                    true
                }
                _ => false,
            };
            if !fits {
                // A value of another type: the block's values as values.
                let vals: Vec<Value> = (0..BLOCK).map(|j| lane_value(lane, ty, &[], j)).collect();
                lane.vals = Vals::Any(vals);
                if let Vals::Any(xs) = &mut lane.vals {
                    xs[j] = v.clone();
                }
            }
            set(&mut lane.valid, j, !matches!(v, Value::Null));
        }
    }

    /// Computes every output over the block's inputs ([`Self::read`]), for
    /// the rows of `picked` at least.
    pub(super) fn eval(&self, sc: &mut Scratch, picked: [u64; 2]) -> Result<()> {
        sc.typed = sc.inputs.iter().all(|l| !matches!(l.vals, Vals::Any(_))) && self.run(sc);
        for (o, out) in self.outs.iter().enumerate() {
            if sc.typed && out.step.is_some() {
                continue;
            }
            let Scratch { inputs, rows, .. } = &mut *sc;
            let lane = &mut rows[o];
            lane.valid = [0; 2];
            let Vals::Any(xs) = &mut lane.vals else { unreachable!("a row-evaluated output holds values") };
            for (w, &word) in picked.iter().enumerate() {
                let mut x = word;
                while x != 0 {
                    let j = w * 64 + x.trailing_zeros() as usize;
                    x &= x - 1;
                    let v = eval::eval(&out.expr, &|r| match r {
                        Ref::Field(f) => {
                            let i = self.inputs.iter().position(|&(g, _)| g == f as usize).ok_or_else(|| Error::Query("an expression of the columns reads a field it was not given".into()))?;
                            Ok(lane_value(&inputs[i], self.inputs[i].1, &[], j))
                        }
                        _ => Err(Error::Query("an expression of the columns reads the row's fields".into())),
                    })?;
                    set(&mut lane.valid, j, !matches!(v, Value::Null));
                    xs[j] = v;
                }
            }
        }
        Ok(())
    }

    /// Output `o`'s lane over the block, and its step's (None from the row
    /// evaluator).
    fn out<'s>(&'s self, sc: &'s Scratch, o: usize) -> (&'s Lane, Option<&'s Step>) {
        match self.outs[o].step {
            Some(s) if sc.typed => (&sc.steps[s], Some(&self.steps[s])),
            _ => (&sc.rows[o], None),
        }
    }

    /// The rows of the block where output `o` has a value.
    pub(super) fn valid(&self, sc: &Scratch, o: usize) -> [u64; 2] {
        self.out(sc, o).0.valid
    }

    /// Output `o`'s value of the block's row `j`.
    pub(super) fn value(&self, sc: &Scratch, o: usize, j: usize) -> Value {
        match self.out(sc, o) {
            (lane, Some(step)) => lane_value(lane, step.ty, &step.pick, j),
            (lane, None) => lane_value(lane, Ty::Null, &[], j),
        }
    }

    /// Output `o`'s INT values over the block, when its steps gave them.
    pub(super) fn ints<'s>(&'s self, sc: &'s Scratch, o: usize) -> Option<&'s [i64; BLOCK]> {
        match self.out(sc, o) {
            (Lane { vals: Vals::Int(x), .. }, Some(Step { ty: Ty::Int, .. })) => Some(x),
            _ => None,
        }
    }

    /// Output `o`'s FLOAT values over the block, when its steps gave them.
    pub(super) fn floats<'s>(&'s self, sc: &'s Scratch, o: usize) -> Option<&'s [f64; BLOCK]> {
        match self.out(sc, o).0.vals {
            Vals::Float(ref x) => Some(x),
            _ => None,
        }
    }

    /// Output `o`'s value of row `j` as the bits of a group key part (as
    /// [`super::agg_columns`]' `bits`): text by its number in `texts`,
    /// this segment's texts so far.
    pub(super) fn bits<S: BuildHasher>(&self, sc: &Scratch, o: usize, j: usize, texts: &mut HashMap<String, u32, S>) -> (u8, u64) {
        let (lane, _) = self.out(sc, o);
        if !bit(lane.valid, j) {
            return (0, 0);
        }
        match &lane.vals {
            Vals::None => (0, 0),
            Vals::Int(x) => (1, x[j] as u64),
            Vals::Float(x) => (2, x[j].to_bits()),
            Vals::Bool(t) => (3, bit(*t, j) as u64),
            Vals::Pick(c) => (7, c[j] as u64),
            Vals::Any(xs) => match &xs[j] {
                Value::Str(s) => {
                    let n = texts.len() as u32;
                    (6, *texts.entry(s.clone()).or_insert(n) as u64)
                }
                v => super::agg_columns::bits(v),
            },
        }
    }

    /// Computes the steps; false where an integer overflowed, which the
    /// row evaluator makes a FLOAT.
    fn run(&self, sc: &mut Scratch) -> bool {
        for (i, step) in self.steps.iter().enumerate() {
            let (before, rest) = sc.steps.split_at_mut(i);
            let out = &mut rest[0];
            let ok = match &step.op {
                Op::Lit(_) => true,
                Op::Input(k) => {
                    let src = &sc.inputs[*k];
                    out.valid = src.valid;
                    match (&mut out.vals, &src.vals) {
                        (Vals::Int(x), Vals::Int(y)) => x.copy_from_slice(&y[..]),
                        (Vals::Float(x), Vals::Float(y)) => x.copy_from_slice(&y[..]),
                        (Vals::Bool(x), Vals::Bool(y)) => *x = *y,
                        _ => unreachable!("an input's lane is of its type"),
                    }
                    true
                }
                Op::Arith(op, l, r) => arith(*op, &before[*l], &before[*r], out),
                Op::Neg(x) => unary(&before[*x], out, i64::checked_neg, |v| -v),
                Op::Abs(x) => unary(&before[*x], out, i64::checked_abs, f64::abs),
                Op::Cmp(op, l, r) => {
                    cmp(*op, &before[*l], &before[*r], out);
                    true
                }
                Op::IsNull(x) => {
                    out.valid = ALL;
                    out.vals = Vals::Bool(not(before[*x].valid));
                    true
                }
                Op::Not(x) => {
                    let c = &before[*x];
                    out.valid = c.valid;
                    out.vals = Vals::Bool(and(not(truths(c)), c.valid));
                    true
                }
                Op::And(xs) | Op::Or(xs) => {
                    // FALSE (TRUE for OR) where one is, else UNKNOWN where
                    // one is.
                    let all = matches!(step.op, Op::And(_));
                    let (mut decided, mut unknown) = ([0; 2], [0; 2]);
                    for &x in xs {
                        let c = &before[x];
                        let t = if all { not(truths(c)) } else { truths(c) };
                        decided = or(decided, and(t, c.valid));
                        unknown = or(unknown, not(c.valid));
                    }
                    out.valid = or(decided, not(unknown));
                    out.vals = Vals::Bool(if all { and(not(decided), not(unknown)) } else { decided });
                    true
                }
                Op::Case(arms, otherwise) => {
                    let mut done = [0u64; 2];
                    out.valid = [0; 2];
                    for (c, then) in arms.iter().map(|(c, t)| (Some(*c), *t)).chain([(None, *otherwise)]) {
                        let hit = match c {
                            Some(c) => and(and(truths(&before[c]), before[c].valid), not(done)),
                            None => not(done),
                        };
                        take(out, hit, then, before);
                        done = or(done, hit);
                    }
                    true
                }
                Op::Coalesce(xs) => {
                    out.valid = [0; 2];
                    for &x in xs {
                        let hit = and(before[x].valid, not(out.valid));
                        take(out, hit, Then::Step(x), before);
                    }
                    true
                }
                Op::Part(p, t) => {
                    let src = &before[*t];
                    let Vals::Int(ms) = &src.vals else { unreachable!("a time's lane holds integers") };
                    out.valid = src.valid;
                    match &mut out.vals {
                        Vals::Int(x) => {
                            for j in 0..BLOCK {
                                if let timestamp::PartValue::Int(v) = timestamp::part(ms[j], *p) {
                                    x[j] = v;
                                }
                            }
                        }
                        Vals::Float(x) => {
                            for j in 0..BLOCK {
                                if let timestamp::PartValue::Float(v) = timestamp::part(ms[j], *p) {
                                    x[j] = v;
                                }
                            }
                        }
                        _ => unreachable!("a part of a time is a number"),
                    }
                    true
                }
                Op::Math(func, xs) => {
                    let mut args = vec![0.0; xs.len()];
                    let Vals::Float(y) = &mut out.vals else { unreachable!("a function of numbers is a FLOAT") };
                    let mut valid = [0; 2];
                    for j in 0..BLOCK {
                        for (a, &x) in args.iter_mut().zip(xs) {
                            *a = f64_at(&before[x], j).unwrap_or(f64::NAN);
                        }
                        let r = func.eval_f64(&args);
                        set(&mut valid, j, !r.is_nan());
                        y[j] = r;
                    }
                    out.valid = valid;
                    true
                }
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

/// A numeric column's value of a key, as [`super::agg_columns`]' `value`
/// makes it.
fn col_value(col: &NumericColumn, k: i64) -> Value {
    match col.kind() {
        NumericKind::Int => Value::Int(k),
        kind => Value::Float(kind.decode(k)),
    }
}

/// A condition lane's truths (none for a NULL step's).
fn truths(l: &Lane) -> [u64; 2] {
    match l.vals {
        Vals::Bool(t) => t,
        _ => [0; 2],
    }
}

/// Row `j`'s value of a lane of type `ty` (of a step with texts `pick`).
fn lane_value(l: &Lane, ty: Ty, pick: &[Value], j: usize) -> Value {
    if !bit(l.valid, j) {
        return Value::Null;
    }
    match &l.vals {
        Vals::None => Value::Null,
        Vals::Int(x) if ty == Ty::Time => Value::Timestamp(x[j]),
        Vals::Int(x) => Value::Int(x[j]),
        Vals::Float(x) => Value::Float(x[j]),
        Vals::Bool(t) => Value::Bool(bit(*t, j)),
        Vals::Pick(c) => pick[c[j] as usize].clone(),
        Vals::Any(xs) => xs[j].clone(),
    }
}

/// Row `j`'s value of a numeric or BOOL lane as a double, as
/// [`Value::as_f64`] makes it; None without one.
fn f64_at(l: &Lane, j: usize) -> Option<f64> {
    if !bit(l.valid, j) {
        return None;
    }
    match &l.vals {
        Vals::Int(x) => Some(x[j] as f64),
        Vals::Float(x) => Some(x[j]),
        Vals::Bool(t) => Some(bit(*t, j) as i64 as f64),
        _ => None,
    }
}

/// Row `j`'s value of an INT, BOOL or TIMESTAMP lane as an integer.
#[inline]
fn int_at(l: &Lane, j: usize) -> i64 {
    match &l.vals {
        Vals::Int(x) => x[j],
        Vals::Bool(t) => bit(*t, j) as i64,
        _ => 0,
    }
}

/// Copies `then`'s values of the rows `hit` into `out`.
fn take(out: &mut Lane, hit: [u64; 2], then: Then, before: &[Lane]) {
    match then {
        Then::Null => {}
        Then::Pick(code) => {
            let Vals::Pick(c) = &mut out.vals else { unreachable!("texts of a CASE") };
            for j in 0..BLOCK {
                c[j] = if bit(hit, j) { code } else { c[j] };
            }
            out.valid = or(out.valid, hit);
        }
        Then::Step(s) => {
            let src = &before[s];
            let rows = and(hit, src.valid);
            match (&mut out.vals, &src.vals) {
                (Vals::Int(x), Vals::Int(y)) => {
                    for j in 0..BLOCK {
                        x[j] = if bit(rows, j) { y[j] } else { x[j] };
                    }
                }
                (Vals::Float(x), Vals::Float(y)) => {
                    for j in 0..BLOCK {
                        x[j] = if bit(rows, j) { y[j] } else { x[j] };
                    }
                }
                (Vals::Bool(x), Vals::Bool(y)) => *x = or(and(*x, not(rows)), and(*y, rows)),
                _ => unreachable!("a CASE's branches are of its type"),
            }
            out.valid = or(out.valid, rows);
        }
    }
}

/// `l op r` of an arithmetic step, as [`eval`]'s `arith`: INT (or a
/// time plus or minus microseconds) when the step is, else FLOAT; false
/// where an integer overflowed (or a time left a TIMESTAMP's range, which
/// the row evaluator refuses).
fn arith(op: ArithOp, l: &Lane, r: &Lane, out: &mut Lane) -> bool {
    let valid = and(l.valid, r.valid);
    out.valid = valid;
    match (&mut out.vals, &l.vals, &r.vals) {
        (Vals::Int(z), Vals::Int(x), Vals::Int(y)) => {
            // A time plus microseconds has the time on either side.
            let over = |f: fn(i64, i64) -> (i64, bool), z: &mut [i64; BLOCK]| {
                let mut over = [0u64; 2];
                for j in 0..BLOCK {
                    let (v, o) = f(x[j], y[j]);
                    z[j] = v;
                    over[j / 64] |= (o as u64) << (j % 64);
                }
                and(over, valid) == [0, 0]
            };
            match op {
                ArithOp::Add => over(i64::overflowing_add, z),
                ArithOp::Sub => over(i64::overflowing_sub, z),
                ArithOp::Mul => over(i64::overflowing_mul, z),
                // i64::MIN % -1 overflows; the remainder is 0. NULL for 0.
                ArithOp::Mod => {
                    for j in 0..BLOCK {
                        z[j] = if y[j] == 0 { 0 } else { x[j].wrapping_rem(y[j]) };
                    }
                    out.valid = and(valid, mask(|j| y[j] != 0));
                    true
                }
                ArithOp::Div => unreachable!("INT / INT is a FLOAT"),
            }
        }
        (Vals::Float(z), _, _) => {
            let (mut ta, mut tb) = ([0f64; BLOCK], [0f64; BLOCK]);
            let a = doubles(l, &mut ta);
            let b = doubles(r, &mut tb);
            match op {
                ArithOp::Add => (0..BLOCK).for_each(|j| z[j] = a[j] + b[j]),
                ArithOp::Sub => (0..BLOCK).for_each(|j| z[j] = a[j] - b[j]),
                ArithOp::Mul => (0..BLOCK).for_each(|j| z[j] = a[j] * b[j]),
                // NULL for a divisor of 0.
                ArithOp::Div | ArithOp::Mod => {
                    if op == ArithOp::Div {
                        (0..BLOCK).for_each(|j| z[j] = a[j] / b[j]);
                    } else {
                        (0..BLOCK).for_each(|j| z[j] = a[j] % b[j]);
                    }
                    out.valid = and(valid, mask(|j| b[j] != 0.0));
                }
            }
            true
        }
        _ => unreachable!("an arithmetic step's lanes are numbers"),
    }
}

/// A lane of numbers as doubles: its own, or its integers written to `tmp`.
fn doubles<'a>(l: &'a Lane, tmp: &'a mut [f64; BLOCK]) -> &'a [f64; BLOCK] {
    match &l.vals {
        Vals::Float(x) => x,
        Vals::Int(x) => {
            for j in 0..BLOCK {
                tmp[j] = x[j] as f64;
            }
            tmp
        }
        _ => unreachable!("an arithmetic step's lanes are numbers"),
    }
}

/// `-x` or `abs(x)` of an INT or FLOAT lane; false where an integer
/// overflowed.
fn unary(x: &Lane, out: &mut Lane, int: fn(i64) -> Option<i64>, float: fn(f64) -> f64) -> bool {
    out.valid = x.valid;
    match (&mut out.vals, &x.vals) {
        (Vals::Int(z), Vals::Int(y)) => {
            let mut bad = [0u64; 2];
            for j in 0..BLOCK {
                match int(y[j]) {
                    Some(v) => z[j] = v,
                    None => set(&mut bad, j, true),
                }
            }
            and(bad, x.valid) == [0, 0]
        }
        (Vals::Float(z), Vals::Float(y)) => {
            for j in 0..BLOCK {
                z[j] = float(y[j]);
            }
            true
        }
        _ => unreachable!("a sign of a number"),
    }
}

/// `l op r` of two lanes of numbers, BOOLs or times, as [`eval::compare`]
/// orders them.
fn cmp(op: CmpOp, l: &Lane, r: &Lane, out: &mut Lane) {
    use std::cmp::Ordering;
    let test = |o: Ordering| match op {
        CmpOp::Eq => o == Ordering::Equal,
        CmpOp::Ne => o != Ordering::Equal,
        CmpOp::Lt => o == Ordering::Less,
        CmpOp::Le => o != Ordering::Greater,
        CmpOp::Gt => o == Ordering::Greater,
        CmpOp::Ge => o != Ordering::Less,
    };
    let t = match (&l.vals, &r.vals) {
        (Vals::Float(x), Vals::Float(y)) => mask(|j| test(x[j].total_cmp(&y[j]))),
        (Vals::Int(x), Vals::Int(y)) => mask(|j| test(x[j].cmp(&y[j]))),
        (Vals::Float(x), _) => mask(|j| test(cmp_int_float(int_at(r, j), x[j]).reverse())),
        (_, Vals::Float(y)) => mask(|j| test(cmp_int_float(int_at(l, j), y[j]))),
        _ => mask(|j| test(int_at(l, j).cmp(&int_at(r, j)))),
    };
    out.valid = and(l.valid, r.valid);
    out.vals = Vals::Bool(and(t, out.valid));
}
