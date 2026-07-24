import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
  configureAppRuntime,
  createDefaultAppRuntime,
  resetAppRuntimeForTests,
  type AppEventsRuntime,
  type RuntimeEvent
} from "../runtime";
import {
  workspaceResourceSnapshotRequestEvent,
  workspaceResourceSnapshotResponseEvent,
  type WorkspaceResourceSnapshotRequest,
  type WorkspaceResourceSnapshotResponse
} from "../lib/workspace-resource-snapshots";
import { useWorkspaceResourceSnapshotResponder } from "./useWorkspaceResourceSnapshotResponder";

type TestTab = Parameters<typeof useWorkspaceResourceSnapshotResponder>[0]["documentTabs"][number];

function tab(patch: Partial<TestTab> = {}): TestTab {
  return {
    content: "# Note",
    deleted: false,
    dirty: true,
    id: "file:/vault/note.md",
    name: "note.md",
    open: true,
    path: "/vault/note.md",
    revision: 1,
    ...patch
  };
}

function installRuntime() {
  const listeners = new Map<string, Set<(event: RuntimeEvent<unknown>) => unknown>>();
  const emitted: Array<{ event: string; payload: unknown }> = [];
  const events: AppEventsRuntime = {
    emit: async (event, payload) => {
      emitted.push({ event, payload });
      listeners.get(event)?.forEach((listener) => listener({ payload }));
    },
    isAvailable: () => true,
    listen: async (event, listener) => {
      const eventListeners = listeners.get(event) ?? new Set();
      eventListeners.add(listener as (event: RuntimeEvent<unknown>) => unknown);
      listeners.set(event, eventListeners);
      return () => eventListeners.delete(listener as (event: RuntimeEvent<unknown>) => unknown);
    }
  };
  const runtime = createDefaultAppRuntime();
  configureAppRuntime({
    ...runtime,
    events,
    window: {
      ...runtime.window,
      getCurrentWindowLabel: async () => "markra-editor-2"
    }
  });

  return { emitted, events, listeners };
}

function request(requestId: string, patch: Partial<WorkspaceResourceSnapshotRequest> = {}) {
  return {
    requestId,
    sourceWindowLabel: "markra-editor-2",
    workspaceSourcePath: "/vault",
    ...patch
  };
}

afterEach(() => {
  resetAppRuntimeForTests();
});

describe("workspace resource snapshot responder", () => {
  it("returns only open dirty saved documents from the matching editor workspace", async () => {
    const runtime = installRuntime();
    renderHook(() => useWorkspaceResourceSnapshotResponder({
      documentTabs: [
        tab(),
        tab({ id: "untitled:1", name: "Untitled.md", path: null }),
        tab({ dirty: false, id: "file:/vault/clean.md", path: "/vault/clean.md" }),
        tab({ deleted: true, id: "file:/vault/deleted.md", path: "/vault/deleted.md" })
      ],
      workspaceSourcePath: "/vault"
    }));
    await waitFor(() => expect(runtime.listeners.has(workspaceResourceSnapshotRequestEvent)).toBe(true));

    await act(async () => {
      await runtime.events.emit(workspaceResourceSnapshotRequestEvent, request("request-1"));
    });

    const response = runtime.emitted.find(
      ({ event }) => event === workspaceResourceSnapshotResponseEvent
    )?.payload as WorkspaceResourceSnapshotResponse;
    expect(response).toEqual({
      documentGeneration: expect.any(Number),
      dirtyDocuments: [{ content: "# Note", path: "/vault/note.md", revision: 1 }],
      requestId: "request-1",
      sourceWindowLabel: "markra-editor-2",
      workspaceSourcePath: "/vault"
    });
  });

  it("increments generation after relevant content changes and ignores mismatched requests", async () => {
    const runtime = installRuntime();
    const { rerender } = renderHook(
      ({ documentTabs }) => useWorkspaceResourceSnapshotResponder({
        documentTabs,
        workspaceSourcePath: "/vault"
      }),
      { initialProps: { documentTabs: [tab()] } }
    );
    await waitFor(() => expect(runtime.listeners.has(workspaceResourceSnapshotRequestEvent)).toBe(true));

    await act(async () => {
      await runtime.events.emit(workspaceResourceSnapshotRequestEvent, request("first"));
      await runtime.events.emit(workspaceResourceSnapshotRequestEvent, request("wrong-window", {
        sourceWindowLabel: "main"
      }));
      await runtime.events.emit(workspaceResourceSnapshotRequestEvent, request("wrong-workspace", {
        workspaceSourcePath: "/other"
      }));
    });
    const first = runtime.emitted.find(
      ({ event, payload }) => event === workspaceResourceSnapshotResponseEvent &&
        (payload as WorkspaceResourceSnapshotResponse).requestId === "first"
    )?.payload as WorkspaceResourceSnapshotResponse;

    rerender({ documentTabs: [tab({ content: "# Changed", revision: 2 })] });
    await act(async () => {
      await runtime.events.emit(workspaceResourceSnapshotRequestEvent, request("second"));
    });
    const responses = runtime.emitted.filter(
      ({ event }) => event === workspaceResourceSnapshotResponseEvent
    ).map(({ payload }) => payload as WorkspaceResourceSnapshotResponse);
    const second = responses.find((response) => response.requestId === "second");

    expect(responses.map((response) => response.requestId)).toEqual(["first", "second"]);
    expect(second?.documentGeneration).toBeGreaterThan(first.documentGeneration);
    expect(second?.dirtyDocuments[0]).toEqual({
      content: "# Changed",
      path: "/vault/note.md",
      revision: 2
    });
  });
});
