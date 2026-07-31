import {
  createDefaultAppRuntime,
  createKernelSettingsRuntime,
  type AppSettingsRuntime,
  type KernelDomainPort,
  type KernelSettingsLocalSupport,
} from "@markra/app/runtime";

export function createServerSettingsRuntime(
  kernel: KernelDomainPort,
  local: KernelSettingsLocalSupport = createDefaultAppRuntime().settings,
): AppSettingsRuntime {
  return createKernelSettingsRuntime(kernel, { local });
}
