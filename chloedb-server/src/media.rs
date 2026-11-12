//! Fetching the files MEDIA fields name over HTTP(S) ([`chloedb::media`]):
//! the [`chloedb::media::Fetcher`] a server gives its database with
//! `--media-fetch`.
//!
//! A fetch has a timeout (`--media-timeout-s`), reads at most
//! `--media-max-bytes`, and follows at most [`MAX_REDIRECTS`] redirects,
//! each to a URL the allowlist has: a redirect off the list fails the
//! fetch, so a listed host cannot send the server to another one. The
//! allowlist is checked against the URL as the client parsed it (its
//! host and port), the one it connects to, before the request and at
//! each redirect.

use std::time::Duration;

use chloedb::media::{Allowlist, FetchError, Fetcher};

/// Redirects a fetch follows.
pub const MAX_REDIRECTS: usize = 5;

/// The default of `--media-timeout-s`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct HttpFetcher {
    client: reqwest::Client,
    allow: Allowlist,
    runtime: tokio::runtime::Handle,
}

/// Whether the allowlist has a URL as the HTTP client parsed it: its
/// scheme, host and port, with no user name or password.
pub fn allowed(allow: &Allowlist, url: &reqwest::Url) -> bool {
    url.username().is_empty() && url.password().is_none() && url.host_str().zip(url.port_or_known_default()).is_some_and(|(h, p)| allow.allows_host(url.scheme(), h, p))
}

impl HttpFetcher {
    /// A fetcher of the URLs `allow` has, running on the current Tokio
    /// runtime.
    pub fn new(allow: Allowlist, timeout: Duration) -> Result<HttpFetcher, String> {
        let list = allow.clone();
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error(format!("more than {MAX_REDIRECTS} redirects"))
            } else if allowed(&list, attempt.url()) {
                attempt.follow()
            } else {
                let to = attempt.url().to_string();
                attempt.error(format!("a redirect to {to}, which is not on the media fetch list"))
            }
        });
        let client = crate::http_client().redirect(policy).timeout(timeout).build().map_err(|e| format!("the media fetch client: {e}"))?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|e| format!("the media fetch client needs a Tokio runtime: {e}"))?;
        Ok(HttpFetcher { client, allow, runtime })
    }
}

impl Fetcher for HttpFetcher {
    /// Called by a write before the writer, on a thread that may block
    /// (never a runtime worker): the fetch runs on the runtime while it
    /// waits.
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        // The resolver checked the URL as it reads it; this checks it as
        // the client does, and the client is given what was checked.
        let url = reqwest::Url::parse(url).map_err(|e| FetchError::Failed(format!("not fetched: not a URL ({e})")))?;
        if !allowed(&self.allow, &url) {
            return Err(FetchError::Failed("not fetched: its host is not on the media fetch list (--media-fetch)".into()));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let client = self.client.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(fetch(&client, &url, max_bytes).await);
        });
        rx.recv().unwrap_or_else(|_| Err(FetchError::Failed("the fetch stopped (the server is shutting down)".into())))
    }
}

async fn fetch(client: &reqwest::Client, url: &reqwest::Url, max: u64) -> Result<Vec<u8>, FetchError> {
    let mut r = client.get(url.clone()).send().await.map_err(failed)?;
    if !r.status().is_success() {
        return Err(FetchError::Failed(format!("GET returned {}", r.status())));
    }
    let too_big = || FetchError::Failed(format!("larger than the media size cap of {max} bytes (--media-max-bytes)"));
    if r.content_length().is_some_and(|n| n > max) {
        return Err(too_big());
    }
    let mut out = Vec::new();
    while let Some(c) = r.chunk().await.map_err(failed)? {
        if (out.len() + c.len()) as u64 > max {
            return Err(too_big());
        }
        out.extend_from_slice(&c);
    }
    Ok(out)
}

/// A reqwest error with its causes (a redirect refused says why).
fn failed(e: reqwest::Error) -> FetchError {
    let mut m = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        m.push_str(&format!(": {s}"));
        src = s.source();
    }
    if e.is_timeout() {
        FetchError::Timeout(format!("the fetch took longer than --media-timeout-s ({m})"))
    } else {
        FetchError::Failed(m)
    }
}
