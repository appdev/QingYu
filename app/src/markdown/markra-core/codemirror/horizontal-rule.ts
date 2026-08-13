import { Decoration, WidgetType, type EditorView as CodeMirrorView } from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin";
import { markraRenderer } from "./renderers";
import { readMarkdownFrontmatter } from "../markdown";

class HorizontalRuleWidget extends WidgetType {
  constructor(readonly from: number) {
    super();
  }

  eq(other: HorizontalRuleWidget) {
    return this.from === other.from;
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const rule = view.dom.ownerDocument.createElement("hr");
    rule.className = "cm-markra-horizontal-rule";
    rule.addEventListener("mousedown", (event) => {
      if (event.button !== 0 || event.ctrlKey) return;
      event.preventDefault();
      event.stopPropagation();
      view.focus();
      view.dispatch({
        selection: { anchor: this.from },
        scrollIntoView: true,
      });
    });
    return rule;
  }
}

export function horizontalRulePlugin() {
  return defineMarkraPlugin({
    id: "markra.horizontal-rule",
    extension: [
      markraRenderer({
        id: "markra.horizontal-rule",
        nodeNames: ["HorizontalRule"],
        render(context) {
          const frontmatterResult = readMarkdownFrontmatter(
            context.state.doc.toString(),
          );
          const frontmatter = frontmatterResult.status === "valid"
            ? frontmatterResult.range
            : null;
          if (
            frontmatter &&
            context.node.from >= frontmatter.from &&
            context.node.to <= frontmatter.to
          ) {
            return true;
          }
          const line = context.state.doc.lineAt(context.node.from);
          // A freshly typed rule leaves the caret at the right boundary. Keep
          // that source editable so `****` can still become `**text**`.
          const caretAtLineEnd = context.view.hasFocus &&
            context.state.selection.ranges.some(
              (selection) => selection.empty && selection.head === line.to,
            );
          if (context.revealed("line") || caretAtLineEnd) return true;
          context.add(
            Decoration.replace({
              widget: new HorizontalRuleWidget(context.node.from),
            }).range(context.node.from, context.node.to),
          );
          return false;
        },
      }),
    ],
  });
}
