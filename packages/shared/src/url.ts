export function normalizedExternalAutolinkUrl(text: string) {
  const trimmed = text.trim();
  if (!trimmed || /\s/u.test(trimmed)) return null;

  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol === "http:" || parsed.protocol === "https:" || parsed.protocol === "mailto:") {
      return parsed.toString();
    }
  } catch {
    return null;
  }

  return null;
}
