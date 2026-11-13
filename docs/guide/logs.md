# Search your logs

ChloeDB keeps log lines in an ordinary table and finds them the way an on-call engineer looks at 3 a.m.: any part of a line (an interface name, an IP, an error code) from a substring index, the last 15 minutes with `now() - INTERVAL '15 minutes'`, counts per host and minute with `time_bucket`. This page starts a server, makes a table for syslog, a key that may only write and read those tables, sends lines to it, and runs the first five queries of an incident. The command center's **Log explorer** does the same with a search box.

<!-- run: needs chloedb-server -->

You need `chloedb-server` on your `PATH` ([install](../quickstart.md)), and `curl` and `python3`.

## A server

```sh
chloedb-server --db ./logs.chloedb --listen 127.0.0.1:7700 --grpc off > server.log 2>&1 &
until curl -sf http://127.0.0.1:7700/v1/health > /dev/null; do sleep 0.2; done
```

A server with no token and no key is open to anything on this machine. Its first key, an admin key, closes it; keep that one for yourself:

```sh
export ADMIN_KEY=$(curl -s -X POST localhost:7700/v1/admin/keys -H 'content-type: application/json' \
  -d '{"name": "admin", "scope": "admin"}' | python3 -c 'import json, sys; print(json.load(sys.stdin)["secret"])')
```

## A table for log lines

A log table is any table with a `TIMESTAMP` and a text column. This one names its columns as OpenTelemetry does (`ts`, `severity`, `host`, `service`, `body`, `attributes`), so that the OpenTelemetry Collector and a JSON lines import both fill it. `substring = true` gives `host` and `body` a [substring index](../chloedb/query-language.md#substrings-like-and-ilike-anywhere-in-a-value), which answers `LIKE` and `ILIKE` with `%` anywhere (`'%GigabitEthernet0/1%'`, `'%10.0.12.2%'`) from the index rather than by reading every line. `retain` drops lines older than 30 days. A log line has no key, so the table has none.

```sql file=logs_syslog.sql
CREATE TABLE logs_syslog (
  ts TIMESTAMP NOT NULL,
  severity KEYWORD,
  host KEYWORD OPTIONS(substring = true),
  service KEYWORD,
  body TEXT OPTIONS(substring = true),
  attributes JSON
) OPTIONS(retain = '30 days', compression = 'zstd')
```

Making a table takes an admin key:

```sh
python3 -c 'import json; print(json.dumps({"sql": open("logs_syslog.sql").read()}))' \
| curl -s localhost:7700/v1/sql -H "authorization: Bearer $ADMIN_KEY" -H 'content-type: application/json' -d @- \
| python3 -c 'import json, sys; print(json.load(sys.stdin).get("error", "made"))'
```

```text output
made
```

## A key for the on-call engineer and the log shipper

The `oncall` preset makes a key [limited to tables](../chloedb/server.md#security): it writes and reads the tables its prefix names (rows, imports and OpenTelemetry logs alike), writes notes into its [ChloeCtx](../chloectx/app.md) spaces `ctx_logs_*`, and reads the fleet's health, events, alerts and metrics. It cannot read other tables or change the server.

```sh
export CHLOEDB_LOGS_KEY=$(curl -s -X POST localhost:7700/v1/admin/keys -H "authorization: Bearer $ADMIN_KEY" \
  -H 'content-type: application/json' -d '{"name": "netops", "preset": "oncall", "prefix": "logs_*"}' \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["secret"])')
curl -s localhost:7700/v1/whoami -H "authorization: Bearer $CHLOEDB_LOGS_KEY" \
  | python3 -c 'import json, sys; w = json.load(sys.stdin); print(w["name"], w.get("preset"), w["tables"])'
```

```text output
netops oncall {'ctx_incidents': 'read', 'ctx_logs_*': 'write', 'logs_*': 'write'}
```

## Send the lines

### From an OpenTelemetry Collector

`POST /v1/logs` takes OTLP over HTTP (protobuf or JSON, gzip or not), the protocol every OpenTelemetry SDK and the Collector export; `?table=` names the table, and a table that does not exist is made with [OTLP's columns](../chloedb/server.md#opentelemetry-logs). This Collector listens for syslog on UDP port 5514 and for OTLP from your applications, and sends both to `logs_syslog` with the key above in its environment (`otelcol-contrib --config collector.yaml`):

```yaml norun
receivers:
  syslog:
    udp:
      listen_address: 0.0.0.0:5514
    protocol: rfc3164
  otlp:
    protocols:
      grpc:
      http:

processors:
  batch:

exporters:
  otlphttp/chloedb:
    logs_endpoint: http://127.0.0.1:7700/v1/logs?table=logs_syslog
    encoding: json
    headers:
      Authorization: Bearer ${env:CHLOEDB_LOGS_KEY}

service:
  pipelines:
    logs:
      receivers: [syslog, otlp]
      processors: [batch]
      exporters: [otlphttp/chloedb]
```

A Collector retries an export that failed, and a row has no key to tell a retry from a new line, so a retried export can leave a line twice.

What the Collector sends is OTLP JSON, which `curl` can send too:

```sh
now=$(python3 -c 'import time; print(time.time_ns())')
curl -s "localhost:7700/v1/logs?table=logs_syslog" -H "authorization: Bearer $CHLOEDB_LOGS_KEY" \
  -H 'content-type: application/json' -d '{"resourceLogs": [{
    "resource": {"attributes": [{"key": "host.name", "value": {"stringValue": "edge-sw-11"}}]},
    "scopeLogs": [{"logRecords": [{"timeUnixNano": "'$now'", "severityText": "WARN",
      "body": {"stringValue": "%STORM_CONTROL-3-FILTERED: A packet storm was detected on Gi1/0/7"}}]}]}]}'
echo
```

```text output
{}
```

An empty answer is OTLP's success: no record was refused (one refused is counted in `partialSuccess`).

### From a file: JSON lines

`POST /v1/import?format=jsonl` takes one JSON object a line, with the table's columns. This script writes an hour of lines from five routers and switches, with an incident in its last ten minutes: `GigabitEthernet0/1` on `core-rtr-02` going down and up, its BGP session to 10.0.12.2 dropping each time, and OSPF on `core-rtr-03` losing the same neighbour.

```python file=make_logs.py
import json
from datetime import datetime, timedelta, timezone

now = datetime.now(timezone.utc)
at = lambda s: (now - timedelta(seconds=s)).isoformat().replace("+00:00", "Z")
hosts = ["core-rtr-01", "core-rtr-02", "core-rtr-03", "edge-sw-11", "fw-01"]
normal = [
    ("info", "%SYS-5-CONFIG_I", "Configured from console by admin on vty0 (192.168.{a}.{b})"),
    ("info", "%SEC-6-IPACCESSLOGP", "list OUTSIDE permitted tcp 192.168.{a}.{b}(5{a}{b}) -> 10.0.0.{a}(443), 1 packet"),
    ("debug", "%OSPF-7-HELLO", "hello from 192.168.{a}.{b} on GigabitEthernet0/{a}"),
    ("warn", "%SYS-4-CPUHOG", "Task is running for ({b}00)msecs, more than (2000)msecs"),
    ("info", "%DHCPD-6-ADDRESS_ASSIGN", "Interface Vlan1{a} assigned DHCP address 192.168.{a}.{b}, mask 255.255.255.0"),
]
lines = []
for i in range(600):  # one line every 6 seconds for an hour
    sev, mn, text = normal[i % 5]
    host = hosts[i % 4] if i % 50 != 25 else "fw-01"
    if i % 50 == 25:
        sev, mn, text = "error", "%FW-3-DENY", "Deny tcp src outside:203.0.113.{b}/4{a}{b} dst inside:10.0.0.{a}/22"
    lines.append({"ts": at(i * 6 + 3), "severity": sev, "host": host, "service": "syslog",
                  "body": f"{mn}: " + text.format(a=i % 7, b=10 + i % 89), "attributes": {"mnemonic": mn}})
for k in range(12):  # the flap: down and up every 50 seconds, the last ten minutes
    down = 590 - k * 50
    lines.append({"ts": at(down), "severity": "error", "host": "core-rtr-02", "service": "syslog",
                  "body": "%LINK-3-UPDOWN: Interface GigabitEthernet0/1, changed state to down",
                  "attributes": {"mnemonic": "%LINK-3-UPDOWN", "iface": "Gi0/1"}})
    lines.append({"ts": at(down - 2), "severity": "error", "host": "core-rtr-02", "service": "syslog",
                  "body": "%BGP-3-NOTIFICATION: sent to neighbor 10.0.12.2 4/0 (hold time expired) 0 bytes",
                  "attributes": {"mnemonic": "%BGP-3-NOTIFICATION", "peer": "10.0.12.2"}})
    lines.append({"ts": at(down - 3), "severity": "warn", "host": "core-rtr-03", "service": "syslog",
                  "body": "%OSPF-5-ADJCHG: Process 1, Nbr 10.0.12.2 on GigabitEthernet0/2 from FULL to DOWN, Neighbor Down: Dead timer expired",
                  "attributes": {"mnemonic": "%OSPF-5-ADJCHG", "iface": "Gi0/2"}})
    lines.append({"ts": at(down - 25), "severity": "notice", "host": "core-rtr-02", "service": "syslog",
                  "body": "%LINK-3-UPDOWN: Interface GigabitEthernet0/1, changed state to up",
                  "attributes": {"mnemonic": "%LINK-3-UPDOWN", "iface": "Gi0/1"}})
with open("syslog.jsonl", "w") as f:
    f.writelines(json.dumps(x) + "\n" for x in lines)
print(len(lines), "lines")
```

```sh
python3 make_logs.py
curl -s "localhost:7700/v1/import?table=logs_syslog&format=jsonl" -H "authorization: Bearer $CHLOEDB_LOGS_KEY" \
  -H 'content-type: application/x-ndjson' --data-binary @syslog.jsonl
echo
```

```text output
648 lines
{"receipt":{"collapsed":0,"deleted":0,"inserted":648,...
```

The same call imports lines from a script or a cron job: `printf '%s\n' '{"ts": "...", "host": "...", "body": "..."}' | curl ... --data-binary @-`.

## The first five queries of an incident

Every query here runs with the on-call key, through `POST /v1/sql`. This shell function sends one and prints its rows:

```sh
sql() {
  python3 -c 'import json, sys; print(json.dumps({"sql": sys.argv[1]}))' "$1" \
  | curl -s localhost:7700/v1/sql -H "authorization: Bearer $CHLOEDB_LOGS_KEY" -H 'content-type: application/json' -d @- \
  | python3 -c '
import json, sys
j = json.load(sys.stdin)
if "error" in j: sys.exit(j["error"])
print(" | ".join(j["columns"]))
for r in j["rows"]: print(" | ".join("" if v is None else str(v) for v in r))'
}
```

**1. What failed in the last 15 minutes, and where.** `now() - INTERVAL '15 minutes'` is one time for the whole statement, so the index on `ts` answers the range:

```sh
sql "SELECT host, count(*) AS errors FROM logs_syslog
     WHERE severity = 'error' AND ts > now() - INTERVAL '15 minutes'
     GROUP BY host ORDER BY errors DESC"
```

```text output
host | errors
core-rtr-02 | 24
fw-01 | 3
```

**2. Per host per minute.** `time_bucket('1 minute', ts)` is the start of each line's minute (`'5 minutes'`, `'1 hour'` for wider buckets), grouped by the executor from the `ts` column:

```sh
sql "SELECT time_bucket('1 minute', ts) AS minute, host, count(*) AS lines FROM logs_syslog
     WHERE ts > now() - INTERVAL '15 minutes' AND severity IN ('error', 'warn')
     GROUP BY minute, host ORDER BY minute DESC LIMIT 6"
```

```text output
minute | host | lines
...
```

**3. An interface flapping.** `LIKE` with `%` on both sides is answered from the substring index of `body`, whatever the pattern. A link that changed state more than 3 times in 5 minutes is flapping:

```sh
sql "SELECT host, count(*) AS changes FROM logs_syslog
     WHERE body LIKE '%LINK-3-UPDOWN%' AND body LIKE '%GigabitEthernet0/1,%'
       AND ts > now() - INTERVAL '5 minutes'
     GROUP BY host HAVING count(*) > 3"
```

```text output
host | changes
core-rtr-02 | 12
```

**4. Everything about one IP**, on every host. An IP is not a word the text analyzer keeps whole, so it is found as a substring:

```sh
sql "SELECT host, attributes.mnemonic AS kind, count(*) AS lines FROM logs_syslog
     WHERE body LIKE '%10.0.12.2 %' AND ts > now() - INTERVAL '1 hour'
     GROUP BY host, kind ORDER BY lines DESC"
```

```text output
host | kind | lines
core-rtr-02 | %BGP-3-NOTIFICATION | 12
core-rtr-03 | %OSPF-5-ADJCHG | 12
```

**5. The top kinds of line.** Syslog names the kind of each line (`%LINK-3-UPDOWN`); kept as a path of the JSON column, it is a group like a column:

```sh
sql "SELECT attributes.mnemonic AS kind, count(*) AS lines FROM logs_syslog
     WHERE ts > now() - INTERVAL '2 hours'
     GROUP BY kind ORDER BY lines DESC LIMIT 5"
```

```text output
kind | lines
%DHCPD-6-ADDRESS_ASSIGN | 120
%OSPF-7-HELLO | 120
%SEC-6-IPACCESSLOGP | 120
%SYS-4-CPUHOG | 120
%SYS-5-CONFIG_I | 108
```

A line without such a name is grouped by its text with the numbers, addresses, ids and times taken out, which the Log explorer's **Patterns** does in the page (SQL has no regular expressions to do it).

The key may not read a table outside `logs_*`:

```sh
sql "SELECT count(*) FROM audit" || true
```

```text output
a query touches table audit, which the key "netops" may not: it is limited to logs_* (write), ctx_logs_* (write), ctx_incidents (read)
```

## The Log explorer

The command center (the server's address in a browser, here http://127.0.0.1:7700/#/explorer) has a **Log explorer** under Operate. Sign in with the on-call key and it lists the tables the key reads that hold log lines (a `TIMESTAMP` and a text column), and shows one of them:

- **a search box**: words and `"a phrase"` find the line's text (`ILIKE '%...%'` on a column with a substring index, `MATCH` on one without, or pick Substring or Words), `host:core-rtr-02` a column's value (`*` for any text), `attributes.iface:Gi0/1` a JSON path, `code>=4` a comparison, `level>=warn` the levels at or above warn as the table spells them, `-debug` leaves lines out, and `WHERE ...` takes a SQL condition as written;
- **the time range**: the last 15 minutes, hour, day or week (`ts > now() - INTERVAL '15 minutes'`), or a window from and to;
- **lines over time**: a count per minute (per 5 or 15 minutes, or 2 hours, over longer ranges) with errors and warnings apart, `time_bucket` grouped by the server; a click on a bar zooms into it;
- **the lines**, newest first, followed live every 3 seconds (Pause stops it), and for a line clicked, the lines before and after it on the same host;
- **patterns**: the newest 2,000 matching lines grouped by their text with numbers, addresses, ids and times as placeholders, most frequent first; a click shows a pattern's lines;
- **the SQL it ran**, each statement to copy or open in the SQL workbench.

Without a log table, it shows how to send logs here: the table above, the Collector's settings with this server's address, the key, and the JSON lines import, with buttons that make the table and the key for an admin.
