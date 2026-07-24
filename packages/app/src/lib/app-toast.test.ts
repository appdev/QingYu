import { toast } from "sonner";
import { appNoticeToasterId, dismissAppToast, showAppToast } from "./app-toast";

vi.mock("sonner", () => ({
  toast: {
    custom: vi.fn(),
    dismiss: vi.fn(),
    error: vi.fn(),
    loading: vi.fn(),
    success: vi.fn()
  }
}));

const mockedToast = vi.mocked(toast);

describe("appToast", () => {
  beforeEach(() => {
    mockedToast.custom.mockReset();
    mockedToast.dismiss.mockReset();
    mockedToast.error.mockReset();
    mockedToast.loading.mockReset();
    mockedToast.success.mockReset();
  });

  it("routes success, loading, error, and dismiss through one shared toast API", () => {
    const action = {
      label: "Restart",
      onClick: vi.fn()
    };

    showAppToast({ message: "Saved", status: "success" });
    showAppToast({ action, id: "update-test", message: "Downloading", status: "loading" });
    showAppToast({
      id: "provider-test",
      message: "Failed",
      status: "error"
    });
    showAppToast({ duration: Infinity, id: "update-ready", message: "Ready", status: "success" });
    dismissAppToast("provider-test");

    expect(mockedToast.success).toHaveBeenCalledWith("Saved", {
      duration: 2000,
      id: "app-toast"
    });
    expect(mockedToast.loading).toHaveBeenCalledWith("Downloading", {
      action,
      duration: Infinity,
      id: "update-test"
    });
    expect(mockedToast.error).toHaveBeenCalledWith("Failed", {
      duration: 2000,
      id: "provider-test"
    });
    expect(mockedToast.success).toHaveBeenCalledWith("Ready", {
      duration: Infinity,
      id: "update-ready"
    });
    expect(mockedToast.dismiss).toHaveBeenCalledWith("provider-test");
  });

  it("passes long error detail through as a toast description", () => {
    const toastWithDescription = {
      description: "S3 image upload failed: HTTP 403",
      message: "Could not save the pasted image.",
      status: "error"
    } as Parameters<typeof showAppToast>[0] & { description: string };

    showAppToast(toastWithDescription);

    expect(mockedToast.error).toHaveBeenCalledWith("Could not save the pasted image.", {
      description: "S3 image upload failed: HTTP 403",
      duration: 2000,
      id: "app-toast"
    });
  });

  it("caps finite toast durations at two seconds without extending shorter feedback", () => {
    showAppToast({ duration: 1200, id: "short-feedback", message: "Copied", status: "success" });
    showAppToast({ duration: 4500, id: "long-feedback", message: "Saved", status: "success" });

    expect(mockedToast.success).toHaveBeenNthCalledWith(1, "Copied", {
      duration: 1200,
      id: "short-feedback"
    });
    expect(mockedToast.success).toHaveBeenNthCalledWith(2, "Saved", {
      duration: 2000,
      id: "long-feedback"
    });
  });

  it("applies the transient sync error presentation without a close control", () => {
    const action = {
      label: "Retry",
      onClick: vi.fn()
    };

    showAppToast({
      action,
      id: "app-sync",
      message: "Sync did not complete",
      presentation: "sync-error",
      status: "error"
    });

    expect(mockedToast.custom).toHaveBeenCalledWith(expect.any(Function), expect.objectContaining({
      className: expect.stringContaining("app-toast-sync-error"),
      classNames: expect.objectContaining({ content: "contents!", title: "contents!" }),
      closeButton: false,
      dismissible: false,
      duration: Infinity,
      id: "app-sync"
    }));
    const renderToast = mockedToast.custom.mock.calls[0]?.[0];
    const element = renderToast?.("app-sync");
    expect(element?.props).toEqual(expect.objectContaining({
      action,
      duration: 2000,
      message: "Sync did not complete",
      status: "error",
      toastId: "app-sync"
    }));
    expect(mockedToast.error).not.toHaveBeenCalled();
  });

  it("routes diagnostic notices through the bottom-right notice toaster", () => {
    showAppToast({
      id: "runtime-error-diagnostics",
      message: "QingYu caught an error.",
      status: "error",
      surface: "notice"
    });

    expect(mockedToast.error).toHaveBeenCalledWith("QingYu caught an error.", {
      duration: 2000,
      id: "runtime-error-diagnostics",
      position: "bottom-right",
      toasterId: appNoticeToasterId
    });
  });
});
