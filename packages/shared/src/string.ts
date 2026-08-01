export function normalizeNullableString(value: unknown) {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
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
