# The app engine (chloedb-apps) in an image of its own: Node, Playwright and
# a headless Chromium (the drawn checks), and Claude Code (the model's
# client). The operator runs it beside a cluster (spec.apps), on the
# cluster's coordinators:
#
#   docker build -f chloedb-apps/deploy/Dockerfile -t strata-apps:dev chloedb-apps   # from the repository root
#
# The build context is chloedb-apps. Run alone, without --strata, it starts a
# chloedb-server of its own (copied from STRATA_SERVER_IMAGE) on /data:
#
#   docker run --rm -p 7730:7730 -v apps:/data -e ANTHROPIC_API_KEY strata-apps:dev
#
# Nothing in it updates itself: Claude Code's updater is off, the engine's
# self-evolving is off (its code is the image's), and the versions below
# are pinned.
ARG STRATA_SERVER_IMAGE=ghcr.io/chloetech-ai/strata-server:latest
ARG NODE_IMAGE=node:24-bookworm-slim
FROM ${STRATA_SERVER_IMAGE} AS server

FROM ${NODE_IMAGE}
# Playwright and Claude Code in /opt/node_modules, where the engine finds
# them from /opt/chloedb-apps as Node looks up the tree; Chromium's headless
# shell only (what chromium.launch() runs), with the libraries it needs.
ENV PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright
WORKDIR /opt
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && npm install --no-audit --no-fund --no-package-lock --omit=dev playwright@1.63.0 @anthropic-ai/claude-code@2.1.310 \
    && npx playwright install --with-deps --only-shell chromium \
    && rm -rf /var/lib/apt/lists/* && npm cache clean --force
WORKDIR /opt/chloedb-apps
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund --omit=dev && npm cache clean --force
COPY engine engine
COPY library library
COPY web web
COPY --from=server /usr/local/bin/chloedb-server /usr/local/bin/chloedb-server
# The data (a chloedb-server's own, uploaded files, the map tiles it keeps,
# what the model has cost) and Claude Code's home are on /data, a volume.
RUN mkdir -p /data/home && chown -R node:node /data
ENV NODE_ENV=production \
    HOME=/data/home \
    STRATA_APPS_DATA=/data \
    STRATA_APPS_CLAUDE=/opt/node_modules/.bin/claude \
    STRATA_APPS_EVOLVE=off \
    STRATA_SERVER_BIN=/usr/local/bin/chloedb-server \
    DISABLE_AUTOUPDATER=1 \
    HOST=0.0.0.0 \
    PORT=7730
USER node
VOLUME /data
EXPOSE 7730
# HOME may be on a new, empty volume.
ENTRYPOINT ["sh", "-c", "mkdir -p \"$HOME\" && exec node engine/server.mjs \"$@\"", "chloedb-apps"]
