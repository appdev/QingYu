import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { SettingsWindow } from "./SettingsWindow";
import {
  installAppTestHarness,
  mockedHideSettingsWindow
} from "../test/app-harness";
import { configureAppRuntime, getAppRuntime } from "../runtime";

const settingsPrimaryWorkspaceState = vi.hoisted(() => ({
  current: {
    canChooseDesktopRoot: true,
    commitDesktopRoot: vi.fn(async () => null),
    commitManagedRoot: vi.fn(async () => null),
    deferDesktopSetup: vi.fn(async () => undefined),
    error: null,
    managedName: null,
    resetOnboarding: vi.fn(async () => undefined),
    retry: vi.fn(async () => undefined),
    root: null as string | null,
    status: "deferred" as "deferred" | "ready",
    workspaceRoot: null as string | null
  }
}));

vi.mock("../hooks/usePrimaryWorkspace", () => ({
  usePrimaryWorkspace: () => settingsPrimaryWorkspaceState.current
}));

installAppTestHarness();

describe("SettingsWindow notes workspace", () => {
  beforeEach(() => {
    settingsPrimaryWorkspaceState.current = {
      ...settingsPrimaryWorkspaceState.current,
      root: null,
      status: "deferred",
      workspaceRoot: null
    };
  });

  it("shows the application workspace path read-only with an explicit notebook switch entry", async () => {
    render(<SettingsWindow />);

    fireEvent.click(await screen.findByRole("button", { name: "Notes Workspace" }));

    expect(screen.getByRole("heading", { name: "Notes Workspace" })).toBeInTheDocument();
    expect(screen.getByText("Not configured")).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Switch Notebook Directory" })).toBeInTheDocument();
  });

  it("chooses a notebook locally and sends a durable request to the primary window", async () => {
    const runtime = getAppRuntime();
    const requestPrimaryNotebookSwitch = vi.fn(async () => undefined);
    configureAppRuntime({
      ...runtime,
      files: {
        ...runtime.files,
        openMarkdownFolder: async () => ({ name: "Notes", path: "/Notes" }),
        requestPrimaryNotebookSwitch
      }
    });
    render(<SettingsWindow />);

    fireEvent.click(await screen.findByRole("button", { name: "Notes Workspace" }));
    fireEvent.click(screen.getByRole("button", { name: "Switch Notebook Directory" }));

    await waitFor(() => expect(requestPrimaryNotebookSwitch).toHaveBeenCalledWith("/Notes"));
  });

  it("renders the cloud notebook action from loaded Synchronization settings", async () => {
    const runtime = getAppRuntime();
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        load: async () => ({
          config: {
            enabled: false,
            intervalSeconds: 900,
            generateConflictDocument: false,
            mode: "automatic",
            provider: "webdav",
            remoteRoot: "qingyu/main",
            s3: {
              accessKeyId: "",
              bucket: "",
              endpointUrl: "",
              region: "",
              secretAccessKey: "",
              requestTimeoutSeconds: 60,
              addressingStyle: "auto",
              tlsVerification: "verify"
            },
            version: 3,
            webdav: {
              password: "password-value",
              serverUrl: "https://dav.example.test",
              username: "user-value"
            }
          },
          configured: true,
          issues: [],
          readiness: "disabled",
          revision: "rev-1",
          status: "loaded"
        })
      }
    });
    render(<SettingsWindow />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync" }));
    const selectCloudNotebook = await screen.findByRole("button", { name: "Select Cloud Notebook" });
    expect(selectCloudNotebook).toBeDisabled();
  });

  it("keeps Settings mounted while its cloud notebook dialog opens and cancels", async () => {
    settingsPrimaryWorkspaceState.current = {
      ...settingsPrimaryWorkspaceState.current,
      root: "/Workspace/Current",
      status: "ready",
      workspaceRoot: "/Workspace"
    };
    const runtime = getAppRuntime();
    configureAppRuntime({
      ...runtime,
      syncConfig: {
        ...runtime.syncConfig,
        listNotebooks: vi.fn(async () => [{
          available: true,
          disabledReason: null,
          displayName: "Archive",
          name: "Archive",
          provider: "webdav" as const,
          repositoryId: null
        }]),
        load: async () => ({
          config: {
            enabled: true,
            intervalSeconds: 900,
            generateConflictDocument: false,
            mode: "automatic",
            provider: "webdav",
            remoteRoot: "qingyu/main",
            s3: {
              accessKeyId: "",
              bucket: "",
              endpointUrl: "",
              region: "",
              secretAccessKey: "",
              requestTimeoutSeconds: 60,
              addressingStyle: "auto",
              tlsVerification: "verify"
            },
            version: 3,
            webdav: {
              password: "password-value",
              serverUrl: "https://dav.example.test",
              username: "user-value"
            }
          },
          configured: true,
          issues: [],
          readiness: "ready",
          revision: "rev-2",
          status: "loaded"
        })
      }
    });
    render(<SettingsWindow />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync" }));
    const settings = screen.getByRole("main", { name: "QingYu settings" });
    expect(await screen.findByText("/Workspace/Current")).toBeInTheDocument();
    const selectCloudNotebook = await screen.findByRole("button", { name: "Select Cloud Notebook" });
    await waitFor(() => expect(selectCloudNotebook).toBeEnabled());
    fireEvent.click(selectCloudNotebook);
    const dialog = await screen.findByRole("dialog", { name: "Restore notebook from cloud" });

    expect(settings).toBeInTheDocument();
    expect(within(dialog).getByRole("radio", { name: "Archive" })).toBeEnabled();
    expect(mockedHideSettingsWindow).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Restore notebook from cloud" }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Cloud Notebook" }))
      .toBeInTheDocument();
  });

  it("opens conflict history inside Settings with the actual local and remote contents", async () => {
    settingsPrimaryWorkspaceState.current = {
      ...settingsPrimaryWorkspaceState.current,
      root: "/Workspace/Current",
      status: "ready",
      workspaceRoot: "/Workspace"
    };
    const conflict = {
      conflictId: "00000000-0000-4000-8000-0000000000c2",
      occurredAt: "2026-07-29T02:42:00Z",
      relativePath: "notes/conflicted.md",
      repositoryId: "00000000-0000-4000-8000-0000000000c1",
      resolution: "keep-local" as const
    };
    const listeners = new Map<string, Set<(event: { payload: unknown }) => unknown>>();
    const loadRepositoryStatus = vi.fn(async () => ({
      attempt: 1,
      automaticFailureCount: 0,
      conflicts: [conflict],
      error: null,
      jobId: "00000000-0000-4000-8000-0000000000c3",
      lastAttemptAt: "2026-07-29T02:42:00Z",
      lastDnsRetryAt: null,
      lastSuccessfulSyncAt: "2026-07-29T02:42:00Z",
      maintenance: { lastLocalPurgeAt: null, nextLocalPurgeAt: null },
      nextScheduledAt: null,
      phase: "succeeded" as const,
      repositoryId: conflict.repositoryId,
      sameCount: 0,
      transfer: {
        downloadBytes: 0,
        downloadChunks: 0,
        downloadFiles: 0,
        uploadBytes: 0,
        uploadChunks: 0,
        uploadFiles: 0
      },
      trigger: "manual" as const,
      version: 1 as const
    }));
    const runtime = getAppRuntime();
    configureAppRuntime({
      ...runtime,
      events: {
        emit: async (event, payload) => {
          for (const listener of listeners.get(event) ?? []) await listener({ payload });
        },
        isAvailable: () => true,
        listen: async (event, listener) => {
          const registered = listeners.get(event) ?? new Set();
          registered.add(listener as (event: { payload: unknown }) => unknown);
          listeners.set(event, registered);
          return () => registered.delete(listener as (event: { payload: unknown }) => unknown);
        }
      },
      syncConfig: {
        ...runtime.syncConfig,
        load: async () => ({
          config: {
            enabled: true,
            generateConflictDocument: false,
            intervalSeconds: 900,
            mode: "fully-manual",
            provider: "s3",
            remoteRoot: "qingyu/test",
            s3: {
              accessKeyId: "",
              addressingStyle: "auto",
              bucket: "test",
              endpointUrl: "https://s3.example.test",
              region: "us-east-1",
              requestTimeoutSeconds: 60,
              secretAccessKey: "",
              tlsVerification: "verify"
            },
            version: 3,
            webdav: {
              password: "",
              serverUrl: "",
              username: ""
            }
          },
          configured: true,
          issues: [],
          readiness: "ready",
          revision: "rev-conflict",
          status: "loaded"
        }),
        loadRepositoryStatus,
        readDejavuConflictHistory: vi.fn(async () => ({
          conflict,
          local: { byteSize: 13, text: "local content" },
          remote: { byteSize: 14, text: "remote content" }
        }))
      }
    });
    const view = render(<SettingsWindow />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync" }));
    fireEvent.click(await screen.findByRole("button", { name: "notes/conflicted.md" }));

    const dialog = await screen.findByRole("dialog", { name: "Sync conflict history" });
    expect(within(dialog).getByText("local content")).toBeInTheDocument();
    expect(within(dialog).getByText("remote content")).toBeInTheDocument();
    expect(loadRepositoryStatus.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(mockedHideSettingsWindow).not.toHaveBeenCalled();

    settingsPrimaryWorkspaceState.current = {
      ...settingsPrimaryWorkspaceState.current,
      root: "/Workspace/Other"
    };
    view.rerender(<SettingsWindow />);
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Sync conflict history" }))
      .not.toBeInTheDocument());
  });
});
