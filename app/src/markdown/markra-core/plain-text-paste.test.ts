import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {EditorView} from "@codemirror/view";
import {codeMirrorClipboardAssetsPlugin} from "./codemirror/clipboard-assets";
import {liveMarkdown} from "./codemirror";
import {
    dispatchPlainTextPaste,
    escapePlainTextMarkdown,
    markNextPlainTextPaste,
} from "./plain-text-paste";

let cleanup: () => void;
beforeEach(() => cleanup = installMarkdownTestDom());
afterEach(() => cleanup());

test("keeps Markdown-looking clipboard text literal", () => {
    assert.equal(escapePlainTextMarkdown("### title\n**bold**\n$math$"), "\\#\\#\\# title\n\\*\\*bold\\*\\*\n\\$math\\$");
});

test("inserts into a text input", () => {
    const input = document.body.appendChild(document.createElement("textarea"));
    input.value = "before after";
    input.setSelectionRange(7, 7);
    assert.equal(dispatchPlainTextPaste(input, "plain"), true);
    assert.equal(input.value, "before plainafter");
});

test("neutralizes reference, footnote, and indented-code syntax", () => {
    assert.equal(escapePlainTextMarkdown("[label]: https://example.test\n[^note]: Footnote\nSee [^note]"),
        "[label\\]\u2060: https\\://example.test\n[\u2060^note\\]\u2060: Footnote\nSee [\u2060^note\\]");
    assert.equal(escapePlainTextMarkdown("    indented"), "\u2060    indented");
});

test("keeps cross-cell selections and multiline breaks structurally intact", () => {
    const content = document.createElement("div");
    const table = document.createElement("table");
    const row = table.insertRow();
    const first = row.insertCell();
    const second = row.insertCell();
    content.className = "cm-content";
    content.setAttribute("contenteditable", "true");
    table.setAttribute("contenteditable", "true");
    first.textContent = "First";
    second.textContent = "Second";
    content.append(table);
    document.body.append(content);
    const range = document.createRange();
    range.setStart(first.firstChild, first.textContent.length);
    range.setEnd(second.firstChild, second.textContent.length);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    assert.equal(dispatchPlainTextPaste(content, "Line one\nLine two"), true);
    assert.equal(row.cells.length, 2);
    assert.equal(first.textContent, "FirstLine oneLine two");
    assert.equal(second.textContent, "Second");
    assert.ok(first.querySelector('br[data-markra-source-break="true"]'));
});

test("routes a pending native plain-text paste through the CodeMirror paste entry", () => {
    const view = new EditorView({
        doc: "",
        extensions: [liveMarkdown({plugins: [codeMirrorClipboardAssetsPlugin()]})],
        parent: document.body,
    });
    markNextPlainTextPaste(view.contentDOM, "use-native-text");
    const event = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(event, "clipboardData", {value: {
        files: [],
        getData: (type: string) => type === "text/plain" ? "### literal" : "",
        types: ["text/plain"],
    }});
    view.contentDOM.dispatchEvent(event);
    assert.equal(event.defaultPrevented, true);
    assert.equal(view.state.doc.toString(), "\\#\\#\\# literal");
    view.destroy();
});
