import { history, undo } from "@codemirror/commands";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { liveMarkdown } from "./index.ts";
import { tableFragmentMergePlugin } from "./table-fragment-merge.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(doc: string, readOnly = false) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        liveMarkdown({ plugins: [tableFragmentMergePlugin()] }),
        history(),
        EditorState.readOnly.of(readOnly),
      ],
      selection: EditorSelection.cursor(doc.length),
    }),
  });
  views.push(view);
  return view;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("tableFragmentMergePlugin", () => {
  it("offers to merge compatible pipe rows without changing their source", () => {
    const doc = [
      "| Name | Value |",
      "| :--- | ---: |",
      "| Alpha | 1 |",
      "",
      "| Beta | 2 |",
      "| Gamma | 3 |",
      "",
      "After",
    ].join("\n");
    const view = createView(doc);

    expect(view.dom.querySelector('[aria-label="Merge into table above"]')).not.toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("merges a fragment into the preceding table as one undoable edit", () => {
    const doc = [
      "| Name | Value |",
      "| --- | --- |",
      "| Alpha | 1 |",
      "",
      "| Beta | 2 |",
      "",
      "After",
    ].join("\n");
    const view = createView(doc);

    view.dom.querySelector<HTMLButtonElement>('[aria-label="Merge into table above"]')?.click();

    expect(view.state.doc.toString()).toBe(doc.replace("| Alpha | 1 |\n\n| Beta", "| Alpha | 1 |\n| Beta"));
    expect(view.state.selection.main.head).toBe(view.state.doc.toString().indexOf("Beta"));
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("ignores fragments with the wrong column count or ordinary prose", () => {
    const wrongWidth = createView([
      "| Name | Value |",
      "| --- | --- |",
      "| Alpha | 1 |",
      "",
      "| Beta |",
    ].join("\n"));
    const prose = createView([
      "| Name | Value |",
      "| --- | --- |",
      "| Alpha | 1 |",
      "",
      "ordinary | prose",
    ].join("\n"));

    expect(wrongWidth.dom.querySelector(".markra-table-fragment-merge")).toBeNull();
    expect(prose.dom.querySelector(".markra-table-fragment-merge")).toBeNull();
  });

  it("does not expose a mutation control in read-only editors", () => {
    const view = createView([
      "| Name | Value |",
      "| --- | --- |",
      "| Alpha | 1 |",
      "",
      "| Beta | 2 |",
    ].join("\n"), true);

    expect(view.dom.querySelector(".markra-table-fragment-merge")).toBeNull();
  });
});
