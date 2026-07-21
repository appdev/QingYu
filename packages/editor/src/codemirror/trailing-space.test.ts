import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { liveMarkdown, trailingSpacePlugin } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("trailingSpacePlugin", () => {
  it("focuses the source-native document end when the area below content is clicked", () => {
    const source = "| Name | Value |\n| --- | --- |\n| Alpha | 1 |";
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: source,
        extensions: [liveMarkdown({ plugins: [trailingSpacePlugin()] })],
        selection: { anchor: 0 },
      }),
    });
    views.push(view);

    const trailingSpace = view.dom.querySelector<HTMLElement>(
      ".cm-markra-trailing-space",
    );
    expect(trailingSpace).not.toBeNull();
    expect(trailingSpace?.closest(".cm-line")).toBeNull();
    trailingSpace?.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      button: 0,
      cancelable: true,
    }));

    expect(view.hasFocus).toBe(true);
    expect(view.state.selection.main.head).toBe(source.length);
    expect(view.state.doc.toString()).toBe(source);
  });
});
