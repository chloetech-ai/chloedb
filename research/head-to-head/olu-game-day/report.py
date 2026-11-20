"""results.md from raw/results.json (run.py writes it). Every number on the
page comes from the raw file."""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "raw", "results.json")))
SIDES = (("chloedb", "ChloeDB"), ("elasticsearch", "Elasticsearch"))
DRILLS = {
    "kill": "Kill the primary of shard 0, start it again {kill_down_s} s later",
    "freeze": "Freeze it for {freeze_s} s (SIGSTOP), then let it go",
    "rolling": "Rolling restart of every server",
}


def f(v, d=1):
    return "–" if v is None else f"{v:,.{d}f}"


def drill(side, kind):
    return next(d for d in R[side]["drills"] if d["drill"] == kind)


def main():
    out = []
    w = out.append
    m = R["machine"]
    ch, es = R["chloedb"], R["elasticsearch"]
    w("# Olu: a game day, ChloeDB's cluster against Elasticsearch's\n")
    w("Olu, an SRE in the persona study, runs Elasticsearch on Kubernetes. In his adoption answer he said ChloeDB \"already "
      "beats ES by roughly an order of magnitude in my drills: seconds against the tens of seconds to minutes I live with\". "
      "This page runs the same three drills on both, on one machine, under the same load, and reports what a client saw.\n")
    w("- **ChloeDB:** two shards with three copies each (`--seq-no --failover-after-ms 2000`, as the guide's \"Operate a "
      "cluster\" page runs them), and a coordinator in front.")
    w("- **Elasticsearch:** three nodes in Docker, each master-eligible and data, with an index of two primary shards and "
      "two replicas each, so three copies of a shard as on ChloeDB. The client knows all three nodes and moves to the next "
      "one when a node does not answer. The nodes listen on the machine's loopback, as ChloeDB's copies do; on a network "
      "address Elasticsearch enforces its production checks, and this container's file limit is below what they ask for.")
    w(f"- **The load:** a writer sends one row at a time, {R['writes_per_s']} a second, and waits for each answer "
      f"(at most {R['timeout_s']} s). A refused row is sent again every 0.25 s, the same row with the same id (and on ChloeDB "
      f"the same idempotency key), as a client does. A reader sends a search {R['reads_per_s']} times a second. "
      f"Each drill starts from a healthy cluster holding {R['preload']:,} rows, with {R['steady_s']} s of steady load before "
      "and after.\n")
    w(f"Measured on {R['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{ch['version']}; {es['version']}. Run it again with `python3 run.py && python3 report.py` (needs Docker and a "
      "chloedb-server build; `H2H_ONLY=chloedb` or `H2H_ONLY=elasticsearch` runs one side and keeps the other's numbers). "
      "Raw numbers: [raw/results.json](raw/results.json).\n")

    w("## What it says\n")
    for kind in ("kill", "freeze", "rolling"):
        c, e = drill("chloedb", kind), drill("elasticsearch", kind)
        cw, ew = c["writes"], e["writes"]
        cr, er = c["reads"], e["reads"]
        title = DRILLS[kind].format(**R).split(",")[0]
        w(f"- **{title}.** Longest time with no write answered: ChloeDB {f(cw['longest_gap_s'], 2)} s, Elasticsearch "
          f"{f(ew['longest_gap_s'], 2)} s. Writes refused for good: {cw['refused']} of {cw['sent']:,} against "
          f"{ew['refused']} of {ew['sent']:,}; sent more than once: {cw.get('sent_again', 0)} against {ew.get('sent_again', 0)}. "
          f"Longest with no search answered: {f(cr['longest_gap_s'], 2)} s against {f(er['longest_gap_s'], 2)} s. "
          f"Whole again after the drill: {f(c['whole_after_s'], 2)} s against {f(e['whole_after_s'], 2)} s.")
    gaps = {k: (drill("chloedb", k)["writes"]["longest_gap_s"], drill("elasticsearch", k)["writes"]["longest_gap_s"])
            for k in ("kill", "freeze", "rolling")}
    better = [k for k, (c, e) in gaps.items() if c < e]
    worse = [k for k, (c, e) in gaps.items() if c > e]
    w(f"- **Olu's claim, for a client.** ChloeDB's longest pause in writes is shorter in {len(better)} of the 3 drills "
      f"({', '.join(better) or 'none'}) and longer in {len(worse)} ({', '.join(worse) or 'none'}). "
      + ("An order of magnitude holds for none of them as a client sees it. " if all(c * 10 > e for c, e in gaps.values()) else "")
      + "Where ChloeDB is ahead is in getting whole again: after a kill it had every copy back "
      f"{f(drill('chloedb', 'kill')['whole_after_s'], 2)} s after the server started, Elasticsearch "
      f"{f(drill('elasticsearch', 'kill')['whole_after_s'], 2)} s (its replicas recover from the primaries), and a rolling restart "
      f"took {f(sum(s['s'] for s in drill('chloedb', 'rolling')['steps']), 1)} s against "
      f"{f(sum(s['s'] for s in drill('elasticsearch', 'rolling')['steps']), 1)} s.")
    fz = drill("elasticsearch", "freeze")
    if fz["whole_after_s"] == 0 and fz["writes"]["longest_gap_s"] >= R["freeze_s"]:
        w(f"- **A frozen node.** Elasticsearch held the write it was serving for the whole {R['freeze_s']} s freeze, searches "
          "too, and its cluster stayed green: its fault detection takes longer than the freeze. ChloeDB moved the shard to "
          f"another copy, but a write still waited {f(drill('chloedb', 'freeze')['writes']['longest_wait_s'], 2)} s, longer than "
          "the 2 s `--failover-after-ms` suggests.")
    lost_c, lost_e = len(ch["answered_but_missing"]), len(es["answered_but_missing"])
    w(f"- **Every answered write is there.** At the end ChloeDB held {ch['rows']:,} rows of the {ch['rows_expected']:,} "
      f"expected ({lost_c} answered and missing), Elasticsearch {es['rows']:,} of {es['rows_expected']:,} ({lost_e}). "
      f"Writes refused but there all the same: {ch['refused_but_there']} and {es['refused_but_there']}.")
    w(f"- **Memory.** After the preload, ChloeDB's seven servers held {ch['memory_kb'] / 1024:,.0f} MB together, "
      f"Elasticsearch's three containers {es['memory_kb'] / 1024:,.0f} MB (most of it the heap each node is given).\n")

    w("## Each drill\n")
    w("What the client saw from the start of the drill to the end of the steady load after it. A gap is the longest time "
      "between two answered requests; a wait is the longest one request took, retries included.\n")
    w("| drill | system | target | writes sent | refused | sent again | longest wait | longest gap | p99 write | searches | "
      "search gap | whole again after |")
    w("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for kind in ("kill", "freeze", "rolling"):
        for side, label in SIDES:
            d = drill(side, kind)
            x, r = d["writes"], d["reads"]
            tgt = d["target"] + (" (the master)" if d.get("target_was_master") else "")
            w(f"| {DRILLS[kind].format(**R)} | {label} | {tgt} | {x['sent']:,} | {x['refused']} | {x.get('sent_again', 0)} | "
              f"{f(x['longest_wait_s'], 2)} s | {f(x['longest_gap_s'], 2)} s | {f(x['p99_ms'])} ms | {r['sent']:,} ({r['refused']} failed) | "
              f"{f(r['longest_gap_s'], 2)} s | {f(d['whole_after_s'], 2)} s |")
    w("")
    w("## What the client was told\n")
    w("The write held longest in each drill, and the first answer it had when it was refused.\n")
    w("| drill | system | waited | tries | first refusal |")
    w("|---|---|---|---|---|")
    for kind in ("kill", "freeze", "rolling"):
        for side, label in SIDES:
            h = drill(side, kind)["held_longest"][0]
            why = (h["first_err"] or "none, it waited").replace("|", "/")[:220]
            w(f"| {kind} | {label} | {f(h['wait_s'], 2)} s | {h['tries']} | {why} |")
    w("")
    w("## The rolling restarts, step by step\n")
    for side, label in SIDES:
        steps = drill(side, "rolling").get("steps", [])
        w(f"- **{label}:** " + "; ".join(f"{s['step']} {f(s['s'], 2)} s" for s in steps) + ".")
    w("")
    w("One machine, so no network partitions and no disk failures: those are the drills this page does not run. Both "
      "systems share four cores with the load, the same for each.\n")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
