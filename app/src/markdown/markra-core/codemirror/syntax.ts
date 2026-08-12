const markdownEscape = /\\([!-/:-@[-`{-~])/gu;

export function unescapeMarkdown(value: string) {
  return value.replace(markdownEscape, "$1");
}

export function unquoteMarkdownTitle(value: string) {
  const first = value.at(0);
  const last = value.at(-1);
  if (
    (first === '"' && last === '"') ||
    (first === "'" && last === "'") ||
    (first === "(" && last === ")")
  ) {
    return unescapeMarkdown(value.slice(1, -1));
  }
  return unescapeMarkdown(value);
}
