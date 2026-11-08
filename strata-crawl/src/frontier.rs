//! The frontier: the URLs waiting to be fetched, one queue per host,
//! handed out breadth first and politely (at most `per_host` requests to a
//! host at a time, and `delay` between the starts of two), with every URL
//! ever queued remembered so that none is fetched twice.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use url::Url;

/// A URL to fetch, and how far from a seed it is.
#[derive(Clone, Debug)]
pub struct Item {
    pub url: Url,
    pub depth: u32,
}

struct Host {
    queue: VecDeque<Item>,
    /// Requests out now.
    active: usize,
    /// When the next request may start.
    next_at: Instant,
    delay: Duration,
    /// Whether the host is in `ready` (so that it is there once).
    scheduled: bool,
}

struct Inner {
    hosts: HashMap<String, Host>,
    /// Hosts with URLs that may be handed out, by when.
    ready: BinaryHeap<Reverse<(Instant, String)>>,
    seen: HashSet<u64>,
    queued: usize,
    in_flight: usize,
    /// URLs handed out so far, which `limit` caps.
    handed: u64,
    limit: u64,
    /// URLs not queued because the queue was full.
    dropped: u64,
    paused: bool,
    stopped: bool,
}

pub struct Frontier {
    inner: Mutex<Inner>,
    notify: Notify,
    per_host: usize,
    delay: Duration,
    max_queued: usize,
}

/// How the frontier stands, for the status.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Counts {
    pub queued: usize,
    pub in_flight: usize,
    pub seen: usize,
    pub hosts: usize,
    pub handed: u64,
    pub dropped: u64,
}

impl Frontier {
    pub fn new(per_host: usize, delay: Duration, limit: u64, max_queued: usize) -> Frontier {
        Frontier {
            inner: Mutex::new(Inner {
                hosts: HashMap::new(),
                ready: BinaryHeap::new(),
                seen: HashSet::new(),
                queued: 0,
                in_flight: 0,
                handed: 0,
                limit,
                dropped: 0,
                paused: false,
                stopped: false,
            }),
            notify: Notify::new(),
            per_host: per_host.max(1),
            delay,
            max_queued,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Marks a URL seen without queueing it (a redirect's target, a
    /// page's canonical address). Whether it was new.
    pub fn mark_seen(&self, url: &str) -> bool {
        self.lock().seen.insert(crate::urls::hash(url))
    }

    /// Queues a URL unless it was seen. Whether it was queued.
    pub fn push(&self, item: Item) -> bool {
        let mut g = self.lock();
        if g.stopped || !g.seen.insert(crate::urls::hash(item.url.as_str())) {
            return false;
        }
        if g.queued >= self.max_queued {
            g.dropped += 1;
            return false;
        }
        let host = item.url.host_str().unwrap_or("").to_string();
        let now = Instant::now();
        let per_host = self.per_host;
        let delay = self.delay;
        let inner = &mut *g;
        let h = inner.hosts.entry(host.clone()).or_insert_with(|| Host { queue: VecDeque::new(), active: 0, next_at: now, delay, scheduled: false });
        h.queue.push_back(item);
        inner.queued += 1;
        if !h.scheduled && h.active < per_host {
            h.scheduled = true;
            inner.ready.push(Reverse((h.next_at.max(now), host)));
        }
        drop(g);
        self.notify.notify_waiters();
        true
    }

    /// Sets a host's delay between requests (from its robots.txt), when
    /// longer than the crawl's own.
    pub fn slow_down(&self, host: &str, delay: Duration) {
        let mut g = self.lock();
        if let Some(h) = g.hosts.get_mut(host) {
            h.delay = h.delay.max(delay);
        }
    }

    /// The next URL to fetch, waiting while every host waits out its delay
    /// or has its requests out; `None` once nothing is queued or out (the
    /// crawl is done), or once stopped or the limit is reached.
    pub async fn next(&self) -> Option<Item> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let wait = {
                let mut g = self.lock();
                // Out of URLs for good: stopped, at the limit, or nothing
                // queued and nothing out that could find more.
                if g.stopped || g.handed >= g.limit || (g.queued == 0 && g.in_flight == 0) {
                    drop(g);
                    self.notify.notify_waiters();
                    return None;
                }
                if g.paused {
                    None
                } else {
                    let now = Instant::now();
                    match g.ready.peek().map(|Reverse((t, _))| *t) {
                        Some(t) if t <= now => {
                            let Reverse((_, host)) = g.ready.pop().expect("peeked");
                            let per_host = self.per_host;
                            let inner = &mut *g;
                            let h = inner.hosts.get_mut(&host).expect("a ready host is known");
                            h.scheduled = false;
                            if h.active >= per_host || h.queue.is_empty() {
                                continue;
                            }
                            let item = h.queue.pop_front().expect("not empty");
                            h.active += 1;
                            h.next_at = now + h.delay;
                            if !h.queue.is_empty() && h.active < per_host {
                                h.scheduled = true;
                                inner.ready.push(Reverse((h.next_at, host)));
                            }
                            inner.queued -= 1;
                            inner.in_flight += 1;
                            inner.handed += 1;
                            return Some(item);
                        }
                        Some(t) => Some(t - now),
                        // URLs queued only behind hosts at their limit: wait for one to finish.
                        None => None,
                    }
                }
            };
            match wait {
                Some(d) => {
                    let _ = tokio::time::timeout(d, notified).await;
                }
                None => {
                    let _ = tokio::time::timeout(Duration::from_secs(1), notified).await;
                }
            }
        }
    }

    /// A URL `next` handed out is done with.
    pub fn done(&self, url: &Url) {
        let mut g = self.lock();
        let host = url.host_str().unwrap_or("").to_string();
        let now = Instant::now();
        let per_host = self.per_host;
        let inner = &mut *g;
        inner.in_flight = inner.in_flight.saturating_sub(1);
        if let Some(h) = inner.hosts.get_mut(&host) {
            h.active = h.active.saturating_sub(1);
            if !h.scheduled && !h.queue.is_empty() && h.active < per_host {
                h.scheduled = true;
                inner.ready.push(Reverse((h.next_at.max(now), host)));
            }
        }
        drop(g);
        self.notify.notify_waiters();
    }

    pub fn set_paused(&self, paused: bool) {
        self.lock().paused = paused;
        self.notify.notify_waiters();
    }

    /// No more URLs: `next` answers `None` from now on.
    pub fn stop(&self) {
        let mut g = self.lock();
        g.stopped = true;
        drop(g);
        self.notify.notify_waiters();
    }

    /// Raises (or lowers) how many URLs may be handed out in all.
    pub fn set_limit(&self, limit: u64) {
        self.lock().limit = limit;
        self.notify.notify_waiters();
    }

    pub fn counts(&self) -> Counts {
        let g = self.lock();
        Counts { queued: g.queued, in_flight: g.in_flight, seen: g.seen.len(), hosts: g.hosts.len(), handed: g.handed, dropped: g.dropped }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(s: &str, depth: u32) -> Item {
        Item { url: Url::parse(s).unwrap(), depth }
    }

    #[tokio::test]
    async fn breadth_first_once_each_and_done() {
        let f = Frontier::new(1, Duration::ZERO, 100, 1000);
        assert!(f.push(item("http://a/1", 0)));
        assert!(!f.push(item("http://a/1", 0)), "seen once");
        assert!(f.push(item("http://a/2", 1)));
        let a = f.next().await.unwrap();
        assert_eq!(a.url.path(), "/1");
        // a's one request is out: the next waits until it is done.
        let waiting = tokio::time::timeout(Duration::from_millis(50), f.next()).await;
        assert!(waiting.is_err());
        f.done(&a.url);
        let b = f.next().await.unwrap();
        assert_eq!(b.url.path(), "/2");
        f.done(&b.url);
        assert!(f.next().await.is_none(), "nothing queued or out: done");
    }

    #[tokio::test]
    async fn delay_between_requests_and_limit() {
        let f = Frontier::new(4, Duration::from_millis(80), 2, 1000);
        for i in 0..5 {
            f.push(item(&format!("http://a/{i}"), 0));
        }
        let t0 = Instant::now();
        let x = f.next().await.unwrap();
        let y = f.next().await.unwrap();
        assert!(t0.elapsed() >= Duration::from_millis(75), "the second start waits the delay");
        f.done(&x.url);
        f.done(&y.url);
        assert!(f.next().await.is_none(), "the limit of 2 is reached");
        assert_eq!(f.counts().handed, 2);
    }

    #[tokio::test]
    async fn hosts_in_parallel() {
        let f = Frontier::new(1, Duration::from_secs(10), 100, 1000);
        f.push(item("http://a/1", 0));
        f.push(item("http://b/1", 0));
        let t0 = Instant::now();
        let x = f.next().await.unwrap();
        let y = f.next().await.unwrap();
        assert_ne!(x.url.host_str(), y.url.host_str());
        assert!(t0.elapsed() < Duration::from_secs(1));
    }
}
