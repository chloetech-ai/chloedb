| reader | context | n | correct | tokens read | $ reader / q | $ memory / q | reader s (p50) |
|---|---|---|---|---|---|---|---|
| haiku | No memory | 100 | 9% | 514 | 0.00018 | 0.00000 | 1.5 |
| haiku | Plain memory file | 100 | 45% | 2,691 | 0.00085 | 0.13120 | 3.0 |
| haiku | ChloeCtx, search | 100 | 68% | 1,831 | 0.00070 | 0.00000 | 3.1 |
| haiku | ChloeCtx, query engine | 100 | 84% | 2,159 | 0.00077 | 0.00115 | 3.1 |
| haiku | Whole history in context | 100 | 88% | 168,447 | 0.17038 | 0.00000 | 4.1 |
| sonnet | No memory | 100 | 8% | 493 | 0.00322 | 0.00000 | 2.0 |
| sonnet | Plain memory file | 100 | 49% | 2,670 | 0.01380 | 0.13120 | 2.4 |
| sonnet | ChloeCtx, search | 100 | 75% | 1,810 | 0.01031 | 0.00000 | 2.3 |
| sonnet | ChloeCtx, query engine | 100 | 87% | 2,138 | 0.01162 | 0.00115 | 2.3 |
| sonnet | Whole history in context | 25 | 96% | 168,879 | 0.54821 | 0.00000 | 3.6 |
| sonnet | No memory (same 25) | 25 | 16% | | | | |
| sonnet | Plain memory file (same 25) | 25 | 60% | | | | |
| sonnet | ChloeCtx, search (same 25) | 25 | 72% | | | | |
| sonnet | ChloeCtx, query engine (same 25) | 25 | 88% | | | | |
| opus | No memory | 100 | 9% | 489 | 0.00751 | 0.00000 | 3.1 |
| opus | Plain memory file | 100 | 50% | 2,666 | 0.02903 | 0.13120 | 4.4 |
| opus | ChloeCtx, search | 100 | 80% | 1,806 | 0.02190 | 0.00000 | 3.7 |
| opus | ChloeCtx, query engine | 100 | 89% | 2,134 | 0.02418 | 0.00115 | 3.8 |
| opus | Whole history in context | 25 | 100% | 168,875 | 1.09620 | 0.00000 | 7.6 |
| opus | No memory (same 25) | 25 | 16% | | | | |
| opus | Plain memory file (same 25) | 25 | 56% | | | | |
| opus | ChloeCtx, search (same 25) | 25 | 84% | | | | |
| opus | ChloeCtx, query engine (same 25) | 25 | 92% | | | | |

models: ['claude-haiku-5-5', 'claude-opus-5-5', 'claude-sonnet-5-5']
plain memory file: 100 histories, 4,787 chars on average (max 6,493), 25.6 calls, $0.1312 and 551 s of model time per history of 47 sessions
query engine: $0.00115 per question, 9.7 s median (two model calls)
