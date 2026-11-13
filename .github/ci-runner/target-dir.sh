#!/usr/bin/env bash
# On the on-demand runner (CI_RUNNER_CACHE, set by boot.sh), builds go to a
# folder on its volume, outside the checkout, which actions/checkout cleans:
# from one run to the next cargo rebuilds only the crates whose files
# changed. Elsewhere this does nothing, and rust-cache restores the
# dependencies as before. Usage: target-dir.sh <name of the build>
set -euo pipefail
if [ -n "${CI_RUNNER_CACHE:-}" ]; then
  dir="$CI_RUNNER_CACHE/target/$1"
  mkdir -p "$dir"
  echo "CARGO_TARGET_DIR=$dir" >> "$GITHUB_ENV"
  echo "building in $dir ($(df -h --output=avail "$dir" | tail -1 | tr -d ' ') free)"
fi
