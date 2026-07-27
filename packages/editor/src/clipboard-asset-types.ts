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
