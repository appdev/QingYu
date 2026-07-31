import {
  createKernelFileRuntimeOwner,
  kernelWorkspaceRoot,
  type KernelDomainPort,
  type KernelFileRuntimeOwner,
  type KernelImageSource,
  type KernelResourceBody,
} from "@markra/app/runtime";
import { KernelApiError } from "@markra/kernel-client";

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
  const lifecycle = new AbortController();
  return Object.freeze({
    materialize: async (_resource, open, signal) => {
      const body = await openServerImageWithRetry(open, signal, lifecycle.signal);
      assertImageSourceActive(signal, lifecycle.signal);
      const source = objectUrls.createObjectURL(body.body);
      active.add(source);
      return source;
    },
    release: (source: string) => {
      if (!active.delete(source)) return undefined;
      objectUrls.revokeObjectURL(source);
      return undefined;
    },
    close: () => {
      lifecycle.abort();
      return undefined;
    },
  } satisfies KernelImageSource);
}

const imageAuthenticationRetryDelaysMs = [25, 75, 225] as const;

async function openServerImageWithRetry(
  open: () => Promise<KernelResourceBody>,
  signal: AbortSignal | null | undefined,
  lifecycleSignal: AbortSignal,
) {
  for (let attempt = 0; ; attempt += 1) {
    assertImageSourceActive(signal, lifecycleSignal);
    try {
      const body = await open();
      assertImageSourceActive(signal, lifecycleSignal);
      return body;
    } catch (error: unknown) {
      assertImageSourceActive(signal, lifecycleSignal);
      const delay = imageAuthenticationRetryDelaysMs[attempt];
      if (!isRetryableImageAuthenticationError(error) || delay === undefined) {
        if (isRetryableImageAuthenticationError(error)) {
          throw new Error("The Server image preview is temporarily unavailable.");
        }
        throw error;
      }
      await waitForImageAuthentication(delay, signal, lifecycleSignal);
    }
  }
}

function isRetryableImageAuthenticationError(error: unknown) {
  return error instanceof KernelApiError &&
    error.code === "authentication_unavailable" &&
    error.status === 503;
}

function waitForImageAuthentication(
  delayMs: number,
  signal: AbortSignal | null | undefined,
  lifecycleSignal: AbortSignal,
) {
  assertImageSourceActive(signal, lifecycleSignal);
  return new Promise<undefined>((resolve, reject) => {
    let settled = false;
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      globalThis.clearTimeout(timer);
      signal?.removeEventListener("abort", cancel);
      lifecycleSignal.removeEventListener("abort", cancel);
      if (error === undefined) resolve(undefined);
      else reject(error);
    };
    const cancel = () => finish(imageSourceCancelled());
    const timer = globalThis.setTimeout(() => finish(), delayMs);
    signal?.addEventListener("abort", cancel, { once: true });
    lifecycleSignal.addEventListener("abort", cancel, { once: true });
  });
}

function assertImageSourceActive(
  signal: AbortSignal | null | undefined,
  lifecycleSignal: AbortSignal,
) {
  if (signal?.aborted === true || lifecycleSignal.aborted) throw imageSourceCancelled();
}

function imageSourceCancelled() {
  const error = new Error("The Server image preview request was cancelled.");
  error.name = "AbortError";
  return error;
}

function confirmInBrowser(message: string) {
  return typeof globalThis.confirm === "function" ? globalThis.confirm(message) : false;
}
