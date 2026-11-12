# chloedb-server, its benchmark and engine-compare in a small Linux image,
# with chloedb-crawl, chloedb-sheets and chloectx, the crawler, the
# spreadsheets and ChloeCtx the operator runs beside a cluster
# (spec.crawler, spec.sheets, spec.chloectx).
#
#   docker build -f chloedb-server/deploy/Dockerfile -t chloedb-server:dev .   # from the repository root
#
# The build context is the repository root, since chloedb-server depends on
# ../chloedb by path.
#
# The image runs on any x86-64 (or aarch64) CPU: ChloeDB picks its SIMD
# kernels at run time (AVX2, AVX-512 and VNNI; the aarch64 dot product
# instructions), and the server logs which at start. To tune the whole
# binary to the build machine instead, which may then not start elsewhere:
#   docker build --build-arg RUSTFLAGS="-C target-cpu=native" ...
FROM rust:1-bookworm AS build
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
# And ChloeCtx (`chloectx app`), which embeds ChloeDB for its records and
# keeps them in the cluster. Its embedding model is not in the image: the
# operator's pod downloads it at start (chloectx fetch-model, with curl).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/chloectx/target \
    cargo build --release --manifest-path chloectx/Cargo.toml --bin chloectx \
    && cp chloectx/target/release/chloectx /out/

FROM debian:bookworm-slim
# iproute2 for `tc`, to add network delay to a pod in tests.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates iproute2 curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
# ChloeDB was called Strata: each program answers to its old name too
# (strata-server, strata-crawl, strata-sheets), which the operator's pods
# run it by.
RUN ln -s chloedb-server /usr/local/bin/strata-server \
    && ln -s chloedb-crawl /usr/local/bin/strata-crawl \
    && ln -s chloedb-sheets /usr/local/bin/strata-sheets
# engine-compare's traffic profiles (engine-compare plan/replay --profile).
COPY chloedb-server/deploy/compare/profiles /usr/local/share/engine-compare/profiles
EXPOSE 7700 7701 7760 7800 7820
# The database lives in /data: mount a volume there to keep it.
RUN mkdir -p /data
ENTRYPOINT ["chloedb-server"]
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
