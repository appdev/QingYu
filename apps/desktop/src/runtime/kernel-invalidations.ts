import type {
  KernelInvalidationNotice,
  KernelInvalidationScope,
  KernelInvalidationSource,
  KernelWorkspaceRelativePath,
} from "@markra/app/runtime";

import type {
  DesktopKernelDomainInvalidation,
  DesktopKernelDomainScope,
} from "./kernel-events";

export interface DesktopKernelInvalidationBridge {
  readonly source: KernelInvalidationSource;
  readonly publish: (invalidation: DesktopKernelDomainInvalidation) => undefined;
  readonly close: () => undefined;
}

export function createDesktopKernelInvalidationBridge(): DesktopKernelInvalidationBridge {
  const listeners = new Set<(notice: KernelInvalidationNotice) => unknown>();
  let active = true;
  const source: KernelInvalidationSource = Object.freeze({
    get available() {
      return active;
    },
    subscribe: (listener) => {
      if (!active) return () => undefined;
      listeners.add(listener);
      let subscribed = true;
      return () => {
        if (!subscribed) return undefined;
        subscribed = false;
        listeners.delete(listener);
        return undefined;
      };
    },
  });
  return Object.freeze({
    source,
    publish: (invalidation: DesktopKernelDomainInvalidation) => {
      if (!active) return undefined;
      const notice = mapDesktopKernelInvalidation(invalidation);
      for (const listener of [...listeners]) {
        if (!active) break;
        if (!listeners.has(listener)) continue;
        try {
          listener(notice);
        } catch {
          // One runtime consumer cannot interrupt the session event owner.
        }
      }
      return undefined;
    },
    close: () => {
      if (!active) return undefined;
      active = false;
      listeners.clear();
      return undefined;
    },
  });
}

function mapDesktopKernelInvalidation(
  invalidation: DesktopKernelDomainInvalidation,
): KernelInvalidationNotice {
  if (invalidation.kind === "snapshot-required") {
    const scopes = expandReloadScopes(invalidation.scopes);
    return {
      ...(scopes.includes("documents") ? { documentChange: "snapshot" as const } : {}),
      scopes,
    };
  }
  const event = invalidation.frame.event;
  switch (event.type) {
    case "workspace-changed":
      return {
        documentChange: "tree",
        scopes: ["workspace", "documents", "resources"],
      };
    case "document-created":
      return {
        documentChange: "tree",
        paths: [event.document.path as KernelWorkspaceRelativePath],
        scopes: ["documents", "resources"],
      };
    case "document-changed":
      return {
        documentChange: "content",
        paths: [event.document.path as KernelWorkspaceRelativePath],
        scopes: ["documents", "resources"],
      };
    case "document-moved":
      return {
        documentChange: "tree",
        paths: [event.previousPath, event.document.path] as KernelWorkspaceRelativePath[],
        scopes: ["documents", "resources"],
      };
    case "document-deleted":
      return {
        documentChange: "tree",
        paths: [event.previousPath as KernelWorkspaceRelativePath],
        scopes: ["documents", "resources"],
      };
    case "settings-changed":
      return { scopes: ["settings"] };
    case "app-config-state-changed":
      return { scopes: ["app-config"] };
    case "sync-config-changed":
      return { scopes: ["sync-config"] };
    case "sync-status-changed":
      return event.status.completionState === "succeeded"
        ? {
            documentChange: "snapshot",
            scopes: ["sync-status", "documents", "resources"],
          }
        : { scopes: ["sync-status"] };
  }
}

function expandReloadScopes(
  scopes: readonly DesktopKernelDomainScope[],
): KernelInvalidationScope[] {
  const expanded = new Set<KernelInvalidationScope>();
  for (const scope of scopes) {
    if (scope === "workspace") {
      expanded.add("workspace");
      expanded.add("documents");
      expanded.add("resources");
    } else if (scope === "documents") {
      expanded.add("documents");
      expanded.add("resources");
    } else {
      expanded.add(scope);
      if (scope === "sync-status") {
        expanded.add("documents");
        expanded.add("resources");
      }
    }
  }
  return [...expanded];
}
