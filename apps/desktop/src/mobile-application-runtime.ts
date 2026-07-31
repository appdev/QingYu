import type { AppRuntime } from "@markra/app/runtime";

import {
  createDesktopApplicationMountOwner,
  type DesktopApplicationMountOptions,
  type DesktopApplicationMountOwner,
} from "./desktop-application";
import { createMobileKernelRuntimeOwner } from "./runtime/mobile";
import { createMobileKernelSessionOwner } from "./runtime/mobile-kernel-session";
import type { NativeKernelSessionOwner } from "./runtime/native-kernel-session";

export type ProductionMobileApplicationMountOptions = Pick<
  DesktopApplicationMountOptions<AppRuntime>,
  "configureRuntime" | "renderDomain" | "renderStartup"
> & {
  readonly owner?: NativeKernelSessionOwner;
};

export function createProductionMobileApplicationMountOwner(
  options: ProductionMobileApplicationMountOptions,
): DesktopApplicationMountOwner {
  return createDesktopApplicationMountOwner({
    configureRuntime: options.configureRuntime,
    createRuntime: createMobileKernelRuntimeOwner,
    owner: options.owner ?? createMobileKernelSessionOwner(),
    renderDomain: options.renderDomain,
    renderStartup: options.renderStartup,
  });
}
