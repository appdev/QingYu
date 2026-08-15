import { EditorSelection, EditorState, Prec, StateField, type Transaction } from "@codemirror/state";
import { Decoration, EditorView, keymap, WidgetType, type DecorationSet, type EditorView as CodeMirrorView } from "@codemirror/view";
import {
  readMarkdownFrontmatter,
  type MarkdownFrontmatterRange,
} from "../markdown";
import { defineMarkraPlugin } from "./plugin";

class EmptyBodyPlaceholderWidget extends WidgetType {
  constructor(readonly content: string) {
    super();
  }

  eq(other: EmptyBodyPlaceholderWidget) {
    return other.content === this.content;
  }

  toDOM(view: CodeMirrorView) {
    const placeholder = view.dom.ownerDocument.createElement("span");
    placeholder.className = "cm-placeholder";
    placeholder.style.pointerEvents = "none";
    placeholder.textContent = this.content;
    placeholder.setAttribute("aria-hidden", "true");
    return placeholder;
  }

  ignoreEvent() {
    return false;
  }
}

function visibleBodyIsEmpty(source: string, range: MarkdownFrontmatterRange) {
  const before = source.slice(0, range.from).replace(/^\ufeff/u, "");
  const after = source.slice(range.to);
  return before === "" && after !== "" && /^(?:\r?\n)+$/u.test(after);
}

function buildFrontmatterHiddenDecorations(
  state: EditorState,
  range: MarkdownFrontmatterRange,
  placeholder: string,
) {
  const emptyBody = visibleBodyIsEmpty(state.doc.toString(), range);
  const editableLine = emptyBody ? state.doc.line(state.doc.lines) : null;
  const editableSeparatorLength = editableLine && state.doc.sliceString(0, editableLine.from).endsWith("\r\n") ? 2 : 1;
  const hiddenTo = editableLine ? editableLine.from - editableSeparatorLength : range.to;
  const ranges = [
    Decoration.replace({ block: true }).range(range.from, hiddenTo),
  ];
  if (editableLine) {
    ranges.push(Decoration.line({ class: "cm-markra-empty-body-line" }).range(editableLine.from));
    if (placeholder) {
      ranges.push(Decoration.widget({
        side: 1,
        widget: new EmptyBodyPlaceholderWidget(placeholder),
      }).range(editableLine.from));
    }
  }
  return Decoration.set(ranges, true);
}

export function readCodeMirrorFrontmatter(source: string) {
  const result = readMarkdownFrontmatter(source);
  return result.status === "valid" ? result.range : null;
}

export function initialVisualMarkdownSelection(source: string) {
  const range = readCodeMirrorFrontmatter(source);
  if (!range) return 0;
  const leadingLineBreaks = /^(?:(?:\r\n)|\r|\n)+/u.exec(source.slice(range.to))?.[0] ?? "";
  return range.to + leadingLineBreaks.length;
}

function readFrontmatterHiddenDecorations(state: EditorState, placeholder: string) {
  const range = readCodeMirrorFrontmatter(state.doc.toString());
  return range
    ? buildFrontmatterHiddenDecorations(state, range, placeholder)
    : Decoration.none;
}

const createFrontmatterHiddenDecorations = (placeholder: string) => StateField.define<DecorationSet>({
  create: (state) => readFrontmatterHiddenDecorations(state, placeholder),
  update: (decorations, transaction) => transaction.docChanged
    ? readFrontmatterHiddenDecorations(transaction.state, placeholder)
    : decorations,
  provide: (field) => [
    EditorView.decorations.from(field),
    EditorView.atomicRanges.of((view) => view.state.field(field)),
  ],
});

function preserveEmptyFrontmatterBody(view: CodeMirrorView) {
  const selection = view.state.selection;
  if (selection.ranges.length !== 1 || !selection.main.empty) return false;
  const source = view.state.doc.toString();
  const range = readCodeMirrorFrontmatter(source);
  return Boolean(range && visibleBodyIsEmpty(source, range));
}

function keepSelectionOutsideInsertedFrontmatter(transaction: Transaction) {
  if (!transaction.docChanged) return transaction;
  const source = transaction.newDoc.toString();
  const range = readCodeMirrorFrontmatter(source);
  const selection = transaction.newSelection;
  if (
    !range ||
    !visibleBodyIsEmpty(source, range) ||
    selection.ranges.length !== 1 ||
    !selection.main.empty ||
    selection.main.head > range.to
  ) {
    return transaction;
  }
  return [
    transaction,
    {
      selection: EditorSelection.cursor(initialVisualMarkdownSelection(source), 1),
      sequential: true,
    },
  ];
}

export function frontmatterHiddenPlugin(placeholder = "") {
  return defineMarkraPlugin({
    id: "markra.frontmatter-hidden",
    extension: [
      EditorState.transactionFilter.of(keepSelectionOutsideInsertedFrontmatter),
      createFrontmatterHiddenDecorations(placeholder),
      Prec.high(keymap.of([
        { key: "Backspace", run: preserveEmptyFrontmatterBody },
      ])),
    ],
  });
}
