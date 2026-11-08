//! Percentiles through a coordinator: `MEDIAN`, `PERCENTILE_CONT` and
//! `PERCENTILE_DISC` reach the shards in plans (plan feature
//! "percentiles"), each shard sends a group's values as runs of equal
//! ones, and the coordinator takes the percentile of them all; over a join
//! or a WITH, the operators compute them over the rows the shards return.
//! Each answer is the one database's of the same rows.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::storage::DbOptions;
use strata::value::Value;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::keys::Principal;
use strata_server::service::{AgentConfig, After, Backend, QueryReq, Service};
use strata_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

const TABLE: &str = "CREATE TABLE snaps (id KEYWORD PRIMARY KEY, item KEYWORD, at TIMESTAMP, total FLOAT, qty INT)";

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let opts = DbOptions { segment_max_docs: 60, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(10), opts).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
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
async fn percentiles_through_a_coordinator_answer_as_one_database() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve(create(d, "s0.strata")).await, serve(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let one = create(d, "one.strata");
    run(&svc, TABLE).await;
    local(&one, TABLE);
    // A crawl of 7 items: prices that repeat, an outlier now and then, and
    // a few rows without one.
    for chunk in (0..900).collect::<Vec<_>>().chunks(90) {
        let values: Vec<String> = chunk
            .iter()
            .map(|i| {
                let total = match i % 41 {
                    0 => "NULL".to_string(),
                    1 => "99999.5".to_string(),
                    _ => format!("{}.25", 100 + (i * 7) % 31),
                };
                let at = strata::timestamp::format(1_785_542_400_000_000 + *i as i64 * 3_600_000_000);
                format!("('s{i:04}', 'item{}', '{at}', {total}, {})", i % 7, (i * 13) % 50)
            })
            .collect();
        let insert = format!("INSERT INTO snaps (id, item, at, total, qty) VALUES {}", values.join(", "));
        run(&svc, &insert).await;
        local(&one, &insert);
    }
    let sorted = |mut rows: Vec<Vec<Value>>| {
        rows.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        rows
    };
    for q in [
        // Groups, each shard's values merged.
        "SELECT item, MEDIAN(total), AVG(total), PERCENTILE_DISC(0.9) WITHIN GROUP (ORDER BY qty), PERCENTILE_CONT(0.1) WITHIN GROUP (ORDER BY qty) FROM snaps GROUP BY item",
        "SELECT MEDIAN(total), PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY at), PERCENTILE_CONT(1) WITHIN GROUP (ORDER BY total) FROM snaps",
        "SELECT qty, MEDIAN(total) AS m FROM snaps WHERE qty < 20 GROUP BY qty HAVING MEDIAN(total) > 110 ORDER BY m DESC, qty LIMIT 5",
        // The operators', over the rows the shards return.
        "WITH t AS (SELECT item, total FROM snaps WHERE total < 1000) SELECT item, MEDIAN(total), PERCENTILE_DISC(0.25) WITHIN GROUP (ORDER BY total) FROM t GROUP BY item",
    ] {
        let want = sorted(local(&one, q));
        assert!(!want.is_empty(), "{q}");
        assert_eq!(sorted(select(&svc, q).await), want, "{q}");
    }
}
