#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
dockerfile="$repo_root/Dockerfile"
compose_file="$repo_root/deploy/docker/compose.contract.yaml"
entrypoint="$repo_root/deploy/docker/entrypoint.sh"
runtime_gate="$repo_root/deploy/docker/verify-runtime.sh"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

require_file() {
  [ -f "$1" ] || fail "missing $2: $1"
}

require_fixed() {
  grep -F -- "$2" "$1" >/dev/null || fail "$3"
}

require_extended() {
  grep -E -- "$2" "$1" >/dev/null || fail "$3"
}

reject_extended() {
  if grep -E -- "$2" "$1" >/dev/null; then
    fail "$3"
  fi
}

reject_extended_case_insensitive() {
  if grep -Ei -- "$2" "$1" >/dev/null; then
    fail "$3"
  fi
}

count_service_list_items() {
  awk -v key="$1" '
    $0 ~ "^    " key ":[[:space:]]*$" { in_list = 1; next }
    in_list && ($0 ~ "^    [A-Za-z_][A-Za-z0-9_-]*:" || $0 ~ "^[^ ]") {
      in_list = 0
    }
    in_list && $0 ~ "^      -" { count += 1 }
    END { print count + 0 }
  ' "$compose_file"
}

require_file "$dockerfile" "root Dockerfile"
require_file "$compose_file" "Compose contract"
require_file "$entrypoint" "container entrypoint"
require_file "$runtime_gate" "runtime phase gate"

require_extended "$dockerfile" \
  '^FROM[[:space:]]+node:[^[:space:]]+[[:space:]]+AS[[:space:]]+web-build[[:space:]]*$' \
  "Dockerfile must build the browser application in a Node stage"
require_fixed "$dockerfile" \
  'corepack prepare pnpm@10.30.3 --activate' \
  "web build stage must activate the repository pnpm version"
require_fixed "$dockerfile" \
  'pnpm install --frozen-lockfile' \
  "web build stage must install from the frozen pnpm lockfile"
require_fixed "$dockerfile" \
  'pnpm --filter @markra/web build' \
  "web build stage must build apps/web"

require_extended "$dockerfile" \
  '^FROM[[:space:]]+rust:[^[:space:]]+[[:space:]]+AS[[:space:]]+kernel-build[[:space:]]*$' \
  "Dockerfile must contain a Rust Kernel build stage"
require_fixed "$dockerfile" \
  'cargo build --locked --release --manifest-path apps/kernel/Cargo.toml --bin qingyu-kernel' \
  "Dockerfile must build the locked release qingyu-kernel binary"

require_extended "$dockerfile" \
  '^FROM[[:space:]]+debian:[^[:space:]]+[[:space:]]+AS[[:space:]]+qingyu-runtime[[:space:]]*$' \
  "Dockerfile must contain the minimal qingyu-runtime stage"
final_from=$(awk 'toupper($1) == "FROM" { line = $0 } END { print line }' "$dockerfile")
printf '%s\n' "$final_from" \
  | grep -E '^FROM[[:space:]]+debian:[^[:space:]]+[[:space:]]+AS[[:space:]]+qingyu-runtime[[:space:]]*$' >/dev/null \
  || fail "qingyu-runtime must be the final image stage"
require_extended "$dockerfile" \
  '^COPY[[:space:]].*--from=kernel-build.*qingyu-kernel[[:space:]]+/usr/local/bin/qingyu-kernel[[:space:]]*$' \
  "runtime image must install the release Kernel binary"
require_extended "$dockerfile" \
  '^COPY[[:space:]].*--from=web-build.*apps/web/dist[[:space:]]+/opt/qingyu/web[[:space:]]*$' \
  "runtime image must carry the built Web assets at /opt/qingyu/web"
require_fixed "$dockerfile" 'USER 10001:10001' \
  "runtime image must use fixed non-root UID/GID 10001"
require_fixed "$dockerfile" 'WORKDIR /data' \
  "runtime image must use fixed /data as its working directory"
require_extended "$dockerfile" '^[[:space:]]*EXPOSE[[:space:]]+3210[[:space:]]*$' \
  "runtime image must expose only Kernel port 3210"
exposed_ports=$(grep -Ec '^[[:space:]]*EXPOSE[[:space:]]+' "$dockerfile" || true)
[ "$exposed_ports" -eq 1 ] || fail "Dockerfile must contain exactly one EXPOSE instruction"
require_extended "$dockerfile" \
  '^[[:space:]]*ENTRYPOINT[[:space:]]+\["/usr/local/bin/qingyu-server-entrypoint"\][[:space:]]*$' \
  "runtime image must own the fixed server entrypoint"
entrypoint_instructions=$(grep -Eic '^[[:space:]]*ENTRYPOINT[[:space:]]+' "$dockerfile" || true)
[ "$entrypoint_instructions" -eq 1 ] \
  || fail "Dockerfile must contain exactly one ENTRYPOINT instruction"
reject_extended_case_insensitive "$dockerfile" \
  '^[[:space:]]*CMD[[:space:]]' \
  "runtime image must not add an alternate Node, Vite, or shell command"
reject_extended "$dockerfile" \
  'QINGYU_SERVER_INITIALIZATION_TOKEN|QINGYU_PUBLIC_ORIGIN|QINGYU_(DATA|WORKSPACE|CONFIG|STATE|LOGS|CACHE)_DIR' \
  "runtime inputs and data-root overrides must not enter the image build"

require_fixed "$entrypoint" \
  ': "${QINGYU_PUBLIC_ORIGIN:?QINGYU_PUBLIC_ORIGIN is required}"' \
  "entrypoint must require an explicit public origin"
require_fixed "$entrypoint" \
  'exec /usr/local/bin/qingyu-kernel server --public-origin "$QINGYU_PUBLIC_ORIGIN"' \
  "entrypoint must pass the exact public origin to the Kernel server CLI"
entrypoint_program=$(sed -e '/^[[:space:]]*$/d' -e '/^[[:space:]]*#/d' "$entrypoint")
expected_entrypoint_program='set -eu
: "${QINGYU_PUBLIC_ORIGIN:?QINGYU_PUBLIC_ORIGIN is required}"
exec /usr/local/bin/qingyu-kernel server --public-origin "$QINGYU_PUBLIC_ORIGIN"'
[ "$entrypoint_program" = "$expected_entrypoint_program" ] \
  || fail "entrypoint may only validate the public origin and exec the Kernel server CLI"
reject_extended "$entrypoint" \
  '(^|[[:space:]])(--data-root|--workspace|--config|--port)([[:space:]]|$)' \
  "entrypoint must not expose a data-root or port override"
reject_extended_case_insensitive "$entrypoint" \
  'node|vite|npm|pnpm' \
  "entrypoint must not launch a second Web runtime process"

require_fixed "$compose_file" 'profiles: ["static-web-serving-required"]' \
  "Compose must remain behind the static Web serving phase gate"
require_fixed "$compose_file" 'target: qingyu-runtime' \
  "Compose must build the final qingyu-runtime stage"
require_fixed "$compose_file" 'user: "10001:10001"' \
  "Compose must enforce fixed non-root UID/GID 10001"
user_lines=$(grep -Ec '^[[:space:]]+user:[[:space:]]*' "$compose_file" || true)
[ "$user_lines" -eq 1 ] || fail "Compose must contain exactly one user declaration"
require_fixed "$compose_file" 'read_only: true' \
  "Compose root filesystem must be read-only"
read_only_lines=$(grep -Ec '^[[:space:]]+read_only:[[:space:]]*' "$compose_file" || true)
[ "$read_only_lines" -eq 1 ] || fail "Compose must contain exactly one read_only declaration"
require_fixed "$compose_file" '- ALL' \
  "Compose must drop all Linux capabilities"
capability_drop_items=$(count_service_list_items cap_drop)
[ "$capability_drop_items" -eq 1 ] || fail "Compose cap_drop must contain only ALL"
require_fixed "$compose_file" '- no-new-privileges:true' \
  "Compose must enable no-new-privileges"
security_option_items=$(count_service_list_items security_opt)
[ "$security_option_items" -eq 1 ] \
  || fail "Compose security_opt must contain only no-new-privileges:true"
reject_extended_case_insensitive "$compose_file" \
  '^[[:space:]]*(privileged:[[:space:]]*true|cap_add:|network_mode:[[:space:]]*host|pid:[[:space:]]*host|ipc:[[:space:]]*host|devices:)' \
  "Compose must not add a privileged host boundary"
require_fixed "$compose_file" '- QINGYU_PUBLIC_ORIGIN' \
  "Compose must pass the required public origin without embedding a value"
require_fixed "$compose_file" '- QINGYU_SERVER_INITIALIZATION_TOKEN' \
  "Compose must pass the optional one-time token without embedding a value"
public_origin_lines=$(grep -Ec '^[[:space:]]*-[[:space:]]+QINGYU_PUBLIC_ORIGIN[[:space:]]*$' "$compose_file" || true)
[ "$public_origin_lines" -eq 1 ] \
  || fail "Compose must contain exactly one value-free QINGYU_PUBLIC_ORIGIN pass-through"
token_lines=$(grep -Ec '^[[:space:]]*-[[:space:]]+QINGYU_SERVER_INITIALIZATION_TOKEN[[:space:]]*$' "$compose_file" || true)
[ "$token_lines" -eq 1 ] \
  || fail "Compose must contain exactly one value-free initialization-token pass-through"
environment_items=$(count_service_list_items environment)
[ "$environment_items" -eq 2 ] \
  || fail "Compose environment must contain only the public origin and optional initialization token"
reject_extended "$compose_file" \
  'QINGYU_(PUBLIC_ORIGIN|SERVER_INITIALIZATION_TOKEN)[[:space:]]*[:=]' \
  "Compose must not contain a public-origin or initialization-token value/default"
require_fixed "$compose_file" '- "127.0.0.1:3210:3210"' \
  "Compose must publish only Kernel port 3210 to reverse proxies on loopback"
published_ports=$(count_service_list_items ports)
[ "$published_ports" -eq 1 ] || fail "Compose must publish exactly one container port"
reject_extended_case_insensitive "$compose_file" '^[[:space:]]*expose:' \
  "Compose must not declare additional ports"
require_fixed "$compose_file" '- qingyu-data:/data' \
  "Compose must mount its single persistent volume at fixed /data"
data_mounts=$(grep -Ec '^[[:space:]]*-[[:space:]]+[^#[:space:]]+:/data[[:space:]]*$' "$compose_file" || true)
[ "$data_mounts" -eq 1 ] || fail "Compose must contain exactly one persistent /data mount"
volume_mounts=$(count_service_list_items volumes)
[ "$volume_mounts" -eq 1 ] || fail "Compose must contain only the persistent /data mount"
require_fixed "$compose_file" \
  '/tmp/qingyu:rw,noexec,nosuid,nodev,size=64m,uid=10001,gid=10001,mode=0700' \
  "Compose must provide an owned, disposable /tmp/qingyu tmpfs"
tmpfs_mounts=$(count_service_list_items tmpfs)
[ "$tmpfs_mounts" -eq 1 ] || fail "Compose must contain only the /tmp/qingyu tmpfs"
reject_extended "$compose_file" \
  'QINGYU_(DATA|WORKSPACE|CONFIG|STATE|LOGS|CACHE)_DIR|--data-root|--workspace|--config' \
  "Compose must not expose a configurable server data layout"
reject_extended_case_insensitive "$compose_file" \
  '^[[:space:]]*(command|entrypoint):' \
  "Compose must use the image-owned Kernel entrypoint"

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

set +e
runtime_output=$("$runtime_gate" --status 2>&1)
runtime_status=$?
set -e
[ "$runtime_status" -eq 78 ] \
  || fail "runtime phase gate must exit 78 while static Web serving is unavailable"
case "$runtime_output" in
  *static-web-serving-required*) ;;
  *) fail "runtime phase gate must identify static-web-serving-required" ;;
esac

printf '%s\n' \
  'PASS: Docker packaging is statically valid; BLOCKED(static-web-serving-required) remains.'
