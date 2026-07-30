import type {
  KernelDocumentLocator,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelRevision,
  KernelRuntimeCapabilities,
  KernelRuntimeSnapshot,
  KernelWorkspaceGeneration,
  KernelWorkspaceSnapshot,
} from "@markra/app/runtime";
import type {
  FetchLike,
  KernelClient,
  NativeBearerAuthentication,
} from "@markra/kernel-client";
import { createKernelClient } from "@markra/kernel-client";

export interface DesktopKernelConnection {
  readonly authentication: NativeBearerAuthentication;
  readonly baseUrl: string;
  readonly generation: string;
  readonly instanceId: string;
  readonly release?: () => unknown;
}

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
  const { baseUrl, generation, instanceId, release } = connection;
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
    if (!matchesDesktopWorkspace(workspace, generation)) {
      throw new DesktopKernelDomainAdapterError("initialization-failed");
    }

    lifecycle = "active";
    const assertWorkspaceGeneration = (candidate: KernelWorkspaceGeneration) => {
      if (candidate !== generation) {
        throw new DesktopKernelDomainAdapterError("workspace-generation-mismatch");
      }
    };
    const assertDocumentIdentity = (
      actual: string,
      expected: KernelDocumentLocator,
    ) => {
      if (actual !== expected) protocolMismatch();
    };
    const port: KernelDomainPort = {
      availability: "available",
      documents: {
        read: async (input) => {
          assertActive();
          assertWorkspaceGeneration(input.workspaceGeneration);
          const document = await client.documents.get(input.locator, {
            signal: requests.signal,
          });
          assertActive();
          assertDocumentIdentity(document.id, input.locator);
          return mapDocument(document, input.workspaceGeneration);
        },
        update: async (input) => {
          assertActive();
          assertWorkspaceGeneration(input.workspaceGeneration);
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
          if (!matchesDesktopWorkspace(current, generation)) {
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

function matchesDesktopRuntime(runtime: RuntimeSource, instanceId: string) {
  return (
    runtime.instanceId === instanceId &&
    runtime.profile === "desktop" &&
    runtime.startupState === "ready" &&
    runtime.capabilities.documents === true
  );
}

function matchesDesktopWorkspace(workspace: WorkspaceSource, generation: string) {
  return workspace.generation === generation && workspace.readiness === "ready";
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
    locator: document.id as KernelDocumentLocator,
    modifiedAt: document.modifiedAt,
    name: document.name,
    revision: document.revision as KernelRevision,
    sizeBytes: document.sizeBytes,
    workspaceGeneration,
  };
}
