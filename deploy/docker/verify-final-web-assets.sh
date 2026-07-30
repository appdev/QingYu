#!/bin/sh

set -eu

maximum_nodes=10000
maximum_depth=32
maximum_file_bytes=16777216
maximum_total_bytes=134217728
maximum_path_metadata_bytes=16777216

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

injected_limit() {
  name=$1
  maximum=$2
  case "$name" in
    QINGYU_VERIFY_WEB_MAX_NODES) value=${QINGYU_VERIFY_WEB_MAX_NODES-} ;;
    QINGYU_VERIFY_WEB_MAX_PATH_BYTES) value=${QINGYU_VERIFY_WEB_MAX_PATH_BYTES-} ;;
    *) fail "unknown injected verifier limit" ;;
  esac
  if [ -z "$value" ]; then
    printf '%s\n' "$maximum"
    return
  fi
  case "$value" in
    *[!0-9]* | 0) fail "$name must be a positive decimal integer" ;;
  esac
  awk -v value="$value" -v maximum="$maximum" \
    'BEGIN { exit !(value <= maximum && value == int(value)) }' \
    || fail "$name may only inject a limit at or below $maximum"
  printf '%s\n' "$value"
}

[ "$#" -eq 1 ] || fail "usage: verify-final-web-assets.sh <Web distribution directory>"
root=$1
[ -d "$root" ] && [ ! -L "$root" ] \
  || fail "Web distribution root must be a directory, not a symbolic link"

maximum_nodes=$(injected_limit QINGYU_VERIFY_WEB_MAX_NODES "$maximum_nodes")
maximum_path_metadata_bytes=$(
  injected_limit QINGYU_VERIFY_WEB_MAX_PATH_BYTES "$maximum_path_metadata_bytes"
)

node_count=$(find "$root" -mindepth 1 -print | awk 'END { print NR + 0 }')
[ "$node_count" -le "$maximum_nodes" ] \
  || fail "Web distribution exceeds the node-count limit"

path_metadata_bytes=$(
  find "$root" -mindepth 1 -exec sh -c '
    root=$1
    shift
    for entry do
      relative=${entry#"$root"/}
      LC_ALL=C printf %s "$relative"
    done
  ' sh "$root" {} + | wc -c | tr -d '[:space:]'
)
[ "$path_metadata_bytes" -le "$maximum_path_metadata_bytes" ] \
  || fail "Web distribution exceeds the path-metadata byte limit"

if find "$root" -mindepth 1 ! -type d ! -type f -print -quit | grep -q .; then
  fail "symbolic links and special files are forbidden in the Web distribution"
fi

find "$root" -mindepth 1 -type d -exec sh -c '
  root=$1
  maximum=$2
  shift 2
  for directory do
    relative=${directory#"$root"/}
    depth=1
    remainder=$relative
    while [ "${remainder#*/}" != "$remainder" ]; do
      depth=$((depth + 1))
      remainder=${remainder#*/}
    done
    [ "$depth" -le "$maximum" ] || exit 1
  done
' sh "$root" "$maximum_depth" {} + \
  || fail "Web distribution exceeds the directory-depth limit"

if find "$root" -type f \( -perm -001 -o -perm -010 -o -perm -100 \) \
  -print -quit | grep -q .; then
  fail "executable files are forbidden in the Web distribution"
fi

find "$root" -type f -exec sh -c '
  for asset do
    if links=$(stat -c %h -- "$asset" 2>/dev/null); then
      :
    elif links=$(stat -f %l -- "$asset" 2>/dev/null); then
      :
    else
      exit 1
    fi
    [ "$links" -eq 1 ] || exit 1
  done
' sh {} + || fail "hard links are forbidden in the Web distribution"

find "$root" -type f -exec sh -c '
  for asset do
    extension=${asset##*.}
    extension=$(LC_ALL=C printf %s "$extension" | tr "[:upper:]" "[:lower:]")
    case "$extension" in
      css | gif | html | ico | jpeg | jpg | js | json | mjs | otf | png | ttf | txt | webp | woff | woff2) ;;
      *) exit 1 ;;
    esac
  done
' sh {} + || fail "unsupported Web asset extension"

if find "$root" -type f -size +${maximum_file_bytes}c -print -quit | grep -q .; then
  fail "Web asset exceeds the single-file byte limit"
fi

total_bytes=$(
  find "$root" -type f -exec sh -c '
    for asset do
      wc -c < "$asset"
    done
  ' sh {} + | awk '{ total += $1 } END { print total + 0 }'
)
[ "$total_bytes" -le "$maximum_total_bytes" ] \
  || fail "Web distribution exceeds the aggregate byte limit"

find "$root" -type f -exec sh -c '
  for asset do
    magic=$(od -An -tx1 -N4 "$asset" | tr -d " \n")
    case "$magic" in
      7f454c46 | cafebabe | bebafeca | cafebabf | bfbafeca | feedface | cefaedfe | feedfacf | cffaedfe | 4d5a*)
        exit 1
        ;;
    esac
  done
' sh {} + || fail "executable binary content is forbidden in the Web distribution"

[ -f "$root/index.html" ] && [ ! -L "$root/index.html" ] \
  || fail "Web distribution must contain a regular root index.html"

printf 'PASS: verified final Web asset tree (%s nodes, %s bytes).\n' \
  "$node_count" "$total_bytes"
