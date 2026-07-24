import { parentPathFromPath } from "@markra/shared";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  loadPrimaryWorkspaceState,
  saveCanonicalPrimaryWorkspaceState,
  savePrimaryWorkspaceState,
  type PrimaryWorkspaceState
} from "../lib/settings/local-state";
import {
  listenPrimaryWorkspaceChanged,
  notifyPrimaryWorkspaceChanged,
  type PrimaryWorkspaceChangedPayload
} from "../lib/settings/primary-workspace-events";
import { getAppRuntime } from "../runtime";

export type PrimaryWorkspaceStatus =
  | "loading"
  | "needs-onboarding"
  | "ready"
  | "deferred"
  | "recovery"
  | "error";

export type PrimaryWorkspaceController = {
  canChooseDesktopRoot: boolean;
  commitDesktopRoot: (path: string) => Promise<string | null>;
  commitManagedRoot: (name: string) => Promise<string | null>;
  deferDesktopSetup: () => Promise<unknown>;
  error: string | null;
  managedName: string | null;
  resetOnboarding: () => Promise<unknown>;
  retry: () => Promise<unknown>;
  root: string | null;
  status: PrimaryWorkspaceStatus;
  workspaceRoot: string | null;
};

type PrimaryWorkspaceControllerState = Pick<
  PrimaryWorkspaceController,
  "error" | "root" | "status" | "workspaceRoot"
>;

const loadingState: PrimaryWorkspaceControllerState = {
  error: null,
  root: null,
  status: "loading",
  workspaceRoot: null
};

const maxDesktopResolutionAttempts = 8;

function primaryWorkspaceError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function createPrimaryWorkspaceSourceId() {
  return globalThis.crypto?.randomUUID?.() ??
    `qingyu-primary-workspace-${Math.random().toString(36).slice(2)}`;
}

export function usePrimaryWorkspace({
  trueMobile
}: {
  trueMobile: boolean;
}): PrimaryWorkspaceController {
  const [state, setState] = useState<PrimaryWorkspaceControllerState>(loadingState);
  const generationRef = useRef(0);
  const eventGenerationRef = useRef(0);
  const eventSourceIdRef = useRef(createPrimaryWorkspaceSourceId());
  const receivedEventGenerationsRef = useRef(new Map<string, number>());
  const mountedRef = useRef(true);
  const persistedStateRef = useRef<PrimaryWorkspaceState | null>(null);
  const stateRef = useRef(state);
  const trueMobileRef = useRef(trueMobile);
  stateRef.current = state;
  trueMobileRef.current = trueMobile;

  const transition = useCallback((nextState: PrimaryWorkspaceControllerState) => {
    setState(nextState);
  }, []);

  const transitionIfCurrent = useCallback((
    generation: number,
    nextState: PrimaryWorkspaceControllerState
  ) => {
    if (!mountedRef.current || generation !== generationRef.current) return false;
    transition(nextState);
    return true;
  }, [transition]);

  const beginOperation = useCallback(() => {
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    transition(loadingState);
    return generation;
  }, [transition]);

  const publishChange = useCallback(async () => {
    const generation = eventGenerationRef.current + 1;
    eventGenerationRef.current = generation;
    try {
      await notifyPrimaryWorkspaceChanged({
        generation,
        sourceId: eventSourceIdRef.current
      });
    } catch {
      // Persisted state remains authoritative when cross-window delivery is unavailable.
    }
  }, []);

  const resolveDesktopState = useCallback(async (
    persistedState: PrimaryWorkspaceState,
    generation: number
  ) => {
    try {
      let authoritativeState = persistedState;
      const resolvedIdentities = new Set<string>();
      for (let attempt = 0; attempt < maxDesktopResolutionAttempts; attempt += 1) {
        if (authoritativeState.onboardingRequestedForNextLaunch) {
          transitionIfCurrent(generation, {
            error: null,
            root: null,
            status: "needs-onboarding",
            workspaceRoot: null
          });
          return null;
        }

        if (!authoritativeState.desktopPath || !authoritativeState.desktopWorkspaceRoot) {
          transitionIfCurrent(generation, {
            error: null,
            root: null,
            status: authoritativeState.onboardingCompleted ? "deferred" : "needs-onboarding",
            workspaceRoot: null
          });
          return null;
        }

        const identityKey = `${authoritativeState.desktopWorkspaceRoot}\0${authoritativeState.desktopPath}`;
        if (resolvedIdentities.has(identityKey)) {
          throw new Error("Primary workspace path resolution did not converge.");
        }
        resolvedIdentities.add(identityKey);

        const folder = await getAppRuntime().files.resolveMarkdownFolder(authoritativeState.desktopPath);
        if (!mountedRef.current || generation !== generationRef.current) return null;
        const workspaceFolder = await getAppRuntime().files.resolveMarkdownFolder(
          authoritativeState.desktopWorkspaceRoot
        );
        if (!mountedRef.current || generation !== generationRef.current) return null;
        if (parentPathFromPath(folder.path) !== workspaceFolder.path) {
          throw new Error("Primary workspace notebook is not a direct child of its Workspace root.");
        }

        if (
          folder.path !== authoritativeState.desktopPath ||
          workspaceFolder.path !== authoritativeState.desktopWorkspaceRoot
        ) {
          const canonicalState = await saveCanonicalPrimaryWorkspaceState(
            {
              ...authoritativeState,
              desktopPath: folder.path,
              desktopWorkspaceRoot: workspaceFolder.path
            },
            authoritativeState
          );
          if (!mountedRef.current || generation !== generationRef.current) return null;
          authoritativeState = canonicalState;
          persistedStateRef.current = authoritativeState;
          continue;
        }

        if (!authoritativeState.onboardingCompleted) {
          transitionIfCurrent(generation, {
            error: null,
            root: null,
            status: "needs-onboarding",
            workspaceRoot: null
          });
          return null;
        }

        transitionIfCurrent(generation, {
          error: null,
          root: folder.path,
          status: "ready",
          workspaceRoot: workspaceFolder.path
        });
        return folder.path;
      }

      throw new Error("Primary workspace path resolution exceeded its retry limit.");
    } catch (error: unknown) {
      transitionIfCurrent(generation, {
        error: primaryWorkspaceError(error),
        root: null,
        status: "recovery",
        workspaceRoot: null
      });
      return null;
    }
  }, [transitionIfCurrent]);

  const resolveMobileState = useCallback(async (
    persistedState: PrimaryWorkspaceState,
    generation: number
  ) => {
    if (
      !persistedState.managedName ||
      !persistedState.onboardingCompleted ||
      persistedState.onboardingRequestedForNextLaunch
    ) {
      transitionIfCurrent(generation, {
        error: null,
        root: null,
        status: "needs-onboarding",
        workspaceRoot: null
      });
      return null;
    }

    try {
      const root = await getAppRuntime().workspace.resolveManagedRoot(persistedState.managedName);
      if (!root) throw new Error("Managed workspace is unavailable.");
      transitionIfCurrent(generation, {
        error: null,
        root,
        status: "ready",
        workspaceRoot: null
      });
      return root;
    } catch (error: unknown) {
      transitionIfCurrent(generation, {
        error: primaryWorkspaceError(error),
        root: null,
        status: "error",
        workspaceRoot: null
      });
      return null;
    }
  }, [transitionIfCurrent]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    transition(loadingState);

    const resolveInitialState = async () => {
      try {
        const persistedState = persistedStateRef.current ?? await loadPrimaryWorkspaceState();
        if (!mountedRef.current || generation !== generationRef.current) return;
        persistedStateRef.current = persistedState;

        if (trueMobile) {
          await resolveMobileState(persistedState, generation);
          return;
        }

        await resolveDesktopState(persistedState, generation);
      } catch (error: unknown) {
        transitionIfCurrent(generation, {
          error: primaryWorkspaceError(error),
          root: null,
          status: "error",
          workspaceRoot: null
        });
      }
    };

    resolveInitialState().catch((error: unknown) => {
      transitionIfCurrent(generation, {
        error: primaryWorkspaceError(error),
        root: null,
        status: "error",
        workspaceRoot: null
      });
    });

    return () => {
      if (generationRef.current === generation) generationRef.current += 1;
    };
  }, [resolveDesktopState, resolveMobileState, transition, transitionIfCurrent, trueMobile]);

  const reloadFromApplicationEvent = useCallback(async (payload: PrimaryWorkspaceChangedPayload) => {
    const lastGeneration = receivedEventGenerationsRef.current.get(payload.sourceId) ?? -1;
    if (payload.generation <= lastGeneration) return null;
    receivedEventGenerationsRef.current.set(payload.sourceId, payload.generation);

    const generation = beginOperation();
    try {
      const persistedState = await loadPrimaryWorkspaceState();
      if (!mountedRef.current || generation !== generationRef.current) return null;
      persistedStateRef.current = persistedState;
      if (trueMobileRef.current) return resolveMobileState(persistedState, generation);
      return resolveDesktopState(persistedState, generation);
    } catch (error: unknown) {
      transitionIfCurrent(generation, {
        error: primaryWorkspaceError(error),
        root: null,
        status: "error",
        workspaceRoot: null
      });
      return null;
    }
  }, [beginOperation, resolveDesktopState, resolveMobileState, transitionIfCurrent]);

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => unknown) | null = null;

    listenPrimaryWorkspaceChanged(eventSourceIdRef.current, (payload) => {
      if (cancelled) return;
      reloadFromApplicationEvent(payload).catch(() => {});
    }).then((stopListening) => {
      if (cancelled) {
        stopListening();
        return;
      }
      cleanup = stopListening;
    }).catch(() => {});

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [reloadFromApplicationEvent]);

  const commitDesktopRoot = useCallback(async (path: string) => {
    if (trueMobileRef.current) return null;

    const previousState = stateRef.current;
    const generation = beginOperation();
    try {
      const folder = await getAppRuntime().files.resolveMarkdownFolder(path);
      if (!mountedRef.current || generation !== generationRef.current) return null;
      const parentPath = parentPathFromPath(folder.path);
      if (!parentPath || parentPath === folder.path) {
        throw new Error("Primary workspace notebook requires a Workspace parent.");
      }
      const workspaceFolder = await getAppRuntime().files.resolveMarkdownFolder(parentPath);
      if (!mountedRef.current || generation !== generationRef.current) return null;
      if (parentPathFromPath(folder.path) !== workspaceFolder.path) {
        throw new Error("Primary workspace notebook is not a direct child of its Workspace root.");
      }

      const persistedState = await savePrimaryWorkspaceState({
        desktopWorkspaceRoot: workspaceFolder.path,
        desktopPath: folder.path,
        managedName: null,
        onboardingCompleted: true,
        version: 3
      });
      if (!mountedRef.current || generation !== generationRef.current) return null;
      persistedStateRef.current = persistedState;
      transitionIfCurrent(generation, {
        error: null,
        root: folder.path,
        status: "ready",
        workspaceRoot: workspaceFolder.path
      });
      await publishChange();
      return folder.path;
    } catch (error: unknown) {
      transitionIfCurrent(generation, previousState.status === "ready"
        ? previousState
        : {
            error: primaryWorkspaceError(error),
            root: null,
            status: "error",
            workspaceRoot: null
          });
      return null;
    }
  }, [beginOperation, publishChange, transitionIfCurrent]);

  const commitManagedRoot = useCallback(async (name: string) => {
    if (!trueMobileRef.current) return null;

    const previousState = stateRef.current;
    const generation = beginOperation();
    try {
      const root = await getAppRuntime().workspace.resolveManagedRoot(name);
      if (!root) throw new Error("Managed workspace is unavailable.");
      if (!mountedRef.current || generation !== generationRef.current) return null;

      const persistedState = await savePrimaryWorkspaceState({
        desktopWorkspaceRoot: null,
        desktopPath: null,
        managedName: name,
        onboardingCompleted: true,
        version: 3
      });
      if (!mountedRef.current || generation !== generationRef.current) return null;
      persistedStateRef.current = persistedState;
      transitionIfCurrent(generation, {
        error: null,
        root,
        status: "ready",
        workspaceRoot: null
      });
      await publishChange();
      return root;
    } catch (error: unknown) {
      transitionIfCurrent(generation, previousState.status === "ready"
        ? previousState
        : {
            error: primaryWorkspaceError(error),
            root: null,
            status: "error",
            workspaceRoot: null
          });
      return null;
    }
  }, [beginOperation, publishChange, transitionIfCurrent]);

  const deferDesktopSetup = useCallback(async () => {
    if (trueMobileRef.current) return null;

    const generation = beginOperation();
    try {
      const persistedState = await savePrimaryWorkspaceState({
        desktopWorkspaceRoot: null,
        desktopPath: null,
        managedName: null,
        onboardingCompleted: true,
        version: 3
      });
      if (!mountedRef.current || generation !== generationRef.current) return null;
      persistedStateRef.current = persistedState;
      transitionIfCurrent(generation, {
        error: null,
        root: null,
        status: "deferred",
        workspaceRoot: null
      });
      await publishChange();
      return persistedState;
    } catch (error: unknown) {
      transitionIfCurrent(generation, {
        error: primaryWorkspaceError(error),
        root: null,
        status: "error",
        workspaceRoot: null
      });
      return null;
    }
  }, [beginOperation, publishChange, transitionIfCurrent]);

  const resetOnboarding = useCallback(async () => {
    try {
      const currentState = persistedStateRef.current ?? await loadPrimaryWorkspaceState();
      const persistedState = await savePrimaryWorkspaceState({
        ...currentState,
        onboardingRequestedForNextLaunch: true
      });
      if (mountedRef.current) persistedStateRef.current = persistedState;
      return persistedState;
    } catch {
      return null;
    }
  }, []);

  const retry = useCallback(async () => {
    const persistedState = persistedStateRef.current;
    if (!persistedState) {
      const generation = beginOperation();
      try {
        const loadedState = await loadPrimaryWorkspaceState();
        if (!mountedRef.current || generation !== generationRef.current) return null;
        persistedStateRef.current = loadedState;
        if (trueMobileRef.current) return resolveMobileState(loadedState, generation);
        return resolveDesktopState(loadedState, generation);
      } catch (error: unknown) {
        transitionIfCurrent(generation, {
          error: primaryWorkspaceError(error),
          root: null,
          status: "error",
          workspaceRoot: null
        });
        return null;
      }
    }

    const generation = beginOperation();
    if (trueMobileRef.current) return resolveMobileState(persistedState, generation);
    return resolveDesktopState(persistedState, generation);
  }, [
    beginOperation,
    resolveDesktopState,
    resolveMobileState,
    transitionIfCurrent
  ]);

  return {
    ...state,
    canChooseDesktopRoot: !trueMobile,
    commitDesktopRoot,
    commitManagedRoot,
    deferDesktopSetup,
    managedName: persistedStateRef.current?.managedName ?? null,
    resetOnboarding,
    retry
  };
}
