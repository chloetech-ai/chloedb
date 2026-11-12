Project: slotwise, a Python appointment library for Lindqvist Physio (four therapists). A front-desk app shows our exceptions to receptionists. Code is in `src/slotwise`, tests in `tests/`. There is no pyproject; pytest isn't installed. Run tests with `PYTHONPATH=src python3 -m unittest discover -s tests -v`. 34 pass.

Branch: `booking-errors-and-gap` (from `main` 8fc1af9), commits a91ec89, 1889f1d, 27e70fd. Clean, not pushed or merged. Don't merge without asking.

Done:
- `errors.py`: `BookingError` base, with `SlotTaken` and `OutsideOpeningHours`. Unknown treatment raises plain `BookingError`.
- `book()` keeps 10 min between the same therapist's appointments, before and after. Opening hours aren't checked against the gap.
- `cancel(id)` sets a flag and keeps the record; cancelled appointments don't block slots or gaps. Storage saves the flag. Unknown id raises `KeyError`.
- `week(therapist, monday)`: seven date keys, active only, time-sorted; `ValueError` if not Monday.
- `clients.py`: `Client`, `ClientBook.add`, `find_by_phone`, `search`. `normalize_phone` makes all phone formats match. Not wired to storage or Schedule.

Rules from the user:
- Exception messages are plain language for receptionists.
- Series bookings: max 9 sessions.
- SMS: ASCII only (ä→ae), max 160 chars, must include 044 512 77 30.
- No iCal or calendar feed (data protection decision).
- The 7-a-day cap must not go in `book()`. It belongs in a separate query for the online page.

Planned, not built (in project memory): cancellation fees, series bookings, SMS reminders, 42-char day plan. The fee boundaries are my reading: under 24h full, 24–48h half, 48h+ free. Confirm them.

Open questions to ask the user:
- Should cancel's unknown-id error be a `BookingError`?
- Should cancelled appointments count toward the 7-a-day cap? `appointments_for()` still returns them.
- Series over 9: refuse or cut? All-or-nothing? Check closing times?
- Fee rounding; cancellation after start.
- SMS over 160 chars; non-ASCII names.
- CHANGELOG is not updated.
