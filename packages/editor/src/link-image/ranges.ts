import type { MarkType, NodeType, Node as ProseNode } from "@milkdown/kit/prose/model";
import { type EditorState, TextSelection } from "@milkdown/kit/prose/state";
import { Transform } from "@milkdown/kit/prose/transform";
import type { AbsoluteRawMarkdownRange, LiveMarkdownReplacement, RawMarkdownRange } from "./types.ts";

const linkMarkdownPattern = /\[([^\]\n]+)]\(([^\s)\n]+)(?:\s+"([^"\n]*)")?\)/g;
const imageMarkdownPattern = /!\[([^\]\n]*)]\(([^\s)\n]+)(?:\s+"([^"\n]*)")?\)/g;

function overlaps(ranges: RawMarkdownRange[], from: number, to: number) {
  return ranges.some((range) => from < range.to && to > range.from);
}

function cursorIsInsideMatch(cursor: number, matchStart: number, matchedText: string) {
  return matchStart <= cursor && cursor <= matchStart + matchedText.length;
}

export function getRawMarkdownRanges(text: string) {
  const ranges: RawMarkdownRange[] = [];

  imageMarkdownPattern.lastIndex = 0;
  for (const match of text.matchAll(imageMarkdownPattern)) {
    if (match.index === undefined) continue;

    const [, alt = "", src, title = ""] = match;
    if (!src) continue;

    ranges.push({
      kind: "image",
      alt,
      src,
      title,
      from: match.index,
      to: match.index + match[0].length
    });
  }

  linkMarkdownPattern.lastIndex = 0;
  for (const match of text.matchAll(linkMarkdownPattern)) {
    if (match.index === undefined) continue;
    if (text[match.index - 1] === "!") continue;

    const [, label, href, title] = match;
    if (!label || !href) continue;

    const from = match.index;
    const labelFrom = from + 1;
    const labelTo = labelFrom + label.length;
    const to = from + match[0].length;
    if (overlaps(ranges, from, to)) continue;

    ranges.push({
      kind: "link",
      label,
      href,
      title: title ?? null,
      from,
      to,
      labelFrom,
      labelTo
    });
  }

  return ranges.sort((left, right) => left.from - right.from || right.to - left.to);
}

export function makeAbsoluteRange(range: RawMarkdownRange, blockStart: number): AbsoluteRawMarkdownRange {
  if (range.kind === "image") {
    return {
      ...range,
      from: blockStart + range.from,
      to: blockStart + range.to
    };
  }

  return {
    ...range,
    from: blockStart + range.from,
    to: blockStart + range.to,
    labelFrom: blockStart + range.labelFrom,
    labelTo: blockStart + range.labelTo
  };
}

export function findActiveRawMarkdownRange(state: EditorState): AbsoluteRawMarkdownRange | null {
  const { selection } = state;
  if (!(selection instanceof TextSelection)) return null;

  const { $from } = selection;
  if (!$from.parent.isTextblock) return null;
  if (!$from.sameParent(selection.$to)) return null;

  const from = Math.min($from.parentOffset, selection.$to.parentOffset);
  const to = Math.max($from.parentOffset, selection.$to.parentOffset);
  const range = getRawMarkdownRanges($from.parent.textContent).find((candidate) =>
    selection.empty
      ? cursorIsInsideMatch(from, candidate.from, $from.parent.textContent.slice(candidate.from, candidate.to))
      : candidate.from <= from && to <= candidate.to
  );

  return range ? makeAbsoluteRange(range, $from.start()) : null;
}

export function normalizeLinkImageLiveMarkdownDocument(doc: ProseNode, link: MarkType, image: NodeType) {
  const replacements: LiveMarkdownReplacement[] = [];

  doc.descendants((node, position) => {
    if (!node.isTextblock) return;

    const blockStart = position + 1;
    for (const range of getRawMarkdownRanges(node.textContent)) {
      const absoluteRange = makeAbsoluteRange(range, blockStart);
      const replacement =
        absoluteRange.kind === "image"
          ? image.create({
              alt: absoluteRange.alt,
              src: absoluteRange.src,
              title: absoluteRange.title
            })
          : doc.type.schema.text(absoluteRange.label, [
              link.create({ href: absoluteRange.href, title: absoluteRange.title })
            ]);

      replacements.push({
        from: absoluteRange.from,
        node: replacement,
        to: absoluteRange.to
      });
    }
  });

  if (replacements.length === 0) return doc;

  const transform = new Transform(doc);
  const sortedReplacements = [...replacements].sort((left, right) => right.from - left.from);
  for (const replacement of sortedReplacements) {
    transform.replaceWith(replacement.from, replacement.to, replacement.node);
  }

  return transform.doc;
}

export function serializeLinkImageLiveMarkdown(
  doc: ProseNode,
  serializeMarkdown: (doc: ProseNode) => string,
  link: MarkType,
  image: NodeType
) {
  return serializeMarkdown(normalizeLinkImageLiveMarkdownDocument(doc, link, image));
}
