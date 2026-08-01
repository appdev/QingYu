import {
  createKernelSettingsRuntime,
  type AppSettingsRuntime,
  type KernelDomainPort,
  type KernelSettingsLocalSupport,
  type KernelSettingsSnapshot,
} from "@markra/app/runtime";

export function createServerSettingsRuntime(
  kernel: KernelDomainPort,
  bootstrap: KernelSettingsSnapshot = kernel.appConfig.bootstrap.settings,
  local: KernelSettingsLocalSupport = {},
): AppSettingsRuntime {
  return createKernelSettingsRuntime(kernel, bootstrap, { local });
}
