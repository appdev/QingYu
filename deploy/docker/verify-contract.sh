#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
dockerfile="$repo_root/Dockerfile"
compose_file="$repo_root/deploy/docker/compose.contract.yaml"
runtime_gate="$repo_root/deploy/docker/verify-runtime.sh"
contract_doc="$repo_root/deploy/docker/README.md"

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

reject_extended() {
  if grep -E -- "$2" "$1" >/dev/null; then
    fail "$3"
  fi
}

require_file "$dockerfile" "root Dockerfile"
require_file "$compose_file" "Compose contract"
require_file "$runtime_gate" "runtime phase gate"
require_file "$contract_doc" "Docker contract documentation"

grep -Ei '^FROM[[:space:]].+[[:space:]]AS[[:space:]]kernel-build[[:space:]]*$' "$dockerfile" >/dev/null \
  || fail "Dockerfile must contain a kernel-build stage"
require_fixed "$dockerfile" \
  'cargo build --locked --release --manifest-path apps/kernel/Cargo.toml --bin qingyu-kernel' \
  "Dockerfile must build the locked release qingyu-kernel artifact"
grep -Ei '^FROM[[:space:]]scratch[[:space:]]AS[[:space:]]kernel-artifact[[:space:]]*$' "$dockerfile" >/dev/null \
  || fail "Dockerfile must end in a non-runnable scratch artifact stage"
require_fixed "$dockerfile" \
  'COPY --from=kernel-build /src/apps/kernel/target/release/qingyu-kernel /qingyu-kernel' \
  "Dockerfile must export the kernel binary from the artifact stage"
reject_extended "$dockerfile" '^[[:space:]]*(CMD|ENTRYPOINT|EXPOSE|HEALTHCHECK)[[:space:]]' \
  "artifact-only Dockerfile must not claim a runnable server contract"
reject_extended "$dockerfile" 'QINGYU_INIT_TOKEN' \
  "the one-time initialization token must never enter the image build"

require_fixed "$compose_file" 'profiles: ["server-entrypoint-required"]' \
  "Compose service must remain behind the server-entrypoint phase gate"
require_fixed "$compose_file" 'image: ${QINGYU_SERVER_IMAGE:?P2 server image required}' \
  "Compose must require an explicit P2 server image"
require_fixed "$compose_file" 'user: "10001:10001"' \
  "Compose must run the future server as a fixed non-root UID/GID"
require_fixed "$compose_file" 'read_only: true' \
  "Compose root filesystem must be read-only"
require_fixed "$compose_file" '- QINGYU_INIT_TOKEN' \
  "Compose must pass the optional one-time token through from the environment"
token_lines=$(grep -Ec '^[[:space:]]*-[[:space:]]+QINGYU_INIT_TOKEN[[:space:]]*$' "$compose_file" || true)
[ "$token_lines" -eq 1 ] \
  || fail "Compose must contain exactly one value-free QINGYU_INIT_TOKEN pass-through"
reject_extended "$compose_file" 'QINGYU_INIT_TOKEN[[:space:]]*[:=]' \
  "Compose must not contain an initialization-token value or default"
require_fixed "$compose_file" '- "${QINGYU_SERVER_PORT:-3210}:3210"' \
  "Compose must publish only the fixed container server port 3210"
published_ports=$(grep -Ec '^[[:space:]]*-[[:space:]]+"[^\"]*:[0-9]+"[[:space:]]*$' "$compose_file" || true)
[ "$published_ports" -eq 1 ] \
  || fail "Compose must publish exactly one container port"
reject_extended "$compose_file" '^[[:space:]]*expose:' \
  "Compose must not declare additional exposed ports"
require_fixed "$compose_file" '- qingyu-data:/data' \
  "Compose must mount its single persistent volume at fixed /data"
data_mounts=$(grep -Ec '^[[:space:]]*-[[:space:]]+[^#[:space:]]+:/data[[:space:]]*$' "$compose_file" || true)
[ "$data_mounts" -eq 1 ] \
  || fail "Compose must contain exactly one persistent mount targeting /data"
reject_extended "$compose_file" 'QINGYU_(DATA|WORKSPACE|CONFIG|STATE|LOGS)_DIR' \
  "Compose must not make the container data layout product-configurable"
require_fixed "$compose_file" 'restart: unless-stopped' \
  "Compose must define the restart contract"
require_fixed "$compose_file" 'disable: true' \
  "healthcheck must remain disabled until the real server entrypoint exists"
require_fixed "$compose_file" 'dev.qingyu.contract.health-live: /api/v1/health/live' \
  "Compose must record the live-health endpoint contract"
require_fixed "$compose_file" 'dev.qingyu.contract.health-ready: /api/v1/health/ready' \
  "Compose must record the ready-health endpoint contract"
reject_extended "$compose_file" '^[[:space:]]*(command|entrypoint|build):' \
  "Compose must not invent a command, entrypoint, or runnable image build"

set +e
runtime_output=$("$runtime_gate" --status 2>&1)
runtime_status=$?
set -e
[ "$runtime_status" -eq 78 ] \
  || fail "runtime phase gate must exit 78 while the P2 server entrypoint is unavailable"
case "$runtime_output" in
  *server-entrypoint-required*) ;;
  *) fail "runtime phase gate must identify the server-entrypoint-required blocker" ;;
esac

printf 'PASS: Docker artifact and Compose contracts are statically valid.\n'
