import type { EditorState } from "@codemirror/state";
import type { Element, MarkdownConfig } from "@lezer/markdown";
import { findCodeMirrorMathRanges } from "./math-preview.ts";
import type { MarkraSyntaxNode } from "./renderers.ts";

interface AttributeToken {
  readonly from: number;
  readonly name: string;
  readonly nameFrom: number;
  readonly nameTo: number;
  readonly to: number;
  readonly valueFrom: number | null;
  readonly valueTo: number | null;
}

interface ScannedAttributeList {
  readonly length: number;
  readonly tokens: readonly AttributeToken[];
}

function isSpace(character: string) {
  return character === " " || character === "\t";
}

function firstUnescapedEquals(source: string, from: number, to: number) {
  let escaped = false;
  for (let position = from; position < to; position += 1) {
    const character = source[position];
    if (!escaped && character === "=") return position;
    escaped = !escaped && character === "\\";
  }
  return -1;
}

function scanAttributeList(source: string): ScannedAttributeList | null {
  if (source[0] !== "{") return null;

  let closingBrace = -1;
  let escaped = false;
  for (let position = 1; position < source.length; position += 1) {
    const character = source[position];
    if (character === "\n" || character === "\r") return null;
    if (!escaped && character === "{") return null;
    if (!escaped && character === "}") {
      closingBrace = position;
      break;
    }
    escaped = !escaped && character === "\\";
  }
  if (closingBrace < 0) return null;

  const tokens: AttributeToken[] = [];
  let position = 1;
  while (position < closingBrace) {
    while (position < closingBrace && isSpace(source[position])) position += 1;
    if (position === closingBrace) break;

    const from = position;
    let tokenEscaped = false;
    while (position < closingBrace) {
      const character = source[position];
      if (!tokenEscaped && isSpace(character)) break;
      tokenEscaped = !tokenEscaped && character === "\\";
      position += 1;
    }
    const to = position;
    const equals = firstUnescapedEquals(source, from, to);

    if (equals >= 0) {
      if (equals === from || equals === to - 1) return null;
      tokens.push({
        from,
        name: source.slice(from, equals),
        nameFrom: from,
        nameTo: equals,
        to,
        valueFrom: equals + 1,
        valueTo: to,
      });
    } else {
      if (
        to === from + 1 ||
        (source[from] !== "#" && source[from] !== ".")
      ) {
        return null;
      }
      tokens.push({
        from,
        name: source.slice(from, to),
        nameFrom: from,
        nameTo: to,
        to,
        valueFrom: null,
        valueTo: null,
      });
    }
  }

  return { length: closingBrace + 1, tokens };
}

export const imageAttributesMarkdown: MarkdownConfig = {
  defineNodes: [
    "ImageAttributes",
    "ImageAttributeMark",
    "ImageAttributeName",
    "ImageAttributeValue",
  ],
  parseInline: [{
    name: "ImageAttributes",
    parse(cx, next, pos) {
      if (next !== 123) return -1;
      const parsed = scanAttributeList(cx.slice(pos, cx.end));
      if (!parsed) return -1;

      const elements: Element[] = [
        cx.elt("ImageAttributeMark", pos, pos + 1),
      ];
      for (const token of parsed.tokens) {
        elements.push(cx.elt(
          "ImageAttributeName",
          pos + token.nameFrom,
          pos + token.nameTo,
        ));
        if (token.valueFrom !== null && token.valueTo !== null) {
          elements.push(cx.elt(
            "ImageAttributeValue",
            pos + token.valueFrom,
            pos + token.valueTo,
          ));
        }
      }
      elements.push(cx.elt(
        "ImageAttributeMark",
        pos + parsed.length - 1,
        pos + parsed.length,
      ));
      return cx.addElement(cx.elt(
        "ImageAttributes",
        pos,
        pos + parsed.length,
        elements,
      ));
    },
  }],
};

export interface ImageAttributeDetails {
  readonly ownedFrom: number;
  readonly attributesFrom: number | null;
  readonly attributesTo: number | null;
  readonly authoredWidthPx: number | null;
  readonly ownedTo: number;
  readonly widthValueFrom: number | null;
  readonly widthValueTo: number | null;
}

export function imageAttributeDetails(
  state: EditorState,
  image: MarkraSyntaxNode,
): ImageAttributeDetails {
  const attributes = image.nextSibling;
  if (
    findCodeMirrorMathRanges(state).some((range) => (
      image.from >= range.from && image.to <= range.to
    )) ||
    attributes?.name !== "ImageAttributes" ||
    attributes.from !== image.to
  ) {
    return {
      ownedFrom: image.from,
      attributesFrom: null,
      attributesTo: null,
      authoredWidthPx: null,
      ownedTo: image.to,
      widthValueFrom: null,
      widthValueTo: null,
    };
  }

  const parsed = scanAttributeList(
    state.sliceDoc(attributes.from, attributes.to),
  );
  const widths = parsed?.tokens.filter((token) => token.name === "width") ?? [];
  const width = widths.length === 1 ? widths[0] : null;
  const value = !width || width.valueFrom === null || width.valueTo === null
    ? null
    : state.sliceDoc(
        attributes.from + width.valueFrom,
        attributes.from + width.valueTo,
      );
  const validWidth = value !== null && /^[1-9]\d*px$/u.test(value);

  return {
    ownedFrom: image.from,
    attributesFrom: attributes.from,
    attributesTo: attributes.to,
    authoredWidthPx: validWidth ? Number.parseInt(value, 10) : null,
    ownedTo: attributes.to,
    widthValueFrom: validWidth && width !== null && width.valueFrom !== null
      ? attributes.from + width.valueFrom
      : null,
    widthValueTo: validWidth && width !== null && width.valueTo !== null
      ? attributes.from + width.valueTo
      : null,
  };
}

function rebuildAttributeList(
  source: string,
  parsed: ScannedAttributeList,
  widthPx: number | null,
) {
  const retained = parsed.tokens
    .map((token, index) => ({ token, index }))
    .filter(({ token }) => token.name !== "width");
  if (retained.length === 0) {
    return widthPx === null ? "" : `{width=${widthPx}px}`;
  }

  let content = "";
  for (let retainedIndex = 0; retainedIndex < retained.length; retainedIndex += 1) {
    const { token, index } = retained[retainedIndex];
    if (retainedIndex === 0) {
      content += index === 0 ? source.slice(1, token.from) : "";
    } else {
      const previous = retained[retainedIndex - 1];
      content += index === previous.index + 1
        ? source.slice(previous.token.to, token.from)
        : " ";
    }
    content += source.slice(token.from, token.to);
  }

  const last = retained[retained.length - 1];
  const suffix = last.index === parsed.tokens.length - 1
    ? source.slice(last.token.to, parsed.length - 1)
    : "";
  if (widthPx !== null) content += ` width=${widthPx}px`;
  return `{${content}${suffix}}`;
}

export function replaceImageWidth(
  source: string,
  details: ImageAttributeDetails,
  widthPx: number | null,
): string {
  if (details.attributesFrom === null || details.attributesTo === null) {
    return widthPx === null ? source : `${source}{width=${widthPx}px}`;
  }

  if (
    widthPx !== null &&
    details.widthValueFrom !== null &&
    details.widthValueTo !== null
  ) {
    const from = details.widthValueFrom - details.ownedFrom;
    const to = details.widthValueTo - details.ownedFrom;
    return `${source.slice(0, from)}${widthPx}px${source.slice(to)}`;
  }

  const attributesFrom = details.attributesFrom - details.ownedFrom;
  const attributesTo = details.attributesTo - details.ownedFrom;
  const attributeSource = source.slice(attributesFrom, attributesTo);
  const parsed = scanAttributeList(attributeSource);
  if (!parsed) return source;
  const replacement = rebuildAttributeList(attributeSource, parsed, widthPx);
  return `${source.slice(0, attributesFrom)}${replacement}${source.slice(attributesTo)}`;
}
