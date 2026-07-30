#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
verifier="$repo_root/deploy/docker/verify-contract.sh"
compose_file="$repo_root/deploy/docker/compose.contract.yaml"
dockerfile="$repo_root/Dockerfile"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/qingyu-docker-contract-mutations.XXXXXX")
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM

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

printf '%s\n' 'PASS: Docker contract semantic mutations are rejected.'
