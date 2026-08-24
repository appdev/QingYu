import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorView} from "@codemirror/view";
import {installMarkdownTestDom} from "../../markraTestDom";
import {findCodeMirrorMathRanges, liveMarkdown, mathPreviewPlugin} from "./index";

let cleanup: () => void;
let view: EditorView;
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => { view?.destroy(); cleanup(); });

test("uses one block replacement for multiline display math", () => {
    view = new EditorView({
        doc: "$$\na+b\n$$\nafter",
        extensions: [liveMarkdown({plugins: [mathPreviewPlugin()]})],
        parent: document.body,
    });
    assert.equal(view.dom.querySelectorAll(".markra-math-render-display").length, 1);
    assert.equal(view.dom.querySelectorAll(".markra-math-render-display .fn__flex-1").length, 1);
    assert.equal(view.dom.querySelector(".cm-markra-math-hidden-line"), null);
});

test("keeps a rendered node for a viewport-only transaction", () => {
    view = new EditorView({doc: "$x$", extensions: [liveMarkdown({plugins: [mathPreviewPlugin()]})], parent: document.body});
    const initial = view.dom.querySelector(".markra-math-render");
    view.dispatch({effects: EditorView.scrollIntoView(0)});
    assert.equal(view.dom.querySelector(".markra-math-render"), initial);
});

test("does not churn math decorations during IME composition", () => {
    view = new EditorView({doc: "before $x$ after", extensions: [liveMarkdown({plugins: [mathPreviewPlugin()]})], parent: document.body});
    const initial = view.dom.querySelector(".markra-math-render");
    view.contentDOM.dispatchEvent(new Event("compositionstart", {bubbles: true}));
    view.contentDOM.dispatchEvent(new FocusEvent("blur", {bubbles: true}));
    assert.equal(view.dom.querySelector(".markra-math-render"), initial);
    view.contentDOM.dispatchEvent(new Event("compositionend", {bubbles: true}));
});

test("finds large and offscreen math documents without dropping ranges", () => {
    const doc = Array.from({length: 250}, (_, index) => `paragraph ${index}\n$x_${index}$`).join("\n");
    view = new EditorView({doc, extensions: [liveMarkdown({plugins: [mathPreviewPlugin()]})], parent: document.body});
    assert.equal(findCodeMirrorMathRanges(view.state).length, 250);
    view.dispatch({selection: {anchor: doc.length}});
    assert.equal(findCodeMirrorMathRanges(view.state).at(-1)?.tex, "x_249");
});
