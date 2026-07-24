import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import type { AppSystemBackSubscriber, RuntimeCleanup } from "../runtime";
import {
  compactHistoryState,
  compactStackFromHistoryState,
  createCompactNavigationSessionId
} from "./compact-navigation-history";

export type CompactSettingsCategory =
  | "general"
  | "mcp"
  | "storage"
  | "sync"
  | "appearance"
  | "editor";

export type CompactPage =
  | { kind: "editor" }
  | { kind: "files" }
  | { kind: "move-target"; path: string }
  | { kind: "settings" }
  | { kind: "settings-detail"; category: CompactSettingsCategory }
  | { kind: "sync-status" }
  | { kind: "sync-form"; mode: "create" | "edit" | "recover" };

export type CompactOverlayPage = Exclude<CompactPage, { kind: "editor" }>;
export type CompactNavigationState = readonly CompactPage[];

export type CompactNavigationAction =
  | { type: "push"; page: CompactOverlayPage }
  | { type: "replace"; page: CompactOverlayPage }
  | { type: "pop" }
  | { type: "pop-to-editor" }
  | { type: "reconcile"; stack: CompactNavigationState };

export type CompactNavigation = {
  canGoBack: boolean;
  page: CompactPage;
  stack: CompactNavigationState;
  popIfCurrent: (page: CompactOverlayPage) => Promise<boolean>;
  push: (page: CompactOverlayPage) => boolean;
  replace: (page: CompactOverlayPage) => boolean;
  pop: () => Promise<boolean>;
  popToEditor: () => Promise<boolean>;
};

type UseCompactNavigationOptions = {
  onBeforePop?: (page: CompactPage) => Promise<unknown> | unknown;
  onNavigationError?: (error: unknown) => unknown;
  subscribeToSystemBack?: AppSystemBackSubscriber;
};

type GuardedExitLock = {
  source: CompactNavigationState;
  target: CompactNavigationState;
};

const editorPage: CompactPage = { kind: "editor" };
const initialState: CompactNavigationState = [editorPage];

export function compactPagesEqual(left: CompactPage, right: CompactPage) {
  if (left.kind !== right.kind) return false;
  if (left.kind === "move-target" && right.kind === "move-target") return left.path === right.path;
  if (left.kind === "settings-detail" && right.kind === "settings-detail") {
    return left.category === right.category;
  }
  if (left.kind === "sync-form" && right.kind === "sync-form") return left.mode === right.mode;
  return true;
}

function stacksEqual(left: CompactNavigationState, right: CompactNavigationState) {
  return left.length === right.length && left.every((page, index) => {
    const rightPage = right[index];
    return rightPage ? compactPagesEqual(page, rightPage) : false;
  });
}

function targetKeepsCurrentStack(
  current: CompactNavigationState,
  target: CompactNavigationState
) {
  return target.length >= current.length && current.every((page, index) => {
    const targetPage = target[index];
    return targetPage ? compactPagesEqual(page, targetPage) : false;
  });
}

export function compactNavigationReducer(
  state: CompactNavigationState,
  action: CompactNavigationAction
): CompactNavigationState {
  if (action.type === "reconcile") return stacksEqual(state, action.stack) ? state : action.stack;
  if (action.type === "pop-to-editor") return state.length === 1 ? state : initialState;
  if (action.type === "pop") return state.length === 1 ? state : state.slice(0, -1);

  const currentPage = state.at(-1) ?? editorPage;
  if (compactPagesEqual(currentPage, action.page)) return state;

  if (action.type === "replace" && state.length > 1) {
    return [...state.slice(0, -1), action.page];
  }

  return [...state, action.page];
}

function pushBrowserHistory(stack: CompactNavigationState, sessionId: string) {
  window.history.pushState(compactHistoryState(window.history.state, stack, sessionId), "");
}

function replaceBrowserHistory(stack: CompactNavigationState, sessionId: string) {
  window.history.replaceState(compactHistoryState(window.history.state, stack, sessionId), "");
}

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return typeof value === "object" && value !== null && typeof (value as { then?: unknown }).then === "function";
}

export function useCompactNavigation({
  onBeforePop,
  onNavigationError,
  subscribeToSystemBack
}: UseCompactNavigationOptions = {}): CompactNavigation {
  const [stack, dispatch] = useReducer(compactNavigationReducer, initialState);
  const stackRef = useRef(stack);
  const browserStackRef = useRef<CompactNavigationState>(initialState);
  const onBeforePopRef = useRef(onBeforePop);
  const onNavigationErrorRef = useRef(onNavigationError);
  const [sessionId] = useState(createCompactNavigationSessionId);
  const requestedTargetRef = useRef<CompactNavigationState | null>(null);
  const reconciliationPromiseRef = useRef<Promise<unknown> | null>(null);
  const browserRecoveryPromiseRef = useRef<Promise<unknown> | null>(null);
  const apiBackPromiseRef = useRef<Promise<boolean> | null>(null);
  const awaitingPopstateRef = useRef(false);
  const guardedExitLockRef = useRef<GuardedExitLock | null>(null);

  stackRef.current = stack;
  onBeforePopRef.current = onBeforePop;
  onNavigationErrorRef.current = onNavigationError;

  const commit = useCallback((action: CompactNavigationAction) => {
    const previousState = stackRef.current;
    const nextState = compactNavigationReducer(previousState, action);
    if (nextState === previousState) return { changed: false, nextState, previousState };

    stackRef.current = nextState;
    dispatch(action);
    return { changed: true, nextState, previousState };
  }, []);

  const reconcileBrowserPosition = useCallback((target: CompactNavigationState) => {
    if (stacksEqual(browserStackRef.current, target)) return;

    const historyDistance = target.length - browserStackRef.current.length;
    if (historyDistance === 0) {
      replaceBrowserHistory(target, sessionId);
      browserStackRef.current = target;
      return;
    }

    awaitingPopstateRef.current = true;
    window.history.go(historyDistance);
  }, [sessionId]);

  const reconcileToLatestTarget = useCallback((target: CompactNavigationState) => {
    requestedTargetRef.current = target;
    if (reconciliationPromiseRef.current) return reconciliationPromiseRef.current;

    const reconciliation = (async () => {
      while (requestedTargetRef.current) {
        let nextTarget = requestedTargetRef.current;
        requestedTargetRef.current = null;
        const currentState = stackRef.current;
        if (stacksEqual(currentState, nextTarget)) continue;

        let reconcileGuardedBrowserPosition = false;
        if (!targetKeepsCurrentStack(currentState, nextTarget)) {
          const leavingPage = currentState.at(-1) ?? editorPage;
          const guardedTarget = nextTarget;
          const beforePopResult = onBeforePopRef.current?.(leavingPage);
          const guardedTransition = isPromiseLike(beforePopResult);
          await beforePopResult;
          const latestTarget = requestedTargetRef.current;
          requestedTargetRef.current = null;

          if (guardedTransition) {
            guardedExitLockRef.current = { source: currentState, target: guardedTarget };
          }

          if (guardedTransition && latestTarget && targetKeepsCurrentStack(currentState, latestTarget)) {
            nextTarget = guardedTarget;
            reconcileGuardedBrowserPosition = true;
          } else {
            nextTarget = latestTarget ?? guardedTarget;
          }
        }

        if (!stacksEqual(stackRef.current, nextTarget)) {
          commit({ type: "reconcile", stack: nextTarget });
        }

        if (reconcileGuardedBrowserPosition) reconcileBrowserPosition(nextTarget);
      }
    })();

    reconciliationPromiseRef.current = reconciliation;
    const clearReconciliation = () => {
      if (reconciliationPromiseRef.current === reconciliation) {
        reconciliationPromiseRef.current = null;
      }
    };
    reconciliation.then(clearReconciliation, (error: unknown) => {
      clearReconciliation();
      try {
        onNavigationErrorRef.current?.(error);
      } catch {
        // Navigation failure remains the primary error even if its observer throws.
      }
    });
    return reconciliation;
  }, [commit, reconcileBrowserPosition]);

  const transitionPending = useCallback(() => (
    reconciliationPromiseRef.current !== null
    || apiBackPromiseRef.current !== null
    || awaitingPopstateRef.current
  ), []);

  const push = useCallback((page: CompactOverlayPage) => {
    if ((page as CompactPage).kind === "editor" || transitionPending()) return false;
    const transition = commit({ type: "push", page });
    if (!transition.changed) return false;
    if (page.kind === "sync-form") guardedExitLockRef.current = null;

    pushBrowserHistory(transition.nextState, sessionId);
    browserStackRef.current = transition.nextState;
    return true;
  }, [commit, sessionId, transitionPending]);

  const replace = useCallback((page: CompactOverlayPage) => {
    if ((page as CompactPage).kind === "editor" || transitionPending()) return false;
    const transition = commit({ type: "replace", page });
    if (!transition.changed) return false;
    if (page.kind === "sync-form") guardedExitLockRef.current = null;

    if (transition.previousState.length === 1) {
      pushBrowserHistory(transition.nextState, sessionId);
    } else {
      replaceBrowserHistory(transition.nextState, sessionId);
    }
    browserStackRef.current = transition.nextState;
    return true;
  }, [commit, sessionId, transitionPending]);

  const navigateBack = useCallback((target: CompactNavigationState, historyDistance: number) => {
    if (transitionPending()) return Promise.resolve(true);

    const startingBrowserStack = browserStackRef.current;
    const reconciliation = reconcileToLatestTarget(target);
    const navigation = (async () => {
      await reconciliation;
      if (
        stacksEqual(browserStackRef.current, startingBrowserStack)
        && !stacksEqual(browserStackRef.current, stackRef.current)
      ) {
        awaitingPopstateRef.current = true;
        if (historyDistance === 1) {
          window.history.back();
        } else {
          window.history.go(-historyDistance);
        }
      }
      return true;
    })();

    apiBackPromiseRef.current = navigation;
    const clearNavigation = () => {
      if (apiBackPromiseRef.current === navigation) apiBackPromiseRef.current = null;
    };
    navigation.then(clearNavigation, clearNavigation);
    return navigation;
  }, [reconcileToLatestTarget, transitionPending]);

  const pop = useCallback(() => {
    if (transitionPending()) return Promise.resolve(true);
    const currentState = stackRef.current;
    if (currentState.length === 1) return Promise.resolve(false);
    return navigateBack(currentState.slice(0, -1), 1);
  }, [navigateBack, transitionPending]);

  const popIfCurrent = useCallback((page: CompactOverlayPage) => {
    const currentState = stackRef.current;
    const currentPage = currentState.at(-1) ?? editorPage;
    if (!compactPagesEqual(currentPage, page) || currentState.length === 1) return Promise.resolve(false);
    if (transitionPending()) return Promise.resolve(true);
    return navigateBack(currentState.slice(0, -1), 1);
  }, [navigateBack, transitionPending]);

  const popToEditor = useCallback(() => {
    if (transitionPending()) return Promise.resolve(true);
    const currentState = stackRef.current;
    if (currentState.length === 1) return Promise.resolve(false);
    return navigateBack(initialState, currentState.length - 1);
  }, [navigateBack, transitionPending]);

  useEffect(() => {
    replaceBrowserHistory(stackRef.current, sessionId);
    browserStackRef.current = stackRef.current;

    const handlePopstate = (event: PopStateEvent) => {
      const target = compactStackFromHistoryState(event.state, sessionId);
      if (!target) return;

      awaitingPopstateRef.current = false;
      browserStackRef.current = target;
      const guardedExitLock = guardedExitLockRef.current;
      const restoresEndedGuard = guardedExitLock
        ? targetKeepsCurrentStack(guardedExitLock.source, target)
        : false;
      const effectiveTarget = restoresEndedGuard && guardedExitLock
        ? guardedExitLock.target
        : target;
      const reconciliation = reconcileToLatestTarget(effectiveTarget);
      if (restoresEndedGuard) {
        reconciliation.then(() => reconcileBrowserPosition(effectiveTarget), () => {});
      }
      if (browserRecoveryPromiseRef.current === reconciliation) return;

      browserRecoveryPromiseRef.current = reconciliation;
      reconciliation.then(() => {
        if (browserRecoveryPromiseRef.current === reconciliation) {
          browserRecoveryPromiseRef.current = null;
        }
      }, () => {
        if (browserRecoveryPromiseRef.current === reconciliation) {
          browserRecoveryPromiseRef.current = null;
          requestedTargetRef.current = null;
          if (!stacksEqual(browserStackRef.current, stackRef.current)) {
            pushBrowserHistory(stackRef.current, sessionId);
            browserStackRef.current = stackRef.current;
          }
        }
      });
    };

    window.addEventListener("popstate", handlePopstate);
    return () => window.removeEventListener("popstate", handlePopstate);
  }, [reconcileBrowserPosition, reconcileToLatestTarget, sessionId]);

  useEffect(() => {
    if (!subscribeToSystemBack) return undefined;

    let active = true;
    let unsubscribe: RuntimeCleanup | null = null;
    const runCleanup = (cleanup: RuntimeCleanup) => {
      Promise.resolve().then(cleanup).catch(() => {});
    };
    subscribeToSystemBack(() => pop().catch(() => true)).then((registeredCleanup) => {
      if (!active) {
        runCleanup(registeredCleanup);
        return;
      }

      unsubscribe = registeredCleanup;
    }).catch((error: unknown) => {
      if (!active) return;
      try {
        onNavigationErrorRef.current?.(error);
      } catch {
        // Listener setup failure remains primary even if its observer throws.
      }
    });

    return () => {
      active = false;
      if (unsubscribe) runCleanup(unsubscribe);
    };
  }, [pop, subscribeToSystemBack]);

  return {
    canGoBack: stack.length > 1,
    page: stack.at(-1) ?? editorPage,
    stack,
    popIfCurrent,
    push,
    replace,
    pop,
    popToEditor
  };
}
