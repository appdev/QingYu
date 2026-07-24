import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { SettingsWindow } from "./SettingsWindow";
import { installAppTestHarness } from "../test/app-harness";
import { configureAppRuntime, getAppRuntime } from "../runtime";

installAppTestHarness();

describe("SettingsWindow notes workspace", () => {
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
            autoSyncOnSave: true,
            enabled: false,
            intervalMinutes: 15,
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
            version: 2,
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
});
