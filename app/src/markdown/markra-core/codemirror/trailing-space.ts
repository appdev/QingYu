import {
  StateField,
  type EditorState,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  WidgetType,
  type EditorView as CodeMirrorView,
} from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin";

class TrailingSpaceWidget extends WidgetType {
  ignoreEvent() {
    return true;
  }

  toDOM(view: CodeMirrorView) {
    const space = view.dom.ownerDocument.createElement("span");
    space.className = "cm-markra-trailing-space";
    space.setAttribute("aria-hidden", "true");
    space.addEventListener("mousedown", (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      const source = view.state.doc.toString();
      const separator = source.length === 0 || source.endsWith("\n\n")
        ? ""
        : source.endsWith("\n")
          ? "\n"
          : "\n\n";
      view.dispatch({
        changes: separator
          ? { from: view.state.doc.length, insert: separator }
          : undefined,
        scrollIntoView: true,
        selection: { anchor: view.state.doc.length + separator.length },
        userEvent: separator ? "input" : undefined,
      });
      view.focus();
    });
    return space;
  }
}

function trailingDecoration(state: EditorState) {
  return Decoration.set([
    Decoration.widget({
      // This must stay a block widget: an inline writing-area widget makes the
      // browser size the final line's caret to the full 6rem click target.
      block: true,
      side: 1,
      widget: new TrailingSpaceWidget(),
    }).range(state.doc.length),
  ]);
}

// CodeMirror only accepts layout-changing block decorations from state fields.
const trailingSpaceField = StateField.define({
  create(state) {
    return trailingDecoration(state);
  },
  update(decorations, transaction) {
    return transaction.docChanged
      ? trailingDecoration(transaction.state)
      : decorations;
  },
  provide(field) {
    return EditorView.decorations.from(field);
  },
});

export function trailingSpacePlugin() {
  return defineMarkraPlugin({
    id: "markra.trailing-space",
    extension: [
      trailingSpaceField,
    ],
  });
}
