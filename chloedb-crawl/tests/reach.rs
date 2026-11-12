//! The crawler reaches only what it may (audit G-B3, G-B4, G-B5), against
//! sites served here:
//!
//! - an address on this machine or a private network is not fetched, linked
//!   or redirected to, unless the crawl was given its host;
//! - a redirect is followed only to a page robots.txt allows and the crawl
//!   follows (its scope and `exclude`);
//! - robots.txt and sitemaps are read to a cap, so a site that answers
//!   without end does not hold the crawler.
//!
//! A test binary of its own: what the crawls of a process may reach is the
//! process's (`chloedb_crawl::reach::set`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{header, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use chloedb_crawl::api::Service;
use chloedb_crawl::fetch::{Fetcher, Miss};
use chloedb_crawl::grow::{Grower, How, Policy};
use chloedb_crawl::job::Spec;
use chloedb_crawl::reach::Reach;
use chloedb_crawl::strata::Strata;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

#[derive(Clone, Default)]
struct Hits(Arc<Mutex<HashMap<String, usize>>>);

impl Hits {
    fn hit(&self, path: &str) {
        *self.0.lock().unwrap().entry(path.to_string()).or_default() += 1;
    }
    fn get(&self, path: &str) -> usize {
        self.0.lock().unwrap().get(path).copied().unwrap_or(0)
    }
    fn all(&self) -> usize {
        self.0.lock().unwrap().values().sum()
    }
}

fn html(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn listen() -> (tokio::net::TcpListener, u16) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

/// A service on the crawler's network that no crawl was given: it counts
/// what reaches it.
async fn internal() -> (Hits, u16) {
    let hits = Hits::default();
    let app = Router::new()
        .fallback(|State(h): State<Hits>, uri: Uri| async move {
            h.hit(uri.path());
            html("<html><head><title>internal</title></head><body>secret</body></html>".into())
        })
        .with_state(hits.clone());
    let (l, port) = listen().await;
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (hits, port)
}

/// A server whose every answer is `head` and then `filler` without end,
/// until the client hangs up.
async fn endless(head: &'static str, filler: &'static str) -> u16 {
    let (l, port) = listen().await;
    tokio::spawn(async move {
        loop {
            let Ok((mut c, _)) = l.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf).await;
                let start = format!("HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\nconnection: close\r\n\r\n{head}");
                if c.write_all(start.as_bytes()).await.is_err() {
                    return;
                }
                let chunk = filler.repeat(4096);
                while c.write_all(chunk.as_bytes()).await.is_ok() {}
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn private_addresses_are_not_fetched_unless_given() {
    let (inside, ip) = internal().await;
    let f = Fetcher::reaching("t", Duration::from_secs(5), 1 << 20, 1, false, Arc::new(Reach::default()));
    for u in [format!("http://127.0.0.1:{ip}/secret"), format!("http://localhost:{ip}/secret"), format!("http://[::1]:{ip}/secret"), format!("http://2130706433:{ip}/secret")] {
        match f.page(&Url::parse(&u).unwrap()).await {
            Err(Miss::Private(why)) => assert!(why.contains("not on the public internet"), "{u}: {why}"),
            other => panic!("{u}: {:?}", other.map(|g| g.url)),
        }
    }
    assert!(f.sitemap(&format!("http://127.0.0.1:{ip}/sitemap.xml"), 10).await.is_empty());
    assert!(f.robots(&Url::parse(&format!("http://127.0.0.1:{ip}/")).unwrap()).await.unreachable.is_none());
    assert_eq!(inside.all(), 0, "nothing reached the internal service");
    // Given its host (and port), it is a site like another.
    let f = Fetcher::reaching("t", Duration::from_secs(5), 1 << 20, 1, false, Arc::new(Reach { private: false, hosts: vec![("127.0.0.1".into(), Some(ip))] }));
    assert_eq!(f.page(&Url::parse(&format!("http://127.0.0.1:{ip}/ok")).unwrap()).await.unwrap().status, 200);
    assert_eq!(inside.get("/ok"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_are_checked_at_every_hop() {
    let (inside, ip) = internal().await;
    let (l, port) = listen().await;
    let base = format!("http://127.0.0.1:{port}");
    let hits = Hits::default();
    let to_inside = format!("http://127.0.0.1:{ip}/secret");
    let app = Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /private/\n" }))
        .route("/go", get(move || async move { Redirect::temporary(&to_inside) }))
        .route("/jump", get(|| async { Redirect::temporary("/private/report") }))
        .route("/out", get(|| async { Redirect::permanent("/excluded/page") }))
        .route("/ok", get(|| async { Redirect::permanent("/fine") }))
        .fallback(|State(h): State<Hits>, uri: Uri| async move {
            h.hit(uri.path());
            html(format!("<html><head><title>{}</title></head><body>a page</body></html>", uri.path()))
        })
        .with_state(hits.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let site = Reach { private: false, hosts: vec![("127.0.0.1".into(), Some(port))] };
    let f = Fetcher::reaching("t", Duration::from_secs(5), 1 << 20, 1, true, Arc::new(site));
    let u = |p: &str| Url::parse(&format!("{base}{p}")).unwrap();
    // To the internal service: refused at the hop, before it is asked.
    assert!(matches!(f.page(&u("/go")).await, Err(Miss::Private(_))));
    assert_eq!(inside.all(), 0);
    // To a page robots.txt keeps out: not fetched.
    assert!(matches!(f.page(&u("/jump")).await, Err(Miss::Robots)));
    assert_eq!(hits.get("/private/report"), 0);
    // To a page the crawl does not follow: not fetched.
    let follows = |x: &Url| !x.path().starts_with("/excluded/");
    match f.page_following(&u("/out"), &follows).await {
        Err(Miss::Redirected(to)) => assert!(to.ends_with("/excluded/page"), "{to}"),
        other => panic!("{:?}", other.map(|g| g.url)),
    }
    assert_eq!(hits.get("/excluded/page"), 0);
    // And a redirect that stays where it may is followed.
    let got = f.page_following(&u("/ok"), &follows).await.unwrap();
    assert_eq!(got.url.path(), "/fine");
    assert_eq!(hits.get("/fine"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn endless_robots_txt_and_sitemaps_are_read_to_a_cap() {
    let robots = endless("User-agent: *\nDisallow: /private/\n", "# and so on\n").await;
    let f = Fetcher::reaching("t", Duration::from_secs(60), 1 << 20, 1, true, Arc::new(Reach::any()));
    let t0 = Instant::now();
    let r = f.robots(&Url::parse(&format!("http://127.0.0.1:{robots}/")).unwrap()).await;
    assert!(r.unreachable.is_none(), "{:?}", r.unreachable);
    assert!(!r.allowed("/private/x") && r.allowed("/public"));
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    let maps = endless("<urlset><url><loc>https://example.com/a</loc></url>", "<!-- padding -->").await;
    let t0 = Instant::now();
    // Larger than a sitemap may be: skipped, and quickly.
    assert!(f.sitemap(&format!("http://127.0.0.1:{maps}/sitemap.xml"), 10).await.is_empty());
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crawl_follows_no_link_or_redirect_to_an_internal_address() {
    let (inside, ip) = internal().await;
    let (l, port) = listen().await;
    let base = format!("http://127.0.0.1:{port}");
    let hits = Hits::default();
    let to_inside = format!("http://127.0.0.1:{ip}/redirected");
    let links = format!(
        r#"<html><head><title>Home</title></head><body><a href="http://127.0.0.1:{ip}/linked">a</a> <a href="http://localhost:{ip}/linked">b</a> <a href="http://169.254.169.254/latest/meta-data/">c</a> <a href="/go">d</a> <a href="/about">e</a></body></html>"#
    );
    let app = Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nAllow: /\n" }))
        .route("/", get(move || async move { html(links.clone()) }))
        .route("/go", get(move || async move { Redirect::temporary(&to_inside) }))
        .fallback(|State(h): State<Hits>, uri: Uri| async move {
            h.hit(uri.path());
            html("<html><head><title>About</title></head><body>about</body></html>".into())
        })
        .with_state(hits.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    chloedb_crawl::reach::set(Reach { private: false, hosts: vec![("127.0.0.1".into(), Some(port))] });
    // No Strata: what matters is what is fetched.
    let strata = Strata::new("http://127.0.0.1:9", None);
    let svc = Service::new(strata.clone(), Grower::new(Policy { how: How::Off, split_rows: 0, max_shards: 1 }, strata, None), None, false);
    let spec: Spec = serde_json::from_value(json!({"seeds": [format!("{base}/")], "scope": "any", "delay_ms": 0, "sitemaps": false, "max_pages": 20})).unwrap();
    let job = svc.start(spec).unwrap();
    let t0 = Instant::now();
    while job.active() && hits.get("/about") == 0 {
        assert!(t0.elapsed() < Duration::from_secs(30), "{:#}", job.status());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Give the other links time to have been asked, had they been.
    tokio::time::sleep(Duration::from_millis(500)).await;
    job.stop();
    assert_eq!(hits.get("/about"), 1, "{:#}", job.status());
    assert_eq!(inside.all(), 0, "{:#}", job.status());
}
