#!/bin/sh

set -eu

if [ "${1-}" != "--status" ]; then
  printf '%s\n' \
    'Runtime verification is unavailable: run with --status to inspect the phase gate.' >&2
  exit 64
fi

printf '%s\n' \
  'BLOCKED(static-web-serving-required): qingyu-kernel server exposes the Kernel API but does not serve the Web build copied to /opt/qingyu/web.' >&2
exit 78
