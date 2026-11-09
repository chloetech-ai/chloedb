# chloedb-server, its benchmark and engine-compare in a small Linux image,
# with chloedb-crawl and chloedb-sheets, the crawler and the spreadsheets the
# operator runs beside a cluster (spec.crawler, spec.sheets).
#
#   docker build -f chloedb-server/deploy/Dockerfile -t strata-server:dev .   # from the repository root
#
# The build context is the repository root, since chloedb-server depends on
# ../chloedb by path.
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
COPY chloedb-server chloedb-server
COPY chloedb-crawl chloedb-crawl
COPY chloedb-sheets chloedb-sheets
# Compiled in (src/mcp.rs).
COPY docs/chloedb/query-language.md docs/chloedb/build-an-app.md docs/chloedb/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/chloedb-server/target \
    cargo build --release --manifest-path chloedb-server/Cargo.toml --bins \
    && mkdir /out && cp chloedb-server/target/release/chloedb-server chloedb-server/target/release/chloedb-server-bench chloedb-server/target/release/engine-compare /out/
# The crawler talks to chloedb-server over HTTP only: a crate of its own.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/chloedb-crawl/target \
    cargo build --release --manifest-path chloedb-crawl/Cargo.toml \
    && cp chloedb-crawl/target/release/chloedb-crawl /out/
# So do the spreadsheets.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/chloedb-sheets/target \
    cargo build --release --manifest-path chloedb-sheets/Cargo.toml \
    && cp chloedb-sheets/target/release/chloedb-sheets /out/

FROM debian:bookworm-slim
# iproute2 for `tc`, to add network delay to a pod in tests.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates iproute2 curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
# ChloeDB was called Strata: each program answers to its new name too
# (chloedb-server, chloedb-crawl, chloedb-sheets), and its old one keeps
# working.
RUN ln -s chloedb-server /usr/local/bin/chloedb-server \
    && ln -s chloedb-crawl /usr/local/bin/chloedb-crawl \
    && ln -s chloedb-sheets /usr/local/bin/chloedb-sheets
# engine-compare's traffic profiles (engine-compare plan/replay --profile).
COPY chloedb-server/deploy/compare/profiles /usr/local/share/engine-compare/profiles
EXPOSE 7700 7701 7800 7820
ENTRYPOINT ["chloedb-server"]
