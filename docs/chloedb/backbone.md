# Backbone Explorer: the internet's routing on ChloeDB

`chloedb-backbone` is an app of a ChloeDB server built on public data about how the internet's networks reach each other: every BGP update a [RIPE RIS](https://ris.ripe.net) collector hears, checked as it arrives against [RPKI](https://www.rpki-client.org) and against the origins [RouteViews](https://www.routeviews.org) saw the day before, with [PeeringDB](https://www.peeringdb.com)'s exchanges and buildings, [CAIDA](https://www.caida.org/catalog/datasets/as-relationships/)'s relationships between networks, [RIPE Atlas](https://atlas.ripe.net)'s measurements to the K root server and [IODA](https://ioda.inetintel.cc.gatech.edu)'s outages. It says what is wrong now, where, and how the problems connect. It is a client of chloedb-server over HTTP, as the crawler and the spreadsheet app are, and the command center can show it as the app `backbone` (`chloedb-server --app backbone=URL`). Every request needs the token the command center's app proxy sends, unless the app runs with `--public`: then it serves its page at `/` and the page's reads to anyone, at most `--per-minute` (600) requests a minute from one address. Readers never send SQL: each page asks its own statements, and shows them under SQL. It runs that way at https://backbone.chloetech.ai.

## The pages

| page | what it shows | from |
|---|---|---|
| Now | the open incidents, most serious first, what they share (an announcer, a carrier, a country), the busiest prefixes and what the collector heard in the last minutes | `bb_incidents`, `bb_alerts`, `bb_updates` |
| Map | the world, each country coloured by its open alerts and its probes' time to K, and lines where a network in one country announced space owned in another; a country's exchanges and networks with their probes, and where slow traceroutes leave it | `bb_alerts`, `bb_ping`, `bb_trace`, `bb_ix`, `bb_member` |
| A network | what it hangs from and what depends on it (`REACHABLE` over the relationships), trouble within three hops, its prefixes and their updates a minute, its alerts | `bb_p2c`, `bb_p2p`, `bb_updates`, `bb_roa` |
| A prefix | every update replayed into who holds a route to it now, its origins and their RPKI state, the path to it, the routes covering it and inside it, its ROAs | `bb_updates`, `bb_roa` |
| An incident | its alerts, who announced, carried and owns the space, and the updates a minute around it | `bb_alerts`, `bb_updates` |
| Diagnose | for the slowest countries and those with the most probes, the stack from users to the K root server (time to K, paths that jump, peering, routing alerts, outages) and the lowest layer that is red | `bb_ping`, `bb_trace`, `bb_alerts`, `bb_outage` |
| Rules | each alert rule's statement, how long it took and what it found | `bb_rules` |
| Data | the tables, their rows, and where each source stands | every table |

## The rules

Each rule is one statement over the tables, run every minute; its rows are the alerts, kept open while it finds them, and alerts that share an owner, an announcer, a carrier or a country are grouped into incidents.

| rule | finds |
|---|---|
| `rpki-invalid` | a route a ROA gives to another network, or longer than its ROA allows, heard by 3 or more peers (IPv4 /24 and IPv6 /48 or shorter: longer ones are mostly blackholing) |
| `new-origin` | a prefix announced by a network that is neither yesterday's origin nor its customer or provider, with no ROA to say which is right |
| `more-specific` | a prefix no one announced yesterday, inside space another network announces, from a network that is not its customer and no ROA covers |
| `flapping` | a prefix announced and withdrawn 10,000 times an hour or more |
| `withdrawn` | twenty or more of a network's prefixes withdrawn by five or more peers at once |
| `blackhole` | a prefix tagged for blackholing (`65535:666`) |
| `ioda` | a country or a region whose routes, pings or traffic IODA sees fall |
| `slow-country` | a country with five or more probes whose median time to the nearest K root server is over 100 ms |

## Running it

```bash
chloedb-backbone fetch --dir DATA          # RPKI, CAIDA, RIPE's names, PeeringDB, Atlas, IODA
chloedb-backbone load --dir DATA           # the tables, and their records
chloedb-backbone feed --dir DATA --from 6h --live   # RIS's archive from six hours back, then RIS Live
chloedb-backbone serve --public            # the page and its API at 127.0.0.1:7890
```

`feed --resume` starts the archive after the newest update kept, so a restart fills its gap, and `--hours 24` stops it after a day so a supervisor starts it again after a new `fetch` and `load`. RIS Live sends every collector's messages whatever filter it is asked for, so the feed keeps those of `--collectors` itself. On Kubernetes, `chloedb-backbone/deploy/k8s.yaml` runs the server, the feed and the app on a node of their own, and `deploy/backup.yaml` checkpoints the database each night and copies it to a volume.

## Sizes

Measured with one collector, rrc00 (about 90 peers), on 16 and 17 November 2026: about 5.5 million updates an hour, about 100 bytes an update stored, so about 13 GB a day; the updates are kept three days. The pages answered in 0.1 to 0.6 seconds on a 4-core machine. The feed holds RPKI's 1.04 million ROAs and yesterday's 1.44 million origins as prefix maps, about 4 GB with the allocator's share.
