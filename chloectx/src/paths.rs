//! Where a project's store, socket and logs live.

use std::path::{Path, PathBuf};

/// Where the stores and the models live: `$CHLOECTX_HOME` (which the
/// programs set from `$ENGRAM_HOME`, its name before the rename), else
/// `chloectx` in the data folder (`$XDG_DATA_HOME`, else `~/.local/share`).
///
/// ChloeCtx was called Engram, and kept its stores in `<data>/engram`. A
/// machine that has that folder and no `<data>/chloectx` keeps using it, so
/// an upgrade moves nothing; [`OLD_HOME`] names it.
pub fn home() -> PathBuf {
    if let Some(d) = std::env::var_os("CHLOECTX_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    home_in(&data_dir())
}

/// The folder [`home`] picks in a data folder.
pub fn home_in(data: &Path) -> PathBuf {
    let new = data.join(HOME);
    let old = data.join(OLD_HOME);
    if !new.exists() && old.is_dir() {
        old
    } else {
        new
    }
}

/// The folder's name in the data folder.
pub const HOME: &str = "chloectx";
/// Its name before the rename, which a machine that has it keeps.
pub const OLD_HOME: &str = "engram";

/// `$XDG_DATA_HOME`, else `~/.local/share`.
fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".local/share")
}

/// Claude Code's user settings: `$CLAUDE_CONFIG_DIR/settings.json`, else
/// `~/.claude/settings.json`.
pub fn user_settings() -> PathBuf {
    if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d).join("settings.json");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".claude/settings.json")
}

/// Claude Code's own state, which holds the user-scope MCP servers:
/// `$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`. ChloeCtx reads
/// it and changes it only through `claude mcp`.
pub fn claude_json() -> PathBuf {
    if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d).join(".claude.json");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".claude.json")
}

/// A short, stable directory name for a project: its last path component
/// plus a hash of the full path (Unix socket paths are limited to 107 bytes).
pub fn project_slug(project: &Path) -> String {
    let abs = project.canonicalize().unwrap_or_else(|_| project.to_path_buf());
    let s = abs.to_string_lossy();
    let base: String = abs
        .file_name()
        .map(|n| n.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).take(32).collect())
        .unwrap_or_else(|| "root".into());
    format!("{base}-{:08x}", crate::embed::fnv(&s) as u32)
}

/// The store directory of a project.
pub fn store_dir(project: &Path) -> PathBuf {
    home().join("projects").join(project_slug(project))
}

/// The project a hook or MCP server runs for: `$CLAUDE_PROJECT_DIR`, else `cwd`.
pub fn project_dir(cwd: Option<&str>) -> PathBuf {
    if let Ok(d) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    cwd.map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub struct StorePaths {
    pub dir: PathBuf,
    pub db: PathBuf,
    pub socket: PathBuf,
    pub spawn_lock: PathBuf,
    pub state: PathBuf,
    pub log: PathBuf,
    pub config: PathBuf,
}

impl StorePaths {
    pub fn new(dir: &Path) -> StorePaths {
        StorePaths {
            dir: dir.to_path_buf(),
            db: dir.join("memory.strata"),
            socket: socket_path(dir),
            spawn_lock: dir.join("spawn.lock"),
            state: dir.join("state.json"),
            log: dir.join("daemon.log"),
            config: dir.join("config.json"),
        }
    }
}

/// `dir/engram.sock`, or a short path in a private runtime directory when
/// that would exceed the Unix socket limit (about 107 bytes). They keep the
/// old name: a daemon started before the rename listens on them.
fn socket_path(dir: &Path) -> PathBuf {
    let p = dir.join("engram.sock");
    if p.as_os_str().len() < 100 {
        return p;
    }
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let base = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(format!("/tmp/engram-{uid}")));
    let _ = std::fs::create_dir_all(&base);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
    }
    base.join(format!("engram-{:016x}.sock", crate::embed::fnv(&dir.to_string_lossy())))
}

/// Creates a store directory readable only by its owner: it holds
/// transcripts, which can contain anything the session saw.
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_with_the_old_folder_keeps_it() {
        let data = tempfile::tempdir().unwrap();
        let d = data.path();
        assert_eq!(home_in(d), d.join("chloectx"), "a new install");
        std::fs::create_dir(d.join("engram")).unwrap();
        assert_eq!(home_in(d), d.join("engram"), "an install from before the rename");
        std::fs::create_dir(d.join("chloectx")).unwrap();
        assert_eq!(home_in(d), d.join("chloectx"), "the new folder wins once it exists");
    }
}
