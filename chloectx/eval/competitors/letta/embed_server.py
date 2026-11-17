#!/usr/bin/env python3
"""A local OpenAI-compatible embeddings endpoint for the Letta lane:
POST /v1/embeddings with BAAI/bge-small-en-v1.5 (384 dimensions) through
fastembed on the CPU, the embedder the Graphiti and Hindsight lanes use.
Letta's folders embed their files' chunks through it
(embedding_endpoint_type "openai", embedding_endpoint http://127.0.0.1:8090/v1).

  letta-venv/bin/python -I eval/competitors/letta/embed_server.py --port 8090
"""

import argparse
import threading

import uvicorn
from fastapi import FastAPI, Request
from fastembed import TextEmbedding

app = FastAPI()
MODEL = {}
LOCK = threading.Lock()


@app.post("/v1/embeddings")
async def embeddings(req: Request):
    body = await req.json()
    texts = body.get("input")
    if isinstance(texts, str):
        texts = [texts]
    texts = [t if isinstance(t, str) else " ".join(map(str, t)) for t in texts]
    with LOCK:
        vecs = [v.tolist() for v in MODEL["m"].embed(texts, batch_size=32)]
    return {
        "object": "list",
        "model": body.get("model", MODEL["name"]),
        "data": [{"object": "embedding", "index": i, "embedding": v} for i, v in enumerate(vecs)],
        "usage": {"prompt_tokens": sum(len(t) // 4 for t in texts), "total_tokens": sum(len(t) // 4 for t in texts)},
    }


@app.get("/v1/models")
async def models():
    return {"object": "list", "data": [{"id": MODEL["name"], "object": "model"}]}


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8090)
    ap.add_argument("--model", default="BAAI/bge-small-en-v1.5")
    ap.add_argument("--threads", type=int, default=3)
    args = ap.parse_args()
    MODEL["name"] = args.model
    MODEL["m"] = TextEmbedding(args.model, threads=args.threads)
    uvicorn.run(app, host="127.0.0.1", port=args.port, log_level="warning")
