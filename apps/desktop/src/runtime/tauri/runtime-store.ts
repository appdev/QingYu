import type {
  RuntimeStore,
  RuntimeStoreLoadOptions
} from "@markra/app/runtime";
import { invokeNative } from "./invoke";

type DesktopRuntimeStorePath = "local-state.json" | "settings.json";
type PendingStoreChange =
  | { key: string; operation: "delete" }
  | { key: string; operation: "set"; value: unknown };
type NativeStoreValue = { exists: boolean; value: unknown };

const readableStoreKeys: Record<DesktopRuntimeStorePath, ReadonlySet<string>> = {
  "local-state.json": new Set([
    "fileTreeSortByWorkspace",
    "pandocPath",
    "recentMarkdownFiles",
    "schemaVersion",
    "welcomeDocumentSeen",
    "workspace"
  ]),
  "settings.json": new Set([
    "appearanceMode",
    "customThemeCss",
    "darkCustomThemeCss",
    "darkTheme",
    "darkThemeId",
    "editorPreferences",
    "exportSettings",
    "fileIgnoreSettings",
    "language",
    "lightCustomThemeCss",
    "lightTheme",
    "lightThemeId",
    "theme"
  ])
};
const mutableStoreKeys: Record<DesktopRuntimeStorePath, ReadonlySet<string>> = {
  "local-state.json": readableStoreKeys["local-state.json"],
  "settings.json": new Set()
};

function supportedStorePath(path: string): DesktopRuntimeStorePath {
  if (path === "local-state.json" || path === "settings.json") return path;
  throw new Error("Unsupported desktop runtime store");
}

function assertSupportedLoadOptions(options: RuntimeStoreLoadOptions) {
  if (options.autoSave || Object.keys(options.defaults).length > 0) {
    throw new Error("Unsupported desktop runtime store options");
  }
}

function assertReadableKey(path: DesktopRuntimeStorePath, key: string) {
  if (!readableStoreKeys[path].has(key)) {
    throw new Error("Unsupported desktop runtime store key");
  }
}

function assertMutableKey(path: DesktopRuntimeStorePath, key: string) {
  if (!mutableStoreKeys[path].has(key)) {
    throw new Error("Unsupported desktop runtime store mutation");
  }
}

export async function loadDesktopRuntimeStore(
  requestedPath: string,
  options: RuntimeStoreLoadOptions
): Promise<RuntimeStore> {
  const path = supportedStorePath(requestedPath);
  assertSupportedLoadOptions(options);
  await invokeNative("load_desktop_runtime_store", { options, path });
  const pending = new Map<string, PendingStoreChange>();

  return {
    delete: async (key) => {
      assertMutableKey(path, key);
      pending.set(key, { key, operation: "delete" });
    },
    get: async <T>(key: string) => {
      assertReadableKey(path, key);
      const pendingChange = pending.get(key);
      if (pendingChange?.operation === "delete") return undefined;
      if (pendingChange?.operation === "set") return pendingChange.value as T;
      const result = await invokeNative<NativeStoreValue>("get_desktop_runtime_store_value", {
        key,
        path
      });
      return result.exists ? result.value as T : undefined;
    },
    save: async () => {
      const changes = [...pending.values()];
      if (changes.length === 0) return;
      await invokeNative("commit_desktop_runtime_store_changes", { changes, path });
      for (const change of changes) {
        if (pending.get(change.key) === change) pending.delete(change.key);
      }
    },
    set: async (key, value) => {
      assertMutableKey(path, key);
      pending.set(key, { key, operation: "set", value });
    }
  };
}
