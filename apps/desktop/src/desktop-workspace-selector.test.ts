import { selectDesktopWorkspaceDirectory } from "./desktop-workspace-selector";

describe("desktop workspace selector", () => {
  it("opens exactly one directory and returns its native path", async () => {
    const openDirectory = vi.fn(async () => "/notes/workspace");

    await expect(selectDesktopWorkspaceDirectory(openDirectory)).resolves.toBe(
      "/notes/workspace",
    );
    expect(openDirectory).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
    });
  });

  it.each([null, ["/notes/one", "/notes/two"]])(
    "treats a non-single-path result as cancellation",
    async (selection) => {
      const openDirectory = vi.fn(async () => selection);

      await expect(selectDesktopWorkspaceDirectory(openDirectory)).resolves.toBeNull();
    },
  );
});
