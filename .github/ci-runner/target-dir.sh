#!/usr/bin/env bash
# On the on-demand runner (CI_RUNNER_CACHE, set by boot.sh), builds go to a
# folder on its volume, outside the checkout, which actions/checkout cleans:
# from one run to the next cargo rebuilds only the crates whose files
# changed. Elsewhere this does nothing, and rust-cache restores the
# dependencies as before. Usage: target-dir.sh <name of the build>
set -euo pipefail
if [ -n "${CI_RUNNER_CACHE:-}" ]; then
  dir="$CI_RUNNER_CACHE/target/$1"
  # cargo never removes what a change of version or dependency made
  # stale, so the folders grow until the volume is full (11-14: a link
  # died of "No space left on device" on the 0.3.3 bump). Under 40 GB
  # free, this job's folder starts again from nothing; the other
  # runner's, which may be building, is left alone.
  free_gb=$(( $(df --output=avail -k "$CI_RUNNER_CACHE" | tail -1) / 1048576 ))
  if [ "$free_gb" -lt 40 ] && [ -d "$dir" ]; then
    echo "only ${free_gb} GB free on the volume: removing $dir"
    rm -rf "$dir"
  fi
  mkdir -p "$dir"
  echo "CARGO_TARGET_DIR=$dir" >> "$GITHUB_ENV"
  echo "building in $dir ($(df -h --output=avail "$dir" | tail -1 | tr -d ' ') free)"
fi
