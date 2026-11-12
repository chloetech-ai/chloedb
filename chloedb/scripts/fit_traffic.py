"""Fits a traffic profile's distributions to data, for engine-compare's
traffic simulator (chloedb-server/deploy/compare/profiles). Prints the
fitted values as TOML to paste into a profile.

  python3 scripts/fit_traffic.py zipf COUNTS [--head 100000]
      Zipf's exponent of the query head and the share of one-off queries,
      from a query log: a TSV of `query<TAB>count`, or ORCAS's click log
      (orcas.tsv.gz: qid, query, doc, url). ORCAS has a line for each
      distinct document clicked for a query, not for each search, so its
      counts flatten the head (it fits 0.35, and 34% of lines a query
      seen once); a log of searches fits better. Rank-frequency least squares over
      ranks 1..head on log-log axes; the tail share is the share of the
      log's lines whose query appears once.

  python3 scripts/fit_traffic.py day [--project en] [--start 2026-10-01] [--days 14] [--dir DIR]
      The day's shape from Wikipedia's hourly page views (Wikimedia's
      projectviews dumps, dumps.wikimedia.org/other/pageviews: one small
      file an hour, desktop and mobile summed): the mean of each UTC hour,
      over the days, as a share of the busiest hour. Prints the peak over
      the trough, where the day peaks and whether one hump or two fits
      better (a raised cosine of each), for a day compressed into the
      profile's day_s. Files go into --dir (default ./projectviews) and are
      read from there on a second run.

The numbers fitted here are inputs, not results: a profile names where
its values came from in its comments.
"""
import argparse
import collections
import gzip
import math
import os
import subprocess
import sys
import datetime as dt

UA = "strata-engine-compare/1.0 (https://github.com/chloetech-ai/chloedb)"


def zipf(a):
    op = gzip.open if a.counts.endswith(".gz") else open
    counts = collections.Counter()
    lines = 0
    with op(a.counts, "rt", encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) == 2 and parts[1].strip().isdigit():
                counts[parts[0].strip().lower()] += int(parts[1])
            elif len(parts) >= 2:
                # ORCAS: qid, query, doc, url (a line a clicked document).
                counts[parts[1].strip().lower()] += 1
            lines += 1
    if not counts:
        sys.exit(f"{a.counts}: no queries")
    freq = sorted(counts.values(), reverse=True)
    total = sum(freq)
    once = sum(1 for c in freq if c == 1)
    head = freq[: a.head]
    xs = [math.log(r) for r in range(1, len(head) + 1)]
    ys = [math.log(c) for c in head]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    sxx = sum((x - mx) ** 2 for x in xs)
    s = -sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sxx if sxx > 0 else 0.0
    head_share = sum(head) / total
    print(f"# {a.counts}: {lines} lines, {len(freq)} distinct queries, {total} occurrences")
    print(f"# the {len(head)} most frequent queries take {100 * head_share:.1f}% of occurrences; {once} queries ({100 * once / total:.1f}% of occurrences) appear once")
    print("[queries]")
    print(f"head = {len(head)}")
    print(f"zipf_s = {s:.3f}")
    print(f"tail_share = {once / total:.3f}")


def fetch_hour(d, when, project):
    name = f"projectviews-{when:%Y%m%d-%H}0000"
    path = os.path.join(d, name)
    if not os.path.exists(path):
        url = f"https://dumps.wikimedia.org/other/pageviews/{when:%Y}/{when:%Y-%m}/{name}"
        tmp = path + ".part"
        r = subprocess.run(["curl", "-sSfL", "--retry", "3", "-A", UA, "-o", tmp, url])
        if r.returncode != 0:
            return None
        os.rename(tmp, path)
    n = 0
    for line in open(path, encoding="utf-8", errors="replace"):
        parts = line.split()
        if len(parts) >= 3 and parts[0] in (project, f"{project}.m"):
            n += int(parts[2])
    return n


def cosine_fit(shape, humps):
    """The best raised cosine with `humps` peaks a day (least squares over the peak's hour): its error, peak hour and trough."""
    best = None
    for k in range(240):
        at = k / 240
        c = [(1 + math.cos(2 * math.pi * humps * (h / 24 - at))) / 2 for h in range(24)]
        # shape ~ low + (1 - low) c: least squares for low with the peak at 1.
        num = sum((1 - c[h]) * (shape[h] - c[h]) for h in range(24))
        den = sum((1 - c[h]) ** 2 for h in range(24))
        low = min(max(num / den if den else 1.0, 0.01), 1.0)
        err = sum((low + (1 - low) * c[h] - shape[h]) ** 2 for h in range(24))
        if best is None or err < best[0]:
            best = (err, at, low)
    return best


def day(a):
    os.makedirs(a.dir, exist_ok=True)
    start = dt.datetime.fromisoformat(a.start)
    by_hour = collections.defaultdict(list)
    missing = 0
    for i in range(a.days * 24):
        when = start + dt.timedelta(hours=i)
        n = fetch_hour(a.dir, when, a.project)
        if n is None:
            missing += 1
            continue
        by_hour[when.hour].append(n)
    if len(by_hour) < 24:
        sys.exit(f"only {len(by_hour)} hours of the day could be read ({missing} files missing)")
    mean = [sum(by_hour[h]) / len(by_hour[h]) for h in range(24)]
    top = max(mean)
    shape = [m / top for m in mean]
    fits = {h: cosine_fit(shape, h) for h in (1, 2)}
    humps = min(fits, key=lambda h: fits[h][0])
    err, at, low = fits[humps]
    print(f"# {a.project}.wikipedia (desktop + mobile), {a.days} days from {a.start}, UTC; {missing} hourly files missing")
    print("# the mean of each hour as a share of the busiest: " + " ".join(f"{s:.2f}" for s in shape))
    print(f"# raw peak over trough {max(mean) / min(mean):.2f}; {humps} hump(s) fit best (squared error {err:.4f}, the other {fits[3 - humps][0]:.4f})")
    print("[traffic]")
    print(f"peak_to_trough = {1 / low:.2f}")
    print(f"humps = {humps}")
    print(f"peak_at = {at:.3f}")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    z = sub.add_parser("zipf")
    z.add_argument("counts")
    z.add_argument("--head", type=int, default=100_000)
    d = sub.add_parser("day")
    d.add_argument("--project", default="en")
    d.add_argument("--start", default="2026-10-01")
    d.add_argument("--days", type=int, default=14)
    d.add_argument("--dir", default="projectviews")
    a = ap.parse_args()
    {"zipf": zipf, "day": day}[a.cmd](a)


if __name__ == "__main__":
    main()
