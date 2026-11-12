**Handoff: bakehouse** (Larkspur Bakery order, production and invoice tool; stdlib Python)

**State:** Branch `fix-batch-rounding-and-checks`, clean at `c6a744a`, not pushed, no PR. Test with `PYTHONPATH=src python -m unittest discover -s tests`. The suite runs 76 tests, and 4 in `test_standing.py` fail. They predate my work, probably from `CLOSED_WEEKDAYS=(0,)`, and I haven't investigated. Commit with a Co-Authored-By trailer, one commit per feature, and keep the CHANGELOG Unreleased section current.

**Rules:** Money is integer pence. Code is stdlib-only and must run on Python 3.7 (the packing-room Pi); no 3.7 interpreter is available here. Don't fill Stopwise notes (60-char cut). Old Mill winter days need no code.

**Done:** batch rounding fix (25 at 12 gives 3); catalog validation; allergen helpers; OrderBook methods; cutoff tests; `dough_weight_g` (whole batches, grams); `crates_needed`; `invoice_for`, `format_pence`, `invoice_number` (1–9999 limit is my choice); `day_summary`, `top_products`, `standing.expand`; Potter's Farm Shop (route 6).

**Open, ask before acting:**
- Greenway must be first drop, but `route_seq` is 4 and its test asserts that.
- Config vs. agreed rules: `ORDER_CUTOFF` is 13:00 (4f1ac09), agreed 14:00; `CLOSED_WEEKDAYS` closes Mondays (b7ecfcf), agreed no Sundays. Harbour Deli now opens Tuesdays; data unchanged.
- Saturday-to-Monday cutoff rule is unresolved.
- Delivery charge £4.50 under £40 (St Aidan's exempt) supersedes memory's £3/£25. Not built; memory not updated.
- January rise: 5%, rounded up to the next 5p. Not built.
- Greenway sale-or-return: unchanged terms, not built.
- Whole-batch dough weighing is my reading, unconfirmed.
- St Aidan's nut block is not built and needs a test before go-live. Allergen labels are hand-typed.
- Not built: Stopwise export, Greta's semicolon/comma CSV, Ledgerly DD/MM/YYYY export, label printing (48 chars, ASCII, allergens in capitals), hottest-oven-first bake sheet.
- November plans need regenerating; Nov orders aren't in the repo. Café Lumen's new manager Jo has no email yet.
