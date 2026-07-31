import {
  createDefaultAppRuntime,
  createKernelSyncConfigRuntime,
  type AppSyncConfigRuntime,
  type KernelDomainPort,
  type KernelSyncConfigRuntimeOptions,
} from "@markra/app/runtime";

export {
  KernelSyncRunError as ServerSyncRunError,
  type KernelSyncRunErrorCode as ServerSyncRunErrorCode,
} from "@markra/app/runtime";

export type ServerSyncConfigRuntimeOptions = Omit<KernelSyncConfigRuntimeOptions, "local">;

export function createServerSyncConfigRuntime(
  kernel: KernelDomainPort,
  options: ServerSyncConfigRuntimeOptions = {},
): AppSyncConfigRuntime {
  const local = createDefaultAppRuntime().syncConfig;
  return createKernelSyncConfigRuntime(kernel, {
    ...options,
    local: {
      cancelApply: local.cancelApply,
      loadEditing: local.loadEditing,
      requestApply: local.requestApply,
      setEditing: local.setEditing,
      settleApply: local.settleApply,
    },
  });
}
