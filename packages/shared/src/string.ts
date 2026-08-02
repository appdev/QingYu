export function normalizeNullableString(value: unknown) {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

const markdownDocumentExtension = /\.(?:markdown|md)$/iu;
const unsafeMarkdownDocumentTitleCharacters: Record<string, string> = {
  "/": "／",
  "\\": "＼",
  ":": "：",
  "*": "＊",
  "?": "？",
  '"': "＂",
  "<": "＜",
  ">": "＞",
  "|": "｜"
};
const windowsDeviceName = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/iu;

export function markdownDocumentTitleFromFileName(fileName: string) {
  return fileName.replace(markdownDocumentExtension, "");
}

export function normalizeMarkdownDocumentTitle(input: string):
  | { ok: true; title: string; fileName: string }
  | { ok: false; reason: "empty" | "too-long" } {
  let title = input
    .replace(/[\r\n\t]+/gu, " ")
    .trim()
    .replace(/[/\\:*?"<>|]/gu, (character) => unsafeMarkdownDocumentTitleCharacters[character])
    .replace(/[ .]+$/gu, "");

  if (title.length === 0) {
    return { ok: false, reason: "empty" };
  }

  if (windowsDeviceName.test(title)) {
    title = `_${title}`;
  }

  const fileName = `${title}.md`;
  if (new TextEncoder().encode(fileName).byteLength > 255) {
    return { ok: false, reason: "too-long" };
  }

  return { ok: true, title, fileName };
}

export function numberedMarkdownDocumentName(suggestedName: string, offset: number) {
  if (!Number.isSafeInteger(offset) || offset < 0) {
    throw new RangeError("Markdown document name offset must be a non-negative safe integer.");
  }
  if (offset === 0) return suggestedName;

  const extension = suggestedName.match(/\.(?:markdown|md)$/i)?.[0] ?? ".md";
  const stem = suggestedName.endsWith(extension)
    ? suggestedName.slice(0, -extension.length)
    : suggestedName;
  const numberedStem = stem.match(/^(.*) (\d+)$/u);
  const parsedIndex = numberedStem ? Number(numberedStem[2]) : 0;
  const safeNumberedStem = numberedStem && Number.isSafeInteger(parsedIndex) ? numberedStem : null;
  const initialIndex = safeNumberedStem ? parsedIndex : 0;
  const baseStem = safeNumberedStem ? safeNumberedStem[1] : stem;
  const nextIndex = initialIndex + offset;
  if (!Number.isSafeInteger(nextIndex)) {
    throw new RangeError("Markdown document name index exceeds the safe integer range.");
  }

  return `${baseStem} ${nextIndex}${extension}`;
}
