#!/bin/sh
# Builds ChloeCtx from this checkout, installs the binary, downloads the
# embedding model and wires ChloeCtx into Claude Code. Run it again after
# `git pull` to upgrade. Non-interactive; exits non-zero when a step fails or
# when `chloectx doctor` reports a failed check. Agents: see AGENTS.md.
#
#   ./install.sh                         every Claude Code session on this machine
#   ./install.sh --project ~/code/app    one project only
#   ./install.sh --workbench             ChloeCtx and Workbench (chloedb-workbench), which
#                                        wires ChloeCtx into each session it starts itself
#
# Options:
#   --project DIR          install for one project instead of every session
#   --distill-model NAME   model that writes down decisions after each turn
#                          (default: haiku; "" uses the Claude Code CLI's default)
#   --prefix DIR           where the binary goes (default: ~/.local, so ~/.local/bin/chloectx;
#                          ~/.local/bin/engram, its name before the rename, is a link to it)
#   --no-wire              build and install the binary only; leave Claude Code alone
#   --workbench            also build and install chloedb-workbench; Claude Code's
#                          settings are left alone (add --user-too to wire them as well)
set -eu

PREFIX="$HOME/.local"
PROJECT=""
DISTILL="haiku"
WIRE=1
WORKBENCH=0
USER_TOO=0
while [ $# -gt 0 ]; do
  case "$1" in
    --project) PROJECT="$2"; shift 2 ;;
    --distill-model) DISTILL="$2"; shift 2 ;;
    --prefix) PREFIX="$2"; shift 2 ;;
    --no-wire) WIRE=0; shift ;;
    --workbench) WORKBENCH=1; shift ;;
    --user-too) USER_TOO=1; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "install.sh: unknown option $1 (see --help)" >&2; exit 2 ;;
  esac
done

say() { printf '==> %s\n' "$*"; }
# Workbench wires each session it starts; the user's settings stay as they
# are unless --user-too asks for every other session too.
if [ "$WORKBENCH" = 1 ] && [ "$USER_TOO" = 0 ] && [ -z "$PROJECT" ]; then WIRE=0; fi
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

ROOT="$(cd "$(dirname "$0")" && pwd)"
[ -f "$ROOT/chloectx/Cargo.toml" ] || die "run this from a checkout of https://github.com/chloetech-ai/chloedb"

case "$(uname -s)" in
  Darwin|Linux) ;;
  *) die "ChloeCtx needs macOS or Linux (on Windows, use WSL)" ;;
esac

# Rust, a C compiler (for the tokenizer's regex library) and curl.
if ! command -v cargo >/dev/null 2>&1; then
  [ -x "$HOME/.cargo/bin/cargo" ] && PATH="$HOME/.cargo/bin:$PATH"
fi
command -v cargo >/dev/null 2>&1 || die "cargo not found; install Rust from https://rustup.rs, then run this again"
command -v cc >/dev/null 2>&1 || die "no C compiler (cc); on macOS run xcode-select --install, on Debian/Ubuntu apt install build-essential"
command -v curl >/dev/null 2>&1 || die "curl not found"
if ! command -v claude >/dev/null 2>&1; then
  [ "$WORKBENCH" = 1 ] && die "the Claude Code CLI (claude) is not on PATH; Workbench runs it for each agent: install it from https://claude.com/claude-code"
  [ "$WIRE" = 1 ] && die "the Claude Code CLI (claude) is not on PATH; install it from https://claude.com/claude-code, or pass --no-wire"
fi

say "building ChloeCtx ($(cargo --version))"
# --locked: the versions in chloectx/Cargo.lock (the ones CI tested), not
# whatever newer ones the registry offers today.
(cd "$ROOT/chloectx" && cargo build --locked --release --bin chloectx) ||
  die "the build failed; if cargo says the lock file needs updating, this checkout's chloectx/Cargo.lock is out of date (git pull, or git checkout chloectx/Cargo.lock)"

BIN="$PREFIX/bin/chloectx"
mkdir -p "$PREFIX/bin"
# Replace the file rather than writing into it: running daemons keep the old one.
TARGET="${CARGO_TARGET_DIR:-target}"
case "$TARGET" in /*) ;; *) TARGET="$ROOT/chloectx/$TARGET" ;; esac
cp "$TARGET/release/chloectx" "$BIN.new" && mv -f "$BIN.new" "$BIN"
# ChloeCtx was called engram: for a release, that name runs it too (an old
# binary there is replaced; daemons running it keep it until they stop).
ln -sf chloectx "$PREFIX/bin/engram.new" && mv -f "$PREFIX/bin/engram.new" "$PREFIX/bin/engram" # the name before the rename
say "installed $BIN (and $PREFIX/bin/engram, its name before the rename, as a link to it)"
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) say "note: $PREFIX/bin is not on your PATH; add it to use chloectx from a shell (the hooks use the full path)" ;;
esac

# Daemons from an earlier build pick up the new one when they restart.
"$BIN" stop --all >/dev/null 2>&1 || true

# Where the stores and the model live, as chloectx picks it: a machine that
# has the folder from before the rename (engram) and not the new one keeps
# using it, and nothing is moved.
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ -d "$DATA/engram" ] && [ ! -e "$DATA/chloectx" ]; then HOMEDIR="$DATA/engram"; else HOMEDIR="$DATA/chloectx"; fi
# ENGRAM_* are the settings' names before the rename, which chloectx reads too.
HOMEDIR="${CHLOECTX_HOME:-${ENGRAM_HOME:-$HOMEDIR}}" # before the rename: ENGRAM_HOME
MODELS="${CHLOECTX_MODELS:-${ENGRAM_MODELS:-$HOMEDIR/models}}" # before the rename: ENGRAM_MODELS
# fetch-model keeps the files already there with the pinned bytes and
# downloads the others (a download cut short among them), each checked
# against its SHA-256 before it is moved into place.
if [ -f "$MODELS/potion-retrieval-32M/model.safetensors" ]; then
  say "checking the embedding model (potion-retrieval-32M)"
else
  say "downloading the embedding model (potion-retrieval-32M, 129 MB)"
fi
"$BIN" fetch-model || die "the embedding model did not download or did not match its pinned SHA-256; run ./install.sh again"

if [ "$WORKBENCH" = 1 ]; then
  say "building Workbench"
  (cd "$ROOT/chloedb-workbench" && cargo build --locked --release --bin chloedb-workbench) ||
    die "the Workbench build failed; if cargo says the lock file needs updating, git pull (or git checkout chloedb-workbench/Cargo.lock)"
  WTARGET="${CARGO_TARGET_DIR:-target}"
  case "$WTARGET" in /*) ;; *) WTARGET="$ROOT/chloedb-workbench/$WTARGET" ;; esac
  WB="$PREFIX/bin/chloedb-workbench"
  cp "$WTARGET/release/chloedb-workbench" "$WB.new" && mv -f "$WB.new" "$WB"
  say "installed $WB"
  # What each session will get: Workbench asks chloectx this at every start.
  "$BIN" wiring --project "$HOME" >/dev/null || die "$BIN wiring failed; this ChloeCtx is too old for Workbench"
  if [ "$WIRE" = 0 ]; then
    claude auth status 2>/dev/null | grep -q '"loggedIn": *true' ||
      say "note: the claude CLI is logged out; ChloeCtx's distiller (claude -p) needs it: run claude auth login"
    cat <<EOF

Next:
  - $WB serve          then open the URL it prints (it has the access token)
    (CHLOEDB_WORKBENCH_CHLOECTX=$BIN if $PREFIX/bin is not on PATH)
  - Each agent Workbench starts gets ChloeCtx's hooks and memory tools in its
    own settings; your Claude Code settings were not changed (--user-too wires
    every other session as well).
  - $BIN ui            browse what ChloeCtx stored, per project and session
EOF
    exit 0
  fi
fi

[ "$WIRE" = 1 ] || { say "done (Claude Code not changed: --no-wire)"; exit 0; }

if [ -n "$PROJECT" ]; then
  say "wiring ChloeCtx into $PROJECT"
  "$BIN" install --project "$PROJECT" ${DISTILL:+--distill-model "$DISTILL"}
  cd "$PROJECT"
else
  say "wiring ChloeCtx into every Claude Code session"
  "$BIN" install --user ${DISTILL:+--distill-model "$DISTILL"}
fi

say "checking"
STATUS=0
"$BIN" doctor || STATUS=$?

cat <<EOF

Next:
  - Start Claude Code (claude) in a project and work as usual. Sessions that
    were already running pick ChloeCtx up when they restart.
  - $BIN ui            browse what ChloeCtx stored, per project and session
  - $BIN doctor        check the setup again
  - $BIN uninstall --user   (or: uninstall in the project) to remove it; stores are kept
EOF
[ "$STATUS" = 0 ] || say "chloectx doctor reported a problem above; fix it and run $BIN doctor again"
exit "$STATUS"
