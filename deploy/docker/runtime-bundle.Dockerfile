# syntax=docker/dockerfile:1.7

FROM debian:bookworm-slim AS qingyu-runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 qingyu \
    && useradd --uid 10001 --gid 10001 --home-dir /nonexistent \
        --shell /usr/sbin/nologin --no-create-home qingyu \
    && install -d -o 10001 -g 10001 -m 0700 /data /tmp/qingyu \
    && install -d -o 0 -g 0 -m 0755 /opt/qingyu/web

COPY --chmod=0555 bin/qingyu-kernel /usr/local/bin/qingyu-kernel
COPY web/ /opt/qingyu/web/
COPY --chmod=0555 scripts/entrypoint.sh /usr/local/bin/qingyu-server-entrypoint
COPY --chmod=0555 scripts/verify-final-web-assets.sh /usr/local/bin/qingyu-verify-final-web-assets

RUN /usr/local/bin/qingyu-verify-final-web-assets /opt/qingyu/web

LABEL dev.qingyu.image.kind="kernel-api-with-served-web-assets" \
      dev.qingyu.image.runtime-status="ready" \
      dev.qingyu.image.live-linux-acceptance="pending" \
      dev.qingyu.image.web-assets="/opt/qingyu/web" \
      dev.qingyu.image.web-assets-served="true"

USER 10001:10001
WORKDIR /data

EXPOSE 3210
STOPSIGNAL SIGTERM
HEALTHCHECK NONE

ENTRYPOINT ["/usr/local/bin/qingyu-server-entrypoint"]
