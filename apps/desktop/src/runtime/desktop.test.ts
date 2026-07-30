import {
  createUnavailableKernelDomainPort,
  createUnavailableNativeShellPort
} from "@markra/app/runtime";
import { createDesktopRuntime } from "./desktop";

describe("desktop runtime composition", () => {
  it("injects domain and native-shell adapters by identity", () => {
    const kernel = createUnavailableKernelDomainPort();
    const nativeShell = createUnavailableNativeShellPort();

    const runtime = createDesktopRuntime({ kernel, nativeShell });

    expect(runtime.kernel).toBe(kernel);
    expect(runtime.nativeShell).toBe(nativeShell);
  });
});
