**Handoff: stockroom** (Python stock-keeping library for Brandt & Sons, a hardware wholesaler with two shops)

**Constraints**
- Must run on Python 3.8 (back-office PC). No 3.8 interpreter or network here, so check with `ast.parse(..., feature_version=(3,8))` and avoid 3.9+ APIs.
- Run tests with `PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_*.py'`. pytest is not installed. Last run: 35 OK.
- Don't commit to `main`. Work is on branch `sku-normalize-reason-codes`, not pushed or merged. Commits: `a2c7e00`, `0107004`, `f2876d9`. Tree is clean.

**Done**
- `reorder.needs_reorder` flags on-hand at or below the reorder point.
- `normalize_sku` (trim, uppercase, reject empty) is used by Catalog, Inventory and CSV load.
- Movements have a required `reason` from RECEIPT, SALE, RETURN, DAMAGE, COUNT. `receive()` records RECEIPT and `issue()` records SALE. CSV has a `reason` column. Legacy files without it load, with positive quantity as RECEIPT and negative as SALE. Zero in a legacy file is an error.
- `Item` rejects `case_size < 1` and negative `reorder_point`.
- CLI (`cli.py`, `__main__.py`): `onhand`, `moves`, `receive`. UNIT_COST is integer cents. `receive` rewrites the whole log, and the write is not atomic.

**Rules and decisions (user)**
- `issue()` must never refuse stock going below zero. The till posts sales after payment.
- Valuation must be FIFO, not average cost (auditor requirement).

**Open, not built**
- December count sheet: walking order sorted by parsed aisle/bay/shelf, not string order. Blind: no on-hand shown.
- Scanner importer: `bin;sku;qty`, no header, sum per SKU. Unresolved: check bin against `Item.location`? Write COUNT movements?
- Purchase suggestions (next sprint): order up to 3× reorder point, rounded up to whole cases.
- No method yet creates RETURN, DAMAGE or COUNT movements.
- FIFO valuation.

Decisions are stored in ChloeCtx memory.
