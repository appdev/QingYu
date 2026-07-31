import {
  createKernelFileRuntimeOwner,
  kernelWorkspaceRoot,
  type KernelDomainPort,
  type KernelFileRuntimeOwner,
  type KernelImageSource,
} from "@markra/app/runtime";

import {
  ServerKernelDomainAdapterError,
} from "./kernel";

export const serverWorkspaceRoot = kernelWorkspaceRoot;

export interface ServerFileRuntimeOptions {
  /** Security sentinels accepted by tests but deliberately never consulted. */
  readonly indexedDB?: unknown;
  readonly showDirectoryPicker?: unknown;
  readonly pollIntervalMs?: number;
  readonly setInterval?: typeof globalThis.setInterval;
  readonly clearInterval?: typeof globalThis.clearInterval;
  readonly objectUrls?: ServerObjectUrlApi;
}

export interface ServerObjectUrlApi {
  readonly createObjectURL: (blob: Blob) => string;
  readonly revokeObjectURL: (url: string) => unknown;
}

export function createServerFileRuntime(
  kernel: KernelDomainPort,
  options: ServerFileRuntimeOptions = {},
) {
  return createServerFileRuntimeOwner(kernel, options).files;
}

export function createServerFileRuntimeOwner(
  kernel: KernelDomainPort,
  options: ServerFileRuntimeOptions = {},
): KernelFileRuntimeOwner {
  return createKernelFileRuntimeOwner(kernel, {
    clearInterval: options.clearInterval,
    imageSource: createServerKernelImageSource(options.objectUrls),
    invalidations: kernel.invalidations,
    isTerminalError: (error) => error instanceof ServerKernelDomainAdapterError &&
      (error.code === "authentication-required" || error.code === "released"),
    nativeShell: {
      confirmMarkdownFileDelete: async (_fileName, labels) => confirmInBrowser(labels.message),
      confirmUnsavedMarkdownDocumentDiscard: async (_fileName, labels) =>
        confirmInBrowser(labels.message),
    },
    pollIntervalMs: options.pollIntervalMs,
    resolveImageSrc: (resource) =>
      `/api/v1/resources/${encodeURIComponent(resource.id)}?kind=image`,
    setInterval: options.setInterval,
  });
}

function createServerKernelImageSource(
  objectUrls: ServerObjectUrlApi = URL,
): KernelImageSource {
  const active = new Set<string>();
  return Object.freeze({
    // Existing resources remain signed same-origin URLs. Only a newly uploaded
    // Blob needs a temporary object URL to avoid immediately downloading the
    // bytes that the browser already owns.
    materialize: async () => undefined,
    materializeCreated: async (_resource, body) => {
      const source = objectUrls.createObjectURL(body.body);
      active.add(source);
      return source;
    },
    release: (source: string) => {
      if (!active.delete(source)) return undefined;
      objectUrls.revokeObjectURL(source);
      return undefined;
    },
  } satisfies KernelImageSource);
}

function confirmInBrowser(message: string) {
  return typeof globalThis.confirm === "function" ? globalThis.confirm(message) : false;
}
