//! Date functions through a coordinator: `date_trunc`, `date_part` and
//! `EXTRACT`, and `strftime` reach the shards in plans (plan feature
//! "dates"), a `GROUP BY` of one over a TIMESTAMP field is each shard's
//! executor's, its groups merged by value, and the row evaluator computes
//! them over the rows the shards return. Each answer is the one
//! database's of the same rows.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Principal;
use chloedb_server::service::{AgentConfig, After, Backend, QueryReq, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

const TABLE: &str = "CREATE TABLE snaps (id KEYWORD PRIMARY KEY, item KEYWORD, at TIMESTAMP, total FLOAT)";

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let opts = DbOptions { segment_max_docs: 60, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(10), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    Arc::new(db)
}

async fn serve(db: Arc<Db>) -> String {
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

async fn run(svc: &Arc<Service>, s: &str) {
    svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message));
}

async fn select(svc: &Arc<Service>, sql: &str) -> Vec<Vec<Value>> {
    svc.query(QueryReq { sql: sql.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{sql}: {}", e.message)).rs.rows
}

fn local(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    match db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn date_functions_through_a_coordinator_answer_as_one_database() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve(create(d, "s0.strata")).await, serve(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let one = create(d, "one.strata");
    run(&svc, TABLE).await;
    local(&one, TABLE);
    // A crawl every 7 hours for 40 days, of 6 items, a few without a time.
    for chunk in (0..800).collect::<Vec<_>>().chunks(80) {
        let values: Vec<String> = chunk
            .iter()
            .map(|i| {
                let at = match i % 37 {
                    0 => "NULL".to_string(),
                    _ => format!("'{}'", chloedb::timestamp::format(1_785_542_400_000_000 + (i / 6) as i64 * 7 * 3_600_000_000 + (i % 6) as i64 * 61_000_000)),
                };
                format!("('s{i:04}', 'item{}', {at}, {}.5)", i % 6, 100 + i % 23)
            })
            .collect();
        let insert = format!("INSERT INTO snaps (id, item, at, total) VALUES {}", values.join(", "));
        run(&svc, &insert).await;
        local(&one, &insert);
    }
    let sorted = |mut rows: Vec<Vec<Value>>| {
        rows.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        rows
    };
    for q in [
        // Groups, each shard's merged by value.
        "SELECT date_trunc('day', at) AS day, COUNT(*), AVG(total), MIN(total) FROM snaps GROUP BY day",
        "SELECT item, date_trunc('week', at) AS w, COUNT(*), MAX(total) FROM snaps GROUP BY item, w",
        "SELECT EXTRACT(dow FROM at) AS dow, COUNT(*) FROM snaps GROUP BY dow HAVING COUNT(*) > 10",
        "SELECT strftime('%Y-%m', at) AS m, SUM(total) FROM snaps WHERE item <> 'item3' GROUP BY m",
        "SELECT date_part('hour', at) AS h, COUNT(*) AS n FROM snaps GROUP BY h ORDER BY n DESC, h LIMIT 3",
        // Values of the shards' rows, and a filter over them.
        "SELECT id, date_trunc('month', at), strftime('%F %T', at), EXTRACT(epoch FROM at) FROM snaps WHERE item = 'item1'",
        "SELECT id FROM snaps WHERE EXTRACT(dow FROM at) = 0 AND item = 'item2'",
        // The trend query: groups by day in a WITH, a moving average over them.
        "WITH daily AS (SELECT item, date_trunc('day', at) AS day, AVG(total) AS px FROM snaps WHERE at IS NOT NULL GROUP BY item, day) \
         SELECT item, day, px, AVG(px) OVER (PARTITION BY item ORDER BY day ROWS BETWEEN 6 PRECEDING AND CURRENT ROW) AS ma7 FROM daily",
    ] {
        let want = sorted(local(&one, q));
        assert!(!want.is_empty(), "{q}");
        assert_eq!(sorted(select(&svc, q).await), want, "{q}");
    }
}
