"""Stored document KV caches for question answering (use 1 and 3 of the write-up).

Every document's KV cache is computed once, on its own, after a shared header
(the system prompt and the instruction), and kept. A question is answered by
stitching the cached documents after the header, each key rotated to its new
position (RoPE rotations compose, so moving a cached key by d positions is
one more rotation by d), and running only the question and the answer. The
documents never attended to each other, which is what this measures, against:

  closed       no documents (does the context matter at all)
  full         header + every document + question prefilled together: what
               re-reading the text on every question costs and scores
  full_sparse  full, then sparse retrieval attention (clusters, ~6% of the prefix)
  stitch       the stored per-document caches stitched, dense attention
  stitch_sparse  stitched, sparse retrieval attention
  bm25_text    ChloeDB BM25 picks the top documents; their text is prefilled
  bm25_stitch  the same documents, from their stored caches

Scored with LongBench's token F1 (and exact match).
"""
import json, math, os, re, string, time, collections
import torch
from transformers import DynamicCache
from transformers.models.qwen2.modeling_qwen2 import rotate_half
import kvsel
from kvsel import ctl


class Runner:
    def __init__(self, model, tok, page=16):
        self.model, self.tok = model, tok
        self.rotary = model.model.rotary_emb
        self.eos = {tok.convert_tokens_to_ids("<|im_end|>"), tok.eos_token_id}
        ctl.page = page
        self.metas = {}

    def ids(self, text):
        return self.tok(text, return_tensors="pt", add_special_tokens=False).input_ids[0]

    # ------------------------------------------------------------ caches
    @torch.no_grad()
    def prefill(self, ids, cache=None, start=0):
        """Dense prefill of ids at positions start.. into cache; returns the cache."""
        ctl.mode, ctl.L = "dense", 0
        cache = cache if cache is not None else DynamicCache()
        n = ids.numel()
        self.model(input_ids=ids[None], position_ids=torch.arange(start, start + n)[None],
                   past_key_values=cache, use_cache=True, logits_to_keep=1)
        return cache

    @staticmethod
    def layers(cache):
        return [(l.keys, l.values) for l in cache.layers]

    def doc_cache(self, header_ids, doc_ids):
        """A document's cache, computed after the header (which it attends to, as a
        sink and for the instruction), with the header's part cut off. Its keys are
        rotated for positions h.. where h = len(header)."""
        c = self.prefill(torch.cat([header_ids, doc_ids]))
        h = header_ids.numel()
        return [(k[:, :, h:].clone(), v[:, :, h:].clone()) for k, v in self.layers(c)]

    def shift(self, k, d):
        """Rotate cached keys (1,H,n,dim) by d more positions."""
        if d == 0:
            return k
        cos, sin = self.rotary(k.float(), torch.tensor([[d]]))
        cos, sin = cos[:, None], sin[:, None]
        kf = k.float()
        return (kf * cos + rotate_half(kf) * sin).to(k.dtype)

    def stitch(self, header_layers, docs):
        """header_layers: the header's own cache; docs: list of per-doc caches (each
        rotated for positions starting at h). Returns a DynamicCache and its length."""
        h = header_layers[0][0].shape[2]
        cache = DynamicCache()
        nl = len(header_layers)
        pos = h
        parts = [[(k, v)] for k, v in header_layers]
        for dc in docs:
            n = dc[0][0].shape[2]
            for li in range(nl):
                k, v = dc[li]
                parts[li].append((self.shift(k, pos - h), v))
            pos += n
        for li in range(nl):
            cache.update(torch.cat([k for k, _ in parts[li]], 2), torch.cat([v for _, v in parts[li]], 2), li)
        return cache, pos

    # ------------------------------------------------------------ blending
    @torch.no_grad()
    def first_layers(self, ids, n=2):
        """A fresh, joint prefill of ids through the first n layers only (n/L of a
        full prefill): the reference CacheBlend compares the stitched cache to."""
        full = self.model.model.layers
        self.model.model.layers = torch.nn.ModuleList(list(full)[:n])
        try:
            c = self.prefill(ids)
        finally:
            self.model.model.layers = full
        return self.layers(c)

    @staticmethod
    def deviation(cache, fresh, layer=1):
        """Per prefix token, how far the stitched K and V of a layer are from the
        fresh joint ones (squared L2 over heads, relative to the fresh norm)."""
        k, v = cache.layers[layer].keys.float(), cache.layers[layer].values.float()
        fk, fv = fresh[layer][0].float(), fresh[layer][1].float()
        num = ((k - fk) ** 2).sum((0, 1, 3)) + ((v - fv) ** 2).sum((0, 1, 3))
        den = (fk ** 2).sum((0, 1, 3)) + (fv ** 2).sum((0, 1, 3))
        return num / den.clamp(min=1e-6)

    @torch.no_grad()
    def blend(self, cache, ids, pos, fresh=None):
        """Recompute the stitched prefix's tokens at positions pos (sorted) through
        every layer, attending to the rest of the stitched cache, and write their
        fresh K and V back in place. With fresh (first_layers' output), those
        layers are first replaced wholesale, as they are exact."""
        P = cache.layers[0].keys.shape[2]
        if fresh is not None:
            for li, (k, v) in enumerate(fresh):
                cache.layers[li].keys, cache.layers[li].values = k.clone(), v.clone()
        pos = torch.as_tensor(pos).sort().values
        ctl.mode, ctl.L, ctl.blend_pos = "blend", 0, pos
        try:
            self.model(input_ids=ids[pos][None], position_ids=pos[None],
                       past_key_values=cache, use_cache=True, logits_to_keep=1)
        finally:
            ctl.mode, ctl.blend_pos = "dense", None
        for l in cache.layers:
            k, v = l.keys, l.values
            k[:, :, pos] = k[:, :, P:]
            v[:, :, pos] = v[:, :, P:]
            l.keys, l.values = k[:, :, :P].contiguous(), v[:, :, :P].contiguous()
        return cache

    # ------------------------------------------------------------ answering
    @torch.no_grad()
    def answer(self, cache, P, tail_ids, mode="dense", method=None, frac=0.0625, max_new=32, meta_key=None):
        """Run tail_ids after a P-token cached prefix and decode greedily. The prefix
        is read sparsely (mode='sparse') at about frac of its tokens per KV head."""
        Lp = (P // ctl.page) * ctl.page
        ctl.reset_prefix(Lp)
        if meta_key is not None:
            # the same stored prefix keeps its index, built once as a store would
            ctl.meta = self.metas.setdefault(meta_key, {})
        ctl.mode, ctl.method = mode, method
        ctl.budget = max(4 * ctl.page, int(round(frac * Lp / ctl.page)) * ctl.page)
        n = tail_ids.numel()
        out = self.model(input_ids=tail_ids[None], position_ids=torch.arange(P, P + n)[None],
                         past_key_values=cache, use_cache=True)
        gen = []
        nxt = int(out.logits[0, -1].argmax())
        pos = P + n
        while nxt not in self.eos and len(gen) < max_new:
            gen.append(nxt)
            out = self.model(input_ids=torch.tensor([[nxt]]), position_ids=torch.tensor([[pos]]),
                             past_key_values=cache, use_cache=True)
            nxt = int(out.logits[0, -1].argmax())
            pos += 1
        cache.crop(-(pos - P))
        ctl.mode, ctl.L = "dense", 0
        return self.tok.decode(gen, skip_special_tokens=True).strip()


# ---------------------------------------------------------------- scoring (LongBench)

def normalize(s):
    s = s.lower()
    s = "".join(ch for ch in s if ch not in set(string.punctuation))
    s = re.sub(r"\b(a|an|the)\b", " ", s)
    return " ".join(s.split())


def f1(pred, gold):
    p, g = normalize(pred).split(), normalize(gold).split()
    common = collections.Counter(p) & collections.Counter(g)
    same = sum(common.values())
    if same == 0:
        return 0.0
    pr, rc = same / len(p), same / len(g)
    return 2 * pr * rc / (pr + rc)


def score(pred, answers):
    return max(f1(pred, a) for a in answers), max(float(normalize(pred) == normalize(a)) for a in answers)
