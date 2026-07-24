import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppEventsRuntime,
  type RuntimeEvent
} from "../runtime";
import type { NativeMarkdownFolderFile } from "../lib/tauri/file";
import {
  analyzeWorkspaceResourceBatch,
  type WorkspaceResourceWorkerRequest
} from "../lib/workspace-resource-worker";
import {
  workspaceResourceSnapshotRequestEvent,
  workspaceResourceSnapshotResponseEvent,
  workspaceResourceFreshnessMatches,
  type WorkspaceResourceFreshness,
  type WorkspaceResourceSnapshotRequest
} from "../lib/workspace-resource-snapshots";
import { useWorkspaceResources } from "./useWorkspaceResources";

function deferred<T>() {
  let resolve!: (value: T) => unknown;
  let reject!: (error: unknown) => unknown;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function markdown(relativePath: string): NativeMarkdownFolderFile {
  return {
    name: relativePath.split("/").at(-1) ?? relativePath,
    path: `/vault/${relativePath}`,
    relativePath
  };
}

function resource(relativePath: string): NativeMarkdownFolderFile {
  return {
    kind: "asset",
    modifiedAt: 100,
    name: relativePath.split("/").at(-1) ?? relativePath,
    path: `/vault/${relativePath}`,
    relativePath,
    sizeBytes: 42
  };
}

class FakeWorker {
  static instances: FakeWorker[] = [];
  onerror: ((event: ErrorEvent) => unknown) | null = null;
  onmessage: ((event: MessageEvent<unknown>) => unknown) | null = null;
  readonly terminate = vi.fn();

  constructor() {
    FakeWorker.instances.push(this);
  }

  postMessage(message: WorkspaceResourceWorkerRequest) {
    queueMicrotask(() => {
      analyzeWorkspaceResourceBatch(message).forEach((response) => {
        this.onmessage?.({ data: response } as MessageEvent<unknown>);
      });
    });
  }
}

const fakeWorkerFactory = () => new FakeWorker() as unknown as Worker;

function snapshotEvents(options: {
  dirtyDocuments?: Array<{ content: string; path: string; revision: number }>;
  generation?: number;
  respond?: boolean;
} = {}): AppEventsRuntime {
  const listeners = new Map<string, Set<(event: RuntimeEvent<unknown>) => unknown>>();
  const publish = (event: string, payload: unknown) => {
    listeners.get(event)?.forEach((listener) => listener({ payload }));
  };

  return {
    emit: async (event, payload) => {
      if (event === workspaceResourceSnapshotRequestEvent && options.respond !== false) {
        const request = payload as WorkspaceResourceSnapshotRequest;
        publish(workspaceResourceSnapshotResponseEvent, {
          documentGeneration: options.generation ?? 3,
          dirtyDocuments: options.dirtyDocuments ?? [],
          requestId: request.requestId,
          sourceWindowLabel: request.sourceWindowLabel,
          workspaceSourcePath: request.workspaceSourcePath
        });
      }
      publish(event, payload);
    },
    isAvailable: () => true,
    listen: async (event, listener) => {
      const eventListeners = listeners.get(event) ?? new Set();
      eventListeners.add(listener as (event: RuntimeEvent<unknown>) => unknown);
      listeners.set(event, eventListeners);
      return () => eventListeners.delete(listener as (event: RuntimeEvent<unknown>) => unknown);
    }
  };
}

function installRuntime(input: {
  confirmResult?: boolean;
  events?: AppEventsRuntime;
  inventory: NativeMarkdownFolderFile[] | Promise<NativeMarkdownFolderFile[]>;
  listError?: Error;
  listInventory?: NativeMarkdownFolderFile[];
  onBatch?: (listener: (files: NativeMarkdownFolderFile[]) => unknown) => unknown;
  read?: (path: string) => Promise<{ content: string; name: string; path: string }>;
  trashResults?: Array<{
    error?: string;
    relativePath: string;
    status: "failed" | "trashed";
  }>;
}) {
  const runtime = createDefaultAppRuntime();
  const readMarkdownFile = vi.fn(input.read ?? (async (path: string) => ({
    content: "",
    name: path.split("/").at(-1) ?? path,
    path
  })));
  const loadMarkdownFilesForPath = vi.fn(async (
    _path: string,
    options?: { onBatch?: (files: NativeMarkdownFolderFile[]) => unknown }
  ) => {
    input.onBatch?.((files) => options?.onBatch?.(files));
    return input.inventory;
  });
  const confirmWorkspaceResourceTrash = vi.fn(async () => input.confirmResult ?? true);
  const listMarkdownFilesForPath = vi.fn(async () => {
    if (input.listError) throw input.listError;
    if (input.listInventory) return input.listInventory;
    if (Array.isArray(input.inventory)) return input.inventory;
    return input.inventory;
  });
  const trashWorkspaceResources = vi.fn(async (
    _rootPath: string,
    resources: readonly { relativePath: string }[]
  ) => input.trashResults ?? resources.map(({ relativePath }) => ({
    relativePath,
    status: "trashed" as const
  })));
  configureAppRuntime({
    ...runtime,
    events: input.events ?? snapshotEvents(),
    files: {
      ...runtime.files,
      confirmWorkspaceResourceTrash,
      listMarkdownFilesForPath,
      loadMarkdownFilesForPath,
      readMarkdownFile,
      resolveWorkspaceResourceRoot: async () => "/vault",
      trashWorkspaceResources
    }
  });

  return {
    confirmWorkspaceResourceTrash,
    listMarkdownFilesForPath,
    loadMarkdownFilesForPath,
    readMarkdownFile,
    trashWorkspaceResources
  };
}

function renderResources(active = true) {
  return renderHook(
    ({ enabled }) => useWorkspaceResources({
      active: enabled,
      globalIgnoreRules: "",
      sourceWindowLabel: "markra-editor-2",
      workerFactory: fakeWorkerFactory,
      workspaceSourcePath: "/vault"
    }),
    { initialProps: { enabled: active } }
  );
}

afterEach(() => {
  FakeWorker.instances = [];
  resetAppRuntimeForTests();
  vi.useRealTimers();
});

describe("workspaceResourceFreshnessMatches", () => {
  const base = (): WorkspaceResourceFreshness => ({
    dirtyDocuments: [{ content: "# Draft", path: "/vault/index.md", revision: 2 }],
    documentGeneration: 5,
    markdownFiles: [{ modifiedAt: 100, path: "/vault/index.md", sizeBytes: 20 }],
    workspaceRoot: "/vault",
    workspaceSourcePath: "/vault"
  });

  it("matches stable arrays regardless of their input order", () => {
    const first = base();
    first.markdownFiles.push({ modifiedAt: 200, path: "/vault/second.md", sizeBytes: 30 });
    const second = base();
    second.markdownFiles.unshift({ modifiedAt: 200, path: "/vault/second.md", sizeBytes: 30 });

    expect(workspaceResourceFreshnessMatches(first, second)).toBe(true);
  });

  it.each([
    ["generation", (value: WorkspaceResourceFreshness) => { value.documentGeneration += 1; }],
    ["dirty content", (value: WorkspaceResourceFreshness) => { value.dirtyDocuments[0]!.content = "# Changed"; }],
    ["dirty revision", (value: WorkspaceResourceFreshness) => { value.dirtyDocuments[0]!.revision += 1; }],
    ["added path", (value: WorkspaceResourceFreshness) => {
      value.markdownFiles.push({ modifiedAt: 100, path: "/vault/new.md", sizeBytes: 1 });
    }],
    ["removed path", (value: WorkspaceResourceFreshness) => { value.markdownFiles = []; }],
    ["size", (value: WorkspaceResourceFreshness) => { value.markdownFiles[0]!.sizeBytes = 21; }],
    ["modified time", (value: WorkspaceResourceFreshness) => { value.markdownFiles[0]!.modifiedAt = 101; }],
    ["workspace root", (value: WorkspaceResourceFreshness) => { value.workspaceRoot = "/other"; }],
    ["workspace source", (value: WorkspaceResourceFreshness) => { value.workspaceSourcePath = "/other"; }]
  ])("rejects changed %s", (_label, mutate) => {
    const changed = base();
    mutate(changed);
    expect(workspaceResourceFreshnessMatches(base(), changed)).toBe(false);
  });
});

describe("useWorkspaceResources", () => {
  it("returns a scanning shell immediately, reports inventory progress, and terminates on deactivation", async () => {
    const inventory = deferred<NativeMarkdownFolderFile[]>();
    let publishBatch: ((files: NativeMarkdownFolderFile[]) => unknown) | null = null;
    installRuntime({
      inventory: inventory.promise,
      onBatch: (listener) => {
        publishBatch = listener;
      }
    });
    const { result, rerender } = renderResources();

    expect(result.current).toEqual(expect.objectContaining({
      canTrash: false,
      graph: null,
      progress: { completed: 0, phase: "inventory", total: 0 },
      status: "scanning"
    }));
    await waitFor(() => expect(publishBatch).not.toBeNull());
    act(() => publishBatch?.([markdown("index.md"), resource("assets/cover.png")]));
    expect(result.current.progress).toEqual({ completed: 2, phase: "inventory", total: 2 });

    rerender({ enabled: false });
    await waitFor(() => expect(FakeWorker.instances[0]?.terminate).toHaveBeenCalledTimes(1));
    expect(result.current.status).toBe("idle");
    inventory.resolve([]);
  });

  it("uses a dirty overlay instead of disk content and publishes final unused resources", async () => {
    const runtime = installRuntime({
      events: snapshotEvents({
        dirtyDocuments: [{
          content: "![Cover](assets/cover.png)",
          path: "/vault/index.md",
          revision: 8
        }],
        generation: 12
      }),
      inventory: [
        markdown("index.md"),
        resource("assets/cover.png"),
        resource("assets/unused.pdf")
      ]
    });
    const { result } = renderResources();

    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(runtime.readMarkdownFile).not.toHaveBeenCalledWith("/vault/index.md");
    expect(result.current.snapshotGeneration).toBe(12);
    expect(result.current.canTrash).toBe(true);
    expect(result.current.graph?.missing).toEqual([]);
    expect(result.current.graph?.unused.map((file) => file.relativePath)).toEqual(["assets/unused.pdf"]);
  });

  it("limits disk reads to four and completes after every queued worker batch", async () => {
    const pending: Array<{
      path: string;
      resolve: (value: { content: string; name: string; path: string }) => unknown;
    }> = [];
    let activeReads = 0;
    let maximumReads = 0;
    const files = Array.from({ length: 6 }, (_, index) => markdown(`note-${index}.md`));
    const runtime = installRuntime({
      inventory: files,
      read: (path) => new Promise((resolve) => {
        activeReads += 1;
        maximumReads = Math.max(maximumReads, activeReads);
        pending.push({
          path,
          resolve: (value) => {
            activeReads -= 1;
            resolve(value);
          }
        });
      })
    });
    const { result } = renderResources();

    await waitFor(() => expect(runtime.readMarkdownFile).toHaveBeenCalledTimes(4));
    act(() => pending.splice(0).forEach(({ path, resolve }) => resolve({ content: "", name: path, path })));
    await waitFor(() => expect(runtime.readMarkdownFile).toHaveBeenCalledTimes(6));
    act(() => pending.splice(0).forEach(({ path, resolve }) => resolve({ content: "", name: path, path })));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    expect(maximumReads).toBe(4);
  });

  it("keeps missing diagnostics but withholds unused resources after a read failure", async () => {
    installRuntime({
      inventory: [
        markdown("index.md"),
        markdown("broken.md"),
        resource("assets/unused.png")
      ],
      read: async (path) => {
        if (path.endsWith("broken.md")) throw new Error("unreadable");
        return { content: "![Missing](assets/missing.png)", name: "index.md", path };
      }
    });
    const { result } = renderResources();

    await waitFor(() => expect(result.current.status).toBe("incomplete"));

    expect(result.current.canTrash).toBe(false);
    expect(result.current.graph?.missing.map((file) => file.relativePath)).toEqual(["assets/missing.png"]);
    expect(result.current.graph?.unused).toEqual([]);
    expect(result.current.graph?.failures).toEqual([
      expect.objectContaining({ path: "/vault/broken.md", stage: "read" })
    ]);
  });

  it("marks snapshot timeout as a non-actionable warning and makes worker errors retryable", async () => {
    vi.useFakeTimers();
    installRuntime({
      events: snapshotEvents({ respond: false }),
      inventory: [markdown("index.md")]
    });
    const { result } = renderResources();
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_500);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.status).toBe("incomplete");
    expect(result.current.warning).toBe("snapshot-unavailable");
    expect(result.current.canTrash).toBe(false);

    act(() => result.current.refresh());
    expect(FakeWorker.instances).toHaveLength(2);
    act(() => FakeWorker.instances[1]?.onerror?.(new ErrorEvent("error", { message: "worker failed" })));
    expect(result.current.status).toBe("error");
    expect(result.current.canTrash).toBe(false);
  });

  it("restores trash eligibility when confirmation is canceled", async () => {
    const runtime = installRuntime({
      confirmResult: false,
      inventory: [markdown("index.md"), resource("assets/unused.png")]
    });
    const { result } = renderResources();
    await waitFor(() => expect(result.current.status).toBe("ready"));
    const unused = result.current.graph?.unused ?? [];

    let outcome!: Awaited<ReturnType<typeof result.current.trashResources>>;
    await act(async () => {
      outcome = await result.current.trashResources(unused, {
        cancelLabel: "Cancel",
        message: "Trash selected resources?",
        okLabel: "Trash"
      });
    });
    expect(outcome).toEqual({ kind: "canceled" });

    await waitFor(() => expect(result.current.canTrash).toBe(true));
    expect(runtime.listMarkdownFilesForPath).not.toHaveBeenCalled();
    expect(runtime.trashWorkspaceResources).not.toHaveBeenCalled();
  });

  it("refreshes without trashing when dirty content changes at the same generation", async () => {
    const snapshotOptions = {
      dirtyDocuments: [{ content: "", path: "/vault/index.md", revision: 1 }],
      generation: 8
    };
    const runtime = installRuntime({
      events: snapshotEvents(snapshotOptions),
      inventory: [markdown("index.md"), resource("assets/unused.png")]
    });
    const { result } = renderResources();
    await waitFor(() => expect(result.current.status).toBe("ready"));
    const unused = result.current.graph?.unused ?? [];
    snapshotOptions.dirtyDocuments = [{ content: "# Changed", path: "/vault/index.md", revision: 1 }];

    let outcome!: Awaited<ReturnType<typeof result.current.trashResources>>;
    await act(async () => {
      outcome = await result.current.trashResources(unused, {
        cancelLabel: "Cancel",
        message: "Trash selected resources?",
        okLabel: "Trash"
      });
    });
    expect(outcome).toEqual({ kind: "stale" });

    expect(runtime.trashWorkspaceResources).not.toHaveBeenCalled();
    await waitFor(() => expect(runtime.loadMarkdownFilesForPath.mock.calls.length).toBeGreaterThan(1));
  });

  it("treats a metadata refresh failure as stale and invokes no trash command", async () => {
    const runtime = installRuntime({
      inventory: [markdown("index.md"), resource("assets/unused.png")],
      listError: new Error("metadata unavailable")
    });
    const { result } = renderResources();
    await waitFor(() => expect(result.current.status).toBe("ready"));

    let outcome!: Awaited<ReturnType<typeof result.current.trashResources>>;
    await act(async () => {
      outcome = await result.current.trashResources(result.current.graph?.unused ?? [], {
        cancelLabel: "Cancel",
        message: "Trash selected resources?",
        okLabel: "Trash"
      });
    });
    expect(outcome).toEqual({ kind: "stale" });

    expect(runtime.trashWorkspaceResources).not.toHaveBeenCalled();
  });

  it("returns independent native success and failure rows, then starts a full refresh", async () => {
    const runtime = installRuntime({
      inventory: [
        markdown("index.md"),
        resource("assets/first.png"),
        resource("assets/second.png")
      ],
      trashResults: [
        { relativePath: "assets/first.png", status: "trashed" },
        { error: "busy", relativePath: "assets/second.png", status: "failed" }
      ]
    });
    const { result } = renderResources();
    await waitFor(() => expect(result.current.status).toBe("ready"));
    const unused = result.current.graph?.unused ?? [];

    let outcome!: Awaited<ReturnType<typeof result.current.trashResources>>;
    await act(async () => {
      outcome = await result.current.trashResources(unused, {
        cancelLabel: "Cancel",
        message: "Trash selected resources?",
        okLabel: "Trash"
      });
    });
    expect(outcome).toEqual({
      failed: [{ error: "busy", relativePath: "assets/second.png", status: "failed" }],
      kind: "completed",
      trashed: [{ relativePath: "assets/first.png", status: "trashed" }]
    });

    expect(runtime.trashWorkspaceResources).toHaveBeenCalledWith("/vault", [
      { modifiedAt: 100, relativePath: "assets/first.png", sizeBytes: 42 },
      { modifiedAt: 100, relativePath: "assets/second.png", sizeBytes: 42 }
    ]);
    await waitFor(() => expect(runtime.loadMarkdownFilesForPath.mock.calls.length).toBeGreaterThan(1));
  });
});
