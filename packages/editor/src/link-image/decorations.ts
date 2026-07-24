import type { Mark, MarkType, Node as ProseNode } from "@milkdown/kit/prose/model";
import { Decoration, DecorationSet } from "@milkdown/kit/prose/view";
import { createImagePreview } from "./images.ts";
import { createLinkIconWidget, nodeLinkMark, type FinalizedLinkRange } from "./links.ts";
import { getRawMarkdownRanges, makeAbsoluteRange } from "./ranges.ts";
import type { AbsoluteRawMarkdownRange, ResolveMarkdownImageSrc } from "./types.ts";

function buildLinkDecorations(
  decorations: Decoration[],
  range: Extract<AbsoluteRawMarkdownRange, { kind: "link" }>,
  isActive: boolean
) {
  const delimiterClass = isActive ? "markra-md-delimiter" : "markra-md-hidden-delimiter";
  const labelAttrs = isActive
    ? {
        class: "markra-live-link-source-label"
      }
    : {
        class: "markra-live-link-label",
        "data-markra-href": range.href
      };

  // Link Markdown remains editable source text; only the label receives link-like styling.
  decorations.push(
    Decoration.inline(range.from, range.labelFrom, {
      class: `markra-live-link-source ${delimiterClass}`
    }),
    Decoration.inline(range.labelFrom, range.labelTo, labelAttrs),
    Decoration.inline(range.labelTo, range.to, {
      class: `markra-live-link-source ${delimiterClass}`
    })
  );

  if (!isActive) {
    decorations.push(
      Decoration.widget(range.labelTo, createLinkIconWidget, {
        ignoreSelection: true,
        key: `markra-raw-link-icon-${range.from}-${range.to}`,
        side: 1
      })
    );
  }
}

function collectFinalizedLinkRanges(doc: ProseNode, link: MarkType) {
  const ranges: FinalizedLinkRange[] = [];
  let activeRange: FinalizedLinkRange | null = null;

  const flush = () => {
    if (!activeRange) return;

    ranges.push(activeRange);
    activeRange = null;
  };

  doc.descendants((node, position) => {
    const mark: Mark | null = nodeLinkMark(node, link);
    if (!node.isText || !mark) {
      flush();
      return;
    }

    const from = position;
    const to = position + node.nodeSize;
    if (activeRange && activeRange.to === from && activeRange.mark.eq(mark)) {
      activeRange.to = to;
      return;
    }

    flush();
    activeRange = { from, mark, to };
  });

  flush();
  return ranges;
}

function buildFinalizedLinkIconDecorations(decorations: Decoration[], doc: ProseNode, link: MarkType) {
  for (const range of collectFinalizedLinkRanges(doc, link)) {
    decorations.push(
      Decoration.widget(range.to, createLinkIconWidget, {
        ignoreSelection: true,
        key: `markra-finalized-link-icon-${range.from}-${range.to}`,
        side: 1
      })
    );
  }
}

function buildImageDecorations(
  decorations: Decoration[],
  range: Extract<AbsoluteRawMarkdownRange, { kind: "image" }>,
  isActive: boolean,
  resolveImageSrc: ResolveMarkdownImageSrc | undefined
) {
  const sourceClass = isActive ? "markra-md-delimiter" : "markra-md-hidden-delimiter";

  decorations.push(
    Decoration.inline(range.from, range.to, {
      class: `markra-live-image-source ${sourceClass}`
    })
  );

  if (!isActive) {
    // Fold inactive image source behind a preview, matching the live Markdown model used for marks.
    decorations.push(Decoration.widget(range.from, () => createImagePreview(range, resolveImageSrc), { side: -1 }));
  }
}

export function buildLiveLinkImageDecorations(
  doc: ProseNode,
  activeRange: AbsoluteRawMarkdownRange | null,
  link: MarkType,
  resolveImageSrc: ResolveMarkdownImageSrc | undefined
) {
  const decorations: Decoration[] = [];

  buildFinalizedLinkIconDecorations(decorations, doc, link);

  doc.descendants((node, position) => {
    if (!node.isTextblock) return;

    const blockStart = position + 1;
    const ranges = getRawMarkdownRanges(node.textContent);

    for (const relativeRange of ranges) {
      const range = makeAbsoluteRange(relativeRange, blockStart);
      const isActive = activeRange?.from === range.from && activeRange.to === range.to;

      if (range.kind === "image") {
        buildImageDecorations(decorations, range, isActive, resolveImageSrc);
      } else {
        buildLinkDecorations(decorations, range, isActive);
      }
    }
  });

  return DecorationSet.create(doc, decorations);
}
