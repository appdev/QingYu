import type { AppDialogRuntime } from "@markra/app/runtime";
import type { WebRuntimeOptions } from "./types";

export function createWebDialogRuntime(_options: WebRuntimeOptions): AppDialogRuntime {
  return {
    showAppAbout: async () => undefined,
    showPandocSetup: async () => "cancel"
  };
}
