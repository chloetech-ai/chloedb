#!/bin/sh
# Install ChloeDB's server, prebuilt, with no Rust and no C compiler:
#
#   curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | sh
#
# It downloads the newest chloedb-server release for this machine from
# GitHub (Linux x86_64 and aarch64 with glibc 2.35 or later, macOS arm64 and
# x86_64, macOS 11 or later), checks the archive against the SHA-256 the
# release publishes beside it, and puts `chloedb-server` in ~/.local/bin.
# It needs curl (or wget), tar and sha256sum (or shasum), changes nothing
# outside the install folder, and asks for no password.
#
# Settings (environment variables; with `curl ... | sh`, put them after the
# pipe: `curl ... | CHLOEDB_VERSION=0.2.0 sh`):
#
#   CHLOEDB_VERSION       the version to install (0.2.0); the newest release by default
#   CHLOEDB_INSTALL_DIR   where to put the program (default ~/.local/bin)
#   CHLOEDB_RELEASES_URL  where the release archives are (default this
#                         repository's GitHub releases; a local folder or
#                         server for testing, laid out as
#                         <url>/chloedb-server-v<version>/chloedb-server-<platform>.tar.gz)
#
# This file is published by GitHub Pages from docs/install.sh of
# https://github.com/chloetech-ai/chloedb. Read it first if you like:
#   curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | less

set -eu

REPO="chloetech-ai/chloedb"
RELEASES_URL="${CHLOEDB_RELEASES_URL:-https://github.com/$REPO/releases/download}"
INSTALL_DIR="${CHLOEDB_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${CHLOEDB_VERSION:-}"

say() { printf '%s\n' "$*"; }
fail() {
  printf 'chloedb install: %s\n' "$*" >&2
  exit 1
}
have() { command -v "$1" > /dev/null 2>&1; }

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

if [ -z "$VERSION" ]; then
  case "$RELEASES_URL" in
    https://github.com/*) ;;
    *) fail "with CHLOEDB_RELEASES_URL, set CHLOEDB_VERSION too" ;;
  esac
  # The repository releases several things (the server, the Python and
  # JavaScript packages), each under its own tag: the newest server tag.
  # GitHub lists releases newest first; drafts are not shown and
  # prereleases are skipped by name (a version with a hyphen).
  VERSION=$(fetch "https://api.github.com/repos/$REPO/releases?per_page=100" \
    | sed -n 's/.*"tag_name": *"chloedb-server-v\([0-9][0-9.]*\)".*/\1/p' | head -n 1) \
    || true
  [ -n "$VERSION" ] || fail "could not find a chloedb-server release on https://github.com/$REPO/releases (set CHLOEDB_VERSION to pick one)"
fi
VERSION=${VERSION#v}
VERSION=${VERSION#chloedb-server-v}

# --- Download, check, install ----------------------------------------------

name="chloedb-server-$platform"
url="$RELEASES_URL/chloedb-server-v$VERSION/$name.tar.gz"

if have sha256sum; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif have shasum; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "needs sha256sum or shasum to check the download"
fi
have tar || fail "needs tar to unpack the release"

tmp=$(mktemp -d 2> /dev/null || mktemp -d -t chloedb)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Installing chloedb-server $VERSION for $platform"
say "  from $url"
fetch "$url" "$tmp/$name.tar.gz" || fail "could not download $url (is $VERSION a released version? see https://github.com/$REPO/releases)"
fetch "$url.sha256" "$tmp/$name.tar.gz.sha256" || fail "could not download $url.sha256"
want=$(cut -d' ' -f1 < "$tmp/$name.tar.gz.sha256")
got=$(sha256 "$tmp/$name.tar.gz")
[ -n "$want" ] && [ "$want" = "$got" ] || fail "the download's SHA-256 is $got, and the release says $want: not installing it"

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
[ -f "$tmp/$name/chloedb-server" ] || fail "the archive has no $name/chloedb-server"

mkdir -p "$INSTALL_DIR"
# Into a temporary name and then renamed, so that a server running from the
# old file keeps running and a failed copy leaves the old one in place.
cp "$tmp/$name/chloedb-server" "$INSTALL_DIR/.chloedb-server.new"
chmod 755 "$INSTALL_DIR/.chloedb-server.new"
mv -f "$INSTALL_DIR/.chloedb-server.new" "$INSTALL_DIR/chloedb-server"
"$INSTALL_DIR/chloedb-server" --version > /dev/null 2>&1 \
  || fail "installed $INSTALL_DIR/chloedb-server, but it does not start on this machine: run it to see why"

say "Installed $INSTALL_DIR/chloedb-server ($("$INSTALL_DIR/chloedb-server" --version 2>&1 | head -n 1))"

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
say "The five-minute guide: https://github.com/$REPO/blob/main/docs/quickstart.md"
