import type {
  ThemeActivationPayload,
  ThemeCatalogSnapshot
} from "@markra/app/runtime";
import { convertFileSrc } from "@tauri-apps/api/core";
import { invokeNative } from "../invoke";

export function listNativeThemes(refresh = false) {
  return invokeNative<ThemeCatalogSnapshot>("list_themes", { refresh });
}

type NativeThemeActivationPayload = Omit<ThemeActivationPayload, "source"> & {
  source:
    | { kind: "inline"; css: string }
    | { kind: "stylesheet"; path: string };
};

function stylesheetHref(path: string, fingerprint: string) {
  const separatorIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  const directory = separatorIndex === -1 ? path : path.slice(0, separatorIndex);
  const filename = separatorIndex === -1 ? "theme.css" : path.slice(separatorIndex + 1);
  const convertedDirectory = convertFileSrc(directory);
  const suffixIndex = [convertedDirectory.indexOf("?"), convertedDirectory.indexOf("#")]
    .filter((index) => index !== -1)
    .reduce((first, index) => Math.min(first, index), convertedDirectory.length);
  const convertedBase = convertedDirectory.slice(0, suffixIndex);
  const convertedSuffix = convertedDirectory.slice(suffixIndex);
  const converted = `${convertedBase}${convertedBase.endsWith("/") ? "" : "/"}${encodeURIComponent(filename)}${convertedSuffix}`;
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
