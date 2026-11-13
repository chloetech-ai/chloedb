"""Embeds with an ONNX export (onnx_export.py): CLS pooling, batches
sorted by length so that padding is small."""
import os, numpy as np, onnxruntime as ort
from transformers import AutoTokenizer


class OnnxEncoder:
    def __init__(self, d, file="model.onnx", threads=4, maxlen=512):
        o = ort.SessionOptions(); o.intra_op_num_threads = threads; o.inter_op_num_threads = 1
        o.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        self.s = ort.InferenceSession(os.path.join(d, file), o, providers=["CPUExecutionProvider"])
        self.tok = AutoTokenizer.from_pretrained(d); self.maxlen = maxlen

    def encode(self, texts, bs=32):
        order = np.argsort([len(t) for t in texts]); out = [None] * len(texts)
        for i in range(0, len(texts), bs):
            idx = order[i:i + bs]
            e = self.tok([texts[j] for j in idx], padding=True, truncation=True, max_length=self.maxlen, return_tensors="np")
            h = self.s.run(None, {"input_ids": e["input_ids"].astype(np.int64), "attention_mask": e["attention_mask"].astype(np.int64)})[0][:, 0]
            h = h / np.linalg.norm(h, axis=1, keepdims=True)
            for j, v in zip(idx, h):
                out[j] = v
        return np.stack(out)
