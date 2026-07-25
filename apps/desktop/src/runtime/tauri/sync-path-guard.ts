import type { AppSyncPathGuardRuntime } from "@markra/app/runtime";
import { invokeNative } from "./invoke";

export function acknowledgeNativePathGuard(
  input: Parameters<AppSyncPathGuardRuntime["acknowledge"]>[0]
) {
  return invokeNative("acknowledge_path_guard", { request: input });
}
