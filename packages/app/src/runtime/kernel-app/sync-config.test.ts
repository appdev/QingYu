import {
  createDefaultAppRuntime,
  createUnavailableKernelDomainPort,
  type KernelDomainPort,
  type KernelRevision,
  type KernelWorkspaceRelativePath,
} from "../index";

import { createKernelSyncConfigRuntime } from "./sync-config";

const revision = "revision-1" as KernelRevision;

function createKernel(completionState: "failed" | "succeeded"): KernelDomainPort {
  const unavailable = createUnavailableKernelDomainPort();
  return {
    ...unavailable,
    availability: "available",
    sync: {
      ...unavailable.sync,
      readStatus: vi.fn(async () => ({
        activeRunId: null,
        completionState,
        configRevision: revision,
        error: completionState === "failed" ? {
          category: "configuration",
          code: "configuration_invalid",
          httpStatus: 400,
          method: "PUT",
          operation: "sync_run",
          provider: "s3" as const,
          providerErrorCode: "InvalidRequest",
          relativePath: "notes/note.md" as KernelWorkspaceRelativePath,
          requestId: "request-1",
          runId: "run-1",
        } : null,
        lastAttemptAt: "2026-07-31T00:00:00Z",
        lastSuccessfulSyncAt: completionState === "succeeded"
          ? "2026-07-31T00:00:00Z"
          : null,
        lastTrigger: "settings-exit" as const,
        provider: "s3" as const,
        summary: completionState === "succeeded" ? {
          bytesDownloaded: 1,
          bytesUploaded: 2,
          conflictFiles: 0,
          downloadedFiles: 1,
          scannedFiles: 3,
          skippedFiles: 0,
          uploadedFiles: 2,
        } : null,
      })),
      trigger: vi.fn(async () => ({
        acceptedAt: "2026-07-31T00:00:00Z",
        configRevision: revision,
        runId: "run-1",
      })),
    },
  };
}

function createRuntime(
  completionState: "failed" | "succeeded",
  settleApply = vi.fn(async () => undefined),
) {
  const shared = createDefaultAppRuntime().syncConfig;
  return {
    runtime: createKernelSyncConfigRuntime(createKernel(completionState), {
      local: {
        cancelApply: shared.cancelApply,
        loadEditing: shared.loadEditing,
        requestApply: shared.requestApply,
        setEditing: shared.setEditing,
        settleApply,
      },
      delay: async () => undefined,
      maxStatusReads: 1,
    }),
    settleApply,
  };
}

const request = {
  applyToken: "apply-1",
  notebookName: "Notes",
  notesRoot: "kernel-workspace://primary",
  revision,
  trigger: "settings-exit" as const,
};

describe("Kernel sync apply settlement", () => {
  it("rejects repository bindings for a non-Kernel workspace before calling Kernel", async () => {
    const kernel = createKernel("succeeded");
    const bindRepository = vi.fn(async () => ({
      jobId: "bind-1",
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
    }));
    Object.assign(kernel.sync, { bindRepository });
    const shared = createDefaultAppRuntime().syncConfig;
    const runtime = createKernelSyncConfigRuntime(kernel, {
      local: {
        cancelApply: shared.cancelApply,
        loadEditing: shared.loadEditing,
        requestApply: shared.requestApply,
        setEditing: shared.setEditing,
        settleApply: shared.settleApply,
      },
    });

    await expect(runtime.bindRepository({
      displayName: "Shared notes",
      notesRoot: "/tmp/not-the-active-kernel-workspace",
      repositoryId: "323df833-764a-44b3-a534-492640c258f2",
      revision,
    })).rejects.toThrow("does not address the active Kernel workspace");
    expect(bindRepository).not.toHaveBeenCalled();
  });

  it("settles an exact settings apply after Kernel success", async () => {
    const { runtime, settleApply } = createRuntime("succeeded");

    const dispatch = await runtime.sync(request);

    expect(settleApply).toHaveBeenCalledWith({
      outcome: dispatch,
      revision,
      token: "apply-1",
    });
  });

  it("settles an exact settings apply after Kernel failure", async () => {
    const { runtime, settleApply } = createRuntime("failed");

    await expect(runtime.sync(request)).rejects.toMatchObject({ code: "run-failed" });
    expect(settleApply).toHaveBeenCalledWith({
      outcome: { status: "failed" },
      revision,
      token: "apply-1",
    });
  });

  it("preserves the complete safe Kernel failure on a failed run", async () => {
    const { runtime } = createRuntime("failed");

    const error = await runtime.sync(request).catch((caught: unknown) => caught);

    expect(error).toMatchObject({
      code: "run-failed",
      runError: {
        category: "configuration",
        code: "configuration_invalid",
        httpStatus: 400,
        method: "PUT",
        objectId: null,
        operation: "sync_run",
        provider: "s3",
        providerErrorCode: "InvalidRequest",
        relativePath: "notes/note.md",
        requestId: "request-1",
        runId: "run-1",
      },
    });
  });

  it("reports a distinct settlement failure after Kernel success", async () => {
    const settlementError = new Error("native settlement unavailable");
    const { runtime } = createRuntime(
      "succeeded",
      vi.fn(async () => Promise.reject(settlementError)),
    );

    const error = await runtime.sync(request).catch((caught: unknown) => caught);

    expect(error).toMatchObject({
      code: "apply-settlement-failed",
      runError: null,
      settlementError,
    });
  });

  it("preserves the Kernel failure when settlement also fails", async () => {
    const settlementError = new Error("native settlement unavailable");
    const { runtime } = createRuntime(
      "failed",
      vi.fn(async () => Promise.reject(settlementError)),
    );

    const error = await runtime.sync(request).catch((caught: unknown) => caught);

    expect(error).toMatchObject({
      code: "apply-settlement-failed",
      runError: {
        category: "configuration",
        code: "configuration_invalid",
        httpStatus: 400,
        method: "PUT",
        objectId: null,
        operation: "sync_run",
        provider: "s3",
        providerErrorCode: "InvalidRequest",
        relativePath: "notes/note.md",
        requestId: "request-1",
        runId: "run-1",
      },
      settlementError,
    });
  });
});
