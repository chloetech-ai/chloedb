//! Records the commit the binary is built from (`CHLOECTX_BUILD_COMMIT`), so
//! that `chloectx doctor` and `chloectx upgrade` can say how far behind the
//! checkout's main branch it is. A build outside a git checkout (a source
//! tarball, an image built without `.git`) records nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let repo = manifest.parent().map(Path::to_path_buf).unwrap_or_default();
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(&repo).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    match git(&["rev-parse", "HEAD"]) {
        Some(commit) => {
            println!("cargo:rustc-env=CHLOECTX_BUILD_COMMIT={commit}");
            println!("cargo:rustc-env=CHLOECTX_VERSION_LINE={version} (built from {})", &commit[..commit.len().min(10)]);
        }
        None => println!("cargo:rustc-env=CHLOECTX_VERSION_LINE={version}"),
    }
    // Built again when the checkout moves to another commit: HEAD names the
    // branch, the branch's ref (or packed-refs) names the commit.
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let dir = PathBuf::from(dir);
        let common = git(&["rev-parse", "--git-common-dir"]).map(|c| if Path::new(&c).is_absolute() { PathBuf::from(c) } else { repo.join(c) }).unwrap_or_else(|| dir.clone());
        let mut watch = vec![dir.join("HEAD"), common.join("packed-refs")];
        if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
            watch.push(common.join(r));
        }
        // A file that does not exist would run this script on every build.
        for f in watch.into_iter().filter(|f| f.is_file()) {
            println!("cargo:rerun-if-changed={}", f.display());
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
