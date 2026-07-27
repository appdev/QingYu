const markdownDocumentHrefPattern = /\.(md|markdown)(?:[?#].*)?$/iu;
const uriSchemePattern = /^[a-z][a-z\d+.-]*:/iu;
const windowsPathPattern = /^[a-z]:[\\/]/iu;

export function isLocalAttachmentHref(href: string) {
  const trimmed = href.trim();
  if (!trimmed || trimmed.startsWith("#")) return false;

  const normalized = trimmed.toLowerCase();
  if (normalized.startsWith("file:")) {
    return !markdownDocumentHrefPattern.test(trimmed);
  }

  if (windowsPathPattern.test(trimmed)) {
    return !markdownDocumentHrefPattern.test(trimmed);
  }

  if (
    normalized.startsWith("data:") ||
    normalized.startsWith("mailto:") ||
    uriSchemePattern.test(normalized)
  ) {
    return false;
  }

  return !markdownDocumentHrefPattern.test(trimmed);
}
