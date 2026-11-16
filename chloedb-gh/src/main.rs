//! chloedb-gh: GitHub's public history on ChloeDB.
//!
//! `prepare` reads GH Archive (https://www.gharchive.org, every public
//! event since 2011, an hourly file each) and writes Parquet rows split by
//! the shard each row belongs to, which `chloedb-server import` loads into
//! each shard's database before the cluster starts. `shard-of` says which
//! shard a key goes to, to check the split against a running cluster.
//!
//! Only public data is read, and no event's email or name fields are kept:
//! the old format's `actor_attributes` and commits' authors are never
//! parsed (see `archive`).
//!
//! `serve` is the app: its API and the view the command center shows
//! (chloedb-server --app gh=URL), over the cluster the rows went into.

use chloedb_gh::{api, apimeta, prepare, reduce, slots, strata::Strata};

use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "chloedb-gh",
    version,
    about = "GitHub's public history on ChloeDB"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// GH Archive's hours from --from to --to (days, inclusive) into
    /// Parquet files under --out: TABLE/sSHARD/DAY.parquet. Days already
    /// done (out/done/DAY.json) are skipped, so a run started again goes on.
    Prepare {
        #[arg(long)]
        from: NaiveDate,
        #[arg(long)]
        to: NaiveDate,
        #[arg(long, default_value_t = 4)]
        shards: usize,
        #[arg(long)]
        out: PathBuf,
        /// Days worked on at once.
        #[arg(long, default_value_t = 8)]
        jobs: usize,
        /// Before 2015: `actors` files of a run over 2015 on, to give each
        /// login its id.
        #[arg(long)]
        logins: Option<PathBuf>,
        /// Before repo ids (until 2012-09 or so): `names` files of a run
        /// over later days, to give each "owner/name" its id.
        #[arg(long)]
        names: Option<PathBuf>,
        /// --logins and --names look only at ids below this: the accounts
        /// and repos of the years without ids.
        #[arg(long, default_value_t = 20_000_000)]
        old_ids_below: i64,
        #[arg(long, default_value = "https://data.gharchive.org")]
        base_url: String,
        /// This machine's share of the days, `i/n`: every n-th day from i.
        #[arg(long, default_value = "0/1")]
        part: String,
        /// Read the hours from this folder rather than download them.
        #[arg(long)]
        local: Option<PathBuf>,
    },
    /// One shard's rows from `prepare` folders into the rows of its keyed
    /// tables, one a key: OUT/repos/sSHARD.parquet, repo_names, actors.
    Reduce {
        /// The `prepare` folders (one a machine).
        #[arg(long = "input", required = true)]
        inputs: Vec<PathBuf>,
        #[arg(long)]
        shard: usize,
        #[arg(long)]
        out: PathBuf,
        /// Keep repos with at least this many stars in the archive.
        #[arg(long, default_value_t = 2)]
        min_stars: i64,
        /// Give repos with at least this many stars the text their vector
        /// is made of (`etext`); the others have no vector, which keeps a
        /// shard's vectors and graph within an 8 GB machine.
        #[arg(long, default_value_t = 10)]
        vec_min_stars: i64,
        #[arg(long, default_value_t = 4)]
        shards: usize,
    },
    /// What GitHub's search API says of each repo with --min-stars or more
    /// today (stars, description, language, topics, license, name), as
    /// `meta` and `names` rows under --out that `reduce` reads after the
    /// days'. At most 10 calls a minute without a token (GITHUB_TOKEN).
    ApiMeta {
        #[arg(long)]
        out: PathBuf,
        /// This machine's share of the bands of stars, `i/n`.
        #[arg(long, default_value = "0/1")]
        part: String,
        #[arg(long, default_value_t = 4)]
        shards: usize,
        #[arg(long, default_value_t = 50)]
        min_stars: i64,
        /// The repos made since this day first (GH Archive has no
        /// description, language or topics for them).
        #[arg(long, default_value = "2025-10-01")]
        since: chrono::NaiveDate,
        /// Seconds between calls: 7 keeps under 10 a minute; 2.1 under 30 with a token.
        #[arg(long)]
        gap: Option<f64>,
        #[arg(long, default_value = "https://api.github.com")]
        base_url: String,
    },
    /// Serve the API and the view for the command center (chloedb-server
    /// --app gh=URL). Every request needs the token but /healthz, unless
    /// --public.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7880")]
        listen: String,
        /// The cluster's coordinator (or one server) that keeps the tables.
        #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
        strata: String,
        #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
        key: Option<String>,
        /// The token each request must carry (Authorization: Bearer):
        /// chloedb-server sends its CHLOEDB_APP_TOKEN. Without one, anyone
        /// who reaches the address may use the app, and `/` serves the page.
        #[arg(long, env = "CHLOEDB_GH_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Serve the page at / to anyone, and its reads without the token.
        #[arg(long)]
        public: bool,
        /// Requests a minute one address may make without the token.
        #[arg(long, default_value_t = 600)]
        per_minute: usize,
        /// Requests without the token answered at once; the rest wait.
        #[arg(long, default_value_t = 8)]
        at_once: usize,
        /// The Hacker News app on ChloeDB (chloedb-hn), to say when a repo
        /// was discussed there.
        #[arg(long, env = "CHLOEDB_GH_HN")]
        hn: Option<String>,
    },
    /// The slot and shard of INT keys.
    ShardOf {
        #[arg(long, default_value_t = 4)]
        shards: usize,
        keys: Vec<i64>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Prepare {
            from,
            to,
            shards,
            out,
            jobs,
            logins,
            names,
            old_ids_below,
            base_url,
            part,
            local,
        } => {
            let Some((i, n)) = part
                .split_once('/')
                .and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?)))
            else {
                bail!("--part is i/n, such as 0/2");
            };
            prepare::run(prepare::Opts {
                from,
                to,
                shards,
                out,
                jobs,
                logins,
                names,
                old_ids_below,
                base_url,
                part: (i, n),
                local,
            })
        }
        Command::Reduce {
            inputs,
            shard,
            out,
            min_stars,
            vec_min_stars,
            shards,
        } => reduce::run(reduce::Opts {
            inputs,
            shard,
            out,
            min_stars,
            vec_min_stars,
            shards,
        }),
        Command::ApiMeta {
            out,
            part,
            shards,
            min_stars,
            since,
            gap,
            base_url,
        } => {
            let (i, n) = part
                .split_once('/')
                .and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?)))
                .ok_or_else(|| anyhow::anyhow!("--part is i/n"))?;
            let token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty());
            let gap = std::time::Duration::from_secs_f64(gap.unwrap_or(if token.is_some() {
                2.1
            } else {
                7.0
            }));
            apimeta::run(apimeta::Opts {
                out,
                part: (i, n),
                shards,
                min_stars,
                since,
                gap,
                token,
                base_url,
            })
        }
        Command::Serve {
            listen,
            strata,
            key,
            token,
            public,
            per_minute,
            at_once,
            hn,
        } => serve(listen, strata, key, token, Public { on: public, per_minute, at_once }, hn),
        Command::ShardOf { shards, keys } => {
            for k in keys {
                println!(
                    "{k}\tslot {}\tshard {}",
                    slots::slot(k),
                    slots::shard(k, shards)
                );
            }
            Ok(())
        }
    }
}

/// Who may read without the token, and how much.
struct Public {
    on: bool,
    per_minute: usize,
    at_once: usize,
}

fn serve(
    listen: String,
    url: String,
    key: Option<String>,
    token: Option<String>,
    public: Public,
    hn: Option<String>,
) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen).await.map_err(|e| anyhow::anyhow!("{listen}: {e}"))?;
        let svc = std::sync::Arc::new(api::Service {
            db: Strata::new(&url, key.filter(|k| !k.is_empty())),
            token: token.filter(|t| !t.is_empty()),
            public: public.on,
            per_minute: public.per_minute.max(1),
            seen: std::sync::Mutex::new(Default::default()),
            turns: tokio::sync::Semaphore::new(public.at_once.max(1)),
            hn: hn.filter(|h| !h.is_empty()).map(|h| api::Hn::new(&h)),
            started: std::time::Instant::now(),
            latest: std::sync::Mutex::new(None),
        });
        tracing::info!(listen = %listener.local_addr()?, strata = %url, token = svc.token.is_some(), public = svc.public, hn = svc.hn.is_some(), "chloedb-gh serves");
        axum::serve(listener, api::router(svc).into_make_service_with_connect_info::<std::net::SocketAddr>()).with_graceful_shutdown(async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a signal handler");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        }).await?;
        Ok(())
    })
}
