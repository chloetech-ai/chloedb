"""Use 2: chloedb-server as a shared KV-cache tier.

One worker prefills a long prefix and writes its KV cache to chloedb-server, one
row per layer whose MEDIA field holds the layer's K and V bytes (kept in the
server's media store, read back raw from /v1/media/<sha256>). A second worker,
in another process, finds the rows by the prefix's hash, fetches the bytes,
rebuilds the cache and answers without prefilling. Both answer the same
question; the answers must be the same.

  python run_tier.py put --tokens 8192 --server http://127.0.0.1:7700 --out results/tier.jsonl
  python run_tier.py get --tokens 8192 --server http://127.0.0.1:7700 --out results/tier.jsonl
"""
import argparse, base64, hashlib, json, os, time
import torch, requests, pandas as pd
from transformers import AutoModelForCausalLM, AutoTokenizer, DynamicCache
import kvsel
from kvsel import ctl
from kvrag import Runner

ap = argparse.ArgumentParser()
ap.add_argument("mode", choices=["put", "get", "local"])
ap.add_argument("--model", default="Qwen/Qwen2.5-1.5B-Instruct")
ap.add_argument("--tokens", type=int, default=8192)
ap.add_argument("--data", default="data/data/test-00000-of-00001-29a571947c0b5ccc.parquet")
ap.add_argument("--server", default="http://127.0.0.1:7700")
ap.add_argument("--local-dir", default="/tmp/kvtier")
ap.add_argument("--out", required=True)
args = ap.parse_args()

torch.set_num_threads(os.cpu_count())
tok = AutoTokenizer.from_pretrained(args.model)
model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.bfloat16, attn_implementation="kvsel").eval()
R = Runner(model, tok)

# the shared prefix: a system prompt and a long document with one fact in it
book = pd.read_parquet(args.data).text.iloc[3]
head = R.ids("<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n")
needle = R.ids(" The access code for the archive room is 48213. ")
body = R.ids(book[: (args.tokens + 2000) * 6])[1000:]
n = args.tokens - head.numel() - needle.numel()
prefix = torch.cat([head, body[: n // 2], needle, body[n // 2: n]])
assert prefix.numel() == args.tokens
tail = R.ids("\n\nWhat is the access code for the archive room? Answer with the number only.<|im_end|>\n<|im_start|>assistant\n")
pkey = hashlib.sha256((args.model + "|" + ",".join(map(str, prefix.tolist()))).encode()).hexdigest()[:32]


def first_token_and_answer(cache):
    t = time.time()
    with torch.no_grad():
        out = model(input_ids=tail[None], position_ids=torch.arange(args.tokens, args.tokens + tail.numel())[None],
                    past_key_values=cache, use_cache=True)
    ttft_tail = time.time() - t
    cache.crop(-tail.numel())
    ans = R.answer(cache, args.tokens, tail, max_new=8)
    return ttft_tail, int(out.logits[0, -1].argmax()), ans


def to_bytes(k, v):
    return k.contiguous().view(torch.int16).numpy().tobytes() + v.contiguous().view(torch.int16).numpy().tobytes()


def from_bytes(b, shape):
    half = len(b) // 2
    k = torch.frombuffer(bytearray(b[:half]), dtype=torch.int16).view(torch.bfloat16).view(shape)
    v = torch.frombuffer(bytearray(b[half:]), dtype=torch.int16).view(torch.bfloat16).view(shape)
    return k, v


rec = {"mode": args.mode, "tokens": args.tokens, "model": args.model, "prefix": pkey}
nl = model.config.num_hidden_layers
shape = (1, model.config.num_key_value_heads, args.tokens, model.config.hidden_size // model.config.num_attention_heads)

if args.mode == "put":
    t = time.time()
    cache = R.prefill(prefix)
    rec["prefill_s"] = time.time() - t
    rec["tail_s"], rec["first_token"], rec["answer"] = first_token_and_answer(cache)
    rec["ttft_cold_s"] = rec["prefill_s"] + rec["tail_s"]
    try:
        requests.post(f"{args.server}/v1/sql", json={"sql": "CREATE TABLE kvcache (id KEYWORD PRIMARY KEY, model KEYWORD, prefix KEYWORD, layer INT, tokens INT, blob MEDIA)"}, timeout=60).raise_for_status()
    except requests.HTTPError:
        pass
    t = time.time()
    nbytes = 0
    for li, (k, v) in enumerate(R.layers(cache)):
        b = to_bytes(k, v)
        nbytes += len(b)
        doc = {"id": f"{pkey}/{li}", "model": args.model, "prefix": pkey, "layer": li, "tokens": args.tokens,
               "blob": {"data": base64.b64encode(b).decode()}}
        requests.post(f"{args.server}/v1/write", json={"ops": [{"op": "upsert", "table": "kvcache", "doc": doc}]}, timeout=300).raise_for_status()
    rec["put_s"] = time.time() - t
    rec["bytes"] = nbytes
    # the same bytes to a local file, for comparison
    os.makedirs(args.local_dir, exist_ok=True)
    t = time.time()
    with open(os.path.join(args.local_dir, pkey), "wb") as f:
        for k, v in R.layers(cache):
            f.write(to_bytes(k, v))
    rec["save_local_s"] = time.time() - t

elif args.mode == "get":
    t = time.time()
    r = requests.post(f"{args.server}/v1/kv/get", json={"table": "kvcache", "keys": [f"{pkey}/{li}" for li in range(nl)],
                                                        "columns": ["layer", "blob"]}, timeout=60)
    r.raise_for_status()
    rows = r.json()["rows"]
    rec["lookup_s"] = time.time() - t
    assert all(rows), "a layer is missing"
    t = time.time()
    s = requests.Session()
    blobs = []
    for row in rows:
        blob = row["blob"] if isinstance(row["blob"], dict) else json.loads(row["blob"])
        sha = blob["sha256"]
        blobs.append(s.get(f"{args.server}/v1/media/{sha}", timeout=300).content)
    rec["fetch_s"] = time.time() - t
    rec["bytes"] = sum(len(b) for b in blobs)
    t = time.time()
    cache = DynamicCache()
    for li, b in enumerate(blobs):
        k, v = from_bytes(b, shape)
        cache.update(k, v, li)
    rec["deserialize_s"] = time.time() - t
    rec["tail_s"], rec["first_token"], rec["answer"] = first_token_and_answer(cache)
    rec["ttft_warm_s"] = rec["lookup_s"] + rec["fetch_s"] + rec["deserialize_s"] + rec["tail_s"]

else:  # local file
    t = time.time()
    with open(os.path.join(args.local_dir, pkey), "rb") as f:
        data = f.read()
    rec["read_s"] = time.time() - t
    per = len(data) // nl
    t = time.time()
    cache = DynamicCache()
    for li in range(nl):
        k, v = from_bytes(data[li * per:(li + 1) * per], shape)
        cache.update(k, v, li)
    rec["deserialize_s"] = time.time() - t
    rec["tail_s"], rec["first_token"], rec["answer"] = first_token_and_answer(cache)
    rec["ttft_local_s"] = rec["read_s"] + rec["deserialize_s"] + rec["tail_s"]

print(json.dumps(rec), flush=True)
with open(args.out, "a") as f:
    f.write(json.dumps(rec) + "\n")
