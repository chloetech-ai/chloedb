| reader | context | n | correct | tokens read | $ reader / q | $ memory / q | $ to store a person | reader s (p50) |
|---|---|---|---|---|---|---|---|---|
| haiku | No memory | 100 | 9% | 514 | 0.00018 | 0.00000 | 0.000 | 1.5 |
| haiku | Plain memory file | 100 | 45% | 2,691 | 0.00085 | 0.00000 | 0.131 | 3.0 |
| haiku | mem0 | 100 | 81% | 2,197 | 0.00074 | 0.00000 | 0.128 | 2.8 |
| haiku | Hindsight, small context | 100 | 71% | 2,702 | 0.00089 | 0.00000 | 1.000 | 3.1 |
| haiku | ChloeCtx, search | 100 | 68% | 1,831 | 0.00070 | 0.00000 | 0.000 | 3.1 |
| haiku | ChloeCtx, query engine | 100 | 84% | 2,159 | 0.00077 | 0.00115 | 0.000 | 3.1 |
| haiku | ChloeCtx, query engine, 16,000 characters | 100 | 84% | 6,018 | 0.00158 | 0.00115 | 0.000 | 3.3 |
| haiku | ChloeCtx, query engine, 48,000 characters | 100 | 90% | 13,881 | 0.00317 | 0.00115 | 0.000 | 3.5 |
| haiku | Hindsight | 100 | 88% | 69,862 | 0.01436 | 0.00000 | 1.000 | 3.6 |
| haiku | Whole history in context | 100 | 88% | 168,447 | 0.17038 | 0.00000 | 0.000 | 4.1 |
| sonnet | No memory | 100 | 8% | 493 | 0.00322 | 0.00000 | 0.000 | 2.0 |
| sonnet | Plain memory file | 100 | 49% | 2,670 | 0.01380 | 0.00000 | 0.131 | 2.4 |
| sonnet | mem0 | 100 | 88% | 2,176 | 0.01199 | 0.00000 | 0.128 | 2.2 |
| sonnet | Hindsight, small context | 100 | 81% | 2,681 | 0.01413 | 0.00000 | 1.000 | 2.4 |
| sonnet | ChloeCtx, search | 100 | 75% | 1,810 | 0.01031 | 0.00000 | 0.000 | 2.3 |
| sonnet | ChloeCtx, query engine | 100 | 87% | 2,138 | 0.01162 | 0.00115 | 0.000 | 2.3 |
| sonnet | ChloeCtx, query engine, 16,000 characters | 100 | 93% | 5,997 | 0.02731 | 0.00115 | 0.000 | 2.2 |
| sonnet | ChloeCtx, query engine, 48,000 characters | 100 | 94% | 13,860 | 0.05894 | 0.00115 | 0.000 | 2.4 |
| sonnet | Hindsight | 100 | 92% | 69,841 | 0.28330 | 0.00000 | 1.000 | 2.8 |
| sonnet | Whole history in context | 25 | 96% | 168,879 | 0.54821 | 0.00000 | 0.000 | 3.6 |
| sonnet | No memory (same 25) | 25 | 16% | | | | | |
| sonnet | Plain memory file (same 25) | 25 | 60% | | | | | |
| sonnet | mem0 (same 25) | 25 | 88% | | | | | |
| sonnet | Hindsight, small context (same 25) | 25 | 84% | | | | | |
| sonnet | ChloeCtx, search (same 25) | 25 | 72% | | | | | |
| sonnet | ChloeCtx, query engine (same 25) | 25 | 88% | | | | | |
| sonnet | ChloeCtx, query engine, 16,000 characters (same 25) | 25 | 96% | | | | | |
| sonnet | ChloeCtx, query engine, 48,000 characters (same 25) | 25 | 88% | | | | | |
| sonnet | Hindsight (same 25) | 25 | 96% | | | | | |
| opus | No memory | 100 | 9% | 489 | 0.00751 | 0.00000 | 0.000 | 3.1 |
| opus | Plain memory file | 100 | 50% | 2,666 | 0.02903 | 0.00000 | 0.131 | 4.4 |
| opus | mem0 | 100 | 88% | 2,172 | 0.02501 | 0.00000 | 0.128 | 4.1 |
| opus | Hindsight, small context | 100 | 84% | 2,677 | 0.02990 | 0.00000 | 1.000 | 4.5 |
| opus | ChloeCtx, search | 100 | 80% | 1,806 | 0.02190 | 0.00000 | 0.000 | 3.7 |
| opus | ChloeCtx, query engine | 100 | 89% | 2,134 | 0.02418 | 0.00115 | 0.000 | 3.8 |
| opus | ChloeCtx, query engine, 16,000 characters | 100 | 95% | 5,993 | 0.05557 | 0.00115 | 0.000 | 4.2 |
| opus | ChloeCtx, query engine, 48,000 characters | 100 | 99% | 13,856 | 0.11830 | 0.00115 | 0.000 | 4.3 |
| opus | Hindsight | 100 | 98% | 69,837 | 0.56707 | 0.00000 | 1.000 | 5.6 |
| opus | Whole history in context | 25 | 100% | 168,875 | 1.09620 | 0.00000 | 0.000 | 7.6 |
| opus | No memory (same 25) | 25 | 16% | | | | | |
| opus | Plain memory file (same 25) | 25 | 56% | | | | | |
| opus | mem0 (same 25) | 25 | 92% | | | | | |
| opus | Hindsight, small context (same 25) | 25 | 88% | | | | | |
| opus | ChloeCtx, search (same 25) | 25 | 84% | | | | | |
| opus | ChloeCtx, query engine (same 25) | 25 | 92% | | | | | |
| opus | ChloeCtx, query engine, 16,000 characters (same 25) | 25 | 96% | | | | | |
| opus | ChloeCtx, query engine, 48,000 characters (same 25) | 25 | 100% | | | | | |
| opus | Hindsight (same 25) | 25 | 100% | | | | | |

models: ['claude-haiku-5-5', 'claude-opus-5-5', 'claude-sonnet-5-5']
plain memory file: 100 histories, 4,787 chars on average (max 6,493), 25.6 calls, $0.1312 and 551 s of model time per history of 47 sessions
query engine: $0.00115 per question, 9.7 s median (two model calls)
