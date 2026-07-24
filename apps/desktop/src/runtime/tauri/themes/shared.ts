import type {
  ThemeActivationPayload,
  ThemeCatalogSnapshot
} from "@markra/app/runtime";
import { convertFileSrc } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { invokeNative } from "../invoke";

export function listNativeThemes() {
  return invokeNative<ThemeCatalogSnapshot>("list_themes");
}

type NativeThemeActivationPayload = Omit<ThemeActivationPayload, "source"> & {
  source:
    | { kind: "inline"; css: string }
    | { kind: "stylesheet"; path: string };
};

function stylesheetHref(path: string, fingerprint: string) {
  const converted = convertFileSrc(path);
  const fragmentIndex = converted.indexOf("#");
  const base = fragmentIndex === -1 ? converted : converted.slice(0, fragmentIndex);
  const fragment = fragmentIndex === -1 ? "" : converted.slice(fragmentIndex);
  const separator = base.endsWith("?") || base.endsWith("&")
    ? ""
    : base.includes("?") ? "&" : "?";

  return `${base}${separator}fingerprint=${encodeURIComponent(fingerprint)}${fragment}`;
}

export async function prepareNativeThemeActivation(id: string, expectedFingerprint: string) {
  const payload = await invokeNative<NativeThemeActivationPayload>("prepare_theme_activation", {
    id,
    expectedFingerprint
  });

  if (payload.source.kind === "inline") {
    return {
      fingerprint: payload.fingerprint,
      id: payload.id,
      source: payload.source,
      token: payload.token
    } satisfies ThemeActivationPayload;
  }

  return {
    ...payload,
    source: {
      kind: "stylesheet" as const,
      href: stylesheetHref(payload.source.path, payload.fingerprint)
    }
  } satisfies ThemeActivationPayload;
}

export function commitNativeThemeActivation(token: string) {
  return invokeNative("commit_theme_activation", { token });
}

export function cancelNativeThemeActivation(token: string) {
  return invokeNative("cancel_theme_activation", { token });
}

export function releaseNativeThemeActivation() {
  return invokeNative("release_theme_activation");
}

export function deleteNativeTheme(id: string, expectedFingerprint: string) {
  return invokeNative("delete_theme", { id, expectedFingerprint });
}

export async function confirmNativeThemeActivation(themeName: string) {
  try {
    return await ask(`Keep the third-party theme “${themeName}”?`, {
      kind: "warning",
      title: "Confirm theme"
    });
  } catch {
    return false;
  }
}
