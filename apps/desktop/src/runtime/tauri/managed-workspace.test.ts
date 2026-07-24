import { invoke } from "@tauri-apps/api/core";
import * as managedWorkspace from "./managed-workspace";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);

describe("managed workspace adapter", () => {
  it("lists exact native managed workspace names through the shallow command", async () => {
    const adapter = managedWorkspace as typeof managedWorkspace & {
      listNativeManagedWorkspaceNames?: () => Promise<string[]>;
    };
    mockedInvoke.mockResolvedValueOnce(["Alpha", "beta", "随笔"]);

    expect(adapter.listNativeManagedWorkspaceNames).toEqual(expect.any(Function));
    await expect(adapter.listNativeManagedWorkspaceNames?.()).resolves.toEqual([
      "Alpha",
      "beta",
      "随笔"
    ]);
    expect(mockedInvoke).toHaveBeenCalledWith("list_managed_workspace_names");
  });
});
