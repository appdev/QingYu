import { describe, expect, it } from "vitest";

import { workspaceSurfaceForRestore } from "./restore-outcome";

describe("workspaceSurfaceForRestore", () => {
  it("returns editor for valid files", () => {
    expect(
      workspaceSurfaceForRestore({
        recoverableDraftCount: 0,
        validFileCount: 1,
        workspaceReady: true,
      }),
    ).toBe("editor");
  });

  it("returns editor for recoverable dirty drafts without valid files", () => {
    expect(
      workspaceSurfaceForRestore({
        recoverableDraftCount: 1,
        validFileCount: 0,
        workspaceReady: true,
      }),
    ).toBe("editor");
  });

  it("returns home when no valid files or drafts survive", () => {
    expect(
      workspaceSurfaceForRestore({
        recoverableDraftCount: 0,
        validFileCount: 0,
        workspaceReady: true,
      }),
    ).toBe("home");
  });

  it("returns recovery when the workspace is not ready", () => {
    expect(
      workspaceSurfaceForRestore({
        recoverableDraftCount: 1,
        validFileCount: 1,
        workspaceReady: false,
      }),
    ).toBe("recovery");
  });
});
