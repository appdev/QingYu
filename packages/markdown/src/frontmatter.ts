import { parse as parseToml, patch as patchToml } from "@decimalturn/toml-patch";
import {
  applyEdits,
  getNodeValue,
  modify,
  parseTree,
  type Edit,
  type Node as JsonNode,
  type ParseError,
} from "jsonc-parser";
import { Document, isMap, parseDocument as parseYamlDocument } from "yaml";

export type MarkdownFrontmatterKind = "yaml" | "toml" | "json";

export interface MarkdownFrontmatterRange {
  readonly from: number;
  readonly to: number;
  readonly contentFrom: number;
  readonly contentTo: number;
  readonly kind: MarkdownFrontmatterKind;
  readonly delimiter?: "---" | "+++";
  readonly source: string;
}

export type MarkdownFrontmatterReadResult =
  | { readonly status: "none" }
  | { readonly status: "malformed" }
  | {
      readonly status: "valid";
      readonly title: string | null;
      readonly range: MarkdownFrontmatterRange;
    };

export type MarkdownFrontmatterTitleUpsertResult =
  | {
      readonly ok: true;
      readonly changed: boolean;
      readonly source: string;
      readonly kind: MarkdownFrontmatterKind;
    }
  | { readonly ok: false; readonly reason: "malformed" };

type FencedDelimiter = "---" | "+++";
type TitleReadResult =
  | { readonly ok: true; readonly title: string | null }
  | { readonly ok: false };

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function sourceStart(source: string) {
  return source.charCodeAt(0) === 0xfeff ? 1 : 0;
}

function detectNewline(source: string) {
  return source.includes("\r\n") ? "\r\n" : "\n";
}

function findClosingDelimiter(
  source: string,
  contentFrom: number,
  delimiter: FencedDelimiter,
) {
  let lineFrom = contentFrom;
  while (lineFrom <= source.length) {
    const newlineAt = source.indexOf("\n", lineFrom);
    const lineTo = newlineAt === -1
      ? source.length
      : newlineAt > lineFrom && source[newlineAt - 1] === "\r"
        ? newlineAt - 1
        : newlineAt;
    const line = source.slice(lineFrom, lineTo);
    if (line.trimEnd() === delimiter && line.slice(delimiter.length).trim() === "") {
      return { from: lineFrom, to: lineTo };
    }
    if (newlineAt === -1) return null;
    lineFrom = newlineAt + 1;
  }
  return null;
}

function fencedRange(
  source: string,
  from: number,
): MarkdownFrontmatterRange | "malformed" | null {
  const opening = /^(---|\+\+\+)[ \t]*(?:\r\n|\n)/u.exec(source.slice(from));
  if (!opening) {
    const openingWithoutNewline = /^(---|\+\+\+)[ \t]*$/u.exec(source.slice(from));
    return openingWithoutNewline ? "malformed" : null;
  }

  const delimiter = opening[1] as FencedDelimiter;
  const contentFrom = from + opening[0].length;
  const closing = findClosingDelimiter(source, contentFrom, delimiter);
  if (!closing) return "malformed";

  return {
    from,
    to: closing.to,
    contentFrom,
    contentTo: closing.from,
    kind: delimiter === "---" ? "yaml" : "toml",
    delimiter,
    source: source.slice(from, closing.to),
  };
}

function findJsonObjectEnd(source: string, from: number) {
  let blockComment = false;
  let depth = 0;
  let escaped = false;
  let lineComment = false;
  let string = false;

  for (let index = from; index < source.length; index += 1) {
    const character = source[index];
    const next = source[index + 1];
    if (lineComment) {
      if (character === "\n" || character === "\r") lineComment = false;
      continue;
    }
    if (blockComment) {
      if (character === "*" && next === "/") {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (string) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === '"') {
        string = false;
      }
      continue;
    }
    if (character === '"') {
      string = true;
    } else if (character === "/" && next === "/") {
      lineComment = true;
      index += 1;
    } else if (character === "/" && next === "*") {
      blockComment = true;
      index += 1;
    } else if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return null;
}

function jsonRange(
  source: string,
  from: number,
): MarkdownFrontmatterRange | "malformed" | null {
  if (source[from] !== "{") return null;
  const to = findJsonObjectEnd(source, from);
  if (to === null) return "malformed";

  let after = to;
  while (source[after] === " " || source[after] === "\t") after += 1;
  if (after < source.length && source[after] !== "\n" && source[after] !== "\r") {
    return "malformed";
  }

  return {
    from,
    to,
    contentFrom: from + 1,
    contentTo: to - 1,
    kind: "json",
    source: source.slice(from, to),
  };
}

function readYamlTitle(content: string) {
  const document = parseYamlDocument(content);
  if (document.errors.length > 0 || !isMap(document.contents)) return { ok: false } as const;
  const title = document.get("title");
  return { ok: true, title: typeof title === "string" ? title : null } as const;
}

function readTomlTitle(content: string) {
  try {
    const value = parseToml(content) as unknown;
    if (!isObject(value)) return { ok: false } as const;
    return {
      ok: true,
      title: typeof value.title === "string" ? value.title : null,
    } as const;
  } catch {
    return { ok: false } as const;
  }
}

function parseJsonObject(content: string): JsonNode | null {
  const errors: ParseError[] = [];
  const root = parseTree(content, errors, {
    allowTrailingComma: false,
    disallowComments: true,
  });
  return errors.length === 0 && root?.type === "object" ? root : null;
}

function readJsonTitle(content: string) {
  const root = parseJsonObject(content);
  if (!root) return { ok: false } as const;
  const value = getNodeValue(root) as unknown;
  if (!isObject(value)) return { ok: false } as const;
  return {
    ok: true,
    title: typeof value.title === "string" ? value.title : null,
  } as const;
}

export function readMarkdownFrontmatter(source: string): MarkdownFrontmatterReadResult {
  const from = sourceStart(source);
  const detected = fencedRange(source, from) ?? jsonRange(source, from);
  if (detected === null) return { status: "none" };
  if (detected === "malformed") return { status: "malformed" };

  const content = source.slice(detected.contentFrom, detected.contentTo);
  const parsed: TitleReadResult = detected.kind === "yaml"
    ? readYamlTitle(content)
    : detected.kind === "toml"
      ? readTomlTitle(content)
      : readJsonTitle(detected.source);
  if (!parsed.ok) return { status: "malformed" };
  return { status: "valid", title: parsed.title, range: detected };
}

function replaceRange(source: string, from: number, to: number, replacement: string) {
  return `${source.slice(0, from)}${replacement}${source.slice(to)}`;
}

function patchYaml(content: string, title: string, newline: string) {
  const document = parseYamlDocument(content);
  if (document.errors.length > 0 || !isMap(document.contents)) return null;
  document.set("title", title);
  return document.toString().replace(/\n/gu, newline);
}

function patchTomlContent(content: string, title: string) {
  try {
    const value = parseToml(content) as unknown;
    if (!isObject(value)) return null;
    value.title = title;
    return patchToml(content, value);
  } catch {
    return null;
  }
}

function jsonIndent(content: string) {
  const match = /(?:\r\n|\n)([\t ]+)"/u.exec(content);
  return match?.[1] ?? "  ";
}

function preserveJsonInsertionLayout(content: string, edits: Edit[], newline: string) {
  if (edits.length !== 1 || !edits[0]) return edits;
  const edit = edits[0];
  const multiline = content.includes("\n");
  if (!edit.content.startsWith(",")) {
    if (!multiline || content[edit.offset - 1] !== "{") return edits;
    return [{
      ...edit,
      content: `${newline}${jsonIndent(content)}${edit.content}`,
    }];
  }
  const lineStart = Math.max(content.lastIndexOf("\n", edit.offset - 1) + 1, 0);
  if (!multiline || content.slice(lineStart, edit.offset).trim() === "") return edits;
  return [{
    ...edit,
    content: `,${newline}${jsonIndent(content)}${edit.content.slice(1)}`,
  }];
}

function patchJson(content: string, title: string, newline: string) {
  if (!parseJsonObject(content)) return null;
  const rawEdits = modify(content, ["title"], title, {});
  const edits = preserveJsonInsertionLayout(content, rawEdits, newline);
  return applyEdits(content, edits);
}

function insertYamlFrontmatter(source: string, title: string) {
  const from = sourceStart(source);
  const newline = detectNewline(source);
  const document = new Document({ title });
  const content = document.toString().replace(/\n/gu, newline);
  const metadata = `---${newline}${content}---${newline}${newline}`;
  return {
    ok: true,
    changed: true,
    kind: "yaml",
    source: `${source.slice(0, from)}${metadata}${source.slice(from)}`,
  } as const;
}

export function upsertMarkdownFrontmatterTitle(
  source: string,
  title: string,
): MarkdownFrontmatterTitleUpsertResult {
  const result = readMarkdownFrontmatter(source);
  if (result.status === "malformed") return { ok: false, reason: "malformed" };
  if (result.status === "none") return insertYamlFrontmatter(source, title);
  if (result.title === title) {
    return {
      ok: true,
      changed: false,
      kind: result.range.kind,
      source,
    };
  }

  const newline = detectNewline(result.range.source);
  const content = source.slice(result.range.contentFrom, result.range.contentTo);
  const patched = result.range.kind === "yaml"
    ? patchYaml(content, title, newline)
    : result.range.kind === "toml"
      ? patchTomlContent(content, title)
      : patchJson(result.range.source, title, newline);
  if (patched === null) return { ok: false, reason: "malformed" };

  const from = result.range.kind === "json"
    ? result.range.from
    : result.range.contentFrom;
  const to = result.range.kind === "json"
    ? result.range.to
    : result.range.contentTo;
  return {
    ok: true,
    changed: true,
    kind: result.range.kind,
    source: replaceRange(source, from, to, patched),
  };
}
