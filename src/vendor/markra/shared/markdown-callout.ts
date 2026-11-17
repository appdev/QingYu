const calloutTones = {
  note: "note", abstract: "note", info: "note", todo: "note", tip: "tip", success: "tip",
  question: "warning", warning: "warning", failure: "caution", danger: "caution",
  bug: "caution", example: "important", quote: "quote",
} as const;
const aliases: Record<string, MarkdownCalloutType> = {
  summary: "abstract", tldr: "abstract", hint: "tip", important: "tip", check: "success", done: "success",
  help: "question", faq: "question", caution: "warning", attention: "warning",
  fail: "failure", missing: "failure", error: "danger", cite: "quote",
};

export type MarkdownCalloutType = keyof typeof calloutTones;

export type MarkdownCalloutDefinition = {
  label: string;
  marker: string;
  type: MarkdownCalloutType;
  tone: typeof calloutTones[MarkdownCalloutType];
};

export type ParsedMarkdownCalloutMarker = MarkdownCalloutDefinition & {
  name: string;
  source: string;
};

export const markdownCalloutTypeOrder = ["note", "info", "tip", "success", "question", "warning", "danger", "bug", "example", "quote"] as const;

export const markdownCalloutDefinitions = Object.fromEntries(
  (Object.keys(calloutTones) as MarkdownCalloutType[]).map(type => [type, {
    label: type[0].toUpperCase() + type.slice(1), marker: type.toUpperCase(), tone: calloutTones[type], type,
  }]),
) as Record<MarkdownCalloutType, MarkdownCalloutDefinition>;

const markdownCalloutMarkerPattern = /^[ \t]*(\[!([a-z][\w-]*)\][+-]?)(?=[ \t\r\n]|$)/iu;

export function parseMarkdownCalloutMarker(text: string): ParsedMarkdownCalloutMarker | null {
  const match = markdownCalloutMarkerPattern.exec(text);
  const source = match?.[1];
  if (!source) return null;
  const name = match[2];
  const lower = name.toLowerCase();
  const type = Object.hasOwn(markdownCalloutDefinitions, lower) ? lower as MarkdownCalloutType
    : Object.hasOwn(aliases, lower) ? aliases[lower] : "note";

  return {
    ...markdownCalloutDefinitions[type],
    label: lower === "tldr" ? "TL;DR" : lower === "faq" ? "FAQ" : lower[0].toUpperCase() + lower.slice(1),
    name,
    source
  };
}

export function markdownCalloutMarkerForType(type: MarkdownCalloutType) {
  return `[!${markdownCalloutDefinitions[type].marker}]`;
}

function lineContentWithoutEnding(line: string) {
  return line.endsWith("\r") ? line.slice(0, -1) : line;
}

function blockquoteLineContent(line: string) {
  const match = /^((?:>\s*)+)(.*)$/u.exec(lineContentWithoutEnding(line));
  return match?.[2] ?? null;
}

function restoreCalloutHardBreakEscapes(markdown: string) {
  const lines = markdown.split("\n");
  let inCallout = false;

  return lines.map((line, index) => {
    const content = lineContentWithoutEnding(line);
    const lineEnding = line.slice(content.length);
    const quotedContent = blockquoteLineContent(line);

    if (quotedContent === null) {
      inCallout = false;
      return line;
    }

    if (parseMarkdownCalloutMarker(quotedContent)) {
      inCallout = true;
    }

    const nextLine = lines[index + 1];
    const nextIsBlockquote = nextLine !== undefined && blockquoteLineContent(nextLine) !== null;
    if (inCallout && nextIsBlockquote && content.endsWith("\\")) {
      return `${content.slice(0, -1)}${lineEnding}`;
    }

    return line;
  }).join("\n");
}

function lineIsEmptyBlockquote(line: string | undefined) {
  if (line === undefined) return false;

  const quotedContent = blockquoteLineContent(line);
  return quotedContent !== null && quotedContent.trim().length === 0;
}

function lineIsEmptyCalloutBodyPlaceholder(line: string | undefined) {
  if (lineIsEmptyBlockquote(line)) return true;

  const quotedContent = line === undefined ? null : blockquoteLineContent(line);
  return quotedContent?.trim() === "<br />";
}

function emptyBlockquoteLineFor(line: string) {
  const match = /^((?:>\s*)+)/u.exec(lineContentWithoutEnding(line));
  return match?.[1]?.trimEnd() || ">";
}

function collapseEmptyCalloutBodyPlaceholders(markdown: string) {
  const lines = markdown.split("\n");
  const collapsedLines: string[] = [];
  let inCallout = false;

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const line = lines[lineIndex] ?? "";
    const quotedContent = blockquoteLineContent(line);
    if (quotedContent === null) {
      inCallout = false;
      collapsedLines.push(line);
      continue;
    }

    if (parseMarkdownCalloutMarker(quotedContent)) {
      inCallout = true;
    }

    collapsedLines.push(line);
    if (!inCallout || lineIsEmptyCalloutBodyPlaceholder(line)) continue;

    let placeholderEndLine = lineIndex + 1;
    while (lineIsEmptyCalloutBodyPlaceholder(lines[placeholderEndLine])) {
      placeholderEndLine += 1;
    }

    const placeholderCount = placeholderEndLine - lineIndex - 1;
    const afterPlaceholdersLine = lines[placeholderEndLine];
    const placeholdersAreAtCalloutEnd =
      afterPlaceholdersLine === undefined ||
      afterPlaceholdersLine.length === 0 ||
      blockquoteLineContent(afterPlaceholdersLine) === null;

    if (placeholderCount >= 2 && placeholderCount % 2 === 0 && placeholdersAreAtCalloutEnd) {
      const emptyLine = emptyBlockquoteLineFor(lines[lineIndex + 1] ?? ">");
      for (let count = 0; count < placeholderCount / 2; count += 1) {
        collapsedLines.push(emptyLine);
      }
      lineIndex = placeholderEndLine - 1;
    }
  }

  return collapsedLines.join("\n");
}

function blockquoteListItemStart(line: string | undefined) {
  if (line === undefined) return null;

  const match = /^((?:>\s?)+)( *)([-+*]|\d+[.)])\s+/u.exec(lineContentWithoutEnding(line));
  if (!match) return null;

  return {
    indent: match[2] ?? "",
    kind: /^\d/u.test(match[3] ?? "") ? "ordered" : "bullet",
    quotePrefix: match[1] ?? ""
  };
}

function calloutListItemStartsShouldStayTight(
  previousListItem: NonNullable<ReturnType<typeof blockquoteListItemStart>>,
  nextListItem: NonNullable<ReturnType<typeof blockquoteListItemStart>>
) {
  if (previousListItem.quotePrefix !== nextListItem.quotePrefix) return false;
  if (previousListItem.indent === nextListItem.indent) return previousListItem.kind === nextListItem.kind;

  return (
    previousListItem.indent.startsWith(nextListItem.indent) ||
    nextListItem.indent.startsWith(previousListItem.indent)
  );
}

function tightenCalloutSimpleListSpacing(markdown: string) {
  const lines = markdown.split("\n");
  const tightenedLines: string[] = [];
  let inCallout = false;

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const line = lines[lineIndex] ?? "";
    const quotedContent = blockquoteLineContent(line);

    if (quotedContent === null) {
      inCallout = false;
      tightenedLines.push(line);
      continue;
    }

    if (parseMarkdownCalloutMarker(quotedContent)) {
      inCallout = true;
    }

    const previousListItem = blockquoteListItemStart(tightenedLines[tightenedLines.length - 1]);
    const nextListItem = blockquoteListItemStart(lines[lineIndex + 1]);
    const isSimpleListSpacer =
      inCallout &&
      lineIsEmptyBlockquote(line) &&
      previousListItem !== null &&
      nextListItem !== null &&
      calloutListItemStartsShouldStayTight(previousListItem, nextListItem);

    if (!isSimpleListSpacer) {
      tightenedLines.push(line);
    }
  }

  return tightenedLines.join("\n");
}

function restoreFormattedMarkdownCalloutMarkers(markdown: string) {
  return markdown.replace(
    /(^|\n)((?:>\s*)+)([*_]{1,3})\\?(\[!(?:NOTE|TIP|IMPORTANT|WARNING|CAUTION)\])\3(?=\s*(?:\n|$))/giu,
    "$1$2$4"
  );
}

export function restoreEscapedMarkdownCalloutMarkers(markdown: string) {
  const restoredMarkers = restoreFormattedMarkdownCalloutMarkers(markdown).replace(
    /(^|\n)((?:>\s*)+)\\(\[!(?:NOTE|TIP|IMPORTANT|WARNING|CAUTION)\])/giu,
    "$1$2$3"
  );
  return collapseEmptyCalloutBodyPlaceholders(
    tightenCalloutSimpleListSpacing(restoreCalloutHardBreakEscapes(restoredMarkers))
  ).replace(/(^|\n)((?:>\s*)+)<br \/>(?=\n|$)/gu, "$1$2");
}
