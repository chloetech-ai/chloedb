"""Block-max retrieval attention over a KV cache: a measurement harness.

A long prefix is prefilled with dense attention into a KV cache. A chunk of
new tokens (the continuation of a book, or a question about the prefix) is
then run with *sparse* attention: each query, in each layer and KV head,
selects a budget of prefix tokens to read, and attends to those plus the
new chunk itself (the local window, always read). Every selection method
works from metadata kept beside the cache, as a block-max index keeps
upper bounds beside postings:

  window        first page (the attention sink) + the most recent pages
  quest         per-page channel min/max; page bound = sum_i max(q_i m_i, q_i M_i)
                (Quest, Tang et al. 2024), the block-max bound of an inner product
  cluster_box   keys reordered by k-means into clusters ("pages" by content,
                as docid reassignment does for postings), same box bound
  cluster_cent  same clusters, scored by q . centroid (ClusterKV/RetrievalAttention-like)
  rabitq        token-level 1-bit RaBitQ estimate of q . k (ChloeDB's quantiser)
  rabitq_page   RaBitQ estimates, max per positional page (page-granular reads)
  oracle_page   pages by true attention mass (the best any page selector can do)
  oracle_token  tokens by true score (the best any token selector can do)

The oracles need the dense scores and exist only as ceilings.

GQA: a KV head is read once for its group of query heads, so selection is
per KV head; scores are max-pooled (mass: summed) over the group.
"""
import math
import time
import torch
from transformers import AttentionInterface
from transformers.integrations.sdpa_attention import sdpa_attention_forward

EPS0 = 3.0  # as chloedb/src/vector/rabitq.rs
PAGE_METHODS = {"window", "quest", "rabitq_page", "oracle_page"}
CLUSTER_METHODS = {"cluster_box", "cluster_cent"}
TOKEN_METHODS = {"rabitq", "oracle_token"}
ALL_METHODS = ["window", "quest", "cluster_box", "cluster_cent", "rabitq", "rabitq_page",
               "oracle_page", "oracle_token"]


class Ctl:
    def __init__(self):
        self.mode = "dense"          # dense | sparse | stats
        self.method = None
        self.budget = 0              # prefix tokens read per query per KV head
        self.page = 16
        self.dense_layers = {0, 1}   # Quest keeps the first two layers dense
        self.L = 0                   # prefix length (multiple of page)
        self.meta = {}
        self.stats = {}
        self.stat_methods = ALL_METHODS
        self.stat_budgets = []
        self.exact_ks = [16, 64]
        self.tchunk = 64
        self.seed = 0
        self.meta_time = 0.0   # seconds spent building metadata (k-means, codes)
        self.meta_key = None
        self.blend_pos = None
        self.record = None     # {layer: [ (H,C) bool per step ]} of clusters read, when a dict
        self.record_size = {}
        self.cur_layer = None  # mode blend: positions of the prefix tokens recomputed

    def reset_prefix(self, L):
        assert L % self.page == 0
        self.L = L
        self.meta = {}


ctl = Ctl()


# ---------------------------------------------------------------- metadata

def _kmeans(x, C, iters=8, seed=0):
    """x (L,d) -> centroids (C,d), assign (L,)"""
    g = torch.Generator().manual_seed(seed)
    cent = x[torch.randperm(x.shape[0], generator=g)[:C]].clone()
    for _ in range(iters):
        # argmax 2 x.c - |c|^2
        a = (2 * x @ cent.T - (cent * cent).sum(-1)).argmax(-1)
        s = torch.zeros_like(cent).index_add_(0, a, x)
        n = torch.bincount(a, minlength=C).to(x.dtype).unsqueeze(-1)
        cent = torch.where(n > 0, s / n.clamp(min=1), cent)
    a = (2 * x @ cent.T - (cent * cent).sum(-1)).argmax(-1)
    return cent, a


def build_meta(layer, k):
    """k (Hkv, L, d) prefix keys of one layer."""
    if layer in ctl.meta:
        return ctl.meta[layer]
    t_build = time.time()
    H, L, d = k.shape
    P = ctl.page
    kp = k.view(H, L // P, P, d)
    m = {"pmin": kp.amin(2), "pmax": kp.amax(2)}
    # clusters: as many as pages, so the metadata is the same size as Quest's
    C = L // P
    cents, assigns, cmin, cmax, csize = [], [], [], [], []
    for h in range(H):
        cent, a = _kmeans(k[h], C, seed=ctl.seed + 1000 * layer + h)
        idx = a.unsqueeze(-1).expand(-1, d)
        lo = torch.full((C, d), float("inf")).scatter_reduce(0, idx, k[h], "amin", include_self=True)
        hi = torch.full((C, d), float("-inf")).scatter_reduce(0, idx, k[h], "amax", include_self=True)
        cents.append(cent); assigns.append(a); cmin.append(lo); cmax.append(hi)
        csize.append(torch.bincount(a, minlength=C))
    m.update(cent=torch.stack(cents), assign=torch.stack(assigns), cmin=torch.stack(cmin),
             cmax=torch.stack(cmax), csize=torch.stack(csize))
    # RaBitQ, one bit per dimension: centre on the mean, rotate, keep signs
    g = torch.Generator().manual_seed(ctl.seed + 7 + layer)
    Q, _ = torch.linalg.qr(torch.randn(d, d, generator=g))
    c = k.mean(1, keepdim=True)                       # (H,1,d)
    x = k - c
    nrm = x.norm(dim=-1, keepdim=True).clamp(min=1e-6)
    o = (x / nrm) @ Q.T                                # rotated unit directions
    ybar = torch.sign(o) / math.sqrt(d)
    ip = (ybar * o).sum(-1, keepdim=True).clamp(min=1e-6)
    # RaBitQ's error bound per row: |estimate - q.k| <= EPS0 * |q| * e (Theorem 3.2)
    e = nrm * (1 - ip * ip).clamp(min=0).sqrt() / (ip * math.sqrt(d - 1))
    m.update(rq_rot=Q, rq_c=c, rq_code=ybar, rq_f=(nrm / ip), rq_e=e.squeeze(-1))
    ctl.meta[layer] = m
    ctl.meta_time += time.time() - t_build
    return m


def box_bound(q, lo, hi):
    """q (H,g,T,d), lo/hi (H,n,d) -> (H,g,T,n): max of q.k over the box."""
    return torch.einsum("hgtd,hnd->hgtn", q.clamp(min=0), hi) + \
        torch.einsum("hgtd,hnd->hgtn", q.clamp(max=0), lo)


# ---------------------------------------------------------------- selection

def _select(method, budget, q, m, S_pre, probs_pre):
    """Return a bool mask (H,T,L) of prefix tokens read, shared over the group.
    q (H,g,T,d) scaled queries; S_pre (H,g,T,L) true scaled scores; probs_pre the
    true softmax mass on the prefix (only the oracles read these two)."""
    H, g, T, d = q.shape
    L, P = ctl.L, ctl.page
    npg = L // P
    nb = max(0, min(npg, budget // P))
    dev = q.device
    if method in PAGE_METHODS:
        if method == "window":
            idx = torch.cat([torch.zeros(1, dtype=torch.long),
                             torch.arange(npg - (nb - 1), npg)]) if nb > 1 else torch.zeros(1, dtype=torch.long)
            pm = torch.zeros(H, T, npg, dtype=torch.bool)
            pm[:, :, idx] = True
        else:
            if method == "quest":
                sc = box_bound(q, m["pmin"], m["pmax"]).amax(1)
            elif method == "rabitq_page":
                sc = rabitq_est(q, m).amax(1).view(H, T, npg, P).amax(-1)
            elif method == "oracle_page":
                sc = probs_pre.sum(1).view(H, T, npg, P).sum(-1)
            sc[..., 0] = float("-inf")   # the sink page is read anyway
            top = sc.topk(nb, dim=-1).indices
            pm = torch.zeros(H, T, npg, dtype=torch.bool).scatter_(-1, top, True)
        return pm.repeat_interleave(P, dim=-1)
    if method in TOKEN_METHODS:
        if method == "rabitq":
            sc = rabitq_est(q, m).amax(1)
        else:
            sc = S_pre.amax(1)
        sc = sc.clone(); sc[..., :P] = float("-inf")
        B = min(L, nb * P)
        top = sc.topk(B, dim=-1).indices
        return torch.zeros(H, T, L, dtype=torch.bool).scatter_(-1, top, True)
    if method in CLUSTER_METHODS:
        if method == "cluster_box":
            sc = box_bound(q, m["cmin"], m["cmax"]).amax(1)
        else:
            sc = torch.einsum("hgtd,hnd->hgtn", q, m["cent"]).amax(1)
        # whole clusters in rank order while they fit in the budget (the
        # first page, the sink, is added for every method in select())
        size = m["csize"]
        sc = sc.masked_fill((size == 0).unsqueeze(1), float("-inf"))
        order = sc.argsort(dim=-1, descending=True)
        cum = size.unsqueeze(1).expand_as(order).gather(-1, order).cumsum(-1)
        take = cum <= (nb - 1) * P
        cm = torch.zeros_like(take).scatter_(-1, order, take)
        if ctl.record is not None:
            # which clusters the last query of the chunk read (reuse across decode steps)
            ctl.record.setdefault(ctl.cur_layer, []).append(cm[:, -1].clone())
            ctl.record_size[ctl.cur_layer] = size
        return cm.gather(-1, m["assign"].unsqueeze(1).expand(H, T, L))
    raise ValueError(method)


def select(method, budget, q, m, S_pre, probs_pre):
    """Every method also reads the first page (the attention sink), within its budget:
    the others choose budget-16 tokens."""
    P = ctl.page
    sel = _select(method, budget - P if method != "window" else budget, q, m, S_pre, probs_pre)
    sel[..., :P] = True
    return sel


def rabitq_est(q, m):
    """q (H,g,T,d) -> (H,g,T,L) estimates of q.k"""
    qr = q @ m["rq_rot"].T
    base = torch.einsum("hgtd,hzd->hgtz", q, m["rq_c"])  # (H,g,T,1)
    return base + torch.einsum("hgtd,hld->hgtl", qr, m["rq_code"]) * m["rq_f"].squeeze(-1)[:, None, None, :]


# ---------------------------------------------------------------- attention

def _attend(S, sel_pre, v, T0, L):
    """S (H,g,t,N) scores, sel_pre (H,t,L) bool or None, v (H,N,d)."""
    H, g, t, N = S.shape
    S = S.clone()
    if sel_pre is not None:
        S[..., :L] = S[..., :L].masked_fill(~sel_pre.unsqueeze(1), float("-inf"))
    p = torch.softmax(S, dim=-1)
    return torch.einsum("hgtn,hnd->hgtd", p, v), p


def kvsel_attention(module, query, key, value, attention_mask, scaling=None, **kw):
    Tq, N = query.shape[2], key.shape[2]
    layer = module.layer_idx
    if Tq == N:
        # prefill of the prefix: plain causal attention
        return sdpa_attention_forward(module, query, key, value, attention_mask, scaling=scaling, **kw)
    if ctl.mode == "blend":
        # CacheBlend-style recompute: the Tq queries are chosen tokens of the
        # stitched prefix (positions ctl.blend_pos), run again through every layer.
        # Each sees the prefix before it with its own old entries hidden, and the
        # fresh entries of the chosen tokens before it (appended after the prefix).
        P = N - Tq
        pos = ctl.blend_pos
        is_sel = torch.zeros(P, dtype=torch.bool)
        is_sel[pos] = True
        allow = torch.cat([(torch.arange(P)[None, :] <= pos[:, None]) & ~is_sel[None, :],
                           pos[None, :] <= pos[:, None]], 1)
        o = torch.nn.functional.scaled_dot_product_attention(
            query, key, value, attn_mask=allow, scale=scaling, enable_gqa=True)
        return o.transpose(1, 2).contiguous(), None
    if ctl.mode == "dense" or ctl.L == 0:
        # plain attention over the cache, the new tokens causal among themselves
        allow = torch.arange(N)[None, :] <= (N - Tq + torch.arange(Tq))[:, None]
        o = torch.nn.functional.scaled_dot_product_attention(
            query, key, value, attn_mask=allow, scale=scaling, enable_gqa=True)
        return o.transpose(1, 2).contiguous(), None
    L = ctl.L
    T = N - L
    # the queries are the last Tq of the T tokens after the prefix (a chunk, or one
    # generated token at a time)
    assert 0 < Tq <= T and query.shape[0] == 1, (Tq, N, L)
    off = T - Tq
    scale = scaling if scaling is not None else 1 / math.sqrt(query.shape[-1])
    qf = query[0].float() * scale
    kf, vf = key[0].float(), value[0].float()
    H, d = kf.shape[0], kf.shape[-1]
    g = qf.shape[0] // H
    qf = qf.view(H, g, Tq, d)
    m = build_meta(layer, kf[:, :L].contiguous())
    outs = []
    for t0 in range(0, Tq, ctl.tchunk):
        t1 = min(Tq, t0 + ctl.tchunk)
        qc = qf[:, :, t0:t1]
        S = torch.einsum("hgtd,hnd->hgtn", qc, kf)
        # causal mask inside the chunk: query t (absolute L+t) sees keys <= L+t
        tt = torch.arange(t0, t1).unsqueeze(-1) + off
        jj = torch.arange(T).unsqueeze(0)
        S[..., L:] = S[..., L:].masked_fill((jj > tt), float("-inf"))
        dense_layer = layer in ctl.dense_layers
        if ctl.mode == "stats":
            o_dense, p = _attend(S, None, vf, t0, L)
            outs.append(o_dense)
            _record(layer, qc, m, S, p, vf, o_dense, L)
        else:
            if dense_layer or ctl.mode == "dense":
                o, _ = _attend(S, None, vf, t0, L)
            else:
                probs = torch.softmax(S, -1)[..., :L] if ctl.method.startswith("oracle") else None
                ctl.cur_layer = layer
                sel = select(ctl.method, ctl.budget, qc, m, S[..., :L], probs)
                o, _ = _attend(S, sel, vf, t0, L)
            outs.append(o)
    o = torch.cat(outs, dim=2).reshape(H * g, Tq, d)   # (Hq,Tq,d)
    return o.transpose(0, 1).unsqueeze(0).to(query.dtype).contiguous(), None


def _acc(key, val):
    s = ctl.stats.get(key)
    ctl.stats[key] = val if s is None else s + val


def _record(layer, qc, m, S, p, v, o_dense, L):
    H, g, t, N = S.shape
    P = ctl.page
    p_pre = p[..., :L]
    S_pre = S[..., :L]
    _acc(("prefix_mass", layer), p_pre.sum(-1).sum(-1))          # (H,g)
    _acc(("count", layer), torch.full((H, g), float(t)))
    onorm = o_dense.norm(dim=-1)
    for meth in ctl.stat_methods:
        for B in ctl.stat_budgets:
            sel = select(meth, B, qc, m, S_pre, p_pre)
            rec = (p_pre * sel.unsqueeze(1)).sum(-1) + p[..., L:].sum(-1)
            o, _ = _attend(S, sel, v, 0, L)
            err = (o - o_dense).norm(dim=-1) / onorm.clamp(min=1e-6)
            _acc(("recall", meth, B, layer), rec.sum(-1))
            _acc(("read", meth, B, layer), sel.float().sum(-1).sum(-1).unsqueeze(1).expand(H, g) / L)
            _acc(("oerr", meth, B, layer), err.sum(-1))
    # exact top-k with block-max pruning: a page can be skipped only if its
    # bound is below the k-th best true score. Fraction of pages that must be read.
    npg = L // P
    ub = box_bound(qc, m["pmin"], m["pmax"])                    # per query head (H,g,t,npg)
    pmax = S_pre.view(H, g, t, npg, P).amax(-1)
    cub = box_bound(qc, m["cmin"], m["cmax"])
    cub = cub.masked_fill((m["csize"] == 0)[:, None, None, :], float("-inf"))
    csz = m["csize"].to(torch.float32)[:, None, None, :]
    est = rabitq_est(qc, m)
    width = EPS0 * qc.norm(dim=-1, keepdim=True) * m["rq_e"][:, None, None, :]
    for k in ctl.exact_ks:
        sk = S_pre.topk(k, dim=-1).values[..., -1:]
        _acc(("exact_pages_quest", k, layer), (ub >= sk).float().mean(-1).sum(-1))
        _acc(("exact_pages_ideal", k, layer), (pmax >= sk).float().mean(-1).sum(-1))
        cand = (est + width) >= sk
        _acc(("exact_tokens_rabitq", k, layer), cand.float().mean(-1).sum(-1))
        topk_mask = S_pre >= sk
        _acc(("exact_miss_rabitq", k, layer), ((topk_mask & ~cand).float().sum(-1) / k).sum(-1))
        _acc(("exact_tokens_cluster", k, layer), (((cub >= sk).float() * csz).sum(-1) / L).sum(-1))


AttentionInterface.register("kvsel", kvsel_attention)
