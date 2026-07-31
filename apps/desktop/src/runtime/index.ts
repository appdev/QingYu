import { platform as readTauriPlatform } from "@tauri-apps/plugin-os";
import type { AppRuntime } from "@markra/app/runtime";

export type NativeRuntimeKind = "desktop" | "mobile";

type NativeRuntimeLoaders = {
  desktop: () => Promise<{ loadDesktopRuntime: () => Promise<AppRuntime> }>;
  mobile: () => Promise<{ mobileRuntime: AppRuntime }>;
};

const nativeRuntimeLoaders: NativeRuntimeLoaders = {
  desktop: () => import("./desktop"),
  mobile: () => import("./mobile")
};

export function nativeRuntimeKind(platform: string | null | undefined): NativeRuntimeKind {
  return platform === "android" || platform === "ios" ? "mobile" : "desktop";
}

export function readNativeRuntimeKind(
  readPlatform: () => string = readTauriPlatform,
): NativeRuntimeKind {
  try {
    return nativeRuntimeKind(readPlatform());
  } catch {
    return "desktop";
  }
}

export async function loadNativeRuntime(
  readPlatform: () => string = readTauriPlatform,
  loaders: NativeRuntimeLoaders = nativeRuntimeLoaders
): Promise<AppRuntime> {
  if (readNativeRuntimeKind(readPlatform) === "mobile") {
    return (await loaders.mobile()).mobileRuntime;
  }

  return (await loaders.desktop()).loadDesktopRuntime();
}
