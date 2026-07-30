#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
semantic_verifier="$repo_root/deploy/docker/verify-contract.rb"
entrypoint="$repo_root/deploy/docker/entrypoint.sh"
final_web_verifier="$repo_root/deploy/docker/verify-final-web-assets.sh"
runtime_gate="$repo_root/deploy/docker/verify-runtime.sh"
compose_file=${QINGYU_VERIFY_COMPOSE_FILE:-"$repo_root/deploy/docker/compose.contract.yaml"}

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

require_file() {
  [ -f "$1" ] || fail "missing $2: $1"
}

require_file "$semantic_verifier" "semantic contract verifier"
require_file "$entrypoint" "container entrypoint"
require_file "$final_web_verifier" "final Web asset verifier"
require_file "$runtime_gate" "runtime phase gate"

ruby "$semantic_verifier"

entrypoint_program=$(sed -e '/^[[:space:]]*$/d' -e '/^[[:space:]]*#/d' "$entrypoint")
expected_entrypoint_program='set -eu
: "${QINGYU_PUBLIC_ORIGIN:?QINGYU_PUBLIC_ORIGIN is required}"
exec /usr/local/bin/qingyu-kernel server --public-origin "$QINGYU_PUBLIC_ORIGIN"'
[ "$entrypoint_program" = "$expected_entrypoint_program" ] \
  || fail "entrypoint may only validate the public origin and exec the Kernel server CLI"

set +e
missing_origin_output=$(env QINGYU_PUBLIC_ORIGIN= "$entrypoint" 2>&1)
missing_origin_status=$?
set -e
[ "$missing_origin_status" -ne 0 ] \
  || fail "entrypoint must fail closed when the public origin is empty"
case "$missing_origin_output" in
  *QINGYU_PUBLIC_ORIGIN*required*) ;;
  *) fail "entrypoint must explain that QINGYU_PUBLIC_ORIGIN is required" ;;
esac

runtime_output=$("$runtime_gate" --status 2>&1) \
  || fail "runtime phase status must pass after the Web KernelClient cutover"
case "$runtime_output" in
  *READY\(runtime-ready\)*PENDING\(final-live-linux-acceptance\)*) ;;
  *) fail "runtime phase status must report ready while preserving pending live Linux acceptance" ;;
esac

inspect_built_image() {
  temporary_directory=$1
  image_id_file="$temporary_directory/image-id"

  docker build \
    --target qingyu-runtime \
    --iidfile "$image_id_file" \
    "$repo_root" >/dev/null
  image_id=$(cat "$image_id_file")
  [ -n "$image_id" ] || fail "Docker build did not return a final image ID"

  image_user=$(docker image inspect --format '{{.Config.User}}' "$image_id")
  [ "$image_user" = "10001:10001" ] \
    || fail "built image must run as UID/GID 10001:10001"
  image_entrypoint=$(docker image inspect --format '{{json .Config.Entrypoint}}' "$image_id")
  [ "$image_entrypoint" = '["/usr/local/bin/qingyu-server-entrypoint"]' ] \
    || fail "built image must retain the fixed server entrypoint"

  docker run --rm --entrypoint /bin/sh "$image_id" -c '
    set -eu
    test -x /usr/local/bin/qingyu-kernel
    test -f /opt/qingyu/web/index.html
    test -x /usr/local/bin/qingyu-verify-final-web-assets
    /usr/local/bin/qingyu-verify-final-web-assets /opt/qingyu/web
    for executable in node nodejs npm pnpm yarn bun corepack; do
      if command -v "$executable" >/dev/null 2>&1; then
        printf "unexpected Node toolchain executable: %s\n" "$executable" >&2
        exit 1
      fi
    done
  ' || fail "built final image contains an invalid runtime filesystem"
}

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  if docker compose version >/dev/null 2>&1; then
    docker compose \
      --profile local-source-build \
      -f "$compose_file" \
      config >/dev/null \
      || fail "Docker Compose rejected the semantic contract"
  fi

  docker_temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/qingyu-docker-image-verification.XXXXXX")
  trap 'rm -rf "$docker_temporary_directory"' EXIT HUP INT TERM
  inspect_built_image "$docker_temporary_directory"
  docker_evidence='Docker final image built and inspected; '
else
  docker_evidence='Docker runtime unavailable, so final-image evidence is pending; '
fi

printf '%s\n' \
  "PASS: ${docker_evidence}runtime packaging is ready; final live Linux acceptance remains pending."
