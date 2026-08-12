import { toString } from "mdast-util-to-string";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkParse from "remark-parse";
import { unified } from "unified";

export type MarkdownResourceReferenceKind =
  | "image"
  | "attachment"
  | "html-image"
  | "html-attachment";

export type MarkdownResourceReference = {
  columnNumber: number;
  from: number;
  href: string;
  kind: MarkdownResourceReferenceKind;
  lineNumber: number;
  text: string;
  to: number;
};

type MarkdownPositionPoint = {
  column?: number;
  line?: number;
  offset?: number;
};

type MarkdownPosition = {
  end?: MarkdownPositionPoint;
  start?: MarkdownPositionPoint;
};

type MarkdownNode = {
  alt?: string;
  children?: MarkdownNode[];
  identifier?: string;
  label?: string;
  position?: MarkdownPosition;
  title?: string;
  type: string;
  url?: string;
  value?: string;
};

type HtmlAttribute = {
  from: number;
  to: number;
  value: string;
};

const markdownDocumentPattern = /\.(?:md|markdown)(?:$|[?#])/iu;
const uriSchemePattern = /^[a-z][a-z0-9+.-]*:/iu;
const windowsAbsolutePathPattern = /^[a-z]:[\\/]/iu;
const resourceReferenceParser = unified().use(remarkParse).use(remarkGfm).use(remarkMath);

function isCandidateResourceHref(href: string) {
  const value = href.trim();
  if (!value || value.startsWith("#") || value.startsWith("//")) return false;
  if (uriSchemePattern.test(value) && !windowsAbsolutePathPattern.test(value)) return false;
  if (value.endsWith("/") || markdownDocumentPattern.test(value)) return false;
  return !/^\[\[.*\]\]$/u.test(value);
}

function walkMarkdownTree(node: MarkdownNode, visit: (node: MarkdownNode) => unknown) {
  visit(node);
  node.children?.forEach((child) => walkMarkdownTree(child, visit));
}

function normalizedDefinitionIdentifier(identifier: string | undefined) {
  return identifier?.trim().toLocaleLowerCase() ?? "";
}

function pointFromOffset(markdown: string, offset: number) {
  const before = markdown.slice(0, Math.max(0, offset));
  const lines = before.split(/\r?\n/u);

  return {
    columnNumber: (lines.at(-1)?.length ?? 0) + 1,
    lineNumber: lines.length
  };
}

function referenceFromNode(
  node: MarkdownNode,
  href: string,
  kind: MarkdownResourceReferenceKind,
  text: string
): MarkdownResourceReference {
  const from = node.position?.start?.offset ?? 0;
  const to = node.position?.end?.offset ?? from;

  return {
    columnNumber: node.position?.start?.column ?? 1,
    from,
    href,
    kind,
    lineNumber: node.position?.start?.line ?? 1,
    text,
    to
  };
}

function htmlAttribute(tag: string, attributeName: "alt" | "href" | "src", tagOffset: number): HtmlAttribute | null {
  const attributePattern = new RegExp(
    `\\b${attributeName}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s"'=<>\\x60]+?)(?=\\s|/?>))`,
    "iu"
  );
  const match = attributePattern.exec(tag);
  if (!match) return null;

  const value = match[1] ?? match[2] ?? match[3] ?? "";
  const matchValueOffset = match[0].lastIndexOf(value);
  const from = tagOffset + (match.index ?? 0) + Math.max(0, matchValueOffset);

  return {
    from,
    to: from + value.length,
    value
  };
}

function htmlResourceReferences(markdown: string, node: MarkdownNode) {
  const html = node.value ?? "";
  const nodeOffset = node.position?.start?.offset ?? 0;
  const references: MarkdownResourceReference[] = [];
  const tagPattern = /<(img|a)\b[^>]*>/giu;

  for (const match of html.matchAll(tagPattern)) {
    const tagName = match[1]?.toLocaleLowerCase();
    const tag = match[0];
    const tagOffset = nodeOffset + (match.index ?? 0);
    const attribute = htmlAttribute(tag, tagName === "img" ? "src" : "href", tagOffset);
    if (!attribute || !isCandidateResourceHref(attribute.value)) continue;

    const point = pointFromOffset(markdown, attribute.from);
    const alt = tagName === "img" ? htmlAttribute(tag, "alt", tagOffset)?.value ?? "" : "";
    references.push({
      columnNumber: point.columnNumber,
      from: attribute.from,
      href: attribute.value,
      kind: tagName === "img" ? "html-image" : "html-attachment",
      lineNumber: point.lineNumber,
      text: alt,
      to: attribute.to
    });
  }

  return references;
}

export function parseMarkdownResourceReferences(markdown: string): MarkdownResourceReference[] {
  const tree = resourceReferenceParser.runSync(resourceReferenceParser.parse(markdown)) as MarkdownNode;
  const definitions = new Map<string, string>();
  const references: MarkdownResourceReference[] = [];

  walkMarkdownTree(tree, (node) => {
    if (node.type !== "definition" || typeof node.url !== "string") return;

    const identifier = normalizedDefinitionIdentifier(node.identifier ?? node.label);
    if (identifier && !definitions.has(identifier)) definitions.set(identifier, node.url);
  });

  walkMarkdownTree(tree, (node) => {
    if (node.type === "html") {
      references.push(...htmlResourceReferences(markdown, node));
      return;
    }

    let href: string | undefined;
    let kind: MarkdownResourceReferenceKind | undefined;
    if (node.type === "image") {
      href = node.url;
      kind = "image";
    } else if (node.type === "link") {
      href = node.url;
      kind = "attachment";
    } else if (node.type === "imageReference") {
      href = definitions.get(normalizedDefinitionIdentifier(node.identifier ?? node.label));
      kind = "image";
    } else if (node.type === "linkReference") {
      href = definitions.get(normalizedDefinitionIdentifier(node.identifier ?? node.label));
      kind = "attachment";
    }

    if (!href || !kind || !isCandidateResourceHref(href)) return;

    const text = node.type.startsWith("image") ? node.alt ?? "" : toString(node as never);
    references.push(referenceFromNode(node, href, kind, text));
  });

  return references.sort((left, right) => left.from - right.from || left.to - right.to);
}
