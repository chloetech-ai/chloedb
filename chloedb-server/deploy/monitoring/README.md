# Monitoring a ChloeDB cluster with Prometheus and Grafana

| file | what |
|---|---|
| `prometheus.yml` | scrapes a coordinator (job `chloedb-coordinator`) and its copies (job `chloedb-copies`) with a key in a file; the addresses are those of [the cluster guide](../../../docs/guide/operate-cluster.md) |
| `chloedb-rules.yml` | an alerting rule for each built-in alert of the servers (`GET /v1/alerts`), with the same conditions and defaults, its runbook as `runbook_url` and the built-in rule's id as the label `chloedb_rule`; one for a target that cannot be scraped (a coordinator that is down sees nothing); disk rules over node_exporter |
| `grafana-dashboard.json` | a dashboard of the cluster, its queries, its writes and its copies; import it (Dashboards, New, Import) and pick your Prometheus |

```sh
cp prometheus.yml chloedb-rules.yml /etc/prometheus/
printf '%s\n' "$READ_KEY" > /etc/prometheus/chloedb.key   # a key of scope read
promtool check config /etc/prometheus/prometheus.yml
```

The servers keep paging on their own (webhook, PagerDuty, Opsgenie) whether or not Prometheus runs: use one of the two to page, or route them to different people, or the same alert pages twice. The thresholds are the servers' defaults (`--slo-error-ratio 0.01`, `--slo-p99-ms 250`): change both places together. `chloedb-server`'s tests check that every built-in alert has a rule here with the runbook the server links, so a new built-in alert needs its rule in `chloedb-rules.yml`.
