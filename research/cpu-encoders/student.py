"""A static student in its teacher's space (fastText-style): the vector of a
text is the mean of its tokens' rows, plus optionally rows of hashed token
bigrams (fastText's buckets), trained so that it points where the teacher's
vector points (cosine loss, as tokenlearn). Starts from Model2Vec's
distillation of the teacher (distill_static.py).

Training texts: C4 passages and spans (prep_corpus.py, teacher_embed.py),
and with --domain the collections' own documents and spans of them, the
texts a database holds (no queries and no judgments are used).

Evaluates on BEIR: the student on both sides, and asymmetric pairs with
the teacher (teacher documents and student queries, and the reverse)."""
import argparse, json, os, random, sys, time
import numpy as np, torch, torch.nn as nn
from model2vec import StaticModel
from common import load, evaluate, unit, cached, hf

p = argparse.ArgumentParser()
p.add_argument("--init", required=True)
p.add_argument("--teacher", default="ibm-granite/granite-embedding-small-english-r2")
p.add_argument("--teacher-name", default="granite-small-r2")
p.add_argument("--train", default=os.path.expanduser("~/corpus/c4-train.json"))
p.add_argument("--domain", default="")
p.add_argument("--bigrams", type=int, default=0, help="hashed bigram buckets (0: none)")
p.add_argument("--epochs", type=int, default=30)
p.add_argument("--lr", type=float, default=0.02)
p.add_argument("--batch", type=int, default=256)
p.add_argument("--no-c4", action="store_true")
p.add_argument("--fit-projection", action="store_true", help="map the init table into the teacher's space by least squares first (for a static model of another space, such as potion)")
p.add_argument("--out", default="")
p.add_argument("--eval", default="scifact,nfcorpus,arguana")
a = p.parse_args()
torch.manual_seed(0); random.seed(0)
torch.set_num_threads(int(os.environ.get("THREADS", "4")))

sm = StaticModel.from_pretrained(os.path.expanduser(a.init))
tok = sm.tokenizer
E0 = torch.tensor(np.asarray(sm.embedding, dtype=np.float32))
V, d = E0.shape
MAXTOK = 512


def ids_of(texts):
    return [e.ids[:MAXTOK] for e in tok.encode_batch(texts, add_special_tokens=False)]


def bigram_ids(ids):
    return [((x * 1000003 + y) % a.bigrams) for x, y in zip(ids, ids[1:])] if a.bigrams else []


class Student(nn.Module):
    def __init__(self):
        super().__init__()
        self.E = nn.EmbeddingBag(V, d, mode="sum", sparse=True)
        self.E.weight.data.copy_(E0)
        if a.bigrams:
            self.G = nn.EmbeddingBag(a.bigrams, d, mode="sum", sparse=True)
            self.G.weight.data.zero_()

    def forward(self, batch):
        flat = torch.tensor([t for ids in batch for t in ids], dtype=torch.long)
        offs = torch.tensor(np.cumsum([0] + [len(x) for x in batch[:-1]]), dtype=torch.long)
        n = torch.tensor([max(1, len(x)) for x in batch], dtype=torch.float32)[:, None]
        v = self.E(flat, offs)
        if a.bigrams:
            bg = [bigram_ids(x) for x in batch]
            bflat = torch.tensor([t for b in bg for t in b], dtype=torch.long)
            boffs = torch.tensor(np.cumsum([0] + [len(x) for x in bg[:-1]]), dtype=torch.long)
            v = v + self.G(bflat, boffs)
        return v / n


def embed(model, texts, bs=2048):
    out = []
    with torch.no_grad():
        for i in range(0, len(texts), bs):
            out.append(model(ids_of(texts[i:i + bs])).numpy())
    return unit(np.concatenate(out))


# ---- training data
texts, Y = [], []
if not a.no_c4:
    t = json.load(open(os.path.expanduser(a.train)))
    for part in ("spans", "passages"):
        X, _ = cached(f"teacher|{a.teacher}|160|{a.train}|{part}", lambda: (_ for _ in ()).throw(SystemExit(f"run teacher_embed.py first ({part})")))
        texts += t[part]; Y.append(X)
teacher_model = None
def teacher():
    global teacher_model
    if teacher_model is None:
        from sentence_transformers import SentenceTransformer
        teacher_model = SentenceTransformer(hf(a.teacher), device="cpu", model_kwargs={"torch_dtype": getattr(torch, os.environ.get("DTYPE", "bfloat16"))}); teacher_model.max_seq_length = 64
    return teacher_model
for ds in [x for x in a.domain.split(",") if x]:
    ids, dtexts, *_ = load(ds)
    D, _ = cached(f"{a.teacher_name}|512|{ds}|docs", lambda: (_ for _ in ()).throw(SystemExit(f"run baselines.py {a.teacher_name} {ds} first")))
    rnd = random.Random(1)
    spans = []
    for x in dtexts:
        w = x.split()
        for _ in range(2):
            k = rnd.randint(3, 16); s = rnd.randrange(0, max(1, len(w) - k)); spans.append(" ".join(w[s:s + k]))
    S, _ = cached(f"teacher-spans|{a.teacher}|64|{ds}", lambda: unit(teacher().encode(spans, batch_size=128, convert_to_numpy=True, normalize_embeddings=True)))
    texts += dtexts + spans; Y += [D, S]
Y = torch.tensor(np.concatenate(Y))
print(f"training on {len(texts)} texts; bigrams {a.bigrams}", file=sys.stderr)
tids = ids_of(texts)
order = list(range(len(texts))); random.shuffle(order)
val, tr = order[:2000], order[2000:]

if a.fit_projection:
    # mean of rows of each training text (a bag of tokens), regressed onto the teacher
    import scipy.sparse as sps
    r, c = [], []
    for i, x in enumerate(tids):
        r += [i] * len(x); c += x
    A = sps.csr_matrix((np.ones(len(r), np.float32), (r, c)), shape=(len(tids), V))
    A = sps.diags(1 / np.maximum(1, np.asarray(A.sum(1)).ravel())) @ A
    X = A @ E0.numpy()
    W, *_ = np.linalg.lstsq(X, Y.numpy(), rcond=None)
    E0 = torch.tensor(E0.numpy() @ W)
    d = E0.shape[1]
    print(f"projected the table to the teacher's {d} dimensions", file=sys.stderr)
model = Student()
opt = torch.optim.SparseAdam(list(model.parameters()), lr=a.lr)


def val_loss():
    with torch.no_grad():
        v = model([tids[i] for i in val])
        return float((1 - nn.functional.cosine_similarity(v, Y[val])).mean())


best, best_state = val_loss(), {k: x.clone() for k, x in model.state_dict().items()}
print(f"epoch 0 val {best:.4f}", file=sys.stderr)
for ep in range(1, a.epochs + 1):
    random.shuffle(tr); t0 = time.time()
    for i in range(0, len(tr), a.batch):
        b = tr[i:i + a.batch]
        loss = (1 - nn.functional.cosine_similarity(model([tids[j] for j in b]), Y[b])).mean()
        opt.zero_grad(); loss.backward(); opt.step()
    vl = val_loss()
    print(f"epoch {ep} val {vl:.4f} ({time.time()-t0:.0f} s)", file=sys.stderr)
    if vl < best:
        best, best_state = vl, {k: x.clone() for k, x in model.state_dict().items()}
    else:
        break
model.load_state_dict(best_state)

tag = os.path.basename(a.out) if a.out else f"student{'-bi' if a.bigrams else ''}{'-dom' if a.domain else ''}"
for ds in a.eval.split(","):
    ids, dtexts, qids, qtexts, qrels = load(ds)
    Ds, Qs = embed(model, dtexts), embed(model, qtexts)
    Dt, _ = cached(f"{a.teacher_name}|512|{ds}|docs", lambda: None)
    Qt, _ = cached(f"{a.teacher_name}|512|{ds}|queries", lambda: None)
    for pair, (D_, Q_) in {"student/student": (Ds, Qs), "teacher docs/student queries": (Dt, Qs), "student docs/teacher queries": (Ds, Qt)}.items():
        nd, rc = evaluate(ids, D_, qids, Q_, qrels)
        print(json.dumps({"student": tag, "dataset": ds, "pair": pair, "ndcg10": round(nd, 4), "recall100": round(rc, 4)}), flush=True)

if a.out:
    from safetensors.numpy import save_file
    os.makedirs(a.out, exist_ok=True)
    sm.save_pretrained(a.out)
    tensors = {"embeddings": model.E.weight.detach().numpy().astype(np.float32)}
    if a.bigrams:
        tensors["bigrams"] = model.G.weight.detach().numpy().astype(np.float32)
    save_file(tensors, os.path.join(a.out, "model.safetensors"))
    print("saved", a.out, file=sys.stderr)
