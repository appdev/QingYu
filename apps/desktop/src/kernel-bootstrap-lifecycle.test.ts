import { createNativeKernelBootstrapLifecycleOwner } from "./kernel-bootstrap";

const INSTANCE_A = "123e4567-e89b-42d3-a456-426614174000";
const INSTANCE_B = "123e4567-e89b-42d3-a456-426614174001";
const CREDENTIAL_A = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CREDENTIAL_B = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";
const CREDENTIAL_C = "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCA";

describe("native Kernel bootstrap lifecycle owner", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("rebinds a recovered ready generation and permanently retires the old lease", async () => {
    const responses = [
      readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "7",
        instanceId: INSTANCE_A,
        port: 49_152
      }),
      readyBootstrap({
        credential: CREDENTIAL_B,
        generation: "8",
        instanceId: INSTANCE_B,
        port: 49_153
      })
    ];
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => responses.shift())
    });

    await expect(owner.refresh()).resolves.toEqual({
      changed: true,
      snapshot: {
        baseUrl: "http://127.0.0.1:49152/",
        generation: "7",
        instanceId: INSTANCE_A,
        status: "ready"
      }
    });
    const first = owner.acquireReady();
    expect(first?.authentication.getCredential()).toBe(CREDENTIAL_A);

    await expect(owner.refresh()).resolves.toEqual({
      changed: true,
      snapshot: {
        baseUrl: "http://127.0.0.1:49153/",
        generation: "8",
        instanceId: INSTANCE_B,
        status: "ready"
      }
    });
    const recovered = owner.acquireReady();

    expect(recovered).not.toBe(first);
    expect(recovered?.authentication.getCredential()).toBe(CREDENTIAL_B);
    expect(() => first?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    first?.release();
    recovered?.release();
    owner.close();
  });

  it.each([
    ["starting", "11"],
    ["retrying", "12"],
    ["failed", "13"],
    ["dormant", "14"]
  ] as const)("preserves the valid %s lifecycle state without offering a connection", async (status, generation) => {
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: async () => ({ status, bootstrapVersion: 1, generation })
    });

    await expect(owner.refresh()).resolves.toEqual({
      changed: true,
      snapshot: { status, generation }
    });
    expect(owner.acquireReady()).toBeNull();
    owner.close();
  });

  it("keeps the active lease when an identical ready publication is observed again", async () => {
    const publication = readyBootstrap({
      credential: CREDENTIAL_A,
      generation: "21",
      instanceId: INSTANCE_A,
      port: 49_152
    });
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => ({ ...publication }))
    });

    await owner.refresh();
    const active = owner.acquireReady();

    await expect(owner.refresh()).resolves.toEqual({
      changed: false,
      snapshot: {
        baseUrl: "http://127.0.0.1:49152/",
        generation: "21",
        instanceId: INSTANCE_A,
        status: "ready"
      }
    });
    expect(active?.authentication.getCredential()).toBe(CREDENTIAL_A);
    owner.close();
  });

  it("withdraws a ready lease during retry and rebinds when that generation becomes ready", async () => {
    const responses = [
      readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "41",
        instanceId: INSTANCE_A,
        port: 49_152
      }),
      { status: "retrying", bootstrapVersion: 1, generation: "42" },
      readyBootstrap({
        credential: CREDENTIAL_B,
        generation: "42",
        instanceId: INSTANCE_B,
        port: 49_153
      })
    ];
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => responses.shift())
    });

    await owner.refresh();
    const previous = owner.acquireReady();

    await expect(owner.refresh()).resolves.toEqual({
      changed: true,
      snapshot: { status: "retrying", generation: "42" }
    });
    expect(owner.acquireReady()).toBeNull();
    expect(() => previous?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );

    await expect(owner.refresh()).resolves.toMatchObject({
      changed: true,
      snapshot: { status: "ready", generation: "42" }
    });
    expect(owner.acquireReady()?.authentication.getCredential()).toBe(CREDENTIAL_B);
    owner.close();
  });

  it.each([
    ["credential", CREDENTIAL_C, 49_152],
    ["base URL", CREDENTIAL_A, 49_154]
  ] as const)("rebinds when the ready %s changes within one generation", async (_name, credential, port) => {
    const responses = [
      readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "22",
        instanceId: INSTANCE_A,
        port: 49_152
      }),
      readyBootstrap({ credential, generation: "22", instanceId: INSTANCE_A, port })
    ];
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => responses.shift())
    });

    await owner.refresh();
    const retired = owner.acquireReady();

    await expect(owner.refresh()).resolves.toMatchObject({ changed: true });
    expect(() => retired?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    expect(owner.acquireReady()?.authentication.getCredential()).toBe(credential);
    owner.close();
  });

  it("does not announce a repeated non-ready lifecycle state as a connection change", async () => {
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: async () => ({ status: "retrying", bootstrapVersion: 1, generation: "23" })
    });

    await owner.refresh();
    await expect(owner.refresh()).resolves.toEqual({
      changed: false,
      snapshot: { status: "retrying", generation: "23" }
    });
    owner.close();
  });

  it("gives multiple consumers independent leases while retaining one credential owner", async () => {
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: async () => readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "24",
        instanceId: INSTANCE_A,
        port: 49_152
      })
    });

    await owner.refresh();
    const documents = owner.acquireReady();
    const events = owner.acquireReady();

    expect(documents).not.toBe(events);
    documents?.release();
    documents?.release();
    expect(() => documents?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    expect(events?.authentication.getCredential()).toBe(CREDENTIAL_A);

    owner.close();
    owner.close();
    expect(() => events?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
  });

  it("fails closed and retires an active lease when a refresh response is invalid", async () => {
    const responses = [
      readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "25",
        instanceId: INSTANCE_A,
        port: 49_152
      }),
      {
        ...readyBootstrap({
          credential: CREDENTIAL_C,
          generation: "26",
          instanceId: INSTANCE_B,
          port: 49_153
        }),
        unexpected: CREDENTIAL_C
      }
    ];
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => responses.shift())
    });

    await owner.refresh();
    const active = owner.acquireReady();

    let thrown: unknown;
    try {
      await owner.refresh();
    } catch (cause: unknown) {
      thrown = cause;
    }

    expect(String(thrown)).toBe("Error: invalid native Kernel bootstrap");
    expect(String(thrown)).not.toContain(CREDENTIAL_C);
    expect(owner.acquireReady()).toBeNull();
    expect(() => active?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    owner.close();
  });

  it("serializes concurrent refreshes so a late older response cannot replace recovery", async () => {
    let resolveFirst: ((value: unknown) => undefined) | undefined;
    const firstResponse = new Promise<unknown>((resolve) => {
      resolveFirst = (value) => {
        resolve(value);
        return undefined;
      };
    });
    let calls = 0;
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => {
        calls += 1;
        if (calls === 1) return firstResponse;
        return readyBootstrap({
          credential: CREDENTIAL_B,
          generation: "31",
          instanceId: INSTANCE_B,
          port: 49_153
        });
      })
    });

    const first = owner.refresh();
    const recovery = owner.refresh();
    resolveFirst?.(readyBootstrap({
      credential: CREDENTIAL_A,
      generation: "30",
      instanceId: INSTANCE_A,
      port: 49_152
    }));

    await Promise.all([first, recovery]);
    expect(owner.acquireReady()).toMatchObject({
      baseUrl: "http://127.0.0.1:49153/",
      generation: "31",
      instanceId: INSTANCE_B
    });
    owner.close();
  });

  it("fails closed across regressed generations until a strictly newer generation arrives", async () => {
    const responses = [
      readyBootstrap({
        credential: CREDENTIAL_B,
        generation: "33",
        instanceId: INSTANCE_B,
        port: 49_153
      }),
      readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "32",
        instanceId: INSTANCE_A,
        port: 49_152
      }),
      readyBootstrap({
        credential: CREDENTIAL_B,
        generation: "33",
        instanceId: INSTANCE_B,
        port: 49_153
      }),
      readyBootstrap({
        credential: CREDENTIAL_A,
        generation: "32",
        instanceId: INSTANCE_A,
        port: 49_152
      }),
      readyBootstrap({
        credential: CREDENTIAL_C,
        generation: "34",
        instanceId: INSTANCE_A,
        port: 49_154
      })
    ];
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => responses.shift())
    });

    await owner.refresh();
    const current = owner.acquireReady();

    let firstRegression: unknown;
    try {
      await owner.refresh();
    } catch (cause: unknown) {
      firstRegression = cause;
    }

    expect(String(firstRegression)).toBe(
      "Error: native Kernel bootstrap generation regressed"
    );
    expect(String(firstRegression)).not.toContain(CREDENTIAL_A);
    expect(String(firstRegression)).not.toContain(CREDENTIAL_B);
    expect(() => current?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    expect(owner.acquireReady()).toBeNull();

    await expect(owner.refresh()).rejects.toThrow(
      "native Kernel bootstrap generation regressed"
    );
    expect(owner.acquireReady()).toBeNull();

    await expect(owner.refresh()).rejects.toThrow(
      "native Kernel bootstrap generation regressed"
    );
    expect(owner.acquireReady()).toBeNull();

    await expect(owner.refresh()).resolves.toEqual({
      changed: true,
      snapshot: {
        baseUrl: "http://127.0.0.1:49154/",
        generation: "34",
        instanceId: INSTANCE_A,
        status: "ready"
      }
    });
    expect(owner.acquireReady()?.authentication.getCredential()).toBe(CREDENTIAL_C);
    owner.close();
  });

  it("redacts provider failures and never writes a credential to browser or logging surfaces", async () => {
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    const pushState = vi.spyOn(window.history, "pushState");
    const replaceState = vi.spyOn(window.history, "replaceState");
    const dispatchEvent = vi.spyOn(window, "dispatchEvent");
    const log = vi.spyOn(console, "log");
    const info = vi.spyOn(console, "info");
    const warn = vi.spyOn(console, "warn");
    const error = vi.spyOn(console, "error");
    const debug = vi.spyOn(console, "debug");
    let calls = 0;
    const owner = createNativeKernelBootstrapLifecycleOwner({
      invokeCommand: vi.fn(async () => {
        calls += 1;
        if (calls === 1) {
          return readyBootstrap({
            credential: CREDENTIAL_C,
            generation: "40",
            instanceId: INSTANCE_A,
            port: 49_152
          });
        }
        throw new Error(`native provider exposed ${CREDENTIAL_C}`);
      })
    });

    const update = await owner.refresh();
    const lease = owner.acquireReady();
    expect(lease?.authentication.getCredential()).toBe(CREDENTIAL_C);
    expect(JSON.stringify({ owner, update, lease })).not.toContain(CREDENTIAL_C);

    let thrown: unknown;
    try {
      await owner.refresh();
    } catch (cause: unknown) {
      thrown = cause;
    }

    expect(String(thrown)).toBe("Error: native Kernel bootstrap refresh failed");
    expect(String(thrown)).not.toContain(CREDENTIAL_C);
    expect(storageWrite).not.toHaveBeenCalled();
    expect(pushState).not.toHaveBeenCalled();
    expect(replaceState).not.toHaveBeenCalled();
    expect(dispatchEvent).not.toHaveBeenCalled();
    expect(log).not.toHaveBeenCalled();
    expect(info).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    expect(debug).not.toHaveBeenCalled();
    expect(owner.acquireReady()).toBeNull();
    expect(() => lease?.authentication.getCredential()).toThrow(
      "native Kernel credential unavailable"
    );
    owner.close();
  });
});

function readyBootstrap({
  credential,
  generation,
  instanceId,
  port
}: {
  readonly credential: string;
  readonly generation: string;
  readonly instanceId: string;
  readonly port: number;
}) {
  return {
    status: "ready",
    bootstrapVersion: 1,
    generation,
    port,
    instanceId,
    credential
  };
}
