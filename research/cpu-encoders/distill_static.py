"""Step 1 of a static student: Model2Vec's distillation of a transformer
(each vocabulary token through the teacher alone, pooled as the teacher
pools, SIF-weighted). No PCA, so the table stays in the teacher's
dimensions, the start of a student in the teacher's space."""
import sys, os
from model2vec.distill import distill
from common import hf

repo, out, pooling = sys.argv[1], sys.argv[2], (sys.argv[3] if len(sys.argv) > 3 else "first")
m = distill(model_name=hf(repo), pca_dims=None, pooling=pooling, quantize_to="float32", trust_remote_code=True)
m.save_pretrained(out)
print("saved", out, m.embedding.shape)
