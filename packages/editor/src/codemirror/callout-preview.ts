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
  markdownCalloutDefinitions,
  markdownCalloutMarkerForType,
  parseMarkdownCalloutMarker,
  type MarkdownCalloutType,
  type ParsedMarkdownCalloutMarker,
} from "@markra/shared";
import { defineMarkraPlugin } from "./plugin.ts";

export interface CalloutPreviewPluginOptions {
  enabled?: boolean;
}

interface CodeMirrorCallout {
  readonly from: number;
  readonly lineFroms: readonly number[];
  readonly marker: ParsedMarkdownCalloutMarker;
  readonly markerFrom: number;
  readonly markerTo: number;
  readonly to: number;
}

const calloutTypeOrder: readonly MarkdownCalloutType[] = [
  "note",
  "tip",
  "important",
  "warning",
  "caution",
];

function blockquoteContent(line: string) {
  const match = /^[ \t]{0,3}((?:>[ \t]*)+)(.*)$/u.exec(line);
  return match
    ? {
        content: match[2] ?? "",
        prefix: match[1] ?? "> ",
      }
    : null;
}

function readCodeMirrorCallouts(view: CodeMirrorView) {
  const callouts: CodeMirrorCallout[] = [];

  for (let lineNumber = 1; lineNumber <= view.state.doc.lines; lineNumber += 1) {
    const line = view.state.doc.line(lineNumber);
    const quote = blockquoteContent(line.text);
    const marker = quote ? parseMarkdownCalloutMarker(quote.content) : null;
    if (!quote || !marker) continue;

    const markerOffset = line.text.indexOf(marker.source, quote.prefix.length);
    if (markerOffset < 0) continue;
    const lineFroms = [line.from];
    let to = line.to;
    let continuationLine = lineNumber + 1;
    while (continuationLine <= view.state.doc.lines) {
      const continuation = view.state.doc.line(continuationLine);
      if (!blockquoteContent(continuation.text)) break;
      lineFroms.push(continuation.from);
      to = continuation.to;
      continuationLine += 1;
    }

    callouts.push({
      from: line.from,
      lineFroms,
      marker,
      markerFrom: line.from + markerOffset,
      markerTo: line.from + markerOffset + marker.source.length,
      to,
    });
    lineNumber = continuationLine - 1;
  }

  return callouts;
}

function selectionTouches(view: CodeMirrorView, from: number, to: number) {
  return (
    view.hasFocus &&
    view.state.selection.ranges.some((selection) =>
      selection.empty
        ? selection.head > from && selection.head < to
        : selection.from < to && selection.to > from,
    )
  );
}

class CalloutHeaderWidget extends WidgetType {
  constructor(readonly callout: CodeMirrorCallout) {
    super();
  }

  eq(other: CalloutHeaderWidget) {
    return (
      other.callout.marker.type === this.callout.marker.type &&
      other.callout.markerFrom === this.callout.markerFrom
    );
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const header = document.createElement("span");
    const icon = document.createElement("span");
    const title = document.createElement("span");
    const select = document.createElement("select");

    header.className = "markra-callout-header";
    icon.className = "markra-callout-icon";
    icon.setAttribute("aria-hidden", "true");
    title.className = "markra-callout-title";
    title.textContent = this.callout.marker.label;
    select.className = "markra-callout-type-select";
    select.ariaLabel = "Callout type";
    select.disabled = view.state.readOnly;

    for (const type of calloutTypeOrder) {
      const option = document.createElement("option");
      option.value = type;
      option.textContent = markdownCalloutDefinitions[type].label;
      option.selected = type === this.callout.marker.type;
      select.append(option);
    }

    select.addEventListener("mousedown", (event) => event.stopPropagation());
    select.addEventListener("click", (event) => event.stopPropagation());
    select.addEventListener("change", (event) => {
      event.preventDefault();
      event.stopPropagation();
      if (view.state.readOnly) return;
      const type = select.value as MarkdownCalloutType;
      if (!calloutTypeOrder.includes(type)) return;
      view.dispatch({
        changes: {
          from: this.callout.markerFrom,
          insert: markdownCalloutMarkerForType(type),
          to: this.callout.markerTo,
        },
      });
      view.focus();
    });

    header.append(icon, title, select);
    return header;
  }
}

function buildCalloutDecorations(view: CodeMirrorView) {
  const ranges: Range<Decoration>[] = [];

  for (const callout of readCodeMirrorCallouts(view)) {
    for (const lineFrom of callout.lineFroms) {
      ranges.push(
        Decoration.line({
          attributes: {
            "data-callout-label": callout.marker.label,
            "data-callout-type": callout.marker.type,
          },
          class: `cm-markra-callout markra-callout markra-callout-${callout.marker.type}`,
        }).range(lineFrom),
      );
    }

    if (selectionTouches(view, callout.markerFrom, callout.markerTo)) continue;
    ranges.push(
      Decoration.replace({ widget: new CalloutHeaderWidget(callout) }).range(
        callout.markerFrom,
        callout.markerTo,
      ),
    );
  }

  return Decoration.set(ranges, true);
}

const calloutTheme = EditorView.baseTheme({
  ".cm-line.cm-markra-callout": {
    background: "color-mix(in srgb, var(--callout-color, #4f7cac) 6%, transparent)",
    borderLeft: "3px solid var(--callout-color, #4f7cac)",
    paddingLeft: "0.75em",
  },
  ".cm-line.cm-markra-callout.markra-callout-tip": {
    "--callout-color": "#299764",
  },
  ".cm-line.cm-markra-callout.markra-callout-important": {
    "--callout-color": "#8250df",
  },
  ".cm-line.cm-markra-callout.markra-callout-warning": {
    "--callout-color": "#9a6700",
  },
  ".cm-line.cm-markra-callout.markra-callout-caution": {
    "--callout-color": "#cf222e",
  },
  ".markra-callout-header": {
    alignItems: "center",
    color: "var(--callout-color, #4f7cac)",
    display: "inline-flex",
    fontWeight: "650",
    gap: "0.4em",
  },
  ".markra-callout-type-select": {
    background: "transparent",
    border: "1px solid transparent",
    borderRadius: "0.35em",
    color: "inherit",
    font: "inherit",
    opacity: "0",
  },
  ".cm-markra-callout:hover .markra-callout-type-select, .markra-callout-type-select:focus": {
    opacity: "1",
  },
});

export function calloutPreviewPlugin(
  options: CalloutPreviewPluginOptions = {},
) {
  const enabled = options.enabled ?? true;
  return defineMarkraPlugin({
    id: "markra.callout-preview",
    extension: enabled
      ? [
          ViewPlugin.fromClass(
            class {
              decorations: DecorationSet;

              constructor(view: CodeMirrorView) {
                this.decorations = buildCalloutDecorations(view);
              }

              update(update: ViewUpdate) {
                if (
                  update.docChanged ||
                  update.selectionSet ||
                  update.focusChanged ||
                  update.viewportChanged
                ) {
                  this.decorations = buildCalloutDecorations(update.view);
                }
              }
            },
            { decorations: (plugin) => plugin.decorations },
          ),
          calloutTheme,
        ]
      : [],
  });
}
