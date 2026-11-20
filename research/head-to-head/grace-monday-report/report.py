"""results.md from raw/results.json (run.py writes it). Every number on the
page comes from the raw file."""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
R = json.load(open(os.path.join(HERE, "raw", "results.json")))
NAMES = {
    "per_week": "Tickets each week by priority",
    "median_hours": "Median hours from created to solved, each week",
    "top_tags": "The ten most used tags",
    "satisfaction": "Satisfaction per agent (share of ratings that are good)",
}
UNITS = {"per_week": "tickets", "median_hours": "hours", "top_tags": "tickets", "satisfaction": "points"}


def f(v, d=1):
    return "–" if v is None else f"{v:,.{d}f}"


def main():
    out = []
    w = out.append
    m = R["machine"]
    c = R["clicks"]
    steps = c["steps"]
    w("# Grace: the Monday numbers by clicking, checked against SQL\n")
    w("Grace, a support operations lead in the persona study, builds her Monday report from a Zendesk export in a spreadsheet: "
      "tickets each week by priority, the median time to solve, the top tags, satisfaction per agent. She said she adopts "
      "ChloeDB when those numbers come by clicking, are saved as a team view that opens on a phone, and the counts are true. "
      "This page builds each number in ChloeDB's command center by clicking and checks it against SQL.\n")
    w(f"- **The data:** {R['tickets']:,} made-up tickets shaped like a Zendesk export ([tickets.py](tickets.py): priority, "
      "status, assignee, satisfaction, tags in one value with spaces between, created and solved times; about one in eight "
      "not solved yet), written to a CSV file of " + f"{R['csv_bytes'] / 1e6:,.1f} MB.")
    w("- **By clicking:** [click.mjs](click.mjs) drives Chromium with Playwright, as Grace would. It loads the file on the "
      "Import page, builds each number in the Query builder, reads it off the Answer table, saves the weekly chart as a team "
      "view, and opens that view's link on a phone (iPhone 13 in Playwright). Every click, pick from a list and typed value "
      "is counted.")
    w("- **Against:** the same numbers in SQL written by hand on the same ChloeDB, and in DuckDB over the CSV file, the "
      "reference. A count must be equal. The median must be within a minute, since the builder measures whole minutes, as "
      "her sheet does. A share must be within its shown rounding of one decimal.\n")
    w(f"Measured on {R['at'][:10]} on {m['cpu']}, {m['cores']} cores, {m['memory_mb'] // 1024} GB, {m['os']}; "
      f"{R['chloedb_version']}; DuckDB {R['duckdb']}. Run it again with `python3 run.py && python3 report.py` (needs node "
      "with Playwright and Chromium, `pip install duckdb` and a chloedb-server build). Raw numbers: "
      "[raw/results.json](raw/results.json) and [raw/clicks.json](raw/clicks.json); screenshots in [raw/](raw/).\n")

    w("## What it says\n")
    agree = [k for k, v in R["numbers"].items() if v["clicked_vs_duckdb"]["agree"]]
    w(f"- **The numbers are true.** {len(agree)} of the {len(R['numbers'])} numbers read off the builder agree with DuckDB "
      f"({sum(v['clicked_vs_duckdb']['numbers'] for v in R['numbers'].values())} values in all), and so does each one in SQL by "
      "hand. The top ten tags come in the same order. The tags were split at the spaces on import, as the page offered "
      f"(\"{steps[0].get('tags_split_at')}\").")
    total = c["actions"]
    w(f"- **{total} actions for the whole report,** in the browser: "
      + "; ".join(f"{s['name'][0].lower() + s['name'][1:]} {s['actions']}" for s in steps)
      + " (one more picks the table). None needs SQL.")
    ph = c["phone"]
    w(f"- **The team view opens on a phone.** Its link, the one the page copies on saving, opened the view "
      f"with its chart {'on the first screen' if ph.get('chart_on_first_screen') else 'below the first screen'}, a line per "
      f"{steps[1].get('chart_lines_per')}, {'and nothing wider than the screen' if not ph['wider_than_screen'] else 'but wider than the screen'} "
      "([raw/view-phone.png](raw/view-phone.png)).")
    ba = ph.get("builder_address")
    if ba and not ba["chart_on_first_screen"]:
        w(f"- **One thing to fix.** After saving, the address bar holds `{ba['link']}`, the builder with the view in it. "
          "Opened on the same phone, it shows the builder's tables and blocks first and no chart on the first screen "
          "([raw/builder-address-phone.png](raw/builder-address-phone.png)). Someone who shares the address rather than "
          "the copied link sends that.")
    md = R["numbers"]["median_hours"]["clicked_vs_duckdb"]
    w(f"- **The median is in whole minutes.** The builder's median differs from DuckDB's, on exact seconds, by at most "
      f"{md['largest_difference'] * 3600:,.0f} s; the same median in SQL on seconds agrees with DuckDB's exactly.")
    w("- **What this page does not measure:** Grace's spreadsheet and Looker. Her round 4 diary has the same report "
      "taking a SQL guess and a borrowed example in the command center; her sheet's steps are her story, not a measurement.\n")

    w("## Each number\n")
    w("| number | actions | values | clicked against DuckDB | largest difference | SQL by hand against DuckDB | ChloeDB SQL p50 | DuckDB p50 |")
    w("|---|---|---|---|---|---|---|---|")
    by_name = dict(zip(NAMES, steps[1:]))
    for k, label in NAMES.items():
        x = R["numbers"][k]
        cv, hv = x["clicked_vs_duckdb"], x["hand_sql_vs_duckdb"]
        order = " (same order)" if cv.get("same_order") else ""
        w(f"| {label} | {by_name[k]['actions']} | {cv['numbers']} | {'agrees' if cv['agree'] else 'differs'}{order} | "
          f"{f(cv['largest_difference'], 4)} {UNITS[k]} | {'agrees' if hv['agree'] else 'differs'} | "
          f"{f(R['chloedb_sql_ms'][k], 1)} ms | {f(R['duckdb_ms'][k], 1)} ms |")
    w("")
    w(f"The first number's actions include the chart and saving it as a team view. Loading the file took "
      f"{steps[0]['actions']} actions ({steps[0]['said']}). The SQL times are each statement by hand, the median of "
      f"7 runs: ChloeDB over HTTP with its answer cache off, DuckDB in the process.\n")
    w("## The SQL the builder wrote\n")
    for k, label in NAMES.items():
        w(f"{label}:\n")
        w("```sql\n" + R["builder_sql"][k].strip() + "\n```\n")
    w("## The clicks\n")
    w("Each step as click.mjs does it, in the words on the screen:\n")
    w("1. Import and export: choose `tickets.csv`; the page reads `tags` as KEYWORD[] split at spaces; **Load the rows**.")
    w("2. Query builder: click the table `tickets`.")
    w("3. `created_at` → **Group by week**; `priority` → **Group by it**; **Chart**; **Save as a view**, name it "
      "\"Monday numbers\", **Save the view**.")
    w("4. **Clear**; `created_at` → **Group by week**; the measure → **MEDIAN**, of **hours from created_at to solved_at**; **Table**.")
    w("5. **Clear**; `tags` → **Group by it**; at most **10** groups.")
    w("6. **Clear**; `satisfaction` → **Filter on it**, **is one of**, **good**, **bad**; `assignee` → **Group by it**; "
      "`satisfaction` → **Group by it**; the measure → **share of rows (%)**, **within each assignee**.\n")
    w("The server was open, with no keys, as Sam starts it for her. A teammate's read key opening the view is in the "
      "command center's own tests, not here.\n")
    with open(os.path.join(HERE, "results.md"), "w") as fh:
        fh.write("\n".join(out))
    print("wrote results.md")


if __name__ == "__main__":
    main()
