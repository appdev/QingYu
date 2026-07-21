import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { liveMarkdown, rawHtmlPreviewPlugin } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(
  doc: string,
  plugin = rawHtmlPreviewPlugin(),
  anchor = doc.length,
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [plugin] })],
      selection: EditorSelection.cursor(anchor),
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

describe("rawHtmlPreviewPlugin", () => {
  it("renders sanitized block HTML without changing its source", () => {
    const doc = [
      '<div class="example" onclick="alert(1)">',
      '<strong>Safe</strong><script>bad()</script>',
      "</div>",
      "",
      "Edit",
    ].join("\n");
    const view = createView(doc);
    const preview = view.dom.querySelector<HTMLElement>(".markra-html-node");

    expect(preview?.textContent).toContain("Safe");
    expect(preview?.textContent).not.toContain("bad()");
    expect(preview?.firstElementChild?.hasAttribute("onclick")).toBe(false);
    expect(preview?.querySelector("script")).toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("renders inline HTML and reveals its source when activated", () => {
    const doc = "Press <kbd>Mod</kbd> now.\n\nEdit";
    const view = createView(doc);
    const preview = view.dom.querySelector<HTMLElement>(".cm-markra-inline-html");

    expect(preview?.tagName).toBe("KBD");
    expect(preview?.textContent).toContain("Mod");
    preview?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));

    expect(view.dom.querySelector(".cm-markra-inline-html")).toBeNull();
    expect(view.dom.textContent).toContain("<kbd>Mod</kbd>");
  });

  it("resolves safe image sources and rejects executable URLs", () => {
    const resolveImageSrc = vi.fn((source: string) =>
      source === "./mock.png" ? "https://assets.example.test/mock.png" : source,
    );
    const doc = [
      '<div><img src="./mock.png" alt="Mock"><img src="javascript:alert(1)"></div>',
      "",
      "Edit",
    ].join("\n");
    const view = createView(doc, rawHtmlPreviewPlugin({ resolveImageSrc }));
    const images = view.dom.querySelectorAll<HTMLImageElement>(".markra-html-node img");

    expect(resolveImageSrc).toHaveBeenCalledWith("./mock.png");
    expect(images[0]?.getAttribute("src")).toBe("https://assets.example.test/mock.png");
    expect(images[1]?.hasAttribute("src")).toBe(false);
  });
});
