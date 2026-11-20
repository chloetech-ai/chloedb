//! chloedb-earth: California and Chile from space, on ChloeDB.
//!
//! `load` makes the tables and loads the places `tools/places.py` wrote
//! (and the years `tools/years.py` did); `goes` reads GOES's fire pixels
//! (the archive from a time on, then with `--live` each new scan) and
//! groups them into fires; `rules` runs the alert rules once; `backtest`
//! measures how well the fires like each one foretold its size; `serve` is
//! the app (its API and its page). Everything read is public.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use chloedb_earth::{analogs, api, feed, load, places::Places, rules, strata::Strata};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chloedb-earth", version, about = "California and Chile from space, on ChloeDB")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Clone)]
struct Server {
    #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
    strata: String,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
}

impl Server {
    fn db(&self) -> Strata {
        Strata::new(&self.strata, self.key.clone().filter(|k| !k.is_empty()))
    }
}

#[derive(Subcommand)]
enum Command {
    /// Make the tables, and load `<dir>/<table>*.jsonl` (ea_places,
    /// ea_years, ea_changes).
    Load {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Megabytes of JSON lines a request.
        #[arg(long, default_value_t = 8)]
        chunk_mb: usize,
        #[command(flatten)]
        server: Server,
    },
    /// Read GOES's fire pixels over a region into ea_detections and group
    /// them into ea_fires: the archive from --from to --to, then with
    /// --live every new scan.
    Goes {
        /// california or chile.
        #[arg(long)]
        region: String,
        /// The archive from this time (2025-01-07, 2025-01-07T17:00:00Z, or
        /// hours back such as 6h); left out, after the newest scan kept.
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// Read a scan every this many seconds of the archive (0: every
        /// scan, 5 minutes over California and 10 over Chile).
        #[arg(long, default_value_t = 0)]
        every: i64,
        #[arg(long)]
        live: bool,
        /// Scans downloaded at once.
        #[arg(long, default_value_t = 12)]
        parallel: usize,
        #[command(flatten)]
        server: Server,
    },
    /// Run every alert rule once, and print what they found.
    Rules {
        #[command(flatten)]
        server: Server,
    },
    /// How well the fires whose first hours were like each fire's foretold
    /// its size, each matched only against fires out before it: written to
    /// ea_backtest.
    Backtest {
        #[arg(long, default_value = "california")]
        region: String,
        /// The most recent fires assessed in each window.
        #[arg(long, default_value_t = 400)]
        limit: usize,
        #[command(flatten)]
        server: Server,
    },
    /// The app: its API, its page, the rules every minute.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7790")]
        listen: String,
        /// The token the command center sends (chloedb-server --app
        /// earth=URL --app-token ...).
        #[arg(long, env = "CHLOEDB_EARTH_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Serve the page and its reads to anyone, without the token.
        #[arg(long)]
        public: bool,
        /// Requests a minute from one address, when public.
        #[arg(long, default_value_t = 120)]
        per_minute: usize,
        /// Requests at once, when public.
        #[arg(long, default_value_t = 8)]
        at_once: usize,
        /// Do not run the alert rules (another process does).
        #[arg(long)]
        no_rules: bool,
        /// The places' yearly pictures (tools/years.py fetch's chips).
        #[arg(long, env = "CHLOEDB_EARTH_CHIPS")]
        chips: Option<PathBuf>,
        #[command(flatten)]
        server: Server,
    },
}

/// A time: a date, a date and time, or hours back from now (`6h`).
fn when(s: &str) -> Result<i64> {
    let s = s.trim();
    if let Some(h) = s.strip_suffix('h') {
        let h: f64 = h.parse()?;
        return Ok(chrono::Utc::now().timestamp() - (h * 3600.0) as i64);
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(d.timestamp());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).map_or(0, |t| t.and_utc().timestamp()));
    }
    bail!("{s}: a time is 2025-01-07, 2025-01-07T17:00:00Z or 6h")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).with_target(false).init();
    match Cli::parse().command {
        Command::Load { dir, only, chunk_mb, server } => {
            let db = server.db();
            let t0 = std::time::Instant::now();
            let n = load::run(&db, &dir, &only, chunk_mb).await?;
            tracing::info!(rows = n, secs = t0.elapsed().as_secs(), "loaded");
        }
        Command::Goes { region, from, to, every, live, parallel, server } => {
            let Some(region) = feed::region(&region) else { bail!("the regions are california and chile") };
            let db = server.db();
            load::create(&db).await?;
            let places = Arc::new(Places::read(&db).await.map_err(|e| anyhow::anyhow!("{e}"))?);
            if places.is_empty() {
                tracing::warn!("no places loaded: fires will have no town, ground or people (run load first)");
            }
            let o = feed::Opts { region, from: from.as_deref().map(when).transpose()?, to: to.as_deref().map(when).transpose()?, live, every, parallel };
            feed::run(db, places, o).await?;
        }
        Command::Rules { server } => {
            let db = server.db();
            load::create(&db).await?;
            let v = rules::run_once(&db).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Command::Backtest { region, limit, server } => {
            let db = server.db();
            load::create(&db).await?;
            let v = analogs::backtest(&db, &region, limit).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Command::Serve { listen, token, public, per_minute, at_once, no_rules, chips, server } => {
            let db = server.db();
            if let Err(e) = load::create(&db).await {
                tracing::warn!(error = %e, "tables");
            }
            api::serve(api::Opts { listen, strata: server.strata, key: server.key, token, public, per_minute, at_once, rules: !no_rules, chips }).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_read() {
        assert_eq!(when("2025-01-07").unwrap(), 1_736_208_000);
        assert_eq!(when("2025-01-07T17:00:00Z").unwrap(), 1_736_269_200);
        assert!(when("6h").unwrap() < chrono::Utc::now().timestamp());
        assert!(when("yesterday").is_err());
    }
}
