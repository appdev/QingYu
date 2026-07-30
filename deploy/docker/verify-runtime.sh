#!/bin/sh

set -eu

if [ "${1-}" != "--status" ]; then
  printf '%s\n' \
    'Runtime verification is unavailable: run with --status to inspect the phase gate.' >&2
  exit 64
fi

printf '%s\n' \
  'BLOCKED(server-entrypoint-required): qingyu-kernel serve is currently a native stdin/loopback child, not a Docker server entrypoint; initialization-token handling is also not implemented.' >&2
exit 78
