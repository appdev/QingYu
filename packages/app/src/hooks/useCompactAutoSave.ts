import { useCallback, useEffect, useRef, useState } from "react";

export type CompactAutoSaveReason = "navigation" | "pagehide" | "retry" | "timer" | "visibility-hidden";

export type CompactAutoSaveState = {
  error: string | null;
  flush: (reason: CompactAutoSaveReason) => Promise<unknown>;
  retry: () => Promise<unknown>;
  status: "dirty" | "error" | "saved" | "saving";
};

export type CompactAutoSaveErrorMessages = {
  noSpace: string;
  permission: string;
  readOnly: string;
};

const defaultErrorMessages: CompactAutoSaveErrorMessages = {
  noSpace: "Not enough storage space to save this note.",
  permission: "Permission denied while saving this note.",
  readOnly: "This note is in a read-only location."
};

type UseCompactAutoSaveOptions = {
  content: string;
  dirty: boolean;
  documentKey: string | null;
  enabled: boolean;
  errorMessage: string;
  errorMessages?: CompactAutoSaveErrorMessages;
  saveDirtyMarkdownFiles: () => Promise<unknown>;
};

function knownErrorText(error: unknown) {
  if (error instanceof Error) return `${error.name} ${error.message}`;
  if (typeof error === "string") return error;
  if (typeof error !== "object" || error === null) return "";

  const code = (error as { code?: unknown }).code;
  return typeof code === "string" ? code : "";
}

export function compactAutoSaveErrorMessage(
  error: unknown,
  fallback: string,
  messages: CompactAutoSaveErrorMessages = defaultErrorMessages
) {
  const text = knownErrorText(error);
  if (/\b(?:ENOSPC|disk full|no space left|out of (?:disk|storage) space|storage full)\b/iu.test(text)) {
    return messages.noSpace;
  }
  if (/\b(?:EROFS|read[- ]only (?:file system|filesystem|location|volume))\b/iu.test(text)) {
    return messages.readOnly;
  }
  if (/\b(?:EACCES|EPERM|permission denied|operation not permitted|access denied)\b/iu.test(text)) {
    return messages.permission;
  }
  return fallback;
}

export function useCompactAutoSave({
  content,
  dirty,
  documentKey,
  enabled,
  errorMessage,
  errorMessages,
  saveDirtyMarkdownFiles
}: UseCompactAutoSaveOptions): CompactAutoSaveState {
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<CompactAutoSaveState["status"]>(
    enabled && dirty ? "dirty" : "saved"
  );
  const flushPromiseRef = useRef<Promise<unknown> | null>(null);
  const timerRef = useRef<number | null>(null);
  const editRevisionRef = useRef(0);
  const requestedRevisionRef = useRef(0);
  const savedRevisionRef = useRef(0);
  const forcedAttemptInFlightRef = useRef(false);
  const forcedAttemptPendingRef = useRef(false);
  const stateEpochRef = useRef(0);
  const documentKeyRef = useRef(documentKey);
  const cancelTimer = useCallback(() => {
    if (timerRef.current === null) return;
    window.clearTimeout(timerRef.current);
    timerRef.current = null;
  }, []);
  const flush = useCallback((reason: CompactAutoSaveReason) => {
    cancelTimer();
    requestedRevisionRef.current = Math.max(
      requestedRevisionRef.current,
      editRevisionRef.current
    );
    if (
      reason !== "timer"
      && !forcedAttemptInFlightRef.current
      && !forcedAttemptPendingRef.current
    ) {
      forcedAttemptPendingRef.current = true;
    }
    if (flushPromiseRef.current) return flushPromiseRef.current;

    const operation = (async () => {
      let latestResult: unknown = [];
      while (
        forcedAttemptPendingRef.current
        || savedRevisionRef.current < requestedRevisionRef.current
      ) {
        const consumesForcedAttempt = forcedAttemptPendingRef.current;
        if (consumesForcedAttempt) {
          forcedAttemptPendingRef.current = false;
          forcedAttemptInFlightRef.current = true;
        }
        const savingRevision = requestedRevisionRef.current;
        const savingDocumentKey = documentKeyRef.current;
        const savingStateEpoch = stateEpochRef.current;
        setStatus("saving");
        try {
          latestResult = await saveDirtyMarkdownFiles();
        } catch (saveError: unknown) {
          if (
            stateEpochRef.current === savingStateEpoch
            && documentKeyRef.current === savingDocumentKey
            && savedRevisionRef.current <= savingRevision
          ) {
            setError(compactAutoSaveErrorMessage(saveError, errorMessage, errorMessages));
            setStatus("error");
          }
          if (forcedAttemptPendingRef.current) continue;
          throw saveError;
        } finally {
          if (consumesForcedAttempt) forcedAttemptInFlightRef.current = false;
        }
        const previouslySavedRevision = savedRevisionRef.current;
        savedRevisionRef.current = Math.max(previouslySavedRevision, savingRevision);
        if (
          stateEpochRef.current === savingStateEpoch
          && documentKeyRef.current === savingDocumentKey
          && previouslySavedRevision <= savingRevision
        ) {
          setError(null);
          setStatus(editRevisionRef.current === savingRevision ? "saved" : "dirty");
        }
      }
      return latestResult;
    })().finally(() => {
      if (flushPromiseRef.current === operation) flushPromiseRef.current = null;
    });
    flushPromiseRef.current = operation;
    return operation;
  }, [cancelTimer, errorMessage, errorMessages, saveDirtyMarkdownFiles]);

  useEffect(() => {
    if (!enabled || !dirty) return undefined;

    editRevisionRef.current += 1;
    setStatus((currentStatus) => currentStatus === "error" ? "error" : "dirty");
    const timer = window.setTimeout(() => {
      if (timerRef.current === timer) timerRef.current = null;
      flush("timer").catch(() => {});
    }, 1500);
    timerRef.current = timer;
    return () => {
      if (timerRef.current !== timer) return;
      window.clearTimeout(timer);
      timerRef.current = null;
    };
  }, [content, dirty, documentKey, enabled, flush]);

  useEffect(() => {
    const documentChanged = documentKeyRef.current !== documentKey;
    documentKeyRef.current = documentKey;
    if (documentChanged) stateEpochRef.current += 1;
    if (enabled && dirty) return;

    if (!documentChanged) stateEpochRef.current += 1;
    cancelTimer();
    savedRevisionRef.current = editRevisionRef.current;
    requestedRevisionRef.current = editRevisionRef.current;
    if (documentChanged && enabled) {
      setStatus((currentStatus) => currentStatus === "error" ? "error" : "saved");
      return;
    }
    setError(null);
    setStatus("saved");
  }, [cancelTimer, dirty, documentKey, enabled]);

  useEffect(() => {
    if (!enabled) return undefined;

    const handleVisibilityChange = () => {
      if (document.visibilityState !== "hidden") return;
      flush("visibility-hidden").catch(() => {});
    };
    const handlePageHide = () => {
      flush("pagehide").catch(() => {});
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("pagehide", handlePageHide);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("pagehide", handlePageHide);
    };
  }, [enabled, flush]);

  return {
    error,
    flush,
    retry: () => flush("retry"),
    status
  };
}
