import { useCallback, useEffect, useRef, useState } from "react";
import type { MarkdownResourceReference } from "@markra/markdown";
import { getAppRuntime } from "../runtime";
import type {
  NativeMarkdownFolderFile,
  TrashWorkspaceResourceResult
} from "../lib/tauri/file";
import {
  buildWorkspaceResourceGraph,
  isManagedResourceRelativePath,
  type WorkspaceExistingResource,
  type WorkspaceMarkdownFile,
  type WorkspaceResourceFailure,
  type WorkspaceResourceFile,
  type WorkspaceResourceGraph
} from "../lib/workspace-resources";
import {
  createWorkspaceResourceWorker,
  isWorkspaceResourceWorkerResponse,
  type WorkspaceResourceWorkerRequest
} from "../lib/workspace-resource-worker";
import {
  requestWorkspaceResourceSnapshot,
  workspaceResourceFreshnessMatches,
  type WorkspaceResourceFreshness,
  type WorkspaceResourceSnapshotResponse
} from "../lib/workspace-resource-snapshots";

export type WorkspaceResourceScanPhase =
  | "inventory"
  | "reading"
  | "analyzing"
  | "finalizing";

export type WorkspaceResourceScanState = {
  canTrash: boolean;
  graph: WorkspaceResourceGraph | null;
  progress: { completed: number; phase: WorkspaceResourceScanPhase; total: number };
  snapshotGeneration: number | null;
  status: "idle" | "scanning" | "ready" | "incomplete" | "error";
  warning: "snapshot-unavailable" | null;
};

export type TrashSelectionResult =
  | { kind: "canceled" }
  | { kind: "stale" }
  | {
      failed: TrashWorkspaceResourceResult[];
      kind: "completed";
      trashed: TrashWorkspaceResourceResult[];
    };

const idleState: WorkspaceResourceScanState = {
  canTrash: false,
  graph: null,
  progress: { completed: 0, phase: "inventory", total: 0 },
  snapshotGeneration: null,
  status: "idle",
  warning: null
};

function scanningState(): WorkspaceResourceScanState {
  return {
    canTrash: false,
    graph: null,
    progress: { completed: 0, phase: "inventory", total: 0 },
    snapshotGeneration: null,
    status: "scanning",
    warning: null
  };
}

function isMarkdownFile(file: NativeMarkdownFolderFile): file is WorkspaceMarkdownFile {
  return file.kind === undefined;
}

function isResourceFile(file: NativeMarkdownFolderFile): file is WorkspaceResourceFile {
  return (file.kind === "asset" || file.kind === "attachment") &&
    typeof file.modifiedAt === "number" &&
    typeof file.sizeBytes === "number" &&
    isManagedResourceRelativePath(file.relativePath);
}

function failureMessage(error: unknown) {
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  return "Workspace resource scan failed.";
}

function isAbortError(error: unknown) {
  return error instanceof DOMException && error.name === "AbortError";
}

function markdownFreshness(files: readonly WorkspaceMarkdownFile[]) {
  return files.map((file) => ({
    modifiedAt: file.modifiedAt ?? null,
    path: file.path,
    sizeBytes: file.sizeBytes ?? null
  })).sort((left, right) => left.path.localeCompare(right.path));
}

export function useWorkspaceResources(input: {
  active: boolean;
  globalIgnoreRules: string;
  sourceWindowLabel: string | null;
  workspaceSourcePath: string | null;
  workerFactory?: () => Worker;
}): WorkspaceResourceScanState & {
  refresh: () => unknown;
  trashResources: (
    resources: readonly WorkspaceExistingResource[],
    labels: { cancelLabel: string; message: string; okLabel: string }
  ) => Promise<TrashSelectionResult>;
} {
  const [refreshSequence, setRefreshSequence] = useState(0);
  const [state, setState] = useState<WorkspaceResourceScanState>(() =>
    input.active ? scanningState() : idleState
  );
  const scanIdRef = useRef(0);
  const freshnessRef = useRef<WorkspaceResourceFreshness | null>(null);
  const trashRunningRef = useRef(false);
  const stateRef = useRef(state);
  stateRef.current = state;
  const refresh = useCallback(() => {
    freshnessRef.current = null;
    if (input.active) setState(scanningState());
    setRefreshSequence((current) => current + 1);
  }, [input.active]);

  useEffect(() => {
    if (!input.active) {
      setState(idleState);
      return;
    }
    if (!input.sourceWindowLabel || !input.workspaceSourcePath) {
      setState({ ...scanningState(), status: "error" });
      return;
    }

    const runtime = getAppRuntime();
    const scanId = ++scanIdRef.current;
    const controller = new AbortController();
    const worker = (input.workerFactory ?? createWorkspaceResourceWorker)();
    let scanFinished = false;
    let workerTerminated = false;
    let postedBatchCount = 0;
    let completedBatchCount = 0;
    let postingComplete = false;
    let resolveWorkerDone: (() => void) | null = null;
    const workerDone = new Promise<void>((resolve) => {
      resolveWorkerDone = resolve;
    });
    const occurrences = new Map<string, MarkdownResourceReference[]>();
    const failures: WorkspaceResourceFailure[] = [];
    const analyzedPaths = new Set<string>();
    let markdownFiles: WorkspaceMarkdownFile[] = [];
    let resources: WorkspaceResourceFile[] = [];
    let workspaceRoot = "";
    let workerFailed = false;
    freshnessRef.current = null;
    let terminateWorker = () => {};
    const isCurrent = () =>
      scanIdRef.current === scanId && !controller.signal.aborted && !scanFinished;
    const finishWorkerWaitIfReady = () => {
      if (postingComplete && completedBatchCount >= postedBatchCount) {
        resolveWorkerDone?.();
        resolveWorkerDone = null;
      }
    };
    const provisionalGraph = () => buildWorkspaceResourceGraph({
      complete: false,
      failures,
      markdownFiles,
      occurrences,
      resources,
      workspaceRoot
    });
    const failWorker = (message: string) => {
      if (!isCurrent()) return;
      workerFailed = true;
      controller.abort();
      setState((current) => ({
        ...current,
        canTrash: false,
        graph: current.graph ?? (workspaceRoot ? provisionalGraph() : null),
        status: "error"
      }));
      resolveWorkerDone?.();
      resolveWorkerDone = null;
      terminateWorker();
      if (message) {
        // The safe state is intentionally message-free; UI copy remains localized.
      }
    };

    worker.onmessage = (event: MessageEvent<unknown>) => {
      if (!isCurrent()) return;
      if (!isWorkspaceResourceWorkerResponse(event.data)) {
        failWorker("Invalid worker response.");
        return;
      }
      if (event.data.scanId !== scanId) return;

      if (event.data.type === "failed") {
        failures.push({ message: event.data.error, path: event.data.path, stage: "parse" });
        analyzedPaths.add(event.data.path);
        return;
      }

      event.data.occurrences.forEach(({ path, references }) => {
        occurrences.set(path, references);
        analyzedPaths.add(path);
      });
      completedBatchCount += 1;
      setState((current) => ({
        ...current,
        canTrash: false,
        graph: provisionalGraph(),
        progress: postingComplete
          ? {
              completed: Math.min(analyzedPaths.size, markdownFiles.length),
              phase: "analyzing",
              total: markdownFiles.length
            }
          : current.progress,
        status: "scanning"
      }));
      finishWorkerWaitIfReady();
    };
    worker.onerror = (event) => failWorker(event.message || "Workspace resource worker failed.");
    terminateWorker = () => {
      if (workerTerminated) return;
      workerTerminated = true;
      worker.terminate();
    };

    setState(scanningState());
    const snapshotPromise = requestWorkspaceResourceSnapshot({
      events: runtime.events,
      signal: controller.signal,
      sourceWindowLabel: input.sourceWindowLabel,
      workspaceSourcePath: input.workspaceSourcePath
    }).then((response) => ({ aborted: false, reliable: true as const, response })).catch((error: unknown) => ({
      aborted: isAbortError(error),
      reliable: false as const,
      response: null
    }));

    const runScan = async () => {
      workspaceRoot = await runtime.files.resolveWorkspaceResourceRoot(input.workspaceSourcePath as string);
      if (!isCurrent()) return;

      const inventoryByPath = new Map<string, NativeMarkdownFolderFile>();
      const inventoryOptions = {
        globalIgnoreRules: input.globalIgnoreRules,
        onBatch: (batch: NativeMarkdownFolderFile[]) => {
          if (!isCurrent()) return;
          batch.forEach((file) => inventoryByPath.set(file.path, file));
          setState((current) => ({
            ...current,
            progress: {
              completed: inventoryByPath.size,
              phase: "inventory",
              total: inventoryByPath.size
            }
          }));
        },
        signal: controller.signal
      };
      const inventory = runtime.files.loadMarkdownFilesForPath
        ? await runtime.files.loadMarkdownFilesForPath(workspaceRoot, inventoryOptions)
        : await runtime.files.listMarkdownFilesForPath(workspaceRoot, inventoryOptions);
      if (!isCurrent()) return;

      inventory.forEach((file) => inventoryByPath.set(file.path, file));
      const inventoryFiles = Array.from(inventoryByPath.values());
      markdownFiles = inventoryFiles.filter(isMarkdownFile);
      resources = inventoryFiles.filter(isResourceFile);
      setState((current) => ({
        ...current,
        progress: { completed: 0, phase: "reading", total: markdownFiles.length }
      }));

      const snapshotResult = await snapshotPromise;
      if (!isCurrent() || snapshotResult.aborted) return;
      const snapshot: WorkspaceResourceSnapshotResponse | null = snapshotResult.response;
      const dirtyDocuments = new Map(
        snapshot?.dirtyDocuments.map((document) => [document.path, document]) ?? []
      );
      const completedDocuments: WorkspaceResourceWorkerRequest["documents"] = [];
      let readCompleted = 0;
      const postCompletedDocuments = () => {
        if (!isCurrent() || completedDocuments.length === 0) return;
        const documents = completedDocuments.splice(0, 4);
        postedBatchCount += 1;
        worker.postMessage({ documents, scanId, type: "analyze" } satisfies WorkspaceResourceWorkerRequest);
      };
      const analyzeFile = async (file: WorkspaceMarkdownFile) => {
        try {
          const overlay = dirtyDocuments.get(file.path);
          const content = overlay
            ? overlay.content
            : (await runtime.files.readMarkdownFile(file.path)).content;
          if (!isCurrent()) return;
          completedDocuments.push({ content, path: file.path });
          if (completedDocuments.length >= 4) postCompletedDocuments();
        } catch (error) {
          if (!isCurrent()) return;
          failures.push({ message: failureMessage(error), path: file.path, stage: "read" });
        } finally {
          readCompleted += 1;
          if (isCurrent()) {
            setState((current) => ({
              ...current,
              progress: {
                completed: readCompleted,
                phase: "reading",
                total: markdownFiles.length
              }
            }));
          }
        }
      };
      const pending = new Set<Promise<unknown>>();
      for (const file of markdownFiles) {
        let task: Promise<unknown>;
        task = analyzeFile(file).finally(() => pending.delete(task));
        pending.add(task);
        if (pending.size >= 4) await Promise.race(pending);
        if (!isCurrent()) return;
      }
      await Promise.all(pending);
      if (!isCurrent()) return;

      postCompletedDocuments();
      postingComplete = true;
      setState((current) => ({
        ...current,
        progress: {
          completed: Math.min(analyzedPaths.size, markdownFiles.length),
          phase: "analyzing",
          total: markdownFiles.length
        }
      }));
      finishWorkerWaitIfReady();
      await workerDone;
      if (!isCurrent() || workerFailed) return;

      setState((current) => ({
        ...current,
        progress: { completed: 1, phase: "finalizing", total: 1 }
      }));
      const reliableSnapshot = snapshotResult.reliable;
      const complete = reliableSnapshot && failures.length === 0;
      const graph = buildWorkspaceResourceGraph({
        complete,
        failures,
        markdownFiles,
        occurrences,
        resources,
        workspaceRoot
      });
      if (!isCurrent()) return;

      freshnessRef.current = reliableSnapshot && snapshot
        ? {
            dirtyDocuments: [...snapshot.dirtyDocuments].sort((left, right) => left.path.localeCompare(right.path)),
            documentGeneration: snapshot.documentGeneration,
            markdownFiles: markdownFreshness(markdownFiles),
            workspaceRoot,
            workspaceSourcePath: input.workspaceSourcePath as string
          }
        : null;
      scanFinished = true;
      terminateWorker();
      setState({
        canTrash: complete,
        graph,
        progress: { completed: 1, phase: "finalizing", total: 1 },
        snapshotGeneration: snapshot?.documentGeneration ?? null,
        status: complete ? "ready" : "incomplete",
        warning: reliableSnapshot ? null : "snapshot-unavailable"
      });
    };

    runScan().catch((error) => {
      if (!isCurrent() || isAbortError(error)) return;
      scanFinished = true;
      terminateWorker();
      setState((current) => ({
        ...current,
        canTrash: false,
        status: "error"
      }));
    });

    return () => {
      controller.abort();
      scanFinished = true;
      resolveWorkerDone?.();
      resolveWorkerDone = null;
      terminateWorker();
    };
  }, [
    input.active,
    input.globalIgnoreRules,
    input.sourceWindowLabel,
    input.workerFactory,
    input.workspaceSourcePath,
    refreshSequence
  ]);

  const trashResources = useCallback(async (
    selectedResources: readonly WorkspaceExistingResource[],
    labels: { cancelLabel: string; message: string; okLabel: string }
  ): Promise<TrashSelectionResult> => {
    const scannedFreshness = freshnessRef.current;
    const currentState = stateRef.current;
    if (
      trashRunningRef.current ||
      !currentState.canTrash ||
      !scannedFreshness ||
      selectedResources.length === 0
    ) {
      if (!trashRunningRef.current) refresh();
      return { kind: trashRunningRef.current ? "canceled" : "stale" };
    }

    trashRunningRef.current = true;
    setState((current) => ({ ...current, canTrash: false }));
    const runtime = getAppRuntime();
    const restoreEligibility = () => {
      if (freshnessRef.current !== scannedFreshness) return;
      setState((current) => current.status === "ready"
        ? { ...current, canTrash: true }
        : current);
    };
    try {
      const confirmed = await runtime.files.confirmWorkspaceResourceTrash(labels);
      if (!confirmed) {
        restoreEligibility();
        return { kind: "canceled" };
      }
      if (freshnessRef.current !== scannedFreshness) {
        refresh();
        return { kind: "stale" };
      }

      let latestSnapshot: WorkspaceResourceSnapshotResponse;
      let latestInventory: NativeMarkdownFolderFile[];
      try {
        [latestSnapshot, latestInventory] = await Promise.all([
          requestWorkspaceResourceSnapshot({
            events: runtime.events,
            sourceWindowLabel: input.sourceWindowLabel as string,
            workspaceSourcePath: scannedFreshness.workspaceSourcePath
          }),
          runtime.files.listMarkdownFilesForPath(scannedFreshness.workspaceRoot, {
            globalIgnoreRules: input.globalIgnoreRules
          })
        ]);
      } catch {
        refresh();
        return { kind: "stale" };
      }

      const latestFreshness: WorkspaceResourceFreshness = {
        dirtyDocuments: latestSnapshot.dirtyDocuments,
        documentGeneration: latestSnapshot.documentGeneration,
        markdownFiles: markdownFreshness(latestInventory.filter(isMarkdownFile)),
        workspaceRoot: scannedFreshness.workspaceRoot,
        workspaceSourcePath: scannedFreshness.workspaceSourcePath
      };
      if (!workspaceResourceFreshnessMatches(scannedFreshness, latestFreshness)) {
        refresh();
        return { kind: "stale" };
      }
      if (freshnessRef.current !== scannedFreshness) {
        refresh();
        return { kind: "stale" };
      }

      const unusedByPath = new Map(
        currentState.graph?.unused.map((resource) => [resource.relativePath, resource]) ?? []
      );
      const selectedPaths = new Set<string>();
      const resourcesToTrash = selectedResources.flatMap((resource) => {
        const scanned = unusedByPath.get(resource.relativePath);
        if (
          !scanned ||
          selectedPaths.has(resource.relativePath) ||
          scanned.modifiedAt !== resource.modifiedAt ||
          scanned.sizeBytes !== resource.sizeBytes
        ) return [];
        selectedPaths.add(resource.relativePath);
        return [{
          modifiedAt: scanned.modifiedAt,
          relativePath: scanned.relativePath,
          sizeBytes: scanned.sizeBytes
        }];
      });
      if (resourcesToTrash.length !== selectedResources.length) {
        refresh();
        return { kind: "stale" };
      }

      const results = await runtime.files.trashWorkspaceResources(
        scannedFreshness.workspaceRoot,
        resourcesToTrash
      );
      const completed: Extract<TrashSelectionResult, { kind: "completed" }> = {
        failed: results.filter((result) => result.status === "failed"),
        kind: "completed",
        trashed: results.filter((result) => result.status === "trashed")
      };
      refresh();
      return completed;
    } catch {
      refresh();
      return { kind: "stale" };
    } finally {
      trashRunningRef.current = false;
    }
  }, [input.globalIgnoreRules, input.sourceWindowLabel, refresh]);

  return { ...state, refresh, trashResources };
}
