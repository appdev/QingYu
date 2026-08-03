import type { AppDialogRuntime } from "@markra/app/runtime";
import type { WebRuntimeOptions } from "./types";

export function createWebDialogRuntime(_options: WebRuntimeOptions): AppDialogRuntime {
  return {
    confirm: async (message) => (
      typeof globalThis.confirm === "function" ? globalThis.confirm(message) : false
    ),
    showAppAbout: async () => undefined,
    showPandocSetup: async () => "cancel"
  };
}
