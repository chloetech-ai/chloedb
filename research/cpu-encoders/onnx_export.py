"""Exports a transformer encoder to ONNX (last hidden state) and makes an
int8 copy with ONNX Runtime's dynamic quantisation (weights int8, activations
quantised on the fly: VNNI on x86, dot-product instructions on ARM)."""
import sys, os, torch
from transformers import AutoModel, AutoTokenizer
from onnxruntime.quantization import quantize_dynamic, QuantType
from common import hf

repo, out = sys.argv[1], os.path.expanduser(sys.argv[2])
os.makedirs(out, exist_ok=True)
tok = AutoTokenizer.from_pretrained(hf(repo))
m = AutoModel.from_pretrained(hf(repo), torch_dtype=torch.float32, attn_implementation="eager").eval()
enc = tok(["a first example sentence", "and a second, longer one to pad the batch with"], padding=True, return_tensors="pt")
inputs = (enc["input_ids"], enc["attention_mask"])
torch.onnx.export(m, inputs, f"{out}/model.onnx", input_names=["input_ids", "attention_mask"], output_names=["last_hidden_state"],
                  dynamic_axes={"input_ids": {0: "b", 1: "n"}, "attention_mask": {0: "b", 1: "n"}, "last_hidden_state": {0: "b", 1: "n"}}, opset_version=17, dynamo=False)
quantize_dynamic(f"{out}/model.onnx", f"{out}/model-int8.onnx", weight_type=QuantType.QInt8, per_channel=True)
tok.save_pretrained(out)
print("exported", out, os.path.getsize(f"{out}/model.onnx") >> 20, "MB,", os.path.getsize(f"{out}/model-int8.onnx") >> 20, "MB int8")
