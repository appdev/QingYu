import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {acquireMarkdownThemeBridge, refreshMarkdownThemeBridge} from "./markdownThemeBridge";
import {installMarkdownTestDom} from "./markraTestDom";

let cleanup: () => void;
let release: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    release?.();
    release = undefined;
    cleanup();
});

test("maps SiYuan semantic theme styles to Markdown variables", () => {
    const style = document.createElement("style");
    style.textContent = `
.protyle-wysiwyg .h1 { color: rgb(12, 34, 56); font-size: 42px; font-weight: 730; }
.protyle-wysiwyg span[data-type~="strong"] { color: rgb(81, 41, 101); }
.protyle-wysiwyg .bq { background-color: rgb(21, 22, 23); border-left: 6px solid rgb(31, 32, 33); }
.protyle-wysiwyg .table th { background-color: rgb(45, 46, 47); font-weight: 680; }
`;
    document.head.append(style);

    release = acquireMarkdownThemeBridge(document);

    const rootStyle = document.documentElement.style;
    assert.equal(rootStyle.getPropertyValue("--b3-markdown-h1-color"), "rgb(12, 34, 56)");
    assert.equal(rootStyle.getPropertyValue("--b3-markdown-h1-font-size"), "42px");
    assert.equal(rootStyle.getPropertyValue("--b3-markdown-strong-color"), "rgb(81, 41, 101)");
    assert.equal(rootStyle.getPropertyValue("--b3-markdown-blockquote-border-left-width"), "6px");
    assert.equal(rootStyle.getPropertyValue("--b3-markdown-table-head-background-color"), "rgb(45, 46, 47)");
});

test("refreshes variables after a theme stylesheet changes", () => {
    const style = document.createElement("style");
    style.textContent = ".protyle-wysiwyg .h2 { color: rgb(10, 20, 30); }";
    document.head.append(style);
    release = acquireMarkdownThemeBridge(document);

    style.textContent = ".protyle-wysiwyg .h2 { color: rgb(110, 120, 130); }";
    refreshMarkdownThemeBridge(document);

    assert.equal(document.documentElement.style.getPropertyValue("--b3-markdown-h2-color"), "rgb(110, 120, 130)");
});

test("keeps one shared probe until the last Markdown editor releases it", () => {
    const firstRelease = acquireMarkdownThemeBridge(document);
    const secondRelease = acquireMarkdownThemeBridge(document);
    assert.equal(document.querySelectorAll(".markdown-theme-probe").length, 1);

    firstRelease();
    assert.equal(document.querySelectorAll(".markdown-theme-probe").length, 1);
    secondRelease();
    assert.equal(document.querySelectorAll(".markdown-theme-probe").length, 0);
});
