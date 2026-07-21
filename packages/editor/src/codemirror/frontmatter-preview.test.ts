import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { frontmatterPreviewPlugin, liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [frontmatterPreviewPlugin()] })],
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

describe("frontmatterPreviewPlugin", () => {
  it.each([
    {
      kind: "yaml",
      source: ["---", "title: Synthetic", "---", "", "# Body"].join("\n"),
    },
    {
      kind: "toml",
      source: ["+++", 'title = "Synthetic"', "+++", "", "# Body"].join("\n"),
    },
    {
      kind: "json",
      source: [
        "{",
        '  "title": "Synthetic {draft}",',
        '  "meta": { "ready": true }',
        "}",
        "",
        "# Body",
      ].join("\n"),
    },
  ])("renders leading $kind metadata without changing source", ({ kind, source }) => {
    const view = createView(source);
    const preview = view.dom.querySelector<HTMLElement>(".cm-markra-frontmatter");

    expect(preview?.dataset.frontmatterKind).toBe(kind);
    expect(preview?.textContent).toContain("Synthetic");
    expect(view.state.doc.toString()).toBe(source);
  });

  it("reveals source for editing and ignores non-leading or malformed metadata", () => {
    const source = ["---", "title: Synthetic", "---", "", "# Body"].join("\n");
    const view = createView(source);

    view.dispatch({ selection: EditorSelection.cursor(source.indexOf("Synthetic")) });
    expect(view.dom.querySelector(".cm-markra-frontmatter")).toBeNull();
    expect(view.dom.textContent).toContain("title: Synthetic");

    const malformed = createView('{"title":"Synthetic",}\n\n# Body');
    const nonLeading = createView("# Intro\n\n---\ntitle: Synthetic\n---");
    expect(malformed.dom.querySelector(".cm-markra-frontmatter")).toBeNull();
    expect(nonLeading.dom.querySelector(".cm-markra-frontmatter")).toBeNull();
  });

  it("keeps metadata rendered during a multi-line range selection", () => {
    const source = ["---", "title: Synthetic", "---", "", "# Body"].join("\n");
    const view = createView(source);

    view.dispatch({ selection: EditorSelection.range(0, source.length) });

    expect(view.dom.querySelector(".cm-markra-frontmatter")?.textContent).toContain(
      "Synthetic",
    );
  });
});
