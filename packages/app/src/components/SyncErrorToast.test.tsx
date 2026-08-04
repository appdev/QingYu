import { act, fireEvent, render, screen } from "@testing-library/react";
import { toast } from "sonner";
import { SyncErrorToast, syncErrorToastHostClassName } from "./SyncErrorToast";

vi.mock("sonner", () => ({
  toast: { dismiss: vi.fn() }
}));

const mockedDismiss = vi.mocked(toast.dismiss);
const mobileToastViewportWidth = 412;
const mobileToastOffset = 16;
const desktopSyncErrorToastWidth = 320;

function syncErrorMobileRect(className: string) {
  const width = className.includes("w-[min(20rem,calc(100vw-1.5rem))]!")
    ? Math.min(desktopSyncErrorToastWidth, mobileToastViewportWidth - 24)
    : mobileToastViewportWidth - mobileToastOffset * 2;
  const left = mobileToastOffset;
  const translateX = className.includes("max-[600px]:translate-x-0!") ? 0 : -width / 2;

  return {
    left: left + translateX,
    right: left + translateX + width,
    width
  };
}

function renderSyncErrorToast(status: "error" | "loading" = "error") {
  return render(
    <SyncErrorToast
      action={{ label: "Retry", onClick: vi.fn() }}
      duration={2000}
      lifecycleKey={1}
      message={status === "loading" ? "Syncing…" : "Sync did not complete"}
      status={status}
      toastId="app-sync"
    />
  );
}

describe("SyncErrorToast", () => {
  beforeEach(() => {
    mockedDismiss.mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("dismisses after two idle seconds", () => {
    renderSyncErrorToast();

    act(() => vi.advanceTimersByTime(1_999));
    expect(mockedDismiss).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(mockedDismiss).toHaveBeenCalledWith("app-sync");
  });

  it("restarts the full dwell after hover", () => {
    renderSyncErrorToast();
    const status = screen.getByRole("status");

    act(() => vi.advanceTimersByTime(1_000));
    fireEvent.pointerEnter(status);
    act(() => vi.advanceTimersByTime(5_000));
    expect(mockedDismiss).not.toHaveBeenCalled();

    fireEvent.pointerLeave(status);
    act(() => vi.advanceTimersByTime(1_999));
    expect(mockedDismiss).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(mockedDismiss).toHaveBeenCalledWith("app-sync");
  });

  it("pauses while Retry is focused and restarts the full dwell after blur", () => {
    renderSyncErrorToast();
    const retry = screen.getByRole("button", { name: "Retry" });

    act(() => vi.advanceTimersByTime(1_000));
    act(() => retry.focus());
    act(() => vi.advanceTimersByTime(5_000));
    expect(mockedDismiss).not.toHaveBeenCalled();

    fireEvent.blur(retry, { relatedTarget: null });
    act(() => vi.advanceTimersByTime(1_999));
    expect(mockedDismiss).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(mockedDismiss).toHaveBeenCalledWith("app-sync");
  });

  it("keeps the focused action stable and moves the spinner into it while loading", () => {
    const { rerender } = renderSyncErrorToast();
    const retry = screen.getByRole("button", { name: "Retry" });
    act(() => retry.focus());

    rerender(
      <SyncErrorToast
        action={{ label: "Retry", onClick: vi.fn() }}
        duration={2000}
        lifecycleKey={2}
        message="Syncing…"
        status="loading"
        toastId="app-sync"
      />
    );

    const loadingAction = screen.getByRole("button", { name: "Syncing…" });
    expect(loadingAction).toBe(retry);
    expect(loadingAction).toHaveFocus();
    expect(loadingAction).toHaveAttribute("aria-disabled", "true");
    expect(loadingAction.querySelector(".lucide-loader-circle")).toBeInTheDocument();
    expect(screen.getByRole("status").querySelector(".lucide-circle-alert")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(10_000));
    expect(mockedDismiss).not.toHaveBeenCalled();
  });

  it("keeps the sync error toast inside a 412px mobile viewport while preserving desktop width", () => {
    expect(syncErrorToastHostClassName).toContain("max-[600px]:left-0!");
    expect(syncErrorToastHostClassName).toContain("max-[600px]:translate-x-0!");
    expect(syncErrorToastHostClassName).not.toContain(" left-0!");
    expect(syncErrorToastHostClassName).not.toContain(" translate-x-0!");
    expect(syncErrorToastHostClassName).toContain("w-[min(20rem,calc(100vw-1.5rem))]!");
    expect(syncErrorToastHostClassName).toContain("max-w-[min(20rem,calc(100vw-1.5rem))]!");

    const rect = syncErrorMobileRect(syncErrorToastHostClassName);

    expect(rect.width).toBe(desktopSyncErrorToastWidth);
    expect(rect.left).toBeGreaterThanOrEqual(mobileToastOffset);
    expect(rect.right).toBeLessThanOrEqual(mobileToastViewportWidth - mobileToastOffset);
  });
});
