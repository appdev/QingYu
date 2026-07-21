import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  imagePreviewPlugin,
  liveMarkdown,
  resolveSafeImageSource,
} from "./index.ts";

import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(
  doc: string,
  plugin = imagePreviewPlugin(),
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [plugin] })],
      selection: { anchor: doc.length },
    }),
  });
  views.push(view);
  view.focus();
  view.dispatch({ selection: view.state.selection });
  return view;
}

function firstLine(view: EditorView) {
  return view.dom.querySelector(".cm-line")?.textContent ?? "";
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("imagePreviewPlugin", () => {
  it("renders a safe Markdown image without changing its source", () => {
    const doc =
      'Before ![Synthetic alt](https://example.test/image.png "Preview") after\n\nEdit';
    const view = createView(doc);
    const image = view.dom.querySelector<HTMLImageElement>(
      ".cm-markra-image",
    );

    expect(image).not.toBeNull();
    expect(image?.getAttribute("src")).toBe(
      "https://example.test/image.png",
    );
    expect(image?.alt).toBe("Synthetic alt");
    expect(image?.title).toBe("Preview");
    expect(image?.loading).toBe("lazy");
    expect(image?.decoding).toBe("async");
    expect(firstLine(view)).toBe("Before  after");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("reveals editable source when the selection enters the image node", () => {
    const doc = "Before ![Synthetic alt](./assets/mock.png) after\n\nEdit";
    const view = createView(doc);

    expect(view.dom.querySelector(".cm-markra-image")).not.toBeNull();
    view.dispatch({ selection: { anchor: doc.indexOf("Synthetic") + 2 } });

    expect(view.dom.querySelector(".cm-markra-image")).toBeNull();
    expect(firstLine(view)).toBe(doc.split("\n")[0]);
  });

  it("rejects executable and local protocols by default", () => {
    const executable = createView(
      "![Unsafe](javascript:alert%281%29)\n\nEdit",
    );
    const local = createView("![Local](file:///mock/private.png)\n\nEdit");

    expect(executable.dom.querySelector(".cm-markra-image")).toBeNull();
    expect(local.dom.querySelector(".cm-markra-image")).toBeNull();
  });

  it("allows common browser image sources but rejects SVG data URLs", () => {
    expect(resolveSafeImageSource("./assets/mock.png")).toBe(
      "./assets/mock.png",
    );
    expect(resolveSafeImageSource("blob:https://example.test/mock")).toBe(
      "blob:https://example.test/mock",
    );
    expect(resolveSafeImageSource("data:image/png;base64,iVBORw0KGgo=")).toBe(
      "data:image/png;base64,iVBORw0KGgo=",
    );
    expect(
      resolveSafeImageSource("data:image/svg+xml,%3Csvg%3E%3C/svg%3E"),
    ).toBeNull();
  });

  it("lets Markra resolve application assets through a host callback", () => {
    const resolveSource = vi.fn((context) =>
      context.source.startsWith("markra://")
        ? "https://assets.example.test/mock.png"
        : null,
    );
    const view = createView(
      '![Asset](markra://images/mock.png "Asset preview")\n\nEdit',
      imagePreviewPlugin({
        className: "markra-image",
        resolveSource,
      }),
    );
    const image = view.dom.querySelector<HTMLImageElement>(".markra-image");

    expect(resolveSource).toHaveBeenCalledWith(
      expect.objectContaining({
        alt: "Asset",
        source: "markra://images/mock.png",
        title: "Asset preview",
        view,
      }),
    );
    expect(image?.getAttribute("src")).toBe(
      "https://assets.example.test/mock.png",
    );
  });
});
