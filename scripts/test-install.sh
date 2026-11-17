#!/bin/sh
# Tests how the one-line installer, docs/install.sh, finds the newest
# release, with a curl on the PATH that answers from a folder in place of
# GitHub and the website: from GitHub's API; without it, from github.com's
# /releases/latest; without both, from the website's versions.json; and
# when nothing answers, a message that names each address and what it
# said, and the exact command to run with the version named (MAYA-1 and
# PRIYA-13 of the persona study of 17 November 2026: a proxy refused
# api.github.com, and the installer stopped with "set CHLOEDB_VERSION",
# not saying how).
#
#   sh scripts/test-install.sh [docs/install.sh]
#
# Needs sh, tar and sha256sum (or shasum); no network. server-release.yml
# runs it on each platform.

set -eu

script=$(cd "$(dirname "${1:-docs/install.sh}")" && pwd)/$(basename "${1:-docs/install.sh}")
[ -f "$script" ] || { echo "no installer at $script" >&2; exit 1; }
work=$(mktemp -d 2> /dev/null || mktemp -d -t chloedb-install-test)
trap 'rm -rf "$work"' EXIT INT TERM
web="$work/web"
mkdir -p "$web/bin" "$web/releases"

# The curl: an address of api.github.com, github.com's /releases/latest or
# the website's versions.json answers with the file of that name in $web
# (api, latest: the address it redirects to; versions.json), or 403 without
# it; a release's download, from $web/releases.
cat > "$web/bin/curl" << 'CURL'
#!/bin/sh
out="" url=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift 2 ;;
    -w | --retry) shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
echo "$url" >> "$FAKE_WEB/asked"
refused() { echo "curl: (22) The requested URL returned error: 403" >&2; exit 22; }
case "$url" in
  https://api.github.com/*) f=api ;;
  https://github.com/*/releases/latest) f=latest ;;
  https://chloetech-ai.github.io/chloedb/site/versions.json) f=versions.json ;;
  https://github.com/*/releases/download/*) f=releases/${url#*/releases/download/} ;;
  *) refused ;;
esac
[ -f "$FAKE_WEB/$f" ] || refused
if [ -n "$out" ] && [ "$out" != /dev/null ]; then cp "$FAKE_WEB/$f" "$out"; else cat "$FAKE_WEB/$f"; fi
CURL
chmod 755 "$web/bin/curl"

# Releases 9.9.8 and 9.9.9 of a server that only says its version, for
# every platform.
for v in 9.9.8 9.9.9; do
  for p in linux-x86_64 linux-aarch64 macos-arm64 macos-x86_64; do
    d="$work/build/chloedb-server-$p"
    mkdir -p "$d" "$web/releases/chloedb-server-v$v"
    printf '#!/bin/sh\necho "chloedb-server %s"\n' "$v" > "$d/chloedb-server"
    chmod 755 "$d/chloedb-server"
    tar -C "$work/build" -czf "$web/releases/chloedb-server-v$v/chloedb-server-$p.tar.gz" "chloedb-server-$p"
    (cd "$web/releases/chloedb-server-v$v" && { sha256sum "chloedb-server-$p.tar.gz" 2> /dev/null || shasum -a 256 "chloedb-server-$p.tar.gz"; } > "chloedb-server-$p.tar.gz.sha256")
    rm -rf "$d"
  done
done

failed=0
# Runs the installer as `curl ... | sh -s -- $@` would, with what $web
# serves; its output in $work/out, its status in $status.
run() {
  rm -rf "$work/bin" "$web/asked"
  status=0
  env -u CHLOEDB_VERSION -u CHLOEDB_RELEASES_URL -u CHLOEDB_WITH PATH="$web/bin:$PATH" FAKE_WEB="$web" CHLOEDB_INSTALL_DIR="$work/bin" \
    sh "$script" "$@" > "$work/out" 2>&1 || status=$?
}
check() {
  what=$1
  shift
  if "$@"; then
    echo "ok   $what"
  else
    echo "FAIL $what"
    sed 's/^/     | /' "$work/out"
    failed=1
  fi
}
installed() { [ "$status" = 0 ] && [ "$("$work/bin/chloedb-server" --version)" = "chloedb-server $1" ]; }
said() { grep -qF -- "$1" "$work/out"; }

# GitHub's API, which lists the packages' releases too.
printf '[\n  {\n    "tag_name": "chloedb-py-v9.9.9",\n  },\n  {\n    "tag_name": "chloedb-server-v9.9.9",\n  }\n]\n' > "$web/api"
echo "https://github.com/chloetech-ai/chloedb/releases/tag/chloedb-server-v9.9.8" > "$web/latest"
echo '{"latest": "9.9.8"}' > "$web/versions.json"
run
check "the API's newest server release" installed 9.9.9

# No API: /releases/latest's page.
rm "$web/api"
run
check "without the API: the release /releases/latest leads to" installed 9.9.8
check "  and it says where the version came from" said "says https://github.com/chloetech-ai/chloedb/releases/latest"

# No API, and the latest release is not the server's: the website.
echo "https://github.com/chloetech-ai/chloedb/releases/tag/chloedb-py-v9.9.9" > "$web/latest"
echo '{"latest": "9.9.9"}' > "$web/versions.json"
run
check "without the API, and /releases/latest a package's: the website's versions.json" installed 9.9.9
check "  and it says so" said "says https://chloetech-ai.github.io/chloedb/site/versions.json"

# Nothing answers: each address and what it said, and the command.
rm "$web/latest" "$web/versions.json"
run --with ctx
check "nothing answers: it fails" [ "$status" = 1 ]
check "  naming the API's refusal" said "https://api.github.com/repos/chloetech-ai/chloedb/releases?per_page=100: The requested URL returned error: 403"
check "  and /releases/latest's" said "https://github.com/chloetech-ai/chloedb/releases/latest: The requested URL returned error: 403"
check "  and the website's" said "https://chloetech-ai.github.io/chloedb/site/versions.json: The requested URL returned error: 403"
check "  and the command, the version after the pipe" said "curl -fsSL https://chloetech-ai.github.io/chloedb/install.sh | CHLOEDB_INSTALL_DIR=$work/bin CHLOEDB_VERSION=<version> sh -s -- --with ctx"
check "  installing nothing" [ ! -e "$work/bin/chloedb-server" ]

# The version given: nothing is asked.
run_given() {
  rm -rf "$work/bin" "$web/asked"
  status=0
  env -u CHLOEDB_RELEASES_URL PATH="$web/bin:$PATH" FAKE_WEB="$web" CHLOEDB_INSTALL_DIR="$work/bin" CHLOEDB_VERSION=9.9.8 \
    sh "$script" > "$work/out" 2>&1 || status=$?
}
run_given
check "CHLOEDB_VERSION: that release" installed 9.9.8
check "  asking nobody which is the newest" [ "$(grep -c -v /releases/download/ "$web/asked")" = 0 ]

[ "$failed" = 0 ] && echo "every check passed" || exit 1
