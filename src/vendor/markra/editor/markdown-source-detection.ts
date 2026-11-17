const markdownSourcePatterns = [
  /(^|\n)\s{0,3}#{1,6}\s+\S/u,
  /(^|\n)\s{0,3}(?:[-+*]|\d+[.)])\s+\S/u,
  /(^|\n)\s{0,3}>\s+\S/u,
  /(^|\n)\s{0,3}(?:```|~~~)/u,
  /(^|\n)\s{0,3}(?:-{3,}|\*{3,}|_{3,})\s*($|\n)/u,
  /(^|\n)\s{0,3}\|.+\|\s*\n\s{0,3}\|(?:\s*:?-+:?\s*\|)+/u,
  /!\[[^\]\n]*\]\([^)]+\)/u,
  /(^|[\s([{])\[[^\]\n]+\]\([^)]+\)/u,
  /(^|[\s([{])\*\*[^*\n]+?\*\*/u,
  /(^|[\s([{])__[^_\n]+?__/u,
  /(^|[\s([{])`[^`\n]+?`(?!`)/u
];

function isStandaloneUrl(text: string) {
  return /^https?:\/\/\S+$/u.test(text.trim());
}

export function looksLikeMarkdownSource(text: string) {
  const normalizedText = text.replace(/\r\n?/g, "\n");
  const trimmedText = normalizedText.trim();
  if (!trimmedText || isStandaloneUrl(trimmedText)) return false;

  return markdownSourcePatterns.some((pattern) => pattern.test(normalizedText));
}
