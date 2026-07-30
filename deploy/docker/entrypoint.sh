#!/bin/sh

set -eu

: "${QINGYU_PUBLIC_ORIGIN:?QINGYU_PUBLIC_ORIGIN is required}"

exec /usr/local/bin/qingyu-kernel server --public-origin "$QINGYU_PUBLIC_ORIGIN"
