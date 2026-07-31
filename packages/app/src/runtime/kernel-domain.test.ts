import { createUnavailableKernelDomainPort } from "./kernel-domain";
import type {
  KernelCreateDocumentInput,
  KernelDeleteDocumentInput,
  KernelDocumentEntrySnapshot,
  KernelDocumentLocator,
  KernelDocumentPageSnapshot,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelHistoryPageSnapshot,
  KernelHistorySnapshot,
  KernelHistorySnapshotId,
  KernelInventorySnapshot,
  KernelListDocumentsInput,
  KernelMoveDocumentInput,
  KernelPageCursor,
  KernelRevision,
  KernelRuntimeSnapshot,
  KernelSearchDocumentsInput,
  KernelSearchPageSnapshot,
  KernelSyncSafeErrorSnapshot,
  KernelUpdateDocumentInput,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
  KernelWorkspaceSnapshot,
} from "./kernel-domain";

type ForbiddenHostKey =
  | "absolutePath"
  | "endpoint"
  | "host"
  | "origin"
  | "port"
  | "rootPath"
  | "token";

describe("KernelDomainPort", () => {
  it("keeps application DTOs free of host and absolute-path fields", () => {
    expectTypeOf<Extract<keyof KernelRuntimeSnapshot, ForbiddenHostKey>>().toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelWorkspaceSnapshot, ForbiddenHostKey>>().toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelDocumentSnapshot, ForbiddenHostKey>>().toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelDocumentEntrySnapshot, ForbiddenHostKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelDocumentPageSnapshot, ForbiddenHostKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelSearchPageSnapshot, ForbiddenHostKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelHistoryPageSnapshot, ForbiddenHostKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelHistorySnapshot, ForbiddenHostKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelInventorySnapshot, ForbiddenHostKey>>()
      .toEqualTypeOf<never>();
    expectTypeOf<Extract<keyof KernelSyncSafeErrorSnapshot, "objectId">>()
      .toEqualTypeOf<never>();
  });

  it("keeps paths, cursors, and history snapshot identities opaque", () => {
    expectTypeOf<string>().not.toMatchTypeOf<KernelWorkspaceRelativePath>();
    expectTypeOf<string>().not.toMatchTypeOf<KernelPageCursor>();
    expectTypeOf<string>().not.toMatchTypeOf<KernelHistorySnapshotId>();
  });

  it("requires the handshake generation on every document-tree operation", () => {
    expectTypeOf<KernelListDocumentsInput>().toMatchTypeOf<{
      cursor?: KernelPageCursor;
      limit?: number;
      parent?: KernelWorkspaceRelativePath;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>();
    expectTypeOf<KernelSearchDocumentsInput>().toMatchTypeOf<{
      cursor?: KernelPageCursor;
      limit?: number;
      query: string;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>();
    expectTypeOf<KernelCreateDocumentInput>().toMatchTypeOf<
      | {
          contents: string;
          kind: "file";
          name: string;
          parent: KernelWorkspaceRelativePath;
          workspaceGeneration: KernelWorkspaceGeneration;
        }
      | {
          kind: "directory";
          name: string;
          parent: KernelWorkspaceRelativePath;
          workspaceGeneration: KernelWorkspaceGeneration;
        }
    >();
  });

  it("requires an explicit revision and deletion policy for existing-resource mutations", () => {
    expectTypeOf<KernelMoveDocumentInput>().toMatchTypeOf<{
      expectedRevision: KernelRevision;
      locator: KernelDocumentLocator;
      name: string;
      targetParent: KernelWorkspaceRelativePath;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>();
    expectTypeOf<KernelDeleteDocumentInput>().toMatchTypeOf<{
      deletionPolicy: "recoverable" | "permanent";
      expectedRevision: KernelRevision;
      locator: KernelDocumentLocator;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>();
  });

  it("requires opaque locators and optimistic workspace/document revisions for updates", () => {
    expectTypeOf<string>().not.toMatchTypeOf<KernelDocumentLocator>();
    expectTypeOf<string>().not.toMatchTypeOf<KernelWorkspaceGeneration>();
    expectTypeOf<string>().not.toMatchTypeOf<KernelRevision>();
    expectTypeOf<Parameters<KernelDomainPort["documents"]["update"]>[0]>()
      .toEqualTypeOf<KernelUpdateDocumentInput>();
    expectTypeOf<KernelUpdateDocumentInput>().toMatchTypeOf<{
      contents: string;
      expectedRevision: KernelRevision;
      locator: KernelDocumentLocator;
      workspaceGeneration: KernelWorkspaceGeneration;
    }>();
  });

  it("requires history reads, resource bodies, and invalidations on every adapter", () => {
    expectTypeOf<KernelDomainPort["documents"]["history"]["read"]>()
      .toBeFunction();
    expectTypeOf<KernelDomainPort["resources"]>().toBeObject();
    expectTypeOf<KernelDomainPort["invalidations"]>().toBeObject();
  });

  it("fails closed when no Kernel adapter is installed", async () => {
    const port = createUnavailableKernelDomainPort();

    expect(port.availability).toBe("unavailable");
    await expect(port.runtime.read()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
    await expect(port.workspace.read()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
    await expect(port.settings.read()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
    await expect(port.settings.patch({
      expectedRevision: "settings-revision" as KernelRevision,
      values: [],
    })).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(port.sync.readConfig()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
    await expect(port.sync.patchConfig({
      changes: {},
      expectedRevision: "sync-revision" as KernelRevision,
    })).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(port.sync.readStatus()).rejects.toMatchObject({
      name: "KernelDomainUnavailableError",
    });
    await expect(port.sync.testConnection({
      changes: {},
      expectedRevision: "sync-revision" as KernelRevision,
    })).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(
      port.sync.trigger("sync-revision" as KernelRevision),
    ).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(
      port.documents.list({
        workspaceGeneration: "generation" as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(
      port.documents.search({
        query: "needle",
        workspaceGeneration: "generation" as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(
      port.documents.history.read({
        locator: "document" as KernelDocumentLocator,
        snapshotId: "snapshot" as KernelHistorySnapshotId,
        workspaceGeneration: "generation" as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(
      port.resources.list({
        workspaceGeneration: "generation" as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    await expect(
      port.resources.open({
        id: "resource",
        kind: "image",
        workspaceGeneration: "generation" as KernelWorkspaceGeneration,
      }),
    ).rejects.toMatchObject({ name: "KernelDomainUnavailableError" });
    expect(port.invalidations.available).toBe(false);
  });
});
