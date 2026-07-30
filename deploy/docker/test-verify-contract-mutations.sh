#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
verifier="$repo_root/deploy/docker/verify-contract.sh"
compose_file="$repo_root/deploy/docker/compose.contract.yaml"
runtime_compose_file="$repo_root/deploy/docker/runtime-bundle.compose.yaml"
dockerfile="$repo_root/Dockerfile"
web_dist_tests="$repo_root/deploy/docker/verify-web-dist.test.mjs"
runtime_gate="$repo_root/deploy/docker/verify-runtime.sh"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/qingyu-docker-contract-mutations.XXXXXX")
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM

tracked_inputs_manifest="$temporary_directory/tracked-build-inputs.current.txt"
git -C "$repo_root" ls-files -z -- \
  .dockerignore Dockerfile package.json pnpm-lock.yaml pnpm-workspace.yaml \
  apps/web/package.json packages/app/package.json packages/editor/package.json \
  packages/editor-react/package.json packages/kernel-client/package.json \
  packages/markdown/package.json packages/scripts/package.json packages/shared/package.json \
  packages/ui/package.json deploy/docker/verify-web-dist.mjs apps/web packages \
  apps/kernel/Cargo.toml apps/kernel/Cargo.lock \
  apps/kernel/crates/qingyu-dejavu/Cargo.toml apps/kernel/crates/qingyu-dejavu/src \
  apps/kernel/src deploy/docker/entrypoint.sh deploy/docker/verify-final-web-assets.sh \
  | tr '\0' '\n' | LC_ALL=C sort >"$tracked_inputs_manifest"
[ -s "$tracked_inputs_manifest" ] \
  || fail "could not derive current tracked Docker build inputs for mutation isolation"
export QINGYU_VERIFY_TRACKED_INPUTS_MANIFEST="$tracked_inputs_manifest"

node --test "$web_dist_tests"

runtime_output_file="$temporary_directory/runtime-status.output"
if ! "$runtime_gate" --status >"$runtime_output_file" 2>&1; then
  fail "runtime phase status must pass after the Web KernelClient cutover"
fi
grep -F 'READY(runtime-ready)' "$runtime_output_file" >/dev/null \
  || fail "runtime phase status must report runtime-ready"
grep -F 'PENDING(final-live-linux-acceptance)' "$runtime_output_file" >/dev/null \
  || fail "runtime phase status must preserve the final live Linux acceptance boundary"

expect_rejection() {
  mutation_name=$1
  expected_message=$2
  shift 2

  output_file="$temporary_directory/$mutation_name.output"
  if "$@" >"$output_file" 2>&1; then
    fail "$mutation_name unexpectedly passed contract verification"
  fi

  grep -F -- "$expected_message" "$output_file" >/dev/null \
    || fail "$mutation_name failed without the expected semantic diagnostic"
}

custom_frontend_dockerfile="$temporary_directory/Dockerfile.custom-frontend"
ruby -e '
  lines = File.readlines(ARGV.fetch(0))
  abort "Dockerfile syntax directive not found" unless lines.first&.start_with?("# syntax=")
  lines[0] = "# syntax=attacker.invalid/custom-frontend:latest\n"
  File.write(ARGV.fetch(1), lines.join)
' "$dockerfile" "$custom_frontend_dockerfile"

expect_rejection \
  dockerfile-custom-frontend \
  'Dockerfile must declare only # syntax=docker/dockerfile:1.7' \
  env QINGYU_VERIFY_DOCKERFILE="$custom_frontend_dockerfile" "$verifier"

extra_parser_directive_dockerfile="$temporary_directory/Dockerfile.extra-parser-directive"
ruby -e '
  lines = File.readlines(ARGV.fetch(0))
  abort "Dockerfile syntax directive not found" unless lines.first&.start_with?("# syntax=")
  lines.insert(1, "# check=skip=all\n")
  File.write(ARGV.fetch(1), lines.join)
' "$dockerfile" "$extra_parser_directive_dockerfile"

expect_rejection \
  dockerfile-extra-parser-directive \
  'Dockerfile must declare only # syntax=docker/dockerfile:1.7' \
  env QINGYU_VERIFY_DOCKERFILE="$extra_parser_directive_dockerfile" "$verifier"

mutated_dockerfile="$temporary_directory/Dockerfile.nodejs-in-final-stage"
awk '
  /^USER 10001:10001$/ && !inserted {
    print "RUN apt-get update && apt-get install --yes nodejs"
    inserted = 1
  }
  { print }
  END {
    if (!inserted) {
      exit 2
    }
  }
' "$dockerfile" >"$mutated_dockerfile" \
  || fail "could not create the final-stage Node mutation"

expect_rejection \
  final-stage-nodejs \
  'final Dockerfile stage must not install or copy Node toolchains' \
  env QINGYU_VERIFY_DOCKERFILE="$mutated_dockerfile" "$verifier"

extra_stage_dockerfile="$temporary_directory/Dockerfile.extra-stage"
ruby -e '
  source = File.read(ARGV.fetch(0))
  runtime = "FROM debian:bookworm-slim AS qingyu-runtime\n"
  abort "runtime stage not found" unless source.include?(runtime)
  extra = "FROM alpine:3 AS unrelated-stage\n\n"
  File.write(ARGV.fetch(1), source.sub(runtime, extra + runtime))
' "$dockerfile" "$extra_stage_dockerfile"

expect_rejection \
  dockerfile-extra-stage \
  'Dockerfile must contain exactly the frozen web-build, kernel-build, and qingyu-runtime stages' \
  env QINGYU_VERIFY_DOCKERFILE="$extra_stage_dockerfile" "$verifier"

mutated_compose="$temporary_directory/compose.read-only-comment-spoof.yaml"
sed 's/read_only: true/read_only: false # read_only: true/' \
  "$compose_file" >"$mutated_compose"
cmp -s "$compose_file" "$mutated_compose" \
  && fail "could not create the Compose read_only mutation"

expect_rejection \
  compose-read-only-comment-spoof \
  'Compose read_only must be the YAML boolean true' \
  env QINGYU_VERIFY_COMPOSE_FILE="$mutated_compose" "$verifier"

expect_runtime_compose_rejection() {
  mutation_name=$1
  expected_message=$2
  mutation=$3
  mutated_runtime_compose="$temporary_directory/runtime-compose.$mutation_name.yaml"
  ruby -ryaml -e '
    compose = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: false)
    service = compose.fetch("services").fetch("qingyu")
    case ARGV.fetch(2)
    when "build"
      service["build"] = { "context" => "." }
    when "image-default"
      service["image"] = "${QINGYU_SERVER_IMAGE:-qingyu-server:local}"
    when "literal-token"
      service["environment"] = [
        "QINGYU_PUBLIC_ORIGIN=https://notes.example.com",
        "QINGYU_SERVER_INITIALIZATION_TOKEN=secret"
      ]
    when "writable-root"
      service["read_only"] = false
    when "wrong-user"
      service["user"] = "0:0"
    when "capability"
      service["cap_drop"] = []
    when "privilege"
      service["security_opt"] = []
    when "public-port"
      service["ports"] = ["3210:3210"]
    when "host-data"
      service["volumes"] = ["./data:/data"]
    when "weak-tmpfs"
      service["tmpfs"] = ["/tmp/qingyu"]
    when "short-stop"
      service["stop_grace_period"] = "5s"
    when "no-restart"
      service["restart"] = "no"
    else
      abort "unknown runtime Compose mutation"
    end
    File.write(ARGV.fetch(1), YAML.dump(compose))
  ' "$runtime_compose_file" "$mutated_runtime_compose" "$mutation"
  expect_rejection \
    "runtime-compose-$mutation_name" \
    "$expected_message" \
    env QINGYU_VERIFY_RUNTIME_COMPOSE_FILE="$mutated_runtime_compose" "$verifier"
}

expect_runtime_compose_rejection source-build \
  'Runtime Compose must not contain build or source configuration' build
expect_runtime_compose_rejection optional-image \
  'Runtime Compose image must require an explicit prebuilt image reference' image-default
expect_runtime_compose_rejection literal-runtime-inputs \
  'Runtime Compose environment must contain only value-free runtime inputs' literal-token
expect_runtime_compose_rejection writable-root \
  'Runtime Compose read_only must be the YAML boolean true' writable-root
expect_runtime_compose_rejection wrong-user \
  'Runtime Compose user must be exactly 10001:10001' wrong-user
expect_runtime_compose_rejection retained-capability \
  'Runtime Compose cap_drop must contain only ALL' capability
expect_runtime_compose_rejection privilege-escalation \
  'Runtime Compose security_opt must contain only no-new-privileges:true' privilege
expect_runtime_compose_rejection public-port \
  'Runtime Compose must publish only Kernel port 3210 on loopback' public-port
expect_runtime_compose_rejection host-data \
  'Runtime Compose must mount only qingyu-data at fixed /data' host-data
expect_runtime_compose_rejection weak-tmpfs \
  'Runtime Compose must provide only the hardened /tmp/qingyu tmpfs' weak-tmpfs
expect_runtime_compose_rejection short-stop \
  'Runtime Compose stop_grace_period must be exactly 35s' short-stop
expect_runtime_compose_rejection no-restart \
  'Runtime Compose restart policy must be unless-stopped' no-restart

weakened_dockerignore="$temporary_directory/dockerignore.missing-nested-env"
awk '$0 != "**/.env.*" { print }' "$repo_root/.dockerignore" >"$weakened_dockerignore"

expect_rejection \
  dockerignore-missing-nested-env \
  '.dockerignore is missing mandatory exclusions: **/.env.*' \
  env QINGYU_VERIFY_DOCKERIGNORE="$weakened_dockerignore" "$verifier"

overbroad_dockerignore="$temporary_directory/dockerignore.excludes-packages"
{
  sed -n '1,$p' "$repo_root/.dockerignore"
  printf '%s\n' 'packages'
} >"$overbroad_dockerignore"

expect_rejection \
  dockerignore-excludes-build-source \
  '.dockerignore excludes tracked Docker build input: packages/app/package.json' \
  env QINGYU_VERIFY_DOCKERIGNORE="$overbroad_dockerignore" "$verifier"

negated_dockerignore="$temporary_directory/dockerignore.reincludes-root-env"
{
  sed -n '1,$p' "$repo_root/.dockerignore"
  printf '%s\n' '!.env'
} >"$negated_dockerignore"

expect_rejection \
  dockerignore-reincludes-root-env \
  'Docker build context policy must match the frozen .dockerignore contract' \
  env QINGYU_VERIFY_DOCKERIGNORE="$negated_dockerignore" "$verifier"

tracked_input_dockerignore="$temporary_directory/dockerignore.excludes-tracked-ui-source"
{
  sed -n '1,$p' "$repo_root/.dockerignore"
  printf '%s\n' 'packages/ui/src/**'
} >"$tracked_input_dockerignore"

expect_rejection \
  dockerignore-excludes-tracked-ui-source \
  '.dockerignore excludes tracked Docker build input: packages/ui/src/Badge.test.tsx' \
  env QINGYU_VERIFY_DOCKERIGNORE="$tracked_input_dockerignore" "$verifier"

stale_tracked_inputs_manifest="$temporary_directory/tracked-build-inputs.stale.txt"
awk '$0 != "packages/ui/src/Badge.test.tsx" { print }' \
  "$tracked_inputs_manifest" >"$stale_tracked_inputs_manifest"

expect_rejection \
  tracked-build-input-fallback-manifest-stale \
  'tracked Docker build input fallback manifest is stale' \
  env QINGYU_VERIFY_TRACKED_INPUTS_MANIFEST="$stale_tracked_inputs_manifest" "$verifier"

shell_spliced_node_dockerfile="$temporary_directory/Dockerfile.shell-spliced-nodejs"
ruby -e '
  lines = File.readlines(ARGV.fetch(0))
  index = lines.index do |line|
    line.include?("apt-get install --yes --no-install-recommends ca-certificates")
  end
  abort "runtime setup instruction not found" unless index
  lines[index] = lines[index].sub("ca-certificates", "ca-certificates no\"\"dejs")
  File.write(ARGV.fetch(1), lines.join)
' "$dockerfile" "$shell_spliced_node_dockerfile"

expect_rejection \
  final-stage-shell-spliced-nodejs \
  'qingyu-runtime instruction sequence must match the frozen contract' \
  env QINGYU_VERIFY_DOCKERFILE="$shell_spliced_node_dockerfile" "$verifier"

smuggled_web_dist_dockerfile="$temporary_directory/Dockerfile.web-dist-node-smuggle"
ruby -e '
  source = File.read(ARGV.fetch(0))
  build = "RUN pnpm --filter @markra/web build\n"
  abort "Web build instruction not found" unless source.include?(build)
  smuggle = "RUN cp /usr/local/bin/node /src/apps/web/dist/.runtime-node\n"
  File.write(ARGV.fetch(1), source.sub(build, build + smuggle))
' "$dockerfile" "$smuggled_web_dist_dockerfile"

expect_rejection \
  web-build-dist-node-smuggle \
  'web-build instruction sequence must match the frozen contract' \
  env QINGYU_VERIFY_DOCKERFILE="$smuggled_web_dist_dockerfile" "$verifier"

missing_final_verifier_dockerfile="$temporary_directory/Dockerfile.missing-final-web-verifier"
awk '$0 != "RUN /usr/local/bin/qingyu-verify-final-web-assets /opt/qingyu/web" { print }' \
  "$dockerfile" >"$missing_final_verifier_dockerfile"

expect_rejection \
  runtime-missing-final-web-verification \
  'runtime image must contain only base-system setup and final Web asset verification' \
  env QINGYU_VERIFY_DOCKERFILE="$missing_final_verifier_dockerfile" "$verifier"

post_verification_write_dockerfile="$temporary_directory/Dockerfile.post-verification-write"
ruby -e '
  source = File.read(ARGV.fetch(0))
  verification = "RUN /usr/local/bin/qingyu-verify-final-web-assets /opt/qingyu/web\n"
  abort "Final Web verification instruction not found" unless source.include?(verification)
  mutation = "RUN printf unsafe > /opt/qingyu/web/assets/late.js\n"
  File.write(ARGV.fetch(1), source.sub(verification, verification + mutation))
' "$dockerfile" "$post_verification_write_dockerfile"

expect_rejection \
  runtime-post-verification-web-write \
  'runtime image must contain only base-system setup and final Web asset verification' \
  env QINGYU_VERIFY_DOCKERFILE="$post_verification_write_dockerfile" "$verifier"

printf '%s\n' 'PASS: Docker contract semantic mutations are rejected.'
