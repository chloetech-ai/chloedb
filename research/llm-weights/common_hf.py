"""Shared by W4 to W6: Qwen3-0.6B in transformers (fp32, CPU), the eval
segments used by B and D (16 ChloeDB docs, 8 strides of 64 tokens after 128
tokens of context), a LoRA that can hold several adapters at once with
weights, and the ChloeDB server's BM25 over doc_chunks."""
import os, math, base64, requests, numpy as np, torch
from transformers import AutoTokenizer, AutoModelForCausalLM

S = os.environ["S"]; URL = "http://127.0.0.1:7700"
torch.set_num_threads(int(os.environ.get("THREADS", 4)))
from gpu import load, generator, DEVICE
tok, model = load(S)
for p in model.parameters(): p.requires_grad_(False)
EMB = model.get_input_embeddings()
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
CTX, STRIDE, START, END = 128, 64, 128, 128 + 8 * 64
DOCS = "/home/user/chloedb/docs"

def toks(path): return tok(open(path).read(), add_special_tokens=False)["input_ids"]
def segments():
    for e in EVAL:
        ids = toks(f"{DOCS}/chloedb/{e}.md")
        for s in range(START, END, STRIDE):
            yield e, s, ids[s - CTX:s], ids[s:s + STRIDE]

def nll(prefix_embeds, ctx, tgt, grad=False):
    """Mean NLL of tgt after [prefix_embeds] + ctx. prefix_embeds: (n, d) tensor or None."""
    ids = torch.tensor([ctx + tgt]); e = EMB(ids)
    if prefix_embeds is not None and len(prefix_embeds): e = torch.cat([prefix_embeds.unsqueeze(0), e], 1)
    with torch.set_grad_enabled(grad):
        lg = model(inputs_embeds=e).logits[0]
        n = len(tgt); lp = torch.log_softmax(lg[-n - 1:-1].float(), -1)
        return -lp[torch.arange(n), torch.tensor(tgt)].mean()

def text_embeds(text, max_tokens=160):
    return EMB(torch.tensor([tok((text + "\n\n"), add_special_tokens=False)["input_ids"][:max_tokens]]))[0]

def mean_hidden(ids):
    with torch.no_grad():
        return model(torch.tensor([ids]), output_hidden_states=True).hidden_states[-1][0].mean(0)

def bm25(q, doc, k=1):
    r = requests.post(URL + "/v1/sql", json={"sql": f"SELECT id, body, score() FROM doc_chunks WHERE doc != :d ORDER BY bm25(body, :q) DESC LIMIT {k}",
                                           "params": {"d": doc, "q": q}}).json()
    return r["rows"]

class LoRA(torch.nn.Module):
    """y = W x + sum_i w_i * B_i A_i x. Adapters are held by name; `active` says which, with what weight."""
    def __init__(self, base, r=8, alpha=16):
        super().__init__(); self.base, self.r, self.scale = base, r, alpha / r
        self.adapters, self.active, self.train_name = {}, [], None
    def new(self, name):
        A = torch.nn.Parameter(torch.randn(self.r, self.base.in_features) * 0.01)
        B = torch.nn.Parameter(torch.zeros(self.base.out_features, self.r))
        self.adapters[name] = (A, B); return [A, B]
    def forward(self, x):
        y = self.base(x)
        for name, w in self.active:
            A, B = self.adapters[name]; y = y + w * self.scale * (x @ A.t()) @ B.t()
        return y

LORAS = []
def add_lora(r=8):
    if LORAS: return LORAS
    for layer in model.model.layers:
        for parent, attr in ((layer.self_attn, "o_proj"), (layer.mlp, "down_proj")):
            m = LoRA(getattr(parent, attr), r); setattr(parent, attr, m); LORAS.append(m)
    return LORAS
def set_active(pairs):
    for m in LORAS: m.active = list(pairs)
def pack_adapter(name):
    flat = torch.cat([t.detach().flatten() for m in LORAS for t in m.adapters[name]]).numpy().astype(np.float16)
    return base64.b64encode(flat.tobytes()).decode()
def unpack_adapter(name, blob):
    flat = torch.tensor(np.frombuffer(base64.b64decode(blob), dtype=np.float16).astype(np.float32)); i = 0
    for m in LORAS:
        A = torch.empty(m.r, m.base.in_features); B = torch.empty(m.base.out_features, m.r)
        for t in (A, B): n = t.numel(); t.copy_(flat[i:i + n].view_as(t)); i += n
        m.adapters[name] = (A, B)
def ppl(xs): return math.exp(float(np.mean(xs)))
