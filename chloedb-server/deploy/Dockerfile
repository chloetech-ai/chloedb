# chloedb-server in a small Linux image, with chloedb-crawl, chloedb-sheets
# and chloectx, the crawler, the spreadsheets and ChloeCtx the operator runs
# beside a cluster (spec.crawler, spec.sheets, spec.chloectx). It runs as
# user 10001 (chloedb), not root; its data directory is /data.
#
#   docker build -f chloedb-server/deploy/Dockerfile -t chloedb-server:dev .   # from the repository root
#
# The benchmark tools (chloedb-server-bench, engine-compare and its traffic
# profiles) are in the `bench` stage, an image of their own that runs as
# root, for the scripts in deploy/ that measure:
#
#   docker build -f chloedb-server/deploy/Dockerfile --target bench -t chloedb-server:bench .
#
# The base images are pinned by digest, so that a release is rebuilt from
# the same bases until the pins are moved on purpose (rust:1-bookworm and
# debian:bookworm-slim, as of 12 November 2026).
#
# The build context is the repository root, since chloedb-server depends on
# ../chloedb by path.
#
# The image runs on any x86-64 (or aarch64) CPU: ChloeDB picks its SIMD
# kernels at run time (AVX2, AVX-512 and VNNI; the aarch64 dot product
# instructions), and the server logs which at start. To tune the whole
# binary to the build machine instead, which may then not start elsewhere:
#   docker build --build-arg RUSTFLAGS="-C target-cpu=native" ...
FROM rust:1-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c AS build
ARG RUSTFLAGS=""
ENV RUSTFLAGS=${RUSTFLAGS}
WORKDIR /src
COPY chloedb chloedb
COPY chloedb-server chloedb-server
COPY chloedb-crawl chloedb-crawl
COPY chloedb-sheets chloedb-sheets
COPY chloectx chloectx
# Compiled in (src/mcp.rs).
COPY docs/chloedb/query-language.md docs/chloedb/build-an-app.md docs/chloedb/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/chloedb-server/target \
    cargo build --release --manifest-path chloedb-server/Cargo.toml --bins \
    && mkdir /out /out-bench && cp chloedb-server/target/release/chloedb-server /out/ \
    && cp chloedb-server/target/release/chloedb-server-bench chloedb-server/target/release/engine-compare /out-bench/
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
# And ChloeCtx (`chloectx app`), which embeds ChloeDB for its records and
# keeps them in the cluster. Its embedding model is not in the image: the
# operator's pod downloads it at start (chloectx fetch-model, with curl).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/chloectx/target \
    cargo build --release --manifest-path chloectx/Cargo.toml --bin chloectx \
    && cp chloectx/target/release/chloectx /out/

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc AS runtime
# curl for ChloeCtx's pod, which downloads its model at start.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
# The user the servers run as, by number (so that Kubernetes' runAsNonRoot
# can check it), and the directory they keep their data in.
RUN groupadd --system --gid 10001 chloedb \
    && useradd --system --uid 10001 --gid 10001 --home-dir /data --shell /usr/sbin/nologin chloedb \
    && mkdir /data && chown 10001:10001 /data
COPY --from=build /out/ /usr/local/bin/
# ChloeDB was called Strata: each program answers to its old name too
# (strata-server, strata-crawl, strata-sheets), which the operator's pods
# run it by.
RUN ln -s chloedb-server /usr/local/bin/strata-server \
    && ln -s chloedb-crawl /usr/local/bin/strata-crawl \
    && ln -s chloedb-sheets /usr/local/bin/strata-sheets
WORKDIR /data
EXPOSE 7700 7701 7760 7800 7820
# The database lives in /data: mount a volume there to keep it.
RUN mkdir -p /data
ENTRYPOINT ["chloedb-server"]

# The benchmark tools on top, as root (deploy/kind-test.sh and
# deploy/compare/compare.sh build this stage).
FROM runtime AS bench
USER 0
COPY --from=build /out-bench/ /usr/local/bin/
# engine-compare's traffic profiles (engine-compare plan/replay --profile).
COPY chloedb-server/deploy/compare/profiles /usr/local/share/engine-compare/profiles

# The image published: the last stage, so the default target.
FROM runtime AS server
USER 10001:10001
# With no arguments, `docker run` serves /data/chloedb.chloedb (created
# with no tables the first time) on all of the container's interfaces,
# which the server allows only with a token:
#
#   docker run -p 127.0.0.1:7700:7700 -e CHLOEDB_TOKEN=pick-a-secret -v chloedb:/data ghcr.io/chloetech-ai/chloedb-server
#
# Arguments replace all of these (pass --listen 0.0.0.0:7700 again). The
# operator's pods give their own command or arguments, so this default never
# applies to them.
CMD ["--db", "/data/chloedb.chloedb", "--listen", "0.0.0.0:7700", "--grpc", "0.0.0.0:7701"]
