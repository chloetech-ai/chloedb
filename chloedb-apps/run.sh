#!/usr/bin/env bash
# Starts chloedb-apps: finds (or builds) chloedb-server, installs the engine's
# one dependency, and serves the studio on http://127.0.0.1:7730/.
#
#   ./run.sh                 # data in chloedb-apps/data
#   ./run.sh --port 8080     # other options go to engine/server.mjs
#   STRATA_APPS_DATA=~/apps-data ./run.sh
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(cd .. && pwd)"

need() { command -v "$1" >/dev/null 2>&1 || { echo "chloedb-apps needs $1: $2" >&2; exit 1; }; }
need node "install Node.js 20 or later (https://nodejs.org)"
major="$(node -p 'process.versions.node.split(".")[0]')"
[ "$major" -ge 20 ] || { echo "chloedb-apps needs Node.js 20 or later (this is $(node -v))" >&2; exit 1; }
need claude "install Claude Code (https://claude.com/claude-code) and sign in with 'claude', or set STRATA_APPS_CLAUDE"

if [ -z "${STRATA_SERVER_BIN:-}" ]; then
  for c in "$REPO/chloedb-server/target/release/chloedb-server" "$REPO/chloedb-server/target/ci/chloedb-server"; do
    if [ -x "$c" ]; then STRATA_SERVER_BIN="$c"; break; fi
  done
fi
if [ -z "${STRATA_SERVER_BIN:-}" ] && command -v chloedb-server >/dev/null 2>&1; then STRATA_SERVER_BIN="$(command -v chloedb-server)"; fi
if [ -z "${STRATA_SERVER_BIN:-}" ]; then
  need cargo "install Rust (https://rustup.rs) to build chloedb-server, or set STRATA_SERVER_BIN to a chloedb-server binary"
  echo "Building chloedb-server (a release build, several minutes the first time)..."
  (cd "$REPO/chloedb-server" && cargo build --release)
  STRATA_SERVER_BIN="$REPO/chloedb-server/target/release/chloedb-server"
fi
export STRATA_SERVER_BIN

[ -d node_modules/quickjs-emscripten ] || npm install --no-audit --no-fund

exec node engine/server.mjs "$@"
