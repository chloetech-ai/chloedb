#!/usr/bin/env bash
# Starts strata-apps: finds (or builds) strata-server, installs the engine's
# one dependency, and serves the studio on http://127.0.0.1:7730/.
#
#   ./run.sh                 # data in strata-apps/data
#   ./run.sh --port 8080     # other options go to engine/server.mjs
#   STRATA_APPS_DATA=~/apps-data ./run.sh
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(cd .. && pwd)"

need() { command -v "$1" >/dev/null 2>&1 || { echo "strata-apps needs $1: $2" >&2; exit 1; }; }
need node "install Node.js 20 or later (https://nodejs.org)"
major="$(node -p 'process.versions.node.split(".")[0]')"
[ "$major" -ge 20 ] || { echo "strata-apps needs Node.js 20 or later (this is $(node -v))" >&2; exit 1; }
need claude "install Claude Code (https://claude.com/claude-code) and sign in with 'claude', or set STRATA_APPS_CLAUDE"

if [ -z "${STRATA_SERVER_BIN:-}" ]; then
  for c in "$REPO/strata-server/target/release/strata-server" "$REPO/strata-server/target/ci/strata-server"; do
    if [ -x "$c" ]; then STRATA_SERVER_BIN="$c"; break; fi
  done
fi
if [ -z "${STRATA_SERVER_BIN:-}" ] && command -v strata-server >/dev/null 2>&1; then STRATA_SERVER_BIN="$(command -v strata-server)"; fi
if [ -z "${STRATA_SERVER_BIN:-}" ]; then
  need cargo "install Rust (https://rustup.rs) to build strata-server, or set STRATA_SERVER_BIN to a strata-server binary"
  echo "Building strata-server (a release build, several minutes the first time)..."
  (cd "$REPO/strata-server" && cargo build --release)
  STRATA_SERVER_BIN="$REPO/strata-server/target/release/strata-server"
fi
export STRATA_SERVER_BIN

[ -d node_modules/quickjs-emscripten ] || npm install --no-audit --no-fund

exec node engine/server.mjs "$@"
