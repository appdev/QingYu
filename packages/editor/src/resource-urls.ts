const supportedEditorResourceProtocols = new Set([
  "blob:",
  "file:",
  "http:",
  "https:"
]);

const editorResourceBaseUrl = "https://editor-resource.invalid/";

export function isSafeEditorResourceUrl(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return false;

  try {
    const markdownDecoded = decodeString(trimmed);
    const url = new URL(decodeURIComponent(markdownDecoded), editorResourceBaseUrl);
    if (url.origin === "https://editor-resource.invalid") return true;

    return supportedEditorResourceProtocols.has(url.protocol);
  } catch {
    return false;
  }
}
import { decodeString } from "micromark-util-decode-string";
