"""Training texts for static students: passages of C4 (one shard) cut to
about 120 words, and short spans of them (3 to 16 words) standing in for
queries."""
import gzip, json, random, sys
from pathlib import Path
random.seed(0)
src, out, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
passages, spans = [], []
with gzip.open(src, "rt") as f:
    for line in f:
        w = json.loads(line)["text"].split()
        if len(w) < 20:
            continue
        s = random.randrange(0, max(1, len(w) - 120))
        passages.append(" ".join(w[s:s + 120]))
        k = random.randint(3, 16); s = random.randrange(0, len(w) - k)
        spans.append(" ".join(w[s:s + k]))
        if len(passages) >= n:
            break
Path(out).write_text(json.dumps({"passages": passages, "spans": spans}))
print(len(passages), len(spans))
