import { Compartment, EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { markraLanguage } from "./index.ts";
import {
  codeMirrorSpellcheckPlugin,
  getActiveCodeMirrorSpellcheckMatch,
  getCodeMirrorSpellcheckState,
  replaceCodeMirrorSpellcheckMatch,
  updateCodeMirrorSpellcheckOptions,
} from "./spellcheck.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createView(
  doc: string,
  selection = EditorSelection.cursor(0),
  enabled = true,
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [markraLanguage, codeMirrorSpellcheckPlugin({ enabled })],
      selection,
    }),
  });
  views.push(view);
  vi.advanceTimersByTime(160);
  return view;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
  vi.useRealTimers();
});

describe("CodeMirror spellcheck", () => {
  it("decorates misspellings with source-aligned suggestions", () => {
    const view = createView("Fix teh word");

    expect(getCodeMirrorSpellcheckState(view.state).matches).toEqual([
      { from: 4, suggestions: ["the"], to: 7, word: "teh" },
    ]);
    expect(view.dom.querySelector(".cm-markra-spellcheck-error")?.textContent).toBe(
      "teh",
    );
  });

  it("rechecks the document after an async dictionary finishes loading", async () => {
    let ready = false;
    let finishLoading: (() => void) | undefined;
    const loading = new Promise<void>((resolve) => {
      finishLoading = () => {
        ready = true;
        resolve();
      };
    });
    const load = vi.fn(() => loading);
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "Fix teh word",
        extensions: [
          markraLanguage,
          codeMirrorSpellcheckPlugin({
            enabled: true,
            spellchecker: {
              check: (word) => word !== "teh",
              isReady: () => ready,
              load,
              suggest: (word) => word === "teh" ? ["the"] : [],
            },
          }),
        ],
      }),
    });
    views.push(view);

    await vi.advanceTimersByTimeAsync(160);
    expect(load).toHaveBeenCalledOnce();
    expect(getCodeMirrorSpellcheckState(view.state).matches).toEqual([]);

    finishLoading?.();
    await vi.advanceTimersByTimeAsync(160);

    expect(getCodeMirrorSpellcheckState(view.state).matches).toEqual([
      { from: 4, suggestions: ["the"], to: 7, word: "teh" },
    ]);
  });

  it("skips code and links like the existing visual editor", () => {
    const view = createView("teh `teh` [teh](https://example.test)\n\n```txt\nteh\n```");

    expect(getCodeMirrorSpellcheckState(view.state).matches.map((match) => match.from)).toEqual([
      0,
    ]);
  });

  it("updates enabled and ignored-word settings without recreating the view", () => {
    const view = createView("teh adress");

    updateCodeMirrorSpellcheckOptions(view, { ignoredWords: ["teh"] });
    vi.advanceTimersByTime(160);
    expect(getCodeMirrorSpellcheckState(view.state).matches.map((match) => match.word)).toEqual([
      "adress",
    ]);

    updateCodeMirrorSpellcheckOptions(view, { enabled: false });
    expect(getCodeMirrorSpellcheckState(view.state).matches).toEqual([]);
    expect(view.dom.querySelector(".cm-markra-spellcheck-error")).toBeNull();
  });

  it("starts checking when a compartment reconfigures the plugin from disabled to enabled", () => {
    const compartment = new Compartment();
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "Fix teh word",
        extensions: [
          markraLanguage,
          compartment.of(codeMirrorSpellcheckPlugin({ enabled: false })),
        ],
      }),
    });
    views.push(view);
    vi.advanceTimersByTime(160);
    expect(getCodeMirrorSpellcheckState(view.state).enabled).toBe(false);

    view.dispatch({
      effects: compartment.reconfigure(
        codeMirrorSpellcheckPlugin({ enabled: true }),
      ),
    });
    vi.advanceTimersByTime(160);

    expect(getCodeMirrorSpellcheckState(view.state).enabled).toBe(true);
    expect(getCodeMirrorSpellcheckState(view.state).matches).toEqual([
      { from: 4, suggestions: ["the"], to: 7, word: "teh" },
    ]);
  });

  it("finds and replaces the misspelling at the current selection", () => {
    const view = createView("Fix teh", EditorSelection.cursor(5));
    const match = getActiveCodeMirrorSpellcheckMatch(view);

    expect(match?.word).toBe("teh");
    expect(replaceCodeMirrorSpellcheckMatch(view, match, "the")).toBe(true);
    expect(view.state.doc.toString()).toBe("Fix the");
  });
});
