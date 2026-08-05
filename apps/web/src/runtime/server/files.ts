import {
  createKernelFileRuntimeOwner,
  kernelWorkspaceRoot,
  type KernelDomainPort,
  type KernelFileRuntimeOwner,
} from "@markra/app/runtime";

import {
  ServerKernelDomainAdapterError,
} from "./kernel";
import {
  createBrowserSettingsFileShell,
  type BrowserSettingsFileShellOptions,
} from "../web/settings-file";

export const serverWorkspaceRoot = kernelWorkspaceRoot;

export interface ServerFileRuntimeOptions extends BrowserSettingsFileShellOptions {
  /** Security sentinels accepted by tests but deliberately never consulted. */
  readonly indexedDB?: unknown;
  readonly showDirectoryPicker?: unknown;
  readonly pollIntervalMs?: number;
  readonly setInterval?: typeof globalThis.setInterval;
  readonly clearInterval?: typeof globalThis.clearInterval;
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
    invalidations: kernel.invalidations,
    isTerminalError: (error) => error instanceof ServerKernelDomainAdapterError &&
      (error.code === "authentication-required" || error.code === "released"),
    nativeShell: {
      ...createBrowserSettingsFileShell(options),
      confirmMarkdownFileDelete: async (_fileName, labels) => confirmInBrowser(labels.message),
      confirmUnsavedMarkdownDocumentDiscard: async (_fileName, labels) =>
        confirmInBrowser(labels.message),
    },
    pollIntervalMs: options.pollIntervalMs,
    setInterval: options.setInterval,
  });
}

function confirmInBrowser(message: string) {
  return typeof globalThis.confirm === "function" ? globalThis.confirm(message) : false;
}
