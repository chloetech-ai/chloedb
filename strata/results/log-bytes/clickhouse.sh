#!/bin/sh
# Bytes a line takes in ClickHouse, in the table the OpenTelemetry
# Collector's ClickHouse exporter creates for logs (its columns for the
# fields parse.py keeps, its codecs, its token Bloom filter on Body and
# its ORDER BY), after OPTIMIZE ... FINAL; and in the same table without
# the body's Bloom filter. Runs in a container started as
#   docker run -d --name ch -v $DATA:/var/lib/clickhouse -v $JSONL:/data \
#     -v $CONF/user_files.xml:/etc/clickhouse-server/config.d/user_files.xml:ro \
#     -e CLICKHOUSE_SKIP_USER_SETUP=1 clickhouse/clickhouse-server:25.8
# where user_files.xml says <clickhouse><user_files_path>/data/</user_files_path></clickhouse>.
# clickhouse.sh NAME  (reads /data/NAME.jsonl; prints one JSON line)
set -e
n=$1
q() { docker exec ch clickhouse-client --max_partitions_per_insert_block=100000 -q "$1"; }
for t in bf nobf; do
  idx=""
  [ $t = bf ] && idx=", INDEX idx_body Body TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 8"
  q "DROP TABLE IF EXISTS ${n}_$t SYNC"
  q "CREATE TABLE ${n}_$t (
      Timestamp DateTime64(9) CODEC(Delta(8), ZSTD(1)),
      TimestampTime DateTime DEFAULT toDateTime(Timestamp),
      SeverityText LowCardinality(String) CODEC(ZSTD(1)),
      SeverityNumber UInt8,
      ServiceName LowCardinality(String) CODEC(ZSTD(1)),
      Body String CODEC(ZSTD(1)),
      ResourceAttributes Map(LowCardinality(String), String) CODEC(ZSTD(1)),
      LogAttributes Map(LowCardinality(String), String) CODEC(ZSTD(1)),
      INDEX idx_res_attr_key mapKeys(ResourceAttributes) TYPE bloom_filter(0.01) GRANULARITY 1,
      INDEX idx_res_attr_value mapValues(ResourceAttributes) TYPE bloom_filter(0.01) GRANULARITY 1,
      INDEX idx_log_attr_key mapKeys(LogAttributes) TYPE bloom_filter(0.01) GRANULARITY 1,
      INDEX idx_log_attr_value mapValues(LogAttributes) TYPE bloom_filter(0.01) GRANULARITY 1
      $idx
    ) ENGINE = MergeTree PARTITION BY toDate(TimestampTime)
    ORDER BY (ServiceName, TimestampTime, Timestamp) SETTINGS index_granularity = 8192, ttl_only_drop_parts = 1"
  q "INSERT INTO ${n}_$t (Timestamp, SeverityText, ServiceName, Body, ResourceAttributes, LogAttributes)
     SELECT fromUnixTimestamp64Micro(ts), severity, service, body, map('host.name', host), if(pid IS NULL, map(), map('process.pid', toString(pid)))
     FROM file('$n.jsonl', JSONEachRow, 'ts Int64, host String, service String, severity String, pid Nullable(Int64), body String')"
  q "OPTIMIZE TABLE ${n}_$t FINAL"
done
q "SELECT '{\"input\":\"$n\",\"lines\":' || toString(any(rows)) || ',' ||
   '\"clickhouse_otel_bytes_per_line\":' || toString(round(sumIf(b, t = '${n}_bf') / any(rows), 1)) || ',' ||
   '\"clickhouse_otel_no_body_bloom_bytes_per_line\":' || toString(round(sumIf(b, t = '${n}_nobf') / any(rows), 1)) || ',' ||
   '\"clickhouse_parts\":' || toString(sumIf(p, t = '${n}_bf')) || '}'
   FROM (SELECT table AS t, sum(bytes_on_disk) AS b, sum(rows) AS rows, count() AS p FROM system.parts WHERE active AND table IN ('${n}_bf', '${n}_nobf') GROUP BY table)"
