import { execFileSync } from "node:child_process";
import {
  chmodSync,
  closeSync,
  constants as fsConstants,
  copyFileSync,
  lstatSync,
  mkdirSync,
  openSync,
  readSync,
  renameSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "../../..");
const desktopTargetPattern = /^(?:[A-Za-z0-9_]+)-(?:apple-darwin|unknown-linux-(?:gnu|musl)|pc-windows-(?:msvc|gnu))$/u;

export function resolveKernelTargetTriple({ environment, rustcVersion }) {
  const explicit =
    environment.MARKRA_DESKTOP_TARGET?.trim()
    || environment.TAURI_ENV_TARGET_TRIPLE?.trim()
    || environment.CARGO_BUILD_TARGET?.trim();
  const target = explicit || rustcHostTriple(rustcVersion);
  if (!target || !desktopTargetPattern.test(target)) {
    throw new Error(`${target || "Missing target"} is not a valid desktop Rust target triple.`);
  }
  return target;
}

export function kernelCargoBuildArgs(manifestPath, targetTriple) {
  return [
    "build",
    "--manifest-path",
    manifestPath,
    "--bin",
    "qingyu-kernel",
    "--locked",
    "--release",
    "--target",
    targetTriple,
  ];
}

export function kernelSidecarPaths(root, targetTriple) {
  const suffix = targetTriple.includes("windows") ? ".exe" : "";
  return {
    source: join(
      root,
      "apps/kernel/target",
      targetTriple,
      "release",
      `qingyu-kernel${suffix}`,
    ),
    destination: join(
      root,
      "apps/desktop/src-tauri/binaries",
      `qingyu-kernel-${targetTriple}${suffix}`,
    ),
  };
}

export function validatePreparedKernelSidecar(
  path,
  targetTriple,
  hostPlatform = process.platform,
) {
  let metadata;
  try {
    metadata = lstatSync(path);
  } catch (error) {
    if (error?.code === "ENOENT") {
      throw new Error(`Kernel sidecar does not exist: ${path}`);
    }
    throw error;
  }
  if (metadata.isSymbolicLink()) {
    throw new Error(`Kernel sidecar must not be a symbolic link: ${path}`);
  }
  if (!metadata.isFile()) {
    throw new Error(`Kernel sidecar must be a regular file: ${path}`);
  }
  if (metadata.size === 0) {
    throw new Error(`Kernel sidecar must not be empty: ${path}`);
  }
  if (
    hostPlatform !== "win32"
    && !targetTriple.includes("windows")
    && (metadata.mode & 0o111) === 0
  ) {
    throw new Error(`Kernel sidecar must be executable: ${path}`);
  }

  assertExecutableFormat(path, targetTriple);
  return { byteLength: metadata.size, target: targetTriple };
}

export function prepareKernelSidecar({
  environment = process.env,
  root = repositoryRoot,
  run = execFileSync,
} = {}) {
  const explicitTarget =
    environment.MARKRA_DESKTOP_TARGET?.trim()
    || environment.TAURI_ENV_TARGET_TRIPLE?.trim()
    || environment.CARGO_BUILD_TARGET?.trim();
  const rustcVersion = explicitTarget
    ? ""
    : run("rustc", ["-vV"], { cwd: root, encoding: "utf8" });
  const targetTriple = resolveKernelTargetTriple({ environment, rustcVersion });
  const manifestPath = join(root, "apps/kernel/Cargo.toml");
  run("cargo", kernelCargoBuildArgs(manifestPath, targetTriple), {
    cwd: root,
    stdio: "inherit",
  });

  const paths = kernelSidecarPaths(root, targetTriple);
  validatePreparedKernelSidecar(paths.source, targetTriple);
  mkdirSync(dirname(paths.destination), { recursive: true });
  const validation = copyKernelSidecarAtomically(
    paths.source,
    paths.destination,
    targetTriple,
  );
  return { ...paths, ...validation };
}

export function copyKernelSidecarAtomically(
  source,
  destination,
  targetTriple,
  hostPlatform = process.platform,
) {
  validatePreparedKernelSidecar(source, targetTriple, hostPlatform);
  rejectHardLink(source, "Kernel sidecar source");
  rejectDestinationDirectorySymlink(destination);
  rejectDestinationLink(destination);

  const temporary = `${destination}.tmp-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  try {
    copyFileSync(source, temporary, fsConstants.COPYFILE_EXCL);
    if (hostPlatform !== "win32" && !targetTriple.includes("windows")) {
      chmodSync(temporary, 0o755);
    }
    const validation = validatePreparedKernelSidecar(
      temporary,
      targetTriple,
      hostPlatform,
    );
    rejectHardLink(temporary, "Temporary Kernel sidecar");
    renameSync(temporary, destination);
    rejectHardLink(destination, "Kernel sidecar destination");
    validatePreparedKernelSidecar(destination, targetTriple, hostPlatform);
    return validation;
  } finally {
    rmSync(temporary, { force: true });
  }
}

function rustcHostTriple(rustcVersion) {
  return rustcVersion
    ?.split(/\r?\n/u)
    .find((line) => line.startsWith("host: "))
    ?.slice("host: ".length)
    .trim();
}

function assertExecutableFormat(path, targetTriple) {
  const architecture = targetArchitecture(targetTriple);
  if (targetTriple.includes("darwin")) {
    assertMachOExecutable(path, architecture);
    return;
  }
  if (targetTriple.includes("linux")) {
    assertElfExecutable(path, architecture);
    return;
  }
  if (targetTriple.includes("windows")) {
    assertPeExecutable(path, architecture);
    return;
  }
  throw new Error(`Unsupported Kernel sidecar target: ${targetTriple}`);
}

function assertMachOExecutable(path, architecture) {
  const header = readBytes(path, 0, 32);
  const magic = header.subarray(0, 4).toString("hex");
  const thinEndian = {
    cefaedfe: "little",
    cffaedfe: "little",
    feedface: "big",
    feedfacf: "big",
  }[magic];
  if (thinEndian) {
    assertMachOHeader(path, 0, lstatSync(path).size, thinEndian, architecture);
    return;
  }

  const fat = {
    cafebabe: { endian: "big", width: 20 },
    cafebabf: { endian: "big", width: 32 },
    bebafeca: { endian: "little", width: 20 },
    bfbafeca: { endian: "little", width: 32 },
  }[magic];
  if (!fat || header.length < 8) {
    throw new Error(`Kernel sidecar must be a Mach-O executable: ${path}`);
  }

  const count = readUInt32(header, 4, fat.endian);
  if (count === 0 || count > 128) {
    throw new Error(`Kernel sidecar must be a valid Mach-O executable: ${path}`);
  }
  const expectedCpu = machCpuType(architecture);
  const fileSize = lstatSync(path).size;
  for (let index = 0; index < count; index += 1) {
    const entry = readBytes(path, 8 + index * fat.width, fat.width);
    if (entry.length < fat.width) break;
    const cpuType = readUInt32(entry, 0, fat.endian);
    if (cpuType !== expectedCpu) continue;
    const sliceOffset = fat.width === 32
      ? Number(readUInt64(entry, 8, fat.endian))
      : readUInt32(entry, 8, fat.endian);
    const sliceSize = fat.width === 32
      ? Number(readUInt64(entry, 16, fat.endian))
      : readUInt32(entry, 12, fat.endian);
    if (
      !Number.isSafeInteger(sliceOffset)
      || !Number.isSafeInteger(sliceSize)
      || sliceSize < 28
      || sliceOffset > fileSize
      || sliceSize > fileSize - sliceOffset
    ) {
      throw new Error(`Kernel sidecar Mach-O slice size is invalid: ${path}`);
    }
    const sliceHeader = readBytes(path, sliceOffset, 32);
    const sliceMagic = sliceHeader.subarray(0, 4).toString("hex");
    const sliceEndian = {
      cefaedfe: "little",
      cffaedfe: "little",
      feedface: "big",
      feedfacf: "big",
    }[sliceMagic];
    if (!sliceEndian) {
      throw new Error(`Kernel sidecar must contain a valid Mach-O executable slice: ${path}`);
    }
    assertMachOHeader(path, sliceOffset, sliceSize, sliceEndian, architecture);
    return;
  }
  throw architectureError(path, architecture);
}

function assertMachOHeader(path, offset, sliceSize, endian, architecture) {
  const header = readBytes(path, offset, 32);
  const magic = header.subarray(0, 4).toString("hex");
  const is64Bit = ["cffaedfe", "feedfacf"].includes(magic);
  const headerSize = is64Bit ? 32 : 28;
  if (header.length < headerSize || sliceSize < headerSize) {
    throw new Error(`Kernel sidecar must be a Mach-O executable: ${path}`);
  }
  if (readUInt32(header, 4, endian) !== machCpuType(architecture)) {
    throw architectureError(path, architecture);
  }
  if (readUInt32(header, 12, endian) !== 2) {
    throw new Error(`Kernel sidecar Mach-O file type must be executable: ${path}`);
  }
  const commandCount = readUInt32(header, 16, endian);
  const commandBytes = readUInt32(header, 20, endian);
  if (
    commandCount === 0
    || commandCount > 4096
    || commandBytes === 0
    || commandBytes > sliceSize - headerSize
  ) {
    throw new Error(`Kernel sidecar Mach-O load commands are invalid: ${path}`);
  }
  const commands = readBytes(path, offset + headerSize, commandBytes);
  if (commands.length !== commandBytes) {
    throw new Error(`Kernel sidecar Mach-O load commands are truncated: ${path}`);
  }
  let commandOffset = 0;
  let hasExecutableSegment = false;
  for (let index = 0; index < commandCount; index += 1) {
    if (commandOffset + 8 > commands.length) {
      throw new Error(`Kernel sidecar Mach-O load commands are truncated: ${path}`);
    }
    const command = readUInt32(commands, commandOffset, endian);
    const commandSize = readUInt32(commands, commandOffset + 4, endian);
    if (commandSize < 8 || commandSize > commands.length - commandOffset) {
      throw new Error(`Kernel sidecar Mach-O load command size is invalid: ${path}`);
    }
    const segmentSize = command === 0x19 ? 72 : command === 0x1 ? 56 : 0;
    if (segmentSize > 0) {
      if (commandSize < segmentSize) {
        throw new Error(`Kernel sidecar Mach-O segment command is truncated: ${path}`);
      }
      const initialProtectionOffset = command === 0x19 ? 60 : 44;
      if ((readUInt32(commands, commandOffset + initialProtectionOffset, endian) & 0x4) !== 0) {
        const virtualSize = command === 0x19
          ? readUInt64(commands, commandOffset + 32, endian)
          : BigInt(readUInt32(commands, commandOffset + 28, endian));
        const fileOffset = command === 0x19
          ? readUInt64(commands, commandOffset + 40, endian)
          : BigInt(readUInt32(commands, commandOffset + 32, endian));
        const fileSize = command === 0x19
          ? readUInt64(commands, commandOffset + 48, endian)
          : BigInt(readUInt32(commands, commandOffset + 36, endian));
        if (
          fileSize === 0n
          || virtualSize < fileSize
          || fileOffset > BigInt(sliceSize)
          || fileSize > BigInt(sliceSize) - fileOffset
        ) {
          throw new Error(`Kernel sidecar Mach-O executable segment payload is invalid: ${path}`);
        }
        hasExecutableSegment = true;
      }
    }
    commandOffset += commandSize;
  }
  if (commandOffset !== commands.length || !hasExecutableSegment) {
    throw new Error(`Kernel sidecar Mach-O must contain executable load commands: ${path}`);
  }
}

function assertElfExecutable(path, architecture) {
  const header = readBytes(path, 0, 64);
  if (!header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) {
    throw new Error(`Kernel sidecar must be an ELF executable: ${path}`);
  }
  const elfClass = header[4];
  const dataEncoding = header[5];
  const endian = dataEncoding === 1 ? "little" : dataEncoding === 2 ? "big" : undefined;
  const headerSize = elfClass === 2 ? 64 : 52;
  if (!endian || ![1, 2].includes(elfClass) || header.length < headerSize) {
    throw new Error(`Kernel sidecar must be a valid ELF executable: ${path}`);
  }
  const expected = elfArchitecture(architecture);
  if (elfClass !== expected.elfClass || readUInt16(header, 18, endian) !== expected.machine) {
    throw architectureError(path, architecture);
  }
  const fileType = readUInt16(header, 16, endian);
  if (![2, 3].includes(fileType)) {
    throw new Error(`Kernel sidecar ELF file type must be executable: ${path}`);
  }
  const entryPoint = elfClass === 2
    ? readUInt64(header, 24, endian)
    : BigInt(readUInt32(header, 24, endian));
  if (fileType === 3 && entryPoint === 0n) {
    throw new Error(`Kernel sidecar ELF shared object must have an executable entry point: ${path}`);
  }
  const declaredHeaderSize = readUInt16(header, elfClass === 2 ? 52 : 40, endian);
  const programOffsetValue = elfClass === 2
    ? readUInt64(header, 32, endian)
    : BigInt(readUInt32(header, 28, endian));
  const programEntrySize = readUInt16(header, elfClass === 2 ? 54 : 42, endian);
  const programCount = readUInt16(header, elfClass === 2 ? 56 : 44, endian);
  const expectedProgramEntrySize = elfClass === 2 ? 56 : 32;
  const fileSize = lstatSync(path).size;
  if (
    declaredHeaderSize < headerSize
    || programOffsetValue > BigInt(Number.MAX_SAFE_INTEGER)
    || programEntrySize < expectedProgramEntrySize
    || programCount === 0
  ) {
    throw new Error(`Kernel sidecar ELF program headers are invalid: ${path}`);
  }
  const programOffset = Number(programOffsetValue);
  const programBytes = programEntrySize * programCount;
  if (programOffset > fileSize || programBytes > fileSize - programOffset) {
    throw new Error(`Kernel sidecar ELF program headers are truncated: ${path}`);
  }
  let hasExecutableLoad = false;
  for (let index = 0; index < programCount; index += 1) {
    const program = readBytes(
      path,
      programOffset + index * programEntrySize,
      programEntrySize,
    );
    const type = readUInt32(program, 0, endian);
    const flags = readUInt32(program, elfClass === 2 ? 4 : 24, endian);
    if (type === 1 && (flags & 0x1) !== 0) {
      const fileOffset = elfClass === 2
        ? readUInt64(program, 8, endian)
        : BigInt(readUInt32(program, 4, endian));
      const virtualAddress = elfClass === 2
        ? readUInt64(program, 16, endian)
        : BigInt(readUInt32(program, 8, endian));
      const payloadSize = elfClass === 2
        ? readUInt64(program, 32, endian)
        : BigInt(readUInt32(program, 16, endian));
      const memorySize = elfClass === 2
        ? readUInt64(program, 40, endian)
        : BigInt(readUInt32(program, 20, endian));
      if (
        payloadSize === 0n
        || memorySize < payloadSize
        || fileOffset > BigInt(fileSize)
        || payloadSize > BigInt(fileSize) - fileOffset
      ) {
        throw new Error(`Kernel sidecar ELF executable segment payload is invalid: ${path}`);
      }
      if (
        entryPoint >= virtualAddress
        && entryPoint < virtualAddress + memorySize
      ) {
        hasExecutableLoad = true;
      }
    }
  }
  if (!hasExecutableLoad) {
    throw new Error(`Kernel sidecar ELF entry point must map to an executable payload: ${path}`);
  }
}

function assertPeExecutable(path, architecture) {
  const dosHeader = readBytes(path, 0, 64);
  if (dosHeader.length < 64 || dosHeader.subarray(0, 2).toString("ascii") !== "MZ") {
    throw new Error(`Kernel sidecar must be a PE executable: ${path}`);
  }
  const peOffset = dosHeader.readUInt32LE(0x3c);
  const peHeader = readBytes(path, peOffset, 24);
  if (peHeader.length < 24 || !peHeader.subarray(0, 4).equals(Buffer.from("PE\0\0", "binary"))) {
    throw new Error(`Kernel sidecar must be a PE executable: ${path}`);
  }
  const expected = peArchitecture(architecture);
  if (peHeader.readUInt16LE(4) !== expected.machine) {
    throw architectureError(path, architecture);
  }
  const characteristics = peHeader.readUInt16LE(22);
  if ((characteristics & 0x0002) === 0) {
    throw new Error(`Kernel sidecar PE image must be executable: ${path}`);
  }
  if ((characteristics & 0x2000) !== 0) {
    throw new Error(`Kernel sidecar PE image must not be a DLL: ${path}`);
  }
  const sectionCount = peHeader.readUInt16LE(6);
  const optionalSize = peHeader.readUInt16LE(20);
  const minimumOptionalSize = expected.optionalMagic === 0x020b ? 112 : 96;
  const optionalHeader = readBytes(path, peOffset + 24, optionalSize);
  if (
    sectionCount === 0
    || sectionCount > 4096
    || optionalSize < minimumOptionalSize
    || optionalHeader.length !== optionalSize
  ) {
    throw new Error(`Kernel sidecar PE optional header is invalid or truncated: ${path}`);
  }
  if (optionalHeader.readUInt16LE(0) !== expected.optionalMagic) {
    throw architectureError(path, architecture);
  }
  if (optionalHeader.readUInt32LE(16) === 0) {
    throw new Error(`Kernel sidecar PE image must have an executable entry point: ${path}`);
  }
  const sectionTableOffset = peOffset + 24 + optionalSize;
  const sectionTableBytes = sectionCount * 40;
  const fileSize = lstatSync(path).size;
  if (
    sectionTableOffset > fileSize
    || sectionTableBytes > fileSize - sectionTableOffset
  ) {
    throw new Error(`Kernel sidecar PE section table is truncated: ${path}`);
  }
  let hasExecutableSection = false;
  for (let index = 0; index < sectionCount; index += 1) {
    const section = readBytes(path, sectionTableOffset + index * 40, 40);
    if ((section.readUInt32LE(36) & 0x20000000) !== 0) {
      const virtualSize = section.readUInt32LE(8);
      const virtualAddress = section.readUInt32LE(12);
      const payloadSize = section.readUInt32LE(16);
      const payloadOffset = section.readUInt32LE(20);
      if (
        virtualSize === 0
        || payloadSize === 0
        || payloadOffset > fileSize
        || payloadSize > fileSize - payloadOffset
      ) {
        throw new Error(`Kernel sidecar PE executable section payload is invalid: ${path}`);
      }
      const sectionSpan = Math.max(virtualSize, payloadSize);
      const entryPoint = optionalHeader.readUInt32LE(16);
      if (
        entryPoint >= virtualAddress
        && entryPoint < virtualAddress + sectionSpan
      ) {
        hasExecutableSection = true;
      }
    }
  }
  if (!hasExecutableSection) {
    throw new Error(`Kernel sidecar PE entry point must map to an executable payload: ${path}`);
  }
}

function targetArchitecture(targetTriple) {
  return targetTriple.split("-", 1)[0];
}

function machCpuType(architecture) {
  const cpuType = {
    aarch64: 0x0100000c,
    x86_64: 0x01000007,
    i686: 7,
  }[architecture];
  if (cpuType === undefined) throw new Error(`Unsupported target architecture ${architecture}.`);
  return cpuType;
}

function elfArchitecture(architecture) {
  const entry = {
    aarch64: { elfClass: 2, machine: 183 },
    armv7: { elfClass: 1, machine: 40 },
    i686: { elfClass: 1, machine: 3 },
    x86_64: { elfClass: 2, machine: 62 },
  }[architecture];
  if (!entry) throw new Error(`Unsupported target architecture ${architecture}.`);
  return entry;
}

function peArchitecture(architecture) {
  const entry = {
    aarch64: { machine: 0xaa64, optionalMagic: 0x020b },
    i686: { machine: 0x014c, optionalMagic: 0x010b },
    x86_64: { machine: 0x8664, optionalMagic: 0x020b },
  }[architecture];
  if (!entry) throw new Error(`Unsupported target architecture ${architecture}.`);
  return entry;
}

function architectureError(path, architecture) {
  return new Error(`Kernel sidecar does not match target architecture ${architecture}: ${path}`);
}

function readUInt16(buffer, offset, endian) {
  return endian === "little" ? buffer.readUInt16LE(offset) : buffer.readUInt16BE(offset);
}

function readUInt32(buffer, offset, endian) {
  return endian === "little" ? buffer.readUInt32LE(offset) : buffer.readUInt32BE(offset);
}

function readUInt64(buffer, offset, endian) {
  return endian === "little" ? buffer.readBigUInt64LE(offset) : buffer.readBigUInt64BE(offset);
}

function readBytes(path, position, length) {
  const descriptor = openSync(path, "r");
  try {
    const buffer = Buffer.alloc(length);
    const bytesRead = readSync(descriptor, buffer, 0, length, position);
    return buffer.subarray(0, bytesRead);
  } finally {
    closeSync(descriptor);
  }
}

function rejectDestinationLink(path) {
  try {
    const metadata = lstatSync(path);
    if (metadata.isSymbolicLink()) {
      throw new Error(`Kernel sidecar destination must not be a symbolic link: ${path}`);
    }
    if (!metadata.isFile()) throw new Error(`Kernel sidecar destination must be a regular file: ${path}`);
    rejectHardLink(path, "Kernel sidecar destination");
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

function rejectDestinationDirectorySymlink(path) {
  const directory = dirname(path);
  const metadata = lstatSync(directory);
  if (metadata.isSymbolicLink()) {
    throw new Error(`Kernel sidecar destination directory must not be a symbolic link: ${directory}`);
  }
  if (!metadata.isDirectory()) {
    throw new Error(`Kernel sidecar destination directory must be a directory: ${directory}`);
  }
}

function rejectHardLink(path, label) {
  const metadata = lstatSync(path);
  if (typeof metadata.nlink === "number" && metadata.nlink > 1) {
    throw new Error(`${label} must not be a hard link: ${path}`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const prepared = prepareKernelSidecar();
  process.stdout.write(`Prepared ${prepared.destination} (${prepared.byteLength} bytes)\n`);
}
