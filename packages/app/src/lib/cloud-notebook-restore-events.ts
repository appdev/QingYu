import { getAppRuntime } from "../runtime";

type PrimaryCloudNotebookRestoreRequestBase = {
  requestId: string;
  revision: string;
};

export type PrimaryCloudNotebookRestoreRequest =
  | PrimaryCloudNotebookRestoreRequestBase & {
      displayName: string;
      notesRoot: string;
      provider: "s3";
      repositoryId: string;
    }
  | PrimaryCloudNotebookRestoreRequestBase & {
      provider: "webdav";
      remoteName: string;
    };

export type PrimaryCloudNotebookRestoreCompletion = {
  requestId: string;
  succeeded: boolean;
};

type PrimaryCloudNotebookRestoreInputBase = {
  revision: string;
  signal?: AbortSignal;
  timeoutMs?: number;
};

export type PrimaryCloudNotebookRestoreInput =
  | PrimaryCloudNotebookRestoreInputBase & {
      displayName: string;
      notesRoot: string;
      provider: "s3";
      repositoryId: string;
    }
  | PrimaryCloudNotebookRestoreInputBase & {
      provider: "webdav";
      remoteName: string;
    };

export const primaryCloudNotebookRestoreRequestedEvent =
  "qingyu://cloud-notebook-restore-requested";
export const primaryCloudNotebookRestoreCompletedEvent =
  "qingyu://cloud-notebook-restore-completed";

const defaultRestoreTimeoutMs = 30 * 60 * 1_000;
let fallbackRequestId = 0;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isValidEventString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && !value.includes("\0");
}

function normalizeRequest(value: unknown): PrimaryCloudNotebookRestoreRequest | null {
  if (!isRecord(value)) return null;
  if (
    !isValidEventString(value.requestId) ||
    !isValidEventString(value.revision)
  ) return null;
  if (value.provider === "s3") {
    if (
      !isValidEventString(value.displayName)
      || !isValidEventString(value.notesRoot)
      || !isValidEventString(value.repositoryId)
    ) return null;
    return {
      displayName: value.displayName,
      notesRoot: value.notesRoot,
      provider: "s3",
      repositoryId: value.repositoryId,
      requestId: value.requestId,
      revision: value.revision
    };
  }
  if (value.provider !== "webdav" || !isValidEventString(value.remoteName)) return null;
  return {
    provider: "webdav",
    remoteName: value.remoteName,
    requestId: value.requestId,
    revision: value.revision
  };
}

function normalizeCompletion(value: unknown): PrimaryCloudNotebookRestoreCompletion | null {
  if (!isRecord(value)) return null;
  if (!isValidEventString(value.requestId) || typeof value.succeeded !== "boolean") return null;

  return {
    requestId: value.requestId,
    succeeded: value.succeeded
  };
}

function createRequestId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }

  fallbackRequestId += 1;
  return `cloud-notebook-restore-${fallbackRequestId}`;
}

export function requestPrimaryCloudNotebookRestore(
  input: PrimaryCloudNotebookRestoreInput
): Promise<boolean> {
  if (!isValidEventString(input.revision) || (
    input.provider === "s3"
      ? !isValidEventString(input.displayName)
        || !isValidEventString(input.notesRoot)
        || !isValidEventString(input.repositoryId)
      : !isValidEventString(input.remoteName)
  )) {
    return Promise.resolve(false);
  }

  const events = getAppRuntime().events;
  if (!events.isAvailable() || input.signal?.aborted) return Promise.resolve(false);

  const request: PrimaryCloudNotebookRestoreRequest = input.provider === "s3"
    ? {
        displayName: input.displayName,
        notesRoot: input.notesRoot,
        provider: "s3",
        repositoryId: input.repositoryId,
        requestId: createRequestId(),
        revision: input.revision
      }
    : {
        provider: "webdav",
        remoteName: input.remoteName,
        requestId: createRequestId(),
        revision: input.revision
      };

  return new Promise((resolve) => {
    let abortListenerRegistered = false;
    let settled = false;
    let stopListening: (() => unknown) | null = null;
    let stopListeningCalled = false;
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (timeout !== null) {
        clearTimeout(timeout);
        timeout = null;
      }
      if (abortListenerRegistered) {
        input.signal?.removeEventListener("abort", handleAbort);
        abortListenerRegistered = false;
      }
      if (stopListening !== null && !stopListeningCalled) {
        stopListeningCalled = true;
        try {
          stopListening();
        } catch {
          // A stale native listener must not prevent the request from settling.
        }
      }
    };
    const settle = (succeeded: boolean) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(succeeded);
    };
    const handleAbort = () => settle(false);
    const setup = async () => {
      stopListening = await events.listen<unknown>(
        primaryCloudNotebookRestoreCompletedEvent,
        ({ payload }) => {
          const completion = normalizeCompletion(payload);
          if (!completion || completion.requestId !== request.requestId) return;
          settle(completion.succeeded);
        }
      );
      if (settled) {
        cleanup();
        return;
      }

      await events.emit(primaryCloudNotebookRestoreRequestedEvent, request);
    };

    if (input.signal) {
      input.signal.addEventListener("abort", handleAbort, { once: true });
      abortListenerRegistered = true;
    }
    timeout = setTimeout(
      () => settle(false),
      input.timeoutMs ?? defaultRestoreTimeoutMs
    );
    setup().catch(() => settle(false));
  });
}

export async function listenPrimaryCloudNotebookRestoreRequested(
  onRequested: (
    request: PrimaryCloudNotebookRestoreRequest
  ) => boolean | Promise<boolean>
): Promise<() => unknown> {
  const events = getAppRuntime().events;
  if (!events.isAvailable()) return () => undefined;

  return events.listen<unknown>(
    primaryCloudNotebookRestoreRequestedEvent,
    async ({ payload }) => {
      const request = normalizeRequest(payload);
      if (!request) return;

      let succeeded = false;
      try {
        succeeded = await onRequested(request) === true;
      } catch {
        succeeded = false;
      }

      await events.emit(primaryCloudNotebookRestoreCompletedEvent, {
        requestId: request.requestId,
        succeeded
      } satisfies PrimaryCloudNotebookRestoreCompletion);
    }
  );
}
