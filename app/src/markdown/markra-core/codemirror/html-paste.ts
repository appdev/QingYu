import TurndownService = require("turndown");
import type { RemoteClipboardImage } from "../clipboard-asset-types";

export interface CodeMirrorHtmlPaste {
  readonly markdown: string;
  readonly remoteImages: readonly RemoteClipboardImage[];
  readonly source: "explicit-code" | "host" | "turndown" | "plain-text";
  readonly structured: boolean;
}

export type ConvertClipboardHtmlToMarkdown = (
  html: string,
) => string | null | undefined;

const codeFontPattern = /(?:monospace|menlo|monaco|consolas|courier|sfmono|fira code|jetbrains mono|cascadia code|source code pro)/iu;
const preformattedWhitespacePattern = /white-space\s*:\s*(?:pre|pre-wrap|break-spaces)/iu;
const richTextSelector = [
  "a[href]",
  "b",
  "blockquote",
  "del",
  "div",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "i",
  "img",
  "ol",
  "p",
  "pre",
  "s",
  "strike",
  "strong",
  "sub",
  "sup",
  "table",
  "ul",
].join(",");
const anchorMarkupPattern = /<a\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?<\/a\s*>/giu;

function preformattedStyle(element: Element) {
  const style = element.getAttribute("style") ?? "";
  return codeFontPattern.test(style) || preformattedWhitespacePattern.test(style);
}

function styledInlineLinkMarkup(link: HTMLAnchorElement) {
  const blocks = Array.from(link.querySelectorAll<HTMLElement>("div, p"));
  // Only flatten compact preformatted wrappers; semantic or multiline content
  // must stay on the normal code/link conversion path.
  if (blocks.length === 0 ||
    link.querySelector("br, code, pre") !== null ||
    /[\r\n]/u.test(link.textContent ?? "") ||
    ![link, ...Array.from(link.querySelectorAll<HTMLElement>("[style]"))]
      .some((element) => preformattedStyle(element))) {
    return null;
  }

  const blockSet = new Set(blocks);
  const blocksWithFollowingBlock = new Set(blocks.filter(
    (block) => Boolean(
      block.nextElementSibling && blockSet.has(block.nextElementSibling as HTMLElement),
    ),
  ));
  for (const block of blocks.reverse()) {
    const span = link.ownerDocument.createElement("span");
    for (const attribute of Array.from(block.attributes)) {
      span.setAttribute(attribute.name, attribute.value);
    }
    span.append(...Array.from(block.childNodes));
    block.replaceWith(span);
    if (blocksWithFollowingBlock.has(block)) span.after(" ");
  }

  return link.outerHTML;
}

function normalizeAnchorBlockMarkup(html: string, parser: DOMParser) {
  // A block wrapper inside a paragraph link makes the HTML parser split one
  // authored anchor into empty and duplicate links before Turndown sees it.
  return html.replace(anchorMarkupPattern, (anchor) => {
    if (!codeFontPattern.test(anchor) && !preformattedWhitespacePattern.test(anchor)) {
      return anchor;
    }
    const fragment = parser.parseFromString(anchor, "text/html");
    const link = fragment.body.querySelector<HTMLAnchorElement>("a[href]");
    return link ? styledInlineLinkMarkup(link) ?? anchor : anchor;
  });
}

function hasStructuredHtml(document: Document) {
  return document.querySelector(richTextSelector) !== null;
}

function normalizedCellMarkdown(service: TurndownService, cell: Element) {
  return service
    .turndown(cell.innerHTML)
    .replace(/\r\n?/gu, "\n")
    .replace(/\s*\n\s*/gu, " ")
    .replace(/\|/gu, "\\|")
    .trim();
}

function tableMarkdown(service: TurndownService, table: Element) {
  const rows = Array.from(table.querySelectorAll("tr")).map((row) =>
    Array.from(row.children)
      .filter((cell) => cell.tagName === "TH" || cell.tagName === "TD")
      .map((cell) => normalizedCellMarkdown(service, cell)),
  ).filter((row) => row.length > 0);
  if (rows.length === 0) return "";

  const columnCount = Math.max(...rows.map((row) => row.length));
  const normalizedRows = rows.map((row) =>
    Array.from({ length: columnCount }, (_, index) => row[index] ?? ""),
  );
  const serializeRow = (row: readonly string[]) => `| ${row.join(" | ")} |`;

  return [
    serializeRow(normalizedRows[0] ?? []),
    serializeRow(Array.from({ length: columnCount }, () => "---")),
    ...normalizedRows.slice(1).map(serializeRow),
  ].join("\n");
}

type TurndownServiceModule = typeof TurndownService | {
  readonly default: typeof TurndownService;
};

export function createClipboardTurndownService(
  turndownModule: TurndownServiceModule = TurndownService as unknown as TurndownServiceModule,
) {
  const TurndownConstructor = typeof turndownModule === "function"
    ? turndownModule
    : turndownModule.default;
  const service = new TurndownConstructor({
    bulletListMarker: "-",
    codeBlockStyle: "fenced",
    emDelimiter: "*",
    headingStyle: "atx",
    strongDelimiter: "**",
  });
  service.addRule("markra-gfm-table", {
    filter: "table",
    replacement(_content, node) {
      const markdown = tableMarkdown(service, node as Element);
      return markdown ? `\n\n${markdown}\n\n` : "";
    },
  });
  service.addRule("markra-strikethrough", {
    filter: (node) => ["DEL", "S", "STRIKE"].includes(node.nodeName),
    replacement(content) {
      return content ? `~~${content}~~` : "";
    },
  });
  return service;
}

function hostMarkdown(
  html: string,
  convertHtmlToMarkdown: ConvertClipboardHtmlToMarkdown | undefined,
) {
  if (!convertHtmlToMarkdown) return "";
  try {
    return convertHtmlToMarkdown(html)?.trim() ?? "";
  } catch {
    return "";
  }
}

function normalizeMarkdown(markdown: string) {
  return markdown
    .replace(/\r\n?/gu, "\n")
    .replace(/\n{3,}/gu, "\n\n")
    .trim();
}

function remoteImage(image: Element): RemoteClipboardImage | null {
  const src = image.getAttribute("src") ?? "";
  try {
    const url = new URL(src);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  } catch {
    return null;
  }
  return {
    alt: image.getAttribute("alt") ?? "",
    src,
    title: image.getAttribute("title") ?? "",
  };
}

export function convertCodeMirrorClipboardHtml(
  html: string,
  plainText = "",
  convertHtmlToMarkdown?: ConvertClipboardHtmlToMarkdown,
): CodeMirrorHtmlPaste | null {
  if (!html.trim() || typeof DOMParser === "undefined") return null;
  const parser = new DOMParser();
  const document = parser.parseFromString(
    normalizeAnchorBlockMarkup(html, parser),
    "text/html",
  );
  const service = createClipboardTurndownService();
  const structured = hasStructuredHtml(document);
  const convertedByHost = hostMarkdown(document.body.innerHTML, convertHtmlToMarkdown);
  const convertedByTurndown = convertHtmlToMarkdown
    ? ""
    : service.turndown(document.body.innerHTML);
  const convertedPlainText = plainText || document.body.textContent || "";
  const markdown = normalizeMarkdown(convertedByHost || convertedByTurndown || convertedPlainText);
  if (!markdown) return null;
  const source = convertedByHost
    ? "host"
    : convertedByTurndown
      ? document.querySelector("pre > code")
        ? "explicit-code"
        : "turndown"
      : "plain-text";

  return {
    markdown,
    remoteImages: Array.from(document.querySelectorAll("img")).flatMap((image) => {
      const remote = remoteImage(image);
      return remote ? [remote] : [];
    }),
    source,
    structured,
  };
}
