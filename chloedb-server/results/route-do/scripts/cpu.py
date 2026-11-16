#!/usr/bin/env python3
"""Mean CPU busy share (and steal) of each node during each scenario, from the
pods' /proc/stat samples (cpu/<pod>.log) and the driver's log (drive.log)."""
import re, sys, os, datetime
S = sys.argv[1]
day = sys.argv[2] if len(sys.argv) > 2 else "2026-11-15"
win = {}
for line in open(os.path.join(S, "drive.log")):
    m = re.match(r"(\d\d:\d\d:\d\d)Z bench (\w+) (start|done)", line)
    if m:
        t = datetime.datetime.fromisoformat(f"{day}T{m[1]}+00:00").timestamp()
        win.setdefault(m[2], [None, None])[0 if m[3] == "start" else 1] = t
pods = sorted(f[:-4] for f in os.listdir(os.path.join(S, "cpu")) if f.endswith(".log"))
rows = {}
CLIENTS = [1, 16, 64]
for p in pods:
    samples = []
    for line in open(os.path.join(S, "cpu", p + ".log")):
        parts = line.split()
        if len(parts) < 10 or parts[1] != "cpu":
            continue
        v = list(map(int, parts[2:]))
        idle = v[3] + v[4]
        total = sum(v[:8])
        samples.append((float(parts[0]), total, idle, v[7]))
    for sc0, (a0, b0) in win.items():
      if a0 is None or b0 is None:
          continue
      # Clients 1, 16 and 64 take turns in that order, a third of the run each.
      for k, c in enumerate(CLIENTS):
        a, b = a0 + (b0 - a0) * k / 3, a0 + (b0 - a0) * (k + 1) / 3
        sc = f"{sc0} @{c}"
        sel = [s for s in samples if a <= s[0] <= b]
        if len(sel) < 2:
            continue
        dt = sel[-1][1] - sel[0][1]
        busy = 1 - (sel[-1][2] - sel[0][2]) / dt if dt else 0
        steal = (sel[-1][3] - sel[0][3]) / dt if dt else 0
        rows.setdefault(sc, {})[p] = (busy, steal)
print("| scenario | " + " | ".join(pods) + " |")
print("|---|" + "---|" * len(pods))
for sc, d in rows.items():
    print(f"| {sc} | " + " | ".join(f"{d[p][0]*100:.0f}%" + (f" (steal {d[p][1]*100:.0f}%)" if d.get(p, (0, 0))[1] > 0.005 else "") if p in d else "-" for p in pods) + " |")
