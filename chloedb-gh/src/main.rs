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

mod archive;
mod prepare;
mod reduce;
mod slots;

use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "chloedb-gh", version, about = "GitHub's public history on ChloeDB")]
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
        Command::Prepare { from, to, shards, out, jobs, logins, names, old_ids_below, base_url, part, local } => {
            let Some((i, n)) = part.split_once('/').and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?))) else {
                bail!("--part is i/n, such as 0/2");
            };
            prepare::run(prepare::Opts { from, to, shards, out, jobs, logins, names, old_ids_below, base_url, part: (i, n), local })
        }
        Command::Reduce { inputs, shard, out, min_stars } => reduce::run(reduce::Opts { inputs, shard, out, min_stars }),
        Command::ShardOf { shards, keys } => {
            for k in keys {
                println!("{k}\tslot {}\tshard {}", slots::slot(k), slots::shard(k, shards));
            }
            Ok(())
        }
    }
}
