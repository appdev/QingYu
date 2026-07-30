#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
verifier="$repo_root/deploy/docker/verify-contract.sh"
compose_file="$repo_root/deploy/docker/compose.contract.yaml"
dockerfile="$repo_root/Dockerfile"
web_dist_tests="$repo_root/deploy/docker/verify-web-dist.test.mjs"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/qingyu-docker-contract-mutations.XXXXXX")
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM

node --test "$web_dist_tests"

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
  '.dockerignore excludes required Docker build input: packages/app/package.json' \
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
  'Docker build context policy must match the frozen .dockerignore contract' \
  env QINGYU_VERIFY_DOCKERIGNORE="$tracked_input_dockerignore" "$verifier"

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

printf '%s\n' 'PASS: Docker contract semantic mutations are rejected.'
