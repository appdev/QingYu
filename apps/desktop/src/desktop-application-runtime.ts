import type { AppRuntime } from "@markra/app/runtime";

import {
  createDesktopApplicationMountOwner,
  type DesktopApplicationMountOptions,
  type DesktopApplicationMountOwner,
} from "./desktop-application";
import { createDesktopKernelRuntimeOwner } from "./runtime/desktop";
import {
  createNativeKernelSessionOwner,
  type NativeKernelSessionOwner,
} from "./runtime/native-kernel-session";

export type ProductionDesktopApplicationMountOptions = Pick<
  DesktopApplicationMountOptions<AppRuntime>,
  "configureRuntime" | "renderDomain" | "renderStartup"
> & {
  readonly owner?: NativeKernelSessionOwner;
};

export function createProductionDesktopApplicationMountOwner(
  options: ProductionDesktopApplicationMountOptions,
): DesktopApplicationMountOwner {
  return createDesktopApplicationMountOwner({
    configureRuntime: options.configureRuntime,
    createRuntime: createDesktopKernelRuntimeOwner,
    owner: options.owner ?? createNativeKernelSessionOwner(),
    renderDomain: options.renderDomain,
    renderStartup: options.renderStartup,
  });
}
