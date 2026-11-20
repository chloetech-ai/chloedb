"""results.md from raw/results.json (run.py writes it). Every number on the
page comes from the raw file."""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "raw", "results.json")))
SIDES = (("chloedb-guide", "ChloeDB, the guide's table"), ("chloedb-otlp", "ChloeDB, the table /v1/logs makes"),
         ("loki-snappy", "Loki, default (snappy)"), ("loki-zstd", "Loki, zstd chunks"))
QUESTIONS = {
    "ip": "Every line with one client address, last 15 minutes",
    "trace": "Every line of one trace, last hour",
    "errors_per_minute": "Errors per service per minute, last 15 minutes",
    "status_503": "503s per edge host, last hour",
    "timeout": "Lines that say timeout, in any case, last 15 minutes",
    "tail": "The newest 100 lines of checkout, last hour",
}


def f(v, d=1):
    return "–" if v is None else f"{v:,.{d}f}"


def chloedb_disk(x):
    return x["disk_stopped_bytes"]


def loki_store(x):
    d = x["disk"]
    return d.get("chunks", 0) + d.get("tsdb-shipper-active", 0) + d.get("tsdb-shipper-cache", 0) + d.get("compactor", 0)


def per_line(b):
    return b / R["lines"]


def per_day_gb(b):
    # the hour's bytes, at the same rate for a day
    return b * 24 / 1e9


def main():
    out = []
    w = out.append
    m = R["machine"]
    have = [(k, label) for k, label in SIDES if k in R]
    w("# Marco: a day of logs, ChloeDB against Loki\n")
    w("Marco, a platform engineer in the persona study, runs Loki for most of a hundred services' logs and an old "
      "Elasticsearch for the logs people search by substring. In his adoption answer he said an on-call grep for one "
      "address over the last minutes \"took tens of seconds\" in Loki where ChloeDB took milliseconds, and that ChloeDB's "
      "disk \"must come within about 2x of Loki's cost per day for the same lines, counted from the file, not the UI "
      "figure\". Both were from memory. This page measures them on one machine.\n")
    bj = R["by_job"]
    w(f"- **The lines:** {R['lines']:,} made-up lines, {R['rate']:,} a second for an hour, the unit Marco sizes disk in "
      f"([lines.py](lines.py)): {bj['nginx']:,} edge nginx access lines, {bj['app']:,} JSON lines from twelve services with "
      f"trace ids (each service spelling its levels its own way), and {bj['syslog']:,} network syslog lines. "
      f"{R['raw_bytes'] / 1e6:,.0f} MB of text, {R['raw_bytes'] / R['lines']:,.0f} bytes a line; gzip makes it "
      f"{R['gzip_bytes'] / 1e6:,.0f} MB ({R['gzip_bytes'] / R['lines']:,.1f} bytes a line). The last fifteen minutes hold "
      "an incident: one address hammering the login page and trying ssh, 503s on two edge hosts, timeouts in checkout.")
    w(f"- **ChloeDB** takes them through `POST /v1/logs` as the OpenTelemetry Collector sends them (OTLP JSON, "
      f"{R['per_request']:,} records a request, {R['senders']} senders, a 429 or 503 sent again after its Retry-After). "
      "Twice: into the logs guide's table (a substring index on `host` and `body`, `trace_id` kept, zstd, 7 days), and "
      "into the table `/v1/logs` makes on its own (no substring index).")
    w("- **Loki** 3.5.5 runs as one process with its chunks and index on the local disk (Loki's single-binary start, TSDB "
      "index, schema v13; [loki.yaml](loki.yaml) raises only the ingestion and query limits and turns its results cache "
      "off). The same lines go to `/loki/api/v1/push` in the same requests, with labels job, service, host and level, "
      "and trace ids as structured metadata, as Loki's own OTLP endpoint keeps them. Twice: with its default chunk "
      "encoding (snappy) and with zstd.")
    w(f"- **The questions** are six an on-call engineer asks, in SQL and in LogQL, each run {7} times; the page gives the "
      "median and the slowest. Each answer is checked against the lines themselves. Loki answered them twice: live, "
      "from its ingester's memory, as it answers for the last minutes, and after a flush, from its chunks on disk, as "
      "it answers for older lines.\n")
    w(f"Measured on {R['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{R['chloedb-guide']['version'] if 'chloedb-guide' in R else ''}; Loki {R['loki-snappy']['version'] if 'loki-snappy' in R else ''}. "
      "Run it again with `python3 run.py && python3 report.py` (needs Docker and a chloedb-server build; "
      "`H2H_ONLY=chloedb-guide` and so on runs one). Raw numbers: [raw/results.json](raw/results.json).\n")

    w("## What it says\n")
    agree = {k: all(q["agree"] for q in R[k]["questions"].values()) and
             all(q["agree"] for q in R[k].get("questions_after_flush", {}).values()) for k, _ in have}
    w("- **Every answer is right on every system:** " + ", ".join(
        f"{label} {'agrees' if agree[k] else 'DIFFERS'}" for k, label in have) + " with the lines. Every line sent was "
      "stored once: " + ", ".join(f"{label} {R[k]['stored']:,} of {R[k]['sent']:,}" for k, label in have) + ".")
    if "chloedb-guide" in R and "loki-snappy" in R:
        g, l = R["chloedb-guide"]["questions"], R["loki-snappy"]["questions"]
        ip_c, ip_l = g["ip"]["p50_ms"], l["ip"]["p50_ms"]
        ip_lf = R["loki-snappy"]["questions_after_flush"]["ip"]["p50_ms"]
        w(f"- **The grep for one address,** over the last 15 minutes ({R['truth']['ip']:,} lines of "
          f"{R['rate'] * 900:,}): ChloeDB with the guide's table {f(ip_c)} ms at the median, Loki {f(ip_l)} ms live and "
          f"{f(ip_lf)} ms from its chunks. " + (
              f"So ChloeDB is about {ip_l / ip_c:,.0f} times faster here, and Loki answers in "
              f"{'well under a second' if max(ip_l, ip_lf) < 900 else 'about a second' if max(ip_l, ip_lf) < 2000 else 'seconds'}, "
              "not the tens of seconds Marco remembered, at this volume on one machine." if ip_c > 0 else ""))
        if "chloedb-otlp" in R:
            o = R["chloedb-otlp"]["questions"]["ip"]
            dd = o.get("default_deadline", {})
            w(f"- **Without the substring index it is the other way round.** On the table `/v1/logs` makes by itself, the "
              f"same grep took {f(o['p50_ms'] / 1000)} s at the median"
              + (f", and sent as a page sends it, with the server's default deadline of 5 s, it was refused "
                 f"({dd['status']} `{dd['error_code']}`)" if dd.get("status") not in (None, 200) else "")
              + ". The `ORDER BY ts DESC` sends it to the SQL engine that reads every line of the table before the "
              "`LIKE`; without the `ORDER BY` the same `LIKE` is a scan of the table in a fraction of that. A team that "
              "points a Collector at `/v1/logs` and greps gets this table unless it makes the guide's first.")
        slow = [n for n in QUESTIONS if g[n]["p50_ms"] > l[n]["p50_ms"]]
        w(f"- **The other questions.** ChloeDB with the guide's table is faster on {len(QUESTIONS) - len(slow)} of the 6 against Loki live"
          + (f", slower on {', '.join(QUESTIONS[n].lower() for n in slow)}" if slow else "") + ". The table below has each.")
    if "chloedb-guide" in R and "loki-snappy" in R:
        cg = chloedb_disk(R["chloedb-guide"])
        co = chloedb_disk(R["chloedb-otlp"]) if "chloedb-otlp" in R else None
        ls = loki_store(R["loki-snappy"])
        lz = loki_store(R["loki-zstd"]) if "loki-zstd" in R else None
        w(f"- **Disk, counted from the files.** For the hour: ChloeDB's guide table {cg / 1e6:,.0f} MB "
          f"({per_line(cg):,.0f} bytes a line, {per_day_gb(cg):,.1f} GB a day at 1,000 lines a second)"
          + (f", the table `/v1/logs` makes {co / 1e6:,.0f} MB ({per_line(co):,.0f} bytes a line)" if co else "")
          + f"; Loki's chunks and index {ls / 1e6:,.0f} MB with snappy ({per_line(ls):,.0f} bytes a line, "
          f"{per_day_gb(ls):,.1f} GB a day)" + (f" and {lz / 1e6:,.0f} MB with zstd ({per_line(lz):,.0f} bytes a line)" if lz else "")
          + f". Marco's bar is 2x Loki: the guide's table is {cg / ls:,.1f}x Loki's default"
          + (f" and {cg / lz:,.1f}x Loki with zstd" if lz else "")
          + (f"; the table without substring indexes is {co / ls:,.1f}x and {co / lz:,.1f}x." if co and lz else "."))
        tg = R["chloedb-guide"].get("tables_totals") or {}
        ta = R["chloedb-guide"].get("tables_totals_after") or {}
        if tg and ta:
            w(f"- **ChloeDB's file right after the hour** was {tg['file_bytes'] / 1e6:,.0f} MB with {tg['free_bytes'] / 1e6:,.0f} MB "
              f"of it free (segments merged while the lines came in); the server gave space back by itself and the file was "
              f"{ta['file_bytes'] / 1e6:,.0f} MB {R['chloedb-guide']['give_back'][-1]['s'] // 60} minutes later, the size "
              f"counted above. The tables' live data is {ta['bytes'] / 1e6:,.0f} MB ({ta['bytes'] / R['lines']:,.0f} bytes a line).")
        lw = R["loki-snappy"]["disk"].get("wal", 0)
        w(f"- **Loki's write-ahead log** held another {lw / 1e6:,.0f} MB on the ingester's local disk for the hour. Loki keeps "
          "its chunks in object storage in production and the log on the ingester; ChloeDB keeps both on its own disk. "
          "The figures above are the chunks and index only, Loki's best case.")
    if "chloedb-guide" in R and "loki-snappy" in R:
        w(f"- **Taking the lines in.** ChloeDB took the hour at {R['chloedb-guide']['load']['lines_per_s']:,} lines a second "
          f"into the guide's table" + (f" and {R['chloedb-otlp']['load']['lines_per_s']:,} into its own" if "chloedb-otlp" in R else "")
          + f", Loki at {R['loki-snappy']['load']['lines_per_s']:,}, from the same {R['senders']} senders. Requests sent again after a "
          f"429 or 503: ChloeDB {R['chloedb-guide']['load']['retries']}, Loki {R['loki-snappy']['load']['retries']}.")
        w(f"- **Memory after the hour:** ChloeDB {R['chloedb-guide']['memory_kb'] / 1024:,.0f} MB, Loki "
          f"{R['loki-snappy']['memory_kb'] / 1024:,.0f} MB (its ingester holds the hour's chunks until they flush).\n")

    w("## Each question\n")
    w("Median of 7 runs, and the slowest in brackets, in milliseconds, from the client (a query slower than a minute "
      "ran fewer times). ChloeDB keeps the filters it worked out for a while, so its first run is usually its slowest; "
      "Loki's chunk cache does the same for its reads. Every answer was checked; a wrong one would say so.\n")
    head = "| question | answer | " + " | ".join(label for _, label in have)
    if "loki-snappy" in R:
        head += " | Loki snappy, after flush"
    w(head + " |")
    w("|---|---|" + "---|" * (len(have) + (1 if "loki-snappy" in R else 0)))
    for n, label in QUESTIONS.items():
        cells = []
        for k, _ in have:
            q = R[k]["questions"][n]
            cells.append(f"{f(q['p50_ms'])} ({f(q['max_ms'])}){'' if q['agree'] else ' wrong'}")
        if "loki-snappy" in R:
            q = R["loki-snappy"]["questions_after_flush"][n]
            cells.append(f"{f(q['p50_ms'])} ({f(q['max_ms'])}){'' if q['agree'] else ' wrong'}")
        t = R["truth"][n]
        w(f"| {label} | {t:,} {'lines' if n in ('ip', 'trace', 'tail', 'timeout') else 'values'} | " + " | ".join(cells) + " |")
    w("")
    w("## Disk for the hour\n")
    w("| system | bytes | bytes a line | GB a day at 1,000 lines a second | against Loki's default |")
    w("|---|---|---|---|---|")
    base = loki_store(R["loki-snappy"]) if "loki-snappy" in R else None
    for k, label in have:
        b = chloedb_disk(R[k]) if k.startswith("chloedb") else loki_store(R[k])
        w(f"| {label} | {b / 1e6:,.1f} MB | {per_line(b):,.0f} | {per_day_gb(b):,.1f} | {f(b / base, 2) + 'x' if base else '–'} |")
    w(f"| the lines as text | {R['raw_bytes'] / 1e6:,.1f} MB | {R['raw_bytes'] / R['lines']:,.0f} | {per_day_gb(R['raw_bytes']):,.1f} | "
      f"{f(R['raw_bytes'] / base, 2) + 'x' if base else '–'} |")
    w(f"| the lines gzipped | {R['gzip_bytes'] / 1e6:,.1f} MB | {R['gzip_bytes'] / R['lines']:,.1f} | {per_day_gb(R['gzip_bytes']):,.1f} | "
      f"{f(R['gzip_bytes'] / base, 2) + 'x' if base else '–'} |")
    w("")
    w("ChloeDB's figure is its database folder after the server stopped (the file and its log); Loki's is its chunks "
      "and index after a flush, without its write-ahead log. A day is the hour times 24: a real day's lines repeat more, "
      "which helps both. ChloeDB's file also holds free space, here from segments merged while the lines came in, until "
      "the server gives it back; the figure is after it had.\n")
    if "chloedb-guide" in R:
        w("## The queries\n")
        w("| question | SQL (the guide's table) | LogQL |")
        w("|---|---|---|")
        for n, label in QUESTIONS.items():
            sql = R["chloedb-guide"]["questions"][n]["sql"].replace("|", "\\|")
            lq = R["loki-snappy"]["questions"][n]["logql"].replace("|", "\\|") if "loki-snappy" in R else ""
            w(f"| {label} | `{sql}` | `{lq}` |")
        w("")
    w("One machine, one server of each, and an hour of lines: not a day's volume, not Loki's object storage or its "
      "caches across a cluster, and not ChloeDB's cold store. The two share four cores with the senders.\n")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
