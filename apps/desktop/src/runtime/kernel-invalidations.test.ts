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
      [{ config: {}, type: "sync-config-changed" }, { scopes: ["sync-config"] }],
      [{ status: { completionState: "attempting" }, type: "sync-status-changed" }, {
        scopes: ["sync-status"],
      }],
      [{ status: { completionState: "succeeded" }, type: "sync-status-changed" }, {
        documentChange: "tree", scopes: ["sync-status", "documents", "resources"],
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

    expect(listener.mock.calls.map(([notice]) => notice)).toEqual([
      ...cases.map(([, expected]) => expected),
      {
        documentChange: "snapshot",
        scopes: ["sync-status", "documents", "resources"],
      },
    ]);
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
});
