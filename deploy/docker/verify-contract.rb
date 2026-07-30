#!/usr/bin/env ruby
# frozen_string_literal: true

require "digest"
require "open3"
require "shellwords"
require "yaml"

Instruction = Struct.new(:name, :arguments, :line, keyword_init: true)
Stage = Struct.new(:base, :alias_name, :instructions, keyword_init: true)

CANONICAL_DOCKERIGNORE_SHA256 =
  "5b8e84a7e25a385040213570ca7847548e61bba617c0ccbf13c337fd95e04644"
CANONICAL_DOCKERFILE_SYNTAX = "# syntax=docker/dockerfile:1.7"
DOCKERFILE_PARSER_DIRECTIVE = /\A\s*#\s*(?:syntax|escape|check)\s*=/i

def fail_contract(message)
  warn "FAIL: #{message}"
  exit 1
end

def assert_contract(condition, message)
  fail_contract(message) unless condition
end

def verify_dockerfile_parser_directives(path)
  lines = File.readlines(path, chomp: true)
  parser_directives = lines.select { |line| line.match?(DOCKERFILE_PARSER_DIRECTIVE) }
  assert_contract(
    lines.first == CANONICAL_DOCKERFILE_SYNTAX &&
      parser_directives == [CANONICAL_DOCKERFILE_SYNTAX],
    "Dockerfile must declare only # syntax=docker/dockerfile:1.7"
  )
end

def read_logical_dockerfile_instructions(path)
  instructions = []
  buffer = +""
  first_line = nil

  File.foreach(path).with_index(1) do |raw_line, line_number|
    stripped = raw_line.rstrip
    next if buffer.empty? && (stripped.strip.empty? || stripped.lstrip.start_with?("#"))

    first_line ||= line_number
    continued = stripped.end_with?("\\")
    fragment = continued ? stripped.delete_suffix("\\").rstrip : stripped
    buffer << " " unless buffer.empty?
    buffer << fragment.strip

    next if continued

    match = buffer.match(/\A([A-Za-z]+)(?:\s+(.*))?\z/m)
    fail_contract("unparseable Dockerfile instruction at line #{first_line}") unless match

    instructions << Instruction.new(
      name: match[1].upcase,
      arguments: (match[2] || "").strip,
      line: first_line
    )
    buffer = +""
    first_line = nil
  end

  fail_contract("unterminated Dockerfile continuation at line #{first_line}") unless buffer.empty?
  instructions
end

def parse_dockerfile(path)
  stages = []
  current_stage = nil

  read_logical_dockerfile_instructions(path).each do |instruction|
    if instruction.name == "FROM"
      match = instruction.arguments.match(
        /\A(?:--platform=\S+\s+)?(\S+?)(?:\s+AS\s+(\S+))?\z/i
      )
      fail_contract("unparseable FROM instruction at line #{instruction.line}") unless match

      current_stage = Stage.new(
        base: match[1],
        alias_name: match[2]&.downcase,
        instructions: []
      )
      stages << current_stage
      next
    end

    fail_contract("Dockerfile instruction before the first FROM at line #{instruction.line}") unless current_stage
    current_stage.instructions << instruction
  end

  assert_contract(!stages.empty?, "Dockerfile must contain build stages")
  stages
end

def instructions_named(stage, name)
  stage.instructions.select { |instruction| instruction.name == name }
end

def require_stage(stages, alias_name, base_pattern, message)
  matching = stages.select { |stage| stage.alias_name == alias_name }
  assert_contract(matching.length == 1, message)
  assert_contract(matching.first.base.match?(base_pattern), message)
  matching.first
end

def instruction_includes?(stage, name, required_text)
  instructions_named(stage, name).any? do |instruction|
    instruction.arguments.gsub(/\s+/, " ").include?(required_text)
  end
end

def assert_canonical_stage(stage, expected_base, expected_instructions, message)
  actual_instructions = stage.instructions.map do |instruction|
    [instruction.name, instruction.arguments.gsub(/\s+/, " ")]
  end
  assert_contract(
    stage.base == expected_base && actual_instructions == expected_instructions,
    message
  )
end

def verify_dockerfile(path)
  verify_dockerfile_parser_directives(path)
  stages = parse_dockerfile(path)
  web_stage = require_stage(
    stages,
    "web-build",
    /\Anode:[^\s]+\z/,
    "Dockerfile must contain one Node web-build stage"
  )
  kernel_stage = require_stage(
    stages,
    "kernel-build",
    /\Arust:[^\s]+\z/,
    "Dockerfile must contain one Rust kernel-build stage"
  )
  runtime_stage = require_stage(
    stages,
    "qingyu-runtime",
    /\Adebian:[^\s]+\z/,
    "Dockerfile must contain one Debian qingyu-runtime stage"
  )

  assert_contract(
    stages.length == 3 &&
      stages[0].equal?(web_stage) &&
      stages[1].equal?(kernel_stage) &&
      stages[2].equal?(runtime_stage),
    "Dockerfile must contain exactly the frozen web-build, kernel-build, and qingyu-runtime stages"
  )

  assert_contract(
    stages.last.equal?(runtime_stage),
    "qingyu-runtime must be the final Dockerfile stage"
  )

  assert_contract(
    instruction_includes?(web_stage, "RUN", "corepack prepare pnpm@10.30.3 --activate"),
    "web-build must activate the repository pnpm version"
  )
  assert_contract(
    instruction_includes?(web_stage, "RUN", "pnpm install --frozen-lockfile"),
    "web-build must install from the frozen pnpm lockfile"
  )
  assert_contract(
    instruction_includes?(web_stage, "RUN", "pnpm --filter @markra/web build"),
    "web-build must build apps/web"
  )
  assert_contract(
    instruction_includes?(
      kernel_stage,
      "RUN",
      "cargo build --locked --release --manifest-path apps/kernel/Cargo.toml --bin qingyu-kernel"
    ),
    "kernel-build must build the locked release qingyu-kernel binary"
  )

  final_forbidden_toolchain =
    /(?:\bnode(?:js)?\b|\bnpm\b|\bpnpm\b|\byarn\b|\bbun\b|\bcorepack\b)/i
  toolchain_sensitive_instructions = %w[RUN COPY ADD CMD ENTRYPOINT SHELL ENV ARG]
  final_has_node_toolchain = runtime_stage.instructions.any? do |instruction|
    toolchain_sensitive_instructions.include?(instruction.name) &&
      instruction.arguments.match?(final_forbidden_toolchain)
  end
  assert_contract(
    !final_has_node_toolchain,
    "final Dockerfile stage must not install or copy Node toolchains"
  )

  runtime_copies = instructions_named(runtime_stage, "COPY").map(&:arguments)
  assert_contract(
    runtime_copies.length == 4,
    "runtime image must contain only the Kernel, Web asset, entrypoint, and asset verifier copies"
  )
  assert_contract(
    runtime_copies.any? do |arguments|
      arguments.match?(
        /(?:\A|\s)--from=kernel-build(?:\s|=).*\/qingyu-kernel\s+\/usr\/local\/bin\/qingyu-kernel\z/
      )
    end,
    "runtime image must copy the release Kernel binary from kernel-build"
  )
  assert_contract(
    runtime_copies.any? do |arguments|
      arguments.match?(
        /(?:\A|\s)--from=web-build(?:\s|=).*\/apps\/web\/dist\s+\/opt\/qingyu\/web\z/
      )
    end,
    "runtime image must copy built Web assets from web-build"
  )
  assert_contract(
    runtime_copies.any? do |arguments|
      arguments.match?(
        /(?:\A|\s)--chmod=0555(?:\s|=).*deploy\/docker\/entrypoint\.sh\s+\/usr\/local\/bin\/qingyu-server-entrypoint\z/
      )
    end,
    "runtime image must install the fixed server entrypoint"
  )
  assert_contract(
    runtime_copies.any? do |arguments|
      arguments.match?(
        /(?:\A|\s)--chmod=0555(?:\s|=).*deploy\/docker\/verify-final-web-assets\.sh\s+\/usr\/local\/bin\/qingyu-verify-final-web-assets\z/
      )
    end,
    "runtime image must install the fixed final Web asset verifier"
  )

  users = instructions_named(runtime_stage, "USER").map(&:arguments)
  assert_contract(users == ["10001:10001"], "runtime image must use only UID/GID 10001:10001")

  workdirs = instructions_named(runtime_stage, "WORKDIR").map(&:arguments)
  assert_contract(workdirs == ["/data"], "runtime image must use fixed /data as its working directory")

  all_exposes = stages.flat_map { |stage| instructions_named(stage, "EXPOSE") }.map(&:arguments)
  assert_contract(all_exposes == ["3210"], "Dockerfile must expose only Kernel port 3210")

  all_entrypoints = stages.flat_map { |stage| instructions_named(stage, "ENTRYPOINT") }.map(&:arguments)
  assert_contract(
    all_entrypoints == ['["/usr/local/bin/qingyu-server-entrypoint"]'],
    "runtime image must own the single fixed server entrypoint"
  )
  assert_contract(
    instructions_named(runtime_stage, "CMD").empty?,
    "runtime image must not add an alternate command"
  )
  assert_contract(
    instructions_named(runtime_stage, "RUN").length == 2,
    "runtime image must contain only base-system setup and final Web asset verification"
  )

  forbidden_runtime_input =
    /QINGYU_SERVER_INITIALIZATION_TOKEN|QINGYU_PUBLIC_ORIGIN|QINGYU_(?:DATA|WORKSPACE|CONFIG|STATE|LOGS|CACHE)_DIR/
  has_build_time_runtime_input = stages.any? do |stage|
    stage.instructions.any? do |instruction|
      %w[ARG ENV RUN LABEL].include?(instruction.name) &&
        instruction.arguments.match?(forbidden_runtime_input)
    end
  end
  assert_contract(
    !has_build_time_runtime_input,
    "runtime inputs and data-root overrides must not enter image instructions"
  )

  assert_canonical_stage(
    web_stage,
    "node:24-bookworm-slim",
    [
      ["WORKDIR", "/src"],
      ["RUN", "corepack enable && corepack prepare pnpm@10.30.3 --activate"],
      ["COPY", "package.json pnpm-lock.yaml pnpm-workspace.yaml ./"],
      ["COPY", "apps/web/package.json apps/web/package.json"],
      ["COPY", "packages/app/package.json packages/app/package.json"],
      ["COPY", "packages/editor/package.json packages/editor/package.json"],
      ["COPY", "packages/editor-react/package.json packages/editor-react/package.json"],
      ["COPY", "packages/kernel-client/package.json packages/kernel-client/package.json"],
      ["COPY", "packages/markdown/package.json packages/markdown/package.json"],
      ["COPY", "packages/scripts/package.json packages/scripts/package.json"],
      ["COPY", "packages/shared/package.json packages/shared/package.json"],
      ["COPY", "packages/ui/package.json packages/ui/package.json"],
      ["COPY", "deploy/docker/verify-web-dist.mjs deploy/docker/verify-web-dist.mjs"],
      ["RUN", "pnpm install --frozen-lockfile"],
      ["COPY", "apps/web apps/web"],
      ["COPY", "packages packages"],
      ["RUN", "pnpm --filter @markra/web build"],
      ["RUN", "node deploy/docker/verify-web-dist.mjs apps/web/dist"]
    ],
    "web-build instruction sequence must match the frozen contract"
  )
  assert_canonical_stage(
    kernel_stage,
    "rust:1.92-bookworm",
    [
      ["WORKDIR", "/src"],
      ["COPY", "apps/kernel/Cargo.toml apps/kernel/Cargo.lock apps/kernel/"],
      ["COPY", "apps/kernel/src apps/kernel/src"],
      [
        "RUN",
        "cargo build --locked --release --manifest-path apps/kernel/Cargo.toml --bin qingyu-kernel"
      ]
    ],
    "kernel-build instruction sequence must match the frozen contract"
  )
  assert_canonical_stage(
    runtime_stage,
    "debian:bookworm-slim",
    [
      [
        "RUN",
        "apt-get update && apt-get install --yes --no-install-recommends ca-certificates " \
          "&& rm -rf /var/lib/apt/lists/* && groupadd --gid 10001 qingyu " \
          "&& useradd --uid 10001 --gid 10001 --home-dir /nonexistent " \
          "--shell /usr/sbin/nologin --no-create-home qingyu " \
          "&& install -d -o 10001 -g 10001 -m 0700 /data /tmp/qingyu " \
          "&& install -d -o 0 -g 0 -m 0755 /opt/qingyu/web"
      ],
      [
        "COPY",
        "--from=kernel-build /src/apps/kernel/target/release/qingyu-kernel " \
          "/usr/local/bin/qingyu-kernel"
      ],
      ["COPY", "--from=web-build /src/apps/web/dist /opt/qingyu/web"],
      [
        "COPY",
        "--chmod=0555 deploy/docker/entrypoint.sh /usr/local/bin/qingyu-server-entrypoint"
      ],
      [
        "COPY",
        "--chmod=0555 deploy/docker/verify-final-web-assets.sh " \
          "/usr/local/bin/qingyu-verify-final-web-assets"
      ],
      ["RUN", "/usr/local/bin/qingyu-verify-final-web-assets /opt/qingyu/web"],
      [
        "LABEL",
        'dev.qingyu.image.kind="kernel-api-with-unserved-web-assets" ' \
          'dev.qingyu.image.phase-gate="static-web-serving-required" ' \
          'dev.qingyu.image.web-assets="/opt/qingyu/web"'
      ],
      ["USER", "10001:10001"],
      ["WORKDIR", "/data"],
      ["EXPOSE", "3210"],
      ["STOPSIGNAL", "SIGTERM"],
      ["HEALTHCHECK", "NONE"],
      ["ENTRYPOINT", '["/usr/local/bin/qingyu-server-entrypoint"]']
    ],
    "qingyu-runtime instruction sequence must match the frozen contract"
  )

  stages
end

def parse_compose(path)
  YAML.safe_load(File.read(path), aliases: false)
rescue Psych::Exception => error
  fail_contract("Compose must be valid YAML: #{error.message.lines.first.strip}")
end

def assert_exact_keys(hash, expected_keys, message)
  assert_contract(hash.is_a?(Hash), message)
  assert_contract(hash.keys.sort == expected_keys.sort, message)
end

def verify_compose(path)
  compose = parse_compose(path)
  assert_exact_keys(compose, %w[name services volumes], "Compose top-level contract contains unexpected keys")
  assert_contract(compose["name"] == "qingyu-server-contract", "Compose project name must be fixed")

  services = compose["services"]
  assert_exact_keys(services, ["qingyu"], "Compose must contain only the qingyu service")
  service = services["qingyu"]
  assert_exact_keys(
    service,
    %w[
      profiles build image user init read_only cap_drop security_opt environment ports volumes
      tmpfs restart healthcheck labels
    ],
    "Compose qingyu service contains unexpected or missing keys"
  )

  assert_contract(
    service["profiles"] == ["static-web-serving-required"],
    "Compose must remain behind only the static-web-serving-required profile gate"
  )
  assert_contract(
    service["build"] == {
      "context" => "../..",
      "dockerfile" => "Dockerfile",
      "target" => "qingyu-runtime"
    },
    "Compose build must target the root qingyu-runtime image without arguments"
  )
  assert_contract(
    service["image"] == "${QINGYU_SERVER_IMAGE:-qingyu-server:local}",
    "Compose image reference must keep the documented local default"
  )
  assert_contract(service["user"] == "10001:10001", "Compose user must be exactly 10001:10001")
  assert_contract(service["init"] == true, "Compose init must be the YAML boolean true")
  assert_contract(
    service["read_only"] == true,
    "Compose read_only must be the YAML boolean true"
  )
  assert_contract(service["cap_drop"] == ["ALL"], "Compose cap_drop must contain only ALL")
  assert_contract(
    service["security_opt"] == ["no-new-privileges:true"],
    "Compose security_opt must contain only no-new-privileges:true"
  )
  assert_contract(
    service["environment"] == ["QINGYU_PUBLIC_ORIGIN", "QINGYU_SERVER_INITIALIZATION_TOKEN"],
    "Compose environment must contain only value-free runtime inputs"
  )
  assert_contract(
    service["ports"] == ["127.0.0.1:3210:3210"],
    "Compose must publish only Kernel port 3210 on loopback"
  )
  assert_contract(
    service["volumes"] == ["qingyu-data:/data"],
    "Compose must mount only qingyu-data at fixed /data"
  )
  assert_contract(
    service["tmpfs"] == [
      "/tmp/qingyu:rw,noexec,nosuid,nodev,size=64m,uid=10001,gid=10001,mode=0700"
    ],
    "Compose must provide only the hardened /tmp/qingyu tmpfs"
  )
  assert_contract(service["restart"] == "unless-stopped", "Compose restart policy must be unless-stopped")
  assert_contract(
    service["healthcheck"] == { "disable" => true },
    "Compose healthcheck disable must be the YAML boolean true"
  )
  assert_contract(
    service["labels"] == {
      "dev.qingyu.contract.runtime-gate" => "static-web-serving-required",
      "dev.qingyu.contract.data-root" => "/data",
      "dev.qingyu.contract.web-assets" => "/opt/qingyu/web",
      "dev.qingyu.contract.web-assets-served" => "false",
      "dev.qingyu.contract.health-live" => "/api/v1/health/live",
      "dev.qingyu.contract.health-ready" => "/api/v1/health/ready"
    },
    "Compose labels must describe the exact gated runtime contract"
  )

  assert_exact_keys(compose["volumes"], ["qingyu-data"], "Compose must declare only qingyu-data")
  volume_definition = compose["volumes"]["qingyu-data"]
  assert_contract(
    volume_definition.nil? || volume_definition == {},
    "qingyu-data must not contain an external or host-path override"
  )
end

def dockerignore_patterns(path)
  File.readlines(path, chomp: true).filter_map do |line|
    stripped = line.strip
    next if stripped.empty? || stripped.start_with?("#")

    stripped
  end
end

def docker_pattern_matches?(pattern, path)
  normalized_pattern = pattern.sub(%r{\A/}, "").sub(%r{/\z}, "")
  normalized_path = path.sub(%r{\A\./}, "").sub(%r{/\z}, "")
  flags = File::FNM_PATHNAME | File::FNM_DOTMATCH
  path_and_parents = [normalized_path]
  parent = File.dirname(normalized_path)
  while parent != "." && parent != "/"
    path_and_parents << parent
    parent = File.dirname(parent)
  end

  if normalized_pattern.include?("/")
    patterns = [normalized_pattern]
    patterns << normalized_pattern.delete_prefix("**/") if normalized_pattern.start_with?("**/")
    return path_and_parents.any? do |candidate|
      patterns.any? { |candidate_pattern| File.fnmatch?(candidate_pattern, candidate, flags) }
    end
  end

  path_and_parents.any? do |candidate|
    candidate.split("/").any? { |segment| File.fnmatch?(normalized_pattern, segment, flags) }
  end
end

def dockerignore_excludes?(patterns, path)
  excluded = false
  patterns.each do |raw_pattern|
    negated = raw_pattern.start_with?("!")
    pattern = negated ? raw_pattern.delete_prefix("!") : raw_pattern
    excluded = !negated if docker_pattern_matches?(pattern, path)
  end
  excluded
end

def local_copy_sources(stages)
  stages.flat_map(&:instructions).filter_map do |instruction|
    next unless instruction.name == "COPY"

    tokens = Shellwords.split(instruction.arguments)
    next if tokens.any? { |token| token.start_with?("--from=") }

    tokens.shift while tokens.first&.start_with?("--")
    fail_contract("COPY at line #{instruction.line} must contain source and destination") if tokens.length < 2
    tokens[0...-1]
  rescue ArgumentError => error
    fail_contract("unparseable COPY at line #{instruction.line}: #{error.message}")
  end.flatten
end

def validate_tracked_build_inputs(paths, description)
  assert_contract(!paths.empty?, "#{description} must not be empty")
  paths.each do |path|
    components = path.split("/", -1)
    assert_contract(
      path.valid_encoding? &&
        !path.empty? &&
        !path.start_with?("/") &&
        !path.include?("\\") &&
        !path.match?(/[[:cntrl:]]/) &&
        components.none? { |component| component.empty? || component == "." || component == ".." },
      "#{description} contains an unsafe path"
    )
  end
  assert_contract(
    paths == paths.uniq.sort,
    "#{description} must contain unique byte-sorted paths"
  )
  paths
end

def read_tracked_build_inputs_manifest(path)
  assert_contract(File.file?(path), "missing tracked Docker build input manifest: #{path}")
  validate_tracked_build_inputs(
    File.readlines(path, chomp: true),
    "tracked Docker build input fallback manifest"
  )
end

def git_tracked_build_inputs(repo_root, sources)
  stdout, _stderr, status = Open3.capture3(
    "git",
    "-C",
    repo_root,
    "ls-files",
    "-z",
    "--",
    *sources
  )
  return nil unless status.success?

  paths = stdout.split("\0", -1)
  paths.pop if paths.last == ""
  validate_tracked_build_inputs(paths.sort, "Git tracked Docker build inputs")
end

def tracked_build_inputs(repo_root, stages, manifest_path)
  sources = [".dockerignore", "Dockerfile", *local_copy_sources(stages)].uniq
  sources.each do |source|
    assert_contract(
      File.exist?(File.join(repo_root, source)),
      "Docker build input is missing: #{source}"
    )
  end
  fallback = read_tracked_build_inputs_manifest(manifest_path)
  discovered = git_tracked_build_inputs(repo_root, sources)
  return fallback unless discovered

  assert_contract(
    discovered == fallback,
    "tracked Docker build input fallback manifest is stale"
  )
  discovered
end

def verify_dockerignore(path, repo_root, stages, tracked_inputs_manifest)
  patterns = dockerignore_patterns(path)
  mandatory_patterns = %w[
    .git .git/** **/.git **/.git/**
    .env .env.* **/.env **/.env.*
    node_modules **/node_modules dist **/dist target **/target
    .cache **/.cache .turbo **/.turbo .vite **/.vite coverage **/coverage
    .npmrc **/.npmrc .aws **/.aws .ssh **/.ssh
    credentials **/credentials secrets **/secrets
    **/*.pem **/*.key **/*.p12 **/*.pfx
  ]
  missing_patterns = mandatory_patterns.reject { |required| patterns.include?(required) }
  assert_contract(
    missing_patterns.empty?,
    ".dockerignore is missing mandatory exclusions: #{missing_patterns.join(', ')}"
  )

  tracked_build_inputs(repo_root, stages, tracked_inputs_manifest).each do |relative_path|
    absolute_path = File.join(repo_root, relative_path)
    assert_contract(File.file?(absolute_path), "tracked Docker build input is missing: #{relative_path}")
    assert_contract(
      !dockerignore_excludes?(patterns, relative_path),
      ".dockerignore excludes tracked Docker build input: #{relative_path}"
    )
  end

  assert_contract(
    Digest::SHA256.file(path).hexdigest == CANONICAL_DOCKERIGNORE_SHA256,
    "Docker build context policy must match the frozen .dockerignore contract"
  )
end

repo_root = File.expand_path("../..", __dir__)
dockerfile = ENV.fetch("QINGYU_VERIFY_DOCKERFILE", File.join(repo_root, "Dockerfile"))
compose_file = ENV.fetch(
  "QINGYU_VERIFY_COMPOSE_FILE",
  File.join(repo_root, "deploy/docker/compose.contract.yaml")
)
dockerignore = ENV.fetch("QINGYU_VERIFY_DOCKERIGNORE", File.join(repo_root, ".dockerignore"))
tracked_inputs_manifest = ENV.fetch(
  "QINGYU_VERIFY_TRACKED_INPUTS_MANIFEST",
  File.join(repo_root, "deploy/docker/tracked-build-inputs.txt")
)

[[dockerfile, "root Dockerfile"], [compose_file, "Compose contract"], [dockerignore, ".dockerignore"]].each do |path, description|
  assert_contract(File.file?(path), "missing #{description}: #{path}")
end

stages = verify_dockerfile(dockerfile)
verify_compose(compose_file)
verify_dockerignore(dockerignore, repo_root, stages, tracked_inputs_manifest)
