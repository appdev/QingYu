import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type {
  QingYuSyncConfig,
  SyncConfigDocument,
  SyncConfigPatch,
  SyncConfigLoadResult
} from "../../lib/sync-config";
import { translate } from "../../test/settings-components";
import { SyncSettings, type SyncSettingsProps } from "./SyncSettings";

const config: QingYuSyncConfig = {
  autoSyncOnSave: true,
  enabled: true,
  intervalMinutes: 15,
  provider: "webdav",
  remoteRoot: "qingyu/main",
  s3: {
    accessKeyId: "access-value",
    bucket: "notes-bucket",
    endpointUrl: "https://s3.example.test",
    region: "us-east-1",
    secretAccessKey: "secret-value",
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
};

function document(overrides: Partial<SyncConfigDocument> = {}): SyncConfigDocument {
  return {
    config,
    configured: true,
    issues: [],
    readiness: "ready",
    revision: "rev-1",
    ...overrides
  };
}

function loaded(overrides: Partial<SyncConfigDocument> = {}): SyncConfigLoadResult {
  return { ...document(overrides), status: "loaded" };
}

function createProps(overrides: Partial<SyncSettingsProps> = {}): SyncSettingsProps {
  const configDocument = document();
  return {
    configDocument,
    loadResult: { ...configDocument, status: "loaded" },
    primaryRoot: "/Notes",
    saving: false,
    status: null,
    syncRunning: false,
    testing: false,
    translate,
    onEnable: vi.fn(async () => undefined),
    onPatch: vi.fn(async (_patch: SyncConfigPatch) => undefined),
    onReset: vi.fn(async () => undefined),
    onRunSync: vi.fn(async () => undefined),
    onSelectCloudNotebook: vi.fn(async () => undefined),
    onTestConnection: vi.fn(async () => ({ checkedTarget: "dav.example.test", provider: "webdav" as const })),
    ...overrides
  };
}

describe("SyncSettings application scope", () => {
  it("groups S3 settings from basic choices through connection status", () => {
    const s3Document = document({ config: { ...config, provider: "s3" } });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" }
    })} />);

    expect(screen.getAllByRole("heading").map((heading) => heading.textContent)).toEqual([
      "Basic settings",
      "Automatic sync",
      "S3 connection",
      "Advanced options",
      "Connection and status"
    ]);
  });

  it("groups WebDAV connection settings without an empty advanced section", () => {
    render(<SyncSettings {...createProps()} />);

    expect(screen.getAllByRole("heading").map((heading) => heading.textContent)).toEqual([
      "Basic settings",
      "Automatic sync",
      "WebDAV connection",
      "Connection and status"
    ]);
  });

  it("shows the current notebook directory as a read-only target", () => {
    render(<SyncSettings {...createProps()} />);

    expect(screen.getByText("/Notes")).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "Current notebook directory" })).not.toBeInTheDocument();
    expect(screen.getByText(
      /data namespace.*not a notebook name.*discovered automatically/i
    )).toBeVisible();
  });

  it("keeps configuration editable but disables immediate sync without a current notebook", () => {
    render(<SyncSettings {...createProps({ primaryRoot: null })} />);

    expect(screen.getByRole("textbox", { name: "Remote root" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Sync now" })).toBeDisabled();
    expect(screen.getByText("Not selected")).toBeVisible();
  });

  it("persists each changed field immediately with the app-level patch shape", () => {
    const onPatch = vi.fn(async (_patch: SyncConfigPatch) => undefined);
    render(<SyncSettings {...createProps({ onPatch })} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/team" }
    });

    expect(onPatch).toHaveBeenCalledWith({ field: "remoteRoot", value: "qingyu/team" });
  });

  it("persists S3 timeout addressing and TLS verification changes immediately", () => {
    const onPatch = vi.fn(async (_patch: SyncConfigPatch) => undefined);
    const s3Document = document({ config: { ...config, provider: "s3" } });
    render(<SyncSettings {...createProps({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" },
      onPatch
    })} />);

    fireEvent.change(screen.getByRole("spinbutton", { name: "Request timeout" }), {
      target: { value: "299" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Addressing style" }), {
      target: { value: "path" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "TLS certificate verification" }), {
      target: { value: "skip" }
    });

    expect(onPatch).toHaveBeenCalledWith({ field: "s3.requestTimeoutSeconds", value: 299 });
    expect(onPatch).toHaveBeenCalledWith({ field: "s3.addressingStyle", value: "path" });
    expect(onPatch).toHaveBeenCalledWith({ field: "s3.tlsVerification", value: "skip" });
  });

  it("omits the loaded-state sync explanation callouts", () => {
    render(<SyncSettings {...createProps()} />);

    expect(screen.queryByText("Remote two-way sync")).not.toBeInTheDocument();
    expect(screen.queryByText("Plaintext credentials")).not.toBeInTheDocument();
    expect(screen.queryByText(/sync-config\.json stores credentials as plaintext in local application data/)).not.toBeInTheDocument();
  });

  it("creates the single app config when absent even without a primary root", () => {
    const onEnable = vi.fn(async () => undefined);
    render(<SyncSettings {...createProps({
      configDocument: null,
      loadResult: { revision: null, status: "absent" },
      onEnable,
      primaryRoot: null
    })} />);

    fireEvent.click(screen.getByRole("button", { name: "Create sync configuration" }));
    expect(onEnable).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Not selected")).toBeVisible();
  });

  it("keeps a failed optimistic value visible and retries the same field", async () => {
    const onPatch = vi.fn()
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(document({
        config: { ...config, remoteRoot: "qingyu/retry" },
        revision: "rev-2"
      }));
    render(<SyncSettings {...createProps({ onPatch })} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/retry" }
    });

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("not saved"));
    expect(screen.getByRole("textbox", { name: "Remote root" })).toHaveValue("qingyu/retry");
    fireEvent.click(screen.getByRole("button", { name: "Retry unsaved changes" }));

    await waitFor(() => expect(onPatch).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText(/not saved/i)).not.toBeInTheDocument());
    expect(onPatch).toHaveBeenLastCalledWith({ field: "remoteRoot", value: "qingyu/retry" });
  });

  it("resets malformed app config only after confirmation and exposes no file reveal action", () => {
    const onReset = vi.fn(async () => undefined);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<SyncSettings {...createProps({
      configDocument: null,
      loadResult: {
        issue: { code: "sync-config-malformed", message: "malformed" },
        revision: "bad-rev",
        status: "malformed"
      },
      onReset
    })} />);

    fireEvent.click(screen.getByRole("button", { name: "Reset configuration" }));
    expect(onReset).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Open configuration" })).not.toBeInTheDocument();
    confirm.mockRestore();
  });

  it("uses the saved app revision before enabling network actions", () => {
    const incompleteDocument = document({
      issues: [{ code: "sync-remote-root-invalid", field: "remoteRoot", message: "required" }],
      readiness: "incomplete"
    });
    const incomplete = { ...incompleteDocument, status: "loaded" as const };
    render(<SyncSettings {...createProps({
      configDocument: incompleteDocument,
      loadResult: incomplete
    })} />);

    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Sync now" })).toBeDisabled();
  });

  it.each(["webdav", "s3"] as const)("offers cloud notebook selection for configured %s and a current desktop root", (provider) => {
    const onSelectCloudNotebook = vi.fn(async () => undefined);
    const providerDocument = document({ config: { ...config, provider } });
    render(<SyncSettings {...createProps({
      configDocument: providerDocument,
      loadResult: { ...providerDocument, status: "loaded" },
      onSelectCloudNotebook
    })} />);

    const button = screen.getByRole("button", { name: "Select Cloud Notebook" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(onSelectCloudNotebook).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["incomplete provider", {
      configDocument: document({ configured: false, readiness: "incomplete" }),
      loadResult: loaded({ configured: false, readiness: "incomplete" })
    }],
    ["missing current notebook", { primaryRoot: null }],
    ["pending save", { saving: true }],
    ["connection test", { testing: true }],
    ["synchronization", { syncRunning: true }]
  ] as const)("disables cloud notebook selection for %s", (_name, overrides) => {
    render(<SyncSettings {...createProps(overrides)} />);

    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeDisabled();
  });

  it("disables cloud notebook selection while an optimistic field draft is unresolved", () => {
    let resolvePatch!: (value: SyncConfigDocument) => unknown;
    const onPatch = vi.fn(() => new Promise<SyncConfigDocument>((resolve) => {
      resolvePatch = resolve;
    }));
    render(<SyncSettings {...createProps({ onPatch })} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/team" }
    });

    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeDisabled();
    resolvePatch(document({ revision: "rev-2" }));
  });

  it("allows cloud notebook selection when global synchronization is disabled but configured", () => {
    const disabledConfig = { ...config, enabled: false };
    const disabledDocument = document({
      config: disabledConfig,
      configured: true,
      readiness: "disabled"
    });
    render(<SyncSettings {...createProps({
      configDocument: disabledDocument,
      loadResult: { ...disabledDocument, status: "loaded" }
    })} />);

    expect(screen.getByRole("button", { name: "Select Cloud Notebook" })).toBeEnabled();
  });

  it.each([
    ["absent", { revision: null, status: "absent" }],
    ["malformed", {
      issue: { code: "sync-config-malformed", message: "malformed" },
      revision: "bad-rev",
      status: "malformed"
    }],
    ["unsupported", {
      issue: { code: "sync-config-unsupported", message: "unsupported" },
      revision: "future-rev",
      status: "unsupported",
      version: 2
    }]
  ] as const)("does not offer cloud notebook selection for %s configuration", (_name, loadResult) => {
    render(<SyncSettings {...createProps({ configDocument: null, loadResult })} />);

    expect(screen.queryByRole("button", { name: "Select Cloud Notebook" })).not.toBeInTheDocument();
  });

  it("reports a successful bounded connection test without displaying credentials", async () => {
    render(<SyncSettings {...createProps()} />);
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));

    await waitFor(() => expect(screen.getByText(/Connection succeeded/)).toBeVisible());
    expect(screen.queryByText("password-value")).not.toBeInTheDocument();
    expect(screen.queryByText("secret-value")).not.toBeInTheDocument();
  });
});
