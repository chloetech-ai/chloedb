#!/usr/bin/env bash
# Brings a branch started before ChloeDB's folders moved across the move.
#
# ChloeDB was called Strata. Phase 3a of the rename moved strata/ to
# chloedb/, strata-server/ to chloedb-server/ (and the other folders),
# renamed the crates and the programs, and rewrote what refers to them. It
# landed on main as two commits: one made by scripts/chloedb-rename.py
# alone, then the fixes made by hand. A branch from before then conflicts
# with nearly every file it touches. This script ports it in three steps,
# in the branch's checkout, with a clean tree:
#
#   1. merges main as it was just before the move (conflicts there are the
#      branch's own, as in any merge of main: resolve, commit, run again);
#   2. runs the same script on the branch's tree and commits the result as
#      a merge of the scripted commit, so git knows the move is in;
#   3. merges main, which then brings only the hand fixes and what main
#      did since, as an ordinary merge.
#
# Nothing is pushed and no history is rewritten: push the branch after.
#
#   scripts/chloedb-port.sh            # main from origin
#   scripts/chloedb-port.sh upstream   # another remote
#
# On a branch from before the move, this file is not there yet: run it from
# main with `git show origin/main:scripts/chloedb-port.sh | bash -s`.
set -euo pipefail

remote=${1:-origin}
die() { echo "chloedb-port: $*" >&2; exit 1; }

cd "$(git rev-parse --show-toplevel)"
git fetch -q "$remote" main
main="$remote/main"
scripted=$(git log "$main" --format=%H -n 1 --grep='^Rename Strata to ChloeDB, phase 3a: move the folders, crates and programs (scripted)$')
[ -n "$scripted" ] || die "$main has no scripted rename commit yet"
if git merge-base --is-ancestor "$scripted" HEAD; then
    echo "chloedb-port: this branch is across already; merging $main as usual"
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
git show "$scripted:scripts/chloedb-rename.py" > "$tmp/rename.py"
python3 "$tmp/rename.py"
mkdir -p scripts
git show "$scripted:scripts/chloedb-rename.py" > scripts/chloedb-rename.py
git show "$scripted:scripts/chloedb-port.sh" > scripts/chloedb-port.sh
chmod +x scripts/chloedb-rename.py scripts/chloedb-port.sh
git add -u
git add scripts/chloedb-rename.py scripts/chloedb-port.sh
commit=$(git commit-tree "$(git write-tree)" -p HEAD -p "$scripted" \
    -m "Port this branch to ChloeDB's folders (scripts/chloedb-rename.py)")
git reset -q "$commit"

# 3. The hand fixes and main since.
if ! git merge --no-edit "$main"; then
    die "the branch is across the move; resolve these conflicts with main and commit"
fi
echo "chloedb-port: done; the branch is across the move and up to date with $main"
