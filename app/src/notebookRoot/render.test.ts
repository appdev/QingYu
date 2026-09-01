import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import test from "node:test";

test("notebook root layouts use fixed and stable aspect ratios", async () => {
    (globalThis as typeof globalThis & {SIYUAN_VERSION: string}).SIYUAN_VERSION = "test";
    (globalThis as typeof globalThis & {NODE_ENV: string}).NODE_ENV = "test";
    const {notebookRootCardRatio} = await import("./rules");
    assert.equal(notebookRootCardRatio("large", 1.37), 1.25);
    assert.equal(notebookRootCardRatio("masonry", 1.37), 1.37);
    assert.equal(notebookRootCardRatio("list", 1.37), 1.37);
});

test("large and masonry views use paper cards while list stays compact", async () => {
    const {notebookRootCardLayout} = await import("./rules");
    assert.equal(notebookRootCardLayout("large"), "paper");
    assert.equal(notebookRootCardLayout("masonry"), "paper");
    assert.equal(notebookRootCardLayout("list"), "list");
});

test("preview rendering ignores the display pixel ratio", async () => {
    const {
        notebookRootPreviewCanvasOptions,
        notebookRootPreviewCaptureRootStyle,
        notebookRootPreviewCaptureStyle,
        notebookRootNeedsMarkdownIdentity,
    } = await import("./rules");
    assert.deepEqual(notebookRootPreviewCanvasOptions(), {
        width: 640,
        height: 960,
        canvasWidth: 640,
        canvasHeight: 960,
        pixelRatio: 1,
    });
    assert.deepEqual(notebookRootPreviewCanvasOptions("#ffffff"), {
        width: 640,
        height: 960,
        canvasWidth: 640,
        canvasHeight: 960,
        pixelRatio: 1,
        backgroundColor: "#ffffff",
    });
    const captureStyle = notebookRootPreviewCaptureStyle("#ffffff", "#111111");
    assert.match(captureStyle, /position:relative/);
    assert.match(captureStyle, /background:#ffffff/);
    assert.doesNotMatch(captureStyle, /-10000px/);
    assert.match(notebookRootPreviewCaptureRootStyle(), /left:-10000px/);
    assert.equal(notebookRootNeedsMarkdownIdentity("markdown", "missing", false), true);
    assert.equal(notebookRootNeedsMarkdownIdentity("markdown", "valid", true), true);
    assert.equal(notebookRootNeedsMarkdownIdentity("markdown", "valid", false), false);
    assert.equal(notebookRootNeedsMarkdownIdentity("sy", "missing", false), false);
});

test("updated timestamps use deterministic localized relative units", async () => {
    const {formatNotebookRootUpdated} = await import("./rules");
    const now = 2_000_000_000_000;
    const nowSeconds = now / 1000;
    assert.equal(formatNotebookRootUpdated(0, now, "en"), "");
    assert.equal(formatNotebookRootUpdated(Number.NaN, now, "en"), "");
    assert.equal(formatNotebookRootUpdated(nowSeconds - 12, now, "en"), "12 seconds ago");
    assert.equal(formatNotebookRootUpdated(nowSeconds - 60, now, "en"), "1 minute ago");
    assert.equal(formatNotebookRootUpdated(nowSeconds - 2 * 60 * 60, now, "en"), "2 hours ago");
    assert.equal(formatNotebookRootUpdated(nowSeconds - 3 * 24 * 60 * 60, now, "en"), "3 days ago");
    assert.equal(formatNotebookRootUpdated(nowSeconds + 2 * 60 * 60, now, "en"), "in 2 hours");
});

test("preview images are not draggable while the document card owns dragging", async () => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM(`<article>
        <div class="notebook-root__paper-header">Title</div>
        <div class="notebook-root__preview-box">
            <div class="notebook-root__placeholder"></div>
            <div class="notebook-root__image-fader"></div>
        </div>
    </article>`);
    const previousDocument = globalThis.document;
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    try {
        const {installDocumentCardPreviewImage} = await import("./previewController");
        const card = dom.window.document.querySelector("article") as unknown as HTMLElement;
        assert.equal(installDocumentCardPreviewImage(card, "/preview.webp"), true);
        assert.equal(card.querySelector(".notebook-root__paper-header")?.textContent, "Title");
        const preview = card.querySelector<HTMLImageElement>(".notebook-root__preview");
        assert.ok(preview);
        assert.equal(preview.draggable, false);
        assert.equal(preview.decoding, "async");
        assert.equal(preview.getAttribute("fetchpriority"), "low");
        assert.ok(card.querySelector(".notebook-root__image-fader"));
        assert.equal(card.querySelector(".notebook-root__placeholder"), null);
        const dragSource = readFileSync(resolve(process.cwd(), "src/notebookRoot/drag.ts"), "utf8");
        assert.match(dragSource, /card\.draggable = true;/);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previousDocument});
        dom.window.close();
    }
});

test("preview session keys change with content, theme, and preview size", async () => {
    const {documentCardPreviewSessionKey} = await import("./previewController");
    const reference = {
        kind: "markdown" as const,
        notebook: "box",
        path: "/doc.md",
        id: "document-id",
        revision: "revision-a",
        updated: 100,
        sourceSize: 200,
    };
    const key = documentCardPreviewSessionKey(reference, "light", "medium");
    assert.notEqual(documentCardPreviewSessionKey({...reference, revision: "revision-b"}, "light", "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey({...reference, updated: 101}, "light", "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey({...reference, sourceSize: 201}, "light", "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey(reference, "dark", "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey(reference, "light", "small"), key);
});

test("document card previews are generated and uploaded as WebP", () => {
    const renderer = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewRenderer.ts"), "utf8");
    const controller = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewController.ts"), "utf8");
    assert.match(renderer, /"image\/webp"/);
    assert.doesNotMatch(renderer, /"image\/jpeg"/);
    assert.match(controller, /`\$\{descriptor\.cacheKey\}\.webp`/);
});

test("all notebook root views reuse the medium document preview", () => {
    const renderer = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewRenderer.ts"), "utf8");
    const controller = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewController.ts"), "utf8");
    assert.match(controller, /const size = "medium";/);
    assert.doesNotMatch(controller, /classList\.contains\("notebook-root__document--list"\).*?"small"/);
    assert.match(renderer, /size: "medium";/);
    assert.doesNotMatch(renderer, /small\.width|small\.height|drawImage\(medium/);
});

test("document card preview rendering prunes hidden content before expensive render work", () => {
    const renderer = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewRenderer.ts"), "utf8");
    const firstPrune = renderer.indexOf("pruneDocumentCardPreviewContent(content, host)");
    const processRenderIndex = renderer.indexOf("processRender(content)");
    const settle = renderer.indexOf("await settlePreviewAssets(content)");
    const secondPrune = renderer.lastIndexOf("pruneDocumentCardPreviewContent(content, host)");
    assert.ok(firstPrune > -1 && firstPrune < processRenderIndex);
    assert.ok(secondPrune > settle);
});

test("view switching binds only the toolbar buttons", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    assert.match(source, /querySelectorAll<HTMLElement>\("\.notebook-root__views \[data-view\]"\)/);
    assert.doesNotMatch(source, /querySelectorAll<HTMLElement>\("\[data-view\]"\)/);
    assert.ok(source.indexOf('viewButton("masonry", "iconLayout"') < source.indexOf('viewButton("large", "iconGallery"'));
    ["new", "sort"].forEach((action) => {
        assert.match(source, new RegExp(`data-action="${action}" data-menu="true"`));
    });
    assert.doesNotMatch(source, /data-action="more"/);
});

test("notebook titles edit inline and new notebooks default to masonry", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const viewState = readFileSync(resolve(process.cwd(), "src/notebookRoot/viewState.ts"), "utf8");
    assert.match(source, /data-action="rename" contenteditable=/);
    assert.match(source, /event\.key === "Enter"/);
    assert.match(source, /event\.key === "Escape"/);
    assert.match(source, /\/api\/notebook\/renameNotebook/);
    assert.match(viewState, /\? value as NotebookRootView : "masonry"/);
});

test("notebook cards keep one selected document across view renders", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    assert.match(source, /private selectedDocumentPath\?: string;/);
    assert.match(source, /document\.addEventListener\("pointerdown"/);
    assert.match(source, /notebook-root__document--selected/);
    assert.match(styles, /&--selected \{/);
    assert.match(styles, /&\.notebook-root__document--selected/);
});

test("list rows render escaped preview text and omit empty previews", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/render.ts"), "utf8");
    assert.match(source, /document\.previewText \? `<span class="notebook-root__document-preview-text">\$\{escapeHtml\(document\.previewText\)\}<\/span>` : ""/);
    assert.doesNotMatch(source, /\$\{document\.previewText\}/);
});

test("list rows keep a stable height while previews load", () => {
    const source = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    assert.match(source, /&--list \{[\s\S]*?grid-template-columns: minmax\(0, 1fr\) 140px 140px;[\s\S]*?height: 80px;/);
    assert.match(source, /\.notebook-root__preview-box \{\s+flex: 0 0 44px;\s+width: 44px;\s+height: 56px;/);
    assert.match(source, /\.notebook-root__placeholder \{\s+width: 100%;\s+height: 100%;\s+min-height: 0;/);
    assert.match(source, /&__document-preview-text \{[\s\S]*?text-overflow: ellipsis;[\s\S]*?white-space: nowrap;[\s\S]*?var\(--b3-theme-on-surface-light\)/);
});

test("notebook root owns scrolling and previews refresh with the theme", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    assert.match(styles, /\.notebook-root \{[\s\S]*?height: 100%;\s+overflow-y: auto;/);
    assert.doesNotMatch(styles, /&__documents \{[\s\S]*?overflow-y: auto;/);
    assert.match(source, /attributeFilter: \["data-theme-mode"\]/);
    assert.match(source, /root\.scrollTop = scrollTop/);
});

test("large preview grid rows follow card height inside the scrolling notebook root", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    assert.match(styles, /&--large \{[\s\S]*?grid-auto-rows: max-content;[\s\S]*?align-content: start;/);
});

test("preview images cannot become the drag feedback source", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/drag.ts"), "utf8");
    assert.match(styles, /&__preview \{[\s\S]*?pointer-events: none;[\s\S]*?-webkit-user-drag: none;/);
    assert.match(source, /event\.dataTransfer\.setDragImage\(card,/);
});
