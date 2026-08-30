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

test("reads the native horizontal rule paint from its visible pseudo-element", () => {
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => "<div class=\"hr\"><div></div></div>",
            }),
        },
    });
    const readComputedStyle = window.getComputedStyle.bind(window);
    window.getComputedStyle = ((element: Element, pseudo?: string | null) => {
        const computed = readComputedStyle(element);
        if (!element.matches(".protyle-wysiwyg .hr > div") || pseudo !== "::after") return computed;
        return new Proxy(computed, {
            get(target, property, receiver) {
                if (property === "backgroundColor") return "rgb(201, 151, 0)";
                return Reflect.get(target, property, receiver);
            },
        });
    }) as typeof window.getComputedStyle;

    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(
        snapshot.values["--b3-editor-appearance-block-horizontal-rule-background-color"],
        "rgb(201, 151, 0)",
    );
});

test("reads the native blockquote decoration without assuming that it is a rail", () => {
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => "<div class=\"bq\"><div class=\"p\">Quote</div></div>",
            }),
        },
    });
    const readComputedStyle = window.getComputedStyle.bind(window);
    window.getComputedStyle = ((element: Element, pseudo?: string | null) => {
        const computed = readComputedStyle(element);
        if (!element.matches(".protyle-wysiwyg .bq") || pseudo !== "::before") return computed;
        return new Proxy(computed, {
            get(target, property, receiver) {
                if (property === "background") return "rgb(120, 130, 140) none repeat scroll 0% 0% / auto padding-box border-box";
                if (property === "width") return "4px";
                return Reflect.get(target, property, receiver);
            },
        });
    }) as typeof window.getComputedStyle;

    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(
        snapshot.values["--b3-editor-appearance-block-blockquote-decoration-background"],
        "rgb(120, 130, 140) none repeat scroll 0% 0% / auto padding-box border-box",
    );
    assert.equal(snapshot.values["--b3-editor-appearance-block-blockquote-decoration-width"], "4px");
});

test("keeps the native blockquote host clipping with its pseudo-element paint", () => {
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => "<div class=\"bq\"><div class=\"p\">Quote</div></div>",
            }),
        },
    });
    const theme = document.createElement("style");
    theme.textContent = `.protyle-wysiwyg .bq {
        clip-path: polygon(18px 0, 100% 0, 100% 100%, 0 100%, 0 18px);
    }`;
    document.head.append(theme);

    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(
        snapshot.values["--b3-editor-appearance-block-blockquote-clip-path"],
        "polygon(18px 0, 100% 0, 100% 100%, 0 100%, 0 18px)",
    );
});

test("preserves complete textured heading paint from third-party themes", () => {
    const theme = document.createElement("style");
    theme.textContent = `.protyle-wysiwyg .h1 {
        background: linear-gradient(90deg, rgb(160, 120, 20), rgb(250, 220, 120));
        background-blend-mode: multiply;
        background-clip: text;
        -webkit-background-clip: text;
        color: transparent;
        -webkit-text-fill-color: transparent;
    }`;
    document.head.append(theme);

    const snapshot = resolveMarkdownAppearanceForTest(document);
    const prefix = "--b3-editor-appearance-block-heading-1";
    assert.match(snapshot.values[`${prefix}-background`] ?? "", /linear-gradient/u);
    assert.match(snapshot.values[`${prefix}-background-image`] ?? "", /linear-gradient/u);
    assert.equal(snapshot.values[`${prefix}-background-blend-mode`], "multiply");
    assert.equal(snapshot.values[`${prefix}-background-clip`], "text");
    assert.equal(snapshot.values[`${prefix}-webkit-text-fill-color`], "transparent");
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

test("resolves the horizontal rule texture and geometry from the native line pseudo-element", () => {
    Object.assign(window, {
        Lute: {
            New: () => ({
                Md2BlockDOM: () => "<div class=\"hr\" data-type=\"NodeThematicBreak\"><div></div></div>",
            }),
        },
    });
    const readComputedStyle = window.getComputedStyle.bind(window);
    window.getComputedStyle = ((element: Element, pseudo?: string | null) => {
        if (pseudo === "::after" && element.matches(".protyle-wysiwyg .hr > div")) {
            return {
                background: "repeating-linear-gradient(90deg, rgb(12, 34, 56), rgb(56, 34, 12) 2px)",
                backgroundColor: "rgb(12, 34, 56)",
                height: "2px",
                top: "13px",
            } as CSSStyleDeclaration;
        }
        return readComputedStyle(element);
    }) as typeof window.getComputedStyle;

    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(
        snapshot.values["--b3-editor-appearance-block-horizontal-rule-background"],
        "repeating-linear-gradient(90deg, rgb(12, 34, 56), rgb(56, 34, 12) 2px)",
    );
    assert.equal(
        snapshot.values["--b3-editor-appearance-block-horizontal-rule-background-color"],
        "rgb(12, 34, 56)",
    );
    assert.equal(snapshot.values["--b3-editor-appearance-block-horizontal-rule-line-height"], "2px");
    assert.equal(snapshot.values["--b3-editor-appearance-block-horizontal-rule-line-top"], "13px");
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

test("removes stale theme values when a standard variable disappears", () => {
    document.documentElement.style.setProperty("--b3-theme-on-background", "rgb(4, 5, 6)");
    const root = document.body.appendChild(document.createElement("div"));
    root.className = "markdown-editor";
    const handle = acquireMarkdownAppearance(root);
    handles.push(handle);

    document.documentElement.style.removeProperty("--b3-theme-on-background");
    refreshMarkdownAppearance(document);

    assert.notEqual(
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
