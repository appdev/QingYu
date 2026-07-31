import {
  invokeMobileKernelBootstrap,
  retryMobileKernelRuntime,
} from "./mobile-kernel-session";

describe("mobile Kernel session bootstrap boundary", () => {
  it("maps only the shared lifecycle reader to the mobile memory-bootstrap command", async () => {
    const invoke = vi.fn(async () => ({
      bootstrapVersion: 1,
      generation: "1",
      status: "starting",
    }));

    await expect(invokeMobileKernelBootstrap(
      "read_native_kernel_bootstrap",
      invoke,
    )).resolves.toMatchObject({ status: "starting" });
    await expect(invokeMobileKernelBootstrap("read_app_settings_group", invoke))
      .rejects.toThrow("mobile Kernel bootstrap unavailable");
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("read_mobile_kernel_bootstrap");
  });

  it("uses the dedicated native retry command instead of reloading the WebView", async () => {
    const invoke = vi.fn(async () => undefined);

    await expect(retryMobileKernelRuntime(invoke)).resolves.toBeUndefined();

    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("retry_mobile_kernel_runtime");
  });
});
