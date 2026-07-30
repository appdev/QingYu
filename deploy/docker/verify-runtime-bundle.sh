#!/bin/sh

set -eu

maximum_archive_bytes=268435456
maximum_web_nodes=10000
maximum_web_depth=32
maximum_web_file_bytes=16777216
maximum_web_total_bytes=134217728
maximum_web_path_bytes=16777216
expected_dockerfile_sha256=4105ed08371190746e77440d4ea0c20744c7f562e0d34c99ae5a7ec33dbfde8d
expected_compose_sha256=7ba2adcb689c5aa3a072f3f1e74132a705e58de90880e146a609fa45fecd2d5e
expected_entrypoint_sha256=514fe144a1655f5444f5a35a243bd79dc72ed1df2c0e34e9b896ca8e85d07dfd
expected_final_web_verifier_sha256=1ba59add40f50071037a10d7c2ce596fc860a5d56ee3a7c0ebadca20e327d002

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

usage() {
  fail 'usage: verify-runtime-bundle.sh --archive <archive.tar.gz> --architecture <amd64|arm64> [--extract-to <new-directory>]'
}

hash_file() {
  target=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$target" | awk '{ print $1 }'
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$target" | awk '{ print $1 }'
    return
  fi
  fail 'SHA-256 utility is required (sha256sum or shasum)'
}

link_count() {
  target=$1
  if count=$(stat -c %h -- "$target" 2>/dev/null); then
    printf '%s\n' "$count"
    return
  fi
  if count=$(stat -f %l -- "$target" 2>/dev/null); then
    printf '%s\n' "$count"
    return
  fi
  fail "could not inspect hard-link count: $target"
}

file_mode() {
  target=$1
  if mode=$(stat -c %a -- "$target" 2>/dev/null); then
    printf '%s\n' "$mode"
    return
  fi
  if mode=$(stat -f %Lp -- "$target" 2>/dev/null); then
    printf '%s\n' "$mode"
    return
  fi
  fail "could not inspect file mode: $target"
}

require_mode() {
  mode_target=$1
  expected_mode=$2
  mode_label=$3
  [ "$(file_mode "$mode_target")" = "$expected_mode" ] \
    || fail "bundle file mode must be $expected_mode: $mode_label"
}

verify_elf() {
  target=$1
  architecture=$2
  [ -f "$target" ] && [ ! -L "$target" ] && [ -x "$target" ] \
    || fail 'bundled Kernel must be one executable regular file'
  [ "$(link_count "$target")" -eq 1 ] \
    || fail 'bundled Kernel must not be a hard link'
  size=$(wc -c <"$target" | tr -d '[:space:]')
  [ "$size" -ge 64 ] \
    || fail 'bundled Kernel must be a 64-bit little-endian ELF executable'
  header=$(od -An -tx1 -N6 "$target" | tr -d '[:space:]')
  [ "$header" = '7f454c460201' ] \
    || fail 'bundled Kernel must be a 64-bit little-endian ELF executable'
  os_abi=$(od -An -tu1 -j7 -N1 "$target" | tr -d '[:space:]')
  case "$os_abi" in
    0 | 3) ;;
    *) fail 'bundled Kernel must be a Linux-compatible ELF executable' ;;
  esac
  set -- $(od -An -tu1 -j16 -N2 "$target")
  [ "$#" -eq 2 ] && { [ "$1" -eq 2 ] || [ "$1" -eq 3 ]; } && [ "$2" -eq 0 ] \
    || fail 'bundled Kernel must be an executable or position-independent ELF binary'
  set -- $(od -An -tu1 -j18 -N2 "$target")
  [ "$#" -eq 2 ] \
    || fail 'bundled Kernel must be a 64-bit little-endian ELF executable'
  machine=$(( $1 + ($2 * 256) ))
  case "$architecture:$machine" in
    amd64:62 | arm64:183) ;;
    *) fail "Kernel ELF architecture does not match requested linux/$architecture" ;;
  esac
  set -- $(od -An -tu1 -j20 -N4 "$target")
  [ "$#" -eq 4 ] \
    && [ "$1" -eq 1 ] && [ "$2" -eq 0 ] && [ "$3" -eq 0 ] && [ "$4" -eq 0 ] \
    || fail 'bundled Kernel has an invalid ELF version'
}

verify_frozen_control_files() {
  control_root=$1
  [ "$(hash_file "$control_root/Dockerfile")" = "$expected_dockerfile_sha256" ] \
    || fail 'frozen runtime control file checksum mismatch: Dockerfile'
  [ "$(hash_file "$control_root/compose.yaml")" = "$expected_compose_sha256" ] \
    || fail 'frozen runtime control file checksum mismatch: compose.yaml'
  [ "$(hash_file "$control_root/scripts/entrypoint.sh")" = "$expected_entrypoint_sha256" ] \
    || fail 'frozen runtime control file checksum mismatch: scripts/entrypoint.sh'
  [ "$(hash_file "$control_root/scripts/verify-final-web-assets.sh")" = "$expected_final_web_verifier_sha256" ] \
    || fail 'frozen runtime control file checksum mismatch: scripts/verify-final-web-assets.sh'
}

verify_passive_web_tree() {
  passive_web_root=$1
  [ -d "$passive_web_root" ] && [ ! -L "$passive_web_root" ] \
    || fail 'Web distribution root must be a directory, not a symbolic link'

  passive_node_count=$(find "$passive_web_root" -mindepth 1 -print | awk 'END { print NR + 0 }')
  [ "$passive_node_count" -le "$maximum_web_nodes" ] \
    || fail 'Web distribution exceeds the node-count limit'
  passive_path_bytes=$(
    find "$passive_web_root" -mindepth 1 -exec sh -c '
      web_root=$1
      shift
      for entry do
        relative=${entry#"$web_root"/}
        LC_ALL=C printf %s "$relative"
      done
    ' sh "$passive_web_root" {} + | wc -c | tr -d '[:space:]'
  )
  [ "$passive_path_bytes" -le "$maximum_web_path_bytes" ] \
    || fail 'Web distribution exceeds the path-metadata byte limit'

  if find "$passive_web_root" -mindepth 1 ! -type d ! -type f -print -quit | grep -q .; then
    fail 'symbolic links and special files are forbidden in the Web distribution'
  fi
  find "$passive_web_root" -mindepth 1 -type d -exec sh -c '
    web_root=$1
    maximum=$2
    shift 2
    for directory do
      relative=${directory#"$web_root"/}
      depth=1
      remainder=$relative
      while [ "${remainder#*/}" != "$remainder" ]; do
        depth=$((depth + 1))
        remainder=${remainder#*/}
      done
      [ "$depth" -le "$maximum" ] || exit 1
    done
  ' sh "$passive_web_root" "$maximum_web_depth" {} + \
    || fail 'Web distribution exceeds the directory-depth limit'
  if find "$passive_web_root" -type f \( -perm -001 -o -perm -010 -o -perm -100 \) \
    -print -quit | grep -q .; then
    fail 'executable files are forbidden in the Web distribution'
  fi
  find "$passive_web_root" -type f -exec sh -c '
    for asset do
      extension=${asset##*.}
      extension=$(LC_ALL=C printf %s "$extension" | tr "[:upper:]" "[:lower:]")
      case "$extension" in
        css | gif | html | ico | jpeg | jpg | js | json | mjs | otf | png | ttf | txt | webp | woff | woff2) ;;
        *) exit 1 ;;
      esac
    done
  ' sh {} + || fail 'unsupported Web asset extension'
  if find "$passive_web_root" -type f -size +${maximum_web_file_bytes}c -print -quit | grep -q .; then
    fail 'Web asset exceeds the single-file byte limit'
  fi
  passive_total_bytes=$(
    find "$passive_web_root" -type f -exec sh -c '
      for asset do wc -c <"$asset"; done
    ' sh {} + | awk '{ total += $1 } END { print total + 0 }'
  )
  [ "$passive_total_bytes" -le "$maximum_web_total_bytes" ] \
    || fail 'Web distribution exceeds the aggregate byte limit'
  find "$passive_web_root" -type f -exec sh -c '
    for asset do
      magic=$(od -An -tx1 -N4 "$asset" | tr -d " \n")
      case "$magic" in
        7f454c46 | cafebabe | bebafeca | cafebabf | bfbafeca | \
        feedface | cefaedfe | feedfacf | cffaedfe | 4d5a*) exit 1 ;;
      esac
    done
  ' sh {} + || fail 'executable binary content is forbidden in the Web distribution'
  [ -f "$passive_web_root/index.html" ] && [ ! -L "$passive_web_root/index.html" ] \
    || fail 'Web distribution must contain a regular root index.html'
}

reject_source_like_web_paths() {
  web_root=$1
  find "$web_root" -mindepth 1 -exec sh -c '
    web_root=$1
    shift
    for entry do
      relative=${entry#"$web_root"/}
      case "$relative" in
        *"\\"*)
          printf "%s\n" "FAIL: Web distribution paths must not contain control characters or backslashes" >&2
          exit 1
          ;;
      esac
      clean=$(LC_ALL=C printf %s "$relative" | tr -d "\n\r\t")
      if [ "$clean" != "$relative" ]; then
        printf "%s\n" "FAIL: Web distribution paths must not contain control characters or backslashes" >&2
        exit 1
      fi
      case "/$relative/" in
        */.git/* | */node_modules/* | */src/*)
          component=$(printf "%s\n" "$relative" | awk -F/ "
            { for (field_index = 1; field_index <= NF; field_index += 1) {
                if (\$field_index == \".git\" || \$field_index == \"node_modules\" || \$field_index == \"src\") {
                  print \$field_index
                  exit
                }
              }
            }
          ")
          printf "%s\n" "FAIL: source-like Web paths are forbidden in a built distribution: $component" >&2
          exit 1
          ;;
      esac
      base=${relative##*/}
      case "$base" in
        package.json | package-lock.json | pnpm-lock.yaml | yarn.lock | bun.lockb | \
        tsconfig.json | tsconfig.*.json | vite.config.*)
          printf "%s\n" "FAIL: source-like Web paths are forbidden in a built distribution: $relative" >&2
          exit 1
          ;;
      esac
    done
  ' sh "$web_root" {} +
}

verify_dockerfile_semantics() {
  dockerfile=$1
  [ "$(grep -c '^FROM ' "$dockerfile")" -eq 1 ] \
    || fail 'runtime-only Dockerfile must contain exactly one runtime stage'
  grep -Fqx 'FROM debian:bookworm-slim AS qingyu-runtime' "$dockerfile" \
    || fail 'runtime-only Dockerfile must use the frozen Debian runtime stage'
  if grep -Eiq '(^|[^[:alnum:]_])(cargo|rustc|rustup|pnpm|npm|node(js)?|yarn|bun|gcc|g\+\+|clang|cmake|make)([^[:alnum:]_]|$)' \
    "$dockerfile"; then
    fail 'runtime-only Dockerfile must not install build toolchains'
  fi
  if grep -Eq '^(ADD|COPY[[:space:]]+(apps|packages|src)(/|[[:space:]]))' "$dockerfile"; then
    fail 'runtime-only Dockerfile must not copy repository source'
  fi
  [ "$(grep -c '^COPY ' "$dockerfile")" -eq 4 ] \
    || fail 'runtime-only Dockerfile must copy exactly the Kernel, Web tree, entrypoint, and Web verifier'
  grep -Fqx 'COPY --chmod=0555 bin/qingyu-kernel /usr/local/bin/qingyu-kernel' "$dockerfile" \
    || fail 'runtime-only Dockerfile must copy only the bundled Kernel binary'
  grep -Fqx 'COPY web/ /opt/qingyu/web/' "$dockerfile" \
    || fail 'runtime-only Dockerfile must copy only the bundled Web distribution'
  grep -Fqx 'COPY --chmod=0555 scripts/entrypoint.sh /usr/local/bin/qingyu-server-entrypoint' "$dockerfile" \
    || fail 'runtime-only Dockerfile must install the bundled entrypoint'
  grep -Fqx 'COPY --chmod=0555 scripts/verify-final-web-assets.sh /usr/local/bin/qingyu-verify-final-web-assets' "$dockerfile" \
    || fail 'runtime-only Dockerfile must install the final Web verifier'
  grep -Fqx 'USER 10001:10001' "$dockerfile" \
    || fail 'runtime-only Dockerfile must retain UID/GID 10001:10001'
  grep -Fqx 'WORKDIR /data' "$dockerfile" \
    || fail 'runtime-only Dockerfile must retain /data as its workdir'
  grep -Fqx 'EXPOSE 3210' "$dockerfile" \
    || fail 'runtime-only Dockerfile must expose only port 3210'
  grep -Fqx 'STOPSIGNAL SIGTERM' "$dockerfile" \
    || fail 'runtime-only Dockerfile must retain SIGTERM'
  grep -Fqx 'ENTRYPOINT ["/usr/local/bin/qingyu-server-entrypoint"]' "$dockerfile" \
    || fail 'runtime-only Dockerfile must retain the fixed entrypoint'
  grep -Fqx 'RUN /usr/local/bin/qingyu-verify-final-web-assets /opt/qingyu/web' "$dockerfile" \
    || fail 'runtime-only Dockerfile must verify final Web assets'
}

verify_manifest() {
  root=$1
  manifest="$root/SHA256SUMS"
  [ -f "$manifest" ] && [ ! -L "$manifest" ] \
    || fail 'bundle must contain one regular SHA256SUMS manifest'
  manifest_paths="$temporary_directory/manifest-paths"
  actual_paths="$temporary_directory/actual-paths"
  : >"$manifest_paths"
  while IFS= read -r line; do
    [ -n "$line" ] || fail 'SHA256SUMS must not contain empty lines'
    digest=${line%% *}
    remainder=${line#"$digest"}
    case "$remainder" in
      '  '*) relative_path=${remainder#'  '} ;;
      *) fail 'SHA256SUMS contains a malformed entry' ;;
    esac
    printf '%s\n' "$digest" | grep -Eq '^[0-9a-f]{64}$' \
      || fail 'SHA256SUMS contains a malformed digest'
    [ -n "$relative_path" ] || fail 'SHA256SUMS contains an empty path'
    case "$relative_path" in
      /* | ./* | *'/../'* | ../* | *'/./'* | *'//'*)
        fail "SHA256SUMS contains an unsafe path: $relative_path"
        ;;
      *'\'*) fail 'SHA256SUMS paths must not contain control characters or backslashes' ;;
    esac
    clean_relative_path=$(LC_ALL=C printf %s "$relative_path" | tr -d '\n\r\t')
    [ "$clean_relative_path" = "$relative_path" ] \
      || fail 'SHA256SUMS paths must not contain control characters or backslashes'
    printf '%s\n' "$relative_path" >>"$manifest_paths"
    actual_digest=$(hash_file "$root/$relative_path")
    [ "$actual_digest" = "$digest" ] \
      || fail "SHA-256 manifest mismatch: $relative_path"
  done <"$manifest"
  [ -s "$manifest_paths" ] || fail 'SHA256SUMS must not be empty'
  [ -z "$(LC_ALL=C sort "$manifest_paths" | uniq -d)" ] \
    || fail 'SHA256SUMS must not contain duplicate paths'
  (
    CDPATH= cd -- "$root"
    find . -type f ! -path './SHA256SUMS' -print \
      | sed 's#^\./##' \
      | LC_ALL=C sort
  ) >"$actual_paths"
  LC_ALL=C sort "$manifest_paths" | cmp -s - "$actual_paths" \
    || fail 'bundle inventory does not exactly match SHA256SUMS'
}

verify_tree() {
  root=$1
  [ -d "$root" ] && [ ! -L "$root" ] || fail 'bundle root must be a retained directory'
  if find "$root" -mindepth 1 ! -type d ! -type f -print -quit | grep -q .; then
    fail 'bundle may contain only regular files and directories'
  fi
  find "$root" -type f -exec sh -c '
    for target do
      if links=$(stat -c %h -- "$target" 2>/dev/null); then :
      elif links=$(stat -f %l -- "$target" 2>/dev/null); then :
      else exit 1
      fi
      [ "$links" -eq 1 ] || exit 1
    done
  ' sh {} + || fail 'hard links are forbidden in the runtime bundle'

  for required_path in \
    Dockerfile \
    compose.yaml \
    BUNDLE-METADATA \
    SHA256SUMS \
    bin/qingyu-kernel \
    scripts/entrypoint.sh \
    scripts/verify-final-web-assets.sh \
    scripts/verify-runtime-bundle.sh \
    web/index.html; do
    [ -f "$root/$required_path" ] && [ ! -L "$root/$required_path" ] \
      || fail "bundle is missing required retained file: $required_path"
  done

  expected_metadata=$(printf 'format=qingyu-runtime-bundle-v2\nos=linux\narchitecture=%s' "$architecture")
  actual_metadata=$(cat "$root/BUNDLE-METADATA")
  [ "$actual_metadata" = "$expected_metadata" ] \
    || fail 'bundle metadata does not match the requested Linux architecture'

  verify_manifest "$root"
  verify_frozen_control_files "$root"
  for read_only_path in Dockerfile compose.yaml BUNDLE-METADATA SHA256SUMS; do
    require_mode "$root/$read_only_path" 444 "$read_only_path"
  done
  for executable_path in \
    bin/qingyu-kernel \
    scripts/entrypoint.sh \
    scripts/verify-final-web-assets.sh \
    scripts/verify-runtime-bundle.sh; do
    require_mode "$root/$executable_path" 555 "$executable_path"
  done
  for retained_directory in bin scripts web; do
    require_mode "$root/$retained_directory" 555 "$retained_directory"
  done
  find "$root/web" -mindepth 1 -type d -exec sh -c '
    for directory do
      if mode=$(stat -c %a -- "$directory" 2>/dev/null); then :
      elif mode=$(stat -f %Lp -- "$directory" 2>/dev/null); then :
      else exit 1
      fi
      [ "$mode" = 555 ] || exit 1
    done
  ' sh {} + || fail 'bundled Web directories must have mode 555'
  find "$root/web" -type f -exec sh -c '
    for asset do
      if mode=$(stat -c %a -- "$asset" 2>/dev/null); then :
      elif mode=$(stat -f %Lp -- "$asset" 2>/dev/null); then :
      else exit 1
      fi
      [ "$mode" = 444 ] || exit 1
    done
  ' sh {} + || fail 'bundled Web files must have mode 444'
  verify_elf "$root/bin/qingyu-kernel" "$architecture"
  reject_source_like_web_paths "$root/web"
  verify_passive_web_tree "$root/web"
  verify_dockerfile_semantics "$root/Dockerfile"

}

archive=
architecture=
extract_to=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --archive)
      [ -z "$archive" ] && [ "$#" -ge 2 ] || usage
      archive=$2
      shift 2
      ;;
    --architecture)
      [ -z "$architecture" ] && [ "$#" -ge 2 ] || usage
      architecture=$2
      shift 2
      ;;
    --extract-to)
      [ -z "$extract_to" ] && [ "$#" -ge 2 ] || usage
      extract_to=$2
      shift 2
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[ -n "$archive" ] && [ -n "$architecture" ] || usage
case "$architecture" in
  amd64 | arm64) ;;
  *) fail 'architecture must be amd64 or arm64' ;;
esac
[ -f "$archive" ] && [ ! -L "$archive" ] \
  || fail 'archive must be one retained regular file, not a symbolic link'
[ "$(link_count "$archive")" -eq 1 ] || fail 'archive must not be a hard link'
archive_bytes=$(wc -c <"$archive" | tr -d '[:space:]')
[ "$archive_bytes" -le "$maximum_archive_bytes" ] \
  || fail 'runtime bundle archive exceeds the compressed-size limit'

if [ -n "$extract_to" ]; then
  [ ! -e "$extract_to" ] && [ ! -L "$extract_to" ] \
    || fail "extract target already exists: $extract_to"
  case "$extract_to" in
    */*) extract_parent=${extract_to%/*}; [ -n "$extract_parent" ] || extract_parent=/ ;;
    *) extract_parent=. ;;
  esac
  [ -d "$extract_parent" ] && [ ! -L "$extract_parent" ] \
    || fail "extract parent must be a retained directory: $extract_parent"
  temporary_directory=$(mktemp -d "$extract_parent/.qingyu-runtime-verify.XXXXXX")
else
  temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/qingyu-runtime-verify.XXXXXX")
fi
cleanup() {
  chmod -R u+w "$temporary_directory" 2>/dev/null || true
  rm -rf "$temporary_directory"
}
trap cleanup EXIT HUP INT TERM

archive_paths="$temporary_directory/archive-paths"
archive_types="$temporary_directory/archive-types"
tar -tzf "$archive" >"$archive_paths" \
  || fail 'runtime bundle is not a readable gzip-compressed tar archive'
[ -s "$archive_paths" ] || fail 'runtime bundle archive must not be empty'
normalized_paths="$temporary_directory/normalized-paths"
: >"$normalized_paths"
while IFS= read -r member; do
  case "$member" in
    . | ./) continue ;;
    ./*) normalized=${member#./} ;;
    *) normalized=$member ;;
  esac
  case "$normalized" in
    '' | /* | ../* | *'/../'* | *'/./'* | *'//'*)
      fail "archive contains an unsafe member path: $member"
      ;;
    *'\'*) fail "archive contains an unsafe member path: $member" ;;
  esac
  clean_normalized=$(LC_ALL=C printf %s "$normalized" | tr -d '\n\r\t')
  [ "$clean_normalized" = "$normalized" ] \
    || fail "archive contains an unsafe member path: $member"
  normalized=${normalized%/}
  case "$normalized" in
    Dockerfile | compose.yaml | BUNDLE-METADATA | SHA256SUMS | \
    bin | bin/qingyu-kernel | \
    scripts | scripts/entrypoint.sh | scripts/verify-final-web-assets.sh | \
    scripts/verify-runtime-bundle.sh | web | web/*) ;;
    *) fail "bundle archive contains unexpected member: $normalized" ;;
  esac
  printf '%s\n' "$normalized" >>"$normalized_paths"
done <"$archive_paths"
[ -z "$(LC_ALL=C sort "$normalized_paths" | uniq -d)" ] \
  || fail 'bundle archive must not contain duplicate member paths'

tar -tvzf "$archive" >"$archive_types" \
  || fail 'runtime bundle archive metadata is unreadable'
while IFS= read -r listing; do
  type=$(printf '%.1s' "$listing")
  case "$type" in
    - | d) ;;
    *) fail 'archive may contain only regular files and directories' ;;
  esac
done <"$archive_types"

bundle_root="$temporary_directory/root"
mkdir "$bundle_root"
tar -xpzf "$archive" -C "$bundle_root" --no-same-owner \
  || fail 'runtime bundle archive could not be extracted'
verify_tree "$bundle_root"

if [ -n "$extract_to" ]; then
  mv "$bundle_root" "$extract_to"
fi
printf 'PASS: verified linux/%s runtime-only bundle (%s compressed bytes).\n' \
  "$architecture" "$archive_bytes"
