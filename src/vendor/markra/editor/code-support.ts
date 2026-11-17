import { common, createLowlight } from "lowlight";

type HighlightAstNode = {
  children?: HighlightAstNode[];
  properties?: { className?: unknown };
  type: string;
  value?: string;
};

export interface MarkraCodeHighlightSpan {
  readonly className: string;
  readonly from: number;
  readonly to: number;
}

const lowlight = createLowlight(common);

export function normalizeMarkraCodeLanguage(language: unknown) {
  if (typeof language !== "string") return "";
  return language.trim().replace(/[\s`]+/gu, "-");
}

function highlightClassNames(node: HighlightAstNode) {
  const className = node.properties?.className;
  if (!Array.isArray(className)) return [];
  return className.filter((value): value is string => typeof value === "string");
}

function collectHighlightRanges(
  node: HighlightAstNode,
  offset: number,
  inheritedClassNames: string[],
  ranges: MarkraCodeHighlightSpan[],
) {
  if (node.type === "text") {
    const value = node.value ?? "";
    if (value.length > 0 && inheritedClassNames.length > 0) {
      ranges.push({
        className: inheritedClassNames.join(" "),
        from: offset,
        to: offset + value.length,
      });
    }
    return offset + value.length;
  }

  const classNames = [...inheritedClassNames, ...highlightClassNames(node)];
  let cursor = offset;
  for (const child of node.children ?? []) {
    cursor = collectHighlightRanges(child, cursor, classNames, ranges);
  }
  return cursor;
}

export function highlightMarkraCode(language: string, code: string) {
  if (!code.trim()) return [];

  const normalizedLanguage = normalizeMarkraCodeLanguage(language);
  if (!normalizedLanguage || !lowlight.registered(normalizedLanguage)) return [];
  const ranges: MarkraCodeHighlightSpan[] = [];
  try {
    const tree = lowlight.highlight(normalizedLanguage, code);
    collectHighlightRanges(tree as HighlightAstNode, 0, [], ranges);
  } catch {
    return [];
  }
  return ranges;
}
