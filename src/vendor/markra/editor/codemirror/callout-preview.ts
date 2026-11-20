import { translatedLabel } from '../../../../ui-language';
import { isolateHistory } from "@codemirror/commands";
import { syntaxTree } from "@codemirror/language";
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
import { Bug, CircleCheck, CircleHelp, CircleX, ClipboardList, Info, Lightbulb, List, ListChecks, Pencil, Quote, TriangleAlert, Zap } from "lucide";
import {
  createLucideIcon,
  markdownCalloutDefinitions,
  parseMarkdownCalloutMarker,
  type MarkdownCalloutType,
  type ParsedMarkdownCalloutMarker,
} from "@markra/shared";
import { syntaxTreeChanged } from "./changes.ts";
import { defineMarkraPlugin } from "./plugin.ts";
import { cursorInsideRange, selectionChangeAffectsReveal } from "./policy.ts";

export interface CalloutPreviewPluginOptions {
  enabled?: boolean;
  showTypeMenu?: (
    anchor: HTMLButtonElement,
    current: MarkdownCalloutType,
    apply: (type: MarkdownCalloutType) => void,
    restoreFocus: () => void,
  ) => void;
}

interface CodeMirrorCallout {
  readonly from: number;
  readonly lineFroms: readonly number[];
  readonly marker: ParsedMarkdownCalloutMarker;
  readonly markerFrom: number;
  readonly markerTo: number;
  readonly hasTitle: boolean;
  readonly to: number;
}

const icons = {
  note: Pencil, abstract: ClipboardList, info: Info, todo: ListChecks, tip: Lightbulb, success: CircleCheck,
  question: CircleHelp, warning: TriangleAlert, failure: CircleX, danger: Zap, bug: Bug, example: List, quote: Quote,
};

export function createCalloutIcon(document: Document, type: MarkdownCalloutType) {
  const icon = createLucideIcon(document, icons[type], "markra-callout-icon");
  icon.dataset.calloutTone = markdownCalloutDefinitions[type].tone;
  return icon;
}

function insideCode(state: CodeMirrorState, position: number) {
  for (let node: ReturnType<typeof syntaxTree>["topNode"] | null = syntaxTree(state).resolveInner(position); node; node = node.parent) {
    if (node.name === "FencedCode" || node.name === "CodeBlock") return true;
  }
  return false;
}

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
    if (markerOffset < 0 || insideCode(state, line.from + markerOffset)) continue;
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
      hasTitle: Boolean(line.text.slice(markerOffset + marker.source.length).trim()),
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
  constructor(readonly callout: CodeMirrorCallout, readonly showTypeMenu: CalloutPreviewPluginOptions["showTypeMenu"]) {
    super();
  }

  eq(other: CalloutHeaderWidget) {
    return (
      other.callout.marker.source === this.callout.marker.source &&
      other.callout.markerFrom === this.callout.markerFrom && other.callout.hasTitle === this.callout.hasTitle
    );
  }

  ignoreEvent() {
    return true;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const header = document.createElement("span");
    header.className = "markra-callout-header";
    const { marker, markerFrom } = this.callout;
    const icon = createCalloutIcon(document, marker.type);
    const showMenu = this.showTypeMenu;
    if (showMenu && !view.state.readOnly) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "markra-callout-type-button";
      button.ariaLabel = translatedLabel('更改标注类型');
      button.setAttribute("aria-haspopup", "menu");
      button.append(icon);
      button.onmousedown = event => event.preventDefault();
      button.onclick = () => {
        const doc = view.state.doc;
        if (view.compositionStarted) return;
        const editing = view.root.activeElement === view.contentDOM;
        showMenu(button, marker.type, type => {
          if (view.state.doc !== doc || view.compositionStarted || type === marker.type) return;
          const from = markerFrom + 2;
          view.dispatch({
            changes: { from, to: from + marker.name.length, insert: type },
            annotations: isolateHistory.of("full"),
            userEvent: "input.callout",
          });
          if (editing) view.focus();
          else {
            const { node } = view.domAtPos(markerFrom);
            const element = node instanceof Element ? node : node.parentElement;
            element?.closest(".cm-line")?.querySelector<HTMLButtonElement>(".markra-callout-type-button")
              ?.focus({ preventScroll: true });
          }
        }, () => {
          if (!button.isConnected) return;
          if (editing) view.focus();
          else button.focus({ preventScroll: true });
        });
      };
      header.append(button);
    } else header.append(icon);
    if (!this.callout.hasTitle) header.append(marker.label);
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

function buildCalloutDecorations(view: CodeMirrorView, showTypeMenu: CalloutPreviewPluginOptions["showTypeMenu"]) {
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
            "data-callout-label": callout.marker.label,
            "data-callout-type": callout.marker.type,
            "data-callout-tone": callout.marker.tone,
          },
          class: `cm-markra-callout markra-callout markra-callout-${callout.marker.type} ${positionClasses}`,
        }).range(lineFrom),
      );
    }

    if (cursorInsideRange(view, callout.markerFrom, callout.markerTo)) continue;
    ranges.push(
      Decoration.replace({ widget: new CalloutHeaderWidget(callout, showTypeMenu) }).range(
        callout.markerFrom,
        callout.markerTo,
      ),
    );
    if (callout.hasTitle) ranges.push(Decoration.mark({ class: "markra-callout-title" }).range(callout.markerTo, view.state.doc.lineAt(callout.markerTo).to));
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
    return transaction.docChanged || syntaxTreeChanged(transaction.startState, transaction.state)
      ? buildCalloutSpacingDecorations(transaction.state)
      : spacing;
  },
});

const calloutTheme = EditorView.baseTheme({
  ".cm-line.cm-markra-callout": {
    background: "color-mix(in srgb, var(--callout-color, #4f7cac) 6%, transparent)",
    borderLeft: "1px solid color-mix(in srgb, var(--callout-color, #4f7cac) 24%, transparent)",
    borderRight: "1px solid color-mix(in srgb, var(--callout-color, #4f7cac) 24%, transparent)",
    paddingInline: "1em",
  },
  ".cm-line.markra-callout-first": {
    borderTopLeftRadius: "0.45em",
    borderTopRightRadius: "0.45em",
    borderTop: "1px solid color-mix(in srgb, var(--callout-color, #4f7cac) 24%, transparent)",
    paddingTop: "0.85em",
  },
  ".cm-line.markra-callout-last": {
    borderBottom: "1px solid color-mix(in srgb, var(--callout-color, #4f7cac) 24%, transparent)",
    borderBottomLeftRadius: "0.45em",
    borderBottomRightRadius: "0.45em",
    paddingBottom: "0.85em",
  },
  ".markra-callout-spacer": {
    height: "1.25em",
  },
  ".cm-line.markra-callout-active": {
    borderColor: "color-mix(in srgb, var(--callout-color, #4f7cac) 38%, transparent)",
  },
  ".markra-callout-title, .markra-callout-header": {
    color: "var(--callout-color, #4f7cac)",
    fontWeight: "650",
  },
  ".markra-callout-header": {
    alignItems: "center",
    display: "inline-flex",
    gap: "0.4em",
    verticalAlign: "middle",
  },
  ".markra-callout-type-button": {
    background: "transparent",
    border: "0",
    color: "inherit",
    font: "inherit",
    padding: "0",
    display: "grid",
    placeItems: "center",
    cursor: "pointer",
    inlineSize: "1.6em",
    blockSize: "1.6em",
  },
  ".markra-callout-icon": {
    inlineSize: "1.15em",
    blockSize: "1.15em",
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
                this.decorations = buildCalloutDecorations(view, options.showTypeMenu);
              }

              update(update: ViewUpdate) {
                if (
                  update.docChanged ||
                  syntaxTreeChanged(update.startState, update.state) ||
                  selectionChangeAffectsReveal(update) ||
                  update.focusChanged ||
                  update.viewportChanged
                ) {
                  this.decorations = buildCalloutDecorations(update.view, options.showTypeMenu);
                }
              }
            },
            { decorations: (plugin) => plugin.decorations },
          ),
          calloutSpacingField,
          calloutTheme,
        ]
      : [],
  });
}
