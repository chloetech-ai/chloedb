# Handoff note: stockroom

## Project
`stockroom` is a Python stock-keeping library for Brandt & Sons, a hardware wholesaler with two shops. It covers the item catalog, stock movements, quantities on hand, and reorder checks. Repo: `/home/user/bench/work/stockroom/engram/stockroom`.

**Hard constraint:** the back-office PC runs Python 3.8, because the scanner vendor's sync tool only runs there. Use no newer syntax or stdlib APIs. No 3.8 interpreter is available here, and there's no network to install one. Checks so far are `ast.parse(..., feature_version=(3, 8))` plus manual review, so the code has never actually run on 3.8.

## Git state
- Branch: `sku-normalize-reason-codes` (off `main`). Nothing is pushed or merged. Ask before doing either.
- Commits on the branch:
  - `a2c7e00`: SKU normalization, reason codes, reorder boundary fix
  - `0107004`: Item validation, legacy CSV loading
  - `f2876d9`: CLI (`onhand`, `moves`, `receive`). This is HEAD.
- Working tree was clean after the last commit.
- `.mcp.json` and `.claude/settings.local.json` are gitignored, as is `__pycache__`. Don't commit them.
- Commit messages use a `Co-Authored-By` trailer, as in previous commits.

## Running tests
`pytest` is not installed here. Use:
```
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_*.py'
```
Last run: 35 tests, OK, on Python 3.13.18.

## Done
1. **Reorder check** (`reorder.py`): changed `<` to `<=`, so an item exactly at its reorder point is flagged, matching the docstring.
2. **SKU normalization** (`items.normalize_sku`): trims and uppercases, and raises `ValueError` on an empty result. The empty check is my addition, not in the spec. `Catalog.add/get/__contains__`, `Inventory.receive/issue/on_hand/movements_for`, and the `sku` column in `load_movements` all go through it. `Catalog.add` rewrites `item.sku` on the object passed in, which mutates the caller's `Item`. I left that as is and flagged it.
3. **Reason codes:** `REASON_CODES = {RECEIPT, SALE, RETURN, DAMAGE, COUNT}` in `inventory.py`. `Movement.reason` is a required field placed after `date`, and `__post_init__` rejects any other value (the check is exact and case-sensitive). `receive()` records RECEIPT and `issue()` records SALE. The CSV has a `reason` column.
4. **Legacy CSV load:** a file with no `reason` column loads, and each row's code is inferred from its quantity. Positive becomes RECEIPT, negative becomes SALE. A zero quantity in such a file raises `ValueError` with the line number, because I didn't want to guess a code for it.
5. **Item validation** (`Item.__post_init__`): `case_size < 1` and `reorder_point < 0` raise `ValueError`.
6. **CLI** (`cli.py`, `__main__.py`, `tests/test_cli.py`), run as `PYTHONPATH=src python3 -m stockroom ...`:
   - `onhand SKU --log F`: prints the quantity on hand.
   - `moves SKU --log F`: one line per movement, `YYYY-MM-DD  REASON   +/-qty`, in log order. No padding on quantities.
   - `receive SKU QTY UNIT_COST --log F [--note N]`: appends a receipt and creates the log if needed. `UNIT_COST` is integer cents. Receipts use today's date.
   - `onhand` and `moves` exit 1 with an error if the log is missing. This is deliberate, so a mistyped path doesn't read as zero.
   - `receive` rewrites the whole log rather than appending. This upgrades legacy logs to the new header. The write is not atomic. A temp file and rename would fix that.

## Decisions and rules from the user
- **Negative stock is allowed.** `issue()` must never refuse a quantity that takes stock below zero. The till posts sales only after payment, so a refusal silently lost the sale in the old system. Stock is corrected at the next count. The only check in `issue()` is that qty is positive. Stored in ChloeCtx as a decision.
- **Stock valuation must use FIFO, not average cost.** The auditor flagged average-cost figures last year and the accounts were restated. Not built yet. Stored as a decision.
- **December stock-take count sheet:** lines in walking order, sorted by bin location (`Item.location`), with aisle, bay and shelf parsed as numbers (so `B-9-1` sorts before `B-12-3`). It must be blind: never show the on-hand quantity, only an empty counted column. Stored as a decision. Not built.
- **Scanner importer** (after the count sheet): input is plain text, semicolon-separated `bin;sku;qty`, no header. A SKU can appear on several lines (overflow bins), and quantities are summed per SKU. SKUs go through `normalize_sku`. Stored as a todo. Not built.
- **Purchase suggestions** (next sprint): order enough to get back up to 3× the reorder point, rounded up to whole cases of `case_size`. Only the validation exists so far. The suggestions are not built.

## Open questions (the user hasn't answered)
1. Should the scanner importer check the bin against `Item.location`, or only total per SKU?
2. Should the summed scanner counts be written as `COUNT` movements?
3. RETURN, DAMAGE and COUNT are valid codes, but no method can create them. `receive()` and `issue()` only produce RECEIPT and SALE. Does the back office need a way to record those?
4. Should `Catalog.add` copy the item instead of mutating it?
5. Should `moves` right-align the quantities?
6. Push or merge the branch, and when?

## Caveats
- Python 3.8 has never been executed. The 3.8 check is grammar and API review only. Run the suite on 3.8 before this ships to the back office.
- `reason` is a required positional field after `date`. Any code that constructs `Movement` positionally with the old signature will break.
- `README.md` and `CHANGELOG.md` have not been updated for any of this.
- ChloeCtx memory entries can be stale. Trust the files over memory.
- Earlier notes said legacy CSVs won't load. That is no longer true, since the legacy fallback was added in `0107004`.
