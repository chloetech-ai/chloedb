#!/usr/bin/env bash
# Brings a branch started before ChloeCtx's folder moved across the move.
#
# ChloeCtx was called Engram. Its rename moved engram/ to chloectx/ and
# docs/engram/ to docs/chloectx/, renamed the crate and the programs, and
# rewrote what refers to them. It landed on main as two commits: one made
# by scripts/chloectx-rename.py alone, then the changes made by hand. A
# branch from before then conflicts with nearly every file it touches in
# engram/. This script ports it in three steps, in the branch's checkout,
# with a clean tree:
#
#   1. merges main as it was just before the move (conflicts there are the
#      branch's own, as in any merge of main: resolve, commit, run again);
#   2. runs the same script on the branch's tree and commits the result as
#      a merge of the scripted commit, so git knows the move is in;
#   3. merges main, which then brings only the changes made by hand and
#      what main did since, as an ordinary merge.
#
# Nothing is pushed and no history is rewritten: push the branch after.
#
#   scripts/chloectx-port.sh            # main from origin
#   scripts/chloectx-port.sh upstream   # another remote
#
# On a branch from before the move, this file is not there yet: run it from
# main with `git show origin/main:scripts/chloectx-port.sh | bash -s`.
set -euo pipefail

remote=${1:-origin}
die() { echo "chloectx-port: $*" >&2; exit 1; }

cd "$(git rev-parse --show-toplevel)"
git fetch -q "$remote" main
main="$remote/main"
scripted=$(git log "$main" --format=%H -n 1 --grep='^Rename Engram to ChloeCtx: move the folder, the crate and the programs (scripted)$')
[ -n "$scripted" ] || die "$main has no scripted rename commit yet"
if git merge-base --is-ancestor "$scripted" HEAD; then
    echo "chloectx-port: this branch is across already; merging $main as usual"
    exec git merge --no-edit "$main"
fi
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "commit or stash your changes first"
git rev-parse -q --verify MERGE_HEAD > /dev/null && die "finish the merge under way first"

# 1. Main just before the move.
if ! git merge --no-edit "$scripted^"; then
    die "resolve these conflicts (they are the branch's own), commit, and run this again"
fi

# 2. The move, on this branch's tree.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git show "$scripted:scripts/chloectx-rename.py" > "$tmp/rename.py"
python3 "$tmp/rename.py"
mkdir -p scripts
git show "$scripted:scripts/chloectx-rename.py" > scripts/chloectx-rename.py
git show "$scripted:scripts/chloectx-port.sh" > scripts/chloectx-port.sh
chmod +x scripts/chloectx-rename.py scripts/chloectx-port.sh
git add -u
git add scripts/chloectx-rename.py scripts/chloectx-port.sh
commit=$(git commit-tree "$(git write-tree)" -p HEAD -p "$scripted" \
    -m "Port this branch to ChloeCtx's folder (scripts/chloectx-rename.py)")
git reset -q "$commit"

# 3. The hand fixes and main since.
if ! git merge --no-edit "$main"; then
    die "the branch is across the move; resolve these conflicts with main and commit"
fi
echo "chloectx-port: done; the branch is across the move and up to date with $main"
