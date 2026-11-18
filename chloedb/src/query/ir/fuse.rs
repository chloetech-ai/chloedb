//! Paths of JSON fields, and parts of times, that the executor reads
//! itself.
//!
//! The planner of compound queries reads a path outside `WHERE` (`GROUP BY
//! attrs.color`, `AVG(attrs.price)`, `ORDER BY attrs.price`) with
//! operators: a leaf returns each row's object, and `json_extract` above it
//! parses them one by one. The executor reads those paths from the field's
//! terms instead ([`crate::json::PathValues`]), and groups by a part of a
//! TIMESTAMP field's times (`GROUP BY date_trunc('day', at)`) from the
//! field's column, so [`paths`] moves the operators over such a leaf into
//! it when the executor runs them as they would (a group by a time, as
//! the first shape, not the order of the second):
//!
//! * `Aggregate` of fields and paths over `Project` of fields over a
//!   `Search` becomes `Aggregate` over the `Search`, under a `Project` of
//!   its outputs (of fields alone too, when each holds one value: the
//!   aggregates under `ROUND(SUM(x), 2)` or a HAVING); with the `Filter`, `Sort`, `Limit` and `Project` above it,
//!   when the executor takes those too;
//! * `Project` over `[Limit]` over `Sort` by a path (then possibly the
//!   primary key) over `Project` of fields over a `Search` becomes the
//!   `Search` ordered by the path, one leaf when the columns are fields and
//!   paths and the path is among them, else under the query's `Project`,
//!   the leaf giving the path as a column after the rows' fields (which a
//!   coordinator merges the rows of shards by).
//!
//! In a plan this process runs itself ([`paths`] with `exprs`, from
//! [`crate::query::compound::fuse_columns`]), an `Aggregate` whose keys or
//! aggregates are expressions of single-valued numeric fields
//! (`SUM(a * b)`, `AVG(EXTRACT(epoch FROM end) - EXTRACT(epoch FROM
//! start))`, `COUNT(CASE WHEN c THEN 1 END)`, which `COUNT(*) FILTER
//! (WHERE c)` is, `GROUP BY CASE WHEN x < 1 THEN 'a' ELSE 'b' END`) moves
//! into its leaf too, and the executor computes them from the columns
//! ([`computable`]). A coordinator's leaves go to shards that may be of an
//! older version, which would not raise them, so its plans keep them above.
//!
//! A node is moved into a leaf only when the leaf raises to the executor's
//! SELECT ([`super::Plan::to_select`]); a query whose plan has none of
//! these shapes plans as before. Joins are left as they are: a leaf on the
//! right of one is restricted to the left's keys, which a leaf that groups
//! or ranks its rows does not allow.

use super::{raise, Agg, Column, Expr, Func, Node, Search, SortKey};
use crate::query::logical::AggFn;
use crate::schema::{FieldType, Logical, Schema};
use crate::value::Value;

/// Moves the operators over leaves that read paths into the leaves
/// ([module docs](self)), in `n` and below it; with `exprs`, only the
/// aggregates of expressions, in a plan already fused without them.
pub(crate) fn paths(n: &mut Node, catalog: &[&Schema], exprs: bool) {
    if matches!(n, Node::Join { .. } | Node::KeyOrder { .. }) {
        return;
    }
    for c in n.inputs_mut() {
        paths(c, catalog, exprs);
    }
    let fused = match exprs {
        false => aggregate(n, catalog).or_else(|| groups(n, catalog)).or_else(|| ordered(n, catalog)),
        true => computed(n, catalog).or_else(|| groups(n, catalog)),
    };
    if let Some(f) = fused {
        *n = f;
    }
}

/// Whether the executor computes `e` from the columns of `schema`'s
/// table, row by row as the row evaluator does ([`super::eval`]): an
/// expression of indexed single-valued INT and FLOAT fields (BOOL and
/// TIMESTAMP ones too) and values, by arithmetic, comparisons, conditions,
/// `CASE`, and scalar functions of numbers, text and times. Not a field
/// alone, which is a column as it is.
pub(super) fn computable(e: &Expr, schema: &Schema) -> bool {
    fn of(e: &Expr, schema: &Schema) -> bool {
        let all = |xs: &[Expr]| xs.iter().all(|x| of(x, schema));
        match e {
            Expr::Field(f) => schema.fields.get(*f as usize).is_some_and(|fld| fld.indexed && !fld.multi && matches!(fld.ty, FieldType::Int | FieldType::Float)),
            Expr::Lit(v) => !matches!(v, Value::List(_) | Value::Vector(_)),
            Expr::Not(x) | Expr::IsNull(x) | Expr::Neg(x) => of(x, schema),
            Expr::And(xs) | Expr::Or(xs) | Expr::Concat(xs) => all(xs),
            Expr::Cmp { left, right, .. } | Expr::Arith { left, right, .. } => of(left, schema) && of(right, schema),
            Expr::Between { expr, lo, hi } => of(expr, schema) && lo.as_deref().is_none_or(|x| of(x, schema)) && hi.as_deref().is_none_or(|x| of(x, schema)),
            Expr::In { expr, list } => of(expr, schema) && all(list),
            Expr::Case { operand, whens, otherwise } => {
                operand.as_deref().is_none_or(|x| of(x, schema)) && whens.iter().all(|w| of(&w.when, schema) && of(&w.then, schema)) && otherwise.as_deref().is_none_or(|x| of(x, schema))
            }
            Expr::Call { func, args } => {
                matches!(
                    func,
                    Func::Lower
                        | Func::Upper
                        | Func::Length
                        | Func::Abs
                        | Func::Round
                        | Func::Coalesce
                        | Func::Ln
                        | Func::Log
                        | Func::Exp
                        | Func::Sqrt
                        | Func::Pow
                        | Func::Least
                        | Func::Greatest
                        | Func::Sigmoid
                        | Func::Clamp
                        | Func::DateTrunc
                        | Func::DatePart
                        | Func::Strftime
                        | Func::DateBin
                        | Func::Hash
                ) && all(args)
            }
            _ => false,
        }
    }
    !matches!(e, Expr::Field(_)) && of(e, schema)
}

/// Whether `e` may give a TIMESTAMP: a group's value keeps a time as its
/// integer, which the answer's types give back only for a column's.
pub(super) fn may_be_time(e: &Expr, schema: &Schema) -> bool {
    let any = |xs: &[Expr]| xs.iter().any(|x| may_be_time(x, schema));
    match e {
        Expr::Field(f) => schema.fields.get(*f as usize).is_some_and(|fld| fld.logical == Some(Logical::Timestamp)),
        Expr::Lit(v) => matches!(v, Value::Timestamp(_)),
        Expr::Arith { left, right, .. } => may_be_time(left, schema) || may_be_time(right, schema),
        Expr::Case { whens, otherwise, .. } => whens.iter().any(|w| may_be_time(&w.then, schema)) || otherwise.as_deref().is_some_and(|x| may_be_time(x, schema)),
        Expr::Call { func: Func::DateTrunc | Func::DateBin, .. } => true,
        Expr::Call { func: Func::Coalesce | Func::Least | Func::Greatest | Func::Clamp, args } => any(args),
        _ => false,
    }
}

/// `e`, an expression of the rows of a `Project` of `cols`, as one of the
/// rows of the Search below, when each column it reads is a field.
fn of_fields(e: &Expr, cols: &[Column]) -> Option<Expr> {
    let mut x = e.clone();
    x.visit_mut(&mut |y| {
        if let Expr::Output(i) = y {
            match cols.get(*i as usize).map(|c| &c.expr) {
                Some(f @ Expr::Field(_)) => *y = f.clone(),
                _ => return Err(crate::error::Error::Query(String::new())),
            }
        }
        Ok(())
    })
    .ok()?;
    Some(x)
}

/// `Aggregate` over `Project` of fields over a `Search`, whose keys or
/// aggregates are expressions the executor computes ([`computable`]), as
/// an `Aggregate` over the Search under a `Project` of its outputs: a
/// leaf. Its aggregates are `COUNT`, `SUM`, `AVG`, `MIN` and `MAX`, which
/// the columns' groups add as the rows' do; at most four keys, none of
/// them a time.
fn computed(n: &Node, catalog: &[&Schema]) -> Option<Node> {
    let Node::Aggregate { input, keys, aggs } = n else { return None };
    let Node::Project { input: below, columns: cols } = &**input else { return None };
    let Node::Search(s) = &**below else { return None };
    if !s.order.is_empty() || !s.phases.is_empty() || s.per.is_some() || keys.len() > 4 {
        return None;
    }
    if !aggs.iter().all(|a| matches!(a.func, AggFn::Count | AggFn::Sum | AggFn::Avg | AggFn::Min | AggFn::Max)) {
        return None;
    }
    let schema = table(catalog, s)?;
    let mut any = false;
    let mut read = |e: &Expr, key: bool| -> Option<Expr> {
        if let Some((x, _)) = column(e, cols, schema) {
            return groupable(schema, &x).then_some(x);
        }
        let x = of_fields(e, cols)?;
        any = true;
        (computable(&x, schema) && !(key && may_be_time(&x, schema))).then_some(x)
    };
    let keys2 = keys.iter().map(|k| read(k, true)).collect::<Option<Vec<_>>>()?;
    let aggs2 = aggs
        .iter()
        .map(|a| {
            let arg = match &a.arg {
                None => None,
                Some(e) => Some(read(e, false)?),
            };
            Some(Agg { func: a.func, arg })
        })
        .collect::<Option<Vec<_>>>()?;
    if !any {
        return None;
    }
    let agg = Node::Aggregate { input: Box::new(Node::Search(s.clone())), keys: keys2, aggs: aggs2 };
    let leaf = Node::Project { input: Box::new(agg), columns: outputs(keys.len() + aggs.len()) };
    raises(&leaf, schema).then_some(leaf)
}

/// The table `s` searches.
fn table<'s>(catalog: &[&'s Schema], s: &Search) -> Option<&'s Schema> {
    catalog.iter().copied().find(|t| t.table.eq_ignore_ascii_case(&s.table))
}

/// Whether the executor takes `n` as a leaf of `schema`'s table.
fn raises(n: &Node, schema: &Schema) -> bool {
    raise::select_of(n, schema).is_ok()
}

/// What a column of a leaf reads ([`column`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Read {
    Field,
    Path,
    /// A part of a TIMESTAMP field's times.
    Time,
}

/// What `e`, an expression of the rows of a `Project` of `cols`, reads of
/// the rows of the Search below: a field the Project passes on, a path of
/// an indexed JSON field it passes on (`json_extract(field, path [,
/// type])`), or a part of the times of an indexed TIMESTAMP field of one
/// value it passes on (`date_trunc(unit, field)`, `date_part(part,
/// field)`, `strftime(format, field)`, the first given as text), and which
/// of those it is. None for any other expression.
fn column(e: &Expr, cols: &[Column], schema: &Schema) -> Option<(Expr, Read)> {
    let field = |i: u32| match cols.get(i as usize)?.expr {
        Expr::Field(f) => Some(f),
        _ => None,
    };
    match e {
        Expr::Output(i) => Some((Expr::Field(field(*i)?), Read::Field)),
        Expr::Call { func: func @ (Func::DateTrunc | Func::DatePart | Func::Strftime), args } => {
            let [how @ Expr::Lit(Value::Str(_)), Expr::Output(i)] = args.as_slice() else { return None };
            let f = field(*i)?;
            let fld = schema.fields.get(f as usize)?;
            if fld.logical != Some(crate::schema::Logical::Timestamp) || fld.multi || !fld.indexed {
                return None;
            }
            Some((Expr::Call { func: *func, args: vec![how.clone(), Expr::Field(f)] }, Read::Time))
        }
        // `date_bin(width, field [, origin])`, the width as text and the origin a literal.
        Expr::Call { func: Func::DateBin, args } => {
            let (width @ Expr::Lit(Value::Str(_)), Expr::Output(i)) = (args.first()?, args.get(1)?) else { return None };
            let origin = args.get(2);
            if origin.is_some_and(|o| !matches!(o, Expr::Lit(Value::Timestamp(_) | Value::Str(_)))) || args.len() > 3 {
                return None;
            }
            let f = field(*i)?;
            let fld = schema.fields.get(f as usize)?;
            if fld.logical != Some(crate::schema::Logical::Timestamp) || fld.multi || !fld.indexed {
                return None;
            }
            let mut out = vec![width.clone(), Expr::Field(f)];
            out.extend(origin.cloned());
            Some((Expr::Call { func: Func::DateBin, args: out }, Read::Time))
        }
        Expr::Call { func: Func::JsonExtract, args } => {
            let (Some(Expr::Output(i)), rest) = (args.first(), &args[1.min(args.len())..]) else { return None };
            if rest.is_empty() || rest.len() > 2 || !rest.iter().all(|a| matches!(a, Expr::Lit(Value::Str(_)))) {
                return None;
            }
            let f = field(*i)?;
            let fld = schema.fields.get(f as usize)?;
            if fld.ty != FieldType::Json || !fld.indexed {
                return None;
            }
            let args = std::iter::once(Expr::Field(f)).chain(rest.iter().cloned()).collect();
            Some((Expr::Call { func: Func::JsonExtract, args }, Read::Path))
        }
        _ => None,
    }
}

/// Whether the executor groups and aggregates by field `f` as the
/// operators do: an indexed keyword or number.
fn groupable(schema: &Schema, f: &Expr) -> bool {
    let Expr::Field(f) = f else { return true };
    schema.fields.get(*f as usize).is_some_and(|fld| fld.indexed && matches!(fld.ty, FieldType::Keyword | FieldType::Int | FieldType::Float))
}

/// The columns of a `Project` that passes on an Aggregate's `width`
/// outputs as they are.
fn outputs(width: usize) -> Vec<Column> {
    (0..width).map(|i| Column { name: format!("#{i}"), expr: Expr::Output(i as u32) }).collect()
}

/// `Aggregate` of fields and paths over `Project` of fields over a
/// `Search`, as an `Aggregate` over the Search under a `Project` of its
/// outputs: a leaf.
fn aggregate(n: &Node, catalog: &[&Schema]) -> Option<Node> {
    let Node::Aggregate { input, keys, aggs } = n else { return None };
    let Node::Project { input: below, columns: cols } = &**input else { return None };
    let Node::Search(s) = &**below else { return None };
    if !s.order.is_empty() || !s.phases.is_empty() || s.per.is_some() {
        return None;
    }
    let schema = table(catalog, s)?;
    let mut any_path = false;
    let mut any_multi = false;
    let mut read = |e: &Expr| {
        let (x, read) = column(e, cols, schema)?;
        any_path |= read != Read::Field;
        if let Expr::Field(f) = x {
            any_multi |= schema.fields.get(f as usize).is_none_or(|fld| fld.multi);
        }
        groupable(schema, &x).then_some(x)
    };
    let keys2 = keys.iter().map(&mut read).collect::<Option<Vec<_>>>()?;
    let aggs2 = aggs
        .iter()
        .map(|a| {
            let arg = match &a.arg {
                None => None,
                Some(e) => Some(read(e)?),
            };
            Some(Agg { func: a.func, arg })
        })
        .collect::<Option<Vec<_>>>()?;
    // Fields alone are taken when each holds one value, so that the
    // aggregates under `ROUND(SUM(x), 2)`, a HAVING or a window run from the
    // columns; groups over a multi-valued field stay with the operators,
    // and percentiles and distinct counts with DataFusion, which sorts and
    // hashes their values faster.
    let simple = aggs.iter().all(|a| matches!(a.func, AggFn::Count | AggFn::Sum | AggFn::Avg | AggFn::Min | AggFn::Max));
    if !any_path && (any_multi || !simple) {
        return None;
    }
    let agg = Node::Aggregate { input: Box::new(Node::Search(s.clone())), keys: keys2, aggs: aggs2 };
    let leaf = Node::Project { input: Box::new(agg), columns: outputs(keys.len() + aggs.len()) };
    raises(&leaf, schema).then_some(leaf)
}

/// The leaf [`aggregate`] makes with the `Project`, `[Limit]`, `[Sort]`
/// and `[Filter]` over it: one leaf, when the executor takes them.
fn groups(n: &Node, catalog: &[&Schema]) -> Option<Node> {
    /// `n` without the `Project` of an Aggregate's outputs below its
    /// Limit, Sort and Filter.
    fn inner(n: &Node) -> Option<Node> {
        Some(match n {
            Node::Limit { input, offset, limit } => Node::Limit { input: Box::new(inner(input)?), offset: *offset, limit: *limit },
            Node::Sort { input, keys } => Node::Sort { input: Box::new(inner(input)?), keys: keys.clone() },
            Node::Filter { input, cond } => Node::Filter { input: Box::new(inner(input)?), cond: cond.clone() },
            Node::Project { input, columns } => {
                let Node::Aggregate { input: s, keys, aggs } = &**input else { return None };
                if !matches!(**s, Node::Search(_)) || *columns != outputs(keys.len() + aggs.len()) {
                    return None;
                }
                (**input).clone()
            }
            _ => return None,
        })
    }
    let Node::Project { input, columns } = n else { return None };
    let leaf = Node::Project { input: Box::new(inner(input)?), columns: columns.clone() };
    let schema = table(catalog, leaf.search()?)?;
    raises(&leaf, schema).then_some(leaf)
}

/// `Project` over `[Limit]` over `Sort` by a path (then possibly the
/// primary key) over `Project` of fields over a `Search`, as the Search
/// ordered by the path.
fn ordered(n: &Node, catalog: &[&Schema]) -> Option<Node> {
    let Node::Project { input, columns: top } = n else { return None };
    let (limit, sort) = match &**input {
        Node::Limit { input, offset, limit } => (Some((*offset, *limit)), &**input),
        x => (None, x),
    };
    let Node::Sort { input: below, keys } = sort else { return None };
    let Node::Project { input: inner, columns: cols } = &**below else { return None };
    let Node::Search(s) = &**inner else { return None };
    if !s.order.is_empty() || !s.phases.is_empty() || s.per.is_some() || keys.is_empty() || keys.len() > 2 {
        return None;
    }
    let schema = table(catalog, s)?;
    let (first, Read::Path) = column(&keys[0].expr, cols, schema)? else { return None };
    let mut order = vec![SortKey { expr: first.clone(), ..keys[0].clone() }];
    if let Some(k) = keys.get(1) {
        let (e, _) = column(&k.expr, cols, schema)?;
        if Some(&e) != schema.primary_key.map(|p| Expr::Field(p as u32)).as_ref() {
            return None;
        }
        order.push(SortKey { expr: e, nulls_first: false, ..k.clone() });
    }
    let search = Node::Search(Search { order, ..s.clone() });
    let rows = match limit {
        Some((offset, limit)) => Node::Limit { input: Box::new(search), offset, limit },
        None => search,
    };
    // One leaf, when the query's columns are fields and paths, the order's
    // among them.
    let own: Option<Vec<Column>> = top.iter().map(|c| column(&c.expr, cols, schema).map(|(expr, _)| Column { name: c.name.clone(), expr })).collect();
    if let Some(own) = own.filter(|o| o.iter().any(|c| c.expr == first)) {
        let leaf = Node::Project { input: Box::new(rows.clone()), columns: own };
        if raises(&leaf, schema) {
            return Some(leaf);
        }
    }
    let mut cols = cols.clone();
    cols.push(Column { name: ORDER_COLUMN.to_string(), expr: first });
    let leaf = Node::Project { input: Box::new(rows), columns: cols };
    raises(&leaf, schema).then(|| Node::Project { input: Box::new(leaf), columns: top.clone() })
}

/// The name of the column of the path a leaf orders its rows by, after
/// the columns the query reads ([`ordered`]).
pub(crate) const ORDER_COLUMN: &str = "\0order";
