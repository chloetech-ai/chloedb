"""Exports the learned sparse document encoder (doc-v3-distill: DistilBERT
and its MLM head) to ONNX with SPLADE's pooling inside the graph, so the
model's output is the documents' term weights, [batch, vocabulary]:
max over tokens of log(1 + relu(logits)), padding masked. Makes an int8
copy with ONNX Runtime's dynamic quantisation, and copies the tokenizer
and the query side's IDF table (idf.json) beside it: the directory is what
ChloeDB's SparseEncoder opens."""
import sys, os, shutil, torch
from transformers import AutoModelForMaskedLM, AutoTokenizer
from onnxruntime.quantization import quantize_dynamic, QuantType
from common import hf

repo = sys.argv[1] if len(sys.argv) > 1 else "opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill"
out = os.path.expanduser(sys.argv[2] if len(sys.argv) > 2 else "~/onnx/sparse-doc-v3")
os.makedirs(out, exist_ok=True)
path = hf(repo)
tok = AutoTokenizer.from_pretrained(path)
mlm = AutoModelForMaskedLM.from_pretrained(path, torch_dtype=torch.float32, attn_implementation="eager").eval()


class Pooled(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, input_ids, attention_mask):
        logits = self.m(input_ids=input_ids, attention_mask=attention_mask).logits
        w = torch.log1p(torch.relu(logits)) * attention_mask.unsqueeze(-1).to(logits.dtype)
        return w.max(dim=1).values


enc = tok(["a first example sentence", "and a second, longer one to pad the batch with"], padding=True, return_tensors="pt")
torch.onnx.export(Pooled(mlm), (enc["input_ids"], enc["attention_mask"]), f"{out}/model.onnx",
                  input_names=["input_ids", "attention_mask"], output_names=["weights"],
                  dynamic_axes={"input_ids": {0: "b", 1: "n"}, "attention_mask": {0: "b", 1: "n"}, "weights": {0: "b"}},
                  opset_version=17, dynamo=False)
quantize_dynamic(f"{out}/model.onnx", f"{out}/model-int8.onnx", weight_type=QuantType.QInt8, per_channel=True)
tok.save_pretrained(out)
for f in ("idf.json", "config.json"):
    shutil.copy(os.path.join(path, f), out)
print("exported", out, os.path.getsize(f"{out}/model.onnx") >> 20, "MB,", os.path.getsize(f"{out}/model-int8.onnx") >> 20, "MB int8")
