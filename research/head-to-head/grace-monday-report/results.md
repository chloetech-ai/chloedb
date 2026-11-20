# Grace: the Monday numbers by clicking, checked against SQL

Grace, a support operations lead in the persona study, builds her Monday report from a Zendesk export in a spreadsheet: tickets each week by priority, the median time to solve, the top tags, satisfaction per agent. She said she adopts ChloeDB when those numbers come by clicking, are saved as a team view that opens on a phone, and the counts are true. This page builds each number in ChloeDB's command center by clicking and checks it against SQL.

- **The data:** 20,000 made-up tickets shaped like a Zendesk export ([tickets.py](tickets.py): priority, status, assignee, satisfaction, tags in one value with spaces between, created and solved times; about one in eight not solved yet), written to a CSV file of 2.4 MB.
- **By clicking:** [click.mjs](click.mjs) drives Chromium with Playwright, as Grace would. It loads the file on the Import page, builds each number in the Query builder, reads it off the Answer table, saves the weekly chart as a team view, and opens that view's link on a phone (iPhone 13 in Playwright). Every click, pick from a list and typed value is counted.
- **Against:** the same numbers in SQL written by hand on the same ChloeDB, and in DuckDB over the CSV file, the reference. A count must be equal. The median must be within a minute, since the builder measures whole minutes, as her sheet does. A share must be within its shown rounding of one decimal.

Measured on 2026-11-20 on Intel(R) Xeon(R) Processor @ 2.10GHz, 4 cores, 15 GB, Linux 6.18.44-fc-v726; chloedb-server 0.3.9 (built from a3283ace6c); DuckDB 2.0.0. Run it again with `python3 run.py && python3 report.py` (needs node with Playwright and Chromium, `pip install duckdb` and a chloedb-server build). Raw numbers: [raw/results.json](raw/results.json) and [raw/clicks.json](raw/clicks.json); screenshots in [raw/](raw/).

## What it says

- **The numbers are true.** 4 of the 4 numbers read off the builder agree with DuckDB (78 values in all), and so does each one in SQL by hand. The top ten tags come in the same order. The tags were split at the spaces on import, as the page offered ("spaces").
- **33 actions for the whole report,** in the browser: load the export (tags split at spaces, as the page offered) 2; tickets each week by priority, as a chart, saved as a team view 8; median hours from created to solved, each week 6; the ten most used tags 4; satisfaction per agent: the share of ratings that are good 12 (one more picks the table). None needs SQL.
- **The team view opens on a phone.** Its link, the one the page copies on saving, opened the view with its chart on the first screen, a line per priority, and nothing wider than the screen ([raw/view-phone.png](raw/view-phone.png)).
- **One thing to fix.** After saving, the address bar holds `#/builder?view=monday_numbers`, the builder with the view in it. Opened on the same phone, it shows the builder's tables and blocks first and no chart on the first screen ([raw/builder-address-phone.png](raw/builder-address-phone.png)). Someone who shares the address rather than the copied link sends that.
- **The median is in whole minutes.** The builder's median differs from DuckDB's, on exact seconds, by at most 38 s; the same median in SQL on seconds agrees with DuckDB's exactly.
- **What this page does not measure:** Grace's spreadsheet and Looker. Her round 4 diary has the same report taking a SQL guess and a borrowed example in the command center; her sheet's steps are her story, not a measurement.

## Each number

| number | actions | values | clicked against DuckDB | largest difference | SQL by hand against DuckDB | ChloeDB SQL p50 | DuckDB p50 |
|---|---|---|---|---|---|---|---|
| Tickets each week by priority | 8 | 48 | agrees | 0.0000 tickets | agrees | 3.0 ms | 4.5 ms |
| Median hours from created to solved, each week | 6 | 12 | agrees | 0.0106 hours | agrees | 21.2 ms | 5.3 ms |
| The ten most used tags | 4 | 10 | agrees (same order) | 0.0000 tickets | agrees | 1.3 ms | 4.2 ms |
| Satisfaction per agent (share of ratings that are good) | 12 | 8 | agrees | 0.0494 points | agrees | 13.1 ms | 3.6 ms |

The first number's actions include the chart and saving it as a team view. Loading the file took 2 actions (20,000 rows written to tickets). The SQL times are each statement by hand, the median of 7 runs: ChloeDB over HTTP with its answer cache off, DuckDB in the process.

## The SQL the builder wrote

Tickets each week by priority:

```sql
SELECT date_trunc('week', created_at) AS created_at_week, priority, COUNT(*) AS n
FROM tickets
GROUP BY date_trunc('week', created_at), priority
ORDER BY date_trunc('week', created_at), priority
LIMIT 1000;
```

Median hours from created to solved, each week:

```sql
SELECT date_trunc('week', created_at) AS created_at_week, MEDIAN(date_diff('minute', created_at, solved_at) / 60.0) AS median_hours_to_solved_at
FROM tickets
GROUP BY date_trunc('week', created_at)
ORDER BY date_trunc('week', created_at)
LIMIT 1000;
```

The ten most used tags:

```sql
SELECT tags, COUNT(*) AS n
FROM tickets
GROUP BY tags
ORDER BY n DESC
LIMIT 10;
```

Satisfaction per agent (share of ratings that are good):

```sql
SELECT assignee, satisfaction, ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (PARTITION BY assignee), 1) AS pct_of_assignee
FROM tickets
WHERE satisfaction IN ('good', 'bad')
GROUP BY assignee, satisfaction
ORDER BY pct_of_assignee DESC
LIMIT 20;
```

## The clicks

Each step as click.mjs does it, in the words on the screen:

1. Import and export: choose `tickets.csv`; the page reads `tags` as KEYWORD[] split at spaces; **Load the rows**.
2. Query builder: click the table `tickets`.
3. `created_at` → **Group by week**; `priority` → **Group by it**; **Chart**; **Save as a view**, name it "Monday numbers", **Save the view**.
4. **Clear**; `created_at` → **Group by week**; the measure → **MEDIAN**, of **hours from created_at to solved_at**; **Table**.
5. **Clear**; `tags` → **Group by it**; at most **10** groups.
6. **Clear**; `satisfaction` → **Filter on it**, **is one of**, **good**, **bad**; `assignee` → **Group by it**; `satisfaction` → **Group by it**; the measure → **share of rows (%)**, **within each assignee**.

The server was open, with no keys, as Sam starts it for her. A teammate's read key opening the view is in the command center's own tests, not here.
