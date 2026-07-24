import { md5 } from "@noble/hashes/legacy.js";
import { bytesToHex } from "@noble/hashes/utils.js";

import type { EditorPreferences } from "./settings/app-settings";
import {
  saveNativeClipboardImage,
  type SavedNativeClipboardImage,
  type SaveNativeClipboardImageInput
} from "./tauri";
import { resolveEditorAssetAction, type EditorAssetContext } from "./editor-assets";
import type { EditorResourceOrigin } from "@markra/editor";

type SaveLocalImage = (input: SaveNativeClipboardImageInput) => Promise<SavedNativeClipboardImage>;
export type SaveEditorImageSkippedReason = "requires-saved-document";

export type SaveEditorImageResult =
  | {
      image: SavedNativeClipboardImage;
      refreshTree: boolean;
      status: "saved";
    }
  | {
      reason: SaveEditorImageSkippedReason;
      status: "skipped";
    };

export type SaveLocalEditorImageInput = {
  context?: EditorAssetContext;
  documentPath: string | null;
  image: File;
  origin?: EditorResourceOrigin;
  preferences: EditorPreferences;
  copyToStorage?: boolean;
  saveLocalImage?: SaveLocalImage;
};

type ImageUploadFileNameOptions = {
  random?: () => string;
  timestamp?: () => string;
};

const imageMimeExtensions: Record<string, string> = {
  "image/avif": "avif",
  "image/bmp": "bmp",
  "image/gif": "gif",
  "image/jpeg": "jpg",
  "image/jpg": "jpg",
  "image/png": "png",
  "image/svg+xml": "svg",
  "image/webp": "webp"
};

export async function createImageUploadFileName(
  image: File,
  pattern: string,
  options: ImageUploadFileNameOptions = {}
) {
  const extension = imageExtensionFromFile(image);
  const name = sanitizeFileNamePart(image.name.replace(/\.[^.]*$/u, "")) || "image";
  const timestamp = options.timestamp?.() ?? String(Date.now());
  const random = options.random?.() ?? defaultRandomToken();
  const md5 = pattern.includes("{md5}") ? await fileMd5Hex(image) : "";
  const renderedPattern = (pattern.trim() || "pasted-image-{timestamp}")
    .replace(/\{name\}/gu, name)
    .replace(/\{timestamp\}/gu, timestamp)
    .replace(/\{random\}/gu, random)
    .replace(/\{md5\}/gu, md5);
  const baseName = sanitizeFileNamePart(stripKnownImageExtension(renderedPattern)) || `pasted-image-${timestamp}`;

  return `${baseName}.${extension}`;
}

export async function saveLocalEditorImage({
  context,
  copyToStorage = true,
  documentPath,
  image,
  origin,
  preferences,
  saveLocalImage = saveNativeClipboardImage
}: SaveLocalEditorImageInput): Promise<SaveEditorImageResult> {
  const action = context && origin
    ? resolveEditorAssetAction({ mode: context.mode, origin })
    : copyToStorage
      ? "copy-document"
      : "reference";
  const shouldCopy = action !== "reference";
  const projectRootPath = context?.mode === "primary-workspace"
    ? context.primaryRootPath
    : null;

  if (!shouldCopy) {
    return {
      image: await saveLocalImage({
        copyToStorage: false,
        documentPath,
        fileName: await createImageUploadFileName(image, preferences.imageUpload.fileNamePattern),
        folder: "assets",
        image
      }),
      refreshTree: false,
      status: "saved"
    };
  }

  if (!documentPath) {
    return {
      reason: "requires-saved-document",
      status: "skipped"
    };
  }

  return {
    image: await saveLocalImage({
      documentPath,
      fileName: await createImageUploadFileName(image, preferences.imageUpload.fileNamePattern),
      folder: context ? "assets" : preferences.clipboardImageFolder,
      image,
      ...(projectRootPath ? { projectRootPath } : {})
    }),
    refreshTree: true,
    status: "saved"
  };
}

function imageExtensionFromFile(image: File) {
  const mimeExtension = imageMimeExtensions[image.type.toLowerCase()];
  if (mimeExtension) return mimeExtension;

  const nameExtension = image.name
    .split(".")
    .pop()
    ?.trim()
    .toLowerCase();
  if (nameExtension && ["avif", "bmp", "gif", "jpg", "jpeg", "png", "svg", "webp"].includes(nameExtension)) {
    return nameExtension === "jpeg" ? "jpg" : nameExtension;
  }

  return "png";
}

function sanitizeFileNamePart(value: string) {
  return value
    .trim()
    .replace(/[^\p{L}\p{N}._-]+/gu, "-")
    .replace(/-+/gu, "-")
    .replace(/^[.-]+|[.-]+$/gu, "");
}

function stripKnownImageExtension(value: string) {
  return value.replace(/\.(?:avif|bmp|gif|jpe?g|png|webp)$/iu, "");
}

async function fileMd5Hex(file: File) {
  return bytesToHex(md5(new Uint8Array(await file.arrayBuffer())));
}

function defaultRandomToken() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID().replace(/-/gu, "").slice(0, 8);
  }

  return Math.random().toString(36).slice(2, 10);
}
