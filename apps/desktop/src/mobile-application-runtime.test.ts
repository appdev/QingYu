import {
  createUnavailableKernelDomainPort,
  type AppRuntime,
  type KernelDomainPort,
} from "@markra/app/runtime";

import { createProductionMobileApplicationMountOwner } from "./mobile-application-runtime";
import type {
  NativeKernelSessionOwner,
  NativeKernelSessionSnapshot,
} from "./runtime/native-kernel-session";

describe("production mobile application composition", () => {
  it("keeps the App unmounted until one authenticated mobile Kernel domain is ready", async () => {
    const kernel = {
      ...createUnavailableKernelDomainPort(),
      availability: "available",
    } as KernelDomainPort;
    const ready: NativeKernelSessionSnapshot = {
      domain: kernel,
      generation: "1",
      instanceId: "123e4567-e89b-42d3-a456-426614174000",
      status: "ready",
    };
    let subscriber: ((snapshot: NativeKernelSessionSnapshot | null) => unknown) | undefined;
    const owner: NativeKernelSessionOwner = {
      close: vi.fn(() => undefined),
      getSnapshot: () => null,
      start: vi.fn(async () => {
        subscriber?.(ready);
        return undefined;
      }),
      subscribe: (next) => {
        subscriber = next;
        return () => undefined;
      },
    };
    const configured: AppRuntime[] = [];
    const rendered: AppRuntime[] = [];
    const startup = vi.fn();
    const mount = createProductionMobileApplicationMountOwner({
      configureRuntime: (runtime) => configured.push(runtime),
      owner,
      renderDomain: ({ runtime }) => {
        rendered.push(runtime);
        return undefined;
      },
      renderStartup: startup,
    });

    await mount.start();

    expect(startup).toHaveBeenCalledOnce();
    expect(configured).toHaveLength(1);
    expect(configured[0]?.kernel).toBe(kernel);
    expect(rendered).toEqual(configured);
    mount.close();
  });
});
