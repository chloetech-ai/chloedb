#!/bin/sh
# Builds ChloeCtx's Claude Desktop extension (a .mcpb file): chloectx, the
# embedding model and the manifest, which Claude Desktop installs with a
# double-click.
#
#   sh build.sh CHLOECTX MODEL_DIR VERSION OUT.mcpb
#
# CHLOECTX is the program for the Mac (the release job makes one universal
# binary of the arm64 and x86_64 builds with lipo); MODEL_DIR holds
# potion-retrieval-32M's config.json, tokenizer.json and model.safetensors
# (`chloectx fetch-model`, each file checked against its pinned SHA-256).
# It packs with Anthropic's mcpb CLI (npx), which checks the manifest.
set -eu
bin=$1 models=$2 version=$3 out=$4
here=$(cd "$(dirname "$0")" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/server" "$stage/models/potion-retrieval-32M"
cp "$bin" "$stage/server/chloectx"
chmod 755 "$stage/server/chloectx"
for f in config.json tokenizer.json model.safetensors; do
  cp "$models/$f" "$stage/models/potion-retrieval-32M/$f"
done
cp "$here/icon.png" "$stage/icon.png"
cp "$here/../../LICENSE" "$stage/LICENSE"
sed "s/\"version\": \"0.0.0\"/\"version\": \"$version\"/" "$here/manifest.json" > "$stage/manifest.json"
grep -q "\"version\": \"$version\"" "$stage/manifest.json"
${MCPB:-npx -y @anthropic-ai/mcpb@2.1.2} validate "$stage/manifest.json"
${MCPB:-npx -y @anthropic-ai/mcpb@2.1.2} pack "$stage" "$out"
ls -la "$out"
