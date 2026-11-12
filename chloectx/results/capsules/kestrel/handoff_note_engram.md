# Handoff note: kestrel

## What it is
kestrel is a Python booking library for Kestrel Physio, a physiotherapy practice with three Auckland clinics: Northgate (NG), Harbour (HB) and Westwick (WK). It uses the standard library only. Data lives in JSON (`storage.py`), and `data/legacy_appointments.csv` is an export from the old booking system. The user is Dana (git author `Dana <dana@example.com>`).

## Running it
- Repo: `/home/user/bench/work/kestrel/engram/kestrel`
- Tests: `PYTHONPATH=src python -m unittest discover -s tests -v`
- Stricter run, used before each commit: `PYTHONPATH=src python -X dev -W always -m unittest discover -s tests`
- Run a single test module with `discover -p 'test_x.py'`. Running `-m unittest tests.test_x` fails because `tests` is not a package.
- CLI: `python -m kestrel day THERAPIST_ID YYYY-MM-DD --data FILE` and `python -m kestrel patients --data FILE`.
- The sandbox has only Python 3.13. The reception PCs run **Python 3.9**. The code has been checked for 3.9 compatibility only by parsing with `ast` at `feature_version=(3, 9)`. It has never run on 3.9.

## Current state
- **Branch:** `calendar-queries`, branched from `main` at `ad53175`. `main` is untouched. Nothing is pushed.
- **Head:** `09978f4`. The commit chain after `ad53175` is `1adc815`, `97afb5b`, `9612014`, `bfc08fa`, `974f7ea`, `ca5ed88`, `33157d8`, `3ed8bb0`, `cf59c17`, `9555525`, `b7795df`, `006e0f3`, `09978f4`.
- **Working tree:** clean, except for untracked `__pycache__/` directories in `src/kestrel/` and `tests/`. Never commit those. The repo has no `.gitignore`, and adding one was suggested but not done.
- **Tests:** 99, all passing, including under `-W error -X dev`.

## Module map
- `models.py`: constants `ASSESSMENT`/`FOLLOWUP`, statuses `BOOKED`/`CANCELLED`/`ATTENDED`/`NO_SHOW`, `CLINICS`. Dataclasses `Clinic`, `Therapist`, `Patient` (`id`, `name`, `phone`, `csc=False`), and `Appointment` (`id`, `patient_id`, `therapist_id`, `clinic`, `start` (tz-aware, stored UTC), `kind`, `treatment`, `status`, `cancelled_at=None`). Uses `Optional`, not `X | None`.
- `calendar.py`: `CLINIC_TZ = ZoneInfo("Pacific/Auckland")`. `Calendar` has `add` (rejects duplicate ids only), `get`, `all` (start order), `for_therapist`, `for_patient` (normalises the id), `for_day(therapist_id, day)`, `booked`, `cancel(id, at)`, and `move(id, new_start)`.
- `ids.py`: `normalize_patient_id`. Input such as `" kp4211 "` becomes `KP-004211`. Invalid input raises `ValueError`.
- `storage.py`: `save`/`load`. `save` writes atomically (see below).
- `invoices.py`: `Invoice` and `make_invoice(appointment, patient, number, amount_cents)`. The patient name is always left empty.
- `pricing.py`: `session_price(kind)` returns cents: assessment 9500, follow-up 7000. Unknown kinds raise `ValueError`.
- `reports.py`: `therapist_load(calendar, week_start)` returns `{therapist_id: {date: count}}` for booked appointments by local date.
- `sms.py`: `BulkieClient.send_batch`, which splits messages into calls of at most `MAX_MESSAGES_PER_CALL = 50`.
- `legacy.py`: `import_legacy(path)` returns `Appointment` objects from the old CSV.
- `waitlist.py`: `WaitlistEntry` and `Waitlist` (`add`, `remove`, `entries`). One entry per patient. `entries()` lists urgent entries first, then by `requested_at`.
- `cli.py` and `__main__.py`: the `day` and `patients` subcommands.
- `CHANGELOG.md`: `Unreleased` section updated in `09978f4`.
- Tests: `tests/helpers.py` provides `appt()` and `utc()`. `tests/data/legacy_sample.csv` is the importer fixture.

## Rules the user set (binding)
- **No overbooking, ever.** A therapist must never have two overlapping appointments, whatever the no-show or cancellation rate. This came from a Harbour incident in which double-booked 07:30 slots led to a patient complaint to the physiotherapy board. It is saved in project memory as a decision.
- **Privacy (Aroha Ngata, privacy officer):**
  - Any file that leaves the system (exports, reports, anything emailed) must contain patient IDs only, never names. Normalise IDs to `KP-######` first. The `Patient` model also holds `phone`, which must stay out of exports.
  - SMS reminders may contain only the date, the time in Pacific/Auckland, and the clinic name. No appointment kind and no treatment.
- **Python 3.9 compatibility:** no `X | None`, `match`, walrus, or other 3.10+ syntax anywhere.
- **Stdlib only.**
- **Commits:** the user asks for them explicitly each time. Stage files by name. Keep unrelated changes in separate commits. Use the existing message style, and keep the `Co-Authored-By: Claude Haiku 5.5` trailer that earlier commits carry. The trailer was added by the assistant, not requested by the user, so confirm it if unsure.
- **Pricing:** do not touch `pricing.py` until the board announces the price change.

## Decisions taken and their reasons
- `Calendar.all()` sorts by `start`, not `id`. The documented contract is start order.
- Times are stored UTC. Day queries and display convert to Pacific/Auckland, so `day` arguments are local dates.
- `cancel` works only on booked appointments. Cancelling an already-cancelled, attended or no-show appointment raises `ValueError`, so the original `cancelled_at` cannot be overwritten. It raises `KeyError` on an unknown id and `ValueError` on a naive time.
- `move` also works only on booked appointments. The user asked only that cancelled appointments be refused. Attended and no-show are refused too, as a choice the assistant made. Confirm it.
- `save` is atomic. It writes a temp file in the target directory, fsyncs it, then runs `os.replace`. A failed write or rename leaves the old file unchanged and removes the temp file. New files get mode 0600, because `mkstemp` creates them that way. Existing files keep their mode. There is no directory fsync, so a power cut could in theory lose the rename. Both points were flagged for the user, who has not responded.
- `Patient.csc` defaults to `False`. Old files without the key still load.
- `Appointment.cancelled_at` is optional. Old files without the key still load.
- The `day` command omits cancelled appointments, a choice the assistant made and flagged. The `patients` command prints names to the terminal only, which is allowed because nothing is written to a file.
- The CLI exits 1 for an unreadable or non-JSON data file, and exits 2 for bad arguments.
- `make_invoice` leaves `patient_name=""` and rejects an appointment that belongs to a different patient. The `Invoice.patient_name` field is still there and could be removed. Flagged, not done.

## Pending requirements (not built)
1. **Overlap check, not yet written.** `Calendar.add` and `move` accept overlapping bookings. The assistant asked whether to add the check, with tests, and the user has not answered. The rule above is standing, so this is a strong candidate. Durations are not stored on `Appointment`, so the check should derive them from `kind`: 45 minutes for an assessment and 30 for a follow-up. Compare half-open intervals `[start, start+duration)` per therapist. Existing data has not been checked for overlaps. `import_legacy` does not go through `Calendar`, so it does not enforce the check.
2. **Booking-form rules** (saved to memory): assessments 45 minutes, follow-ups 30. Starts on a quarter hour. A 15-minute notes gap after an assessment, none after a follow-up. A cap of 8 patient appointments per therapist per day under the union agreement. Clinic hours (Mon–Fri 07:30–18:00 for all three clinics; Sat 08:00–12:00 for NG only; Sun closed) are also needed. The assistant assumed an appointment must end by closing time. Confirm.
3. **Late-cancellation fee.** Cancelling less than 48 hours before the start costs 50% of the session price. The 24-hour rule was superseded by a board decision on 2026-11-12. The exact boundary, strictly less than 48 hours or inclusive, is not confirmed. A no-show pays full price. The data needed is in place: `start` and `cancelled_at`.
4. **Price rise on 1 April 2027:** assessment $105 and follow-up $78. Appointments before 1 April keep the old prices. Do not change `pricing.py` until the board announces. Open questions: whether the date rule uses the appointment start in Pacific/Auckland (the assistant's suggestion) or the booking date; and whether the CSC discount goes in the same change. `session_price(kind)` takes no date or patient, so the signature will change, and callers need updating.
5. **CSC discount:** 20% off follow-ups only, not assessments. Not applied. Rounding of cents is unspecified. It needs the patient's `csc` flag, so the signature or a new function is needed.
6. **Invoice numbers:** `INV-<clinic>-<year>-<5-digit seq>`, sequence per clinic, restarting each year. Example: `INV-HB-2027-00042`. Not built. Open: which date sets the year, and where the sequence is persisted.
7. **Monthly Xero export** for Mere Tui Accounting: CSV with a header row, dates DD/MM/YYYY, amounts as plain numbers with two decimals and no dollar sign, file `kestrel-invoices-YYYY-MM.csv`. Patient IDs only. The Xero column names (for example `*ContactName`, `*InvoiceNumber`) are unverified. Confirm with Mere before building.
8. **SMS reminder builder:** not written. Content is limited to date, time and clinic name, as above. `BulkieClient` sends whatever text it is given, so the rule belongs in the builder.
9. **Waitlist offering logic:** not built. Urgent entries go first, then request order.
10. **Legacy import into live data:** `import_legacy` returns objects, but no code writes them to a data file or creates therapist records. The file has 420 rows: NG 140, HB 153, WK 127. Statuses: attended 214, no_show 77, cancelled 65, booked 64. Kinds: follow-up 314, assessment 106. Dates run 01/09/2026 to 31/10/2026. All IDs are already canonical. The file uses therapist codes T01 to T08.

## Open items from the original request
- The user first said "something in the calendar is off". The only confirmed defect was the sort order, now fixed. The assistant asked for the specific symptom and has had no reply. Clinic filtering and the legacy data have not been checked.

## Pitfalls
- A brief note on the CHANGELOG: the `Unreleased` section lists only work that exists. Planned features are left out. The `day` command's omission of cancelled appointments is not documented there.
- The assistant's earlier drafts had several bugs that were then fixed: a circular assertion in `test_ids`, a wrong boundary case in `test_reports`, a missing `datetime` import in `test_storage`, and a placeholder for line numbers in `legacy.py`. Check new tests carefully for the same kinds of mistake.
- `ast` checks catch syntax only. They do not catch runtime 3.9 issues, so check any new stdlib usage against 3.9 docs.
