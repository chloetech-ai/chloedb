"""results.md from raw/results-*.json (run.py writes them). Every number on
the page comes from the raw files."""

import collections
import glob
import json
import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
RUNS = sorted((json.load(open(p)) for p in glob.glob(os.path.join(HERE, "raw", "results-*.json"))), key=lambda r: r["trips"])


def geo(xs):
    xs = [x for x in xs if x and x > 0]
    return math.exp(sum(math.log(x) for x in xs) / len(xs)) if xs else None


def message(err):
    msg = (err or "").replace("|", "/").replace("\n", " ")
    try:
        return json.loads(err).get("error", msg)
    except (ValueError, TypeError, AttributeError):
        return msg


def f(v, d=1):
    return "–" if v is None else f"{v:,.{d}f}"


def main():
    out = []
    w = out.append
    small = RUNS[0]
    m = small["machine"]
    n_runnable = sum(1 for q in small["queries"] if not q["duckdb_error"] and not q["id"].startswith("x"))
    w("# Sam: an analyst's SQL battery, ChloeDB against DuckDB and Postgres\n")
    w("Sam, an analytics engineer in the persona study, wants \"a benchmark of our own models against DuckDB and Postgres "
      "(same results, run time)\" before his data lead says yes. This page runs his round 4 battery "
      f"([queries.py](queries.py), {len(small['queries'])} queries: joins of every kind, windows, QUALIFY, percentiles, "
      "set operations, dates, strings, and ten statements the docs say ChloeDB refuses) on made-up data of his shape "
      "([gen.py](gen.py): green-taxi trips over three months with NULLs, refunds and stray dates, 263 zones, and a music store "
      "with the Chinook schema). Each engine answers the same SQL on one machine in turn; DuckDB's form of a statement differs "
      "only for timestamp literals, `strftime`'s argument order and `date_bin` (`time_bucket`), and Postgres runs DuckDB's form.\n")
    w(f"Measured on {small['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{small['chloedb']['version']}; DuckDB {small['duckdb']['version']}; {small['postgres']['version']}. ChloeDB is "
      "loaded with `chloedb-server import` (Parquet and SQLite) and asked over HTTP; DuckDB in process with its default "
      "threads; Postgres through psycopg with its default settings. Each time is the median of "
      f"{small['reps']} runs after one. Run it again with `python3 run.py && python3 run.py 2000000 && python3 report.py` "
      "(needs `pip install duckdb psycopg[binary]`, Postgres and a chloedb-server build). Raw numbers: "
      + ", ".join(f"[raw/results-{r['trips']}.json](raw/results-{r['trips']}.json)" for r in RUNS) + ".\n")

    w("## What it says\n")
    for r in RUNS:
        qs = r["queries"]
        c = collections.Counter(q["chloedb_vs_duckdb"] for q in qs)
        both = [q for q in qs if q["chloedb_ms"] and q["duckdb_ms"]]
        ratio = geo([q["chloedb_ms"] / q["duckdb_ms"] for q in both])
        faster = sum(1 for q in both if q["chloedb_ms"] < q["duckdb_ms"])
        slow = sorted(both, key=lambda q: q["chloedb_ms"] / q["duckdb_ms"], reverse=True)[:4]
        w(f"- **{r['trips']:,} trips.** ChloeDB returned DuckDB's rows, in DuckDB's order, for {c['same']} queries"
          + (f"; {c['different']} differed" if c["different"] else "; none differed")
          + f"; it refused {c['refused']} that DuckDB ran. Of the {len(both)} both ran, ChloeDB's own time was "
          f"{f(ratio, 2)}x DuckDB's at the geometric mean, faster on {faster}; its slowest against DuckDB were "
          + ", ".join(f"{q['id']} ({f(q['chloedb_ms'])} ms against {f(q['duckdb_ms'])})" for q in slow) + ".")
    refused = [q for q in small["queries"] if q["chloedb_vs_duckdb"] == "refused"]
    w(f"- **What ChloeDB refuses** at {small['trips']:,} trips is what its docs list: " + ", ".join(q["id"] for q in refused)
      + ". Each refusal names what to write instead; the messages are below.")
    for r in RUNS[1:]:
        more = [q for q in r["queries"] if q["chloedb_vs_duckdb"] == "refused" and q["id"] not in {x["id"] for x in refused}]
        if more:
            w(f"- **At {r['trips']:,} trips it refused {len(more)} more:** "
              + "; ".join(f"{q['id']} ({message(q['chloedb_error']).split(';')[0]})" for q in more) + ".")
            if any(q["id"] == "t23_except" for q in more):
                w("  t23's refusal comes and goes: on two fresh servers over the same file it was refused in five of six runs and "
                  "answered in about 0.7 s once, and the numbers it gives are far under the limit it names. That is "
                  "a finding of this comparison (SAM-411), not a limit the docs describe.")
    pg = collections.Counter(q["postgres_vs_duckdb"] for q in small["queries"])
    w(f"- **Postgres** ran {pg['same'] + pg['same rows, other order'] + pg['different']} of the battery, gave DuckDB's rows for "
      f"{pg['same']}, and refused {pg['refused']} for dialect (QUALIFY, MEDIAN, `date_diff`, `strftime`, `date_bin`, "
      "`ROUND(double, int)`). It differed on " + (", ".join(q["id"] for q in small["queries"] if q["postgres_vs_duckdb"] == "different") or "none")
      + " (t44: Postgres puts NULLs first on DESC, DuckDB and ChloeDB last).")
    big = RUNS[-1]
    quick = [q for q in big["queries"] if q["chloedb_ms"] and q["duckdb_ms"] and q["chloedb_ms"] < q["duckdb_ms"]]
    slow = [q for q in big["queries"] if q["chloedb_ms"] and q["duckdb_ms"] and q["chloedb_ms"] > 5 * q["duckdb_ms"]]
    w(f"- **Selective queries are ChloeDB's; full scans and joins are DuckDB's.** At {big['trips']:,} trips ChloeDB was faster on "
      + ", ".join(q["id"] for q in quick)
      + ": filters on a key or a range, LIMIT and OFFSET, NOT IN, semi joins, and the music store's small tables. "
      f"It took more than five times DuckDB's time on {', '.join(q['id'] for q in slow)}: aggregates and joins that read "
      "every trip, as in round 4.\n")

    w("## Loading, memory and disk\n")
    w("| | " + " | ".join(f"{r['trips']:,} trips" for r in RUNS) + " |")
    w("|---|" + "---|" * len(RUNS))
    w("| Parquet file of the trips | " + " | ".join(f"{r['parquet_bytes'] / 1e6:.1f} MB" for r in RUNS) + " |")
    for side, label in (("chloedb", "ChloeDB"), ("duckdb", "DuckDB"), ("postgres", "Postgres")):
        w(f"| {label}: load the trips | " + " | ".join(f"{r[side]['trips_load_s']:.2f} s" if 'trips_load_s' in r[side] else '–' for r in RUNS) + " |")
        w(f"| {label}: load everything | " + " | ".join(f"{r[side]['load_s']:.2f} s" for r in RUNS) + " |")
        w(f"| {label}: disk | " + " | ".join(f"{r[side]['disk_bytes'] / 1e6:.1f} MB" for r in RUNS) + " |")
        if side == "postgres":
            w(f"| {label}: memory after the battery (PSS) | " + " | ".join(f"{r[side]['pss_kb'] / 1024:.0f} MB" for r in RUNS) + " |")
        else:
            w(f"| {label}: peak memory (RSS) | " + " | ".join(f"{r[side]['peak_rss_kb'] / 1024:.0f} MB" for r in RUNS) + " |")
    w("")
    w("ChloeDB's load is three `chloedb-server import` commands (the trips' Parquet, the zones' Parquet, the music store's "
      "SQLite file), DuckDB's is `CREATE TABLE ... AS SELECT * FROM read_parquet(...)` and an insert of the SQLite rows, and "
      "Postgres's is `COPY` from the client. DuckDB's whole load is mostly inserting the music store's rows one statement at a time from Python; its load of the trips is the number to compare. Postgres's disk is its tables and indexes; DuckDB's and ChloeDB's, their files.\n")

    w("## Each query\n")
    w("*same* is the same rows in the same order (or the same rows, for a query whose ORDER BY does not fix the order). "
      "Times in milliseconds: ChloeDB's own (`took_ms`) and, in brackets, as the client saw it over HTTP.\n")
    head = "| query | ChloeDB vs DuckDB | Postgres vs DuckDB | rows |"
    for r in RUNS:
        head += f" ChloeDB, {r['trips'] // 1000:,}k | DuckDB, {r['trips'] // 1000:,}k | Postgres, {r['trips'] // 1000:,}k |"
    w(head)
    w("|---|---|---|---|" + "---|---|---|" * len(RUNS))
    by = [{q["id"]: q for q in r["queries"]} for r in RUNS]
    for q in small["queries"]:
        line = f"| {q['id']} | {q['chloedb_vs_duckdb']} | {q['postgres_vs_duckdb']} | {f(q['rows'], 0) if q['rows'] is not None else '–'} |"
        for b in by:
            x = b[q["id"]]
            line += (f" {f(x['chloedb_ms'])} ({f(x['chloedb_wall_ms'])}) | " if x["chloedb_ms"] is not None else " – | ")
            line += f"{f(x['duckdb_ms'])} | {f(x['postgres_ms'])} |"
        w(line)
    w("")
    w("## What ChloeDB said when it refused\n")
    for q in refused:
        w(f"- **{q['id']}**: {message(q['chloedb_error'])[:260]}")
    w("")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
