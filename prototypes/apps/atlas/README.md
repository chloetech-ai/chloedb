# MCP Atlas

Every server in the Model Context Protocol registry, mapped by the credential it **declares**,
with the least-privilege binding that would contain each one. Defensive security research: the
Atlas states what each server's record says — never that a server is malicious, and with no risk
score of our own.

Slug `atlas`. Dev ports: ChloeDB `127.0.0.1:7802`, hub `PORT=8002`. Tables prefixed `atlas_`.

## What it shows

- **The map** — 65,878 points, one per server, on topic islands: k-means (k = 48, fixed seed, in
  Node at load) over the ChloeDB embedding of its title + description (potion-base-8M, built in the
  database with `embed()`); each island labelled with the words frequent in it and rare elsewhere
  (not from a single publisher), islands placed by a 2-D projection of their centroids and pushed
  apart, points nearer the centre the closer they are to the centroid. Coloured by the credential
  class each server declares; no-secret points dim. `x`, `y`, `island` are on `atlas_servers` and
  the islands in `atlas_islands` (id, label, words, size, secret, x, y, r). A time-lapse reveals servers by publication date; filter chips by
  credential class; a time slider; hover to inspect, click for the card. 2-D canvas with a uniform
  grid for picking (66k points at 60 fps).
- **Findings** — the deterministic numbers, each one a row in `atlas_numbers` computed by a SQL
  `COUNT`/`SUM` at load; click **SQL** on any card for the statement. Exact / inferred / unknown
  bindings reported **separately**. MCPShield grades for the repositories that join.
- **Credentials** — pick a class (e.g. `github`); every server that declares it lights up, with
  what one leaked token would reach.
- **Server card** — what the server declares, its generated `bindings.toml` in **Nodwyn's format**
  (`[[bind]]` rows of class, dest, positions, methods; the basis as a comment on each row; no row for
  an unknown destination, since Nodwyn denies unbound egress by default, ADR 6), whether **Nodwyn's
  own parser** accepted it, its blast radius (a `REACHABLE` walk), similar servers (`rrf`), its publisher's others.
- **Search** — hybrid `rrf(bm25(description, :q), similarity(emb, :q))` with `MAX 2 PER publisher`,
  plus credential and capability filters.

## Headline numbers (from this build; SQL at load)

| | value | note |
|---|--:|---|
| Servers | 65,878 | the registry |
| Publishers | 40,642 | |
| Declare a secret | 17,009 (25.8%) | env var or remote header marked secret |
| Secret environment variables | 13,847 | |
| Key/token/secret/password vars **not** marked secret | 1,029 | the honest gap |
| Ask for a private key / seed phrase / wallet | 563 | |
| Ask for a password | 562 | |
| Ask for a database URL | 156 | |
| Ask for an LLM provider key | 357 | |
| Five largest publishers | 8,475 (12.9%) | |
| Published since 1 Oct 2026 | 37,341 (56.7%) | |
| Remote endpoints with a declared secret header | 7,667 | |
| Near-duplicate servers (cosine > 0.95 within publisher) | 817 (1.2%) | big publishers sampled; a floor |
| Bindings generated | 22,036 | **exact 7,751 · inferred 1,206 · unknown 13,079** |
| Nodwyn `[[bind]]` rows generated | 8,706 | for 8,293 servers; 251 exact/inferred with no host name get none |
| …accepted by Nodwyn's parser | 8,706 of 8,706 | `binding.Parse` at cdc30069; every server's file |
| Repositories joining MCPShield | 629 of 65,878 | A 278 · B 105 · C 60 · D 50 · F 136 |

The four name-pattern counts (private key, password, database URL, LLM key) are computed from
environment-variable **names**: they count what a server asks for, not what it does with it.

## Data and licences (shown on the Method tab)

- **MCP registry** `servers-latest.jsonl` — 65,878 servers, the authoritative source. The registry
  **states no data licence**; it is the official MCP registry. The Atlas reproduces only declared
  metadata. (Said on screen.)
- **MCPShield security scan 2026** — grades A–F for GitHub repos, **CC-BY-4.0**. Only **629 of
  65,878** registry servers join by GitHub slug; grades shown are MCPShield's, attributed as such.
- **secrets/provider-formats.jsonl** — Nosey Parker's and gitleaks's rule sets (Apache-2.0, MIT);
  consulted only for the names of the credential classes and the providers' hosts in the inference
  map. No rule or example from them is in the Atlas.

## How the bindings are made (`basis` on every row)

- **exact** — a remote URL plus a declared secret header: host and position are both in the record.
- **inferred** — a curated map from a well-known environment-variable **name** pattern to a provider
  host (`lib/creds.mjs`), shown with the rule that produced it.
- **unknown** — a declared secret with no destination we can name → `unbound: denied by default`
  (bound to nothing). We never guess a host.

## The one model pass

`measure.mjs` labels each of the 17,009 secret-bearing servers by what its **description** says the
server can do (read / write / execute / send / pay / unclear) with `claude-haiku-5-5` through
`lib/llm.mjs` (spend-capped, batched 25/call, ~$0.15 total). Labelled everywhere as *"from the
description, by a model"*. The ChloeDB-native form is the `llm(... choices => ...)` call written in
`measure.mjs`'s header; the labels go through `lib/llm.mjs` instead, so that they count against the
shared spend cap. The map, counts and bindings use **no** model.

## Build & run (local dev)

```sh
cd prototypes
chloedb-server --db .dev/atlas.chloedb --listen 127.0.0.1:7802 --grpc off --models .dev/models --start-apps off &
export CHLOEDB_URL=http://127.0.0.1:7802 ANTHROPIC_API_KEY=...   # the key only for measure.mjs
node apps/atlas/prep.mjs          # the datasets (needs /home/user/datasets, or ATLAS_REGISTRY and ATLAS_SHIELD) -> data/
node apps/atlas/load.mjs          # creates tables, embeds, islands, Nodwyn rows + parse, numbers
node apps/atlas/measure.mjs       # the model classification + model numbers (needs the key)
PROTOS_APPS=atlas PORT=8002 node server.mjs   # http://127.0.0.1:8002/atlas/
```

`ATLAS_LIMIT=4000 node apps/atlas/load.mjs` loads a subset for a quick dev cycle, and
`ATLAS_MEASURE_LIMIT=500 node apps/atlas/measure.mjs` labels a sample.

On a database that is already loaded, two steps rerun on their own:

```sh
node apps/atlas/load.mjs --layout-only     # the map: k-means, labels, packing from the stored vectors (~65s); no rows or numbers change
node apps/atlas/prep.mjs && node apps/atlas/load.mjs --bindings-only
                                           # the Nodwyn rows, Nodwyn's parse and the nodwyn_* numbers (~3s after prep); needs Go
```

`prep.mjs` is rerun first only when `data/atlas-bindings.jsonl` predates the `scheme`/`transports`
fields (a header's declared auth scheme and the remote's transport).

## Nodwyn bindings

Each exact or inferred binding with a host name becomes a `[[bind]]` row the way Nodwyn's
Connections writes one (`internal/frontend/rpc/mcp.go`): a remote server gets class
`<server>-key` (a Nodwyn server name, `^[a-z][a-z0-9-]{0,31}$`), its URL's host, the secret header
as the position (`header:Authorization:Bearer` when the record names no scheme, Nodwyn's default;
the record's scheme, e.g. `App`, when it gives one) and methods `["POST"]` (`["GET", "POST"]` for
SSE); a contained server gets the provider's host, `env:NAME` and `["*"]`. Nodwyn's built-in
classes are used where they exist (`openai-key`, `anthropic-key`, `github-token`,
`aws-access-key`, `slack-token`), else `<provider>-key`. A URL whose subdomain is a variable
(`{tenant}.example.com`) and a provider that serves each account on a subdomain (AWS, GCP, Azure,
Supabase, Pinecone) bind `*.domain`; a host that is only a variable (`{host}`) or a database URL's
value gets no row.

`bindcheck/` checks them with Nodwyn itself: `binding/binding.go` is Nodwyn's
`internal/binding/binding.go` copied unmodified at commit cdc30069 (Apache-2.0, see
`binding/NOTICE`), with `github.com/BurntSushi/toml` v1.6.0 as Nodwyn pins it; `main.go` runs
`binding.Parse` over each server's whole file and reports, per row, whether Parse decoded it back
unchanged. `load.mjs` builds it with `go build`, stores the verdict per binding
(`atlas_bindings.nodwyn_ok`, `nodwyn_err`, `nodwyn_toml`) and the counts in `atlas_numbers`
(`nodwyn_rows`, `nodwyn_accepted`, …). Without Go the rows are stored unchecked and no "accepted"
number is written. This shows the rows are well-formed Nodwyn policy, not that a server runs
under them.

## Files

- `prep.mjs` — reads the datasets (local only), writes `data/atlas-servers.jsonl`,
  `data/atlas-bindings.jsonl`, `data/atlas-creds.json`, `data/atlas-stats.json`. The first two
  (47 MB and 7 MB) are **not committed**: `prep.mjs` rebuilds them from the registry, and the
  deployed site's tables were loaded by running `load.mjs` from a machine that had them.
- `load.mjs` — idempotent; creates `atlas_servers`, `atlas_bindings` (edge table), `atlas_creds`,
  `atlas_cocred` (edge table), `atlas_islands`, `atlas_numbers`; embeds; lays out the islands;
  writes and checks the Nodwyn rows; counts near-duplicates.
- `lib/layout.mjs` — k-means, island labels and packing for the map.
- `lib/nodwyn.mjs` — the Nodwyn `[[bind]]` rows; `bindcheck/` — Nodwyn's parser and its driver.
- `measure.mjs` — the model pass; writes `capability` and the model number rows.
- `lib/creds.mjs` — the credential taxonomy and binding rules.
- `lib/numbers.mjs` — the numbers table (every displayed figure, with its SQL).
- `app.mjs`, `public/` — the API and the page.

## What this does NOT claim

- Nothing here says a server is malicious or compromised — only what it declares.
- The near-duplicate figure samples the largest publishers, so it is a floor, not exact.
- Islands are k-means clusters of 256-d embeddings and their labels are word statistics, not a
  taxonomy: proximity is a hint, not a proof.
- "Accepted by Nodwyn's parser" means the file is well-formed Nodwyn policy; 5,204 header rows use
  Nodwyn's default Bearer scheme because the record names none.
- Grades cover only the 629 scanned repositories; the rest of the registry is unscanned.
- "Inferred" destination hosts come from a name→host map, not from observing traffic.
