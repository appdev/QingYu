import { syntaxTree } from "@codemirror/language";
import type { Range } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type EditorView as CodeMirrorView,
  type ViewUpdate,
} from "@codemirror/view";
import {
  sanitizeRawHtml,
  type ResolveRawHtmlSrc,
} from "../raw-html-sanitize.ts";
import { defineMarkraPlugin } from "./plugin.ts";

export interface RawHtmlPreviewPluginOptions {
  resolveImageSrc?: ResolveRawHtmlSrc;
}

interface CodeMirrorHtmlRange {
  readonly block: boolean;
  readonly from: number;
  readonly source: string;
  readonly to: number;
}

function blockHtmlRanges(view: CodeMirrorView) {
  const ranges: CodeMirrorHtmlRange[] = [];
  syntaxTree(view.state).iterate({
    enter(node) {
      if (node.name !== "HTMLBlock") return;
      ranges.push({
        block: true,
        from: node.from,
        source: view.state.sliceDoc(node.from, node.to),
        to: node.to,
      });
      return false;
    },
  });
  return ranges;
}

function overlaps(
  range: { from: number; to: number },
  other: { from: number; to: number },
) {
  return range.from < other.to && range.to > other.from;
}

function inlineHtmlRanges(
  view: CodeMirrorView,
  blocks: readonly CodeMirrorHtmlRange[],
) {
  const ranges: CodeMirrorHtmlRange[] = [];
  const pattern = /<([A-Za-z][\w:-]*)(?:\s[^<>]*?)?>([^\n]*?)<\/\1\s*>/gu;

  for (let lineNumber = 1; lineNumber <= view.state.doc.lines; lineNumber += 1) {
    const line = view.state.doc.line(lineNumber);
    pattern.lastIndex = 0;
    for (const match of line.text.matchAll(pattern)) {
      const from = line.from + match.index;
      const to = from + match[0].length;
      if (blocks.some((block) => overlaps({ from, to }, block))) continue;
      ranges.push({ block: false, from, source: match[0], to });
    }
  }
  return ranges;
}

function selectionTouches(view: CodeMirrorView, range: CodeMirrorHtmlRange) {
  return (
    view.hasFocus &&
    view.state.selection.ranges.some((selection) =>
      selection.empty
        ? selection.head > range.from && selection.head < range.to
        : selection.from < range.to && selection.to > range.from,
    )
  );
}

function activateHtml(view: CodeMirrorView, range: CodeMirrorHtmlRange) {
  view.dispatch({
    selection: { anchor: Math.min(range.to - 1, range.from + 1) },
    scrollIntoView: true,
  });
  view.focus();
}

class RawHtmlWidget extends WidgetType {
  constructor(
    readonly range: CodeMirrorHtmlRange,
    readonly options: RawHtmlPreviewPluginOptions,
  ) {
    super();
  }

  eq(other: RawHtmlWidget) {
    return other.range.source === this.range.source && other.range.block === this.range.block;
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const nodes = sanitizeRawHtml(this.range.source, document, this.options);
    const wrapper = this.range.block ? document.createElement("div") : null;
    let root: HTMLElement;

    if (!this.range.block && nodes.length === 1 && nodes[0] instanceof HTMLElement) {
      root = nodes[0];
      root.classList.add("cm-markra-inline-html");
    } else {
      root = wrapper ?? document.createElement("span");
      root.append(...nodes);
      root.classList.add(this.range.block ? "markra-html-node" : "cm-markra-inline-html");
    }

    root.dataset.type = "html";
    root.dataset.value = this.range.source;
    root.tabIndex = 0;
    root.setAttribute("role", "button");
    root.setAttribute("aria-label", "Edit HTML source");
    const activate = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      activateHtml(view, this.range);
    };
    root.addEventListener("mousedown", activate);
    root.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") activate(event);
    });
    return root;
  }
}

function addBlockReplacement(
  view: CodeMirrorView,
  ranges: Range<Decoration>[],
  range: CodeMirrorHtmlRange,
  widget: WidgetType,
) {
  const firstLine = view.state.doc.lineAt(range.from);
  const lastLine = view.state.doc.lineAt(range.to);
  const firstTo = Math.min(firstLine.to, range.to);
  ranges.push(Decoration.replace({ widget }).range(range.from, firstTo));

  for (let lineNumber = firstLine.number + 1; lineNumber <= lastLine.number; lineNumber += 1) {
    const line = view.state.doc.line(lineNumber);
    const segmentTo = Math.min(line.to, range.to);
    if (line.from >= segmentTo) continue;
    if (segmentTo === line.to) {
      ranges.push(
        Decoration.line({ class: "cm-markra-html-hidden-line" }).range(line.from),
      );
    } else {
      ranges.push(Decoration.replace({}).range(line.from, segmentTo));
    }
  }
}

function buildRawHtmlDecorations(
  view: CodeMirrorView,
  options: RawHtmlPreviewPluginOptions,
) {
  const ranges: Range<Decoration>[] = [];
  const blocks = blockHtmlRanges(view);
  const htmlRanges = [...blocks, ...inlineHtmlRanges(view, blocks)].sort(
    (left, right) => left.from - right.from,
  );

  for (const range of htmlRanges) {
    if (selectionTouches(view, range)) continue;
    const widget = new RawHtmlWidget(range, options);
    if (range.block && range.source.includes("\n")) {
      addBlockReplacement(view, ranges, range, widget);
    } else {
      ranges.push(Decoration.replace({ widget }).range(range.from, range.to));
    }
  }

  return Decoration.set(ranges, true);
}

const rawHtmlTheme = EditorView.baseTheme({
  ".cm-markra-html-hidden-line": {
    display: "none",
  },
  ".markra-html-node": {
    display: "block",
    maxWidth: "100%",
  },
  ".markra-html-node img": {
    maxWidth: "100%",
  },
  ".cm-markra-inline-html": {
    cursor: "text",
  },
});

export function rawHtmlPreviewPlugin(
  options: RawHtmlPreviewPluginOptions = {},
) {
  return defineMarkraPlugin({
    id: "markra.raw-html-preview",
    extension: [
      ViewPlugin.fromClass(
        class {
          decorations: DecorationSet;

          constructor(view: CodeMirrorView) {
            this.decorations = buildRawHtmlDecorations(view, options);
          }

          update(update: ViewUpdate) {
            if (
              update.docChanged ||
              update.selectionSet ||
              update.focusChanged ||
              update.viewportChanged
            ) {
              this.decorations = buildRawHtmlDecorations(update.view, options);
            }
          }
        },
        { decorations: (plugin) => plugin.decorations },
      ),
      rawHtmlTheme,
    ],
  });
}
