import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorView, minimalSetup} from "codemirror";
import {
    calloutPreviewPlugin,
    codeBlockPreviewPlugin,
    imagePreviewPlugin,
    liveMarkdown,
    mathPreviewPlugin,
    rawHtmlPreviewPlugin,
} from "../markra-core/codemirror";
import {installMarkdownTestDom} from "../markraTestDom";
import {getAppearanceContract} from "./contracts";

let cleanup: () => void;
let view: EditorView | undefined;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

for (const [id, states] of new Map<string, string[]>([
    ["block.callout-note", ["default", "expanded", "selected"]],
    ["block.callout-tip", ["default", "expanded", "selected"]],
    ["block.callout-important", ["default", "expanded", "selected"]],
    ["block.callout-warning", ["default", "expanded", "selected"]],
    ["block.callout-caution", ["default", "expanded", "selected"]],
    ["block.math", ["default", "focus", "error"]],
    ["block.mermaid", ["default", "focus", "error"]],
    ["block.raw-html", ["default", "focus", "expanded", "error"]],
    ["media.image", ["default", "hover", "selected", "drag", "error"]],
])) {
    test(`${id} declares every rendered state`, () => {
        assert.deepEqual(getAppearanceContract(id)?.states, states);
    });
}

test("Mermaid preview exposes loading, ready, and error states", async () => {
    let complete: ((svg: string) => void) | undefined;
    view = new EditorView({
        doc: "```mermaid\ngraph TD; A-->B\n```",
        extensions: [minimalSetup, liveMarkdown({plugins: [codeBlockPreviewPlugin({
            renderMermaid: () => new Promise((resolve) => complete = resolve),
        })]})],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    const preview = view.dom.querySelector<HTMLElement>(".markra-mermaid-render");
    assert.equal(preview?.dataset.appearanceState, "loading");
    complete?.("<svg></svg>");
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(preview?.dataset.appearanceState, "ready");
    const zoomButton = view.dom.querySelector<HTMLButtonElement>(".markra-mermaid-zoom-button");
    assert.ok(zoomButton);
    zoomButton.click();
    const dialog = document.querySelector<HTMLElement>(".markra-media-viewer-dialog");
    assert.ok(dialog);
    dialog.querySelector<HTMLButtonElement>(".markra-media-viewer-fullscreen-button")?.click();
    assert.equal(dialog.dataset.fullscreen, "true");
    dialog.querySelector<HTMLButtonElement>(".markra-media-viewer-close-button")?.click();
    assert.equal(document.querySelector(".markra-media-viewer-dialog"), null);

    view.destroy();
    view = new EditorView({
        doc: "```mermaid\ngraph TD; A-->B\n```",
        extensions: [minimalSetup, liveMarkdown({plugins: [codeBlockPreviewPlugin({
            renderMermaid: async () => {
                throw new Error("render failed");
            },
        })]})],
        parent: document.body,
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(view.dom.querySelector<HTMLElement>(".markra-mermaid-render")?.dataset.appearanceState, "error");
});

test("synchronous rendered blocks expose ready or error states", async () => {
    view = new EditorView({
        doc: "before\n\n$$\nx^2\n$$\n\n<div>raw html</div>\n\n> [!NOTE] Ready",
        extensions: [minimalSetup, liveMarkdown({plugins: [
            mathPreviewPlugin(),
            rawHtmlPreviewPlugin(),
            calloutPreviewPlugin(),
        ]})],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(
        view.dom.querySelector<HTMLElement>(".markra-math-render-display")?.dataset.appearanceState,
        "ready",
    );
    assert.equal(
        view.dom.querySelector<HTMLElement>(".markra-html-node")?.dataset.appearanceState,
        "ready",
    );
    assert.equal(
        view.dom.querySelector<HTMLElement>(".cm-markra-callout")?.dataset.appearanceState,
        "ready",
    );
});

test("images expose loading, ready, selected, and error states", async () => {
    view = new EditorView({
        doc: "before\n\n![preview](assets/preview.png)",
        extensions: [minimalSetup, liveMarkdown({plugins: [imagePreviewPlugin()]})],
        parent: document.body,
    });
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const root = view.dom.querySelector<HTMLElement>(".markra-image-node");
    const image = root?.querySelector("img");
    assert.equal(root?.dataset.appearanceState, "loading");
    image?.dispatchEvent(new Event("load"));
    assert.equal(root?.dataset.appearanceState, "ready");

    root?.dispatchEvent(new MouseEvent("click", {bubbles: true, cancelable: true}));
    assert.equal(root?.classList.contains("markra-image-node-selected"), true);

    image?.dispatchEvent(new Event("error"));
    assert.equal(root?.dataset.appearanceState, "error");
    assert.equal(root?.getAttribute("aria-invalid"), "true");
});
