//! The crawler is polite to the sites it reads, served here (no Strata):
//!
//! - its user agent names it, links to the page for site owners and
//!   carries the crawl's contact, also behind a crawl's own user agent;
//! - busy answers (429, 502, 503, 504) and their Retry-After are told
//!   apart from a page that is not there;
//! - a site whose robots.txt does not answer is not read until its
//!   robots.txt is read again, and then its Crawl-delay holds;
//! - a crawl gives up on a site that keeps refusing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::json;
use chloedb_crawl::api::Service;
use chloedb_crawl::fetch::{user_agent, Fetcher, Miss, ABOUT};
use chloedb_crawl::grow::{Grower, How, Policy};
use chloedb_crawl::job::Spec;
use chloedb_crawl::strata::Strata;
use url::Url;

#[derive(Clone, Default)]
struct Site {
    hits: Arc<Mutex<HashMap<String, usize>>>,
    agents: Arc<Mutex<Vec<String>>>,
}

impl Site {
    fn hit(&self, path: &str, h: &HeaderMap) -> usize {
        self.agents.lock().unwrap().push(h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string());
        let mut g = self.hits.lock().unwrap();
        let n = g.entry(path.to_string()).or_default();
        *n += 1;
        *n
    }
    fn hits(&self, prefix: &str) -> usize {
        self.hits.lock().unwrap().iter().filter(|(p, _)| p.starts_with(prefix)).map(|(_, n)| n).sum()
    }
}

fn page(title: &str) -> axum::response::Response {
    ([(header::CONTENT_TYPE, "text/html")], format!("<html><head><title>{title}</title></head><body><p>{title}</p></body></html>")).into_response()
}

async fn serve(app: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

fn fetcher(agent: &str) -> Fetcher {
    Fetcher::new(agent, Duration::from_secs(5), 1 << 20, 1, true)
}

fn url(base: &str, path: &str) -> Url {
    Url::parse(&format!("{base}{path}")).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_user_agent_names_the_crawler_its_page_and_the_contact() {
    let site = Site::default();
    let app = Router::new().route("/*p", get(|State(s): State<Site>, Path(p): Path<String>, h: HeaderMap| async move { s.hit(&p, &h); page("hi") })).with_state(site.clone());
    let base = serve(app).await;
    let v = env!("CARGO_PKG_VERSION");
    fetcher(&user_agent(None, Some("mailto:ops@example.com"))).page(&url(&base, "/a")).await.unwrap();
    fetcher(&user_agent(Some("AcmeBot/2.0"), None)).page(&url(&base, "/b")).await.unwrap();
    let agents = site.agents.lock().unwrap().clone();
    // Each fetcher read robots.txt with the same agent first.
    assert_eq!(agents[1], format!("strata-crawl/{v} (+{ABOUT}; mailto:ops@example.com)"));
    assert_eq!(agents[3], format!("AcmeBot/2.0 strata-crawl/{v} (+{ABOUT})"));
    assert_eq!(site.hits("robots.txt"), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn busy_answers_and_their_retry_after_are_told_apart() {
    let in_90s = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(90));
    let app = Router::new()
        .route("/robots.txt", get(|| async { StatusCode::NOT_FOUND }))
        .route("/slow-down", get(|| async { (StatusCode::TOO_MANY_REQUESTS, [(header::RETRY_AFTER, "7")], "slow down") }))
        .route("/down", get(move || { let at = in_90s.clone(); async move { (StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, at)], "down") } }))
        .route("/gateway", get(|| async { StatusCode::BAD_GATEWAY }))
        .route("/staff", get(|| async { StatusCode::FORBIDDEN }))
        .route("/gone", get(|| async { StatusCode::NOT_FOUND }));
    let base = serve(app).await;
    let f = fetcher(&user_agent(None, None));
    let miss = |p: &'static str| {
        let f = &f;
        let u = url(&base, p);
        async move { f.page(&u).await.err().unwrap() }
    };
    assert_eq!(miss("/slow-down").await, Miss::Busy(429, Some(Duration::from_secs(7))));
    match miss("/down").await {
        Miss::Busy(503, Some(d)) => assert!(d > Duration::from_secs(80) && d <= Duration::from_secs(90), "{d:?}"),
        m => panic!("{m:?}"),
    }
    assert_eq!(miss("/gateway").await, Miss::Busy(502, None));
    let staff = miss("/staff").await;
    assert_eq!((staff.clone(), staff.refusal()), (Miss::Status(403), Some((false, None))), "a refusal, not asked again");
    let gone = miss("/gone").await;
    assert_eq!((gone.clone(), gone.refusal()), (Miss::Status(404), None), "no refusal");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_whose_robots_txt_does_not_answer_waits_until_it_is_read_again() {
    let site = Site::default();
    let app = Router::new()
        .route(
            "/robots.txt",
            get(|State(s): State<Site>, h: HeaderMap| async move {
                if s.hit("robots.txt", &h) == 1 {
                    (StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, "1")], String::new()).into_response()
                } else {
                    "User-agent: *\nCrawl-delay: 0.3\n".into_response()
                }
            }),
        )
        .route("/p", get(|State(s): State<Site>, h: HeaderMap| async move { s.hit("p", &h); page("p") }))
        .with_state(site.clone());
    let base = serve(app).await;
    let f = fetcher(&user_agent(None, None));
    match f.page(&url(&base, "/p")).await {
        // Read again in 10 s at the soonest, whatever its Retry-After says.
        Err(Miss::RobotsUnreachable(why, wait)) => {
            assert!(why.contains("503"), "{why}");
            assert!(wait > Duration::from_secs(9) && wait <= Duration::from_secs(10), "{wait:?}");
            assert_eq!(Miss::RobotsUnreachable(why, wait).refusal(), Some((true, Some(wait))));
        }
        r => panic!("{:?}", r.err()),
    }
    assert_eq!(site.hits("p"), 0, "no page from a site whose robots.txt did not answer");
    tokio::time::sleep(Duration::from_millis(10_100)).await;
    f.page(&url(&base, "/p")).await.unwrap();
    assert_eq!(f.robots(&url(&base, "/")).await.crawl_delay, Some(0.3));
    f.page(&url(&base, "/p")).await.unwrap();
    assert_eq!(site.hits("robots.txt"), 2, "read again once, then kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crawl_gives_up_on_a_site_that_keeps_refusing() {
    let site = Site::default();
    let app = Router::new()
        .route("/robots.txt", get(|| async { StatusCode::NOT_FOUND }))
        .route("/x/:i", get(|State(s): State<Site>, Path(i): Path<u32>, h: HeaderMap| async move { s.hit(&format!("x/{i}"), &h); (StatusCode::SERVICE_UNAVAILABLE, [(header::RETRY_AFTER, "0")], "busy") }))
        .with_state(site.clone());
    let base = serve(app).await;
    // No Strata: nothing is read, so nothing is written.
    let strata = Strata::new("http://127.0.0.1:9", None);
    let svc = Service::new(strata.clone(), Grower::new(Policy { how: How::Off, split_rows: 0, max_shards: 1 }, strata, None), None, false);
    let seeds: Vec<String> = (1..=12).map(|i| format!("{base}/x/{i}")).collect();
    let spec: Spec = serde_json::from_value(json!({"seeds": seeds, "delay_ms": 0, "sitemaps": false})).unwrap();
    let job = svc.start(spec).unwrap();
    let t0 = Instant::now();
    while job.active() {
        assert!(t0.elapsed() < Duration::from_secs(30), "{:#}", job.status());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let s = job.status();
    assert_eq!(s["phase"], "failed");
    assert!(s["error"].as_str().unwrap().contains("kept refusing"), "{}", s["error"]);
    let c = &s["counts"];
    assert_eq!(c["gave_up"], json!(["127.0.0.1"]), "{c:#}");
    // One page at a time, each asked up to 4 times, until 10 refusals in
    // a row: 4 + 4 + 2.
    assert_eq!(site.hits("x/"), 10, "{c:#}");
    assert_eq!((site.hits("x/1"), site.hits("x/2"), site.hits("x/3"), site.hits("x/4")), (4, 4, 2, 0));
    assert_eq!(c["asked_again"], 7, "{c:#}");
    assert_eq!(c["misses"]["busy"], 10, "{c:#}");
    let events = s["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e["kind"] == "polite" && e["text"].as_str().unwrap().starts_with("gave up on 127.0.0.1: 10 refusals in a row")), "{events:#?}");
    assert!(events.iter().any(|e| e["kind"] == "polite" && e["text"].as_str().unwrap().contains("HTTP 503: the site is busy")), "{events:#?}");
}
