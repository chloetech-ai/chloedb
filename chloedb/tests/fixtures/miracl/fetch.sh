#!/usr/bin/env bash
# Downloads MIRACL (Zhang et al., 2023: Wikipedia passages, queries and
# human judgements in 18 languages) for the languages named, into
# DIR/miracl-<lang>, in BEIR's layout (corpus.jsonl, queries.jsonl,
# qrels/dev.tsv), which chloedb-bm25-eval reads:
#
#   chloedb/tests/fixtures/miracl/fetch.sh DIR yo sw th hi
#   cargo run --release --bin chloedb-bm25-eval -- --dir DIR --split dev \
#       --collections miracl-yo,miracl-sw --analyzer standard \
#       --variants plain_r100 --tag miracl_standard --out results/miracl
#
# The files come from the miracl datasets on Hugging Face (miracl/miracl
# for the dev queries and judgements, miracl/miracl-corpus for the
# passages), at pinned revisions. Yoruba and Swahili, which the relevance
# gate reads (tests/relevance_gate.rs, CHLOEDB_MIRACL_DIR=DIR), are checked
# against their SHA-256 too. A language already there is kept. Needs curl,
# gunzip and python3. Sizes: yo 49 k passages, sw 132 k, hi 506 k, te 518 k,
# th 542 k, ko 1.5 M, ar 2.1 M, zh 4.9 M, ja 7.0 M.
set -euo pipefail

dir=${1:?usage: fetch.sh DIR LANG...}
shift
hf=https://huggingface.co
topics_rev=5be20db9509754dadad47689368639fcec739c00
corpus_rev=d921ec7e349ce0d28daf30b2da9da5ee698bef0d

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

# The SHA-256 of a language's file as this script writes it, where pinned.
pinned() {
    case "$1/$2" in
        yo/corpus.jsonl) echo 91a6badcf0b33a4906900b3e59311f44302caf68b724de858ecf8f1fd7d5e101 ;;
        yo/queries.jsonl) echo 2b801f37cf731e0a2ecf89a90f84d34780c886e09046889d3277aa25f3f751b8 ;;
        yo/qrels/dev.tsv) echo 7c0709e0087a55a4987702ef8f4d98a100f83c4d645a4b0b52def7036c3077f0 ;;
        sw/corpus.jsonl) echo 12ddcc6ca9a3784414a0d9479362cb974e357ba97ec4afbbc2c491edbfebdf11 ;;
        sw/queries.jsonl) echo e678e86493984bd67c3b8ecb693e680184669279b153f255c1be4478a71d8cb0 ;;
        sw/qrels/dev.tsv) echo 0b16d5f168b053c1753aecd6978e8a94e832d945d0c62851bf378646af90ef9e ;;
    esac
}

for lang in "$@"; do
    out="$dir/miracl-$lang"
    if [ -f "$out/corpus.jsonl" ] && [ -f "$out/queries.jsonl" ] && [ -f "$out/qrels/dev.tsv" ]; then
        continue
    fi
    mkdir -p "$out/qrels"
    topics="$hf/datasets/miracl/miracl/resolve/$topics_rev/miracl-v1.0-$lang"
    curl -fsSL --retry 5 --retry-delay 5 "$topics/topics/topics.miracl-v1.0-$lang-dev.tsv" | python3 -c '
import json, sys
for line in sys.stdin:
    qid, text = line.rstrip("\n").split("\t", 1)
    print(json.dumps({"_id": qid, "text": text}, ensure_ascii=False))
' > "$out/queries.jsonl.tmp"
    # TREC qrels (query, 0, passage, grade) as BEIR's (query, passage, grade).
    { printf 'query-id\tcorpus-id\tscore\n'; curl -fsSL --retry 5 --retry-delay 5 "$topics/qrels/qrels.miracl-v1.0-$lang-dev.tsv" | awk -F'\t' '{ print $1 "\t" $3 "\t" $4 }'; } > "$out/qrels/dev.tsv.tmp"
    # The passages' files, in order.
    files=$(curl -fsSL --retry 5 --retry-delay 5 "$hf/api/datasets/miracl/miracl-corpus/tree/$corpus_rev/miracl-corpus-v1.0-$lang" | python3 -c '
import json, re, sys
names = [f["path"] for f in json.load(sys.stdin) if f["path"].endswith(".jsonl.gz")]
print("\n".join(sorted(names, key=lambda n: int(re.search(r"docs-(\d+)", n).group(1)))))
')
    : > "$out/corpus.jsonl.tmp"
    for f in $files; do
        curl -fsSL --retry 5 --retry-delay 5 "$hf/datasets/miracl/miracl-corpus/resolve/$corpus_rev/$f" | gunzip | python3 -c '
import json, sys
for line in sys.stdin:
    d = json.loads(line)
    print(json.dumps({"_id": d["docid"], "title": d.get("title", ""), "text": d.get("text", "")}, ensure_ascii=False))
' >> "$out/corpus.jsonl.tmp"
    done
    for f in corpus.jsonl queries.jsonl qrels/dev.tsv; do
        want=$(pinned "$lang" "$f")
        got=$(sha256 "$out/$f.tmp")
        if [ -n "$want" ] && [ "$got" != "$want" ]; then
            echo "$out/$f: SHA-256 $got, expected $want" >&2
            exit 1
        fi
    done
    mv "$out/queries.jsonl.tmp" "$out/queries.jsonl"
    mv "$out/qrels/dev.tsv.tmp" "$out/qrels/dev.tsv"
    mv "$out/corpus.jsonl.tmp" "$out/corpus.jsonl"
    echo "fetched $out: $(wc -l < "$out/corpus.jsonl") passages, $(wc -l < "$out/queries.jsonl") queries"
done
