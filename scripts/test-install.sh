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
# And how it checks an archive's build provenance with gh (LENA-201 of round 2
# of the study: a .sha256 beside the archive proves nothing about who built
# it), with a gh that verifies a made-up bundle: a release whose bundle
# names the archive installs; one whose archive and .sha256 were both
# replaced, or whose bundle is missing, does not; without gh, or for a
# release older than provenance, it installs on the SHA-256 and says so.
# server-release.yml checks the real thing, with GitHub's gh, on the
# attestations a run made.
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

# A gh whose `attestation verify FILE --bundle B ... --source-ref R` passes
# when B holds a line "<FILE's SHA-256> R", as a release's bundle names its
# archive's digest and its tag; it notes what it was asked.
cat > "$web/bin/gh" << 'GH'
#!/bin/sh
[ "$1 $2" = "attestation verify" ] || exit 2
shift 2
if [ "$1" = --help ]; then echo "  --source-ref string   Enforce that the git ref ..."; exit 0; fi
echo "$*" >> "$FAKE_WEB/gh-asked"
file=$1 bundle="" ref=""
while [ $# -gt 0 ]; do
  case "$1" in
    --bundle) bundle=$2; shift 2 ;;
    --source-ref) ref=$2; shift 2 ;;
    *) shift ;;
  esac
done
sum=$({ sha256sum "$file" 2> /dev/null || shasum -a 256 "$file"; } | cut -d' ' -f1)
if grep -qxF "$sum $ref" "$bundle"; then echo "Verification succeeded!"; exit 0; fi
echo "Error: verifying with issuer \"sigstore.dev\": no matching attestation for $sum" >&2
exit 1
GH
chmod 755 "$web/bin/gh"

# Releases 0.3.7, 9.9.8 and 9.9.9 of a server that only says its version,
# for every platform; 9.9.x with a bundle beside each archive, as releases
# from 0.3.9 have.
for v in 0.3.7 9.9.8 9.9.9; do
  for p in linux-x86_64 linux-aarch64 macos-arm64 macos-x86_64; do
    d="$work/build/chloedb-server-$p"
    mkdir -p "$d" "$web/releases/chloedb-server-v$v"
    printf '#!/bin/sh\necho "chloedb-server %s"\n' "$v" > "$d/chloedb-server"
    chmod 755 "$d/chloedb-server"
    tar -C "$work/build" -czf "$web/releases/chloedb-server-v$v/chloedb-server-$p.tar.gz" "chloedb-server-$p"
    (cd "$web/releases/chloedb-server-v$v" && { sha256sum "chloedb-server-$p.tar.gz" 2> /dev/null || shasum -a 256 "chloedb-server-$p.tar.gz"; } > "chloedb-server-$p.tar.gz.sha256")
    if [ "$v" != 0.3.7 ]; then
      echo "$(cut -d' ' -f1 < "$web/releases/chloedb-server-v$v/chloedb-server-$p.tar.gz.sha256") refs/tags/chloedb-server-v$v" > "$web/releases/chloedb-server-v$v/chloedb-server-$p.tar.gz.sigstore.jsonl"
    fi
    rm -rf "$d"
  done
done

failed=0
# Runs the installer as `curl ... | sh -s -- $@` would, with what $web
# serves; its output in $work/out, its status in $status.
# Without gh unless a test names one (CHLOEDB_GH), whatever this machine has.
run() {
  rm -rf "$work/bin" "$web/asked" "$web/gh-asked"
  status=0
  env -u CHLOEDB_VERSION -u CHLOEDB_RELEASES_URL -u CHLOEDB_WITH -u CHLOEDB_VERIFY -u CHLOEDB_VERIFY_SOURCE_REF \
    PATH="$web/bin:$PATH" FAKE_WEB="$web" CHLOEDB_INSTALL_DIR="$work/bin" CHLOEDB_GH="${GH_FOR_TEST:-$web/no-gh}" \
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
  rm -rf "$work/bin" "$web/asked" "$web/gh-asked"
  status=0
  env -u CHLOEDB_RELEASES_URL -u CHLOEDB_VERIFY_SOURCE_REF PATH="$web/bin:$PATH" FAKE_WEB="$web" CHLOEDB_INSTALL_DIR="$work/bin" \
    CHLOEDB_VERSION=${V:-9.9.8} CHLOEDB_VERIFY=${VERIFY:-auto} CHLOEDB_GH="${GH_FOR_TEST:-$web/no-gh}" \
    sh "$script" "$@" > "$work/out" 2>&1 || status=$?
}
run_given
check "CHLOEDB_VERSION: that release" installed 9.9.8
check "  asking nobody which is the newest" [ "$(grep -c -v /releases/download/ "$web/asked")" = 0 ]
check "  without gh: on the SHA-256 alone, and it says so" said "build provenance not checked: no gh here"

# With gh: the provenance, from the bundle beside the archive.
here=$(uname -s | sed 's/Linux/linux/; s/Darwin/macos/')-$(uname -m | sed 's/amd64/x86_64/')
case "$here" in macos-aarch64 | macos-arm64) here=macos-arm64 ;; linux-arm64) here=linux-aarch64 ;; esac
rel="$web/releases/chloedb-server-v9.9.8"
GH_FOR_TEST="$web/bin/gh"
run_given
check "with gh: the provenance is checked" said "build provenance checked: built by chloetech-ai/chloedb's server-release.yml from refs/tags/chloedb-server-v9.9.8"
check "  and it installs" installed 9.9.8
check "  asking gh, from the bundle, for this repository's release workflow, the tag and GitHub's runners" \
  grep -qE -- "--bundle [^ ]*chloedb-server-$here.tar.gz.sigstore.jsonl --repo chloetech-ai/chloedb --signer-workflow chloetech-ai/chloedb/.github/workflows/server-release.yml --source-ref refs/tags/chloedb-server-v9.9.8 --deny-self-hosted-runners" "$web/gh-asked"
check "  downloading the bundle beside the archive" grep -qF "chloedb-server-v9.9.8/chloedb-server-$here.tar.gz.sigstore.jsonl" "$web/asked"

# The archive and its .sha256 replaced together, as whoever can change the
# release could: the SHA-256 matches, the provenance does not.
cp "$rel/chloedb-server-$here.tar.gz" "$work/good.tar.gz"
cp "$rel/chloedb-server-$here.tar.gz.sha256" "$work/good.sha256"
mkdir -p "$work/evil/chloedb-server-$here"
printf '#!/bin/sh\necho "chloedb-server 9.9.8 (not ours)"\n' > "$work/evil/chloedb-server-$here/chloedb-server"
chmod 755 "$work/evil/chloedb-server-$here/chloedb-server"
tar -C "$work/evil" -czf "$rel/chloedb-server-$here.tar.gz" "chloedb-server-$here"
(cd "$rel" && { sha256sum "chloedb-server-$here.tar.gz" 2> /dev/null || shasum -a 256 "chloedb-server-$here.tar.gz"; } > "chloedb-server-$here.tar.gz.sha256")
run_given
check "an archive and .sha256 replaced together: refused" [ "$status" = 1 ]
check "  saying gh did not verify it" said "gh could not verify the build provenance"
check "  with gh's reason" said "no matching attestation"
check "  installing nothing" [ ! -e "$work/bin/chloedb-server" ]
# (A variable set before a function call stays set after it in some
# shells, macOS's sh among them: each is unset again.)
VERIFY=off run_given
unset VERIFY
check "  CHLOEDB_VERIFY=off installs it on the SHA-256 alone" [ "$status" = 0 ]
cp "$work/good.tar.gz" "$rel/chloedb-server-$here.tar.gz"
cp "$work/good.sha256" "$rel/chloedb-server-$here.tar.gz.sha256"

# The bundle gone: refused, since the release should have one.
mv "$rel/chloedb-server-$here.tar.gz.sigstore.jsonl" "$work/bundle"
run_given
check "no bundle beside the archive of a release that has provenance: refused" [ "$status" = 1 ]
check "  saying so" said "chloedb-server-v9.9.8/chloedb-server-$here.tar.gz.sigstore.jsonl, the archive's build provenance: not installing it"
check "  installing nothing" [ ! -e "$work/bin/chloedb-server" ]
mv "$work/bundle" "$rel/chloedb-server-$here.tar.gz.sigstore.jsonl"

# A release from before provenance: on its SHA-256, saying so.
V=0.3.7 run_given
unset V
check "a release older than provenance installs with gh here" installed 0.3.7
check "  on its SHA-256 alone, saying so" said "release 0.3.7 is older than build provenance (0.3.9)"
check "  asking gh nothing" [ ! -e "$web/gh-asked" ]

# require: no gh, no install.
GH_FOR_TEST="$web/no-gh" VERIFY=require run_given
unset VERIFY
check "CHLOEDB_VERIFY=require without gh: refused" [ "$status" = 1 ]
check "  saying why" said "CHLOEDB_VERIFY=require, and there is no gh here"
unset GH_FOR_TEST

# The first attested release is the same everywhere: in the three installers
# and in the docs URL gate, which holds a release from it back until its
# bundles are there. When a release goes out before attestations do, all
# four move to the next one together.
root=$(cd "$(dirname "$0")/.." && pwd)
froms=$( (sed -n 's/^ATTESTED_FROM=//p' "$root/docs/install.sh" "$root/install.sh" "$root/chloectx/plugin/scripts/chloectx-launch"
  sed -n 's/^ATTESTED_FROM = "\(.*\)"$/\1/p' "$root/scripts/check-doc-urls.py") | sort -u)
check "one first attested release ($froms) in the installers and the docs gate" [ "$(printf '%s\n' "$froms" | grep -c .)" = 1 ]

[ "$failed" = 0 ] && echo "every check passed" || exit 1
