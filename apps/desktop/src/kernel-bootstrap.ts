import { invoke } from "@tauri-apps/api/core";

const NATIVE_KERNEL_BOOTSTRAP_COMMAND = "read_native_kernel_bootstrap";
const BOOTSTRAP_VERSION = 1;
const MAX_GENERATION = BigInt("18446744073709551615");
const CANONICAL_GENERATION = /^(?:0|[1-9][0-9]*)$/u;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;
const BASE64URL_CREDENTIAL = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/u;

const DORMANT_KEYS = ["bootstrapVersion", "status"] as const;
const READY_KEYS = [
  "bootstrapVersion",
  "credential",
  "generation",
  "instanceId",
  "port",
  "status"
] as const;

export type NativeKernelBootstrapInvoke = (command: string) => Promise<unknown>;

export interface NativeKernelAuthentication {
  readonly kind: "native-bearer";
  readonly getCredential: () => string;
}

export interface NativeKernelBootstrap {
  readonly authentication: NativeKernelAuthentication;
  readonly baseUrl: string;
  readonly generation: string;
  readonly instanceId: string;
  readonly release: () => undefined;
}

interface ReadyBootstrap {
  credential: string;
  generation: string;
  instanceId: string;
  port: number;
}

export async function readNativeKernelBootstrap(
  invokeCommand: NativeKernelBootstrapInvoke = invoke
): Promise<NativeKernelBootstrap | null> {
  const response = await invokeCommand(NATIVE_KERNEL_BOOTSTRAP_COMMAND);
  const record = asRecord(response);

  if (
    record.status === "dormant" &&
    record.bootstrapVersion === BOOTSTRAP_VERSION &&
    hasExactKeys(record, DORMANT_KEYS)
  ) {
    return null;
  }

  const ready = parseReadyBootstrap(record);
  return createNativeKernelBootstrap(ready);
}

function parseReadyBootstrap(record: Record<string, unknown>): ReadyBootstrap {
  if (
    record.status !== "ready" ||
    record.bootstrapVersion !== BOOTSTRAP_VERSION ||
    !hasExactKeys(record, READY_KEYS) ||
    !isCanonicalGeneration(record.generation) ||
    typeof record.port !== "number" ||
    !Number.isInteger(record.port) ||
    record.port < 1 ||
    record.port > 65_535 ||
    typeof record.instanceId !== "string" ||
    !UUID.test(record.instanceId) ||
    typeof record.credential !== "string" ||
    !BASE64URL_CREDENTIAL.test(record.credential)
  ) {
    throw invalidBootstrap();
  }

  return {
    credential: record.credential,
    generation: record.generation,
    instanceId: record.instanceId,
    port: record.port
  };
}

function createNativeKernelBootstrap(ready: ReadyBootstrap): NativeKernelBootstrap {
  let credential: string | undefined = ready.credential;
  const baseUrl = `http://127.0.0.1:${ready.port}/`;

  const getCredential = () => {
    if (credential === undefined) {
      throw new Error("native Kernel credential unavailable");
    }
    return credential;
  };
  const release = () => {
    credential = undefined;
    return undefined;
  };
  const authentication = Object.freeze(
    Object.defineProperties({}, {
      kind: { enumerable: true, value: "native-bearer" },
      getCredential: { value: getCredential }
    })
  ) as NativeKernelAuthentication;

  return Object.freeze(
    Object.defineProperties({}, {
      authentication: { enumerable: true, value: authentication },
      baseUrl: { enumerable: true, value: baseUrl },
      generation: { enumerable: true, value: ready.generation },
      instanceId: { enumerable: true, value: ready.instanceId },
      release: { value: release }
    })
  ) as NativeKernelBootstrap;
}

function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw invalidBootstrap();
  }
  return value as Record<string, unknown>;
}

function hasExactKeys(
  record: Record<string, unknown>,
  expected: readonly string[]
): boolean {
  const keys = Object.keys(record).sort();
  return keys.length === expected.length && keys.every((key, index) => key === expected[index]);
}

function isCanonicalGeneration(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= 20 &&
    CANONICAL_GENERATION.test(value) &&
    BigInt(value) <= MAX_GENERATION
  );
}

function invalidBootstrap(): Error {
  return new Error("invalid native Kernel bootstrap");
}
