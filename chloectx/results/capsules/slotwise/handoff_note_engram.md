# Handoff note: slotwise

## What the project is
slotwise is a Python appointment library for Lindqvist Physio, a physiotherapy practice with four therapists. It covers treatments, opening hours, booking, a JSON file store, and (new) a client register. A separate front-desk app imports the library and shows our exception messages directly to receptionists.

- Repo: `/home/user/bench/work/slotwise/engram/slotwise`, git, Python 3.13, `src/` layout.
- No `pyproject.toml`, `package.json` or Makefile. pytest is not installed.
- Run tests with: `PYTHONPATH=src python3 -m unittest discover -s tests -v`
- Last run: **34 tests, all passing**.

## Branch and commits
- All work is on branch **`booking-errors-and-gap`**, not `main`. The name no longer describes the contents. Nothing is pushed, no PR is open, and nothing is merged. Ask before merging or pushing.
- Commits so far:
  - `8fc1af9` initial version (on `main`)
  - `a91ec89` BookingError hierarchy and 10-minute gap
  - `1889f1d` Schedule.cancel and Schedule.week
  - `27e70fd` client register with phone lookup and name search
- Commit messages are multi-line and explain the why. They end with `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`. Follow that pattern.
- The user asked for commits when work was finished, so commit on the branch when a piece is done and tested. Do not commit to `main`.
- `CHANGELOG.md` has an empty "Unreleased" section that has never been updated.

## Current code (by file)
- **`model.py`**: `TREATMENTS` maps name to (minutes, price in cents): `initial` 60 min / 14500, `standard` 45 min / 9800, `short` 30 min / 6800. `Appointment` is a dataclass with `therapist`, `client` (a name string), `start`, `treatment="standard"`, an auto `id`, and `cancelled=False`. It has `end` and `price` properties.
- **`hours.py`**: `OPENING_HOURS` by weekday (Monday=0). Mon–Thu 07:30–19:00, Fri 07:30–17:00, Sat 08:00–12:00, Sun closed. Functions: `opening_hours(day)`, `is_open_between(start, end)`.
- **`errors.py`**: `BookingError(Exception)`, with subclasses `SlotTaken` and `OutsideOpeningHours`.
- **`schedule.py`**: class `Schedule`.
  - `book(appointment)`:
    - An unknown treatment raises a plain `BookingError` that lists the valid treatments.
    - A time outside opening hours raises `OutsideOpeningHours`.
    - A clash raises `SlotTaken`. The rule pads both appointments by `GAP` (10 minutes) on each side, for the same therapist only. Cancelled appointments are skipped.
    - The gap is not checked against opening hours.
  - `cancel(appointment_id)` sets `cancelled=True` and returns the appointment. Cancelling twice does nothing. An unknown id raises `KeyError`.
  - `get(id)` raises `KeyError` if the id is unknown.
  - `appointments_for(therapist, day)` returns all appointments, **including cancelled ones**, sorted by start.
  - `week(therapist, monday)` returns a dict with all seven dates as keys, each mapping to active (non-cancelled) appointments sorted by start. It raises `ValueError` if `monday` is not a Monday.
- **`storage.py`**: JSON store. It saves and loads `cancelled` with `d.get("cancelled", False)`, so older files still load. Only appointments are persisted, as far as I can see. I did not extend storage for clients.
- **`clients.py`** (new, committed):
  - `normalize_phone(phone)` strips non-digits. A leading `+` means it is already international. A `00` prefix is removed. A leading `0` becomes `41` (Swiss assumption). It raises `ValueError` if there are no digits.
  - `Client` dataclass: `id`, `name`, `phone`, `email=""`. The phone is kept as given for display.
  - `ClientBook`: `add(client)` raises `ValueError` on a duplicate normalized phone. `find_by_phone(phone)` returns a Client or `None`. `search(text)` is a case-insensitive substring match on name, in registration order. Surrounding whitespace is stripped, accents are not folded, and an empty string returns everyone.
- **Tests**: `test_schedule.py`, `test_model.py`, `test_hours.py`, `test_storage.py`, `test_clients.py`.

## Rules and conventions from the user
- **Booking exceptions**: one hierarchy under `BookingError`, so the front-desk app catches every booking failure in one place. Messages must be plain language a receptionist can read as-is. Example messages:
  - "The practice is closed at 18:30 on Tuesday 8 September. Please choose a time within opening hours."
  - "Lena is already booked 09:00–09:45 and needs 10 minutes between appointments. Please choose another time."
- **Gap**: therapists need 10 minutes between appointments, before and after, for cleaning and notes.
- **Cancelled appointments** keep their record, because cancellation fees need it. They no longer block their slot or gap.
- **Privacy**: the user has been firm. Patient names must not reach external services. See the iCal decision below.

## Decisions and their reasons
- **No iCal or calendar feed** (memory 3d4cb106:343). A feed was tried last year. Phone calendars cached it for hours, so cancelled appointments showed up. The data protection officer stopped it because patient names reached Google and Apple servers. Do not build a calendar feed.
- **The 7-a-day cap is not enforced in `book()`** (memory 3d4cb106:165). Reception sometimes books over 7 when a therapist agrees. The online booking page (launching in a few weeks) must treat a therapist with 7 appointments that day as fully booked. This goes in a separate query, for example `Schedule.is_fully_booked(therapist, day)`. Not built.
- **Cancelling frees the slot and the gap.** Done.
- **Unknown id in `cancel()` raises `KeyError`**, not `BookingError`, because cancelling is not a booking failure. Still undecided whether the front-desk app should catch it.
- **Empty days in `week()` appear as empty lists**, so the overview always shows seven days.

## Planned work (not built, in memory)
Each item is stored as a todo in ChloeCtx memory. Query with `mcp__chloectx__memory_ask` before starting.

1. **Cancellation fees** (next sprint, memory 3b6b9e8a:154). Cancelling less than 48 hours before costs half the treatment price. Less than 24 hours costs the full price. Earlier is free. The old receptionist got these wrong by hand. Open questions I raised, none answered yet:
   - Are exactly 48 or 24 hours free or charged? I assumed strict "less than".
   - How is cancelling after the start time handled?
   - Does the library store the fee on the appointment, or only return the amount?
   - How are half-prices rounded to cents?
2. **Online-page 7-a-day query** (memory 3d4cb106:165 / 3b6b9e8a:165). Open: should cancelled appointments count toward the 7? I assumed no.
3. **Series bookings** (next month, memory 3d4cb106:306). Weekly, same weekday and time. A taken week is skipped and a week is added at the end, so the client gets all their sessions. A series may never exceed 9 sessions, because insurance pays for 9 per prescription and reception keeps trying to book 12. My proposed answers, not yet confirmed:
   - Refuse a request for more than 9, with a plain message, instead of silently cutting it.
   - Make the whole series all-or-nothing, so a prescription is never half-booked.
   - Check the moved-to-end weeks against opening hours.
4. **SMS reminders** (next month, memory 3d4cb106:324). The gateway takes plain ASCII only. Transliterate first: ä→ae, ö→oe, ü→ue, ß→ss, and strip accents, so "Brändli" becomes "Braendli". A message is at most 160 characters after transliteration, otherwise it is billed as two. Every reminder must include reception's number, **044 512 77 30**. Open questions:
   - When the message is too long, what gets trimmed? I suggested trimming wording first and never dropping the number.
   - What happens to characters with no ASCII form, such as Turkish dotless i or non-Latin scripts? Drop them, or refuse and flag?
   - Does the 160-character check live in the library or in the sending code?
5. **Printable day plan** (later job). The receipt printer is 42 characters per line, and longer lines wrap badly. Lines must fit 42 characters.

## Open issues and things to check
- **Client register is not linked to appointments.** `Appointment.client` is a plain name string, not a client id. Linking them has not been decided.
- **Client register is not persisted.** `ClientBook` is in memory only.
- **Exception convention mismatch**: `ClientBook.add` (duplicate phone) and `normalize_phone` (no digits) raise `ValueError`. `week()` also raises `ValueError`. The front-desk app shows booking errors to receptionists, so decide whether these should use `BookingError` or a separate client error. Not decided.
- **`normalize_phone` assumes Swiss numbers** (leading 0 becomes 41). It does not handle other national formats.
- **`appointments_for` includes cancelled ones.** Callers that want active only should use `week()` or filter.
- Test-writing mistakes happened twice (a wrong assertion in a cancel test, and a wrong search string in a client test). Run the suite after each edit.

## Working style the user expects
- Plan-only requests ("next month", "later") should be saved to ChloeCtx memory as todos and not coded yet.
- Build what is asked, run the tests, and report results plainly. Say which choices were mine so the user can redirect.
- Confirm before merging, pushing, or anything outward-facing.
