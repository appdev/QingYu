import type {
  NativeSettingsFile,
  SavedNativeSettingsFile,
  SaveNativeSettingsFileInput
} from "@markra/app/runtime";
import { createBrowserDownload, resolveBrowserPicker } from "./browser";
import type { WebFileHandle, WebRuntimeOptions } from "./types";

export type BrowserSettingsFileShellOptions = Pick<WebRuntimeOptions,
  | "document"
  | "downloadFile"
  | "showOpenFilePicker"
  | "showSaveFilePicker"
>;

const jsonFileType = "application/json;charset=utf-8";
const settingsFilePickerTypes = [{
  accept: {
    "application/json": [".json"]
  },
  description: "QingYu settings"
}];

export function createBrowserSettingsFileShell(options: BrowserSettingsFileShellOptions = {}) {
  const downloadFile = options.downloadFile ?? createBrowserDownload(options.document);
  const showOpenFilePicker = options.showOpenFilePicker ?? resolveBrowserPicker("showOpenFilePicker");
  const showSaveFilePicker = options.showSaveFilePicker ?? resolveBrowserPicker("showSaveFilePicker");

  return {
    openSettingsFile: async (labels?: { title?: string }): Promise<NativeSettingsFile | null> => {
      if (canUseFileSystemAccessPicker(showOpenFilePicker)) {
        let handle: WebFileHandle | undefined;
        try {
          [handle] = await showOpenFilePicker({
            multiple: false,
            types: settingsFilePickerTypes
          });
        } catch (error: unknown) {
          if (isBrowserPickerCancel(error)) return null;

          throw error;
        }
        if (!handle) return null;

        return settingsFileFromHandle(handle);
      }

      return pickSettingsFileWithInput(options.document, labels);
    },
    saveSettingsFile: async (input: SaveNativeSettingsFileInput): Promise<SavedNativeSettingsFile | null> => {
      if (canUseFileSystemAccessPicker(showSaveFilePicker)) {
        let handle: WebFileHandle;
        try {
          handle = await showSaveFilePicker({
            suggestedName: input.suggestedName,
            types: settingsFilePickerTypes
          });
        } catch (error: unknown) {
          if (isBrowserPickerCancel(error)) return null;

          throw error;
        }
        await writeFileHandle(handle, input.contents);

        return {
          name: handle.name,
          path: createWebFilePath(handle.name)
        };
      }

      await downloadFile({
        contents: input.contents,
        name: input.suggestedName,
        type: jsonFileType
      });

      return {
        name: input.suggestedName,
        path: `web-download://${encodeURIComponent(input.suggestedName)}`
      };
    }
  };
}

function canUseFileSystemAccessPicker<TPicker extends ((options?: unknown) => Promise<unknown>) | undefined>(
  picker: TPicker
): picker is Exclude<TPicker, undefined> {
  return picker !== undefined && globalThis.isSecureContext !== false;
}

function isBrowserPickerCancel(error: unknown) {
  return error instanceof DOMException && error.name === "AbortError";
}

async function settingsFileFromHandle(handle: WebFileHandle): Promise<NativeSettingsFile> {
  const file = await handle.getFile();

  return {
    content: await file.text(),
    name: file.name || handle.name,
    path: createWebFilePath(handle.name)
  };
}

function pickSettingsFileWithInput(
  documentOverride: Document | undefined,
  labels?: { title?: string }
): Promise<NativeSettingsFile | null> {
  const documentTarget = documentOverride ?? globalThis.document;
  if (!documentTarget?.body) {
    return Promise.reject(new Error("Browser settings import requires a document or the File System Access API."));
  }

  return new Promise((resolve, reject) => {
    const input = documentTarget.createElement("input");
    let settled = false;
    input.type = "file";
    input.accept = "application/json,.json";
    input.multiple = false;
    input.style.display = "none";
    if (labels?.title) input.title = labels.title;

    const settle = (result: NativeSettingsFile | null) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(result);
    };
    const fail = (error: unknown) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };
    const cleanup = () => {
      input.removeEventListener("change", onChange);
      input.removeEventListener("cancel", onCancel);
      input.remove();
    };
    const onCancel = () => settle(null);
    const onChange = () => {
      const file = input.files?.[0];
      if (!file) {
        settle(null);
        return;
      }

      file.text()
        .then((content) => settle({
          content,
          name: file.name,
          path: `web-upload://${encodeURIComponent(file.name)}`
        }))
        .catch(fail);
    };

    input.addEventListener("change", onChange);
    input.addEventListener("cancel", onCancel);
    documentTarget.body.appendChild(input);
    input.click();
  });
}

async function writeFileHandle(handle: WebFileHandle, contents: BlobPart) {
  if (!handle.createWritable) {
    throw new Error("Browser settings export requires a writable file handle.");
  }

  const writable = await handle.createWritable();
  await writable.write(contents);
  await writable.close();
}

function createWebFilePath(name: string) {
  return `web-file://${createHandleId()}/${encodeURIComponent(name)}`;
}

function createHandleId() {
  if (typeof globalThis.crypto?.randomUUID === "function") return globalThis.crypto.randomUUID();

  return `handle-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
