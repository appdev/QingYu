import type {
  KernelCreatedDocumentSnapshot,
  KernelDocumentEntrySnapshot,
  KernelDocumentLocator,
  KernelDocumentPageSnapshot,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelHistoryPageSnapshot,
  KernelHistorySnapshotId,
  KernelPageCursor,
  KernelRevision,
  KernelRuntimeCapabilities,
  KernelRuntimeSnapshot,
  KernelSearchPageSnapshot,
  KernelWorkspaceGeneration,
  KernelWorkspaceRelativePath,
  KernelWorkspaceSnapshot,
} from "@markra/app/runtime";
import type {
  FetchLike,
  KernelClient,
  NativeBearerAuthentication,
} from "@markra/kernel-client";
import { createKernelClient } from "@markra/kernel-client";

interface DesktopKernelConnectionBase {
  readonly authentication: NativeBearerAuthentication;
  readonly baseUrl: string;
  readonly instanceId: string;
  readonly release?: () => unknown;
}

export type DesktopKernelConnection = DesktopKernelConnectionBase & (
  | {
      readonly generation: string;
      readonly processGeneration?: never;
    }
  | {
      readonly generation?: never;
      readonly processGeneration: string;
    }
);

export interface DesktopKernelDomainAdapter {
  readonly port: KernelDomainPort;
  readonly release: () => undefined;
}

export interface DesktopKernelDomainAdapterOptions {
  readonly fetch?: FetchLike;
}

export type DesktopKernelDomainAdapterErrorCode =
  | "initialization-failed"
  | "protocol-mismatch"
  | "released"
  | "workspace-generation-mismatch";

const ERROR_MESSAGES: Record<DesktopKernelDomainAdapterErrorCode, string> = {
  "initialization-failed": "The desktop Kernel adapter could not be initialized.",
  "protocol-mismatch": "The desktop Kernel no longer matches its bootstrap contract.",
  released: "The desktop Kernel adapter has been released.",
  "workspace-generation-mismatch": "The document belongs to a different workspace generation.",
};

export class DesktopKernelDomainAdapterError extends Error {
  readonly code: DesktopKernelDomainAdapterErrorCode;

  constructor(code: DesktopKernelDomainAdapterErrorCode) {
    super(ERROR_MESSAGES[code]);
    this.name = "DesktopKernelDomainAdapterError";
    this.code = code;
  }
}

export async function createDesktopKernelDomainAdapter(
  connection: DesktopKernelConnection,
  options: DesktopKernelDomainAdapterOptions = {},
): Promise<DesktopKernelDomainAdapter> {
  const { baseUrl, instanceId, release } = connection;
  const processGeneration = connection.processGeneration ?? connection.generation;
  let authentication: NativeBearerAuthentication | undefined = connection.authentication;
  let lifecycle: "initializing" | "active" | "closed" = "initializing";
  let ownershipReleased = false;
  const requests = new AbortController();
  const releaseOwnership = () => {
    if (ownershipReleased) return undefined;
    ownershipReleased = true;
    authentication = undefined;
    try {
      release?.();
    } catch {
      // Releasing credentials is best-effort and must not expose provider errors.
    }
    return undefined;
  };
  const close = () => {
    if (lifecycle === "closed") return undefined;
    lifecycle = "closed";
    requests.abort();
    releaseOwnership();
    return undefined;
  };
  const assertActive = () => {
    if (lifecycle !== "active") {
      throw new DesktopKernelDomainAdapterError("released");
    }
  };
  const protocolMismatch = (): never => {
    close();
    throw new DesktopKernelDomainAdapterError("protocol-mismatch");
  };

  try {
    if (!isCanonicalProcessGeneration(processGeneration)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const client = createKernelClient({
      auth: {
        kind: "native-bearer",
        getCredential: () => {
          if (lifecycle === "closed" || authentication === undefined) {
            throw new Error("credential unavailable");
          }
          return authentication.getCredential();
        },
      },
      baseUrl,
      fetch: options.fetch ?? globalThis.fetch,
    });

    const ready = await client.system.ready({ signal: requests.signal });
    if (ready.instanceId !== instanceId) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const runtime = await client.system.runtime({ signal: requests.signal });
    if (!matchesDesktopRuntime(runtime, instanceId)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const workspace = await client.workspace.get({ signal: requests.signal });
    if (!matchesReadyWorkspace(workspace)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }
    const workspaceGeneration = workspace.generation as KernelWorkspaceGeneration;
    const workspaceId = workspace.id;

    lifecycle = "active";
    const assertWorkspaceGeneration = (candidate: KernelWorkspaceGeneration) => {
      if (candidate !== workspaceGeneration) {
        throw new DesktopKernelDomainAdapterError("workspace-generation-mismatch");
      }
    };
    const assertDocumentIdentity = (
      actual: string,
      expected: KernelDocumentLocator,
    ) => {
      if (actual !== expected) protocolMismatch();
    };
    const confirmWorkspaceIdentity = async () => {
      const current = await client.workspace.get({ signal: requests.signal });
      assertActive();
      if (!matchesDesktopWorkspace(current, workspaceId, workspaceGeneration)) {
        protocolMismatch();
      }
    };
    const prepareDocumentOperation = async (candidate: KernelWorkspaceGeneration) => {
      assertActive();
      assertWorkspaceGeneration(candidate);
      await confirmWorkspaceIdentity();
    };
    const port: KernelDomainPort = {
      availability: "available",
      documents: {
        create: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.create(
            input.kind === "file"
              ? {
                  contents: input.contents,
                  kind: input.kind,
                  name: input.name,
                  parent: input.parent,
                  workspaceGeneration: input.workspaceGeneration,
                }
              : {
                  kind: input.kind,
                  name: input.name,
                  parent: input.parent,
                  workspaceGeneration: input.workspaceGeneration,
                },
            { signal: requests.signal },
          );
          assertActive();
          if (
            document.kind !== input.kind ||
            document.name !== input.name ||
            document.parent !== input.parent ||
            document.path !== joinWorkspacePath(input.parent, input.name)
          ) {
            protocolMismatch();
          }
          await confirmWorkspaceIdentity();
          return mapCreatedDocument(document, input.workspaceGeneration);
        },
        delete: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const deleted = await client.documents.delete(
            input.locator,
            {
              deletionPolicy: input.deletionPolicy,
              expectedRevision: input.expectedRevision,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return deleted;
        },
        history: {
          list: async (input) => {
            await prepareDocumentOperation(input.workspaceGeneration);
            const page = await client.documents.listHistory(
              input.locator,
              { cursor: input.cursor, limit: input.limit },
              { signal: requests.signal },
            );
            assertActive();
            for (const entry of page.items) {
              assertDocumentIdentity(entry.documentId, input.locator);
            }
            await confirmWorkspaceIdentity();
            return mapHistoryPage(page, input.workspaceGeneration);
          },
          restore: async (input) => {
            await prepareDocumentOperation(input.workspaceGeneration);
            const document = await client.documents.restoreHistory(
              input.locator,
              input.snapshotId,
              {
                expectedRevision: input.expectedRevision,
                workspaceGeneration: input.workspaceGeneration,
              },
              { signal: requests.signal },
            );
            assertActive();
            assertDocumentIdentity(document.id, input.locator);
            await confirmWorkspaceIdentity();
            return mapDocument(document, input.workspaceGeneration);
          },
        },
        list: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const page = await client.documents.list(
            { cursor: input.cursor, limit: input.limit, parent: input.parent },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return mapDocumentPage(page, input.workspaceGeneration);
        },
        move: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.move(
            input.locator,
            {
              expectedRevision: input.expectedRevision,
              name: input.name,
              targetParent: input.targetParent,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          );
          assertActive();
          assertDocumentIdentity(document.id, input.locator);
          if (
            document.name !== input.name ||
            document.parent !== input.targetParent ||
            document.path !== joinWorkspacePath(input.targetParent, input.name)
          ) {
            protocolMismatch();
          }
          await confirmWorkspaceIdentity();
          return mapDocumentEntry(document, input.workspaceGeneration);
        },
        read: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.get(input.locator, {
            signal: requests.signal,
          });
          assertActive();
          assertDocumentIdentity(document.id, input.locator);
          await confirmWorkspaceIdentity();
          return mapDocument(document, input.workspaceGeneration);
        },
        search: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const page = await client.workspace.search(
            {
              cursor: input.cursor,
              limit: input.limit,
              query: input.query,
            },
            { signal: requests.signal },
          );
          assertActive();
          await confirmWorkspaceIdentity();
          return mapSearchPage(page, input.workspaceGeneration);
        },
        update: async (input) => {
          await prepareDocumentOperation(input.workspaceGeneration);
          const document = await client.documents.update(
            input.locator,
            {
              contents: input.contents,
              expectedRevision: input.expectedRevision,
              workspaceGeneration: input.workspaceGeneration,
            },
            { signal: requests.signal },
          );
          assertActive();
          assertDocumentIdentity(document.id, input.locator);
          await confirmWorkspaceIdentity();
          return mapDocument(document, input.workspaceGeneration);
        },
      },
      runtime: {
        read: async () => {
          assertActive();
          const current = await client.system.runtime({ signal: requests.signal });
          assertActive();
          if (!matchesDesktopRuntime(current, instanceId)) {
            return protocolMismatch();
          }
          return mapRuntime(current);
        },
      },
      workspace: {
        read: async () => {
          assertActive();
          const current = await client.workspace.get({ signal: requests.signal });
          assertActive();
          if (!matchesDesktopWorkspace(current, workspaceId, workspaceGeneration)) {
            return protocolMismatch();
          }
          return mapWorkspace(current);
        },
      },
    };

    return {
      port,
      release: close,
    };
  } catch {
    close();
    throw new DesktopKernelDomainAdapterError("initialization-failed");
  }
}

type RuntimeSource = Awaited<ReturnType<KernelClient["system"]["runtime"]>>;
type WorkspaceSource = Awaited<ReturnType<KernelClient["workspace"]["get"]>>;
type DocumentSource = Awaited<ReturnType<KernelClient["documents"]["get"]>>;
type CreatedDocumentSource = Awaited<ReturnType<KernelClient["documents"]["create"]>>;
type DocumentEntrySource = Awaited<ReturnType<KernelClient["documents"]["move"]>>;
type DocumentPageSource = Awaited<ReturnType<KernelClient["documents"]["list"]>>;
type HistoryPageSource = Awaited<ReturnType<KernelClient["documents"]["listHistory"]>>;
type SearchPageSource = Awaited<ReturnType<KernelClient["workspace"]["search"]>>;

function matchesDesktopRuntime(runtime: RuntimeSource, instanceId: string) {
  return (
    runtime.instanceId === instanceId &&
    runtime.profile === "desktop" &&
    runtime.startupState === "ready" &&
    runtime.capabilities.documents === true
  );
}

function matchesReadyWorkspace(workspace: WorkspaceSource) {
  return workspace.readiness === "ready" && workspace.generation.length > 0;
}

function matchesDesktopWorkspace(
  workspace: WorkspaceSource,
  workspaceId: string,
  generation: KernelWorkspaceGeneration,
) {
  return (
    matchesReadyWorkspace(workspace) &&
    workspace.id === workspaceId &&
    workspace.generation === generation
  );
}

function isCanonicalProcessGeneration(value: string) {
  if (!/^(?:0|[1-9][0-9]*)$/u.test(value) || value.length > 20) return false;
  return BigInt(value) <= BigInt("18446744073709551615");
}

function mapCapabilities(
  capabilities: RuntimeSource["capabilities"],
): KernelRuntimeCapabilities {
  return {
    documents: capabilities.documents,
    history: capabilities.history,
    portableSettings: capabilities.portableSettings,
    s3: capabilities.s3,
    search: capabilities.search,
    settings: capabilities.settings,
    sync: capabilities.sync,
    webdav: capabilities.webdav,
  };
}

function mapRuntime(runtime: RuntimeSource): KernelRuntimeSnapshot {
  return {
    capabilities: mapCapabilities(runtime.capabilities),
    instanceId: runtime.instanceId,
    profile: runtime.profile,
    startupState: runtime.startupState,
  };
}

function mapWorkspace(workspace: WorkspaceSource): KernelWorkspaceSnapshot {
  return {
    displayName: workspace.displayName,
    generation: workspace.generation as KernelWorkspaceGeneration,
    id: workspace.id,
    readiness: workspace.readiness,
    revision: workspace.revision as KernelRevision,
  };
}

function mapDocument(
  document: DocumentSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelDocumentSnapshot {
  return {
    contents: document.contents,
    ...mapDocumentEntry(document, workspaceGeneration),
    kind: "file",
  };
}

function mapCreatedDocument(
  document: CreatedDocumentSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelCreatedDocumentSnapshot {
  if (document.kind === "file") {
    return {
      contents: document.contents,
      ...mapDocumentEntry(document, workspaceGeneration),
      kind: "file",
    };
  }
  return {
    ...mapDocumentEntry(document, workspaceGeneration),
    kind: "directory",
  };
}

function mapDocumentEntry(
  document: DocumentEntrySource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelDocumentEntrySnapshot {
  return {
    kind: document.kind,
    locator: document.id as KernelDocumentLocator,
    modifiedAt: document.modifiedAt,
    name: document.name,
    parent: document.parent as KernelWorkspaceRelativePath,
    relativePath: document.path as KernelWorkspaceRelativePath,
    revision: document.revision as KernelRevision,
    sizeBytes: document.sizeBytes,
    workspaceGeneration,
  };
}

function mapDocumentPage(
  page: DocumentPageSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelDocumentPageSnapshot {
  return {
    items: page.items.map((document) => mapDocumentEntry(document, workspaceGeneration)),
    nextCursor: page.nextCursor as KernelPageCursor | null,
    workspaceGeneration,
  };
}

function mapSearchPage(
  page: SearchPageSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelSearchPageSnapshot {
  return {
    items: page.items.map((item) => ({
      column: item.column,
      document: mapDocumentEntry(item.document, workspaceGeneration),
      line: item.line,
      preview: item.preview,
    })),
    nextCursor: page.nextCursor as KernelPageCursor | null,
    workspaceGeneration,
  };
}

function mapHistoryPage(
  page: HistoryPageSource,
  workspaceGeneration: KernelWorkspaceGeneration,
): KernelHistoryPageSnapshot {
  return {
    items: page.items.map((entry) => ({
      createdAt: entry.createdAt,
      documentLocator: entry.documentId as KernelDocumentLocator,
      revision: entry.revision as KernelRevision,
      sizeBytes: entry.sizeBytes,
      snapshotId: entry.snapshotId as KernelHistorySnapshotId,
      workspaceGeneration,
    })),
    nextCursor: page.nextCursor as KernelPageCursor | null,
    workspaceGeneration,
  };
}

function joinWorkspacePath(parent: string, name: string) {
  return parent === "" ? name : `${parent}/${name}`;
}
