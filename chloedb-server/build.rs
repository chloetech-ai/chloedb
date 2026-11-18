// Compiles proto/strata.proto, and the OpenTelemetry logs protocol's files
// in otlp/ (the collector's LogsService, `crate::otlp`), with protox, so
// no protoc is needed. Records the commit the binary is built from
// (`CHLOEDB_BUILD_COMMIT`, `crate::BUILD_COMMIT`), so that two builds of
// one version tell themselves apart; a build outside a git checkout (a
// source tarball, an image built without `.git`) records none.
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/strata.proto");
    println!("cargo:rerun-if-changed=otlp");
    println!("cargo:rerun-if-changed=build.rs");
    commit();
    let fds = protox::compile(["proto/strata.proto"], ["proto"])?;
    tonic_build::configure().compile_fds(fds)?;
    let fds = protox::compile(["opentelemetry/proto/collector/logs/v1/logs_service.proto"], ["otlp"])?;
    tonic_build::configure().compile_fds(fds)?;
    Ok(())
}

fn commit() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let repo = manifest.parent().map(Path::to_path_buf).unwrap_or_default();
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(&repo).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    match git(&["rev-parse", "HEAD"]) {
        Some(commit) => {
            let short = &commit[..commit.len().min(10)];
            println!("cargo:rustc-env=CHLOEDB_BUILD_COMMIT={short}");
            println!("cargo:rustc-env=CHLOEDB_VERSION_LINE={version} (built from {short})");
        }
        None => println!("cargo:rustc-env=CHLOEDB_VERSION_LINE={version}"),
    }
    // Run again when the checkout moves to another commit: HEAD names the
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
}
