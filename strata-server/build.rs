// Compiles proto/strata.proto, and the OpenTelemetry logs protocol's files
// in otlp/ (the collector's LogsService, `crate::otlp`), with protox, so
// no protoc is needed.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/strata.proto");
    println!("cargo:rerun-if-changed=otlp");
    let fds = protox::compile(["proto/strata.proto"], ["proto"])?;
    tonic_build::configure().compile_fds(fds)?;
    let fds = protox::compile(["opentelemetry/proto/collector/logs/v1/logs_service.proto"], ["otlp"])?;
    tonic_build::configure().compile_fds(fds)?;
    Ok(())
}
