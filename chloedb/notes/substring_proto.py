#!/usr/bin/env python3
"""Prototype of the substring index format (parts of chloedb/notes/substring.md): checks the
levelwise 4-ary Huffman-shaped wavelet tree, backward search and locate against brute force."""
import random, heapq, sys
SEP = 0xFF
BUCKET = 512

def build_text(docs):
    """docs: list of bytes or None. Returns T and, per text position, (ordinal, kind) for marking."""
    t = bytearray([SEP]); starts = []; ords = []
    o = 0
    for d in docs:
        if d is None: continue
        starts.append(len(t)); t += d; t.append(SEP); o += 1
    return bytes(t), starts

def suffix_array(t):
    n = len(t)
    return sorted(range(n + 1), key=lambda i: t[i:] + b"" if i < n else b"")  # empty suffix first

def huff4_lens(counts, limit=12):
    syms = [c for c in range(256) if counts[c]]
    if len(syms) == 1: return {syms[0]: 1}
    h = [(counts[c], c, [c]) for c in syms]
    k = len(h)
    while (k - 1) % 3: h.append((0, 256 + k, [])); k += 1
    heapq.heapify(h); lens = {c: 0 for c in syms}; tie = 1000
    while len(h) > 1:
        tot = 0; mem = []
        for _ in range(4):
            w, _, m = heapq.heappop(h); tot += w; mem += m
        for c in mem: lens[c] += 1
        tie += 1; heapq.heappush(h, (tot, tie, mem))
    assert max(lens.values()) <= limit
    return lens

class Tree:
    """Slots: at depth k the first I[k] slots are internal nodes, the next are leaves by symbol."""
    def __init__(self, lens):
        self.lens = lens; K = max(lens.values()); self.K = K
        leaf = [sorted(c for c in lens if lens[c] == k) for k in range(K + 1)]
        I = [0] * (K + 2)
        for k in range(K - 1, -1, -1): I[k] = -(-(I[k + 1] + len(leaf[k + 1])) // 4)
        assert I[0] == 1, I
        self.I = I; self.leaf = leaf; self.code = {}
        for k in range(1, K + 1):
            for j, c in enumerate(leaf[k]):
                slot = I[k] + j; digits = []
                for kk in range(k, 0, -1): digits.append(slot % 4); slot //= 4
                assert slot == 0
                self.code[c] = digits[::-1]
    def child(self, k, node, d):
        """(is_leaf, symbol or internal index) of child d of internal node `node` at depth k."""
        slot = 4 * node + d
        if slot < self.I[k + 1]: return False, slot
        j = slot - self.I[k + 1]
        return True, (self.leaf[k + 1][j] if j < len(self.leaf[k + 1]) else None)

class Levels:
    def __init__(self, tree, L):
        self.t = tree; K = tree.K
        # node index of each position's prefix at each level, positions ordered by (node, position)
        cur = [(0, i) for i in range(len(L))]  # (internal node at depth k, bwt position), sorted
        self.seq = []; self.start = []; self.cnt = []
        for k in range(K):
            digs = [tree.code[L[i]][k] for _, i in cur]
            self.seq.append(digs)
            start = [0] * (tree.I[k] + 1); cnt = [[0] * 4 for _ in range(tree.I[k])]
            for (node, _), d in zip(cur, digs): cnt[node][d] += 1
            for nd in range(tree.I[k]): start[nd + 1] = start[nd] + sum(cnt[nd])
            self.start.append(start); self.cnt.append(cnt)
            nxt = []
            for (node, i), d in zip(cur, digs):
                leaf, x = tree.child(k, node, d)
                if not leaf: nxt.append((x, i))
            nxt.sort(key=lambda p: p[0])  # stable: keeps bwt order inside a node
            cur = nxt
        assert not cur
        # the claim the format rests on: a child's start is its parent's start plus the smaller digits in the parent
        for k in range(K - 1):
            for nd in range(tree.I[k]):
                for d in range(4):
                    leaf, x = tree.child(k, nd, d)
                    if not leaf: assert self.start[k + 1][x] == self.start[k][nd] + sum(self.cnt[k][nd][:d]), (k, nd, d)
    def rankd(self, k, d, pos): return sum(1 for x in self.seq[k][:pos] if x == d)  # the blocks' job
    def rank(self, c, i):
        """Occurrences of c in L[0..i)."""
        if c not in self.t.code: return 0
        node = 0; o = i
        for k, d in enumerate(self.t.code[c]):
            s = self.start[k][node]
            o = self.rankd(k, d, s + o) - self.rankd(k, d, s)
            leaf, x = self.t.child(k, node, d)
            if leaf: return o
            node = x
    def access_rank(self, j):
        """(L[j], occurrences of it in L[0..j))."""
        node = 0; o = j
        for k in range(self.t.K):
            s = self.start[k][node]; d = self.seq[k][s + o]
            o = self.rankd(k, d, s + o) - self.rankd(k, d, s)
            leaf, x = self.t.child(k, node, d)
            if leaf: return x, o
            node = x

def build(docs, sample):
    T, starts = build_text(docs); n = len(T)
    sa = suffix_array(T); primary = sa.index(0)
    L = bytes(T[i - 1] for i in sa if i > 0)                     # n symbols, the sentinel's row left out
    counts = [0] * 256
    for c in T: counts[c] += 1
    C = [1] * 257
    for c in range(256): C[c + 1] = C[c] + counts[c]
    tree = Tree(huff4_lens(counts)); lv = Levels(tree, L)
    # marks: SA rows whose text position is a document's start + a multiple of `sample`, or the separator before a document
    mark = {}
    for o, st in enumerate(starts):
        mark[st - 1] = o
        end = T.index(SEP, st)
        for p in range(st, end, sample): mark[p] = o
    buckets = {}
    for row, p in enumerate(sa):
        if p in mark: buckets.setdefault(row // BUCKET, []).append((row % BUCKET, mark[p]))
    return dict(T=T, n=n, sa=sa, primary=primary, L=L, C=C, lv=lv, buckets=buckets, sample=sample, starts=starts)

def rank(ix, c, row):
    """Occurrences of c in BWT rows [0, row), rows counted with the sentinel's."""
    return ix["lv"].rank(c, row - (row > ix["primary"]))

def search(ix, pat):
    lo, hi = 0, ix["n"] + 1
    for c in reversed(pat):
        lo = ix["C"][c] + rank(ix, c, lo); hi = ix["C"][c] + rank(ix, c, hi)
        if lo >= hi: return 0, 0
    return lo, hi

def locate(ix, row):
    steps = 0
    while True:
        for off, o in ix["buckets"].get(row // BUCKET, []):
            if off == row % BUCKET: return o, steps
        assert row != ix["primary"] and steps < ix["sample"], "walked past a sample"
        c, r = ix["lv"].access_rank(row - (row > ix["primary"]))
        row = ix["C"][c] + r; steps += 1

def check(seed):
    rnd = random.Random(seed)
    alpha = rnd.choice([b"ab", b"abcdefgh \n", bytes(range(32, 127)), "aé東京ไทย ,.x".encode(), bytes([0, 1, 2, 0xfe, 0x7f])])
    skew = rnd.choice([1, 3])
    docs = []
    for _ in range(rnd.randint(1, 40)):
        if rnd.random() < 0.15: docs.append(None); continue
        ln = rnd.choice([0, 1, 2, rnd.randint(0, 30), rnd.randint(0, 200)])
        docs.append(bytes(alpha[int(len(alpha) * rnd.random() ** skew)] for _ in range(ln)))
    sample = rnd.choice([1, 2, 4, 8, 32])
    ix = build(docs, sample)
    T, L, lv = ix["T"], ix["L"], ix["lv"]
    for _ in range(60):  # rank and access against the BWT itself
        c = rnd.choice(sorted(set(L))); i = rnd.randint(0, len(L))
        assert lv.rank(c, i) == L[:i].count(c)
        if L:
            j = rnd.randrange(len(L)); assert lv.access_rank(j) == (L[j], L[:j].count(L[j]))
    present = [d for d in docs if d is not None]
    for _ in range(60):
        kind = rnd.choice(["sub", "sub", "prefix", "suffix", "whole", "miss"])
        src = rnd.choice(present) if present else b""
        a = rnd.randint(0, len(src)); b = rnd.randint(a, min(len(src), a + 6))
        body = src[a:b] if kind != "miss" else bytes(rnd.choice(alpha) for _ in range(rnd.randint(1, 4)))
        if kind == "prefix": body = src[:b - a]
        if kind == "suffix": body = src[len(src) - (b - a):]
        if kind == "whole": body = src
        pat = {"prefix": bytes([SEP]) + body, "suffix": body + bytes([SEP]), "whole": bytes([SEP]) + body + bytes([SEP])}.get(kind, body)
        if not pat or (not body and kind in ("prefix", "suffix")): continue
        want_docs = set(); occ = 0
        for o, d in enumerate(present):
            k = {"prefix": d.startswith(body), "suffix": d.endswith(body), "whole": d == body}.get(kind)
            if k is None:
                c = sum(1 for i in range(len(d) - len(body) + 1) if d[i:i + len(body)] == body); occ += c; k = c > 0
            else: occ += k
            if k: want_docs.add(o)
        lo, hi = search(ix, pat)
        assert hi - lo == occ, (kind, pat, hi - lo, occ)
        got = set()
        for row in range(lo, hi):
            o, steps = locate(ix, row); got.add(o); assert steps < max(sample, 1)
        assert got == want_docs, (kind, pat, got, want_docs)
    return len(T), lv.t.K

if __name__ == "__main__":
    tot = 0; maxk = 0
    for seed in range(int(sys.argv[1]) if len(sys.argv) > 1 else 300):
        n, k = check(seed); tot += n; maxk = max(maxk, k)
    print("ok", tot, "bytes of text checked; deepest tree", maxk, "levels")
