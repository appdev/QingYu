import { platform as readTauriPlatform } from "@tauri-apps/plugin-os";
import type { AppRuntime } from "@markra/app/runtime";

export type NativeRuntimeKind = "desktop" | "mobile";

type NativeRuntimeLoaders = {
  desktop: () => Promise<{ desktopRuntime: AppRuntime }>;
  mobile: () => Promise<{ mobileRuntime: AppRuntime }>;
};

const nativeRuntimeLoaders: NativeRuntimeLoaders = {
  desktop: () => import("./desktop"),
  mobile: () => import("./mobile")
};

export function nativeRuntimeKind(platform: string | null | undefined): NativeRuntimeKind {
  return platform === "android" || platform === "ios" ? "mobile" : "desktop";
}

export async function loadNativeRuntime(
  readPlatform: () => string = readTauriPlatform,
  loaders: NativeRuntimeLoaders = nativeRuntimeLoaders
): Promise<AppRuntime> {
  let platform: string;

  try {
    platform = readPlatform();
  } catch {
    return (await loaders.desktop()).desktopRuntime;
  }

  if (nativeRuntimeKind(platform) === "mobile") {
    return (await loaders.mobile()).mobileRuntime;
  }

  return (await loaders.desktop()).desktopRuntime;
}
