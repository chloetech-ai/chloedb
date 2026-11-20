//! Graph walks over shards: `REACHABLE`, `PATH`, `hops()` and `ppr` of a
//! table whose rows several shards hold.
//!
//! A shard cannot walk its part alone: a row's edges name keys other
//! shards hold. So the coordinator walks, with the traversals one database
//! runs ([`chloedb::graph::traverse`]: breadth-first search, bidirectional
//! shortest paths, PageRank by forward push), over what it has read of the
//! graph so far. A row it meets whose edges it has not read, or an edge
//! whose other end it does not know to be a row, stops that run; it reads
//! everything the run missed in one round (every shard at once, the keys
//! in batches) and runs again from the start. A run that misses nothing is
//! the walk one database would make, so it finds the same rows at the same
//! hops, a shortest path of the same length and the same PageRank, up to
//! the order of ties and rounding. A walk of `h` hops takes about `h + 1`
//! rounds; PageRank takes a round per step its pushes spread.
//!
//! Each round is ordinary reads through the coordinator:
//!
//! * a row and its out-edges: `SELECT key, edges FROM t WHERE key IN (...)`;
//! * the rows linking to keys: `REACHABLE(edges, (...), hops => 1, min_hops
//!   => 0, dir => 'in')`, which each shard answers alone from its reverse
//!   index whatever shard holds the keys' rows
//!   ([`chloedb::query::exec::one_hop_in`]);
//! * an edge table's rows: `SELECT src, dst, w FROM e WHERE src IN (...)`
//!   (or `dst`);
//! * `through`: `SELECT key FROM t WHERE key IN (...) AND (through)`.
//!
//! The result goes to the shards in the plan, each key with its hop
//! distance, place on the path or score ([`Seeds::Given`]), and each shard
//! answers the rest of the query for the rows of those keys it holds.

use std::collections::HashMap;
use std::time::Instant;

use chloedb::graph::traverse::{self, Adjacency, Node};
use chloedb::query::compound::MAX_ROWS;
use chloedb::query::logical::{Direction, GraphKind, GraphQuery, Pred, Seeds};
use chloedb::query::{Params, Select};
use chloedb::schema::{FieldType, Schema};
use chloedb::value::Value;
use futures::future::join_all;

use super::{Coordinator, Merged};
use crate::service::QueryReq;
use crate::ApiError;

/// Keys a read asks for at most.
const BATCH: usize = 2_000;
/// Rounds of reads a walk takes at most.
const MAX_ROUNDS: usize = 256;

/// The refusal of a graph that does not walk through a coordinator, if
/// `select` has one: a walk over a VECTOR field (each hop a vector search
/// over every shard).
pub fn refusal(schema: &Schema, select: &Select) -> Option<ApiError> {
    select.graphs.iter().find(|g| g.edges.is_none() && matches!(schema.fields.get(g.field).map(|f| &f.ty), Some(FieldType::Vector { .. }))).map(|g| {
        ApiError::bad(format!(
            "a walk over a VECTOR field ({}: each row's most similar rows) does not run through a coordinator: walk an EDGES field or an edge table, or run it on a server that holds the whole table",
            schema.fields[g.field].name
        ))
    })
}

/// Whether every graph of `select` is one each shard answers alone.
pub fn shard_local(schema: &Schema, select: &Select) -> bool {
    select.graphs.iter().all(|g| matches!(g.seeds, Seeds::Given(_)) || chloedb::query::exec::one_hop_in(g, schema))
}

/// A key as a map key.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    Str(String),
}

impl Key {
    fn of(v: &Value) -> Option<Key> {
        match v {
            Value::Int(i) => Some(Key::Int(*i)),
            Value::Str(s) => Some(Key::Str(s.clone())),
            _ => None,
        }
    }
}

/// Whether a key is a live row of the walked table.
#[derive(Clone, Copy, PartialEq)]
enum Found {
    Unknown,
    Missing,
    Live,
}

/// What a walk has read of the graph: the keys it met, numbered in the
/// order it met them (the traversals' nodes), and per key whether it is a
/// row, whether it passes `through`, and the edges read along and against
/// the edges' direction.
#[derive(Default)]
struct Known {
    ids: HashMap<Key, Node>,
    keys: Vec<Value>,
    found: Vec<Found>,
    pass: Vec<Option<bool>>,
    out: Vec<Option<Vec<(Node, f32)>>>,
    back: Vec<Option<Vec<(Node, f32)>>>,
}

impl Known {
    fn id(&mut self, v: &Value) -> Option<Node> {
        let k = Key::of(v)?;
        if let Some(&n) = self.ids.get(&k) {
            return Some(n);
        }
        let n = self.keys.len() as Node;
        self.ids.insert(k, n);
        self.keys.push(v.clone());
        self.found.push(Found::Unknown);
        self.pass.push(None);
        self.out.push(None);
        self.back.push(None);
        Some(n)
    }
}

/// What a run missed: the keys to read in the next round.
#[derive(Default)]
struct Want {
    /// Whether the key is a row (and, for an EDGES field, its out-edges).
    found: Vec<Node>,
    /// Out-edges (an edge table's).
    out: Vec<Node>,
    /// The edges reaching the key.
    back: Vec<Node>,
    /// Whether it passes `through`.
    pass: Vec<Node>,
}

impl Want {
    fn is_empty(&self) -> bool {
        self.found.is_empty() && self.out.is_empty() && self.back.is_empty() && self.pass.is_empty()
    }
}

/// The traversals' view of what is known: the neighbours known to be rows,
/// and what it misses noted in `want`.
struct Walker<'k> {
    known: &'k Known,
    dir: Direction,
    /// An edge table's walk: a key's out-edges are a read of its own.
    table: bool,
    through: bool,
    want: Want,
    noted: HashMap<(u8, Node), ()>,
}

impl Walker<'_> {
    fn note(&mut self, what: u8, n: Node) {
        if self.noted.insert((what, n), ()).is_none() {
            match what {
                0 => self.want.found.push(n),
                1 => self.want.out.push(n),
                2 => self.want.back.push(n),
                _ => self.want.pass.push(n),
            }
        }
    }

    /// Appends the edges of list `l` whose other end is a row; notes those
    /// not known to be.
    fn rows_of(&mut self, l: &[(Node, f32)], out: &mut Vec<(Node, f32)>) {
        for &(v, w) in l {
            match self.known.found[v as usize] {
                Found::Live => out.push((v, w)),
                Found::Missing => {}
                Found::Unknown => self.note(0, v),
            }
        }
    }
}

impl Adjacency for Walker<'_> {
    fn neighbors(&mut self, n: Node, reverse: bool, out: &mut Vec<(Node, f32)>) -> chloedb::error::Result<()> {
        let dir = match (self.dir, reverse) {
            (Direction::Out, true) => Direction::In,
            (Direction::In, true) => Direction::Out,
            (d, _) => d,
        };
        let known = self.known;
        if matches!(dir, Direction::Out | Direction::Both) {
            match &known.out[n as usize] {
                Some(l) => self.rows_of(l, out),
                None => self.note(if self.table { 1 } else { 0 }, n),
            }
        }
        if matches!(dir, Direction::In | Direction::Both) {
            match &known.back[n as usize] {
                Some(l) => self.rows_of(l, out),
                None => self.note(2, n),
            }
        }
        Ok(())
    }

    fn passable(&mut self, n: Node) -> bool {
        if !self.through {
            return true;
        }
        match self.known.pass[n as usize] {
            Some(p) => p,
            None => {
                self.note(3, n);
                false
            }
        }
    }
}

/// What walking a query's graphs found, for its answer.
pub struct Walked {
    /// The query, each graph replaced by its result.
    pub select: Select,
    /// A plan line per graph walked.
    pub lines: Vec<String>,
    pub capped: bool,
    pub unsettled: bool,
}

/// One graph's walk.
struct Walk<'a> {
    coord: &'a Coordinator,
    req: &'a QueryReq,
    schema: &'a Schema,
    g: &'a GraphQuery,
    deadline: Instant,
    /// The edge table's schema and its (src, dst, weight) columns.
    table: Option<(Schema, String, String, Option<String>)>,
    known: Known,
    rounds: usize,
    reads: usize,
}

impl Coordinator {
    /// Walks each graph of `select` that the shards cannot answer alone,
    /// over the shards, and gives the query back with their results
    /// ([`Seeds::Given`]).
    pub(super) async fn walk(&self, req: &QueryReq, schema: &Schema, select: &Select, deadline: Instant) -> Result<Walked, ApiError> {
        if let Some(e) = refusal(schema, select) {
            return Err(e);
        }
        let mut out = select.clone();
        let mut lines = Vec::new();
        let (mut capped, mut unsettled) = (false, false);
        for (i, g) in select.graphs.iter().enumerate() {
            if matches!(g.seeds, Seeds::Given(_)) || chloedb::query::exec::one_hop_in(g, schema) {
                continue;
            }
            let t0 = Instant::now();
            let table = match &g.edges {
                None => None,
                Some(name) => {
                    let et = self.catalog().into_iter().find(|s| s.table.eq_ignore_ascii_case(name)).ok_or_else(|| ApiError::bad(format!("query error: no such table: {name}")))?;
                    let (src, dst, w) = et.edge_fields().ok_or_else(|| ApiError::bad(format!("query error: table {} is not an edge table: a walk follows an EDGES field or a table with OPTIONS(edges = 'src, dst, w')", et.table)))?;
                    let name = |f: usize| et.fields[f].name.clone();
                    Some(((*et).clone(), name(src), name(dst), w.map(name)))
                }
            };
            let mut w = Walk { coord: self, req, schema, g, deadline, table, known: Known::default(), rounds: 0, reads: 0 };
            let (given, what, edges, extra, c, u) = w.run().await?;
            capped |= c;
            unsettled |= u;
            let gq = &mut out.graphs[i];
            lines.push(format!(
                "graph {i}: {what} over {}, walked by the coordinator in {} rounds of reads ({} reads), {} rows, {edges} edges followed{extra}, {:.2} ms",
                g.edges.as_deref().unwrap_or(&schema.fields[g.field].name),
                w.rounds,
                w.reads,
                given.len(),
                t0.elapsed().as_secs_f64() * 1e3
            ));
            gq.seeds = Seeds::Given(given);
            gq.through = None;
            if let GraphKind::Path { to, .. } = &mut gq.kind {
                *to = Seeds::Keys(Vec::new());
            }
        }
        Ok(Walked { select: out, lines, capped, unsettled })
    }
}

/// A graph's result as the shards are given it, and what its plan line
/// says: what it was, the edges it followed, the rest of its line, and
/// whether PageRank stopped at `max_pushes` or settled less than half.
type Ran = (Vec<(Value, f64)>, &'static str, u64, String, bool, bool);

impl Walk<'_> {
    async fn run(&mut self) -> Result<Ran, ApiError> {
        let g = self.g;
        let seeds = self.seeds(&g.seeds).await?;
        let to = match &g.kind {
            GraphKind::Path { to, .. } => self.seeds(to).await?,
            _ => Vec::new(),
        };
        let seed_nodes: Vec<Node> = seeds.iter().map(|s| s.0).collect();
        let to_nodes: Vec<Node> = to.iter().map(|s| s.0).collect();
        loop {
            let mut w = Walker { known: &self.known, dir: g.dir, table: self.table.is_some(), through: g.through.is_some(), want: Want::default(), noted: HashMap::new() };
            let ran: Option<Ran> = match &g.kind {
                GraphKind::Reach { min_hops, max_hops } => {
                    let r = traverse::bfs(&mut w, &seed_nodes, *max_hops, g.max_nodes).map_err(ApiError::from)?;
                    // A run that met too many rows with part of the graph
                    // meets as many with all of it.
                    if r.truncated {
                        return Err(ApiError::bad(format!("query error: REACHABLE meets more than max_nodes => {} rows within hops => {max_hops}: raise max_nodes or lower hops", g.max_nodes)));
                    }
                    let mut rows: Vec<(Node, u32)> = r.dist.into_iter().filter(|&(_, d)| d >= *min_hops && d <= *max_hops).collect();
                    rows.sort_unstable();
                    let given = rows.into_iter().map(|(n, d)| (self.known.keys[n as usize].clone(), d as f64)).collect();
                    Some((given, "reachable", r.edges, String::new(), false, false))
                }
                GraphKind::Path { max_hops, .. } => {
                    let p = traverse::shortest_path(&mut w, &seed_nodes, &to_nodes, *max_hops, g.max_nodes).map_err(ApiError::from)?;
                    if p.truncated && w.want.is_empty() {
                        return Err(ApiError::bad(format!("query error: PATH meets more than max_nodes => {} rows before finding a path: raise max_nodes", g.max_nodes)));
                    }
                    let len = p.nodes.as_ref().map_or(0, |p| p.len());
                    let given = p.nodes.into_iter().flatten().enumerate().map(|(i, n)| (self.known.keys[n as usize].clone(), i as f64)).collect();
                    Some((given, "path", p.edges, format!(", length {}", len.saturating_sub(1)), false, false))
                }
                GraphKind::Ppr { alpha, eps, max_pushes, weighted } => {
                    let weights: Vec<(Node, f64)> = seeds.iter().map(|&(n, sc)| (n, if *weighted { sc.max(1e-9) } else { 1.0 })).collect();
                    let r = traverse::ppr(&mut w, &weights, false, *alpha, *eps, *max_pushes).map_err(ApiError::from)?;
                    let unsettled = r.settled < traverse::SETTLED;
                    let mut rows: Vec<(Node, f64)> = r.scores.into_iter().filter(|&(_, s)| s > 0.0).collect();
                    rows.sort_unstable_by_key(|x| x.0);
                    let given = rows.into_iter().map(|(n, s)| (self.known.keys[n as usize].clone(), s)).collect();
                    let cut = if r.capped { format!(" (stopped at max_pushes => {max_pushes}: scores of a shorter walk; raise max_pushes or eps)") } else { String::new() };
                    let lowered = if *eps <= 0.0 && r.eps < traverse::EPS { format!(", eps lowered to {:.1e}", r.eps) } else { String::new() };
                    Some((given, "ppr", r.edges, format!(", {} pushes{lowered}, {:.0}% settled{cut}", r.pushes, 100.0 * r.settled), r.capped, unsettled))
                }
            };
            let want = w.want;
            if want.is_empty() {
                return Ok(ran.expect("a run"));
            }
            self.read(want).await?;
        }
    }

    /// The seeds' rows, with their scores (1 for keys), in their order: the
    /// keys that are rows, or the rows of a subquery.
    async fn seeds(&mut self, s: &Seeds) -> Result<Vec<(Node, f64)>, ApiError> {
        let listed: Vec<(Value, f64)> = match s {
            Seeds::Keys(keys) => keys.iter().map(|k| (k.clone(), 1.0)).collect(),
            Seeds::Weighted(ws) => ws.clone(),
            Seeds::Query(q) => {
                let m: Merged = Box::pin(self.coord.query(&self.inner(String::new()), self.schema, q, self.deadline)).await?;
                let pk = self.schema.primary_key.ok_or_else(|| ApiError::bad("query error: graph queries need a primary key"))?;
                let keys = row_keys(&m, &self.schema.fields[pk].name);
                let scores = &m.rs.scores;
                keys.into_iter().enumerate().map(|(i, k)| (k, scores.get(i).copied().unwrap_or(1.0))).collect()
            }
            Seeds::Plan(_) | Seeds::Given(_) => return Err(ApiError::internal("a walk's seeds are keys or a query of the table")),
        };
        let ids: Vec<Node> = listed.iter().filter_map(|(k, _)| self.known.id(k)).collect();
        let unknown: Vec<Node> = ids.iter().copied().filter(|&n| self.known.found[n as usize] == Found::Unknown).collect();
        if !unknown.is_empty() {
            self.read(Want { found: unknown, ..Default::default() }).await?;
        }
        // As one database resolves them: the keys that are rows, a key
        // listed twice in a row once, weights of one key summed.
        let mut out: Vec<(Node, f64)> = Vec::new();
        let mut at: HashMap<Node, usize> = HashMap::new();
        for (k, w) in &listed {
            let Some(n) = self.known.id(k) else { continue };
            if self.known.found[n as usize] != Found::Live {
                continue;
            }
            match s {
                Seeds::Weighted(_) => match at.get(&n) {
                    Some(&i) => out[i].1 += w,
                    None => {
                        at.insert(n, out.len());
                        out.push((n, *w));
                    }
                },
                Seeds::Keys(_) if out.last().is_some_and(|x| x.0 == n) => {}
                _ => out.push((n, *w)),
            }
        }
        Ok(out)
    }

    /// A request of the walk's own reads.
    fn inner(&self, sql: String) -> QueryReq {
        QueryReq { sql, after: self.req.after.clone(), timeout_ms: self.req.timeout_ms, partial: Some(false), quiet: true, ..Default::default() }
    }

    /// Reads, in one round, what a run missed.
    async fn read(&mut self, want: Want) -> Result<(), ApiError> {
        self.rounds += 1;
        if self.rounds > MAX_ROUNDS {
            return Err(ApiError::bad(format!("query error: the walk took more than {MAX_ROUNDS} rounds of reads over the shards: lower hops, or give ppr a larger eps")));
        }
        if Instant::now() >= self.deadline {
            return Err(ApiError { code: None, status: 504, message: "query error: the query's deadline passed while it walked the graph".into() });
        }
        if self.known.keys.len() > MAX_ROWS {
            return Err(ApiError::bad(format!("query error: the walk met more than {MAX_ROWS} keys over the shards: lower hops or max_nodes")));
        }
        let pk = self.schema.primary_key.ok_or_else(|| ApiError::bad("query error: graph queries need a primary key"))?;
        let (t, key) = (chloedb::query::sql::ident(&self.schema.table), chloedb::query::sql::ident(&self.schema.fields[pk].name));
        let field = chloedb::query::sql::ident(&self.schema.fields[self.g.field].name);
        let lits = |ns: &[Node], known: &Known| ns.iter().map(|&n| chloedb::query::hint::sql_value(&known.keys[n as usize])).collect::<Vec<_>>().join(", ");
        // Each read: its SQL, the table it reads, the keys it asks about,
        // and what it reads.
        let mut reads: Vec<(String, Schema, Vec<Node>, u8, Option<Pred>)> = Vec::new();
        let edges_with_rows = self.table.is_none() && self.g.dir != Direction::In;
        for chunk in want.found.chunks(BATCH) {
            let sql = match edges_with_rows {
                true => format!("SELECT {key}, {field} FROM {t} WHERE {key} IN ({}) LIMIT {}", lits(chunk, &self.known), chunk.len()),
                false => format!("SELECT {key} FROM {t} WHERE {key} IN ({}) LIMIT {}", lits(chunk, &self.known), chunk.len()),
            };
            reads.push((sql, self.schema.clone(), chunk.to_vec(), 0, None));
        }
        for chunk in want.pass.chunks(BATCH) {
            let sql = format!("SELECT {key} FROM {t} WHERE {key} IN ({}) LIMIT {}", lits(chunk, &self.known), chunk.len());
            reads.push((sql, self.schema.clone(), chunk.to_vec(), 3, self.g.through.clone()));
        }
        match &self.table {
            None => {
                for chunk in want.back.chunks(BATCH) {
                    let sql = format!("SELECT {key}, {field} FROM {t} WHERE REACHABLE({field}, ({}), hops => 1, min_hops => 0, dir => 'in') LIMIT {MAX_ROWS}", lits(chunk, &self.known));
                    reads.push((sql, self.schema.clone(), chunk.to_vec(), 2, None));
                }
            }
            Some((et, src, dst, w)) => {
                let (e, s, d) = (chloedb::query::sql::ident(&et.table), chloedb::query::sql::ident(src), chloedb::query::sql::ident(dst));
                let w = w.as_ref().map_or_else(|| "1".to_string(), |w| chloedb::query::sql::ident(w));
                for (ns, by, what) in [(&want.out, &s, 1u8), (&want.back, &d, 2u8)] {
                    for chunk in ns.chunks(BATCH) {
                        let sql = format!("SELECT {s}, {d}, {w} FROM {e} WHERE {by} IN ({}) LIMIT {MAX_ROWS}", lits(chunk, &self.known));
                        reads.push((sql, et.clone(), chunk.to_vec(), what, None));
                    }
                }
            }
        }
        self.reads += reads.len();
        let deadline = self.deadline;
        let me: &Walk = self;
        let calls = reads.iter().map(|(sql, schema, _, _, through)| async move {
            let mut select = chloedb::query::bind(sql, schema, &Params::new()).map_err(ApiError::from)?;
            if let Some(p) = through {
                select.filter = Pred::And(vec![select.filter.clone(), p.clone()]);
            }
            let inner = me.inner(sql.clone());
            me.coord.routed(|| me.coord.query_once(&inner, schema, &select, deadline)).await
        });
        let answers = join_all(calls).await;
        let int_keys = chloedb::graph::int_keys(self.schema);
        let labels = &self.g.labels;
        let label_ok = |e: &chloedb::graph::Edge| labels.is_empty() || e.label.as_ref().is_some_and(|l| labels.contains(l));
        for ((_, _, asked, what, _), a) in reads.iter().zip(answers) {
            let m = a?;
            if m.rs.rows.len() >= MAX_ROWS {
                return Err(ApiError::bad(format!("query error: a round of the walk reads more than {MAX_ROWS} edges: lower hops or max_nodes")));
            }
            match what {
                0 => {
                    for &n in asked {
                        self.known.found[n as usize] = Found::Missing;
                    }
                    for r in &m.rs.rows {
                        let Some(n) = r.first().and_then(|k| self.known.id(k)) else { continue };
                        self.known.found[n as usize] = Found::Live;
                        if edges_with_rows {
                            let edges = r.get(1).map(|v| chloedb::graph::parse_edges(v, int_keys)).transpose().map_err(ApiError::from)?.unwrap_or_default();
                            let list: Vec<(Node, f32)> = edges.iter().filter(|e| label_ok(e)).filter_map(|e| Some((self.known.id(&e.to)?, e.weight))).collect();
                            self.known.out[n as usize] = Some(list);
                        }
                    }
                }
                3 => {
                    for &n in asked {
                        self.known.pass[n as usize] = Some(false);
                    }
                    for r in &m.rs.rows {
                        if let Some(n) = r.first().and_then(|k| self.known.id(k)) {
                            self.known.pass[n as usize] = Some(true);
                        }
                    }
                }
                _ if self.table.is_none() => {
                    // The rows linking to the keys (and the keys' own rows):
                    // each edge of theirs that names a key asked about.
                    let asked_set: HashMap<Node, ()> = asked.iter().map(|&n| (n, ())).collect();
                    let mut lists: HashMap<Node, Vec<(Node, f32)>> = asked.iter().map(|&n| (n, Vec::new())).collect();
                    for r in &m.rs.rows {
                        let Some(src) = r.first().and_then(|k| self.known.id(k)) else { continue };
                        self.known.found[src as usize] = Found::Live;
                        let edges = r.get(1).map(|v| chloedb::graph::parse_edges(v, int_keys)).transpose().map_err(ApiError::from)?.unwrap_or_default();
                        let mut own = Vec::new();
                        for e in edges.iter().filter(|e| label_ok(e)) {
                            let Some(to) = self.known.id(&e.to) else { continue };
                            own.push((to, e.weight));
                            if asked_set.contains_key(&to) {
                                lists.entry(to).or_default().push((src, e.weight));
                            }
                        }
                        if self.g.dir == Direction::Both && self.known.out[src as usize].is_none() {
                            self.known.out[src as usize] = Some(own);
                        }
                    }
                    for (n, l) in lists {
                        self.known.back[n as usize] = Some(l);
                    }
                }
                _ => {
                    // An edge table's rows: out-edges by source, or the
                    // edges reaching a key by target; weights of 0 or less
                    // (or NaN) are not followed.
                    let mut lists: HashMap<Node, Vec<(Node, f32)>> = asked.iter().map(|&n| (n, Vec::new())).collect();
                    for r in &m.rs.rows {
                        let (Some(s), Some(d)) = (r.first().and_then(|k| self.known.id(k)), r.get(1).and_then(|k| self.known.id(k))) else { continue };
                        let w = r.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                        if !(w > 0.0) {
                            continue;
                        }
                        match what {
                            1 => lists.entry(s).or_default().push((d, w)),
                            _ => lists.entry(d).or_default().push((s, w)),
                        }
                    }
                    for (n, l) in lists {
                        match what {
                            1 => self.known.out[n as usize] = Some(l),
                            _ => self.known.back[n as usize] = Some(l),
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// The keys of a merged answer's rows: its column of the key, or its keys.
fn row_keys(m: &Merged, pk: &str) -> Vec<Value> {
    match m.rs.columns.iter().position(|c| c.eq_ignore_ascii_case(pk)) {
        Some(i) if m.rs.keys.is_empty() => m.rs.rows.iter().filter_map(|r| r.get(i).cloned()).collect(),
        _ => m.rs.keys.clone(),
    }
}
