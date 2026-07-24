import type { MarkType, NodeType } from "@milkdown/kit/prose/model";
import { Selection, TextSelection, type EditorState, type Transaction } from "@milkdown/kit/prose/state";
import { findActiveRawMarkdownRange } from "./ranges.ts";
import type { AbsoluteRawMarkdownRange } from "./types.ts";

function replaceLinkMarkdown(
  state: EditorState,
  link: MarkType,
  target: Extract<AbsoluteRawMarkdownRange, { kind: "link" }>
) {
  const linkText = state.schema.text(target.label, [link.create({ href: target.href, title: target.title })]);
  const tr = state.tr.replaceWith(target.from, target.to, linkText);

  return tr.setSelection(TextSelection.create(tr.doc, target.from + target.label.length));
}

function replaceImageMarkdown(
  state: EditorState,
  image: NodeType,
  target: Extract<AbsoluteRawMarkdownRange, { kind: "image" }>
) {
  const imageNode = image.create({ src: target.src, alt: target.alt, title: target.title });
  const tr = state.tr.replaceWith(target.from, target.to, imageNode);

  // Image is an inline atom, so place the cursor after it with a nearby valid selection.
  return tr.setSelection(Selection.near(tr.doc.resolve(target.from + imageNode.nodeSize)));
}

export function replaceRawMarkdownTarget(state: EditorState, link: MarkType, image: NodeType): Transaction | null {
  const target = findActiveRawMarkdownRange(state);
  if (!target) return null;

  return target.kind === "image"
    ? replaceImageMarkdown(state, image, target)
    : replaceLinkMarkdown(state, link, target);
}
