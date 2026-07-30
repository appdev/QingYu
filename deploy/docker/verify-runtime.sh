#!/bin/sh

set -eu

if [ "${1-}" != "--status" ]; then
  printf '%s\n' \
    'Runtime verification is unavailable: run with --status to inspect the phase gate.' >&2
  exit 64
fi

printf '%s\n' \
  'READY(runtime-ready): the precompiled Kernel and server-backed Web application are packaged for the fixed /data runtime.' \
  'PENDING(final-live-linux-acceptance): Docker/Linux startup, persistence, reverse-proxy, WebSocket, and graceful-drain evidence must still be captured on the target host.'
