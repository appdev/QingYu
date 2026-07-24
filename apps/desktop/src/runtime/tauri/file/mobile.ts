import { open, type OpenDialogOptions } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import type {
  NativeMarkdownPickerLabels,
  SavedNativeClipboardImage,
  SaveNativeClipboardImageInput
} from "@markra/app/runtime";

import { mobileImageFileFromBytes } from "../mobile-image-file";
import { invokeNative } from "../invoke";

const mobileImageFilters = [{
  extensions: [
    "image/avif",
    "image/bmp",
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/svg+xml",
    "image/webp"
  ],
  name: "Images"
}];

type MobileDialogOpen = (options: OpenDialogOptions) => Promise<string | string[] | null>;
type MobileReadFile = (uri: string) => Promise<Uint8Array>;

function selectedUris(selection: string | string[] | null) {
  if (!selection) return [];
  return Array.isArray(selection) ? selection : [selection];
}

export function createMobileLocalImagePicker({
  open: openDialog,
  readFile: readSelectedFile
}: {
  open: MobileDialogOpen;
  readFile: MobileReadFile;
}) {
  return async (labels?: NativeMarkdownPickerLabels) => {
    const title = labels?.title.trim();
    const selection = await openDialog({
      fileAccessMode: "scoped",
      filters: mobileImageFilters,
      multiple: true,
      pickerMode: "image",
      ...(title ? { title } : {})
    });
    const images: File[] = [];
    for (const uri of selectedUris(selection)) {
      let bytes: Uint8Array;
      try {
        bytes = await readSelectedFile(uri);
      } catch {
        throw new Error("Could not read selected image. Check photo access and try again.");
      }
      images.push(mobileImageFileFromBytes({ bytes, uri }));
    }
    return images;
  };
}

export const openMobileLocalImages = createMobileLocalImagePicker({ open, readFile });

function imageAltFromFileName(fileName: string) {
  const trimmedName = fileName.trim();
  if (!trimmedName) return "image";
  return trimmedName.replace(/\.[^.]*$/u, "").trim() || "image";
}

function encodeMarkdownUrlSegment(segment: string) {
  return encodeURIComponent(segment).replace(/[!'()*]/gu, (character) =>
    `%${character.charCodeAt(0).toString(16).toUpperCase()}`
  );
}

function encodeMarkdownRelativePath(path: string) {
  return path.split("/").map(encodeMarkdownUrlSegment).join("/");
}

export async function saveMobileClipboardImage({
  copyToStorage = true,
  documentPath,
  fileName,
  image,
  projectRootPath
}: SaveNativeClipboardImageInput): Promise<SavedNativeClipboardImage> {
  if (!copyToStorage || !projectRootPath) {
    throw new Error("Mobile image import requires the primary workspace.");
  }
  if (!documentPath) throw new Error("Current document must be a saved Markdown file.");

  const bytes = Array.from(new Uint8Array(await image.arrayBuffer()));
  const savedImage = await invokeNative<{ relativePath: string }>("save_clipboard_image", {
    bytes,
    documentPath,
    fileName,
    folder: "assets",
    mimeType: image.type,
    projectRootPath
  });
  return {
    alt: imageAltFromFileName(image.name),
    src: encodeMarkdownRelativePath(savedImage.relativePath)
  };
}
