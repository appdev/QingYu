import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  linksPlugin,
  listMarkraUi,
  liveMarkdown,
  resolveSafeLinkTarget,
  runMarkraCommand,
  type LinksPluginOptions,
} from "./index.ts";

import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(
  doc: string,
  options: LinksPluginOptions,
  selection = doc.length,
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [liveMarkdown({ plugins: [linksPlugin(options)] })],
      selection: { anchor: selection },
    }),
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("linksPlugin", () => {
  it("accepts navigation-safe targets and rejects executable protocols", () => {
    expect(resolveSafeLinkTarget("https://example.test/docs")).toBe(
      "https://example.test/docs",
    );
    expect(resolveSafeLinkTarget("mailto:author@example.test")).toBe(
      "mailto:author@example.test",
    );
    expect(resolveSafeLinkTarget("tel:+15550100")).toBe("tel:+15550100");
    expect(resolveSafeLinkTarget("../guide.md#install")).toBe(
      "../guide.md#install",
    );
    expect(resolveSafeLinkTarget("#commands")).toBe("#commands");

    expect(resolveSafeLinkTarget("javascript:alert(1)")).toBeNull();
    expect(resolveSafeLinkTarget("javascript\\:alert(1)")).toBeNull();
    expect(resolveSafeLinkTarget("data:text/html,unsafe")).toBeNull();
    expect(resolveSafeLinkTarget("file:///mock/private.md")).toBeNull();
    expect(resolveSafeLinkTarget("markra://documents/mock.md")).toBeNull();
    expect(resolveSafeLinkTarget("https://example.test/\u0000unsafe")).toBeNull();
  });

  it("opens a rendered link on modifier-click without changing Markdown", () => {
    const doc =
      "Read [Synthetic guide](https://example.test/guide) now\n\nEdit here";
    const open = vi.fn();
    const view = createView(doc, { open });
    const link = view.dom.querySelector<HTMLElement>(".cm-markra-link");
    const plainPointerDown = new MouseEvent("mousedown", {
      bubbles: true,
      button: 0,
      cancelable: true,
    });
    const modifierPointerDown = new MouseEvent("mousedown", {
      bubbles: true,
      button: 0,
      cancelable: true,
      metaKey: true,
    });
    const modifierClick = new MouseEvent("click", {
      bubbles: true,
      button: 0,
      cancelable: true,
      metaKey: true,
    });

    expect(link).not.toBeNull();
    link?.dispatchEvent(plainPointerDown);
    expect(open).not.toHaveBeenCalled();

    expect(link?.dispatchEvent(modifierPointerDown)).toBe(false);
    expect(open).toHaveBeenCalledTimes(1);
    link?.dispatchEvent(modifierClick);
    expect(open).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "https://example.test/guide",
        target: "https://example.test/guide",
        view,
      }),
    );
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("publishes a stable context-menu command and keyboard shortcut", () => {
    const doc = "Read [Synthetic guide](./guide.md) now";
    const open = vi.fn();
    const view = createView(doc, { label: "打开链接", open }, doc.indexOf("guide"));

    expect(listMarkraUi(view, "context-menu")).toMatchObject([
      {
        command: "link.open",
        enabled: true,
        icon: "external-link",
        label: "打开链接",
        plugin: "markra.links",
      },
    ]);
    expect(runMarkraCommand(view, "link.open")).toBe(true);

    const shortcut = new KeyboardEvent("keydown", {
      bubbles: true,
      ctrlKey: true,
      key: "Enter",
    });
    expect(runScopeHandlers(view, shortcut, "editor")).toBe(true);
    expect(open).toHaveBeenCalledTimes(2);
  });

  it("lets Markra resolve application links through a host callback", () => {
    const doc = "Open [Mock document](markra://documents/mock.md)";
    const open = vi.fn();
    const resolveTarget = vi.fn(({ source }) =>
      source.startsWith("markra://") ? `/documents/${source.slice(9)}` : null,
    );
    const view = createView(
      doc,
      { open, resolveTarget },
      doc.indexOf("Mock document"),
    );

    expect(runMarkraCommand(view, "link.open")).toBe(true);
    expect(resolveTarget).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "markra://documents/mock.md",
        view,
      }),
    );
    expect(open).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "markra://documents/mock.md",
        target: "/documents/documents/mock.md",
        view,
      }),
    );
  });

  it("does not expose or run unsafe links when the host does not resolve them", () => {
    const doc = "Open [Unsafe](javascript:alert%281%29)";
    const open = vi.fn();
    const view = createView(doc, { open }, doc.indexOf("Unsafe"));

    expect(listMarkraUi(view, "context-menu")).toEqual([]);
    expect(runMarkraCommand(view, "link.open")).toBe(false);
    expect(open).not.toHaveBeenCalled();
  });

  it("can opt into single-click activation for read-oriented hosts", () => {
    const doc = "Read [Synthetic guide](https://example.test/guide) now\n\nEdit";
    const open = vi.fn();
    const view = createView(doc, { activation: "click", open });
    const link = view.dom.querySelector<HTMLElement>(".cm-markra-link");
    const pointerDown = new MouseEvent("mousedown", {
      bubbles: true,
      button: 0,
      cancelable: true,
    });

    expect(link?.dispatchEvent(pointerDown)).toBe(false);
    expect(open).toHaveBeenCalledTimes(1);
  });
});
