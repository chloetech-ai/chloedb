//! Worktrees: where a session runs when it has a checkout of its own.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("git {} failed", args.join(" ")) } else { err })
    }
}

/// A checkout, read back to its repository.
#[derive(Clone, Debug)]
pub struct Checkout {
    /// The repository's main checkout.
    pub main: PathBuf,
}

/// The checkout `path` is in, and its repository's main checkout; an error
/// when it is not in a git checkout.
pub fn checkout(path: &Path) -> Result<Checkout, String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let top = git(path, &["rev-parse", "--show-toplevel"]).map_err(|_| format!("{} is not in a git checkout", path.display()))?;
    let top = PathBuf::from(top);
    let common = git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = PathBuf::from(common);
    let main = match common.file_name().and_then(|n| n.to_str()) {
        Some(".git") => common.parent().map(Path::to_path_buf).unwrap_or_else(|| top.clone()),
        _ => top.clone(),
    };
    let main = main.canonicalize().unwrap_or(main);
    Ok(Checkout { main })
}

/// The branch checked out at `dir`, or `None` on a detached HEAD.
pub fn current_branch(dir: &Path) -> Option<String> {
    git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok().filter(|b| !b.is_empty())
}

/// Whether the local branch `name` exists.
pub fn branch_exists(repo: &Path, name: &str) -> bool {
    git(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{name}")]).is_ok()
}

/// A short, stable hash of a path (FNV-1a), as 8 hex digits.
pub fn hash8(path: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", (h ^ (h >> 32)) as u32)
}

/// The folder the worktrees of `repo` go in.
pub fn worktrees_dir(data_dir: &Path, repo: &Path) -> PathBuf {
    let name = repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
    data_dir.join("worktrees").join(format!("{name}-{}", hash8(repo)))
}

/// Adds a worktree of `repo` at `path` on `branch`: a new branch from
/// `base` (the repository's HEAD when `None`), or the branch as it is when
/// it exists already.
pub fn add_worktree(repo: &Path, path: &Path, branch: &str, base: Option<&str>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let p = path.to_string_lossy().to_string();
    if branch_exists(repo, branch) {
        git(repo, &["worktree", "add", &p, branch])?;
    } else {
        let base = base.unwrap_or("HEAD");
        git(repo, &["worktree", "add", &p, "-b", branch, base])?;
    }
    Ok(())
}
