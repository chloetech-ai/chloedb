# strata-server, its benchmark and engine-compare in a small Linux image,
# with strata-crawl, the crawler the operator runs beside a cluster
# (spec.crawler).
#
#   docker build -f strata-server/deploy/Dockerfile -t strata-server:dev .   # from the repository root
#
# The build context is the repository root, since strata-server depends on
# ../strata by path.
#
# The image runs on any x86-64 (or aarch64) CPU: Strata picks its SIMD
# kernels at run time (AVX2, AVX-512 and VNNI; the aarch64 dot product
# instructions), and the server logs which at start. To tune the whole
# binary to the build machine instead, which may then not start elsewhere:
#   docker build --build-arg RUSTFLAGS="-C target-cpu=native" ...
FROM rust:1-bookworm AS build
ARG RUSTFLAGS=""
ENV RUSTFLAGS=${RUSTFLAGS}
WORKDIR /src
COPY strata strata
COPY strata-server strata-server
COPY strata-crawl strata-crawl
# Compiled in (src/mcp.rs).
COPY docs/strata/query-language.md docs/strata/build-an-app.md docs/strata/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/strata-server/target \
    cargo build --release --manifest-path strata-server/Cargo.toml --bins \
    && mkdir /out && cp strata-server/target/release/strata-server strata-server/target/release/strata-server-bench strata-server/target/release/engine-compare /out/
# The crawler talks to strata-server over HTTP only: a crate of its own.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/strata-crawl/target \
    cargo build --release --manifest-path strata-crawl/Cargo.toml \
    && cp strata-crawl/target/release/strata-crawl /out/

FROM debian:bookworm-slim
# iproute2 for `tc`, to add network delay to a pod in tests.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates iproute2 curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
# ChloeDB was called Strata: each program answers to its new name too
# (chloedb-server, chloedb-crawl), and its old one keeps working.
RUN ln -s strata-server /usr/local/bin/chloedb-server && ln -s strata-crawl /usr/local/bin/chloedb-crawl
# engine-compare's traffic profiles (engine-compare plan/replay --profile).
COPY strata-server/deploy/compare/profiles /usr/local/share/engine-compare/profiles
EXPOSE 7700 7701 7800
ENTRYPOINT ["strata-server"]
