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
          name: "Archive"
        }]),
        load: async () => ({
          config: {
            enabled: true,
            intervalSeconds: 900,
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
});
