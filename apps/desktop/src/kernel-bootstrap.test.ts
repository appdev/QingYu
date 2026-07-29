import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { readNativeKernelBootstrap } from "./kernel-bootstrap";

const INSTANCE_ID = "123e4567-e89b-42d3-a456-426614174000";
const CREDENTIAL = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

const READY_BOOTSTRAP = {
  status: "ready",
  bootstrapVersion: 1,
  generation: "1",
  port: 49_152,
  instanceId: INSTANCE_ID,
  credential: CREDENTIAL
};

describe("native Kernel bootstrap reader", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("maps the exact dormant v1 response to null", async () => {
    const invoke = vi.fn(async () => ({ status: "dormant", bootstrapVersion: 1 }));

    await expect(readNativeKernelBootstrap(invoke)).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("read_native_kernel_bootstrap");
  });

  it("remains disconnected from every production application module", () => {
    const productionSources = collectProductionSources("src").filter(
      (path) => path !== join("src", "kernel-bootstrap.ts")
    );

    for (const path of productionSources) {
      const source = readFileSync(path, "utf8");
      expect(source, path).not.toContain("kernel-bootstrap");
      expect(source, path).not.toContain("readNativeKernelBootstrap");
      expect(source, path).not.toContain("read_native_kernel_bootstrap");
    }
  });

  it("exposes a fixed loopback endpoint while keeping the credential only behind a closure", async () => {
    const invoke = vi.fn(async () => READY_BOOTSTRAP);

    const bootstrap = await readNativeKernelBootstrap(invoke);

    expect(bootstrap).not.toBeNull();
    expect(bootstrap).toMatchObject({
      authentication: { kind: "native-bearer" },
      baseUrl: "http://127.0.0.1:49152/",
      generation: "1",
      instanceId: INSTANCE_ID
    });
    expect(bootstrap?.authentication.getCredential()).toBe(CREDENTIAL);
    expect(Object.isFrozen(bootstrap?.authentication)).toBe(true);
    expect(Object.keys(bootstrap ?? {})).toEqual([
      "authentication",
      "baseUrl",
      "generation",
      "instanceId"
    ]);
    expect(Object.getOwnPropertyNames(bootstrap ?? {})).not.toContain("credential");
    expect(JSON.stringify(bootstrap)).toBe(
      `{"authentication":{"kind":"native-bearer"},"baseUrl":"http://127.0.0.1:49152/","generation":"1","instanceId":"${INSTANCE_ID}"}`
    );
  });

  it("does not copy the credential into storage, navigation, logs, or events", async () => {
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    const pushState = vi.spyOn(window.history, "pushState");
    const replaceState = vi.spyOn(window.history, "replaceState");
    const dispatchEvent = vi.spyOn(window, "dispatchEvent");
    const log = vi.spyOn(console, "log");
    const info = vi.spyOn(console, "info");
    const warn = vi.spyOn(console, "warn");
    const error = vi.spyOn(console, "error");
    const debug = vi.spyOn(console, "debug");

    const bootstrap = await readNativeKernelBootstrap(async () => READY_BOOTSTRAP);
    expect(bootstrap?.authentication.getCredential()).toBe(CREDENTIAL);

    expect(storageWrite).not.toHaveBeenCalled();
    expect(pushState).not.toHaveBeenCalled();
    expect(replaceState).not.toHaveBeenCalled();
    expect(dispatchEvent).not.toHaveBeenCalled();
    expect(log).not.toHaveBeenCalled();
    expect(info).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    expect(debug).not.toHaveBeenCalled();
    expect(window.location.search).toBe("");
  });

  it("makes the credential permanently unavailable after release", async () => {
    const bootstrap = await readNativeKernelBootstrap(async () => READY_BOOTSTRAP);

    bootstrap?.release();
    bootstrap?.release();

    expect(() => bootstrap?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    expect(JSON.stringify(bootstrap)).not.toContain(CREDENTIAL);
  });

  it.each([
    ["null", null],
    ["array", []],
    ["unknown status", { status: "starting", bootstrapVersion: 1 }],
    ["wrong dormant version", { status: "dormant", bootstrapVersion: 2 }],
    ["extra dormant field", { status: "dormant", bootstrapVersion: 1, port: 49_152 }],
    ["missing ready field", { ...READY_BOOTSTRAP, instanceId: undefined }],
    ["extra ready field", { ...READY_BOOTSTRAP, endpoint: "private" }],
    ["wrong ready version", { ...READY_BOOTSTRAP, bootstrapVersion: "1" }]
  ])("rejects a %s response instead of accepting a non-exact bootstrap shape", async (_name, value) => {
    await expect(readNativeKernelBootstrap(async () => value)).rejects.toThrow(
      "invalid native Kernel bootstrap"
    );
  });

  it.each([
    ["number", 1],
    ["empty", ""],
    ["leading zero", "01"],
    ["sign", "+1"],
    ["decimal point", "1.0"],
    ["negative", "-1"],
    ["outside u64", "18446744073709551616"]
  ])("rejects a non-canonical %s generation", async (_name, generation) => {
    await expect(
      readNativeKernelBootstrap(async () => ({ ...READY_BOOTSTRAP, generation }))
    ).rejects.toThrow("invalid native Kernel bootstrap");
  });

  it("rejects an overlong generation before attempting arbitrary-precision parsing", async () => {
    const parseBigInt = vi.fn(() => {
      throw new Error("overlong generation reached BigInt");
    });
    vi.stubGlobal("BigInt", parseBigInt);

    await expect(
      readNativeKernelBootstrap(async () => ({
        ...READY_BOOTSTRAP,
        generation: "1".repeat(21)
      }))
    ).rejects.toThrow("invalid native Kernel bootstrap");
    expect(parseBigInt).not.toHaveBeenCalled();
  });

  it.each([0, 65_536, 1.5, "49152", Number.NaN, Number.POSITIVE_INFINITY])(
    "rejects invalid port %s",
    async (port) => {
      await expect(
        readNativeKernelBootstrap(async () => ({ ...READY_BOOTSTRAP, port }))
      ).rejects.toThrow("invalid native Kernel bootstrap");
    }
  );

  it.each([
    "123e4567e89b42d3a456426614174000",
    "123e4567-e89b-42d3-a456-42661417400z",
    "123e4567-e89b-42d3-a456-426614174000-extra"
  ])("rejects invalid instance UUID %s", async (instanceId) => {
    await expect(
      readNativeKernelBootstrap(async () => ({ ...READY_BOOTSTRAP, instanceId }))
    ).rejects.toThrow("invalid native Kernel bootstrap");
  });

  it.each([
    CREDENTIAL.slice(1),
    `${CREDENTIAL}A`,
    `${CREDENTIAL.slice(0, -1)}+`,
    `${CREDENTIAL.slice(0, -1)}/`,
    `${CREDENTIAL.slice(0, -1)}=`,
    `${CREDENTIAL.slice(0, -1)}B`
  ])("rejects an invalid base64url credential without exposing it in the error", async (credential) => {
    let thrown: unknown;
    try {
      await readNativeKernelBootstrap(async () => ({ ...READY_BOOTSTRAP, credential }));
    } catch (cause: unknown) {
      thrown = cause;
    }

    expect(thrown).toBeInstanceOf(Error);
    expect(String(thrown)).toBe("Error: invalid native Kernel bootstrap");
    expect(String(thrown)).not.toContain(credential);
  });

  it.each("AEIMQUYcgkosw048".split(""))(
    "accepts canonical 32-byte base64url credential tail %s",
    async (tail) => {
      const bootstrap = await readNativeKernelBootstrap(async () => ({
        ...READY_BOOTSTRAP,
        credential: `${CREDENTIAL.slice(0, -1)}${tail}`
      }));

      expect(bootstrap?.authentication.getCredential().endsWith(tail)).toBe(true);
      bootstrap?.release();
    }
  );
});

function collectProductionSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      return collectProductionSources(path);
    }
    if (!/\.tsx?$/u.test(entry.name) || /\.test\.tsx?$/u.test(entry.name)) {
      return [];
    }
    return [path];
  });
}
