//! Rewrites of a statement's syntax tree, before it is planned, into forms
//! the planner takes, with the same answer:
//!
//! - `a RIGHT JOIN b ON ..` as `b LEFT JOIN a ON ..` (the first join of a
//!   FROM; `SELECT *` keeps its columns' order, `a.*, b.*`);
//! - a correlated `EXISTS (SELECT .. FROM t WHERE t.k = outer.k AND ..)`
//!   in WHERE as `outer.k IN (SELECT t.k FROM t WHERE .. AND t.k IS NOT
//!   NULL)`, and `NOT EXISTS` as `(outer.k IS NULL OR outer.k NOT IN
//!   (..))`: a semi join and an anti join, each subquery run once.
//!
//! `FILTER (WHERE ..)` and the statistical aggregates are written as
//! other aggregates in [`super::compound::filters_as_case`].

use sqlparser::ast::{self as sql, BinaryOperator, Expr, SelectItem, TableFactor};

use super::compound::{walk, walk_mut, AGGREGATES};

/// Applies the rewrites to `q`, its `WITH` queries, set operations' arms,
/// derived tables and subqueries.
pub(crate) fn query(q: &mut sql::Query) {
    each_select(q, &mut |s| {
        right_join(s);
        exists_as_in(s);
    });
}

fn each_select(q: &mut sql::Query, f: &mut dyn FnMut(&mut sql::Select)) {
    if let Some(w) = &mut q.with {
        for cte in &mut w.cte_tables {
            each_select(&mut cte.query, f);
        }
    }
    body(&mut q.body, f);
}

fn body(b: &mut sql::SetExpr, f: &mut dyn FnMut(&mut sql::Select)) {
    match b {
        sql::SetExpr::SetOperation { left, right, .. } => {
            body(left, f);
            body(right, f);
        }
        sql::SetExpr::Query(q) => each_select(q, f),
        sql::SetExpr::Select(s) => {
            f(s);
            for t in &mut s.from {
                for r in std::iter::once(&mut t.relation).chain(t.joins.iter_mut().map(|j| &mut j.relation)) {
                    if let TableFactor::Derived { subquery, .. } = r {
                        each_select(subquery, f);
                    }
                }
            }
            let mut sub = |e: &mut Expr| match e {
                Expr::Subquery(q) | Expr::Exists { subquery: q, .. } | Expr::InSubquery { subquery: q, .. } => each_select(q, f),
                _ => {}
            };
            let mut exprs: Vec<&mut Expr> = Vec::new();
            for item in &mut s.projection {
                if let SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } = item {
                    exprs.push(e);
                }
            }
            exprs.extend(s.selection.iter_mut());
            exprs.extend(s.having.iter_mut());
            for e in exprs {
                subqueries(e, &mut sub);
            }
        }
        _ => {}
    }
}

/// Calls `f` on each subquery of `e` (not on those within it).
fn subqueries(e: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    let mut found: Vec<*const Expr> = Vec::new();
    walk(e, &mut |x| {
        if matches!(x, Expr::Subquery(_) | Expr::Exists { .. } | Expr::InSubquery { .. }) {
            found.push(x as *const Expr);
        }
    });
    if found.is_empty() {
        return;
    }
    walk_mut(e, &mut |x| {
        if found.contains(&(x as *const Expr)) {
            f(x);
        }
    });
}

/// The names a FROM gives its tables: each table's name and alias, and
/// each derived table's alias, lowercase.
fn names(from: &[sql::TableWithJoins]) -> Vec<String> {
    let mut out = Vec::new();
    for t in from {
        for r in std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)) {
            match r {
                TableFactor::Table { name, alias, .. } => {
                    out.push(super::sql::table_name(name).to_ascii_lowercase());
                    out.extend(alias.iter().map(|a| a.name.value.to_ascii_lowercase()));
                }
                TableFactor::Derived { alias: Some(a), .. } => out.push(a.name.value.to_ascii_lowercase()),
                _ => {}
            }
        }
    }
    out
}

/// The name `SELECT a.*` gives a table of a FROM: its alias, or its name.
fn qualifier(r: &TableFactor) -> Option<sql::ObjectName> {
    match r {
        TableFactor::Table { alias: Some(a), .. } | TableFactor::Derived { alias: Some(a), .. } => Some(sql::ObjectName::from(vec![a.name.clone()])),
        TableFactor::Table { name, alias: None, .. } => Some(name.clone()),
        _ => None,
    }
}

/// `a RIGHT JOIN b ON c` (the first join of the FROM) as `b LEFT JOIN a
/// ON c`: the rows of b, each with its matches in a or with NULLs.
fn right_join(s: &mut sql::Select) {
    let [t] = s.from.as_mut_slice() else { return };
    let Some(first) = t.joins.first() else { return };
    let c = match &first.join_operator {
        sql::JoinOperator::Right(c) | sql::JoinOperator::RightOuter(c) => c.clone(),
        _ => return,
    };
    // `*` keeps the order of the tables as written.
    if s.projection.iter().any(|i| matches!(i, SelectItem::Wildcard(_))) {
        let Some(qs) = std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)).map(qualifier).collect::<Option<Vec<_>>>() else { return };
        let mut projection = Vec::new();
        for item in std::mem::take(&mut s.projection) {
            match item {
                SelectItem::Wildcard(opts) => {
                    projection.extend(qs.iter().map(|q| SelectItem::QualifiedWildcard(sql::SelectItemQualifiedWildcardKind::ObjectName(q.clone()), opts.clone())))
                }
                item => projection.push(item),
            }
        }
        s.projection = projection;
    }
    let first = &mut t.joins[0];
    std::mem::swap(&mut t.relation, &mut first.relation);
    first.join_operator = match first.join_operator {
        sql::JoinOperator::Right(_) => sql::JoinOperator::Left(c),
        _ => sql::JoinOperator::LeftOuter(c),
    };
}

/// Each `[NOT] EXISTS (..)` among the conditions of `s`'s WHERE (joined by
/// AND, OR and NOT) that names the outer row by one equality, as a
/// semi join or an anti join ([`exists_in`]).
fn exists_as_in(s: &mut sql::Select) {
    let outer = names(&s.from);
    fn conds(e: &mut Expr, outer: &[String]) {
        match e {
            Expr::BinaryOp { left, op: BinaryOperator::And | BinaryOperator::Or, right } => {
                conds(left, outer);
                conds(right, outer);
            }
            Expr::UnaryOp { op: sql::UnaryOperator::Not, expr: x } | Expr::Nested(x) => conds(x, outer),
            Expr::Exists { subquery, negated } => {
                if let Some(x) = exists_in(subquery, *negated, outer) {
                    *e = x;
                }
            }
            _ => {}
        }
    }
    if let Some(w) = &mut s.selection {
        conds(w, &outer);
    }
}

/// `EXISTS (SELECT .. FROM t WHERE t.k = o.k AND rest)`, `o` a table of the
/// outer query (`outer`) and not of the subquery's, as `o.k IN (SELECT
/// t.k FROM t WHERE rest AND t.k IS NOT NULL)`; `NOT EXISTS` as `(o.k IS
/// NULL OR o.k NOT IN (..))`. Both are TRUE or FALSE as EXISTS is, never
/// NULL. `None` for a subquery this does not cover (one that groups or
/// aggregates, has an OFFSET, or names the outer row elsewhere), which is
/// planned as it is.
fn exists_in(sub: &sql::Query, negated: bool, outer: &[String]) -> Option<Expr> {
    let sql::SetExpr::Select(si) = sub.body.as_ref() else { return None };
    if sub.with.is_some() || sub.fetch.is_some() || !sub.locks.is_empty() || si.having.is_some() || si.distinct.is_some() || si.from.is_empty() {
        return None;
    }
    if !matches!(&si.group_by, sql::GroupByExpr::Expressions(g, _) if g.is_empty()) {
        return None;
    }
    let aggregates = si.projection.iter().any(|i| match i {
        SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => {
            let mut found = false;
            walk(e, &mut |x| found |= matches!(x, Expr::Function(f) if f.over.is_none() && AGGREGATES.contains(&f.name.to_string().to_ascii_lowercase().as_str())));
            found
        }
        _ => false,
    });
    if aggregates {
        return None;
    }
    // A LIMIT of one or more rows does not change whether a row exists.
    match &sub.limit_clause {
        None => {}
        Some(sql::LimitClause::LimitOffset { limit, offset: None, limit_by }) if limit_by.is_empty() => match limit {
            None => {}
            Some(Expr::Value(v)) if matches!(&v.value, sql::Value::Number(n, _) if n.parse::<u64>().is_ok_and(|n| n > 0)) => {}
            Some(_) => return None,
        },
        Some(_) => return None,
    }
    let inner = names(&si.from);
    let is_outer = |e: &Expr| matches!(e, Expr::CompoundIdentifier(ids) if ids.len() == 2 && { let q = ids[0].value.to_ascii_lowercase(); outer.contains(&q) && !inner.contains(&q) });
    let names_outer = |e: &Expr| {
        let mut found = false;
        walk(e, &mut |x| found |= is_outer(x));
        found
    };
    let mut correlation: Option<(Expr, Expr)> = None;
    let mut rest: Vec<Expr> = Vec::new();
    for c in si.selection.iter().flat_map(split_and) {
        let pair = match &c {
            Expr::BinaryOp { left, op: BinaryOperator::Eq, right } if is_outer(left) && !names_outer(right) => Some(((**left).clone(), (**right).clone())),
            Expr::BinaryOp { left, op: BinaryOperator::Eq, right } if is_outer(right) && !names_outer(left) => Some(((**right).clone(), (**left).clone())),
            _ => None,
        };
        match pair {
            Some(p) if correlation.is_none() => correlation = Some(p),
            // Two equalities with the outer row: a pair of keys.
            Some(_) => return None,
            None if names_outer(&c) => return None,
            None => rest.push(c),
        }
    }
    let (outer_key, inner_key) = correlation?;
    rest.push(Expr::IsNotNull(Box::new(inner_key.clone())));
    let selection = rest.into_iter().reduce(|a, b| Expr::BinaryOp { left: Box::new(a), op: BinaryOperator::And, right: Box::new(b) });
    let mut s = si.clone();
    s.projection = vec![SelectItem::UnnamedExpr(inner_key)];
    s.selection = selection;
    let q = sql::Query { body: Box::new(sql::SetExpr::Select(s)), order_by: None, limit_clause: None, ..sub.clone() };
    let in_list = Expr::InSubquery { expr: Box::new(outer_key.clone()), subquery: Box::new(q), negated };
    Some(match negated {
        false => in_list,
        true => Expr::Nested(Box::new(Expr::BinaryOp { left: Box::new(Expr::IsNull(Box::new(outer_key))), op: BinaryOperator::Or, right: Box::new(in_list) })),
    })
}

fn split_and(e: &Expr) -> Vec<Expr> {
    match e {
        Expr::BinaryOp { left, op: BinaryOperator::And, right } => [split_and(left), split_and(right)].concat(),
        Expr::Nested(x) if matches!(x.as_ref(), Expr::BinaryOp { op: BinaryOperator::And, .. }) => split_and(x),
        e => vec![e.clone()],
    }
}
