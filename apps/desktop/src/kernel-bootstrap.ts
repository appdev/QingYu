import { invoke } from "@tauri-apps/api/core";

const NATIVE_KERNEL_BOOTSTRAP_COMMAND = "read_native_kernel_bootstrap";
const BOOTSTRAP_VERSION = 1;
const MAX_GENERATION = BigInt("18446744073709551615");
const CANONICAL_GENERATION = /^(?:0|[1-9][0-9]*)$/u;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;
const BASE64URL_CREDENTIAL = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/u;

const DORMANT_KEYS = ["bootstrapVersion", "status"] as const;
const LIFECYCLE_KEYS = ["bootstrapVersion", "generation", "status"] as const;
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

export type NativeKernelBootstrapLifecycleSnapshot =
  | { readonly status: "dormant"; readonly generation?: string }
  | {
      readonly status: "starting" | "retrying" | "failed";
      readonly generation: string;
    }
  | {
      readonly status: "ready";
      readonly generation: string;
      readonly instanceId: string;
      readonly baseUrl: string;
    };

export interface NativeKernelBootstrapLifecycleUpdate {
  readonly changed: boolean;
  readonly snapshot: NativeKernelBootstrapLifecycleSnapshot;
}

export interface NativeKernelBootstrapLifecycleOwner {
  /** Re-read and atomically adopt the latest native publication. */
  refresh(): Promise<NativeKernelBootstrapLifecycleUpdate>;
  /**
   * Return a consumer-specific lease for the current ready publication.
   * Releasing one lease never releases another consumer's lease. Every lease
   * is revoked when refresh adopts a different publication or close is called.
   */
  acquireReady(): NativeKernelBootstrap | null;
  /** Permanently revoke the owned publication and all outstanding leases. */
  close(): undefined;
}

export interface NativeKernelBootstrapLifecycleOwnerOptions {
  readonly invokeCommand?: NativeKernelBootstrapInvoke;
}

interface OwnedReadyBootstrap {
  active: boolean;
  readonly source: NativeKernelBootstrap;
}

type ParsedNativeKernelBootstrap =
  | { readonly status: "dormant"; readonly generation?: string }
  | {
      readonly status: "starting" | "retrying" | "failed";
      readonly generation: string;
    }
  | { readonly status: "ready"; readonly bootstrap: NativeKernelBootstrap };

export function createNativeKernelBootstrapLifecycleOwner({
  invokeCommand = invoke
}: NativeKernelBootstrapLifecycleOwnerOptions = {}): NativeKernelBootstrapLifecycleOwner {
  let closed = false;
  let current: OwnedReadyBootstrap | undefined;
  let currentSnapshot: NativeKernelBootstrapLifecycleSnapshot | undefined;
  let highestGeneration: bigint | undefined;
  let recoveryFloor: bigint | undefined;
  let refreshTail: Promise<unknown> = Promise.resolve();

  const retireCurrent = () => {
    if (current === undefined) return undefined;
    const retired = current;
    current = undefined;
    retired.active = false;
    releaseBootstrap(retired.source);
    return undefined;
  };

  const performRefresh = async () => {
    if (closed) throw lifecycleOwnerClosed();
    let state: ParsedNativeKernelBootstrap;
    try {
      state = await readNativeKernelBootstrapState(invokeCommand);
    } catch (cause: unknown) {
      retireCurrent();
      currentSnapshot = undefined;
      if (cause instanceof Error && cause.message === "invalid native Kernel bootstrap") {
        throw cause;
      }
      throw refreshFailed();
    }
    if (closed) {
      if (state.status === "ready") releaseBootstrap(state.bootstrap);
      throw lifecycleOwnerClosed();
    }

    const generation = generationFor(state);
    if (
      generation !== undefined &&
      (
        (highestGeneration !== undefined && generation < highestGeneration) ||
        (recoveryFloor !== undefined && generation <= recoveryFloor)
      )
    ) {
      retireCurrent();
      currentSnapshot = undefined;
      if (highestGeneration !== undefined) recoveryFloor = highestGeneration;
      if (state.status === "ready") releaseBootstrap(state.bootstrap);
      throw generationRegressed();
    }
    if (
      generation !== undefined &&
      (highestGeneration === undefined || generation > highestGeneration)
    ) {
      highestGeneration = generation;
    }

    if (state.status !== "ready") {
      const snapshot = Object.freeze({ ...state });
      const changed = !sameLifecycleSnapshot(currentSnapshot, snapshot);
      retireCurrent();
      currentSnapshot = snapshot;
      return {
        changed,
        snapshot
      } satisfies NativeKernelBootstrapLifecycleUpdate;
    }

    const bootstrap = state.bootstrap;
    const snapshot = Object.freeze({
      status: "ready" as const,
      baseUrl: bootstrap.baseUrl,
      generation: bootstrap.generation,
      instanceId: bootstrap.instanceId
    });
    if (
      current !== undefined &&
      sameLifecycleSnapshot(currentSnapshot, snapshot) &&
      sameCredential(current.source, bootstrap)
    ) {
      releaseBootstrap(bootstrap);
      return { changed: false, snapshot };
    }

    retireCurrent();
    current = { active: true, source: bootstrap };
    currentSnapshot = snapshot;
    return {
      changed: true,
      snapshot
    } satisfies NativeKernelBootstrapLifecycleUpdate;
  };

  const refresh = () => {
    const pending = refreshTail.then(performRefresh, performRefresh);
    refreshTail = pending.then(
      () => undefined,
      () => undefined
    );
    return pending;
  };

  return Object.freeze({
    refresh,
    acquireReady: () => (
      current === undefined ? null : createNativeKernelBootstrapLease(current)
    ),
    close: () => {
      if (closed) return undefined;
      closed = true;
      retireCurrent();
      return undefined;
    }
  });
}

function releaseBootstrap(bootstrap: NativeKernelBootstrap): undefined {
  try {
    bootstrap.release();
  } catch {
    // Credential retirement is best-effort and never reports provider details.
  }
  return undefined;
}

function generationFor(state: ParsedNativeKernelBootstrap): bigint | undefined {
  if (state.status === "dormant") {
    return state.generation === undefined ? undefined : BigInt(state.generation);
  }
  if (state.status === "ready") return BigInt(state.bootstrap.generation);
  return BigInt(state.generation);
}

function sameLifecycleSnapshot(
  current: NativeKernelBootstrapLifecycleSnapshot | undefined,
  next: NativeKernelBootstrapLifecycleSnapshot
): boolean {
  if (current === undefined || current.status !== next.status) return false;
  if (current.status === "ready" && next.status === "ready") {
    return (
      current.baseUrl === next.baseUrl &&
      current.generation === next.generation &&
      current.instanceId === next.instanceId
    );
  }
  return current.generation === next.generation;
}

function sameCredential(
  current: NativeKernelBootstrap,
  next: NativeKernelBootstrap
): boolean {
  try {
    return current.authentication.getCredential() === next.authentication.getCredential();
  } catch {
    return false;
  }
}

export async function readNativeKernelBootstrap(
  invokeCommand: NativeKernelBootstrapInvoke = invoke
): Promise<NativeKernelBootstrap | null> {
  const state = await readNativeKernelBootstrapState(invokeCommand);
  return state.status === "ready" ? state.bootstrap : null;
}

async function readNativeKernelBootstrapState(
  invokeCommand: NativeKernelBootstrapInvoke
): Promise<ParsedNativeKernelBootstrap> {
  const response = await invokeCommand(NATIVE_KERNEL_BOOTSTRAP_COMMAND);
  const record = asRecord(response);

  if (
    record.status === "dormant" &&
    record.bootstrapVersion === BOOTSTRAP_VERSION &&
    hasExactKeys(record, DORMANT_KEYS)
  ) {
    return { status: "dormant" };
  }

  if (
    isUnavailableLifecycleStatus(record.status) &&
    record.bootstrapVersion === BOOTSTRAP_VERSION &&
    hasExactKeys(record, LIFECYCLE_KEYS) &&
    isCanonicalGeneration(record.generation)
  ) {
    return {
      status: record.status,
      generation: record.generation
    };
  }

  const ready = parseReadyBootstrap(record);
  return {
    status: "ready",
    bootstrap: createNativeKernelBootstrap(ready)
  };
}

function isUnavailableLifecycleStatus(
  value: unknown
): value is "dormant" | "starting" | "retrying" | "failed" {
  return (
    value === "dormant" ||
    value === "starting" ||
    value === "retrying" ||
    value === "failed"
  );
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
    if (credential === undefined) throw credentialUnavailable();
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

function createNativeKernelBootstrapLease(owned: OwnedReadyBootstrap): NativeKernelBootstrap {
  let active = true;
  const getCredential = () => {
    if (!active || !owned.active) throw credentialUnavailable();
    return owned.source.authentication.getCredential();
  };
  const release = () => {
    active = false;
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
      baseUrl: { enumerable: true, value: owned.source.baseUrl },
      generation: { enumerable: true, value: owned.source.generation },
      instanceId: { enumerable: true, value: owned.source.instanceId },
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

function credentialUnavailable(): Error {
  return new Error("native Kernel credential unavailable");
}

function lifecycleOwnerClosed(): Error {
  return new Error("native Kernel bootstrap lifecycle owner closed");
}

function generationRegressed(): Error {
  return new Error("native Kernel bootstrap generation regressed");
}

function refreshFailed(): Error {
  return new Error("native Kernel bootstrap refresh failed");
}
