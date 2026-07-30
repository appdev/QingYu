#!/usr/bin/env ruby
# frozen_string_literal: true

require "shellwords"
require "yaml"

Instruction = Struct.new(:name, :arguments, :line, keyword_init: true)
Stage = Struct.new(:base, :alias_name, :instructions, keyword_init: true)

def fail_contract(message)
  warn "FAIL: #{message}"
  exit 1
end

def assert_contract(condition, message)
  fail_contract(message) unless condition
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

def verify_dockerfile(path)
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
    runtime_copies.length == 3,
    "runtime image must contain only the Kernel, Web asset, and entrypoint copies"
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
    instructions_named(runtime_stage, "RUN").length == 1,
    "runtime image must contain only its single base-system setup instruction"
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

def verify_dockerignore(path, repo_root, stages)
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

  required_inputs = [
    "Dockerfile",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "apps/web/package.json",
    "apps/web/src/main.tsx",
    "apps/kernel/Cargo.toml",
    "apps/kernel/Cargo.lock",
    "apps/kernel/src/bin/qingyu-kernel.rs",
    "packages/app/package.json",
    "packages/app/src/index.ts",
    "packages/kernel-client/package.json",
    "packages/kernel-client/src/index.ts",
    "packages/shared/package.json",
    "packages/shared/src/index.ts",
    "deploy/docker/entrypoint.sh"
  ]
  required_inputs.concat(local_copy_sources(stages))
  required_inputs.uniq.each do |relative_path|
    absolute_path = File.join(repo_root, relative_path)
    assert_contract(File.exist?(absolute_path), "Docker build input is missing: #{relative_path}")
    assert_contract(
      !dockerignore_excludes?(patterns, relative_path),
      ".dockerignore excludes required Docker build input: #{relative_path}"
    )
  end
end

repo_root = File.expand_path("../..", __dir__)
dockerfile = ENV.fetch("QINGYU_VERIFY_DOCKERFILE", File.join(repo_root, "Dockerfile"))
compose_file = ENV.fetch(
  "QINGYU_VERIFY_COMPOSE_FILE",
  File.join(repo_root, "deploy/docker/compose.contract.yaml")
)
dockerignore = ENV.fetch("QINGYU_VERIFY_DOCKERIGNORE", File.join(repo_root, ".dockerignore"))

[[dockerfile, "root Dockerfile"], [compose_file, "Compose contract"], [dockerignore, ".dockerignore"]].each do |path, description|
  assert_contract(File.file?(path), "missing #{description}: #{path}")
end

stages = verify_dockerfile(dockerfile)
verify_compose(compose_file)
verify_dockerignore(dockerignore, repo_root, stages)
