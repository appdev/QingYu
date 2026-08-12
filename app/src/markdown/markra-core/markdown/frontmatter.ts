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
import {
  Document,
  isMap,
  isNode,
  isScalar,
  parseDocument as parseYamlDocument,
  type Node as YamlNode,
  type YAMLMap,
} from "yaml";

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

export interface MarkdownFrontmatterMetadata {
  readonly title: string | null;
  readonly tags: readonly string[];
  readonly icon: string | null;
  readonly cover: string | null;
}

export type MarkdownFrontmatterReadResult =
  | { readonly status: "none" }
  | { readonly status: "malformed" }
  | {
      readonly status: "valid";
      readonly title: string | null;
      readonly tags: readonly string[];
      readonly icon: string | null;
      readonly cover: string | null;
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

export type MarkdownFrontmatterMetadataPatch = Partial<{
  title: string;
  tags: readonly string[];
  icon: string;
  cover: string;
}>;

export type MarkdownFrontmatterMetadataUpsertResult = MarkdownFrontmatterTitleUpsertResult;

type FencedDelimiter = "---" | "+++";
type MetadataReadResult =
  | { readonly ok: true; readonly metadata: MarkdownFrontmatterMetadata }
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

function metadataFromObject(value: Record<string, unknown>): MarkdownFrontmatterMetadata {
  const tags = Array.isArray(value.tags)
    ? value.tags.filter((tag): tag is string => typeof tag === "string")
    : typeof value.tags === "string"
      ? [value.tags]
      : [];
  return {
    title: typeof value.title === "string" ? value.title : null,
    tags,
    icon: typeof value.icon === "string" && value.icon ? value.icon : null,
    cover: typeof value.cover === "string" && value.cover ? value.cover : null,
  };
}

function readYamlMetadata(content: string) {
  const document = parseYamlDocument(content);
  if (document.errors.length > 0 || !isMap(document.contents)) return { ok: false } as const;
  const value = document.toJSON() as unknown;
  return isObject(value)
    ? { ok: true, metadata: metadataFromObject(value) } as const
    : { ok: false } as const;
}

function readTomlMetadata(content: string) {
  try {
    const value = parseToml(content) as unknown;
    if (!isObject(value)) return { ok: false } as const;
    return { ok: true, metadata: metadataFromObject(value) } as const;
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

function readJsonMetadata(content: string) {
  const root = parseJsonObject(content);
  if (!root) return { ok: false } as const;
  const value = getNodeValue(root) as unknown;
  if (!isObject(value)) return { ok: false } as const;
  return { ok: true, metadata: metadataFromObject(value) } as const;
}

export function readMarkdownFrontmatter(source: string): MarkdownFrontmatterReadResult {
  const from = sourceStart(source);
  const detected = fencedRange(source, from) ?? jsonRange(source, from);
  if (detected === null) return { status: "none" };
  if (detected === "malformed") return { status: "malformed" };

  const content = source.slice(detected.contentFrom, detected.contentTo);
  const parsed: MetadataReadResult = detected.kind === "yaml"
    ? readYamlMetadata(content)
    : detected.kind === "toml"
      ? readTomlMetadata(content)
      : readJsonMetadata(detected.source);
  if (!parsed.ok) return { status: "malformed" };
  return { status: "valid", ...parsed.metadata, range: detected };
}

function replaceRange(source: string, from: number, to: number, replacement: string) {
  return `${source.slice(0, from)}${replacement}${source.slice(to)}`;
}

function topLevelYamlValue(map: YAMLMap, key: string): YamlNode | null {
  const pair = map.items.find((item) => isScalar(item.key) && item.key.value === key);
  return isNode(pair?.value) ? pair.value : null;
}

function encodeYamlValue(key: string, value: string | readonly string[], newline: string) {
  const entry = new Document({ [key]: value }).toString();
  const document = parseYamlDocument(entry);
  if (document.errors.length > 0 || !isMap(document.contents)) return null;
  const node = topLevelYamlValue(document.contents, key);
  if (!node?.range) return null;
  return {
    entry: entry.replace(/\n/gu, newline),
    value: entry.slice(node.range[0], node.range[1]).replace(/\n/gu, newline),
  };
}

function patchYaml(content: string, key: string, value: string | readonly string[], newline: string) {
  const document = parseYamlDocument(content);
  if (document.errors.length > 0 || !isMap(document.contents)) return null;
  const encoded = encodeYamlValue(key, value, newline);
  if (!encoded) return null;
  const node = topLevelYamlValue(document.contents, key);
  if (!node?.range) {
    const insertionAt = document.directives?.docEnd && document.range
      ? document.range[1]
      : content.length;
    const before = content.slice(0, insertionAt);
    const separator = before.length > 0 && !before.endsWith("\n") ? newline : "";
    return replaceRange(content, insertionAt, insertionAt, `${separator}${encoded.entry}`);
  }

  const [from, to] = node.range;
  const existingValue = content.slice(from, to);
  let replacement = encoded.value;
  if (from === to && content[from - 1] === ":") replacement = ` ${replacement}`;
  if (from === to && content[from] === "#") replacement = `${replacement} `;
  if (existingValue.endsWith("\n") && !replacement.endsWith(newline)) {
    replacement = `${replacement}${newline}`;
  }
  return replaceRange(content, from, to, replacement);
}

function patchTomlContent(content: string, key: string, nextValue: string | readonly string[]) {
  try {
    const value = parseToml(content) as unknown;
    if (!isObject(value)) return null;
    value[key] = typeof nextValue === "string" ? nextValue : [...nextValue];
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

function lastJsonPropertyValue(root: JsonNode, key: string) {
  const properties = root.children ?? [];
  for (let index = properties.length - 1; index >= 0; index -= 1) {
    const [keyNode, valueNode] = properties[index]?.children ?? [];
    if (keyNode?.value === key && valueNode) return valueNode;
  }
  return null;
}

function patchJson(content: string, key: string, value: string | readonly string[], newline: string) {
  const root = parseJsonObject(content);
  if (!root) return null;
  const existingValue = lastJsonPropertyValue(root, key);
  if (existingValue) {
    return applyEdits(content, [{
      offset: existingValue.offset,
      length: existingValue.length,
      content: JSON.stringify(value),
    }]);
  }
  const rawEdits = modify(content, [key], value, {});
  const edits = preserveJsonInsertionLayout(content, rawEdits, newline);
  return applyEdits(content, edits);
}

function insertYamlFrontmatter(source: string, patch: MarkdownFrontmatterMetadataPatch) {
  const from = sourceStart(source);
  const newline = detectNewline(source);
  const document = new Document(patch);
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
  return upsertMarkdownFrontmatterMetadata(source, { title });
}

export function upsertMarkdownFrontmatterMetadata(
  source: string,
  patch: MarkdownFrontmatterMetadataPatch,
): MarkdownFrontmatterMetadataUpsertResult {
  const result = readMarkdownFrontmatter(source);
  if (result.status === "malformed") return { ok: false, reason: "malformed" };
  const entries = Object.entries(patch) as Array<[
    keyof MarkdownFrontmatterMetadataPatch,
    string | readonly string[],
  ]>;
  if (entries.length === 0) {
    return {
      ok: true,
      changed: false,
      kind: result.status === "valid" ? result.range.kind : "yaml",
      source,
    };
  }
  if (result.status === "none") return insertYamlFrontmatter(source, patch);

  let nextSource = source;
  let changed = false;
  for (const [key, value] of entries) {
    const current = readMarkdownFrontmatter(nextSource);
    if (current.status !== "valid") return { ok: false, reason: "malformed" };
    const currentValue = current[key];
    if (Array.isArray(value)
      ? Array.isArray(currentValue) && value.length === currentValue.length
        && value.every((item, index) => item === currentValue[index])
      : currentValue === value) continue;

    const newline = detectNewline(current.range.source);
    const content = nextSource.slice(current.range.contentFrom, current.range.contentTo);
    const patched = current.range.kind === "yaml"
      ? patchYaml(content, key, value, newline)
      : current.range.kind === "toml"
        ? patchTomlContent(content, key, value)
        : patchJson(current.range.source, key, value, newline);
    if (patched === null) return { ok: false, reason: "malformed" };

    const from = current.range.kind === "json"
      ? current.range.from
      : current.range.contentFrom;
    const to = current.range.kind === "json"
      ? current.range.to
      : current.range.contentTo;
    nextSource = replaceRange(nextSource, from, to, patched);
    changed = true;
  }
  return {
    ok: true,
    changed,
    kind: result.range.kind,
    source: nextSource,
  };
}
