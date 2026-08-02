// @vitest-environment jsdom
import { cursorCharBackward } from "@codemirror/commands";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { frontmatterHiddenPlugin, liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [frontmatterHiddenPlugin()] })],
      selection: EditorSelection.cursor(doc.length),
    }),
  });
  view.focus();
  view.dispatch({ selection: view.state.selection });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("frontmatterHiddenPlugin", () => {
  it.each([
    {
      kind: "YAML",
      source: ["---", "title: Synthetic", "---", "# Body"].join("\n"),
    },
    {
      kind: "TOML",
      source: ["+++", 'title = "Synthetic"', "+++", "# Body"].join("\n"),
    },
    {
      kind: "JSON",
      source: ["{", '  "title": "Synthetic"', "}", "# Body"].join("\n"),
    },
  ])("replaces the complete leading $kind range without a widget", ({ source }) => {
    const view = createView(source);
    const lines = [...view.contentDOM.querySelectorAll<HTMLElement>(".cm-line")];

    expect(view.dom.querySelector(".cm-markra-frontmatter-editor")).toBeNull();
    expect(view.dom.querySelector(".cm-markra-frontmatter")).toBeNull();
    expect(lines.map((line) => line.textContent)).toEqual(["# Body"]);
    expect(view.contentDOM.textContent).toBe("# Body");
    expect(view.state.doc.toString()).toBe(source);
  });

  it("keeps metadata bytes unchanged when body input is made", () => {
    const source = ["---", "title: Synthetic", "---", "# Body"].join("\n");
    const bodyFrom = source.indexOf("# Body");
    const metadata = source.slice(0, bodyFrom);
    const view = createView(source);

    view.dispatch({
      changes: { from: view.state.doc.length, insert: "!" },
      selection: EditorSelection.cursor(view.state.doc.length + 1),
      userEvent: "input",
    });

    expect(view.state.doc.sliceString(0, bodyFrom)).toBe(metadata);
    expect(view.state.doc.toString()).toBe(`${source}!`);
  });

  it("skips the complete hidden range when moving backward from the body", () => {
    const source = ["---", "title: Synthetic", "---", "# Body"].join("\n");
    const bodyFrom = source.indexOf("# Body");
    const metadataTo = bodyFrom - 1;
    const view = createView(source);
    const positions: number[] = [];

    view.dispatch({ selection: EditorSelection.cursor(bodyFrom) });
    while (view.state.selection.main.head > 0) {
      expect(cursorCharBackward(view)).toBe(true);
      positions.push(view.state.selection.main.head);
    }

    expect(view.state.selection.main.head).toBe(0);
    expect(positions).toContain(0);
    expect(positions.every((position) => position >= metadataTo || position === 0)).toBe(true);
  });

  it("leaves malformed leading Front Matter visible", () => {
    const source = ["---", "title: [unterminated", "---", "# Body"].join("\n");
    const view = createView(source);

    expect(view.dom.querySelector(".cm-markra-frontmatter-editor")).toBeNull();
    expect(view.contentDOM.textContent).toContain("title: [unterminated");
    expect(view.contentDOM.textContent).toContain("# Body");
  });
});
