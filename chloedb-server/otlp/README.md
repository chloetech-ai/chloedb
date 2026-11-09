The OpenTelemetry protocol's files for logs, as
[opentelemetry-proto](https://github.com/open-telemetry/opentelemetry-proto)
v1.5.0 has them (Apache License 2.0, as each file says), unchanged:
`build.rs` compiles them for `src/otlp.rs`, which takes logs from
OpenTelemetry's SDKs and Collector.
