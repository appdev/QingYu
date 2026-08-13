import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {
    acquireMarkdownAppearance,
    refreshMarkdownAppearance,
    resolveMarkdownAppearanceForTest,
} from "./themeResolver";

let cleanup: () => void;
const handles: Array<{release(): void}> = [];

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => `<div class="h1" data-type="NodeHeading"><div>Heading</div></div>
<div class="code-block" data-type="NodeCodeBlock"><div class="protyle-action"><span class="protyle-action__language">java</span><span class="fn__flex-1"></span><span class="protyle-icon protyle-action__copy"></span><span class="protyle-icon protyle-action__menu"></span></div><div class="hljs"><div>code</div></div></div>`,
            }),
        },
    });
});

afterEach(() => {
    handles.splice(0).forEach((handle) => handle.release());
    cleanup();
});

test("prefers SiYuan variables and probes standard Protyle selectors", () => {
    document.documentElement.style.setProperty("--b3-theme-on-background", "rgb(1, 2, 3)");
    const theme = document.createElement("style");
    theme.textContent = ".protyle-wysiwyg .h1 { color: rgb(9, 8, 7); font-size: 41px; }";
    document.head.append(theme);
    const root = document.body.appendChild(document.createElement("div"));
    root.className = "markdown-editor";

    const handle = acquireMarkdownAppearance(root);
    handles.push(handle);
    refreshMarkdownAppearance(document);

    assert.equal(
        root.style.getPropertyValue("--b3-editor-appearance-shell-document-color"),
        "rgb(1, 2, 3)",
    );
    assert.equal(
        root.style.getPropertyValue("--b3-editor-appearance-block-heading-1-color"),
        "rgb(9, 8, 7)",
    );
    assert.equal(document.querySelectorAll("[data-markdown-appearance-probe]").length, 1);
});

test("probes native code and block control components", () => {
    const theme = document.createElement("style");
    theme.textContent = `
        .protyle-wysiwyg .protyle-action__copy { color: rgb(12, 34, 56); }
        .block__icons .block__icon { color: rgb(65, 43, 21); height: 24px; width: 24px; }
    `;
    document.head.append(theme);

    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(snapshot.values["--b3-editor-appearance-control-code-copy-color"], "rgb(12, 34, 56)");
    assert.equal(snapshot.values["--b3-editor-appearance-control-table-button-color"], "rgb(65, 43, 21)");
    assert.equal(snapshot.values["--b3-editor-appearance-control-table-button-height"], "24px");
});

test("decomposes inset callout rings into continuous line-edge shadows", () => {
    const readComputedStyle = window.getComputedStyle.bind(window);
    window.getComputedStyle = ((element: Element) => readComputedStyle(element)) as typeof window.getComputedStyle;
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => "<div class=\"bq\"><div class=\"p\"><div contenteditable=\"true\">[!NOTE]\nNote callout</div></div><div class=\"protyle-attr\"></div></div>",
            }),
        },
    });
    const theme = document.createElement("style");
    theme.textContent = `.protyle-wysiwyg [data-type="NodeCallout"][data-subtype="NOTE"] {
        box-shadow: inset 0 0 0 2px rgb(12, 34, 56);
    }`;
    document.head.append(theme);

    const snapshot = resolveMarkdownAppearanceForTest(document);
    const prefix = "--b3-editor-appearance-block-callout-note-box-shadow";
    assert.equal(
        snapshot.values[`${prefix}-inline`],
        "rgb(12, 34, 56) 2px 0 0 inset, rgb(12, 34, 56) -2px 0 0 inset",
    );
    assert.equal(
        snapshot.values[`${prefix}-first`],
        "rgb(12, 34, 56) 2px 0 0 inset, rgb(12, 34, 56) -2px 0 0 inset, rgb(12, 34, 56) 0 2px 0 inset",
    );
    assert.equal(
        snapshot.values[`${prefix}-last`],
        "rgb(12, 34, 56) 2px 0 0 inset, rgb(12, 34, 56) -2px 0 0 inset, rgb(12, 34, 56) 0 -2px 0 inset",
    );
});

test("resolves theme variables from the Protyle probe scope before the application root", () => {
    document.documentElement.style.setProperty("--b3-theme-on-background", "rgb(1, 2, 3)");
    document.documentElement.style.setProperty("--b3-theme-primary", "rgb(4, 5, 6)");
    const theme = document.createElement("style");
    theme.textContent = `[data-markdown-appearance-probe] {
        --b3-theme-on-background: rgb(7, 8, 9);
        --b3-theme-primary: rgb(10, 11, 12);
    }`;
    document.head.append(theme);
    const root = document.body.appendChild(document.createElement("div"));
    root.className = "markdown-editor";

    const handle = acquireMarkdownAppearance(root);
    handles.push(handle);

    assert.equal(
        root.style.getPropertyValue("--b3-editor-appearance-shell-document-color"),
        "rgb(7, 8, 9)",
    );
    assert.equal(
        root.style.getPropertyValue("--b3-editor-appearance-editor-cursor-border-left-color"),
        "rgb(10, 11, 12)",
    );
});

test("retains the last valid snapshot when a standard variable disappears", () => {
    document.documentElement.style.setProperty("--b3-theme-on-background", "rgb(4, 5, 6)");
    const root = document.body.appendChild(document.createElement("div"));
    root.className = "markdown-editor";
    const handle = acquireMarkdownAppearance(root);
    handles.push(handle);

    document.documentElement.style.removeProperty("--b3-theme-on-background");
    refreshMarkdownAppearance(document);

    assert.equal(
        root.style.getPropertyValue("--b3-editor-appearance-shell-document-color"),
        "rgb(4, 5, 6)",
    );
});

test("shares one resolver per document and releases roots independently", () => {
    const first = document.body.appendChild(document.createElement("div"));
    const second = document.body.appendChild(document.createElement("div"));
    first.className = "markdown-editor";
    second.className = "markdown-editor";
    const firstHandle = acquireMarkdownAppearance(first);
    const secondHandle = acquireMarkdownAppearance(second);
    assert.equal(document.querySelectorAll("[data-markdown-appearance-probe]").length, 1);

    firstHandle.release();
    assert.equal(document.querySelectorAll("[data-markdown-appearance-probe]").length, 1);
    secondHandle.release();
    assert.equal(document.querySelectorAll("[data-markdown-appearance-probe]").length, 0);
});

test("can resolve one snapshot without registering an editor root", () => {
    document.documentElement.style.setProperty("--b3-theme-primary", "rgb(11, 22, 33)");
    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(snapshot.values["--b3-editor-appearance-editor-cursor-border-left-color"], "rgb(11, 22, 33)");
    assert.equal(document.querySelectorAll("[data-markdown-appearance-probe]").length, 0);
});
