import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  closeMarkraDocumentLinks,
  documentLinksPlugin,
  getMarkraDocumentLinksState,
  liveMarkdown,
  type MarkraDocumentLinkItem,
} from "./index.ts";

const items: readonly MarkraDocumentLinkItem[] = [
  {
    detail: "docs/getting-started.md",
    href: "./docs/getting-started.md",
    id: "getting-started",
    keywords: ["intro", "setup"],
    label: "Getting started",
  },
  {
    detail: "docs/plugins.md",
    href: "./docs/plugins.md",
    id: "plugins",
    keywords: ["extension", "Markra"],
    label: "Plugin authoring",
  },
];

import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView({
  doc = "Open [[",
  readOnly = false,
  selection = doc.length,
  source = () => items,
}: {
  doc?: string;
  readOnly?: boolean;
  selection?: number;
  source?: Parameters<typeof documentLinksPlugin>[0]["items"];
} = {}) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        EditorState.readOnly.of(readOnly),
        liveMarkdown({ plugins: [documentLinksPlugin({ items: source })] }),
      ],
      selection: EditorSelection.cursor(selection),
    }),
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("documentLinksPlugin", () => {
  it("opens for [[query and filters host-provided document items", () => {
    const source = vi.fn(() => items);
    const doc = "See [[markra";
    const view = createView({ doc, source });
    const menu = getMarkraDocumentLinksState(view);

    expect(source).toHaveBeenCalledWith(
      expect.objectContaining({ query: "markra", state: view.state, view }),
    );
    expect(menu).toMatchObject({
      from: 4,
      open: true,
      query: "markra",
      selectedIndex: 0,
      to: doc.length,
    });
    expect(menu.items.map((item) => item.id)).toEqual(["plugins"]);
  });

  it("navigates with the keyboard and inserts standard Markdown", () => {
    const view = createView();
    const down = new KeyboardEvent("keydown", {
      bubbles: true,
      key: "ArrowDown",
    });
    const enter = new KeyboardEvent("keydown", {
      bubbles: true,
      key: "Enter",
    });

    expect(runScopeHandlers(view, down, "editor")).toBe(true);
    expect(getMarkraDocumentLinksState(view).selectedIndex).toBe(1);
    expect(runScopeHandlers(view, enter, "editor")).toBe(true);
    expect(view.state.doc.toString()).toBe(
      "Open [Plugin authoring](./docs/plugins.md)",
    );
    expect(view.state.selection.main.head).toBe(view.state.doc.length);
    expect(getMarkraDocumentLinksState(view).open).toBe(false);
  });

  it("lets an item provide custom Markdown and run from host UI", () => {
    const view = createView({
      source: () => [
        {
          href: "markra://documents/mock",
          id: "mock",
          label: "Mock document",
          markdown: "[[documents/mock|Mock document]]",
        },
      ],
    });
    const action = getMarkraDocumentLinksState(view).items[0];

    expect(action?.run()).toBe(true);
    expect(view.state.doc.toString()).toBe(
      "Open [[documents/mock|Mock document]]",
    );
  });

  it("escapes labels and whitespace in generated Markdown links", () => {
    const view = createView({
      source: [
        {
          href: "./docs/Mock guide.md#intro",
          id: "mock-guide",
          label: "Mock [guide]",
        },
      ],
    });

    expect(getMarkraDocumentLinksState(view).items[0]?.run()).toBe(true);
    expect(view.state.doc.toString()).toBe(
      "Open [Mock \\[guide\\]](./docs/Mock%20guide.md#intro)",
    );
  });

  it("keeps an escaped completion closed until its range changes", () => {
    const view = createView();

    expect(closeMarkraDocumentLinks(view)).toBe(true);
    expect(getMarkraDocumentLinksState(view).open).toBe(false);
    view.dispatch({});
    expect(getMarkraDocumentLinksState(view).open).toBe(false);

    view.dispatch({
      changes: { from: view.state.doc.length, insert: "p" },
      selection: EditorSelection.cursor(view.state.doc.length + 1),
    });
    expect(getMarkraDocumentLinksState(view)).toMatchObject({
      open: true,
      query: "p",
    });
  });

  it("stays inactive in code blocks, read-only views, and non-empty selections", () => {
    const code = "```md\n[[\n```";
    const codeView = createView({ doc: code, selection: code.indexOf("[[") + 2 });
    const readOnlyView = createView({ readOnly: true });
    const selectedView = createView();
    selectedView.dispatch({ selection: EditorSelection.range(0, 4) });

    expect(getMarkraDocumentLinksState(codeView).open).toBe(false);
    expect(getMarkraDocumentLinksState(readOnlyView).open).toBe(false);
    expect(getMarkraDocumentLinksState(selectedView).open).toBe(false);
  });
});
