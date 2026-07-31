import { createDesktopKernelImageSource } from "./kernel-image-source";

describe("desktop Kernel image source", () => {
  it("creates and revokes bearer-authenticated Blob URLs", async () => {
    const createObjectURL = vi.fn(() => "blob:desktop-kernel-image");
    const revokeObjectURL = vi.fn();
    const source = createDesktopKernelImageSource({ createObjectURL, revokeObjectURL });

    await expect(source.materialize({} as never, async () => ({
      body: new Blob(["image bytes"], { type: "image/png" }),
      mediaType: "image/png",
    }))).resolves.toBe("blob:desktop-kernel-image");
    expect(createObjectURL).toHaveBeenCalledWith(expect.any(Blob));

    source.release("blob:desktop-kernel-image");
    source.release("blob:desktop-kernel-image");
    expect(revokeObjectURL).toHaveBeenCalledOnce();
  });
});
