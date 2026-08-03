export const realtimeMarkdownAutoSaveIdleMs = 1_000;

export type RealtimeMarkdownAutoSaveAttempt = "durable" | "ineligible";
export type RealtimeMarkdownAutoSaveFlushResult = RealtimeMarkdownAutoSaveAttempt | "failed";

export type RealtimeMarkdownAutoSaveController = {
  cancel: (tabId: string) => unknown;
  dispose: () => unknown;
  enqueueNow: (tabId: string) => Promise<RealtimeMarkdownAutoSaveFlushResult>;
  flush: (tabId: string) => Promise<RealtimeMarkdownAutoSaveFlushResult>;
  flushAll: (tabIds: readonly string[]) => Promise<boolean>;
  schedule: (tabId: string) => unknown;
};

type TabAutoSaveState = {
  errorReported: boolean;
  inFlight: Promise<RealtimeMarkdownAutoSaveFlushResult> | null;
  rerunAfterFlight: boolean;
  timer: number | null;
};

export function createRealtimeMarkdownAutoSaveController({
  idleMs = realtimeMarkdownAutoSaveIdleMs,
  isDirty,
  onError,
  saveLatest
}: {
  idleMs?: number;
  isDirty: (tabId: string) => boolean;
  onError: (tabId: string, error: unknown) => unknown;
  saveLatest: (tabId: string) => Promise<RealtimeMarkdownAutoSaveAttempt>;
}): RealtimeMarkdownAutoSaveController {
  const states = new Map<string, TabAutoSaveState>();
  let disposed = false;

  const getState = (tabId: string) => {
    const existingState = states.get(tabId);
    if (existingState) return existingState;

    const state: TabAutoSaveState = {
      errorReported: false,
      inFlight: null,
      rerunAfterFlight: false,
      timer: null
    };
    states.set(tabId, state);
    return state;
  };

  const clearTimer = (state: TabAutoSaveState) => {
    if (state.timer === null) return;

    window.clearTimeout(state.timer);
    state.timer = null;
  };

  const clearPending = (state: TabAutoSaveState) => {
    clearTimer(state);
    state.rerunAfterFlight = false;
  };

  const reportFailure = (tabId: string, state: TabAutoSaveState, error: unknown) => {
    if (state.errorReported) return;

    state.errorReported = true;
    try {
      onError(tabId, error);
    } catch {
      // Error reporting must not create an unhandled save rejection.
    }
  };

  const runNow = (tabId: string): Promise<RealtimeMarkdownAutoSaveFlushResult> => {
    const state = getState(tabId);
    if (state.inFlight) {
      state.rerunAfterFlight = true;
      return state.inFlight;
    }

    let savePromise: Promise<RealtimeMarkdownAutoSaveAttempt>;
    try {
      savePromise = saveLatest(tabId);
    } catch (error) {
      savePromise = Promise.reject(error);
    }

    const inFlight = Promise.resolve(savePromise).then(
      (result) => {
        if (result === "durable") state.errorReported = false;
        return result;
      },
      (error): RealtimeMarkdownAutoSaveFlushResult => {
        reportFailure(tabId, state, error);
        return "failed";
      }
    );
    state.inFlight = inFlight;

    inFlight.then((result) => {
      if (state.inFlight !== inFlight) return result;

      state.inFlight = null;
      const shouldRerun = state.rerunAfterFlight && state.timer === null && isDirty(tabId);
      state.rerunAfterFlight = false;
      if (!shouldRerun) return result;

      return runNow(tabId);
    }).catch(() => undefined);

    return inFlight;
  };

  const cancel = (tabId: string) => {
    const state = states.get(tabId);
    if (!state) return;

    clearPending(state);
    state.errorReported = false;
  };

  const schedule = (tabId: string) => {
    if (disposed) return;

    const state = getState(tabId);
    state.errorReported = false;
    clearTimer(state);
    state.timer = window.setTimeout(() => {
      state.timer = null;
      runNow(tabId).catch(() => undefined);
    }, idleMs);
  };

  const enqueueNow = (tabId: string) => {
    if (disposed) return Promise.resolve("ineligible" as const);

    const state = getState(tabId);
    clearTimer(state);
    return runNow(tabId);
  };

  const flush = async (tabId: string): Promise<RealtimeMarkdownAutoSaveFlushResult> => {
    if (disposed) return "ineligible";

    const state = getState(tabId);
    clearPending(state);
    let latestResult: RealtimeMarkdownAutoSaveFlushResult = "durable";

    if (state.inFlight) {
      latestResult = await state.inFlight;
      if (latestResult === "failed" || latestResult === "ineligible") return latestResult;
    }

    while (isDirty(tabId)) {
      latestResult = state.inFlight
        ? await state.inFlight
        : await runNow(tabId);
      if (latestResult === "failed" || latestResult === "ineligible") return latestResult;
    }

    return latestResult;
  };

  const flushAll = async (tabIds: readonly string[]) => {
    const uniqueTabIds = new Set(tabIds);
    let succeeded = true;
    for (const tabId of uniqueTabIds) {
      if (await flush(tabId) === "failed") succeeded = false;
    }
    return succeeded;
  };

  const dispose = () => {
    disposed = true;
    for (const state of states.values()) {
      clearTimer(state);
      state.rerunAfterFlight = false;
    }
  };

  return { cancel, dispose, enqueueNow, flush, flushAll, schedule };
}
