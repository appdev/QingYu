export type SavedClipboardImage = {
  alt: string;
  src: string;
};

export type SaveClipboardImage = (
  image: File,
) => Promise<SavedClipboardImage | null>;

export type SavedClipboardAttachment = {
  label: string;
  src: string;
};

export type SaveClipboardAttachment = (
  attachment: File,
) => Promise<SavedClipboardAttachment | null>;

export type RemoteClipboardImage = {
  alt: string;
  src: string;
  title: string;
};

export type SaveRemoteClipboardImage = (
  image: RemoteClipboardImage,
) => Promise<SavedClipboardImage | null>;

export type EditorResourceOrigin = "clipboard" | "drop" | "import" | "remote";

export type EditorResourceRequest =
  | {
      files: File[];
      origin: Exclude<EditorResourceOrigin, "remote">;
    }
  | {
      origin: "remote";
      urls: string[];
    };

export type SavedEditorResource =
  | ({ kind: "image" } & SavedClipboardImage)
  | ({ kind: "attachment" } & SavedClipboardAttachment);

export type SaveEditorResources = (
  request: EditorResourceRequest,
) => Promise<SavedEditorResource[]>;

const canonicalImageMediaTypesByAlias = new Map<string, string>([
  ["image/avif", "image/avif"],
  ["image/x-avif", "image/avif"],
  ["image/bmp", "image/bmp"],
  ["image/x-bmp", "image/bmp"],
  ["image/x-ms-bmp", "image/bmp"],
  ["image/gif", "image/gif"],
  ["image/x-gif", "image/gif"],
  ["image/jpeg", "image/jpeg"],
  ["image/jpg", "image/jpeg"],
  ["image/pjpeg", "image/jpeg"],
  ["image/png", "image/png"],
  ["image/x-png", "image/png"],
  ["image/svg+xml", "image/svg+xml"],
  ["image/svg", "image/svg+xml"],
  ["application/svg+xml", "image/svg+xml"],
  ["text/svg", "image/svg+xml"],
  ["image/webp", "image/webp"],
  ["image/x-webp", "image/webp"],
]);

const canonicalImageMediaTypesByExtension = new Map<string, string>([
  ["avif", "image/avif"],
  ["bmp", "image/bmp"],
  ["gif", "image/gif"],
  ["jpeg", "image/jpeg"],
  ["jpg", "image/jpeg"],
  ["png", "image/png"],
  ["svg", "image/svg+xml"],
  ["webp", "image/webp"],
]);

function fileNameImageMediaType(name: string) {
  const extension = name.match(/\.([^.]+)$/u)?.[1]?.toLocaleLowerCase("en-US") ?? "";
  return canonicalImageMediaTypesByExtension.get(extension) ?? null;
}

/**
 * Normalizes the MIME aliases emitted by native file providers without
 * weakening byte validation, which remains owned by the Kernel.
 */
export function normalizeEditorImageFile(file: File): File | null {
  const declared = file.type.split(";", 1)[0]?.trim().toLocaleLowerCase("en-US") ?? "";
  const declaredMediaType = canonicalImageMediaTypesByAlias.get(declared) ?? null;
  const namedMediaType = fileNameImageMediaType(file.name);
  const undeclared = declared === "" || declared === "application/octet-stream";
  const mediaType = declaredMediaType ?? (undeclared ? namedMediaType : null);
  if (mediaType === null || (declaredMediaType !== null && namedMediaType !== null && declaredMediaType !== namedMediaType)) {
    return null;
  }
  if (file.type === mediaType) return file;

  const normalized = new File([file], file.name, {
    lastModified: file.lastModified,
    type: mediaType,
  });
  const nativePath = (file as File & { path?: unknown }).path;
  if (typeof nativePath === "string" && nativePath !== "") {
    Object.defineProperty(normalized, "path", {
      configurable: false,
      enumerable: true,
      value: nativePath,
    });
  }
  return normalized;
}

export function createEditorResourceRequest(
  origin: Exclude<EditorResourceOrigin, "remote">,
  files: File[],
): EditorResourceRequest;
export function createEditorResourceRequest(
  origin: "remote",
  urls: string[],
): EditorResourceRequest;
export function createEditorResourceRequest(
  origin: EditorResourceOrigin,
  resources: File[] | string[],
): EditorResourceRequest {
  if (origin === "remote") {
    return { origin, urls: resources as string[] };
  }

  return { files: resources as File[], origin };
}
