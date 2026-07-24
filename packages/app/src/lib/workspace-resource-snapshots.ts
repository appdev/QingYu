import type { AppEventsRuntime } from "../runtime";

export const workspaceResourceSnapshotRequestEvent =
  "qingyu://workspace-resource-snapshot-request";
export const workspaceResourceSnapshotResponseEvent =
  "qingyu://workspace-resource-snapshot-response";

export type WorkspaceResourceSnapshotRequest = {
  requestId: string;
  sourceWindowLabel: string;
  workspaceSourcePath: string;
};

export type WorkspaceResourceDirtyDocument = {
  content: string;
  path: string;
  revision: number;
};

export type WorkspaceResourceSnapshotResponse = {
  documentGeneration: number;
  dirtyDocuments: WorkspaceResourceDirtyDocument[];
  requestId: string;
  sourceWindowLabel: string;
  workspaceSourcePath: string;
};

export type WorkspaceResourceFreshness = {
  dirtyDocuments: WorkspaceResourceDirtyDocument[];
  documentGeneration: number;
  markdownFiles: Array<{
    modifiedAt: number | null;
    path: string;
    sizeBytes: number | null;
  }>;
  workspaceRoot: string;
  workspaceSourcePath: string;
};

export type WorkspaceResourceSnapshotErrorCode =
  | "invalid-response"
  | "timeout"
  | "unavailable";

export class WorkspaceResourceSnapshotError extends Error {
  readonly code: WorkspaceResourceSnapshotErrorCode;

  constructor(code: WorkspaceResourceSnapshotErrorCode) {
    super(code);
    this.name = "WorkspaceResourceSnapshotError";
    this.code = code;
  }
}

let fallbackSnapshotRequestId = 0;

function createSnapshotRequestId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }

  fallbackSnapshotRequestId += 1;
  return `workspace-resource-snapshot-${fallbackSnapshotRequestId}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isDirtyDocument(value: unknown): value is WorkspaceResourceDirtyDocument {
  if (!isRecord(value)) return false;

  return typeof value.content === "string" &&
    typeof value.path === "string" &&
    value.path.trim().length > 0 &&
    typeof value.revision === "number" &&
    Number.isSafeInteger(value.revision) &&
    value.revision >= 0;
}

function isSnapshotResponse(value: unknown): value is WorkspaceResourceSnapshotResponse {
  if (!isRecord(value)) return false;

  return typeof value.documentGeneration === "number" &&
    Number.isSafeInteger(value.documentGeneration) &&
    value.documentGeneration >= 0 &&
    Array.isArray(value.dirtyDocuments) &&
    value.dirtyDocuments.every(isDirtyDocument) &&
    typeof value.requestId === "string" &&
    value.requestId.length > 0 &&
    typeof value.sourceWindowLabel === "string" &&
    value.sourceWindowLabel.length > 0 &&
    typeof value.workspaceSourcePath === "string" &&
    value.workspaceSourcePath.length > 0;
}

function stableFreshness(value: WorkspaceResourceFreshness): WorkspaceResourceFreshness {
  return {
    ...value,
    dirtyDocuments: [...value.dirtyDocuments].sort((left, right) => left.path.localeCompare(right.path)),
    markdownFiles: [...value.markdownFiles].sort((left, right) => left.path.localeCompare(right.path))
  };
}

export function workspaceResourceFreshnessMatches(
  left: WorkspaceResourceFreshness,
  right: WorkspaceResourceFreshness
) {
  const first = stableFreshness(left);
  const second = stableFreshness(right);
  if (
    first.documentGeneration !== second.documentGeneration ||
    first.workspaceRoot !== second.workspaceRoot ||
    first.workspaceSourcePath !== second.workspaceSourcePath ||
    first.dirtyDocuments.length !== second.dirtyDocuments.length ||
    first.markdownFiles.length !== second.markdownFiles.length
  ) return false;

  return first.dirtyDocuments.every((document, index) => {
    const other = second.dirtyDocuments[index];
    return other !== undefined &&
      document.content === other.content &&
      document.path === other.path &&
      document.revision === other.revision;
  }) && first.markdownFiles.every((file, index) => {
    const other = second.markdownFiles[index];
    return other !== undefined &&
      file.modifiedAt === other.modifiedAt &&
      file.path === other.path &&
      file.sizeBytes === other.sizeBytes;
  });
}

function abortError() {
  return new DOMException("The workspace resource snapshot request was canceled.", "AbortError");
}

export function requestWorkspaceResourceSnapshot(input: {
  events: AppEventsRuntime;
  signal?: AbortSignal;
  sourceWindowLabel: string;
  timeoutMs?: number;
  workspaceSourcePath: string;
}): Promise<WorkspaceResourceSnapshotResponse> {
  if (!input.events.isAvailable()) {
    return Promise.reject(new WorkspaceResourceSnapshotError("unavailable"));
  }
  if (input.signal?.aborted) return Promise.reject(abortError());

  const request: WorkspaceResourceSnapshotRequest = {
    requestId: createSnapshotRequestId(),
    sourceWindowLabel: input.sourceWindowLabel,
    workspaceSourcePath: input.workspaceSourcePath
  };

  return new Promise((resolve, reject) => {
    let settled = false;
    let stopListening: (() => unknown) | null = null;
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (timeout !== null) clearTimeout(timeout);
      timeout = null;
      input.signal?.removeEventListener("abort", handleAbort);
      try {
        stopListening?.();
      } catch {
        // A stale native listener must not prevent the request from settling.
      }
      stopListening = null;
    };
    const rejectOnce = (error: unknown) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };
    const resolveOnce = (response: WorkspaceResourceSnapshotResponse) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(response);
    };
    const handleAbort = () => rejectOnce(abortError());
    const setup = async () => {
      stopListening = await input.events.listen<unknown>(
        workspaceResourceSnapshotResponseEvent,
        ({ payload }) => {
          if (!isRecord(payload) || payload.requestId !== request.requestId) return;
          if (
            typeof payload.sourceWindowLabel === "string" &&
            payload.sourceWindowLabel !== request.sourceWindowLabel
          ) return;
          if (
            typeof payload.workspaceSourcePath === "string" &&
            payload.workspaceSourcePath !== request.workspaceSourcePath
          ) return;
          if (!isSnapshotResponse(payload)) {
            rejectOnce(new WorkspaceResourceSnapshotError("invalid-response"));
            return;
          }
          if (
            payload.sourceWindowLabel !== request.sourceWindowLabel ||
            payload.workspaceSourcePath !== request.workspaceSourcePath
          ) return;

          resolveOnce(payload);
        }
      );
      if (settled) {
        cleanup();
        return;
      }
      if (input.signal?.aborted) {
        handleAbort();
        return;
      }

      input.signal?.addEventListener("abort", handleAbort, { once: true });
      timeout = setTimeout(
        () => rejectOnce(new WorkspaceResourceSnapshotError("timeout")),
        input.timeoutMs ?? 1_500
      );
      await input.events.emit(workspaceResourceSnapshotRequestEvent, request);
    };

    setup().catch(() => rejectOnce(new WorkspaceResourceSnapshotError("unavailable")));
  });
}
