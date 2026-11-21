| Model | Technique | Closed book | With RAG | Notes |
|---|---|---|---|---|
| 0.6B | base model | 2% (5) | 46% (106) | ppl 35.1, WikiText 27.6 |
| 0.6B | ChloeDB rows (4 layers, gated, per doc) | 3% (7) | 49% (113) | ppl 20.4, WikiText 27.0 |
| 0.6B | LoRA fine-tune | 6% (14) | 45% (103) | ppl 16.1, WikiText 41.3 |
| 0.6B | FFN fine-tune (one layer) | 4% (9) | 49% (113) | ppl 20.5, WikiText 30.7 |
| 0.6B | kNN-LM served by ChloeDB | 2% (4) | 46% (106) | lambda 0.1, temp 100.0, 1.07 ms/lookup |
| 0.6B | one LoRA per doc, BM25-routed | 5% (12) | 43% (99) | router right 88% |
| 0.6B |   same, told the right doc | 6% (14) | 43% (99) | ceiling |
| 0.6B | facts as ChloeDB rows | 41% (95) | 45% (104) | matched 230/230 (159 right doc), 2.29 s/fact, delete 0.44->0.25 (base 0.00) |
| 4B | base model | 6% (13) | 60% (139) | ppl 22.5, WikiText 17.9 |
| 4B | ChloeDB rows (4 layers, gated, per doc) | 10% (24) | 60% (138) | ppl 11.1, WikiText 17.6 |
| 4B | LoRA fine-tune | 10% (24) | 59% (136) | ppl 9.7, WikiText 18.2 |
| 4B | FFN fine-tune (one layer) | 7% (15) | 61% (140) | ppl 12.8, WikiText 16.2 |
| 4B | kNN-LM served by ChloeDB | 6% (13) | 60% (139) | lambda 0.1, temp 1000.0, 2.26 ms/lookup |
| 4B | one LoRA per doc, BM25-routed | 9% (20) | 58% (134) | router right 88% |
| 4B |   same, told the right doc | 9% (20) | 58% (134) | ceiling |
| 4B | facts as ChloeDB rows | 44% (102) | 52% (120) | matched 228/230 (176 right doc), 4.06 s/fact, delete 0.50->0.19 (base 0.06) |
| 8B | base model | 7% (15) | 57% (131) | ppl 18.2, WikiText 12.9 |
| 8B | ChloeDB rows (4 layers, gated, per doc) | 12% (27) | 57% (131) | ppl 9.9, WikiText 15.5 |
| 8B | LoRA fine-tune | 14% (33) | 57% (132) | ppl 8.5, WikiText 14.7 |
| 8B | kNN-LM served by ChloeDB | 7% (15) | 57% (131) | lambda 0.1, temp 3000.0, 1.90 ms/lookup |
| 8B | one LoRA per doc, BM25-routed | 11% (25) | 56% (128) | router right 88% |
| 8B |   same, told the right doc | 10% (23) | 56% (129) | ceiling |
| 8B | facts as ChloeDB rows | 41% (95) | 49% (112) | matched 229/230 (171 right doc), 1.5 s/fact, delete 0.44->0.31 (base 0.12) |
