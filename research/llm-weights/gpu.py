"""Run the prototypes on a GPU when there is one, unchanged otherwise.
MODEL (a path or a Hugging Face name) picks the model; the default is the
Qwen3-0.6B the CPU runs used. On CUDA, new tensors go to the GPU and
.numpy() copies to the CPU first, so the scripts' numpy and ChloeDB code
stays as it is."""
import os, torch
from transformers import AutoTokenizer, AutoModelForCausalLM

DEVICE = "cuda" if torch.cuda.is_available() and os.environ.get("DEVICE") != "cpu" else "cpu"
if DEVICE == "cuda":
    _numpy = torch.Tensor.numpy
    def _to_numpy(self, *a, **k): return _numpy(self.cpu() if self.is_cuda else self, *a, **k)
    torch.Tensor.numpy = _to_numpy

def model_path(S): return os.environ.get("MODEL", f"{S}/models/qwen3-0.6b-hf")

def load(S):
    tok = AutoTokenizer.from_pretrained(model_path(S))
    model = AutoModelForCausalLM.from_pretrained(model_path(S), dtype=torch.float32).to(DEVICE).eval()
    if DEVICE == "cuda": torch.set_default_device("cuda")   # after loading: transformers refuses a default device without accelerate
    return tok, model

def generator(seed): return torch.Generator(device=DEVICE).manual_seed(seed)
