import { openUrl } from "@tauri-apps/plugin-opener";

const supportedExternalProtocols = new Set(["http:", "https:"]);
const genericOpenerFailure = "The system could not open the link.";

function parsedExternalUrl(value: string) {
  const trimmed = value.trim();
  let parsed: URL;

  try {
    parsed = new URL(trimmed);
  } catch {
    throw new Error("Invalid external URL.");
  }

  if (!supportedExternalProtocols.has(parsed.protocol)) {
    throw new Error("Unsupported external URL scheme.");
  }
  if (!parsed.hostname) {
    throw new Error("Invalid external URL.");
  }
  if (parsed.username || parsed.password) {
    throw new Error("External URLs with embedded credentials are not supported.");
  }

  return parsed.toString();
}

export async function openNativeExternalUrl(url: string) {
  const parsedUrl = parsedExternalUrl(url);

  if (!("__TAURI_INTERNALS__" in window)) {
    try {
      window.open(parsedUrl, "_blank", "noopener,noreferrer");
    } catch {
      throw new Error(genericOpenerFailure);
    }
    return;
  }

  try {
    await openUrl(parsedUrl);
  } catch {
    throw new Error(genericOpenerFailure);
  }
}
