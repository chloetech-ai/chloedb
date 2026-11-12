Project: `kestrel`, a stdlib-only Python booking library for Kestrel Physio (clinics NG, HB, WK; Auckland). Test with `PYTHONPATH=src python -m unittest discover -s tests` (99 passing). Work is on branch `calendar-queries`, not `main`. Commit as Dana with a Haiku co-author trailer, staging files by name. `__pycache__/` stays untracked (no .gitignore). Nothing is uncommitted.

Rules:
- Target Python 3.9: no `X | None` or other 3.10+ syntax. Check with `ast.parse(..., feature_version=(3, 9))`.
- Privacy (Aroha Ngata): outgoing files (exports, invoices, reports, email) carry patient IDs only, never names. SMS reminders contain only date, time (Pacific/Auckland) and clinic name, with no kind or treatment.
- Never overbook: a therapist must never have overlapping appointments.
- Starts are stored in UTC. Local dates use `calendar.CLINIC_TZ`.
- Don't touch `pricing.py` until the board announces the price change.

Built: Calendar (add, cancel, move, for_day, for_therapist, for_patient, booked, all sorted by start), Patient.csc, Appointment.cancelled_at, atomic storage.save, normalize_patient_id, session_price (9500/7000 cents), BulkieClient batches of 50, import_legacy, make_invoice (patient_name empty), therapist_load, Waitlist, CLI `day` and `patients`.

Planned, not built:
- Overlap check in `Calendar.add`/`move`. This is required by the no-overbook rule and is missing.
- From 1 Apr 2027: assessment $105, follow-up $78. The rule for "before 1 April" (start vs booking date) is undecided.
- Cancelling under 48h: 50% fee. No-show: full price. CSC: 20% off follow-ups only.
- Booking: assessments 45 min, follow-ups 30, starts on quarter hours, 15 min gap after assessments, max 8 patient appointments per therapist per day.
- Hours: Mon–Fri 07:30–18:00 (all clinics); Sat NG 08:00–12:00; Sun closed. Confirm whether appointments must end by closing.
- Invoice numbers: INV-<clinic>-<year>-<5-digit seq>, counted per clinic, restarting yearly.
- Xero export: CSV with header, DD/MM/YYYY dates, two-decimal amounts, named kestrel-invoices-YYYY-MM.csv. Column names are unconfirmed with Mere.
- SMS reminder builder; waitlist offer logic (urgent first).

Known gaps: legacy patient IDs aren't normalised on load (only queries are). The `day` CLI hides cancelled appointments, which isn't documented.
