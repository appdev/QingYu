import { StateField } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import {
  readMarkdownFrontmatter,
  type MarkdownFrontmatterRange,
} from "@markra/markdown";
import { defineMarkraPlugin } from "./plugin.ts";

function buildFrontmatterHiddenDecorations(range: MarkdownFrontmatterRange) {
  return Decoration.set([
    Decoration.replace({ block: true }).range(range.from, range.to),
  ]);
}

function readFrontmatterHiddenDecorations(source: string) {
  const result = readMarkdownFrontmatter(source);
  return result.status === "valid"
    ? buildFrontmatterHiddenDecorations(result.range)
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
  provide: (field) => EditorView.decorations.from(field),
});

export function frontmatterHiddenPlugin() {
  return defineMarkraPlugin({
    id: "markra.frontmatter-hidden",
    extension: frontmatterHiddenDecorations,
  });
}
