import { invoke } from "@tauri-apps/api/core";

import {
  createNativeKernelSessionOwner,
  type NativeKernelSessionOwner,
} from "./native-kernel-session";
import { createMobileKernelDomainAdapter } from "./mobile";

const SHARED_BOOTSTRAP_READER = "read_native_kernel_bootstrap";
const MOBILE_BOOTSTRAP_READER = "read_mobile_kernel_bootstrap";
const MOBILE_KERNEL_RETRY = "retry_mobile_kernel_runtime";

export type MobileKernelBootstrapInvoke = (command: string) => Promise<unknown>;

export function invokeMobileKernelBootstrap(
  command: string,
  invokeCommand: MobileKernelBootstrapInvoke = invoke,
): Promise<unknown> {
  if (command !== SHARED_BOOTSTRAP_READER) {
    return Promise.reject(new Error("mobile Kernel bootstrap unavailable"));
  }
  return invokeCommand(MOBILE_BOOTSTRAP_READER);
}

export function createMobileKernelSessionOwner(
  invokeCommand: MobileKernelBootstrapInvoke = invoke,
): NativeKernelSessionOwner {
  return createNativeKernelSessionOwner({
    createDomainAdapter: createMobileKernelDomainAdapter,
    invokeCommand: (command) => invokeMobileKernelBootstrap(command, invokeCommand),
  });
}

export function retryMobileKernelRuntime(
  invokeCommand: MobileKernelBootstrapInvoke = invoke,
): Promise<unknown> {
  return invokeCommand(MOBILE_KERNEL_RETRY);
}
