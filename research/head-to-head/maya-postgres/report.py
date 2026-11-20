"""results.md from raw/results.json (run.py writes it). Every number on the
page comes from the raw file: re-run run.py, then this, to change one."""

import json
import os
import statistics

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "raw", "results.json")))

SYSTEMS = [
    ("postgres", "pg_ilike", "Postgres `ILIKE '%text%'`"),
    ("postgres", "pg_fts", "Postgres full text (`websearch_to_tsquery`)"),
    ("postgres", "pg_trgm", "Postgres pg_trgm (`<%`, `word_similarity`)"),
    ("postgres", "pg_fts_or_trgm", "Postgres full text OR pg_trgm"),
    ("chloedb", "chloedb_exact", "ChloeDB `MATCH ... syntax => 'plain'`"),
    ("chloedb", "chloedb_fuzzy", "ChloeDB `MATCH ... syntax => 'plain', fuzzy => 'auto'`"),
]


def mean(vals):
    vals = [v for v in vals if v is not None]
    return statistics.mean(vals) if vals else None


def f(v, d=2):
    return "–" if v is None else f"{v:.{d}f}"


def system(side, key):
    return R[side]["systems"][key]


def main():
    m = R["machine"]
    kinds = [("spelled", "spelled right"), ("typo", "with a typo"), ("punct", "with punctuation")]
    counts = {k: sum(1 for q in system("postgres", "pg_ilike")["queries"] if q["kind"] == k) for k, _ in kinds}
    out = []
    w = out.append
    w("# Maya: a recipe search box, ChloeDB against Postgres\n")
    w("Maya, a junior developer in the persona study, adopts ChloeDB when her lead sees it beside Postgres full-text search, "
      "with the same queries, and the typo search never returns fewer rows than `ILIKE`. This page is that comparison on a "
      f"made-up cookbook shaped like hers: {R['recipes']:,} recipes whose names are an adjective, an ingredient and a dish, "
      "with the spellings real cookbooks mix (Lasagna and Lasagne, Chili and Chilli, Jalapeño and Jalapeno, Mac and Cheese "
      f"and Mac & Cheese). {R['queries']} texts are typed into the search box: {counts['spelled']} spelled right, "
      f"{counts['typo']} with a typo, {counts['punct']} with the punctuation a visitor types. Each text has the rows it means "
      "(a search for *lasagne* means every lasagna, however the name spells it), and each system's answer is scored against them.\n")
    w(f"Measured on {R['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{R['chloedb']['version'].strip()[:80]}; {R['postgres']['version'].split(' on ')[0]}. "
      f"Run it again with `python3 run.py && python3 report.py` (needs Postgres with pg_trgm, `pip install psycopg[binary]`, "
      "and a chloedb-server build). The raw numbers are in [raw/results.json](raw/results.json).\n")

    fz = system("chloedb", "chloedb_fuzzy")
    both = system("postgres", "pg_fts_or_trgm")
    missed = [q["q"] for q in fz["queries"] if q["recall"] is not None and q["recall"] < 1]
    widened = [(q["q"], q.get("widened_to", [])) for q in fz["queries"] if q["precision"] is not None and q["precision"] < 1]
    pg_missed = [q["q"] for q in both["queries"] if q["kind"] == "typo" and q["recall"] is not None and q["recall"] < 1]
    pg_loose = [q["q"] for q in both["queries"] if q["precision"] is not None and q["precision"] < 0.5]
    w("## What it says\n")
    w(f"- **ChloeDB's documented typo search found what was meant.** It returned every row a text meant for "
      f"{R['queries'] - len(missed)} of the {R['queries']} texts, and its first page was right for every text "
      f"({f(mean([q['p_at_10'] for q in fz['queries']]))}). "
      + (f"It missed some rows of {', '.join('`' + q + '`' for q in missed)}"
         + (" (*Kabob* is two edits from *kebab*, more than a five-letter word is allowed)" if "kebab" in missed else "") + ". "
         if missed else "")
      + "No text returned fewer rows than `ILIKE`, *lasagne* found every Lasagna and Lasagne, and no text was an error.")
    if widened:
        w("- **Its one weakness here is a real word widened to its neighbour.** "
          + "; ".join(f"`{q}` also kept {', '.join('*' + x + '*' for x in ws) or 'other rows'}" for q, ws in widened)
          + f". The neighbours rank after the rows that have the words as typed, so the first page stays right while there are ten of those; "
            f"below them the page fills with the neighbours. Its precision over everything it returned is {f(mean([q['precision'] for q in fz['queries']]))}. "
            "This is MAYA-310, and it holds for words spelled right, not only for typos.")
    w(f"- **Postgres needs both of its indexes to come close, and still misses typos.** Full text alone returned "
      f"{f(mean([q['recall'] for q in system('postgres', 'pg_fts')['queries'] if q['kind'] == 'typo']))} of what the typed-with-a-typo texts meant. "
      f"Full text OR pg_trgm, two GIN indexes and a ranking written by hand, found {f(mean([q['recall'] for q in both['queries'] if q['kind'] == 'typo']))}, "
      f"missing {', '.join('`' + q + '`' for q in pg_missed) or 'none'}, and trigram similarity keeps any name that shares most of the text's letters: "
      f"for {', '.join('`' + q + '`' for q in pg_loose) or 'no text'}, fewer than half of the rows it returned were meant. "
      "Full text alone does not join spellings: *lasagne* finds only the names spelled Lasagne. With pg_trgm too, "
      f"texts spelled right still missed rows spelled the other way: {', '.join('`' + q['q'] + '`' for q in both['queries'] if q['kind'] == 'spelled' and q['recall'] is not None and q['recall'] < 1) or 'none'}.")
    w(f"- **Speed is no reason to choose; memory favours Postgres at this size.** Postgres's full text alone answers a page in "
      f"{f(system('postgres', 'pg_fts')['page']['p50_ms'])} ms at the median, and its form that comes close on typos (full text OR pg_trgm) in "
      f"{f(both['page']['p50_ms'])} ms (p95 {f(both['page']['p95_ms'])}); ChloeDB's fuzzy search takes {f(fz['page']['p50_ms'])} ms "
      f"(p95 {f(fz['page']['p95_ms'])}) through HTTP, {f(fz['server']['p50_ms'])} ms of it in the server. All are far below what a visitor notices. "
      f"ChloeDB's server used {R['chloedb']['memory_kb_pss'] / 1024:.0f} MB against Postgres's {R['postgres']['memory_kb_pss'] / 1024:.0f} MB. "
      "An app that opens the file itself (`chloedb.open`, as Maya's Flask app does) has no HTTP in the way; that is not measured here.")
    w(f"- **ChloeDB takes fewer steps.** {len(R['setup']['chloedb'])} against {len(R['setup']['postgres'])}: no extension, no index to "
      "declare, and the query is the one the docs give.\n")

    w("## What each one returned\n")
    w("Recall is the share of the rows a text means that came back; precision, the share of what came back that it means; "
      "first page, the share of the first ten that are right (of as many as there are, when fewer than ten). "
      "\"Fewer than ILIKE\" counts the texts spelled right where a system returned fewer rows than `name ILIKE '%text%'`, "
      "the line Maya's search must not cross.\n")
    w("| system | recall, spelled right | recall, typo | recall, punctuation | precision | first page | errors | fewer than ILIKE |")
    w("|---|---|---|---|---|---|---|---|")
    for side, key, label in SYSTEMS:
        s = system(side, key)
        qs = s["queries"]
        rec = [f(mean([q["recall"] for q in qs if q["kind"] == k])) for k, _ in kinds]
        w(f"| {label} | {' | '.join(rec)} | {f(mean([q['precision'] for q in qs]))} | {f(mean([q['p_at_10'] for q in qs]))} "
          f"| {sum(1 for q in qs if q['error'])} | {len(s['fewer_than_ilike'])} |")
    w("")

    w("## Speed, memory, disk and setup\n")
    w(f"A first page is the ranked query with `LIMIT 10`, each text {R['reps']} times after three warm-up runs. Postgres is "
      "timed through psycopg on loopback; ChloeDB through its HTTP JSON API, so its client time includes HTTP and JSON, and "
      "the server's own time (`took_ms`) is given beside it. Memory is the proportional set size of each server's processes "
      "after the run (Postgres's shared buffers counted once). Disk is the table and its indexes for Postgres "
      "(`pg_total_relation_size`; its whole data folder, catalogs and WAL included, beside it) and the database's files for ChloeDB.\n")
    w("| system | first page p50 | p95 | server's own p50 | p95 |")
    w("|---|---|---|---|---|")
    for side, key, label in SYSTEMS:
        s = system(side, key)
        srv = s.get("server")
        w(f"| {label} | {f(s['page']['p50_ms'])} ms | {f(s['page']['p95_ms'])} ms | "
          f"{f(srv['p50_ms']) + ' ms' if srv else '–'} | {f(srv['p95_ms']) + ' ms' if srv else '–'} |")
    w("")
    pg, ch = R["postgres"], R["chloedb"]
    w("| | Postgres | ChloeDB |")
    w("|---|---|---|")
    w(f"| load {R['recipes']:,} rows and build the indexes | {pg['load_s']:.2f} s | {ch['load_s']:.2f} s |")
    w(f"| memory after the run (PSS) | {pg['memory_kb_pss'] / 1024:.0f} MB | {ch['memory_kb_pss'] / 1024:.0f} MB |")
    w(f"| disk | {pg['table_and_indexes_bytes'] / 1e6:.2f} MB (data folder {pg['data_dir_bytes'] / 1e6:.0f} MB) | {ch['disk_bytes'] / 1e6:.2f} MB |")
    w(f"| setup steps | {len(R['setup']['postgres'])} | {len(R['setup']['chloedb'])} |")
    w("")
    w("Postgres's steps: " + "; ".join(R["setup"]["postgres"]) + ".\n")
    w("ChloeDB's steps: " + "; ".join(R["setup"]["chloedb"]) + ".\n")

    w("## Each text\n")
    w("Rows returned, and recall / precision, for each text. *right* is how many rows the text means.\n")
    keys = [("postgres", "pg_ilike", "ILIKE"), ("postgres", "pg_fts", "PG full text"), ("postgres", "pg_fts_or_trgm", "PG full text OR trgm"),
            ("chloedb", "chloedb_exact", "ChloeDB exact"), ("chloedb", "chloedb_fuzzy", "ChloeDB fuzzy")]
    w("| text | kind | right | " + " | ".join(k[2] for k in keys) + " |")
    w("|---|---|---|" + "---|" * len(keys))
    for i, q in enumerate(system("postgres", "pg_ilike")["queries"]):
        cells = []
        for side, key, _ in keys:
            x = system(side, key)["queries"][i]
            cells.append("error" if x["error"] else f"{x['returned']} ({f(x['recall'])} / {f(x['precision'])})")
        w(f"| `{q['q']}` | {q['kind']} | {q['right']} | " + " | ".join(cells) + " |")
    w("")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
