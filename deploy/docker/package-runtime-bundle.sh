#!/bin/sh

set -eu

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

usage() {
  fail 'usage: package-runtime-bundle.sh --architecture <amd64|arm64> --kernel <ELF> --web-dist <directory> --output <archive.tar.gz>'
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

verify_elf() {
  target=$1
  architecture=$2
  [ -f "$target" ] && [ ! -L "$target" ] \
    || fail 'Kernel input must be one retained regular file, not a symbolic link'
  [ "$(link_count "$target")" -eq 1 ] \
    || fail 'Kernel input must not be a hard link'
  [ -x "$target" ] || fail 'Kernel input must have executable mode bits'
  size=$(wc -c <"$target" | tr -d '[:space:]')
  [ "$size" -ge 64 ] \
    || fail 'Kernel input must be a 64-bit little-endian ELF executable'
  header=$(od -An -tx1 -N6 "$target" | tr -d '[:space:]')
  [ "$header" = '7f454c460201' ] \
    || fail 'Kernel input must be a 64-bit little-endian ELF executable'
  os_abi=$(od -An -tu1 -j7 -N1 "$target" | tr -d '[:space:]')
  case "$os_abi" in
    0 | 3) ;;
    *) fail 'Kernel input must be a Linux-compatible ELF executable' ;;
  esac
  set -- $(od -An -tu1 -j16 -N2 "$target")
  [ "$#" -eq 2 ] && { [ "$1" -eq 2 ] || [ "$1" -eq 3 ]; } && [ "$2" -eq 0 ] \
    || fail 'Kernel input must be an executable or position-independent ELF binary'
  set -- $(od -An -tu1 -j18 -N2 "$target")
  [ "$#" -eq 2 ] \
    || fail 'Kernel input must be a 64-bit little-endian ELF executable'
  machine=$(( $1 + ($2 * 256) ))
  case "$architecture:$machine" in
    amd64:62 | arm64:183) ;;
    *) fail "Kernel ELF architecture does not match requested linux/$architecture" ;;
  esac
  set -- $(od -An -tu1 -j20 -N4 "$target")
  [ "$#" -eq 4 ] \
    && [ "$1" -eq 1 ] && [ "$2" -eq 0 ] && [ "$3" -eq 0 ] && [ "$4" -eq 0 ] \
    || fail 'Kernel input has an invalid ELF version'
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

architecture=
kernel=
web_dist=
output=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --architecture)
      [ -z "$architecture" ] && [ "$#" -ge 2 ] || usage
      architecture=$2
      shift 2
      ;;
    --kernel)
      [ -z "$kernel" ] && [ "$#" -ge 2 ] || usage
      kernel=$2
      shift 2
      ;;
    --web-dist)
      [ -z "$web_dist" ] && [ "$#" -ge 2 ] || usage
      web_dist=$2
      shift 2
      ;;
    --output)
      [ -z "$output" ] && [ "$#" -ge 2 ] || usage
      output=$2
      shift 2
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

[ -n "$architecture" ] && [ -n "$kernel" ] && [ -n "$web_dist" ] && [ -n "$output" ] \
  || usage
case "$architecture" in
  amd64 | arm64) ;;
  *) fail 'architecture must be amd64 or arm64' ;;
esac
case "$output" in
  *.tar.gz) ;;
  *) fail 'output must end in .tar.gz' ;;
esac
output_name=$(basename "$output")
case "$output_name" in
  *'\'*) fail 'output filename must not contain control characters or backslashes' ;;
esac
clean_output_name=$(LC_ALL=C printf %s "$output_name" | tr -d '\n\r\t')
[ "$clean_output_name" = "$output_name" ] \
  || fail 'output filename must not contain control characters or backslashes'
[ ! -e "$output" ] && [ ! -L "$output" ] \
  || fail "output archive already exists: $output"
[ ! -e "$output.sha256" ] && [ ! -L "$output.sha256" ] \
  || fail "output checksum already exists: $output.sha256"
[ -d "$web_dist" ] && [ ! -L "$web_dist" ] \
  || fail 'Web distribution root must be a directory, not a symbolic link'

case "$output" in
  */*) output_parent=${output%/*}; [ -n "$output_parent" ] || output_parent=/ ;;
  *) output_parent=. ;;
esac
[ -d "$output_parent" ] && [ ! -L "$output_parent" ] \
  || fail "output parent must be a retained directory: $output_parent"

script_directory=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
dockerfile_template="$script_directory/runtime-bundle.Dockerfile"
compose_template="$script_directory/runtime-bundle.compose.yaml"
entrypoint_template="$script_directory/entrypoint.sh"
web_verifier_template="$script_directory/verify-final-web-assets.sh"
bundle_verifier_template="$script_directory/verify-runtime-bundle.sh"
for template in \
  "$dockerfile_template" \
  "$compose_template" \
  "$entrypoint_template" \
  "$web_verifier_template" \
  "$bundle_verifier_template"; do
  [ -f "$template" ] && [ ! -L "$template" ] \
    || fail "missing retained runtime bundle template: $template"
done

verify_elf "$kernel" "$architecture"
"$web_verifier_template" "$web_dist" >/dev/null
reject_source_like_web_paths "$web_dist"

umask 077
temporary_directory=$(mktemp -d "$output_parent/.qingyu-runtime-bundle.XXXXXX")
temporary_archive=$(mktemp "$output_parent/.qingyu-runtime-archive.XXXXXX")
temporary_checksum=$(mktemp "$output_parent/.qingyu-runtime-checksum.XXXXXX")
cleanup() {
  chmod -R u+w "$temporary_directory" 2>/dev/null || true
  rm -rf "$temporary_directory"
  rm -f "$temporary_archive" "$temporary_checksum"
}
trap cleanup EXIT HUP INT TERM

bundle_root="$temporary_directory/root"
mkdir -p "$bundle_root/bin" "$bundle_root/scripts" "$bundle_root/web"
install -m 0444 "$dockerfile_template" "$bundle_root/Dockerfile"
install -m 0444 "$compose_template" "$bundle_root/compose.yaml"
cat >"$bundle_root/BUNDLE-METADATA" <<EOF
format=qingyu-runtime-bundle-v2
os=linux
architecture=$architecture
EOF
chmod 0444 "$bundle_root/BUNDLE-METADATA"
install -m 0555 "$kernel" "$bundle_root/bin/qingyu-kernel"
install -m 0555 "$entrypoint_template" "$bundle_root/scripts/entrypoint.sh"
install -m 0555 "$web_verifier_template" "$bundle_root/scripts/verify-final-web-assets.sh"
install -m 0555 "$bundle_verifier_template" "$bundle_root/scripts/verify-runtime-bundle.sh"
cp -R "$web_dist/." "$bundle_root/web"
find "$bundle_root/web" -type d -exec chmod 0555 {} +
find "$bundle_root/web" -type f -exec chmod 0444 {} +

verify_elf "$bundle_root/bin/qingyu-kernel" "$architecture"
"$bundle_root/scripts/verify-final-web-assets.sh" "$bundle_root/web" >/dev/null
reject_source_like_web_paths "$bundle_root/web"

manifest_list="$temporary_directory/manifest-list"
(
  CDPATH= cd -- "$bundle_root"
  find . -type f ! -path './SHA256SUMS' -print \
    | sed 's#^\./##' \
    | LC_ALL=C sort
) >"$manifest_list"
: >"$bundle_root/SHA256SUMS"
while IFS= read -r relative_path; do
  printf '%s  %s\n' \
    "$(hash_file "$bundle_root/$relative_path")" \
    "$relative_path" >>"$bundle_root/SHA256SUMS"
done <"$manifest_list"
chmod 0444 "$bundle_root/SHA256SUMS"
find "$bundle_root" -type d -exec chmod 0555 {} +

(
  CDPATH= cd -- "$bundle_root"
  COPYFILE_DISABLE=1 tar -czf "$temporary_archive" \
    Dockerfile compose.yaml BUNDLE-METADATA SHA256SUMS bin scripts web
)

"$bundle_verifier_template" \
  --archive "$temporary_archive" \
  --architecture "$architecture" >/dev/null

archive_digest=$(hash_file "$temporary_archive")
printf '%s  %s\n' "$archive_digest" "$(basename "$output")" >"$temporary_checksum"
chmod 0444 "$temporary_archive" "$temporary_checksum"
mv "$temporary_archive" "$output"
mv "$temporary_checksum" "$output.sha256"
printf 'PASS: packaged verified linux/%s runtime bundle: %s\n' "$architecture" "$output"
printf 'SHA-256: %s\n' "$archive_digest"
