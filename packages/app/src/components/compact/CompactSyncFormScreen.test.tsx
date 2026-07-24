import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CompactNavigation } from "../../hooks/useCompactNavigation";
import type { CompactSyncSettingsController } from "../../hooks/useCompactSyncSettings";
import type { SyncConfigDocument, SyncConfigPatch } from "../../lib/sync-config";
import { CompactSyncFormScreen } from "./CompactSyncFormScreen";

function document(revision = "rev-1"): SyncConfigDocument {
  return {
    config: {
      autoSyncOnSave: false,
      enabled: true,
      intervalMinutes: 0,
      provider: "webdav",
      remoteRoot: "qingyu",
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
      webdav: { password: "", serverUrl: "https://dav.example.test", username: "" }
    },
    configured: true,
    issues: [],
    readiness: "ready",
    revision
  };
}

function navigation(): CompactNavigation {
  return {
    canGoBack: true,
    page: { kind: "sync-form", mode: "edit" },
    pop: vi.fn(async () => true),
    popIfCurrent: vi.fn(async () => true),
    popToEditor: vi.fn(async () => true),
    push: vi.fn(() => true),
    replace: vi.fn(() => true),
    stack: [{ kind: "editor" }, { kind: "sync-form", mode: "edit" }]
  };
}

function controller(overrides: Partial<CompactSyncSettingsController> = {}): CompactSyncSettingsController {
  const loaded = { ...document(), status: "loaded" as const };
  return {
    available: true,
    begin: vi.fn(async () => loaded),
    configDocument: document(),
    dirty: false,
    enable: vi.fn(async () => document("rev-enabled")),
    end: vi.fn(async () => undefined),
    loadResult: loaded,
    patch: vi.fn(async (_patch: SyncConfigPatch) => document("rev-2")),
    primaryRoot: "/Notes",
    recover: vi.fn(async () => document("rev-recovered")),
    reset: vi.fn(async () => document("rev-reset")),
    runImmediate: vi.fn(async () => undefined),
    saving: false,
    sessionId: "session-1",
    status: null,
    syncRunning: false,
    testConnection: vi.fn(async () => ({ checkedTarget: "dav.example.test", provider: "webdav" as const })),
    testing: false,
    ...overrides
  };
}

describe("CompactSyncFormScreen application config", () => {
  it("starts a newly created application sync config disabled", async () => {
    const setup = controller({
      configDocument: null,
      loadResult: { revision: null, status: "absent" },
      sessionId: null
    });
    render(<CompactSyncFormScreen controller={setup} language="en" mode="create" navigation={navigation()} />);

    await waitFor(() => expect(setup.enable).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("checkbox", { name: "Enable sync" })).not.toBeChecked();
    expect(screen.getByRole("textbox", { name: "Remote root" })).toHaveValue("qingyu");
  });

  it("writes app-level fields immediately", () => {
    const setup = controller();
    render(<CompactSyncFormScreen controller={setup} language="en" mode="edit" navigation={navigation()} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Remote root" }), {
      target: { value: "qingyu/team" }
    });

    expect(setup.patch).toHaveBeenCalledWith({ field: "remoteRoot", value: "qingyu/team" });
  });

  it("writes the same S3 transport options from the compact form", () => {
    const s3Document = document();
    s3Document.config.provider = "s3";
    const setup = controller({
      configDocument: s3Document,
      loadResult: { ...s3Document, status: "loaded" }
    });
    render(<CompactSyncFormScreen controller={setup} language="en" mode="edit" navigation={navigation()} />);

    fireEvent.change(screen.getByRole("spinbutton", { name: "Request timeout" }), {
      target: { value: "120" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Addressing style" }), {
      target: { value: "virtual-hosted" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "TLS certificate verification" }), {
      target: { value: "skip" }
    });

    expect(setup.patch).toHaveBeenCalledWith({ field: "s3.requestTimeoutSeconds", value: 120 });
    expect(setup.patch).toHaveBeenCalledWith({ field: "s3.addressingStyle", value: "virtual-hosted" });
    expect(setup.patch).toHaveBeenCalledWith({ field: "s3.tlsVerification", value: "skip" });
  });

  it("tests the application connection without requiring a project config", async () => {
    const setup = controller();
    render(<CompactSyncFormScreen controller={setup} language="en" mode="edit" navigation={navigation()} />);

    fireEvent.click(screen.getByRole("button", { name: "Test Connection" }));
    await waitFor(() => expect(setup.testConnection).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Connection succeeded.")).toBeVisible();
  });

  it("shows the application-local plaintext credential warning", () => {
    render(<CompactSyncFormScreen controller={controller()} language="en" mode="edit" navigation={navigation()} />);

    expect(screen.getByText(/sync-config\.json stores credentials as plaintext in local application data/)).toBeVisible();
  });

  it("distinguishes the data namespace from discovered notebook directories", () => {
    render(<CompactSyncFormScreen controller={controller()} language="en" mode="edit" navigation={navigation()} />);

    expect(screen.getByText(/data namespace.*not a notebook name.*discovered automatically/i)).toBeVisible();
  });
});
