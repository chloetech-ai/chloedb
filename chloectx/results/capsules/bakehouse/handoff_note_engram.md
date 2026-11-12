# Handoff note: bakehouse (Larkspur Bakery order and production tool)

## Project
- Python project, standard library only, tests run with `PYTHONPATH=src python -m unittest discover -s tests`. Must stay compatible with **Python 3.7**: a Raspberry Pi in the packing room runs 3.7 with no internet. Avoid the walrus operator, `zoneinfo`, `dict |`, builtin generics, `match`, and similar. The sandbox has only Python 3.11+, so the 3.7 check is an AST parse with `feature_version=(3,7)`, not a real run.
- Layout: `src/bakehouse/` (config, customers, invoice, orders, production, products, reports, standing), `scripts/print_plan.py`, `tests/`, `data/` (customers.csv, orders.csv, products.csv), `CHANGELOG.md`.
- Repo branch: `fix-batch-rounding-and-checks` (local only, never pushed; `main` untouched). Author is Dana. Commit trailer used: `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`. Commits are made only when the user asks.
- Last commit: `c6a744a`. The tree was clean after it. Last test run: 76 tests, **4 failures, all in `tests/test_standing.py`** (`test_end_date_is_inclusive`, `test_one_line_per_matching_day_in_range`, `test_output_is_in_date_order_across_mixed_weekdays`, `test_sundays_are_skipped_because_we_do_not_deliver_then`). They predate the most recent work and have **not been investigated**. Start there. They probably relate to `CLOSED_WEEKDAYS` now being `(0,)` (Monday), since the tests assume Sunday.

## People
- **Greta Vogel**: owner. Sets prices and accepts customers. Her spreadsheets and Excel are in German: semicolon separators, comma decimals.
- **Tomasz**: head baker, starts 3 a.m., bakes from the printed plan. Ana covers his shifts 9–23 November (he's on holiday). Ana is new to computers.
- **Priya**: books and VAT return. Uses Ledgerly: plain comma-separated file, header row, DD/MM/YYYY dates, amounts in pounds with a decimal point and two decimals.
- **Dev**: van driver, leaves 5:30. Uses the Stopwise app: header exactly `Name,Address,Postcode,Notes`, rows in driving order. Any other header is refused, and out-of-order rows get re-optimised the wrong way round the one-way system.
- **Sam**: set up the packing-room Raspberry Pi.
- **Marek**: Greenway Grocers' buyer. Contact for delivery changes. Email is in customers.csv.
- Customers: LUMEN (Café Lumen, bank transfer monthly; new manager Jo, email not yet given), HARBOUR (Harbour Deli, pays Dev cash on delivery), OLDMILL (The Old Mill Tearoom, wants Wed–Sat deliveries in winter, orders for themselves), GREENWAY (Greenway Grocers), AIDAN (St Aidan's Primary, nut-free), POTTER (Potter's Farm Shop, joins next month, route_seq 6).

## Rules and conventions the user set
- **Money is integer pence everywhere.** Greta's float-rounding VAT error (£1.37 off, HMRC correction) is why. Pounds conversion only in the export layer.
- **Batches round up** (`batches_needed` uses ceiling division). Bug fixed in session 1 (25 baguettes at 12/batch was 2 batches, now 3).
- **Order cutoff**: `config.ORDER_CUTOFF`, 14:00 on the day before, 14:00 exactly counts. Tests build times from the config. **Current config value is 13:00** (commit 4f1ac09, for the mill's 13:30 flour order). See open issues.
- **Closed days**: originally `CLOSED_WEEKDAYS=(6,)` (Sunday). **Current config is `(0,)`** (Monday; commit b7ecfcf, "Deliver on Sundays, close on Mondays instead", for the Sunday farmers' market stall). See open issues.
- **Unknown product codes** are rejected by `OrderBook.add` with ValueError. Greta asked for this after a typo in spring silently dropped Greenway's sandwich-loaf order. Keep it strict.
- **Catalog validation** (`load_catalog`): batch size below 1 or negative price raises ValueError naming the product code. Price 0 is allowed (user didn't say otherwise).
- **Allergens**: `Product.has_allergen(name)` is case-insensitive. Allergen labels are hand-typed in products.csv. Nuts are on BKW and FLO. St Aidan's must never receive nut products once orders go through the tool. **This block is not built.** It must be tested before the tool goes live for the school.
- **Label printer** (packing room): plain ASCII only, no accents or umlauts. Limit went from 32 to **48 characters per line** (new printer, Greta's purchase). Allergens printed in capitals by law. Labels are not built yet. Memory notes still say 32, so update them.
- **Dough weights**: in whole grams (Tomasz's new platform scale). `dough_weight_g` weighs whole batches (25 baguettes at 350 g = 36 pieces = 12,600 g), which is **my reading, not confirmed by the user**. Asked to be told if they want pieces-ordered instead. The kilo display (5.4 kg) from an earlier request was **superseded by grams**, not built.
- **Dev's route**: Greenway must always be first drop (opens 7:00). Greenway currently has route_seq 4 in data and a test asserts that. **Unresolved conflict**, needs Dev's or the user's decision. Do not silently change it. Setting it to 1 would collide with LUMEN under the duplicate-route check.
- **Duplicate route_seq** is rejected by `load_customers`.
- **Sale-or-return credit (Greenway, sandwich loaves only)**, for Priya, not built: half the wholesale price per returned loaf, capped at 10% of the sandwich loaves delivered that day; excess returns not credited. **Terms unchanged** per Greta. Saved in memory.
- **Delivery charge**: the user changed it in the most recent message from £3.00 under £25.00 to **£4.50 on any order under £40.00** (4000 pence), from November, value per customer per delivery day before VAT. **St Aidan's exempt** (contract price includes delivery). Not built, deferred to the invoice rework. **Memory still holds the old £3.00 rule.** Ask the user to confirm before replacing it. The old note is wrong if this supersedes it.
- **£50 cash-and-carry minimum** is their rule, not ours. Not coded.
- **Price rise**: January, **5%** (changed from 4%), each new price rounded **up** to the next 5p. Not built, deferred to December. Greta sets prices.
- **VAT**: invoices stay without VAT. Almost all food is zero-rated. Florentines (FLO) are 20% (chocolate-coated). Priya handles VAT in Ledgerly.
- **Stopwise Notes column** is cut at 60 characters. No notes filled in yet.
- **St Aidan's holidays**: no holiday logic. Greta removes those weeks by hand from the standing order after the school's September term dates.
- **Greenway phone changes until 16:00**: a manual favour by Greta, not coded.
- **Café Lumen 5% discount from December**: Greta hasn't decided. Nothing coded.
- **Mrs Okonjo** (St Aidan's kitchen) emails orders Thursdays for the following week. No code change needed.
- Commits: only when asked. Several commits were done in logical pieces, and one covering a whole session when the user asked for a single commit. Don't push or open PRs without asking.
- Before deleting or overwriting, look at the target. Report test failures plainly.

## Built and committed (branch `fix-batch-rounding-and-checks`)
- `production.py`: `batches_needed` (ceiling), `dough_weight_g` (whole batches, rejects negative), `plan`, `total_batches`, `crates_needed(units, per_crate)` (ceiling, rejects per_crate < 1 and negative units). Last commit c6a744a.
- `products.py`: validation in `load_catalog`, `has_allergen`, `allergen_text`, `price_text` (uses `format_pence` from invoice.py), `Catalog.get` raises KeyError naming the code.
- `orders.py`: strict `add`, `total_for`, `customers_for`, `lines_for`, `remove` (removes all matching lines, returns bool), `__len__`, `days()`. `is_on_time` and boundary tests. `expand` will need to respect closed days.
- `customers.py`: `__str__`, duplicate route_seq rejection, optional `email` column (default ""). Python 3.7 fix (no walrus).
- `invoice.py`: `format_pence` (negative gets leading minus), `invoice_for` (pence, sorted by product code), `invoice_number(year, seq)` returns `LK-2026-0001`, seq must be 1–9999 (my choice, mention it).
- `reports.py`: `day_summary` (pence per customer, uses `customers_for`), `top_products(orderbook, start, end, n=5)`, ties broken by code.
- `standing.py`: `StandingLine` (weekday 0–6 check), `expand(standing_lines, start, end)` skips closed days via `is_delivery_day`.
- `scripts/print_plan.py` was replaced by `python -m bakehouse plan` (commit 4f54457, not by me, in the user's history). Check the current state of the script before relying on it.
- `data/customers.csv`: header now `code,name,address,postcode,route_seq,email`. Typo fixed (Tearoom). Greenway email marek@greenway-grocers.example. POTTER added with address `"Lower Farm, Ashby Road"` (quoted), LK5 3PF, route_seq 6, empty email.
- CHANGELOG updated in c6a744a. Items listed are from the session work.

## Open tasks and decisions needed
1. **Investigate the 4 failing `test_standing.py` tests.** Likely caused by the Monday closure (`CLOSED_WEEKDAYS=(0,)`) vs. tests assuming Sunday. Don't assume; read first.
2. **Config conflicts** (user or Greta must confirm, don't change silently):
   - `ORDER_CUTOFF` is 13:00 in config, but the agreed rule is 14:00.
   - `CLOSED_WEEKDAYS` is `(0,)` (Monday), but the rule says no baking or delivery on Sundays. The current config closes Monday and delivers Sunday, which reverses the rule.
   - The Saturday→Monday cutoff question is still open: under the literal rule, a Monday delivery's cutoff falls on Sunday, a closed day.
3. **Greenway first drop**: route_seq 4 vs. always first. Needs Dev's or the user's decision.
4. **Harbour Deli Tuesdays**: user said they now open Tuesdays, and Dev delivers daily. Earlier notes said Tuesday closed. Data and config are not changed. Needs confirmation and a schedule change if wanted.
5. **Delivery charge** rule update: £4.50 under £40.00, St Aidan's exempt, from November. Replace the old £3.00/£25.00 note in memory once the user confirms.
6. **Stopwise export** (`Name,Address,Postcode,Notes`, driving order): not built. Depends on item 3.
7. **Greta's German CSV export** (semicolons, comma decimals): not built.
8. **Ledgerly export** (DD/MM/YYYY, pounds with decimal point): not built.
9. **Bake sheet**: products hottest oven first (Tomasz, deck oven). Not built. Changelog has a "Planned" line for it.
10. **Label generation** (48 chars/line, ASCII only, allergens in caps, customer name and quantity + product name on line 2, allergens after). Not built.
11. **Greenway sale-or-return credit** function for Priya. Not built.
12. **January 5% price rise** rounded up to 5p. Not built, for December.
13. **St Aidan's nut block** in the order tool. Not built, must be tested before go-live.
14. **Allergen label check** against recipes. Hand-typed labels in products.csv need verifying, and new products added later need checking too.
15. **Jo's email** for Café Lumen: not given. Ask for it.
16. **Nov plan regeneration**: `data/orders.csv` only has 2026-10-20 and 2026-10-21. November orders aren't in the repo. The 11 Nov plan made under the floor-division bug needs regenerating from wherever the orders are kept.
17. **Empty-day warning in print_plan**: optional, unanswered. Ana might read a bare header as "nothing to bake."
18. **Python 3.7 run on the Pi**: not verified. Someone must run `python3 -m unittest discover -s tests` there.
19. **Memory corrections**: 32→48 char label limit; the ValueError for negative `dough_weight_g` is in place; the delivery charge; the note that closed-day rules and cutoff conflict with config. The user has offered to save the Greenway incident to memory; not yet done.
20. **Not decided**: pushing the branch and opening a PR (ask first). Changelog for the 4 fixes from session 1 is not recorded.
21. **Sundays and `expand`**: `expand` skips `CLOSED_WEEKDAYS`, so it follows whatever config says (currently Monday). Re-check once item 2 is settled.

## Working style notes
- Explain choices in the final message and list them so the user can redirect. Mention when a reading of the request was my judgement (e.g. whole-batch dough weights, 1–9999 invoice range, duplicate-code removal removes all matches).
- Edits to the working tree are fine. Commits only when asked. Ask before outward-facing actions.
