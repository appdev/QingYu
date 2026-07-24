import type { Mark, MarkType, Node as ProseNode } from "@milkdown/kit/prose/model";
import { TextSelection, type EditorState } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";

type FoldedLinkRange = {
  from: number;
  label: string;
  mark: Mark;
  to: number;
};

export type FinalizedLinkRange = {
  from: number;
  mark: Mark;
  to: number;
};

export function createLinkIconWidget() {
  const icon = document.createElement("span");
  icon.className = "markra-live-link-icon markra-md-virtual-delimiter";
  icon.ariaHidden = "true";
  icon.contentEditable = "false";
  icon.draggable = false;
  return icon;
}

function markdownLinkDestination(href: string) {
  if (!/[\s()<>]/u.test(href)) return href;

  return `<${href.replace(/>/gu, "%3E")}>`;
}

function markdownLinkTitle(title: string | null) {
  if (!title) return "";

  return ` "${title.replace(/\\/gu, "\\\\").replace(/"/gu, '\\"')}"`;
}

function linkSourceSuffix(mark: Mark) {
  const href = String(mark.attrs.href ?? "");
  const title = typeof mark.attrs.title === "string" ? mark.attrs.title : null;

  return `](${markdownLinkDestination(href)}${markdownLinkTitle(title)})`;
}

function escapeLinkLabel(label: string) {
  return label.replace(/\\/gu, "\\\\").replace(/\]/gu, "\\]");
}

function linkMarkdownSource(label: string, mark: Mark) {
  return `[${escapeLinkLabel(label)}${linkSourceSuffix(mark)}`;
}

export function nodeLinkMark(node: ProseNode, link: MarkType) {
  return node.isText ? node.marks.find((mark) => mark.type === link) ?? null : null;
}

function findFoldedLinkRangeAtPosition(state: EditorState, link: MarkType, position: number): FoldedLinkRange | null {
  const docPosition = Math.max(0, Math.min(position, state.doc.content.size));
  const $position = state.doc.resolve(docPosition);
  if (!$position.parent.isTextblock) return null;

  const parent = $position.parent;
  const parentStart = $position.start();
  const children: ProseNode[] = [];
  let targetIndex: number | null = null;
  let targetMark: Mark | null = null;

  parent.forEach((node) => {
    children.push(node);
  });

  let offset = 0;
  for (let index = 0; index < children.length; index += 1) {
    const node = children[index];
    if (!node) continue;

    if (targetMark) {
      break;
    }

    const from = parentStart + offset;
    const to = from + node.nodeSize;
    if (docPosition < from || docPosition > to) {
      offset += node.nodeSize;
      continue;
    }

    const mark = nodeLinkMark(node, link);
    if (mark) {
      targetIndex = index;
      targetMark = mark;
    }
    offset += node.nodeSize;
  }

  if (targetIndex === null || !targetMark) return null;

  let startIndex = targetIndex;
  while (startIndex > 0 && nodeLinkMark(children[startIndex - 1], link)?.eq(targetMark)) {
    startIndex -= 1;
  }

  let endIndex = targetIndex;
  while (endIndex + 1 < children.length && nodeLinkMark(children[endIndex + 1], link)?.eq(targetMark)) {
    endIndex += 1;
  }

  let fromOffset = 0;
  for (let childIndex = 0; childIndex < startIndex; childIndex += 1) {
    fromOffset += children[childIndex]?.nodeSize ?? 0;
  }

  let toOffset = fromOffset;
  for (let childIndex = startIndex; childIndex <= endIndex; childIndex += 1) {
    toOffset += children[childIndex]?.nodeSize ?? 0;
  }

  return {
    from: parentStart + fromOffset,
    label: parent.textBetween(fromOffset, toOffset, ""),
    mark: targetMark,
    to: parentStart + toOffset
  };
}

export function expandFinalizedLinkMarkdown(view: EditorView, link: MarkType, position: number) {
  const range = findFoldedLinkRangeAtPosition(view.state, link, position);
  if (!range) return false;

  const markdown = linkMarkdownSource(range.label, range.mark);
  const cursorOffset = Math.max(0, Math.min(position - range.from, range.label.length));
  const tr = view.state.tr.replaceWith(range.from, range.to, view.state.schema.text(markdown));

  tr.setSelection(TextSelection.create(tr.doc, range.from + 1 + cursorOffset));
  view.dispatch(tr.scrollIntoView());
  view.focus();
  return true;
}

export function linkElementFromEventTarget(target: EventTarget | null) {
  const targetElement = target instanceof Element ? target : target instanceof Node ? target.parentElement : null;
  return targetElement?.closest<HTMLAnchorElement>("a[href]") ?? null;
}
