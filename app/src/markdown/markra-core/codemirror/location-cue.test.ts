import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorView} from "@codemirror/view";
import {installMarkdownTestDom} from "../../markraTestDom";
import {codeMirrorLocationCue, showCodeMirrorLocationCue} from "./location-cue";

let cleanup: () => void;
let view: EditorView;
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => { view?.destroy(); cleanup(); });

test("shows one transient cue and clears on selection changes", () => {
    view = new EditorView({doc: "one\ntwo", extensions: [codeMirrorLocationCue()], parent: document.body});
    showCodeMirrorLocationCue(view, 5);
    assert.equal(view.dom.querySelectorAll(".cm-markra-location-cue").length, 1);
    view.dispatch({selection: {anchor: 4}});
    assert.equal(view.dom.querySelector(".cm-markra-location-cue"), null);
});

test("ignores non-finite positions", () => {
    view = new EditorView({doc: "one", extensions: [codeMirrorLocationCue()], parent: document.body});
    showCodeMirrorLocationCue(view, Number.NaN);
    assert.equal(view.dom.querySelector(".cm-markra-location-cue"), null);
});
