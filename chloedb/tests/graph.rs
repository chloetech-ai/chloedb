//! Graph queries end to end: a random graph spread over disk segments and
//! the memtable, queried with REACHABLE, PATH, ppr() and hops(), and checked
//! against traversals computed here; then again after deletes, updates and
//! a merge.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use rand::{Rng, SeedableRng};
use chloedb::db::Db;
use chloedb::graph::{edges_value, Edge};
use chloedb::query::Params;
use chloedb::storage::{DbOptions, MergePolicy};
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema, VectorIndexKind};

const DIM: usize = 4;
const LABELS: [&str; 3] = ["cites", "extends", "refutes"];

fn schema() -> Schema {
    Schema::new(
        "papers",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("title", FieldType::Text { positions: false }),
            Field::new("year", FieldType::Int),
            Field::new("kind", FieldType::Keyword),
            Field::new("emb", FieldType::Vector { dim: DIM, index: VectorIndexKind::Flat }),
            Field::new("cites", FieldType::Edges),
        ],
        Some("id"),
    )
    .unwrap()
}

/// The test's own copy of the graph: id -> (year, kind, vector, edges).
#[derive(Clone, Default)]
struct Model {
    rows: BTreeMap<String, (i64, String, Vec<f32>, Vec<(String, String)>)>,
}

fn key(i: usize) -> String {
    format!("p{i:03}")
}

impl Model {
    fn doc(&self, id: &str) -> Vec<Value> {
        let (year, kind, v, edges) = &self.rows[id];
        let es: Vec<Edge> = edges.iter().map(|(l, t)| Edge::labeled(l, t.as_str())).collect();
        vec![
            Value::Str(id.to_string()),
            Value::Str(format!("paper {id} about {}", if year % 3 == 0 { "graph search" } else { "vectors" })),
            Value::Int(*year),
            Value::Str(kind.clone()),
            Value::Vector(v.clone().into()),
            edges_value(&es),
        ]
    }

    /// Neighbours along (`out`), against, or both ways, with label filter.
    fn nbrs(&self, u: &str, dir: &str, label: Option<&str>) -> Vec<String> {
        let ok = |l: &str| label.is_none_or(|x| x == l);
        let mut out = Vec::new();
        if dir != "in" {
            out.extend(self.rows[u].3.iter().filter(|(l, t)| ok(l) && self.rows.contains_key(t)).map(|(_, t)| t.clone()));
        }
        if dir != "out" {
            for (src, r) in &self.rows {
                out.extend(r.3.iter().filter(|(l, t)| ok(l) && t == u).map(|_| src.clone()));
            }
        }
        out
    }

    fn bfs(&self, seed: &str, hops: u32, dir: &str, label: Option<&str>, through: Option<&dyn Fn(&str) -> bool>) -> BTreeMap<String, u32> {
        let mut dist = BTreeMap::new();
        if !self.rows.contains_key(seed) {
            return dist;
        }
        dist.insert(seed.to_string(), 0);
        let mut q = VecDeque::from([seed.to_string()]);
        while let Some(u) = q.pop_front() {
            let d = dist[&u];
            if d == hops || (d > 0 && through.is_some_and(|t| !t(&u))) {
                continue;
            }
            for v in self.nbrs(&u, dir, label) {
                if !dist.contains_key(&v) {
                    dist.insert(v.clone(), d + 1);
                    q.push_back(v);
                }
            }
        }
        dist
    }
}

fn random_model(n: usize, rng: &mut rand::rngs::StdRng) -> Model {
    let mut m = Model::default();
    for i in 0..n {
        let edges = (0..rng.gen_range(0..5))
            .map(|_| (LABELS[rng.gen_range(0..3)].to_string(), key(rng.gen_range(0..n + 20))))
            .collect();
        let v: Vec<f32> = (0..DIM).map(|_| rng.gen_range(-1.0..1.0)).collect();
        m.rows.insert(key(i), (2000 + rng.gen_range(0..25), ["paper", "survey"][rng.gen_range(0..2)].to_string(), v, edges));
    }
    m
}

fn ids(db: &Db, sql: &str, params: &Params) -> Vec<String> {
    db.query(sql, params).unwrap_or_else(|e| panic!("{sql}: {e}")).rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect()
}

fn set(db: &Db, sql: &str) -> BTreeSet<String> {
    ids(db, sql, &Params::new()).into_iter().collect()
}

/// Checks reachability in every direction, with labels and a through
/// predicate, hop distances, and counts, from several seeds.
fn check_reach(db: &Db, m: &Model, seeds: &[String]) {
    // The checks below mean something only if the graph is not trivial.
    let reached: usize = seeds.iter().map(|s| m.bfs(s, 3, "out", None, None).len()).sum();
    let through = |u: &str| m.rows[u].1 == "survey";
    let filtered: usize = seeds.iter().map(|s| m.bfs(s, 3, "both", None, Some(&through)).len()).sum();
    assert!(reached > 10 * seeds.len() && filtered > 2 * seeds.len() && filtered < m.rows.len() * seeds.len(), "{reached} {filtered}");
    for seed in seeds {
        for (dir, hops) in [("out", 1), ("out", 3), ("in", 2), ("both", 2)] {
            let want = m.bfs(seed, hops, dir, None, None);
            let want_set: BTreeSet<String> = want.iter().filter(|(_, &d)| d >= 1).map(|(k, _)| k.clone()).collect();
            let sql = format!("SELECT id FROM papers WHERE REACHABLE(cites, '{seed}', hops => {hops}, dir => '{dir}') LIMIT 100000");
            assert_eq!(set(db, &sql), want_set, "{sql}");
            let rs = db.query(&format!("SELECT id, hops() FROM papers WHERE REACHABLE(cites, '{seed}', hops => {hops}, dir => '{dir}', min_hops => 0) LIMIT 100000"), &Params::new()).unwrap();
            let got: BTreeMap<String, u32> = rs.rows.iter().map(|r| (r[0].as_str().unwrap().to_string(), r[1].as_i64().unwrap() as u32)).collect();
            assert_eq!(got, want, "hops from {seed} {dir} {hops}");
            let n = db.query(&format!("SELECT COUNT(*) FROM papers WHERE REACHABLE(cites, '{seed}', hops => {hops}, dir => '{dir}')"), &Params::new()).unwrap();
            assert_eq!(n.rows[0][0], Value::Int(want_set.len() as i64));
        }
        let want: BTreeSet<String> = m.bfs(seed, 2, "out", Some("cites"), None).into_iter().filter(|(_, d)| *d >= 1).map(|(k, _)| k).collect();
        assert_eq!(set(db, &format!("SELECT id FROM papers WHERE REACHABLE(cites, '{seed}', hops => 2, label => 'cites') LIMIT 100000")), want);
        // Through surveys only; end points of any kind, then filtered to papers.
        let through = |u: &str| m.rows[u].1 == "survey";
        let want: BTreeSet<String> = m
            .bfs(seed, 3, "both", None, Some(&through))
            .into_iter()
            .filter(|(k, d)| *d >= 1 && m.rows[k].1 == "paper")
            .map(|(k, _)| k)
            .collect();
        let sql = format!("SELECT id FROM papers WHERE REACHABLE(cites, '{seed}', hops => 3, dir => 'both', through => kind = 'survey') AND kind = 'paper' LIMIT 100000");
        assert_eq!(set(db, &sql), want, "{sql}");
    }
}

fn open(dir: &std::path::Path) -> Db {
    let opts = DbOptions { segment_max_docs: 70, memtable_chunk_docs: 64, sync: false, ..DbOptions::default() };
    Db::create(&dir.join("g.strata"), schema(), opts).unwrap()
}

#[test]
fn reachability_paths_and_pagerank_over_segments_and_memtable() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    let mut m = random_model(300, &mut rng);
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    // 220 rows in disk segments of at most 70, the rest in the memtable.
    let keys: Vec<String> = m.rows.keys().cloned().collect();
    db.load(keys[..220].iter().map(|k| m.doc(k))).unwrap();
    let mut txn = db.storage().begin().unwrap();
    for k in &keys[220..] {
        txn.insert(m.doc(k)).unwrap();
    }
    txn.commit().unwrap();
    let segs = db.snapshot().segments().len();
    assert!(segs >= 5, "{segs} segments");

    let seeds: Vec<String> = (0..12).map(|_| keys[rng.gen_range(0..keys.len())].clone()).collect();
    check_reach(&db, &m, &seeds);

    // Updates and deletes: rewire some rows, delete others.
    let mut txn = db.storage().begin().unwrap();
    for _ in 0..30 {
        let k = keys[rng.gen_range(0..keys.len())].clone();
        if !m.rows.contains_key(&k) {
            continue;
        }
        if rng.gen_bool(0.4) {
            m.rows.remove(&k);
            txn.delete(&Value::Str(k)).unwrap();
        } else {
            let edges = (0..rng.gen_range(1..6)).map(|_| (LABELS[rng.gen_range(0..3)].to_string(), keys[rng.gen_range(0..keys.len())].clone())).collect();
            m.rows.get_mut(&k).unwrap().3 = edges;
            txn.upsert(m.doc(&k)).unwrap();
        }
    }
    txn.commit().unwrap();
    check_reach(&db, &m, &seeds);

    db.storage().flush().unwrap();
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 1_000, ..MergePolicy::default() }).unwrap();
    assert!(db.snapshot().segments().len() < segs);
    check_reach(&db, &m, &seeds);

    // Paths: as long as the reference distance, and every step an edge.
    for (a, b) in seeds.iter().zip(seeds.iter().rev()) {
        let want = m.bfs(a, 8, "out", None, None).get(b).copied();
        let rs = db.query(&format!("SELECT id, hops() FROM papers WHERE PATH(cites, '{a}', '{b}', max_hops => 8) ORDER BY hops() LIMIT 100"), &Params::new()).unwrap();
        let path: Vec<String> = rs.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
        match want {
            None => assert!(path.is_empty() || !m.rows.contains_key(a), "{a} -> {b}: {path:?}"),
            Some(d) => {
                assert_eq!(path.len() as u32, d + 1, "{a} -> {b}: {path:?}");
                assert_eq!((path.first().unwrap(), path.last().unwrap()), (a, b));
                for w in path.windows(2) {
                    assert!(m.nbrs(&w[0], "out", None).contains(&w[1]), "{} -> {} is not an edge", w[0], w[1]);
                }
                let hops: Vec<i64> = rs.rows.iter().map(|r| r[1].as_i64().unwrap()).collect();
                assert_eq!(hops, (0..path.len() as i64).collect::<Vec<_>>());
            }
        }
    }

    // PageRank against power iteration on the undirected multigraph.
    let seed = seeds.iter().find(|s| m.rows.contains_key(*s)).unwrap();
    let ids_all: Vec<&String> = m.rows.keys().collect();
    let pos: HashMap<&String, usize> = ids_all.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    let nb: Vec<Vec<usize>> = ids_all.iter().map(|k| m.nbrs(k, "both", None).iter().map(|t| pos[t]).collect()).collect();
    let alpha = 0.15;
    let s0 = pos[seed];
    let mut x = vec![0.0f64; nb.len()];
    for _ in 0..300 {
        let mut y = vec![0.0; nb.len()];
        y[s0] += alpha;
        for u in 0..nb.len() {
            if nb[u].is_empty() {
                y[u] += (1.0 - alpha) * x[u];
                continue;
            }
            for &v in &nb[u] {
                y[v] += (1.0 - alpha) * x[u] / nb[u].len() as f64;
            }
        }
        x = y;
    }
    let rs = db.query(&format!("SELECT id, score() FROM papers ORDER BY ppr(cites, '{seed}', eps => 1e-9) DESC LIMIT 15"), &Params::new()).unwrap();
    for r in &rs.rows {
        let want = x[pos[&r[0].as_str().unwrap().to_string()]];
        let got = r[1].as_f64().unwrap();
        assert!((got - want).abs() < 1e-5, "{}: {got} vs {want}", r[0].as_str().unwrap());
    }
    let mut want_top: Vec<(f64, usize)> = x.iter().enumerate().map(|(i, v)| (*v, i)).collect();
    want_top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    assert_eq!(rs.rows[0][0].as_str().unwrap(), ids_all[want_top[0].1]);
}

#[test]
fn seeds_from_subqueries_vectors_and_semantic_edges() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(5);
    let m = random_model(200, &mut rng);
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    db.load(m.rows.keys().map(|k| m.doc(k))).unwrap();
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();

    // Seeds from a text query; neighbourhood ranked by similarity.
    let q: Vec<f32> = vec![0.5, -0.2, 0.1, 0.7];
    let params = Params::new().set("q", Value::Vector(q.clone().into()));
    let seeds = ids(&db, "SELECT id FROM papers WHERE MATCH(title, 'graph') ORDER BY year DESC, id LIMIT 3", &params);
    let mut want: BTreeSet<String> = BTreeSet::new();
    for s in &seeds {
        want.extend(m.bfs(s, 1, "both", None, None).into_keys());
    }
    let sql = "SELECT id FROM papers WHERE REACHABLE(cites, (SELECT id FROM papers WHERE MATCH(title, 'graph') ORDER BY year DESC, id LIMIT 3), \
               dir => 'both', min_hops => 0) ORDER BY similarity(emb, :q) DESC LIMIT 1000";
    let got = ids(&db, sql, &params);
    assert_eq!(got.iter().cloned().collect::<BTreeSet<_>>(), want);
    let mut want_order: Vec<(f32, String)> = want.iter().map(|k| (dot(&m.rows[k].2, &q), k.clone())).collect();
    want_order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    assert_eq!(got, want_order.into_iter().map(|x| x.1).collect::<Vec<_>>());
    let plan = db.query(sql, &params).unwrap().stats.plan.join("\n");
    assert!(plan.contains("PreFilter"), "{plan}");

    // Tuple seeds, and fusion of similarity with PageRank.
    let (a, b) = (seeds[0].clone(), seeds[1].clone());
    let n = db.query(&format!("SELECT COUNT(*) FROM papers WHERE REACHABLE(cites, ('{a}', '{b}'))"), &params).unwrap();
    let mut want: BTreeSet<String> = m.bfs(&a, 1, "out", None, None).into_iter().filter(|x| x.1 == 1).map(|x| x.0).collect();
    want.extend(m.bfs(&b, 1, "out", None, None).into_iter().filter(|x| x.1 == 1).map(|x| x.0));
    let both_seeds_reached = [&a, &b].iter().filter(|s| want.contains(**s)).count();
    assert!(n.rows[0][0].as_i64().unwrap() as usize >= want.len() - both_seeds_reached);
    let rs = db.query(&format!("SELECT id FROM papers ORDER BY rrf(similarity(emb, :q), ppr(cites, '{a}')) DESC LIMIT 10"), &params).unwrap();
    assert_eq!(rs.rows.len(), 10);
    let rs = db.query(&format!("SELECT id FROM papers ORDER BY 0.5 * minmax(similarity(emb, :q)) + 0.5 * minmax(ppr(cites, '{a}')) DESC LIMIT 10"), &params).unwrap();
    assert_eq!(rs.rows.len(), 10);

    // Semantic edges: a row's k most similar rows, then theirs.
    let near = |k: &str, n: usize| -> Vec<String> {
        let mut v: Vec<(f32, &String)> = m.rows.iter().filter(|(id, _)| id.as_str() != k).map(|(id, r)| (dot(&r.2, &m.rows[k].2), id)).collect();
        v.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap().then(x.1.cmp(y.1)));
        v.into_iter().take(n).map(|x| x.1.clone()).collect()
    };
    let got = set(&db, &format!("SELECT id FROM papers WHERE REACHABLE(emb, '{a}', k => 3) LIMIT 100"));
    assert_eq!(got, near(&a, 3).into_iter().collect());
    let mut want: BTreeSet<String> = near(&a, 3).into_iter().collect();
    for x in near(&a, 3) {
        want.extend(near(&x, 3));
    }
    want.remove(&a);
    assert_eq!(set(&db, &format!("SELECT id FROM papers WHERE REACHABLE(emb, '{a}', k => 3, hops => 2) LIMIT 1000")), want);
}

#[test]
fn integer_keys_unstored_edges_and_errors() {
    let schema = Schema::new(
        "g",
        vec![Field::new("id", FieldType::Int), Field::new("next", FieldType::Edges).not_stored()],
        Some("id"),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("i.strata"), schema, DbOptions { segment_max_docs: 4, sync: false, ..DbOptions::default() }).unwrap();
    // A ring 0 -> 1 -> ... -> 9 -> 0, with a chord 0 -> 5 of weight 2.
    db.load((0..10i64).map(|i| {
        let mut es = vec![Edge::to(Value::Int((i + 1) % 10))];
        if i == 0 {
            es.push(Edge::labeled("chord", Value::Int(5)).weight(2.0));
        }
        vec![Value::Int(i), edges_value(&es)]
    }))
    .unwrap();
    let col = |sql: &str| -> Vec<i64> { db.query(sql, &Params::new()).unwrap().rows.iter().map(|r| r[0].as_i64().unwrap()).collect() };
    assert_eq!(col("SELECT id FROM g WHERE REACHABLE(next, 0, hops => 2) ORDER BY hops(), id LIMIT 20"), vec![1, 5, 2, 6]);
    assert_eq!(col("SELECT id FROM g WHERE PATH(next, 2, 7) ORDER BY hops() LIMIT 20"), vec![2, 3, 4, 5, 6, 7]);
    assert_eq!(col("SELECT id FROM g WHERE PATH(next, 9, 6) ORDER BY hops() LIMIT 20"), vec![9, 0, 5, 6]);
    assert_eq!(col("SELECT id FROM g WHERE REACHABLE(next, 3, dir => 'in') LIMIT 20"), vec![2]);
    assert_eq!(col("SELECT id FROM g WHERE REACHABLE(next, 0, label => 'chord') LIMIT 20"), vec![5]);
    assert_eq!(col("SELECT id FROM g WHERE REACHABLE(next, 0, hops => 2) ORDER BY hops() DESC, id LIMIT 2"), vec![2, 6]);
    // Unstored edges come back from the index.
    let rs = db.query("SELECT next FROM g WHERE id = 0", &Params::new()).unwrap();
    let edges = chloedb::graph::parse_edges(&rs.rows[0][0], true).unwrap();
    assert_eq!(edges, vec![Edge::to(Value::Int(1)), Edge::labeled("chord", Value::Int(5)).weight(2.0)]);
    // ...and survive a merge.
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100, ..MergePolicy::default() }).unwrap();
    assert_eq!(col("SELECT id FROM g WHERE PATH(next, 9, 6) ORDER BY hops() LIMIT 20"), vec![9, 0, 5, 6]);

    // Ties on hops() are broken by the key over all tied rows, not the
    // first k found: a star whose leaves sit in several segments.
    let star = Schema::new("s", vec![Field::new("id", FieldType::Int), Field::new("e", FieldType::Edges)], Some("id")).unwrap();
    let sdb = Db::create(&dir.path().join("s.strata"), star, DbOptions { segment_max_docs: 7, sync: false, ..DbOptions::default() }).unwrap();
    let mut leaves: Vec<i64> = (1..=40).collect();
    leaves.reverse();
    let mut rows = vec![vec![Value::Int(0), edges_value(&leaves.iter().map(|&l| Edge::to(Value::Int(l))).collect::<Vec<_>>())]];
    rows.extend(leaves.iter().map(|&l| vec![Value::Int(l), Value::Null]));
    sdb.load(rows).unwrap();
    let got: Vec<i64> = sdb.query("SELECT id FROM s WHERE REACHABLE(e, 0) ORDER BY hops(), id LIMIT 5", &Params::new()).unwrap().rows.iter().map(|r| r[0].as_i64().unwrap()).collect();
    assert_eq!(got, vec![1, 2, 3, 4, 5]);
    let got: Vec<i64> = sdb.query("SELECT id FROM s WHERE REACHABLE(e, 0) ORDER BY hops(), id DESC LIMIT 3", &Params::new()).unwrap().rows.iter().map(|r| r[0].as_i64().unwrap()).collect();
    assert_eq!(got, vec![40, 39, 38]);

    for bad in [
        "SELECT id FROM g WHERE REACHABLE(id, 0)",
        "SELECT id FROM g WHERE REACHABLE(next, 0, hopz => 2)",
        "SELECT id FROM g WHERE REACHABLE(next, 0, dir => 'up')",
        "SELECT id FROM g WHERE REACHABLE(next, 0, hops => 1, min_hops => 2)",
        "SELECT id, hops() FROM g LIMIT 1",
        "SELECT id FROM g WHERE REACHABLE(next, 0, through => REACHABLE(next, 1))",
    ] {
        assert!(db.query(bad, &Params::new()).is_err(), "{bad}");
    }
    let mut txn = db.storage().begin().unwrap();
    assert!(txn.insert(vec![Value::Int(99), Value::Str("[\"x\"]".into())]).is_err());
    assert!(txn.insert(vec![Value::Int(98), Value::Str("not json".into())]).is_err());
    assert!(Schema::new("h", vec![Field::new("a", FieldType::Int), Field::new("e", FieldType::Edges)], None).is_err());
}

/// `PATH` with `through` against a breadth-first search that passes only
/// through the rows that satisfy it (NADIA-305): 2,000 rows of kind x or
/// y, four random out-edges each, written in four batches. The answer was
/// a path through a row of kind y whenever the two halves of the search
/// met there, and sometimes a path where no allowed one exists.
#[test]
fn path_keeps_to_through_on_a_random_graph_written_in_batches() {
    let schema = Schema::new("n", vec![Field::new("id", FieldType::Int), Field::new("kind", FieldType::Keyword), Field::new("e", FieldType::Edges)], Some("id")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("p.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(305);
    let n = 2000usize;
    let kind: Vec<bool> = (0..n).map(|_| rng.gen_bool(0.5)).collect();
    let out: Vec<Vec<usize>> = (0..n).map(|_| (0..4).map(|_| rng.gen_range(0..n)).collect()).collect();
    for batch in 0..4 {
        let mut txn = db.storage().begin().unwrap();
        for i in (batch * n / 4)..((batch + 1) * n / 4) {
            let es: Vec<Edge> = out[i].iter().map(|&t| Edge::to(Value::Int(t as i64))).collect();
            txn.insert(vec![Value::Int(i as i64), Value::Str(if kind[i] { "x" } else { "y" }.into()), edges_value(&es)]).unwrap();
        }
        txn.commit().unwrap();
        db.storage().flush().unwrap();
    }
    assert!(db.snapshot().segments().len() >= 4);
    // The shortest path from s to t whose middle rows are all of kind x.
    let bfs = |s: usize, t: usize, max: u32| -> Option<u32> {
        let mut dist = vec![u32::MAX; n];
        dist[s] = 0;
        let mut q = VecDeque::from([s]);
        while let Some(u) = q.pop_front() {
            if u == t {
                return Some(dist[u]);
            }
            if dist[u] == max || (u != s && !kind[u]) {
                continue;
            }
            for &v in &out[u] {
                if dist[v] == u32::MAX {
                    dist[v] = dist[u] + 1;
                    q.push_back(v);
                }
            }
        }
        None
    };
    let (mut found, mut none) = (0, 0);
    for _ in 0..200 {
        let (s, t) = (rng.gen_range(0..n), rng.gen_range(0..n));
        let sql = format!("SELECT id, hops() FROM n WHERE PATH(e, {s}, {t}, max_hops => 6, through => kind = 'x') ORDER BY hops() LIMIT 100");
        let rs = db.query(&sql, &Params::new()).unwrap();
        let path: Vec<usize> = rs.rows.iter().map(|r| r[0].as_i64().unwrap() as usize).collect();
        match bfs(s, t, 6) {
            None => {
                none += 1;
                assert!(path.is_empty(), "{s} -> {t}: no path keeps to x within 6 hops, got {path:?}");
            }
            Some(d) => {
                found += 1;
                assert_eq!(path.len() as u32, d + 1, "{s} -> {t}: {path:?}");
                assert_eq!((path[0], *path.last().unwrap()), (s, t));
                for w in path.windows(2) {
                    assert!(out[w[0]].contains(&w[1]), "{} -> {} is not an edge", w[0], w[1]);
                }
                for &m in &path[1..path.len() - 1] {
                    assert!(kind[m], "{s} -> {t}: {m} is of kind y, in {path:?}");
                }
            }
        }
        // The same both ways round: against the edges, from t back to s.
        let rs = db.query(&format!("SELECT id FROM n WHERE PATH(e, {t}, {s}, max_hops => 6, dir => 'in', through => kind = 'x') ORDER BY hops() LIMIT 100"), &Params::new()).unwrap();
        let back: Vec<usize> = rs.rows.iter().map(|r| r[0].as_i64().unwrap() as usize).collect();
        assert_eq!(back.len(), path.len(), "{t} <- {s}: {back:?} against {path:?}");
        if back.len() > 2 {
            assert!(back[1..back.len() - 1].iter().all(|&m| kind[m]), "{t} <- {s}: {back:?}");
        }
    }
    // Both kinds of answer were checked.
    assert!(found > 20 && none > 20, "{found} paths, {none} without");
}

/// `ppr` from a user over a user-item graph whose items each have about
/// 900 edges (NADIA-302): at `eps` 10⁻⁴ a user's 30 items get a residual
/// of 0.028 each, far below 10⁻⁴ times 900, so the walk pushed nothing
/// past the seed and ranked no item. Without `eps` it lowers it until half
/// of the walk is settled; with `eps` it walks as asked, and says that its
/// scores stayed near the seed.
#[test]
fn ppr_from_a_user_reaches_items_with_many_edges() {
    let schema = Schema::new("nodes", vec![Field::new("id", FieldType::Keyword), Field::new("kind", FieldType::Keyword), Field::new("e", FieldType::Edges)], Some("id")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("ui.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(302);
    let (users, items) = (3000usize, 100usize);
    let mut likes: Vec<Vec<usize>> = Vec::new();
    for _ in 0..users {
        let mut l: Vec<usize> = Vec::new();
        while l.len() < 30 {
            // Items with a lower number are liked more often.
            let i = (rng.gen_range(0.0f64..1.0).powi(2) * items as f64) as usize;
            if !l.contains(&i) {
                l.push(i);
            }
        }
        likes.push(l);
    }
    let mut rows: Vec<Vec<Value>> = (0..items).map(|i| vec![Value::Str(format!("i{i}")), Value::Str("item".into()), Value::Null]).collect();
    for (u, l) in likes.iter().enumerate() {
        let es: Vec<Edge> = l.iter().map(|i| Edge::to(format!("i{i}").as_str())).collect();
        rows.push(vec![Value::Str(format!("u{u}")), Value::Str("user".into()), edges_value(&es)]);
    }
    db.load(rows).unwrap();
    // PageRank from u0 by power iteration, the graph read both ways.
    let n = users + items;
    let mut nb: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u, l) in likes.iter().enumerate() {
        for &i in l {
            nb[items + u].push(i);
            nb[i].push(items + u);
        }
    }
    let least = (0..items).map(|i| nb[i].len()).min().unwrap();
    assert!(least > 300, "the least liked item has {least} edges");
    let alpha = 0.15;
    let mut x = vec![0.0f64; n];
    for _ in 0..200 {
        let mut y = vec![0.0; n];
        y[items] += alpha;
        for u in 0..n {
            for &v in &nb[u] {
                y[v] += (1.0 - alpha) * x[u] / nb[u].len() as f64;
            }
        }
        x = y;
    }
    let sql = |opt: &str| format!("SELECT id, score() FROM nodes WHERE kind = 'item' AND NOT REACHABLE(e, 'u0', hops => 1) ORDER BY ppr(e, 'u0'{opt}) DESC LIMIT 10");
    // As asked: 10⁻⁴ settles the seed's own share and stops.
    let rs = db.query(&sql(", eps => 1e-4"), &Params::new()).unwrap();
    assert!(rs.rows.is_empty(), "{:?}", rs.rows);
    assert!(rs.stats.ppr_unsettled, "{:?}", rs.stats.plan);
    assert!(rs.stats.plan.iter().any(|l| l.contains("stayed near the seeds") && l.contains("lower eps")), "{:?}", rs.stats.plan);
    assert_eq!(serde_json::to_value(&rs.stats).unwrap()["ppr_unsettled"].as_bool(), Some(true));
    // Left to itself, the walk ranks the items u0 has not liked, each
    // score at most its PageRank (a push never overshoots), and close.
    let rs = db.query(&sql(""), &Params::new()).unwrap();
    assert_eq!(rs.rows.len(), 10, "{:?}", rs.stats.plan);
    assert!(!rs.stats.ppr_unsettled && !rs.stats.max_pushes_reached, "{:?}", rs.stats.plan);
    let line = rs.stats.plan.iter().find(|l| l.contains("ppr over e")).unwrap();
    assert!(line.contains("eps lowered to") && !line.contains("near the seeds"), "{line}");
    let liked: BTreeSet<usize> = likes[0].iter().copied().collect();
    let mut want: Vec<(f64, usize)> = (0..items).filter(|i| !liked.contains(i)).map(|i| (x[i], i)).collect();
    want.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let top: BTreeSet<String> = want.iter().take(10).map(|w| format!("i{}", w.1)).collect();
    let mut overlap = 0;
    for r in &rs.rows {
        let id = r[0].as_str().unwrap();
        let i: usize = id[1..].parse().unwrap();
        let got = r[1].as_f64().unwrap();
        assert!(!liked.contains(&i), "{id} was liked");
        assert!(got <= x[i] + 1e-12 && got > 0.5 * x[i], "{id}: {got} against {}", x[i]);
        overlap += top.contains(id) as usize;
    }
    assert!(overlap >= 7, "{overlap} of the 10 best: {:?}", rs.rows);
    // A walk cut short by max_pushes says so both ways.
    let rs = db.query(&sql(", max_pushes => 3"), &Params::new()).unwrap();
    assert!(rs.stats.max_pushes_reached && rs.stats.ppr_unsettled, "{:?}", rs.stats.plan);
    // eps must be above 0 when it is given.
    assert!(db.query(&sql(", eps => 0"), &Params::new()).is_err());
}
