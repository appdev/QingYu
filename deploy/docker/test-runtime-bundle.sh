#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
packager="$repo_root/deploy/docker/package-runtime-bundle.sh"
verifier="$repo_root/deploy/docker/verify-runtime-bundle.sh"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/qingyu-runtime-bundle-tests.XXXXXX")
cleanup() {
  chmod -R u+w "$temporary_directory" 2>/dev/null || true
  rm -rf "$temporary_directory"
}
trap cleanup EXIT HUP INT TERM

expect_rejection() {
  mutation_name=$1
  expected_message=$2
  shift 2

  output_file="$temporary_directory/$mutation_name.output"
  if "$@" >"$output_file" 2>&1; then
    fail "$mutation_name unexpectedly passed"
  fi
  grep -F -- "$expected_message" "$output_file" >/dev/null \
    || fail "$mutation_name failed without the expected diagnostic: $expected_message"
}

make_elf() {
  output=$1
  architecture=$2
  dd if=/dev/zero of="$output" bs=64 count=1 2>/dev/null
  printf '\177ELF\002\001\001\000' | dd of="$output" bs=1 seek=0 conv=notrunc 2>/dev/null
  printf '\003\000' | dd of="$output" bs=1 seek=16 conv=notrunc 2>/dev/null
  case "$architecture" in
    amd64) printf '\076\000' ;;
    arm64) printf '\267\000' ;;
    *) fail "unsupported test ELF architecture: $architecture" ;;
  esac | dd of="$output" bs=1 seek=18 conv=notrunc 2>/dev/null
  printf '\001\000\000\000' | dd of="$output" bs=1 seek=20 conv=notrunc 2>/dev/null
  chmod 0755 "$output"
}

hash_file() {
  sha256sum "$1" | awk '{ print $1 }'
}

rewrite_manifest() {
  bundle_root=$1
  manifest="$bundle_root/SHA256SUMS"
  list="$temporary_directory/manifest-list"
  (
    CDPATH= cd -- "$bundle_root"
    find . -type f ! -path './SHA256SUMS' -print \
      | sed 's#^\./##' \
      | LC_ALL=C sort
  ) >"$list"
  chmod 0644 "$manifest"
  : >"$manifest"
  while IFS= read -r relative_path; do
    printf '%s  %s\n' "$(hash_file "$bundle_root/$relative_path")" "$relative_path" \
      >>"$manifest"
  done <"$list"
  chmod 0444 "$manifest"
}

archive_tree() {
  bundle_root=$1
  archive=$2
  (
    CDPATH= cd -- "$bundle_root"
    COPYFILE_DISABLE=1 tar -czf "$archive" .
  )
}

kernel="$temporary_directory/qingyu-kernel"
web_dist="$temporary_directory/web-dist"
archive="$temporary_directory/qingyu-runtime-linux-amd64.tar.gz"
extracted="$temporary_directory/extracted"
mkdir -p "$web_dist/assets"
printf '%s\n' '<!doctype html><script type="module" src="/assets/app.js"></script>' \
  >"$web_dist/index.html"
printf '%s\n' 'globalThis.__QINGYU_WEB__ = true;' >"$web_dist/assets/app.js"
chmod 0644 "$web_dist/index.html" "$web_dist/assets/app.js"
make_elf "$kernel" amd64

"$packager" \
  --architecture amd64 \
  --kernel "$kernel" \
  --web-dist "$web_dist" \
  --output "$archive"

[ -f "$archive" ] || fail "packager did not create the runtime archive"
[ -f "$archive.sha256" ] || fail "packager did not create the archive checksum sidecar"
(
  CDPATH= cd -- "$temporary_directory"
  sha256sum -c "$(basename "$archive.sha256")"
) >/dev/null || fail "archive checksum sidecar does not match the archive"

"$verifier" \
  --archive "$archive" \
  --architecture amd64 \
  --extract-to "$extracted"

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
  [ -f "$extracted/$required_path" ] \
    || fail "verified bundle is missing $required_path"
done

grep -F 'architecture=amd64' "$extracted/BUNDLE-METADATA" >/dev/null \
  || fail "bundle metadata does not freeze the target architecture"
grep -F 'format=qingyu-runtime-bundle-v2' "$extracted/BUNDLE-METADATA" >/dev/null \
  || fail "bundle metadata does not identify the Compose-ready release format"
grep -F '  Dockerfile' "$extracted/SHA256SUMS" >/dev/null \
  || fail "internal manifest does not include the runtime-only Dockerfile"
grep -F '  compose.yaml' "$extracted/SHA256SUMS" >/dev/null \
  || fail "internal manifest does not include the runtime-only Compose file"
grep -F '  scripts/entrypoint.sh' "$extracted/SHA256SUMS" >/dev/null \
  || fail "internal manifest does not include the entrypoint"
grep -F '  scripts/verify-final-web-assets.sh' "$extracted/SHA256SUMS" >/dev/null \
  || fail "internal manifest does not include the final Web verifier"

if grep -E '(cargo|rustc|pnpm|npm|nodejs|COPY[[:space:]]+(apps|packages|src))' \
  "$extracted/Dockerfile" >/dev/null; then
  fail "runtime-only Dockerfile contains a source build or toolchain reference"
fi
grep -F 'USER 10001:10001' "$extracted/Dockerfile" >/dev/null \
  || fail "runtime-only Dockerfile lost the numeric runtime user"
grep -F 'WORKDIR /data' "$extracted/Dockerfile" >/dev/null \
  || fail "runtime-only Dockerfile lost the fixed data root"
grep -F 'EXPOSE 3210' "$extracted/Dockerfile" >/dev/null \
  || fail "runtime-only Dockerfile lost the Kernel port"
grep -F 'STOPSIGNAL SIGTERM' "$extracted/Dockerfile" >/dev/null \
  || fail "runtime-only Dockerfile lost the graceful stop signal"

ruby -ryaml -e '
  compose = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: false)
  abort "runtime Compose top-level contract changed" unless compose.keys.sort == %w[name services volumes]
  service = compose.fetch("services").fetch("qingyu")
  abort "runtime Compose contains a source build" if service.key?("build")
  abort "runtime Compose image is not a required prebuilt input" unless service.fetch("image") == "${QINGYU_SERVER_IMAGE:?QINGYU_SERVER_IMAGE is required}"
  abort "runtime Compose user changed" unless service.fetch("user") == "10001:10001"
  abort "runtime Compose root is writable" unless service.fetch("read_only") == true
  abort "runtime Compose capabilities changed" unless service.fetch("cap_drop") == ["ALL"]
  abort "runtime Compose privileges changed" unless service.fetch("security_opt") == ["no-new-privileges:true"]
  abort "runtime Compose runtime inputs are not value-free" unless service.fetch("environment") == ["QINGYU_PUBLIC_ORIGIN", "QINGYU_SERVER_INITIALIZATION_TOKEN"]
  abort "runtime Compose port is not loopback-only" unless service.fetch("ports") == ["127.0.0.1:3210:3210"]
  abort "runtime Compose data root changed" unless service.fetch("volumes") == ["qingyu-data:/data"]
  abort "runtime Compose stop grace changed" unless service.fetch("stop_grace_period") == "35s"
  abort "runtime Compose restart policy changed" unless service.fetch("restart") == "unless-stopped"
' "$extracted/compose.yaml"

symlink_kernel="$temporary_directory/symlink-kernel"
ln -s "$kernel" "$symlink_kernel"
expect_rejection \
  kernel-symlink \
  'Kernel input must be one retained regular file, not a symbolic link' \
  "$packager" --architecture amd64 --kernel "$symlink_kernel" \
  --web-dist "$web_dist" --output "$temporary_directory/symlink-kernel.tar.gz"

non_elf_kernel="$temporary_directory/non-elf-kernel"
printf '%s\n' '#!/bin/sh' >"$non_elf_kernel"
chmod 0755 "$non_elf_kernel"
expect_rejection \
  kernel-non-elf \
  'Kernel input must be a 64-bit little-endian ELF executable' \
  "$packager" --architecture amd64 --kernel "$non_elf_kernel" \
  --web-dist "$web_dist" --output "$temporary_directory/non-elf.tar.gz"

wrong_arch_kernel="$temporary_directory/wrong-arch-kernel"
make_elf "$wrong_arch_kernel" arm64
expect_rejection \
  kernel-wrong-architecture \
  'Kernel ELF architecture does not match requested linux/amd64' \
  "$packager" --architecture amd64 --kernel "$wrong_arch_kernel" \
  --web-dist "$web_dist" --output "$temporary_directory/wrong-arch.tar.gz"

arm64_archive="$temporary_directory/qingyu-runtime-linux-arm64.tar.gz"
"$packager" --architecture arm64 --kernel "$wrong_arch_kernel" \
  --web-dist "$web_dist" --output "$arm64_archive" >/dev/null
"$verifier" --archive "$arm64_archive" --architecture arm64 >/dev/null

symlink_web="$temporary_directory/symlink-web"
cp -R "$web_dist" "$symlink_web"
ln -s app.js "$symlink_web/assets/linked.js"
expect_rejection \
  web-symlink \
  'symbolic links and special files are forbidden in the Web distribution' \
  "$packager" --architecture amd64 --kernel "$kernel" \
  --web-dist "$symlink_web" --output "$temporary_directory/symlink-web.tar.gz"

special_web="$temporary_directory/special-web"
cp -R "$web_dist" "$special_web"
mkfifo "$special_web/assets/runtime.txt"
expect_rejection \
  web-special-file \
  'symbolic links and special files are forbidden in the Web distribution' \
  "$packager" --architecture amd64 --kernel "$kernel" \
  --web-dist "$special_web" --output "$temporary_directory/special-web.tar.gz"

source_web="$temporary_directory/source-web"
cp -R "$web_dist" "$source_web"
mkdir "$source_web/src"
printf '%s\n' 'export const source = true;' >"$source_web/src/main.js"
expect_rejection \
  web-source-tree \
  'source-like Web paths are forbidden in a built distribution: src' \
  "$packager" --architecture amd64 --kernel "$kernel" \
  --web-dist "$source_web" --output "$temporary_directory/source-web.tar.gz"

expect_rejection \
  extra-packager-input \
  'unknown argument: --source' \
  "$packager" --architecture amd64 --kernel "$kernel" --web-dist "$web_dist" \
  --output "$temporary_directory/extra-input.tar.gz" --source "$repo_root/apps/kernel/src"

tampered_tree="$temporary_directory/tampered-tree"
tampered_archive="$temporary_directory/tampered.tar.gz"
cp -R "$extracted" "$tampered_tree"
chmod 0644 "$tampered_tree/web/assets/app.js"
printf '%s\n' 'globalThis.__TAMPERED__ = true;' >>"$tampered_tree/web/assets/app.js"
archive_tree "$tampered_tree" "$tampered_archive"
expect_rejection \
  tampered-content \
  'SHA-256 manifest mismatch: web/assets/app.js' \
  "$verifier" --archive "$tampered_archive" --architecture amd64

extra_tree="$temporary_directory/extra-tree"
extra_archive="$temporary_directory/extra.tar.gz"
cp -R "$extracted" "$extra_tree"
chmod 0755 "$extra_tree"
printf '%s\n' 'unexpected' >"$extra_tree/EXTRA.txt"
archive_tree "$extra_tree" "$extra_archive"
expect_rejection \
  extra-bundle-content \
  'bundle archive contains unexpected member: EXTRA.txt' \
  "$verifier" --archive "$extra_archive" --architecture amd64

tampered_compose_tree="$temporary_directory/tampered-compose-tree"
tampered_compose_archive="$temporary_directory/tampered-compose.tar.gz"
cp -R "$extracted" "$tampered_compose_tree"
chmod 0644 "$tampered_compose_tree/compose.yaml"
printf '%s\n' '# unmanifested mutation' >>"$tampered_compose_tree/compose.yaml"
chmod 0444 "$tampered_compose_tree/compose.yaml"
archive_tree "$tampered_compose_tree" "$tampered_compose_archive"
expect_rejection \
  tampered-compose-content \
  'SHA-256 manifest mismatch: compose.yaml' \
  "$verifier" --archive "$tampered_compose_archive" --architecture amd64

expect_compose_rejection() {
  mutation_name=$1
  expected_message=$2
  mutation=$3
  mutation_tree="$temporary_directory/compose-$mutation_name-tree"
  mutation_archive="$temporary_directory/compose-$mutation_name.tar.gz"
  cp -R "$extracted" "$mutation_tree"
  chmod 0644 "$mutation_tree/compose.yaml"
  ruby -ryaml -e '
    compose = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: false)
    service = compose.fetch("services").fetch("qingyu")
    case ARGV.fetch(1)
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
      abort "unknown Compose mutation"
    end
    File.write(ARGV.fetch(0), YAML.dump(compose))
  ' "$mutation_tree/compose.yaml" "$mutation"
  chmod 0444 "$mutation_tree/compose.yaml"
  rewrite_manifest "$mutation_tree"
  archive_tree "$mutation_tree" "$mutation_archive"
  expect_rejection \
    "compose-$mutation_name" \
    "$expected_message" \
    "$verifier" --archive "$mutation_archive" --architecture amd64
}

expect_compose_rejection source-build \
  'frozen runtime control file checksum mismatch: compose.yaml' build
expect_compose_rejection optional-image \
  'frozen runtime control file checksum mismatch: compose.yaml' image-default
expect_compose_rejection literal-runtime-inputs \
  'frozen runtime control file checksum mismatch: compose.yaml' literal-token
expect_compose_rejection writable-root \
  'frozen runtime control file checksum mismatch: compose.yaml' writable-root
expect_compose_rejection wrong-user \
  'frozen runtime control file checksum mismatch: compose.yaml' wrong-user
expect_compose_rejection retained-capability \
  'frozen runtime control file checksum mismatch: compose.yaml' capability
expect_compose_rejection privilege-escalation \
  'frozen runtime control file checksum mismatch: compose.yaml' privilege
expect_compose_rejection public-port \
  'frozen runtime control file checksum mismatch: compose.yaml' public-port
expect_compose_rejection host-data \
  'frozen runtime control file checksum mismatch: compose.yaml' host-data
expect_compose_rejection weak-tmpfs \
  'frozen runtime control file checksum mismatch: compose.yaml' weak-tmpfs
expect_compose_rejection short-stop \
  'frozen runtime control file checksum mismatch: compose.yaml' short-stop
expect_compose_rejection no-restart \
  'frozen runtime control file checksum mismatch: compose.yaml' no-restart

invalid_web_tree="$temporary_directory/invalid-web-tree"
invalid_web_archive="$temporary_directory/invalid-web.tar.gz"
cp -R "$extracted" "$invalid_web_tree"
chmod 0755 "$invalid_web_tree/web/assets"
printf '%s\n' 'export const source = true;' >"$invalid_web_tree/web/assets/source.ts"
chmod 0444 "$invalid_web_tree/web/assets/source.ts"
chmod 0555 "$invalid_web_tree/web/assets"
rewrite_manifest "$invalid_web_tree"
archive_tree "$invalid_web_tree" "$invalid_web_archive"
expect_rejection \
  invalid-bundled-web \
  'unsupported Web asset extension' \
  "$verifier" --archive "$invalid_web_archive" --architecture amd64

toolchain_tree="$temporary_directory/toolchain-tree"
toolchain_archive="$temporary_directory/toolchain.tar.gz"
cp -R "$extracted" "$toolchain_tree"
chmod 0644 "$toolchain_tree/Dockerfile"
awk '
  /^USER 10001:10001$/ && !inserted {
    print "RUN apt-get update && apt-get install --yes nodejs"
    inserted = 1
  }
  { print }
  END { if (!inserted) exit 2 }
' "$extracted/Dockerfile" >"$toolchain_tree/Dockerfile.mutated" \
  || fail 'could not create the runtime Dockerfile toolchain mutation'
mv "$toolchain_tree/Dockerfile.mutated" "$toolchain_tree/Dockerfile"
chmod 0444 "$toolchain_tree/Dockerfile"
rewrite_manifest "$toolchain_tree"
archive_tree "$toolchain_tree" "$toolchain_archive"
expect_rejection \
  runtime-dockerfile-toolchain \
  'frozen runtime control file checksum mismatch: Dockerfile' \
  "$verifier" --archive "$toolchain_archive" --architecture amd64

untrusted_script_tree="$temporary_directory/untrusted-script-tree"
untrusted_script_archive="$temporary_directory/untrusted-script.tar.gz"
untrusted_script_marker="$temporary_directory/untrusted-script-ran"
cp -R "$extracted" "$untrusted_script_tree"
chmod 0644 "$untrusted_script_tree/scripts/verify-final-web-assets.sh"
printf '%s\n' '#!/bin/sh' ": > \"$untrusted_script_marker\"" \
  >"$untrusted_script_tree/scripts/verify-final-web-assets.sh"
chmod 0555 "$untrusted_script_tree/scripts/verify-final-web-assets.sh"
rewrite_manifest "$untrusted_script_tree"
archive_tree "$untrusted_script_tree" "$untrusted_script_archive"
expect_rejection \
  untrusted-bundled-verifier \
  'frozen runtime control file checksum mismatch: scripts/verify-final-web-assets.sh' \
  "$verifier" --archive "$untrusted_script_archive" --architecture amd64
[ ! -e "$untrusted_script_marker" ] \
  || fail 'archive verification executed an untrusted bundled script'

wrong_metadata_tree="$temporary_directory/wrong-metadata-tree"
wrong_metadata_archive="$temporary_directory/wrong-metadata.tar.gz"
cp -R "$extracted" "$wrong_metadata_tree"
chmod 0644 "$wrong_metadata_tree/BUNDLE-METADATA"
sed 's/architecture=amd64/architecture=arm64/' \
  "$extracted/BUNDLE-METADATA" >"$wrong_metadata_tree/BUNDLE-METADATA"
chmod 0444 "$wrong_metadata_tree/BUNDLE-METADATA"
rewrite_manifest "$wrong_metadata_tree"
archive_tree "$wrong_metadata_tree" "$wrong_metadata_archive"
expect_rejection \
  wrong-bundled-architecture \
  'Kernel ELF architecture does not match requested linux/arm64' \
  "$verifier" --archive "$wrong_metadata_archive" --architecture arm64

malicious_archive="$temporary_directory/path-traversal.tar.gz"
ruby -rrubygems/package -rzlib -rstringio -e '
  buffer = StringIO.new("".b)
  Gem::Package::TarWriter.new(buffer) do |tar|
    contents = "escape\n"
    tar.add_file("../escape", 0o644) { |entry| entry.write(contents) }
  end
  Zlib::GzipWriter.open(ARGV.fetch(0)) do |gzip|
    gzip.write(buffer.string)
  end
' "$malicious_archive"
expect_rejection \
  archive-path-traversal \
  'archive contains an unsafe member path: ../escape' \
  "$verifier" --archive "$malicious_archive" --architecture amd64

symlink_archive_tree="$temporary_directory/symlink-archive-tree"
symlink_archive="$temporary_directory/symlink-archive.tar.gz"
cp -R "$extracted" "$symlink_archive_tree"
chmod 0755 "$symlink_archive_tree/scripts"
rm "$symlink_archive_tree/scripts/entrypoint.sh"
ln -s ../Dockerfile "$symlink_archive_tree/scripts/entrypoint.sh"
archive_tree "$symlink_archive_tree" "$symlink_archive"
expect_rejection \
  archive-symlink \
  'archive may contain only regular files and directories' \
  "$verifier" --archive "$symlink_archive" --architecture amd64

printf '%s\n' 'PASS: runtime-only bundle packaging and adversarial verification contract.'
