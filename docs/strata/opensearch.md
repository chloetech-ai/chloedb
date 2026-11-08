# The OpenSearch API

`strata-server --opensearch-listen 127.0.0.1:9200` answers OpenSearch's REST API on a port of its own: an application written for OpenSearch or Elasticsearch (Gitea, Mastodon, a client library) indexes into Strata and searches it without a change. Each index is a Strata table; queries in OpenSearch's Query DSL become SQL.

## Run it

```sh
strata-server --db search.strata --opensearch-listen 127.0.0.1:9200
curl -s localhost:9200/                      # {"version": {"number": "2.11.0", "distribution": "opensearch"}, ...}
curl -s -XPUT localhost:9200/books -H 'content-type: application/json' \
  -d '{"mappings": {"properties": {"title": {"type": "text"}, "year": {"type": "long"}}}}'
curl -s -XPOST 'localhost:9200/books/_doc/1?refresh=true' -H 'content-type: application/json' -d '{"title": "The quick brown fox", "year": 2026}'
curl -s localhost:9200/books/_search -H 'content-type: application/json' -d '{"query": {"match": {"title": "fox"}}}'
```

| flag | environment | what |
|---|---|---|
| `--opensearch-listen ADDR` | `STRATA_OPENSEARCH_LISTEN` | where the API listens (off when not given) |
| `--opensearch-version V` | `STRATA_OPENSEARCH_VERSION` | the version `GET /` reports, `2.11.0` by default; a `7.x` version (`7.10.2`) answers as Elasticsearch 7 does, which clients of Elasticsearch 7 check for (elasticsearch-ruby 7.17, which Mastodon uses, refuses OpenSearch 2.x itself) |

It shares the server's TLS (`--tls-cert`), hosts (`--allow-host`) and credentials. With a token or keys, a request carries one as `Authorization: Bearer <token>`, or as the password of Basic authentication (any user name), which is how most OpenSearch clients send one: `http://user:<token>@host:9200`. Reads need a key of scope `read`, writes of documents `write`, and changes to indexes, mappings, aliases and settings `admin`; an index made by its first document needs `admin` too. On a coordinator, or beside one (`--coordinator-listen`), the indexes are tables of the cluster, split over its shards.

## Tested with

| client | how | result |
|---|---|---|
| [opensearch-py](https://pypi.org/project/opensearch-py/) | 34 checks: indexes, `helpers.bulk`, searches, scroll, aggregations, documents, aliases, errors | 34 of 34 pass, as they do on OpenSearch 2.11 |
| [Gitea](https://gitea.com) 1.24.6 | its issue indexer and code indexer set to `elasticsearch` at this port, next to a second Gitea on OpenSearch 2.11, the same repository and issues in both | 42 of 42 searches find the same issues and files |
| [Mastodon](https://joinmastodon.org) 4.3 | its four indexes (accounts, tags, statuses, public statuses: analyzers, mappings) and the searches its services make (account autocomplete and full search with follower boosts, hashtag search, status search over two indexes), replayed on 1,500 accounts, 1,200 tags and 4,000 statuses next to OpenSearch 2.11 | 112 of 114 searches return the same hits in the same order; the other two differ in the order of two hits tied at score 0 |
| elasticsearch-ruby 7.17 (Mastodon's client, through Chewy) | its product check, index creation, `_bulk`, a search with `script_score` | works with `--opensearch-version 7.10.2` |
| OpenSearch 2.11 itself | 100 requests sent to both, answers compared field by field | 96 identical; the others are listed under [Differences](#differences) |
| OpenSearch 2.11 on [TREC-COVID](#numbers) | 3,000 searches of ten kinds over 171,332 abstracts | every one finds as many hits; see [Numbers](#numbers) for the first pages |

Gitea's `app.ini`:

```ini
[indexer]
ISSUE_INDEXER_TYPE = elasticsearch
ISSUE_INDEXER_CONN_STR = http://127.0.0.1:9200
REPO_INDEXER_ENABLED = true
REPO_INDEXER_TYPE = elasticsearch
REPO_INDEXER_CONN_STR = http://127.0.0.1:9200
```

Mastodon's `.env.production`: `ES_ENABLED=true`, `ES_HOST=localhost`, `ES_PORT=9200`, `ES_PRESET=single_node_cluster`, and the server started with `--opensearch-version 7.10.2`; then `tootctl search deploy`.

## What it answers

**APIs**: `GET /`, `_cluster/health`, `_cluster/settings`, `_cluster/state`, `_nodes`, `_cat/indices|aliases|health|count` (text with `v`, `h`, `s`, or `format=json`), index `PUT`/`GET`/`HEAD`/`DELETE` (names, patterns with `*`, `_all`, comma lists, aliases), `_mapping` and `_settings` (read and add), `_aliases`/`_alias`, `_stats`, `_refresh`, `_flush`, `_forcemerge` (accepted; Strata needs none of them), `_doc`, `_create`, `_update` (`doc`, `upsert`, `doc_as_upsert`, `detect_noop`), `_source`, `_mget`, `_bulk` (`index`, `create`, `update`, `delete`), `_search`, `_msearch`, `_count`, `_search/scroll`, `_delete_by_query`, `_update_by_query` (without a script), `if_seq_no`, `version_type=external`, `refresh`, `filter_path`, `pretty`, `_source` filtering, and gzip request bodies.

**Queries**: `bool` (`must`, `filter`, `should` with `minimum_should_match`, `must_not`), `match` (`operator`, `minimum_should_match`, `fuzziness`, `zero_terms_query`), `match_phrase`, `match_phrase_prefix`, `match_bool_prefix`, `multi_match` (`best_fields`, `most_fields`, `cross_fields`, `phrase`, `phrase_prefix`, `bool_prefix`, `field^boost`, `field.*`), `query_string` and `simple_query_string` (Lucene's syntax: fields, `AND`/`OR`/`NOT`, `+`/`-`, phrases, `*` and `~`), `term`, `terms`, `ids`, `range` (with date math: `now-7d/d`), `exists`, `prefix`, `wildcard` (a trailing `*`), `fuzzy`, `constant_score`, `dis_max`, `function_score` (`field_value_factor`, `gauss`/`exp`/`linear` on numbers and dates, `script_score`, `weight`, every `score_mode` and `boost_mode`), `script_score`, `nested` (as paths of objects), `knn` (with `filter`), `match_all`, `match_none`, and `term`/`terms` on `_index`.

**Scripts**: `script_score` runs the part of Painless that scoring scripts are written in: numbers, `params.x`, `doc['field'].value` of a number or a date, `_score`, `+ - * /`, `Math.log10`, `log`, `log1p`, `exp`, `sqrt`, `pow`, `abs`, `max`, `min`, and Painless's `saturation`, `sigmoid` and `decayNumericGauss|Exp|Linear`, with an optional `return` and `;`. Anything else is refused with a `script_exception` that names what it met.

**Searches**: `from`/`size` (to 10,000, or the index's `max_result_window`), `sort` (fields, `_score`, `_doc`, `missing`), `search_after`, `track_total_hits`, `post_filter`, `highlight` (`pre_tags`, `post_tags`, `fragment_size`, `number_of_fragments`, `encoder`), `fields`/`docvalue_fields`, `version`, `seq_no_primary_term`, `collapse` (the first hit of each value), scrolls (past 10,000 hits when in the documents' order), and searches of several indexes at once (`a,b/_search`, a pattern, an alias of several), which merge their hits in the search's order.

**Aggregations**: `terms` (`size`, `order`, `min_doc_count`, `missing`, `include`/`exclude` lists), `composite` (`terms` and `date_histogram` sources, `after`), `histogram`, `date_histogram` (`calendar_interval`, `fixed_interval`, `min_doc_count`, `extended_bounds`; in UTC), `range`, `date_range`, `filter`, `filters`, `global`, `missing`, `avg`, `sum`, `min`, `max`, `value_count`, `cardinality` (exact), `stats`, `extended_stats`, `percentiles`, `top_hits`, nested in each other.

## How an index is kept

An index `books` is the table `os_books`: its key `_id`, `_version`, `_hash` (a hash of the id, the order scrolls page through), and `_source`, a JSON field holding the document, whose index answers term, range, prefix and exists queries and the aggregations on every keyword, number and boolean, mapped or not. Fields that need more have columns of their own: `text` a TEXT column, `date` a TIMESTAMP, `knn_vector` a VECTOR, and `long`/`double` an INT or FLOAT holding the field's smallest value, which scores read. A text field without a keyword sub-field is kept in its column alone, and put back into `_source` when read. The mappings, settings and aliases are rows of the table `opensearch_indices`. Dynamic mapping adds fields as OpenSearch does (`text` with a `keyword` sub-field, `long`, `float`, `boolean`, `date`, objects), adding their columns with `ALTER TABLE`; `dynamic: false` and `strict` are kept.

Text is read by one of Strata's two analyzers, chosen from the field's: `english` for OpenSearch's `english` analyzer or a custom one with a stemmer, `simple` (lowercased, folded, nothing dropped) for the rest. Before Strata reads it, the text is cut into words as the field's analyzer would:

- the `standard` tokenizer keeps `merge_segments`, `mastodon.social`, `children's`, `3.14`, a word with a soft hyphen in it and a word with combining marks, viramas or tone marks (`हिन्दी`, `ผู้ใหญ่`) whole, makes each emoji (`★`, `👩‍💻`, a flag), each ideograph (Chinese, Japanese) and each hiragana a word, and a run of katakana one, and cuts at `²`, `¼`, `①`, `☆` and `✓`. Each character from U+00A0 to U+1FBFF was compared with OpenSearch 2.11's `_analyze`: they differ only for a combining mark that follows a space;
- accents are kept (`sévère` is not `severe`), unless the analyzer has an `asciifolding` filter;
- the `english` analyzer, and a `stemmer` filter of `possessive_english`, drop a word's `'s`;
- `whitespace` keeps every word between spaces whole, and `keyword` the whole text;
- `word_delimiter_graph` cuts where case or digits change (`CatsOfMastodon2026` is `cats of mastodon 2026`);
- an `edge_ngram` or `ngram` tokenizer or token filter makes the field's column hold the n-grams themselves, each a word, so that a query's words match them and BM25 counts them as Lucene does (on Mastodon's account search, scores agree with OpenSearch's to five digits);
- a field's `search_analyzer` cuts the query's text.

## Differences

What a request gets that OpenSearch would answer otherwise:

- **Analysis**: only what the list above names. Stop words other than English's, synonyms, other languages' stemmers, `pattern` tokenizers (taken as Strata's words), `path_hierarchy`, `shingle`, phonetic filters and normalizers are not applied. Chinese and Japanese are cut a word per ideograph, as the `standard` tokenizer cuts them, not into the `cjk` analyzer's pairs or a dictionary's words.
- **Ranking**: BM25 with OpenSearch's defaults (k1 1.2, b 0.75) over the same words and the same one-byte lengths, so scores agree to about five digits when the words do, and two documents of nearly equal scores can swap places. Hits of equal scores come in the order of their `_id` (OpenSearch's in the order they were written), which Strata keeps the same on one server and over shards. A phrase (`match_phrase`, or one in `query_string`) scores the sum of its words' BM25, where OpenSearch scores how often the phrase itself occurs: phrase searches find the same documents and rank them otherwise. A ranked query lists the rows none of its words scores after the others, with score 0.
- **Queries**: `wildcard` takes a trailing `*` alone; `match_phrase`'s `slop` is ignored; `nested` queries read the paths of objects, without matching inside one object; `regexp`, `terms` lookups, `geo_*`, `more_like_this`, `percolate`, `span_*`, `intervals`, `rank_feature` and `random_score` are refused, as are per-function `filter`s and `min_score`; a range's `time_zone` is not applied (dates are UTC).
- **Searches**: aggregations read one index at a time; `collapse` takes no `inner_hits`; point-in-time searches are refused; `track_scores` is ignored (a field sort scores nothing); a scroll ordered by score or a field is limited to 10,000 hits.
- **Aggregations**: `composite` returns at most 9,999 groups a page; `cardinality` is exact; a `histogram` of a number field that some documents hold an array of is refused; scripts are refused.
- **Documents**: a keyword holds at most 256 bytes as a term; `_source` holds at most 1,000 values, objects 16 deep and paths of 256 bytes (a document past them is refused, naming the limit); a date field holds one date; versions are read before the write, so two writers of one document at once can both pass an `if_seq_no` check.
- **Cluster**: one node and one shard are reported; settings other than `max_result_window` and the analysis are kept and ignored. A write is searchable once it returns, so `refresh` is accepted and needs no wait.

## Numbers

OpenSearch 2.11 (its default 1 GB heap) and Strata's OpenSearch API on the same machine, one at a time, the other stopped: a cloud container of 4 vCPUs (Intel Xeon @ 2.10GHz) and 15.7 GB, 2026-11-08. [BEIR's TREC-COVID](https://huggingface.co/datasets/BeIR/trec-covid) (171,332 abstracts) in one index, written with `_bulk`, then ten kinds of search 300 times each from one client, the same requests to both; OpenSearch's request cache off. The script, its output and how it was run: [`strata-server/results/opensearch`](https://github.com/chloetech-ai/strata/tree/main/strata-server/results/opensearch).

| search | OpenSearch p50 / p99 ms | Strata p50 / p99 ms |
|---|---|---|
| `match`, one word | 2.96 / 8.06 | 1.70 / 3.06 |
| `match`, a topic's words | 6.41 / 18.99 | 2.22 / 4.79 |
| `match` + `term` + `range` filters | 9.47 / 31.87 | 3.69 / 7.57 |
| `multi_match` | 6.89 / 30.16 | 4.10 / 9.75 |
| `match_phrase` | 2.97 / 5.84 | 1.86 / 4.86 |
| `term` filter | 2.00 / 7.00 | 1.49 / 2.23 |
| `range`, sorted by date | 2.96 / 6.46 | 1.52 / 2.44 |
| `terms` aggregation | 6.82 / 18.93 | 4.30 / 5.56 |
| `date_histogram` of every document | 9.73 / 15.50 | 6.71 / 8.80 |
| `match` + `highlight` | 7.69 / 24.84 | 3.27 / 5.66 |

| | OpenSearch | Strata |
|---|---|---|
| searches a second, 8 clients | 638 | 1,183 |
| `_bulk` of the 171,332 documents | 21.4 s | 39.0 s |
| resident memory, empty | 1,348 MiB | 50 MiB |
| resident memory, after the searches | 1,652 MiB | 726 MiB |
| on disk | 415 MB (195 MB after a `_flush`) | 260 MB |

All 3,000 searches find as many hits on both. Of the 2,400 that return hits, 808 return the same first page in the same order, and 1,292 the same scores or dates in the same places, with ties in another order (Strata's by `_id`); the 300 phrase searches find the same documents and rank them otherwise ([Differences](#differences)).
