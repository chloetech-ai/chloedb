//! `chloedb-workbench hook`: what each session's hooks run. It posts the
//! hook's JSON to the server and never fails or prints, so that a session
//! never stops because Workbench did.

use std::io::Read;
use std::time::Duration;

/// The server a session reports to, from its environment.
pub struct Env {
    pub url: String,
    pub token: String,
    pub agent: String,
}

impl Env {
    pub fn from_env() -> Option<Env> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Some(Env { url: get("CHLOEDB_WORKBENCH_URL")?.trim_end_matches('/').to_string(), token: get("CHLOEDB_WORKBENCH_TOKEN")?, agent: get("CHLOEDB_WORKBENCH_AGENT").unwrap_or_default() })
    }
}

/// An HTTP client for the server on loopback: no proxy, errors as answers.
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).proxy(None).build().into()
}

pub fn run() {
    let Some(env) = Env::from_env() else { return };
    if env.agent.is_empty() {
        return;
    }
    let mut body = Vec::new();
    if std::io::stdin().take(8 << 20).read_to_end(&mut body).is_err() || body.is_empty() {
        return;
    }
    let _ = agent(Duration::from_secs(2))
        .post(format!("{}/api/hook", env.url))
        .header("authorization", format!("Bearer {}", env.token))
        .header("x-workbench-agent", &env.agent)
        .header("content-type", "application/json")
        .send(&body[..]);
}
