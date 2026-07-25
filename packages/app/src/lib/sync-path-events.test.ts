import {
  editorReadOnlyForTarget,
  editorReadOnlyForPath,
  guardedPathsForRequests,
  parseSyncPathGuardRelease,
  parseSyncPathGuardRequest,
  syncExistingDocumentWriteBlockedByGuardedPath,
  syncSaveBlockedByGuardedPath,
  syncMutationIntersectsGuardedPaths
} from "./sync-path-events";

describe("sync path event contract", () => {
  const root = "/notes";
  const request = {
    jobId: "319b5308-1e93-4909-95ac-cd198cc454ac",
    notesRoot: root,
    relativePaths: ["folder/a.md"],
    requestId: "e728a5d6-31ed-490d-bb8a-8f15cb550e74"
  };

  it("rejects malformed identities, traversal paths, and mismatched release payloads", () => {
    expect(parseSyncPathGuardRequest(request)).toEqual(request);
    expect(parseSyncPathGuardRequest({ ...request, requestId: "UPPER" })).toBeNull();
    expect(parseSyncPathGuardRequest({ ...request, relativePaths: ["../secret.md"] })).toBeNull();
    expect(parseSyncPathGuardRequest({ ...request, relativePaths: ["folder\\a.md"] })).toBeNull();
    expect(parseSyncPathGuardRequest({ ...request, notesRoot: "/notes/../other" })).toBeNull();
    expect(parseSyncPathGuardRelease({
      notesRoot: root,
      relativePaths: request.relativePaths,
      requestId: request.requestId
    })).toEqual({
      notesRoot: root,
      relativePaths: request.relativePaths,
      requestId: request.requestId
    });
    expect(parseSyncPathGuardRelease({
      notesRoot: root,
      relativePaths: ["folder/b.md"],
      requestId: request.requestId
    }, request)).toBeNull();
  });

  it("keeps overlapping request paths guarded until their own release", () => {
    const requests = new Map([
      ["one", new Set(["/notes/shared.md", "/notes/one.md"])],
      ["two", new Set(["/notes/shared.md", "/notes/two.md"])]
    ]);
    expect([...guardedPathsForRequests(requests)].sort()).toEqual([
      "/notes/one.md",
      "/notes/shared.md",
      "/notes/two.md"
    ]);
    requests.delete("one");
    expect([...guardedPathsForRequests(requests)].sort()).toEqual([
      "/notes/shared.md",
      "/notes/two.md"
    ]);
  });

  it("computes main and side editor readonly state from each editor's own path", () => {
    const guarded = new Set(["/notes/side.md"]);
    expect(editorReadOnlyForPath(false, "/notes/main.md", guarded)).toBe(false);
    expect(editorReadOnlyForPath(false, "/notes/side.md", guarded)).toBe(true);
    expect(editorReadOnlyForPath(true, "/notes/main.md", guarded)).toBe(true);
  });

  it("uses the focused side document path for global editing commands", () => {
    const guarded = new Set(["/notes/side.md"]);
    expect(editorReadOnlyForTarget(
      "main",
      "/notes/main.md",
      "/notes/side.md",
      false,
      guarded
    )).toBe(false);
    expect(editorReadOnlyForTarget(
      "side",
      "/notes/main.md",
      "/notes/side.md",
      false,
      guarded
    )).toBe(true);
  });

  it("blocks only mutations whose exact target or subtree intersects a guard", () => {
    const guarded = new Set(["/notes/folder/guarded.md"]);
    expect(syncMutationIntersectsGuardedPaths({ sourcePath: "/notes/folder" }, guarded)).toBe(true);
    expect(syncMutationIntersectsGuardedPaths({ sourcePath: "/notes/folder/guarded.md" }, guarded)).toBe(true);
    expect(syncMutationIntersectsGuardedPaths({ destinationPath: "/notes/folder/other.md" }, guarded)).toBe(false);
    expect(syncMutationIntersectsGuardedPaths({ sourcePath: "/notes/unrelated.md" }, guarded)).toBe(false);
  });

  it("blocks ordinary saves only for the selected guarded document", () => {
    const guarded = new Set(["/notes/main.md", "/notes/side.md"]);
    expect(syncSaveBlockedByGuardedPath(false, "/notes/main.md", guarded)).toBe(true);
    expect(syncSaveBlockedByGuardedPath(false, "/notes/side.md", guarded)).toBe(true);
    expect(syncSaveBlockedByGuardedPath(false, "/notes/other.md", guarded)).toBe(false);
    expect(syncSaveBlockedByGuardedPath(true, "/notes/main.md", guarded)).toBe(false);
    expect(syncExistingDocumentWriteBlockedByGuardedPath("/notes/main.md", guarded)).toBe(true);
    expect(syncExistingDocumentWriteBlockedByGuardedPath("/notes/other.md", guarded)).toBe(false);
  });
});
