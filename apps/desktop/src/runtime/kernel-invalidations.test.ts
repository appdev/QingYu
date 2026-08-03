import {
  createKernelFileRuntime,
  createUnavailableKernelDomainPort,
  kernelWorkspaceRoot,
} from "@markra/app/runtime";

import { createDesktopKernelInvalidationBridge } from "./kernel-invalidations";

const identity = {
  generation: "7",
  instanceId: "123e4567-e89b-42d3-a456-426614174000",
};

describe("desktop Kernel invalidation bridge", () => {
  it("maps all event families and snapshot scopes without transport details", () => {
    const bridge = createDesktopKernelInvalidationBridge();
    const listener = vi.fn();
    bridge.source.subscribe(listener);
    const document = {
      id: "document.signature",
      kind: "file" as const,
      modifiedAt: "2026-07-31T00:00:00Z",
      name: "note.md",
      parent: "",
      path: "note.md",
      revision: "revision-1",
      sizeBytes: 4,
    };
    const cases = [
      [{ type: "workspace-changed", workspace: {} }, {
        documentChange: "tree", scopes: ["workspace", "documents", "resources"],
      }],
      [{ document, type: "document-created" }, {
        documentChange: "tree", paths: ["note.md"], scopes: ["documents", "resources"],
      }],
      [{ document, type: "document-changed" }, {
        documentChange: "content", paths: ["note.md"], scopes: ["documents", "resources"],
      }],
      [{ document: { ...document, path: "archive/note.md" }, previousPath: "note.md", type: "document-moved" }, {
        documentChange: "tree",
        paths: ["note.md", "archive/note.md"],
        scopes: ["documents", "resources"],
      }],
      [{ previousPath: "note.md", type: "document-deleted" }, {
        documentChange: "tree", paths: ["note.md"], scopes: ["documents", "resources"],
      }],
      [{ settings: {}, type: "settings-changed" }, { scopes: ["settings"] }],
      [{
        revision: "app-config-revision-1",
        type: "app-config-state-changed",
        workspaceGeneration: "workspace-generation-1",
        workspaceId: "workspace-1",
      }, { scopes: ["app-config"] }],
      [{ config: {}, type: "sync-config-changed" }, { scopes: ["sync-config"] }],
      [{ status: { completionState: "attempting" }, type: "sync-status-changed" }, {
        scopes: ["sync-status"],
      }],
      [{ status: { completionState: "succeeded" }, type: "sync-status-changed" }, {
        documentChange: "snapshot", scopes: ["sync-status", "documents", "resources"],
      }],
    ] as const;

    cases.forEach(([event], index) => bridge.publish({
      ...identity,
      frame: {
        connectionId: identity.instanceId,
        event,
        protocolVersion: 1,
        resource: { kind: "workspace" },
        revision: `revision-${index}`,
        sequence: index + 1,
        type: "event",
      } as never,
      kind: "event",
      scope: "workspace",
    }));
    bridge.publish({
      ...identity,
      kind: "snapshot-required",
      reason: "sequence-gap",
      scopes: ["sync-status"],
    });
    bridge.publish({
      ...identity,
      kind: "snapshot-required",
      reason: "sequence-gap",
      scopes: ["app-config"],
    });

    expect(listener.mock.calls.map(([notice]) => notice)).toEqual([
      ...cases.map(([, expected]) => expected),
      {
        documentChange: "snapshot",
        scopes: ["sync-status", "documents", "resources"],
      },
      { scopes: ["app-config"] },
    ]);
  });

  it("refreshes watched document content and tree after successful sync", async () => {
    const bridge = createDesktopKernelInvalidationBridge();
    const files = createKernelFileRuntime(createUnavailableKernelDomainPort(), {
      invalidations: bridge.source,
    });
    const watchedPath = `${kernelWorkspaceRoot}/note.md`;
    const onChange = vi.fn(async () => undefined);
    const onTreeChange = vi.fn(async () => undefined);
    const stopWatching = await files.watchMarkdownFile(watchedPath, onChange, onTreeChange);

    bridge.publish({
      ...identity,
      frame: {
        connectionId: identity.instanceId,
        event: {
          status: { completionState: "succeeded" },
          type: "sync-status-changed",
        },
        protocolVersion: 1,
        resource: { kind: "sync" },
        revision: "revision-sync-succeeded",
        sequence: 1,
        type: "event",
      } as never,
      kind: "event",
      scope: "sync-status",
    });

    await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(watchedPath));
    expect(onTreeChange).toHaveBeenCalledWith(watchedPath);

    stopWatching();
    bridge.close();
  });

  it("closes subscriptions and ignores late invalidations", () => {
    const bridge = createDesktopKernelInvalidationBridge();
    const listener = vi.fn();
    const unsubscribe = bridge.source.subscribe(listener);

    bridge.close();
    bridge.close();
    unsubscribe();
    bridge.publish({
      ...identity,
      kind: "snapshot-required",
      reason: "reconnect",
      scopes: ["workspace"],
    });

    expect(bridge.source.available).toBe(false);
    expect(listener).not.toHaveBeenCalled();
  });

  it("stops a publication when a listener closes or removes a later subscription", () => {
    const invalidation = {
      ...identity,
      kind: "snapshot-required" as const,
      reason: "reconnect" as const,
      scopes: ["workspace" as const],
    };
    const closingBridge = createDesktopKernelInvalidationBridge();
    const afterClose = vi.fn();
    closingBridge.source.subscribe(() => closingBridge.close());
    closingBridge.source.subscribe(afterClose);

    closingBridge.publish(invalidation);

    expect(afterClose).not.toHaveBeenCalled();

    const unsubscribingBridge = createDesktopKernelInvalidationBridge();
    const afterUnsubscribe = vi.fn();
    let unsubscribeLater: () => unknown = () => undefined;
    unsubscribingBridge.source.subscribe(() => unsubscribeLater());
    unsubscribeLater = unsubscribingBridge.source.subscribe(afterUnsubscribe);

    unsubscribingBridge.publish(invalidation);

    expect(afterUnsubscribe).not.toHaveBeenCalled();
  });
});
