#!/bin/sh
# The zip the Chrome Web Store takes: the extension's files only (not its
# test, README or store notes).
#
#   sh pack.sh OUT.zip
set -eu
out=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
cd "$(dirname "$0")"
rm -f "$out"
zip -q -X "$out" manifest.json background.js content.js sites.js options.html options.js icon16.png icon48.png icon128.png
unzip -l "$out"
