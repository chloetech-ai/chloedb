//! The traffic simulator: `plan` makes a schedule of requests from a
//! profile (the distributions of a workload: its query kinds, queries,
//! documents, filters, sessions, traffic and writes) and a seed; `replay`
//! sends a schedule to an engine as an open loop. See
//! deploy/compare/README.md, "Traffic simulator".

pub mod dist;
pub mod gen;
pub mod profile;
pub mod schedule;

use std::path::Path;

use clap::ValueEnum;

use profile::Profile;
use schedule::Shape;

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub enum ShapeArg {
    /// The profile's day (`--rate`: requests a second at its peak).
    Day,
    /// The profile's ramp of rising rates, to find the most the SLO holds at.
    Ramp,
    /// run's closed loop: each kind's requests in turn, at each concurrency.
    Closed,
}

pub fn shape(arg: ShapeArg, rate: Option<f64>) -> Result<Shape, String> {
    match arg {
        ShapeArg::Day => match rate {
            Some(r) if r > 0.0 => Ok(Shape::Day { rate: r }),
            _ => Err("--shape day needs --rate, the requests a second at the day's peak".into()),
        },
        ShapeArg::Ramp => Ok(Shape::Ramp),
        ShapeArg::Closed => Ok(Shape::Closed),
    }
}

/// Reads a profile file: the profile and the SHA-256 of its text.
pub fn read_profile(path: &Path) -> Result<(Profile, String), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p = Profile::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((p, schedule::sha256_hex(text.as_bytes())))
}

/// `engine-compare plan`.
pub fn plan(collection: &str, profile: &Path, seed: u64, shape: Shape, out: &Path) -> Result<gen::Plan, String> {
    let (p, sha) = read_profile(profile)?;
    let t0 = std::time::Instant::now();
    let catalog = gen::Catalog::load(collection)?;
    if let Some(dir) = out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    let plan = gen::plan(&p, &sha, &catalog, seed, shape, out)?;
    eprint!("{}", gen::summary(&plan));
    eprintln!("wrote {} in {:.1} s", out.display(), t0.elapsed().as_secs_f64());
    Ok(plan)
}

#[cfg(test)]
pub mod fixture;
#[cfg(test)]
mod tests;
