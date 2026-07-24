type DetectedMobileImage = {
  extension: "avif" | "bmp" | "gif" | "jpg" | "png" | "svg" | "webp";
  mimeType: "image/avif" | "image/bmp" | "image/gif" | "image/jpeg" | "image/png" | "image/svg+xml" | "image/webp";
};

const unsupportedMobileImageMessage = "The selected file is not a supported image.";

function startsWithBytes(bytes: Uint8Array, signature: readonly number[]) {
  return signature.every((byte, index) => bytes[index] === byte);
}

function asciiAt(bytes: Uint8Array, offset: number, value: string) {
  if (offset + value.length > bytes.length) return false;
  return [...value].every((character, index) => bytes[offset + index] === character.charCodeAt(0));
}

function isAvif(bytes: Uint8Array) {
  if (bytes.length < 16 || !asciiAt(bytes, 4, "ftyp")) return false;
  const declaredSize = ((bytes[0] ?? 0) << 24)
    | ((bytes[1] ?? 0) << 16)
    | ((bytes[2] ?? 0) << 8)
    | (bytes[3] ?? 0);
  const boxEnd = declaredSize >= 16 && declaredSize <= bytes.length ? declaredSize : bytes.length;
  for (let offset = 8; offset + 4 <= boxEnd; offset += 4) {
    if (offset === 12) continue;
    if (asciiAt(bytes, offset, "avif") || asciiAt(bytes, offset, "avis")) return true;
  }
  return false;
}

function isUtf8Svg(bytes: Uint8Array) {
  let source: string;
  try {
    source = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return false;
  }

  const withoutPrefix = source
    .replace(/^\uFEFF/u, "")
    .trimStart()
    .replace(/^(?:<\?xml[\s\S]*?\?>\s*)?(?:<!--[\s\S]*?-->\s*)*/iu, "");
  return /^<svg(?:\s|>)/iu.test(withoutPrefix);
}

function detectMobileImage(bytes: Uint8Array): DetectedMobileImage | null {
  if (startsWithBytes(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return { extension: "png", mimeType: "image/png" };
  }
  if (startsWithBytes(bytes, [0xff, 0xd8, 0xff])) {
    return { extension: "jpg", mimeType: "image/jpeg" };
  }
  if (asciiAt(bytes, 0, "GIF87a") || asciiAt(bytes, 0, "GIF89a")) {
    return { extension: "gif", mimeType: "image/gif" };
  }
  if (bytes.length >= 12 && asciiAt(bytes, 0, "RIFF") && asciiAt(bytes, 8, "WEBP")) {
    return { extension: "webp", mimeType: "image/webp" };
  }
  if (startsWithBytes(bytes, [0x42, 0x4d])) {
    return { extension: "bmp", mimeType: "image/bmp" };
  }
  if (isAvif(bytes)) {
    return { extension: "avif", mimeType: "image/avif" };
  }
  if (isUtf8Svg(bytes)) {
    return { extension: "svg", mimeType: "image/svg+xml" };
  }
  return null;
}

function decodeUriPart(value: string) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function safePickedFileStem(uri: string) {
  let parsed: URL;
  try {
    parsed = new URL(uri);
  } catch {
    return null;
  }

  const queryName = ["displayName", "filename", "name"]
    .map((key) => parsed.searchParams.get(key))
    .find((value): value is string => Boolean(value?.trim()));
  const pathName = parsed.pathname.split("/").filter(Boolean).at(-1) ?? "";
  const candidate = decodeUriPart(queryName ?? pathName).trim();
  if (!candidate || candidate.includes("/") || candidate.includes("\\") || /[\u0000-\u001f\u007f]/u.test(candidate)) {
    return null;
  }
  const extensionIndex = candidate.lastIndexOf(".");
  if (extensionIndex <= 0 || extensionIndex === candidate.length - 1) return null;
  const stem = candidate.slice(0, extensionIndex)
    .replace(/[<>:"|?*]+/gu, "-")
    .replace(/\s+/gu, " ")
    .replace(/-+/gu, "-")
    .trim()
    .replace(/^[.-]+|[.-]+$/gu, "");
  if (!stem || stem === "." || stem === "..") return null;
  return stem.slice(0, 160);
}

export function mobileImageFileFromBytes({
  bytes,
  uri
}: {
  bytes: Uint8Array;
  uri: string;
}) {
  const detected = detectMobileImage(bytes);
  if (!detected) throw new Error(unsupportedMobileImageMessage);
  const stem = safePickedFileStem(uri) ?? "picked-image";
  return new File([new Uint8Array(bytes)], `${stem}.${detected.extension}`, { type: detected.mimeType });
}
