# syntax=docker/dockerfile:1.7

FROM node:24-bookworm-slim AS web-build

WORKDIR /src

RUN corepack enable \
    && corepack prepare pnpm@10.30.3 --activate

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/package.json
COPY packages/app/package.json packages/app/package.json
COPY packages/editor/package.json packages/editor/package.json
COPY packages/editor-react/package.json packages/editor-react/package.json
COPY packages/kernel-client/package.json packages/kernel-client/package.json
COPY packages/markdown/package.json packages/markdown/package.json
COPY packages/scripts/package.json packages/scripts/package.json
COPY packages/shared/package.json packages/shared/package.json
COPY packages/ui/package.json packages/ui/package.json

RUN pnpm install --frozen-lockfile

COPY apps/web apps/web
COPY packages packages

RUN pnpm --filter @markra/web build

FROM rust:1.92-bookworm AS kernel-build

WORKDIR /src

COPY apps/kernel/Cargo.toml apps/kernel/Cargo.lock apps/kernel/
COPY apps/kernel/src apps/kernel/src

RUN cargo build --locked --release --manifest-path apps/kernel/Cargo.toml --bin qingyu-kernel

FROM debian:bookworm-slim AS qingyu-runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 qingyu \
    && useradd --uid 10001 --gid 10001 --home-dir /nonexistent \
        --shell /usr/sbin/nologin --no-create-home qingyu \
    && install -d -o 10001 -g 10001 -m 0700 /data /tmp/qingyu \
    && install -d -o 0 -g 0 -m 0755 /opt/qingyu/web

COPY --from=kernel-build /src/apps/kernel/target/release/qingyu-kernel /usr/local/bin/qingyu-kernel
COPY --from=web-build /src/apps/web/dist /opt/qingyu/web
COPY --chmod=0555 deploy/docker/entrypoint.sh /usr/local/bin/qingyu-server-entrypoint

LABEL dev.qingyu.image.kind="kernel-api-with-unserved-web-assets" \
      dev.qingyu.image.phase-gate="static-web-serving-required" \
      dev.qingyu.image.web-assets="/opt/qingyu/web"

USER 10001:10001
WORKDIR /data

EXPOSE 3210
STOPSIGNAL SIGTERM
HEALTHCHECK NONE

ENTRYPOINT ["/usr/local/bin/qingyu-server-entrypoint"]
