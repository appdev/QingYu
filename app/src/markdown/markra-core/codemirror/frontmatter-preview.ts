import { StateField } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import {
  readMarkdownFrontmatter,
  type MarkdownFrontmatterRange,
} from "../markdown";
import { defineMarkraPlugin } from "./plugin";

function buildFrontmatterHiddenDecorations(range: MarkdownFrontmatterRange) {
  return Decoration.set([
    Decoration.replace({ block: true }).range(range.from, range.to),
  ]);
}

export function readCodeMirrorFrontmatter(source: string) {
  const result = readMarkdownFrontmatter(source);
  return result.status === "valid" ? result.range : null;
}

function readFrontmatterHiddenDecorations(source: string) {
  const range = readCodeMirrorFrontmatter(source);
  return range
    ? buildFrontmatterHiddenDecorations(range)
    : Decoration.none;
}

const frontmatterHiddenDecorations = StateField.define<DecorationSet>({
  create(state) {
    return readFrontmatterHiddenDecorations(state.doc.toString());
  },
  update(decorations, transaction) {
    return transaction.docChanged
      ? readFrontmatterHiddenDecorations(transaction.state.doc.toString())
      : decorations;
  },
  provide: (field) => [
    EditorView.decorations.from(field),
    EditorView.atomicRanges.of((view) => view.state.field(field)),
  ],
});

export function frontmatterHiddenPlugin() {
  return defineMarkraPlugin({
    id: "markra.frontmatter-hidden",
    extension: frontmatterHiddenDecorations,
  });
}
