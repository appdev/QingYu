import TurndownService from "turndown";
import type { RemoteClipboardImage } from "../clipboard-asset-types.ts";

export interface CodeMirrorHtmlPaste {
  readonly markdown: string;
  readonly remoteImages: readonly RemoteClipboardImage[];
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

function createTurndownService() {
  const service = new TurndownService({
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

export function convertCodeMirrorClipboardHtml(html: string): CodeMirrorHtmlPaste | null {
  if (!html.trim() || typeof DOMParser === "undefined") return null;
  const document = new DOMParser().parseFromString(html, "text/html");
  const service = createTurndownService();
  const markdown = service
    .turndown(document.body.innerHTML)
    .replace(/\r\n?/gu, "\n")
    .replace(/\n{3,}/gu, "\n\n")
    .trim();
  if (!markdown) return null;

  return {
    markdown,
    remoteImages: Array.from(document.querySelectorAll("img")).flatMap((image) => {
      const remote = remoteImage(image);
      return remote ? [remote] : [];
    }),
  };
}
