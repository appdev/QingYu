#!/bin/sh

set -eu

if [ "${1-}" != "--status" ]; then
  printf '%s\n' \
    'Runtime verification is unavailable: run with --status to inspect the phase gate.' >&2
  exit 64
fi

printf '%s\n' \
  'BLOCKED(web-kernel-runtime-required): qingyu-kernel serves /opt/qingyu/web on the API origin, but the Web entrypoint is not yet wired exclusively to KernelClient.' >&2
exit 78
