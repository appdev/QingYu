import {
  StateField,
  type EditorState as CodeMirrorState,
  type Range,
} from "@codemirror/state";
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
} from "../shared";
import { defineMarkraPlugin } from "./plugin";
import { cursorInsideRange, selectionChangeAffectsReveal } from "./policy";
import {focusAdjacentVisualBlockBoundary} from "./visual-block-navigation";

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

function readCodeMirrorCallouts(state: CodeMirrorState) {
  const callouts: CodeMirrorCallout[] = [];

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    const line = state.doc.line(lineNumber);
    const quote = blockquoteContent(line.text);
    const marker = quote ? parseMarkdownCalloutMarker(quote.content) : null;
    if (!quote || !marker) continue;

    const markerOffset = line.text.indexOf(marker.source, quote.prefix.length);
    if (markerOffset < 0) continue;
    const lineFroms = [line.from];
    let to = line.to;
    let continuationLine = lineNumber + 1;
    while (continuationLine <= state.doc.lines) {
      const continuation = state.doc.line(continuationLine);
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

function selectionTouchesCallout(
  view: CodeMirrorView,
  callout: CodeMirrorCallout,
) {
  return (
    view.hasFocus &&
    view.state.selection.ranges.some(
      (selection) =>
        selection.empty &&
        selection.head >= callout.from &&
        selection.head <= callout.to,
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

class CalloutSpacerWidget extends WidgetType {
  constructor(readonly edge: "before" | "after") {
    super();
  }

  eq(other: CalloutSpacerWidget) {
    return other.edge === this.edge;
  }

  toDOM(view: CodeMirrorView) {
    const spacer = view.dom.ownerDocument.createElement("div");
    spacer.className = `markra-callout-spacer markra-callout-spacer-${this.edge}`;
    spacer.setAttribute("aria-hidden", "true");
    return spacer;
  }
}

function buildCalloutDecorations(view: CodeMirrorView) {
  const ranges: Range<Decoration>[] = [];

  for (const callout of readCodeMirrorCallouts(view.state)) {
    const active = selectionTouchesCallout(view, callout);
    for (const [index, lineFrom] of callout.lineFroms.entries()) {
      const positionClasses = [
        index === 0 ? "markra-callout-first" : "",
        index === callout.lineFroms.length - 1 ? "markra-callout-last" : "",
        active ? "markra-callout-active" : "",
      ].filter(Boolean).join(" ");
      ranges.push(
        Decoration.line({
          attributes: {
            "data-appearance-state": "ready",
            "data-callout-label": callout.marker.label,
            "data-callout-type": callout.marker.type,
          },
          class: `cm-markra-callout markra-callout markra-callout-${callout.marker.type} ${positionClasses}`,
        }).range(lineFrom),
      );
    }

    if (cursorInsideRange(view, callout.markerFrom, callout.markerTo)) continue;
    ranges.push(
      Decoration.replace({ widget: new CalloutHeaderWidget(callout) }).range(
        callout.markerFrom,
        callout.markerTo,
      ),
    );
  }

  return Decoration.set(ranges, true);
}

function buildCalloutSpacingDecorations(state: CodeMirrorState) {
  const ranges: Range<Decoration>[] = [];

  for (const callout of readCodeMirrorCallouts(state)) {
    // Margins on a .cm-line can sit outside CodeMirror's measured height map,
    // making pointer coordinates drift after every callout. Block widgets keep
    // the visual spacing and the editor's document geometry synchronized.
    ranges.push(
      Decoration.widget({
        block: true,
        side: -1,
        widget: new CalloutSpacerWidget("before"),
      }).range(callout.from),
      Decoration.widget({
        block: true,
        side: 1,
        widget: new CalloutSpacerWidget("after"),
      }).range(callout.to),
    );
  }

  return Decoration.set(ranges, true);
}

const calloutSpacingField = StateField.define<DecorationSet>({
  create: buildCalloutSpacingDecorations,
  provide: (field) => EditorView.decorations.from(field),
  update(spacing, transaction) {
    return transaction.docChanged
      ? buildCalloutSpacingDecorations(transaction.state)
      : spacing;
  },
});

export function calloutPreviewPlugin(
  options: CalloutPreviewPluginOptions = {},
) {
  const enabled = options.enabled ?? true;
  return defineMarkraPlugin({
    id: "markra.callout-preview",
    visualBlocks: enabled
      ? [{
          read(state) {
            return readCodeMirrorCallouts(state).map((callout) => ({
              from: callout.from,
              to: callout.to,
              enter(view: CodeMirrorView, direction: "backward" | "forward") {
                const contentLineFrom = direction === "forward"
                  ? callout.lineFroms[1]
                  : callout.lineFroms[callout.lineFroms.length - 1];
                if (contentLineFrom === undefined || contentLineFrom === callout.lineFroms[0]) {
                  return focusAdjacentVisualBlockBoundary(
                    view,
                    callout.from,
                    callout.to,
                    direction,
                  );
                }
                const line = view.state.doc.lineAt(contentLineFrom);
                const quote = blockquoteContent(line.text);
                const anchor = direction === "forward"
                  ? line.from + (quote?.prefix.length ?? 0)
                  : line.to;
                view.dispatch({selection: {anchor}, scrollIntoView: true});
                view.focus();
                return true;
              },
            }));
          },
        }]
      : [],
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
                  selectionChangeAffectsReveal(update) ||
                  update.focusChanged ||
                  update.viewportChanged
                ) {
                  this.decorations = buildCalloutDecorations(update.view);
                }
              }
            },
            { decorations: (plugin) => plugin.decorations },
          ),
          calloutSpacingField,
        ]
      : [],
  });
}
