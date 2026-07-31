import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  createUnavailableKernelDomainPort,
  kernelWorkspaceRoot,
  type KernelDomainPort,
} from "@markra/app/runtime";

import {
  createMobileKernelRuntimeOwner,
  mobileRuntime,
} from "./mobile";
import * as fileConfirm from "./tauri/file/confirm";
import * as mobileFiles from "./tauri/file/mobile";
import * as mobileBack from "./tauri/mobile-back";
import * as mcpPolicy from "./tauri/mcp-policy";
import * as themes from "./tauri/themes/shared";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-fs", () => ({ readFile: vi.fn() }));
vi.mock("@tauri-apps/plugin-log", () => ({
  error: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-store", () => ({ load: vi.fn() }));

const mockedConfirm = vi.mocked(confirm);
const mockedInvoke = vi.mocked(invoke);

describe("mobile Kernel runtime boundary", () => {
  beforeEach(() => {
    mockedConfirm.mockReset();
    mockedInvoke.mockReset();
  });

  it("keeps the pre-Kernel shell free of legacy document, settings, and sync adapters", () => {
    const source = readFileSync(resolve(process.cwd(), "src/runtime/mobile.ts"), "utf8");

    expect(source).toContain('from "./tauri/file/mobile"');
    expect(source).toContain('from "./tauri/file/confirm"');
    expect(source).not.toContain('from "./tauri/file/shared"');
    expect(source).not.toContain('from "./tauri/settings"');
    expect(source).not.toContain('from "./tauri/sync-config/shared"');
    expect(source).not.toContain('from "./tauri/managed-workspace"');
    expect(source).not.toContain('from "./tauri/web-resource"');
    expect(mobileRuntime.kernel.availability).toBe("unavailable");
    expect(mobileRuntime.mcp.policyAvailable).toBe(true);
    expect(mobileRuntime.mcp.localServiceAvailable).toBe(false);
  });

  it("composes document, settings, and sync over one ready Kernel port", async () => {
    const kernel = {
      ...createUnavailableKernelDomainPort(),
      availability: "available",
    } as KernelDomainPort;
    const owner = createMobileKernelRuntimeOwner(kernel);

    expect(owner.runtime.kernel).toBe(kernel);
    expect(owner.runtime.files.openLocalImages).toBe(mobileFiles.openMobileLocalImages);
    mockedConfirm.mockResolvedValue(true);
    await expect(owner.runtime.files.confirmMarkdownFileDelete("note.md", {
      cancelLabel: "Cancel",
      message: "Delete note?",
      okLabel: "Delete",
    })).resolves.toBe(true);
    expect(owner.runtime.settings.readGroup).toEqual(expect.any(Function));
    expect(owner.runtime.settings.writeGroup).toEqual(expect.any(Function));
    expect(owner.runtime.syncConfig.load).not.toBe(mobileRuntime.syncConfig.load);
    expect(owner.runtime.syncConfig.patch).not.toBe(mobileRuntime.syncConfig.patch);
    expect(owner.runtime.workspace.rootPolicy).toMatchObject({
      canChooseLocalRoot: false,
      kind: "fixed",
    });
    await expect(owner.runtime.workspace.rootPolicy?.kind === "fixed"
      ? owner.runtime.workspace.rootPolicy.resolveRoot()
      : Promise.reject(new Error("mobile root remained selectable")))
      .resolves.toBe(kernelWorkspaceRoot);

    owner.release();
    owner.release();
  });

  it("keeps native confirmation, image picker, system back, external links, and themes", async () => {
    mockedConfirm.mockResolvedValue(true);

    await expect(mobileRuntime.files.confirmMarkdownFileDelete("note.md", {
      cancelLabel: "Cancel",
      message: "Delete note?",
      okLabel: "Delete",
    })).resolves.toBe(true);

    expect(mobileRuntime.files.openLocalImages).toBe(mobileFiles.openMobileLocalImages);
    expect(mobileRuntime.navigation.subscribeToSystemBack)
      .toBe(mobileBack.subscribeToMobileSystemBack);
    expect(mobileRuntime.window.openExternalUrl).toEqual(expect.any(Function));
    expect(mobileRuntime.themes).toMatchObject({
      cancelActivation: themes.cancelNativeThemeActivation,
      commitActivation: themes.commitNativeThemeActivation,
      delete: themes.deleteNativeTheme,
      list: themes.listNativeThemes,
      prepareActivation: themes.prepareNativeThemeActivation,
      releaseActivation: themes.releaseNativeThemeActivation,
    });
  });

  it("keeps mobile MCP policy native without enabling the desktop local service", async () => {
    const config = { enabled: false } as Parameters<
      typeof mobileRuntime.mcp.updateSettings
    >[0]["config"];
    mockedInvoke
      .mockResolvedValueOnce({ config, revision: "revision-1" })
      .mockResolvedValueOnce({ config: { ...config, enabled: true }, revision: "revision-2" });

    await mobileRuntime.mcp.getSettings();
    await mobileRuntime.mcp.updateSettings({
      config: { ...config, enabled: true },
      expectedRevision: "revision-1",
    });

    expect(mockedInvoke.mock.calls).toEqual([
      ["get_mcp_policy"],
      ["update_mcp_policy", {
        input: {
          config: { ...config, enabled: true },
          expectedRevision: "revision-1",
        },
      }],
    ]);
  });
});
