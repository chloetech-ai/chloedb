# UPDATE and DELETE by key: before and after

`strata-py/bench/laptop/a_writes.py --systems strata --no-stream` on the
Shop's 137,260 products (Amazon Reviews 2023 Video Games, prepared with
`shop_prep.py --embed random`: random vectors, real titles and
descriptions), run twice on one machine with only the Python extension
changed:

- `writes_strata_main.json`: main at de51793;
- `writes_strata_by_key.json`: the change that reads the rows of an UPDATE
  or DELETE whose WHERE names keys by key.

Machine: a Claude Code cloud container, 4 vCPU Intel Xeon (model not
named by the VM), 16 GB, Linux 6.18; `maturin build --release`; Python
3.13. One run each.

| update (rows/s, p50) | main | by key |
|---|---|---|
| price and stock (in place) | 1,177, 0.79 ms | 1,391, 0.66 ms |
| counter + 1 (in place) | 1,065, 0.83 ms | 1,453, 0.62 ms |
| title (re-indexed) | 48, 20.5 ms | 855, 1.09 ms |
| vector (HNSW) | 21, 47.2 ms | 506, 1.46 ms |
| upsert of the whole row | 3,045, 0.30 ms | 2,911, 0.33 ms |
| delete | 5,595, 0.17 ms | 4,996, 0.18 ms |
