//! chloedb-backbone: the internet's backbone on ChloeDB.
//!
//! `fetch` downloads the public sources into a folder; `load` puts their
//! records into a server's tables; `feed` reads RIPE RIS's BGP updates
//! (its archive from a time on, then RIS Live) and keeps Atlas and IODA
//! current; `serve` is the app (its API and its page), which runs the alert
//! rules every minute. Everything read is public.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use chloedb_backbone::{api, feed, fetch, load, refdata::Reference, rules, strata::{RowExt, Strata}};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chloedb-backbone", version, about = "The internet's backbone on ChloeDB")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download the public sources into --dir.
    Fetch {
        #[arg(long)]
        dir: PathBuf,
        /// Only these: rpki, caida, ripe, pdb, atlas, ioda.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Atlas measurements whose latest round to fetch (1001: pings to
        /// the K root server from every probe; 5001: traceroutes to it).
        #[arg(long, value_delimiter = ',', default_value = "1001,5001")]
        atlas: Vec<u64>,
    },
    /// Make the tables, and load the records `fetch` downloaded.
    Load {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
        strata: String,
        #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
        key: Option<String>,
    },
    /// Read RIS's updates into bb_updates: the archive from --from, then
    /// (with --live) RIS Live, beside Atlas and IODA every five minutes.
    Feed {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, value_delimiter = ',', default_value = "rrc00")]
        collectors: Vec<String>,
        /// The archive from this time (2026-11-17T00:00:00Z, or a number of
        /// hours back such as 3h).
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        live: bool,
        /// Start the archive after the newest update already kept (its next
        /// five-minute file), else at --from: a restart fills its gap.
        #[arg(long)]
        resume: bool,
        /// Stop after this many hours, so a supervisor starts it again
        /// with the day's RPKI, origins and names (fetched before it).
        #[arg(long)]
        hours: Option<f64>,
        #[arg(long, value_delimiter = ',', default_value = "1001,5001")]
        atlas: Vec<u64>,
        #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
        strata: String,
        #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
        key: Option<String>,
    },
    /// Run every alert rule once, and print what they found.
    Rules {
        #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
        strata: String,
        #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
        key: Option<String>,
    },
    /// What an announcement means: `check --dir D 84.255.184.0/24 6453`.
    Check {
        #[arg(long)]
        dir: PathBuf,
        prefix: String,
        origin: u32,
    },
    /// Serve the API and the page, and run the rules every minute.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7890")]
        listen: String,
        #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
        strata: String,
        #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
        key: Option<String>,
        /// The token each request must carry (Authorization: Bearer):
        /// chloedb-server sends its CHLOEDB_APP_TOKEN. Without one, anyone
        /// who reaches the address may use the app, and `/` serves the page.
        #[arg(long, env = "CHLOEDB_BACKBONE_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Serve the page at / to anyone, and its reads without the token.
        #[arg(long)]
        public: bool,
        #[arg(long, default_value_t = 600)]
        per_minute: usize,
        #[arg(long, default_value_t = 8)]
        at_once: usize,
        /// Don't run the rules (another process does).
        #[arg(long)]
        no_rules: bool,
    },
}

fn when(s: &str) -> Result<i64> {
    if let Some(h) = s.strip_suffix('h') {
        return Ok(chrono::Utc::now().timestamp() - h.parse::<i64>()? * 3600);
    }
    match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(d) => Ok(d.timestamp()),
        Err(_) => bail!("a time is 2026-11-17T00:00:00Z or 3h (hours back)"),
    }
}

fn when_iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.to_rfc3339()).unwrap_or_default()
}

fn logs() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Command::Fetch { dir, only, atlas } = cli.command {
        return fetch::run(fetch::Opts { dir, only, atlas });
    }
    logs();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        match cli.command {
            Command::Fetch { .. } => unreachable!(),
            Command::Load { dir, only, strata, key } => {
                let db = Strata::new(&strata, key);
                let r = tokio::task::spawn_blocking({
                    let dir = dir.clone();
                    move || Reference::load(&dir)
                })
                .await??;
                load::run(&db, load::Opts { dir: &dir, only }, &r).await
            }
            Command::Feed { dir, collectors, from, to, live, resume, hours, atlas, strata, key } => {
                for c in &collectors {
                    if !feed::COLLECTORS.contains(&c.as_str()) {
                        bail!("{c}: not a RIS collector ({})", feed::COLLECTORS.join(", "));
                    }
                }
                let db = Strata::new(&strata, key);
                load::create(&db).await?;
                let r = tokio::task::spawn_blocking({
                    let dir = dir.clone();
                    move || Reference::load(&dir)
                })
                .await??;
                tracing::info!(roas = r.roas.len(), usual = r.usual.len(), names = r.names.len(), "reference loaded");
                let mut from = from.as_deref().map(when).transpose()?;
                if resume {
                    let rows = db.query("SELECT MAX(ts) AS t FROM bb_updates WHERE collector IN (:c)", serde_json::json!({ "c": collectors })).await?;
                    let newest = rows.first().map(|r| r.s("t")).and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok()).map(|t| t.timestamp());
                    if let Some(t) = newest {
                        // The next file: the one holding `t` was read already,
                        // and RIS Live's rows have keys of their own.
                        from = Some((t / 300 + 1) * 300);
                        tracing::info!(from = %when_iso(from.unwrap_or(0)), "resuming after the newest update kept");
                    }
                }
                let to = to.as_deref().map(when).transpose()?;
                let run = feed::run(db, Arc::new(r), feed::Opts { dir, collectors, from, to, live, atlas });
                match hours {
                    Some(h) => match tokio::time::timeout(std::time::Duration::from_secs_f64(h * 3600.0), run).await {
                        Ok(r) => r,
                        Err(_) => {
                            tracing::info!(hours = h, "stopping to start again with the day's reference data");
                            Ok(())
                        }
                    },
                    None => run.await,
                }
            }
            Command::Rules { strata, key } => {
                let db = Strata::new(&strata, key);
                let names = api::names(&db).await;
                let v = rules::run_once(&db, &names).await?;
                println!("{}", serde_json::to_string_pretty(&v)?);
                Ok(())
            }
            Command::Check { dir, prefix, origin } => {
                let r = Reference::load(&dir)?;
                let Some(p) = chloedb_backbone::mrt::Prefix::parse(&prefix) else { bail!("{prefix}: not a prefix") };
                let c = r.check(&p, origin);
                println!("{p} from AS{origin} ({}): rpki {}, seen {}, owner AS{} ({})", r.name(origin), c.rpki, c.seen, c.owner, r.name(c.owner));
                for (q, v) in r.roas.covering(&p) {
                    println!("  ROA {q}: {v:?}");
                }
                for (q, v) in r.usual.covering(&p) {
                    println!("  usual {q}: {v:?}");
                }
                Ok(())
            }
            Command::Serve { listen, strata, key, token, public, per_minute, at_once, no_rules } => {
                api::serve(api::Opts { listen, strata, key, token, public, per_minute, at_once, rules: !no_rules }).await
            }
        }
    })
}
