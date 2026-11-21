#!/bin/sh
# Install ChloeDB's server, prebuilt, with no Rust and no C compiler:
#
#   curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh
#
# With --with, it installs programs of the same release beside the server:
#
#   curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh -s -- --with crawl,sheets,ctx
#
#   crawl    chloedb-crawl, the crawler (Crawl to search, the crawler app)
#   sheets   chloedb-sheets, the spreadsheet app
#   ctx      chloectx, ChloeCtx: long-term memory for Claude Code (then
#            `chloectx install --project DIR`; see docs/chloectx/setup.md)
#   all      the three
#   netops   chloedb-netops, the network operations app (releases after 0.3.2; not in all)
#
# It downloads the newest chloedb-server release for this machine from
# GitHub (Linux x86_64 and aarch64 with glibc 2.35 or later, macOS arm64 and
# x86_64, macOS 11 or later), checks the archive against the SHA-256 the
# release publishes beside it, and puts `chloedb-server` in ~/.local/bin.
# Where GitHub's CLI is installed (gh, a version with `gh attestation
# verify --source-ref`), it also checks the archive's build provenance
# (releases from 0.3.8): that this repository's release workflow,
# server-release.yml, built it from the release's tag, on GitHub's own
# runners. It reads the Sigstore bundle the release publishes beside the
# archive (<archive>.sigstore.jsonl), so gh needs no sign-in, and an
# archive that does not verify is not installed. Without gh it installs on
# the SHA-256 alone and says so.
# The newest release is asked of GitHub's API, and where that cannot be
# reached (a proxy, or its limit of 60 requests an hour without a token),
# of github.com's /releases/latest and then of this website.
# It needs curl (or wget), tar and sha256sum (or shasum), changes nothing
# outside the install folder, and asks for no password.
#
# Settings (environment variables; with `curl ... | sh`, put them after the
# pipe: `curl ... | CHLOEDB_VERSION=0.2.0 sh`):
#
#   CHLOEDB_VERSION       the version to install (0.2.0); the newest release by default
#   CHLOEDB_WITH          the programs --with names (crawl,sheets,ctx), when not given
#   CHLOEDB_INSTALL_DIR   where to put the program (default ~/.local/bin)
#   CHLOEDB_RELEASES_URL  where the release archives are (default this
#                         repository's GitHub releases; a local folder or
#                         server for testing, laid out as
#                         <url>/chloedb-server-v<version>/chloedb-server-<platform>.tar.gz)
#   CHLOEDB_VERIFY        the provenance check: auto (default: when gh is
#                         here), require (fail without gh) or off (the SHA-256 alone)
#   CHLOEDB_GH            the gh to check it with (default: gh on the PATH)
#   CHLOEDB_VERIFY_SOURCE_REF  the git ref the provenance must name (default
#                         refs/tags/chloedb-server-v<version>; for testing a
#                         run of the release workflow on a branch)
#
# This file is published by GitHub Pages from docs/install.sh of
# https://github.com/chloetech-ai/chloedb. Read it first if you like:
#   curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | less

set -eu

REPO="chloetech-ai/chloedb"
# The website that serves this script, the docs and versions.json.
SITE="https://chloetech-ai.github.io/chloedb"
RELEASES_URL="${CHLOEDB_RELEASES_URL:-https://github.com/$REPO/releases/download}"
INSTALL_DIR="${CHLOEDB_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${CHLOEDB_VERSION:-}"
WITH="${CHLOEDB_WITH:-}"
VERIFY="${CHLOEDB_VERIFY:-auto}"
# The first release whose archives have build provenance beside them.
ATTESTED_FROM=0.3.8

say() { printf '%s\n' "$*"; }
fail() {
  printf 'chloedb install: %s\n' "$*" >&2
  exit 1
}
have() { command -v "$1" > /dev/null 2>&1; }
# The header above, as --help: a heredoc, since with `curl ... | sh` there
# is no file to read it from.
usage() {
  cat << 'USAGE'
Install ChloeDB's server, prebuilt (no Rust, no C compiler), into ~/.local/bin:

  curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh
  curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh -s -- --with crawl,sheets,ctx

Options:
  --with LIST   programs of the same release to install beside the server:
                crawl (chloedb-crawl), sheets (chloedb-sheets),
                ctx (chloectx, memory for Claude Code), or all (the three);
                netops (chloedb-netops, network operations) by name
  -h, --help    this text

Settings (after the pipe: curl ... | CHLOEDB_VERSION=0.3.10 sh):
  CHLOEDB_VERSION       the version to install; the newest release by default
  CHLOEDB_WITH          the programs --with names, when it is not given
  CHLOEDB_INSTALL_DIR   where to put the programs (default ~/.local/bin)
  CHLOEDB_RELEASES_URL  where the release archives are (default GitHub's)
  CHLOEDB_VERIFY        the build provenance check: auto (when gh is here),
                        require (fail without gh) or off

It checks each download against the release's SHA-256 and, where GitHub's
CLI (gh) is installed, against its build provenance (from 0.3.8: GitHub's
attestation that the release workflow built it from the release's tag), and
changes nothing outside the install folder. Platforms: Linux x86_64 and
aarch64 (glibc 2.35 or later), macOS 11 or later on arm64 and x86_64.
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --with) [ $# -gt 1 ] || fail "--with needs a list: crawl,sheets,ctx or all"; WITH="$2"; shift 2 ;;
    --with=*) WITH="${1#--with=}"; shift ;;
    -h | --help) usage; exit 0 ;;
    *) fail "unknown option $1 (--with crawl,sheets,ctx is the one there is)" ;;
  esac
done
# The programs to install beside the server, by their binaries' names.
EXTRA=""
for w in $(printf '%s' "$WITH" | tr ',' ' '); do
  case "$w" in
    crawl | chloedb-crawl) EXTRA="$EXTRA chloedb-crawl" ;;
    sheets | chloedb-sheets) EXTRA="$EXTRA chloedb-sheets" ;;
    ctx | chloectx) EXTRA="$EXTRA chloectx" ;;
    netops | chloedb-netops) EXTRA="$EXTRA chloedb-netops" ;;
    all) EXTRA="$EXTRA chloedb-crawl chloedb-sheets chloectx" ;;
    *) fail "--with $w: the programs are crawl, sheets, ctx (or all), and netops" ;;
  esac
done

# Fetch a URL to stdout (or to a file with a second argument). A path or
# file:// URL is read from disk, for testing against a local release.
fetch() {
  case "$1" in
    /*) if [ $# -gt 1 ]; then cp "$1" "$2"; else cat "$1"; fi; return ;;
    file://*) p=${1#file://}; if [ $# -gt 1 ]; then cp "$p" "$2"; else cat "$p"; fi; return ;;
  esac
  if have curl; then
    if [ $# -gt 1 ]; then curl -fsSL --retry 3 -o "$2" "$1"; else curl -fsSL --retry 3 "$1"; fi
  elif have wget; then
    if [ $# -gt 1 ]; then wget -q -O "$2" "$1"; else wget -q -O - "$1"; fi
  else
    fail "needs curl or wget to download the release"
  fi
}

# --- Which archive this machine runs ---------------------------------------

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  MINGW* | MSYS* | CYGWIN* | Windows_NT)
    fail "Windows has no prebuilt server yet: run this inside WSL 2, or use Docker (docker run ghcr.io/$REPO-server)" ;;
  *) fail "no prebuilt server for $os: build it from source (https://github.com/$REPO#quick-start) or use Docker" ;;
esac
case "$arch" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) if [ "$os" = macos ]; then arch=arm64; else arch=aarch64; fi ;;
  *) fail "no prebuilt server for $os on $arch: build it from source (https://github.com/$REPO#quick-start) or use Docker" ;;
esac
# A Mac with Apple silicon runs a shell under Rosetta as x86_64: take the
# native build there.
if [ "$os" = macos ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2> /dev/null || echo 0)" = 1 ]; then
  arch=arm64
fi
platform="$os-$arch"

# The Linux builds need glibc 2.35 or later (Ubuntu 22.04, Debian 12,
# RHEL 10 and newer); Alpine and other musl systems have none.
if [ "$os" = linux ]; then
  glibc=$( (getconf GNU_LIBC_VERSION 2> /dev/null || true) | sed -n 's/^glibc \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')
  if [ -z "$glibc" ]; then
    fail "this Linux has no glibc (Alpine and other musl systems): use Docker (docker run ghcr.io/$REPO-server) or build from source"
  fi
  major=${glibc%%.*}
  minor=${glibc#*.}
  if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 35 ]; }; then
    fail "the prebuilt server needs glibc 2.35 or later (Ubuntu 22.04, Debian 12), and this machine has $glibc: use Docker (docker run ghcr.io/$REPO-server) or build from source"
  fi
fi

# --- Which version ---------------------------------------------------------

tmp=$(mktemp -d 2> /dev/null || mktemp -d -t chloedb)
trap 'rm -rf "$tmp"' EXIT INT TERM

# The address URL $1 redirects to, reading no page.
redirect() {
  if have curl; then
    curl -fsSI -o /dev/null -w '%{redirect_url}' --retry 3 "$1"
  elif have wget; then
    wget -S --spider --max-redirect=0 "$1" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tr -d '\r' | head -n 1
  else
    fail "needs curl or wget to download the release"
  fi
}

# Runs "$@" (a fetch of URL $1, or the address it redirects to), its
# output in $got; on failure, the line "  URL: what went wrong" is added to
# $tried, for the message that says so.
tried=""
attempt() {
  url=$1
  shift
  if got=$("$@" 2> "$tmp/err") && [ -n "$got" ]; then
    return 0
  fi
  why=$(tail -n 1 "$tmp/err" | sed 's/^curl: ([0-9]*) //')
  tried="$tried
  $url: ${why:-no answer}"
  got=""
  return 1
}

# The command that installs version $1 as this one was asked to, to show.
again() {
  line="curl -fsSL $SITE/install.sh | "
  [ -z "${CHLOEDB_INSTALL_DIR:-}" ] || line="${line}CHLOEDB_INSTALL_DIR=$CHLOEDB_INSTALL_DIR "
  line="${line}CHLOEDB_VERSION=$1 sh"
  [ -z "$WITH" ] || line="$line -s -- --with $WITH"
  printf '%s' "$line"
}

if [ -z "$VERSION" ]; then
  case "$RELEASES_URL" in
    https://github.com/*) ;;
    *) fail "with CHLOEDB_RELEASES_URL, set CHLOEDB_VERSION too" ;;
  esac
  # The repository releases several things (the server, the Python and
  # JavaScript packages), each under its own tag: the newest server tag.
  # GitHub lists releases newest first; drafts are not shown and
  # prereleases are skipped by name (a version with a hyphen).
  api="https://api.github.com/repos/$REPO/releases?per_page=100"
  if attempt "$api" fetch "$api"; then
    VERSION=$(printf '%s\n' "$got" | sed -n 's/.*"tag_name": *"chloedb-server-v\([0-9][0-9.]*\)".*/\1/p' | head -n 1)
    [ -n "$VERSION" ] || tried="$tried
  $api: no chloedb-server release listed"
  fi
  # Without the API (MAYA-1 of the persona study of 17 November 2026: a
  # proxy refused it): the release GitHub marks as the latest, the
  # server's (release-publish.yml marks it so), by the address of its page
  # that /releases/latest redirects to.
  if [ -z "$VERSION" ]; then
    latest="https://github.com/$REPO/releases/latest"
    if attempt "$latest" redirect "$latest"; then
      VERSION=$(printf '%s\n' "$got" | sed -n 's|.*/releases/tag/chloedb-server-v\([0-9][0-9.]*\)$|\1|p')
      [ -n "$VERSION" ] || tried="$tried
  $latest: leads to $got, not a chloedb-server release"
      [ -z "$VERSION" ] || say "The newest release is $VERSION, says $latest (GitHub's API did not answer)"
    fi
  fi
  # Then the website this script comes from: the release its docs describe.
  if [ -z "$VERSION" ]; then
    site="$SITE/site/versions.json"
    if attempt "$site" fetch "$site"; then
      VERSION=$(printf '%s\n' "$got" | sed -n 's/.*"latest": *"\([0-9][0-9.]*\)".*/\1/p' | head -n 1)
      [ -n "$VERSION" ] || tried="$tried
  $site: names no release"
      [ -z "$VERSION" ] || say "The newest release is $VERSION, says $site (GitHub did not answer)"
    fi
  fi
  if [ -z "$VERSION" ]; then
    printf 'chloedb install: could not tell which chloedb-server release is the newest:%s\n' "$tried" >&2
    printf 'Name the version after the pipe (the releases are https://github.com/%s/releases, chloedb-server-v<version>; 0.3.10 was the newest when this script was written):\n' "$REPO" >&2
    printf '  %s\n' "$(again '<version>')" >&2
    exit 1
  fi
fi
VERSION=${VERSION#v}
VERSION=${VERSION#chloedb-server-v}

# --- Download, check, install ----------------------------------------------

if have sha256sum; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif have shasum; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "needs sha256sum or shasum to check the download"
fi
have tar || fail "needs tar to unpack the release"

# Whether version $1 is $2 or later (numbers.numbers.numbers).
at_least() {
  a=$1 b=$2
  for _ in 1 2 3; do
    x=${a%%.*} y=${b%%.*}
    x=${x%%[!0-9]*} y=${y%%[!0-9]*}
    [ "${x:-0}" -gt "${y:-0}" ] && return 0
    [ "${x:-0}" -lt "${y:-0}" ] && return 1
    case "$a" in *.*) a=${a#*.} ;; *) a=0 ;; esac
    case "$b" in *.*) b=${b#*.} ;; *) b=0 ;; esac
  done
  return 0
}

case "$VERIFY" in
  auto | require | off) ;;
  *) fail "CHLOEDB_VERIFY is auto, require or off, not $VERIFY" ;;
esac
# A gh that verifies attestations against a tag (`gh attestation verify
# --source-ref`), or nothing.
GH=
gh=${CHLOEDB_GH:-gh}
if [ "$VERIFY" != off ] && have "$gh" && "$gh" attestation verify --help 2> /dev/null | grep -q -- '--source-ref'; then GH=$gh; fi
[ "$VERIFY" = require ] && [ -z "$GH" ] && fail "CHLOEDB_VERIFY=require, and there is no gh here that verifies attestations (gh attestation verify --source-ref): install a recent gh from https://cli.github.com"

# Checks the archive $1, downloaded from $2, against the build provenance of
# the release: GitHub's attestation, in the Sigstore bundle $2.sigstore.jsonl,
# that server-release.yml of this repository built it from the release's
# tag on a GitHub-hosted runner. Fails when it does not verify; without gh,
# or for a release from before $ATTESTED_FROM (0.3.8), says so and returns.
provenance() {
  [ "$VERIFY" = off ] && return 0
  if ! at_least "$VERSION" "$ATTESTED_FROM"; then
    [ "$VERIFY" = require ] && fail "release $VERSION has no build provenance (releases have it from $ATTESTED_FROM)"
    say "  release $VERSION is older than build provenance ($ATTESTED_FROM): checked by its SHA-256 alone"
    return 0
  fi
  if [ -z "$GH" ]; then
    say "  build provenance not checked: no gh here (with a recent gh, from https://cli.github.com, this checks it; SECURITY.md says how by hand)"
    return 0
  fi
  fetch "$2.sigstore.jsonl" "$1.sigstore.jsonl" 2> /dev/null \
    || fail "could not download $2.sigstore.jsonl, the archive's build provenance: not installing it (CHLOEDB_VERIFY=off installs on the SHA-256 alone)"
  ref=${CHLOEDB_VERIFY_SOURCE_REF:-refs/tags/chloedb-server-v$VERSION}
  if out=$("$GH" attestation verify "$1" --bundle "$1.sigstore.jsonl" --repo "$REPO" \
    --signer-workflow "$REPO/.github/workflows/server-release.yml" --source-ref "$ref" --deny-self-hosted-runners 2>&1); then
    say "  build provenance checked: built by $REPO's server-release.yml from $ref"
  else
    printf '%s\n' "$out" | tail -n 8 >&2
    fail "gh could not verify the build provenance of $2 (above): not installing it. CHLOEDB_VERIFY=off installs on the SHA-256 alone"
  fi
}

# Installs the program $1 from its archive, $1-<platform>.tar.gz, of the
# release chloedb-server-v$VERSION, after checking its SHA-256 and, with gh,
# its build provenance.
install_program() {
  prog=$1
  name="$prog-$platform"
  url="$RELEASES_URL/chloedb-server-v$VERSION/$name.tar.gz"
  say "Installing $prog $VERSION for $platform"
  say "  from $url"
  fetch "$url" "$tmp/$name.tar.gz" || fail "could not download $url (is $VERSION a released version, and does it have $prog? Releases have chloedb-crawl, chloedb-sheets and chloectx from 0.3.2, chloedb-netops in releases after 0.3.2; see https://github.com/$REPO/releases, or build it from source: https://github.com/$REPO#quick-start)"
  fetch "$url.sha256" "$tmp/$name.tar.gz.sha256" || fail "could not download $url.sha256"
  want=$(cut -d' ' -f1 < "$tmp/$name.tar.gz.sha256")
  got=$(sha256 "$tmp/$name.tar.gz")
  [ -n "$want" ] && [ "$want" = "$got" ] || fail "the download's SHA-256 is $got, and the release says $want: not installing it"
  provenance "$tmp/$name.tar.gz" "$url"

  tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
  [ -f "$tmp/$name/$prog" ] || fail "the archive has no $name/$prog"

  mkdir -p "$INSTALL_DIR"
  # Into a temporary name and then renamed, so that a program running from
  # the old file keeps running and a failed copy leaves the old one in place.
  cp "$tmp/$name/$prog" "$INSTALL_DIR/.$prog.new"
  chmod 755 "$INSTALL_DIR/.$prog.new"
  mv -f "$INSTALL_DIR/.$prog.new" "$INSTALL_DIR/$prog"
  "$INSTALL_DIR/$prog" --version > /dev/null 2>&1 \
    || fail "installed $INSTALL_DIR/$prog, but it does not start on this machine: run it to see why"
  say "Installed $INSTALL_DIR/$prog ($("$INSTALL_DIR/$prog" --version 2>&1 | head -n 1))"
}

install_program chloedb-server
for prog in $EXTRA; do
  install_program "$prog"
done

case ":$PATH:" in
  *":$INSTALL_DIR:"*) bin=chloedb-server ;;
  *)
    bin="$INSTALL_DIR/chloedb-server"
    say ""
    say "$INSTALL_DIR is not on your PATH. To add it, put this line in your shell's profile (~/.zshrc, ~/.bashrc):"
    say "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

say ""
say "Try it: a demo catalogue of 20,000 products, with the query console at http://127.0.0.1:7700"
say "  $bin --demo 20000 --db demo.chloedb"
say "Then a first search, from another terminal:"
say "  curl -s 127.0.0.1:7700/v1/sql -H 'content-type: application/json' -d '{\"sql\": \"SELECT title FROM products WHERE MATCH(body, '\\''waterproof jacket'\\'') LIMIT 5\"}'"
say "The five-minute guide: $SITE/site/docs/quickstart.html"
if "$INSTALL_DIR/chloedb-server" doctor --help > /dev/null 2>&1; then
  say "Something in the way? $bin doctor checks the version, the Python package, the models and port 7700."
fi
case " $EXTRA " in
  *" chloectx "*) say "ChloeCtx for Claude Code: $INSTALL_DIR/chloectx fetch-model && $INSTALL_DIR/chloectx install --project /absolute/path/to/a/project --distill-model haiku (see $SITE/site/docs/chloectx-setup.html)" ;;
esac
