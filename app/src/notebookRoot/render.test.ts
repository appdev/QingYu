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

test("paper card titles reserve preview space and clamp only at complete lines", async () => {
    const {notebookRootTitleLineCount} = await import("./titleLayout");
    assert.equal(notebookRootTitleLineCount(375, 96, 24, 20), 12);
    assert.equal(notebookRootTitleLineCount(240, 96, 24, 20), 6);
    assert.equal(notebookRootTitleLineCount(120, 96, 24, 20), 1);
    assert.equal(notebookRootTitleLineCount(375, 96, 24, 0), 1);

    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const header = styles.slice(styles.indexOf("&__paper-header {"), styles.indexOf("&__preview-box {"));
    assert.doesNotMatch(header, /max-height: 45%|overflow: hidden;\s+background/);
    assert.match(header, /-webkit-line-clamp: var\(--notebook-title-lines, 2\);/);
    assert.match(styles, /&__preview-box \{[\s\S]*?min-height: 96px;/);
});

test("paper cards use the Craft Mac hierarchy without metadata", () => {
    const renderer = readFileSync(resolve(process.cwd(), "src/notebookRoot/render.ts"), "utf8");
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const paperBranch = renderer.slice(renderer.indexOf("notebook-root__paper-header"), renderer.indexOf("}).join"));
    assert.match(paperBranch, /notebook-root__document-title/);
    assert.doesNotMatch(paperBranch, /notebook-root__paper-separator/);
    assert.match(paperBranch, /notebook-root__preview-box/);
    assert.doesNotMatch(paperBranch, /notebook-root__paper-meta|notebook-root__notebook-name|notebook-root__updated/);
    assert.doesNotMatch(renderer, /notebookIcon|unicode2Emoji/);
    assert.doesNotMatch(styles, /&__paper-meta|&__notebook-icon|&__notebook-name|&__updated/);
    assert.match(renderer, /notebook-root__list-time/);
    assert.match(renderer, /notebookRootTimeGroup\(document\[groupField\]/);
});

test("paper card density follows the Craft Mac measurements", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const toolbar = styles.slice(styles.indexOf("&__toolbar {"), styles.indexOf("&__toolbar-group {"));
    const documents = styles.slice(styles.indexOf("&__documents {"), styles.indexOf("&__document {"));
    const paper = styles.slice(styles.indexOf("&__document {"), styles.indexOf("&__preview-box {"));
    assert.match(toolbar, /width: 100%;[\s\S]*?max-width: 1280px;[\s\S]*?margin-inline: auto;/);
    assert.match(documents, /width: 100%;[\s\S]*?max-width: 1280px;[\s\S]*?margin-inline: auto;[\s\S]*?padding: 28px 16px 48px;/);
    assert.match(documents, /&--large \{[\s\S]*?repeat\(auto-fill, minmax\(200px, 1fr\)\);[\s\S]*?gap: 20px;/);
    assert.match(documents, /&--masonry \{[\s\S]*?position: relative;/);
    assert.match(paper, /&--masonry \{[\s\S]*?position: absolute;[\s\S]*?margin: 0;/);
    assert.doesNotMatch(styles, /column-count|break-inside/);
    assert.doesNotMatch(styles, /repeat\([234], minmax\(0, 300px\)\)/);
    assert.doesNotMatch(styles, /notebook-root__documents--large \{\s*grid-template-columns: minmax\(0, 1fr\);/);
    assert.match(paper, /border-radius: 14px;/);
    assert.match(paper, /border: 1px solid color-mix\(in srgb, var\(--b3-border-color\) 36%, transparent\);/);
    assert.match(paper, /&:hover:not\(\.notebook-root__document--selected\)/);
    assert.match(paper, /&__paper-header > &__document-title \{[\s\S]*?font-size: 16px;[\s\S]*?line-height: 1\.25;/);
    assert.doesNotMatch(styles, /&__paper-separator/);
});

test("notebook canvas and paper cards use distinct theme layers", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const canvas = styles.slice(styles.indexOf(".notebook-root {"), styles.indexOf("&__toolbar {"));
    const toolbar = styles.slice(styles.indexOf("&__toolbar {"), styles.indexOf("&__toolbar-group {"));
    const card = styles.slice(styles.indexOf("&__document {"), styles.indexOf("&--list {", styles.indexOf("&__document {")));
    const header = styles.slice(styles.indexOf("&__paper-header {"), styles.indexOf("&__paper-header >"));
    assert.match(canvas, /background: var\(--b3-theme-surface\);/);
    assert.match(toolbar, /background: var\(--b3-theme-surface\);/);
    assert.match(card, /background: var\(--b3-theme-background\);/);
    assert.match(header, /background: var\(--b3-theme-background\);/);
});

test("preview placeholders stay transparent across themes", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const previewBox = styles.slice(styles.indexOf("&__preview-box {"), styles.indexOf("&__placeholder {"));
    const placeholder = styles.slice(styles.indexOf("&__placeholder {", styles.indexOf("&__preview-box {")), styles.indexOf("&__preview-status {"));
    const imageFader = styles.slice(styles.indexOf("&__image-fader {"), styles.indexOf("&__document-meta {"));
    assert.match(previewBox, /background: transparent;/);
    assert.match(placeholder, /background: transparent;/);
    assert.doesNotMatch(placeholder, /linear-gradient/);
    assert.doesNotMatch(imageFader, /background|linear-gradient/);
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

test("notebook root time grouping follows the active time sort and local calendar boundaries", async () => {
    const {notebookRootTimeGroup, notebookRootTimeGroupField} = await import("./rules");
    assert.equal(notebookRootTimeGroupField(2), "updated");
    assert.equal(notebookRootTimeGroupField(3), "updated");
    assert.equal(notebookRootTimeGroupField(9), "created");
    assert.equal(notebookRootTimeGroupField(10), "created");
    [0, 1, 4, 5, 6, 7, 8, 11, 12, 13].forEach((sortMode) => {
        assert.equal(notebookRootTimeGroupField(sortMode), undefined);
    });

    const labels = {
        today: "Today",
        yesterday: "Yesterday",
        past7Days: "Past 7 days",
        past30Days: "Past 30 days",
    };
    const now = new Date(2026, 8, 1, 12).getTime();
    const atLocalNoon = (daysAgo: number) => new Date(2026, 8, 1 - daysAgo, 12).getTime() / 1000;
    assert.equal(notebookRootTimeGroup(atLocalNoon(-1), now, "en", labels)?.key, "today");
    assert.equal(notebookRootTimeGroup(atLocalNoon(0), now, "en", labels)?.key, "today");
    assert.equal(notebookRootTimeGroup(atLocalNoon(1), now, "en", labels)?.key, "yesterday");
    assert.equal(notebookRootTimeGroup(atLocalNoon(2), now, "en", labels)?.key, "past-7-days");
    assert.equal(notebookRootTimeGroup(atLocalNoon(6), now, "en", labels)?.key, "past-7-days");
    assert.equal(notebookRootTimeGroup(atLocalNoon(7), now, "en", labels)?.key, "past-30-days");
    assert.equal(notebookRootTimeGroup(atLocalNoon(29), now, "en", labels)?.key, "past-30-days");
    const monthGroup = notebookRootTimeGroup(atLocalNoon(30), now, "en", labels);
    assert.match(monthGroup?.key || "", /^month-\d{4}-\d{2}$/);
    assert.equal(monthGroup?.label, new Intl.DateTimeFormat("en", {
        year: "numeric",
        month: "long",
    }).format(new Date(atLocalNoon(30) * 1000)));
    const crossYearTimestamp = new Date(2025, 11, 15, 12).getTime() / 1000;
    const crossYearGroup = notebookRootTimeGroup(crossYearTimestamp, now, "en", labels);
    assert.equal(crossYearGroup?.key, "month-2025-12");
    assert.equal(crossYearGroup?.label, "December 2025");
    assert.equal(notebookRootTimeGroup(0, now, "en", labels), undefined);
    assert.equal(notebookRootTimeGroup(Number.NaN, now, "en", labels), undefined);

    const groupKeys = [0, 1, 6, 7, 30].map((daysAgo) =>
        notebookRootTimeGroup(atLocalNoon(daysAgo), now, "en", labels)?.key,
    );
    assert.deepEqual(groupKeys, ["today", "yesterday", "past-7-days", "past-30-days", monthGroup?.key]);
    assert.deepEqual([...groupKeys].reverse(), [monthGroup?.key, "past-30-days", "past-7-days", "yesterday", "today"]);
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

test("preview session keys change with content, appearance, theme, and preview size", async () => {
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
    const appearance = "appearance-a";
    const key = documentCardPreviewSessionKey(reference, "light", appearance, "medium");
    assert.notEqual(documentCardPreviewSessionKey({...reference, revision: "revision-b"}, "light", appearance, "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey({...reference, updated: 101}, "light", appearance, "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey({...reference, sourceSize: 201}, "light", appearance, "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey(reference, "dark", appearance, "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey(reference, "light", "appearance-b", "medium"), key);
    assert.notEqual(documentCardPreviewSessionKey(reference, "light", appearance, "small"), key);
});

test("preview appearance keys include active theme assets, attributes, and semantic colors", async () => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM(`<!doctype html><html data-theme-mode="light"><head>
        <link id="themeDefaultStyle" href="/appearance/themes/daylight/theme.css?v=1">
    </head><body></body></html>`);
    const previousDocument = globalThis.document;
    const previousWindow = globalThis.window;
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    Object.defineProperty(globalThis, "window", {configurable: true, value: dom.window});
    Object.assign(dom.window, {
        siyuan: {config: {appearance: {mode: 0, themeLight: "daylight", themeDark: "midnight", themeVer: "1"}}},
    });
    dom.window.document.documentElement.style.setProperty("--b3-theme-background", "rgb(255, 255, 255)");
    try {
        const {documentCardPreviewAppearanceKey} = await import("./theme");
        const initial = await documentCardPreviewAppearanceKey();
        assert.match(initial, /^[0-9a-f]{64}$/);
        dom.window.document.documentElement.setAttribute("data-custom-theme", "savor");
        dom.window.document.documentElement.style.setProperty("--b3-theme-background", "rgb(250, 248, 240)");
        assert.notEqual(await documentCardPreviewAppearanceKey(), initial);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previousDocument});
        Object.defineProperty(globalThis, "window", {configurable: true, value: previousWindow});
        dom.window.close();
    }
});

test("document card previews are generated and uploaded as WebP", () => {
    const renderer = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewRenderer.ts"), "utf8");
    const controller = readFileSync(resolve(process.cwd(), "src/notebookRoot/previewController.ts"), "utf8");
    assert.match(renderer, /"image\/webp"/);
    assert.doesNotMatch(renderer, /"image\/jpeg"/);
    assert.match(renderer, /cardPreview: true/);
    assert.match(controller, /`\$\{descriptor\.cacheKey\}\.webp`/);
    assert.match(controller, /installImage\(job\.key, descriptor\.url, generation\)/);
    assert.doesNotMatch(controller, /URL\.createObjectURL|revokeObjectURL/);
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

test("toolbar actions and the view switcher share one geometric system", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    assert.match(styles, /&__action \{[\s\S]*?flex: 0 0 34px;[\s\S]*?width: 34px;[\s\S]*?height: 34px;/);
    assert.match(styles, /&__action \{[\s\S]*?background: var\(--b3-theme-surface\);[\s\S]*?border-radius: 8px;/);
    assert.match(styles, /&__action\[data-action="new"\] \{[\s\S]*?background: var\(--b3-theme-surface\);/);
    assert.match(styles, /&__views \{[\s\S]*?height: 34px;[\s\S]*?border-radius: 8px;/);
    assert.match(styles, /&__view--active \{[\s\S]*?background: var\(--b3-list-hover\) !important;/);
    assert.match(source, /notebook-root__action block__icon block__icon--show/);
    assert.doesNotMatch(styles, /&__action \{[\s\S]*?border-radius: 50%;/);
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

test("notebook root toolbar uses unified navigation-free controls and themed title typography", () => {
    const component = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const titleStyles = styles.slice(styles.indexOf("&__title-editable"), styles.indexOf("&__views"));
    assert.doesNotMatch(component, /data-action="back"|\bgoBack\b/);
    assert.match(component, /class="notebook-root__action block__icon block__icon--show b3-tooltips__n" data-action="new"/);
    assert.doesNotMatch(component, /notebook-root__new/);
    assert.match(titleStyles, /font-family: var\(--b3-font-family-protyle\);/);
    assert.match(titleStyles, /font-size: calc\(var\(--b3-font-size-editor\) \* 1\.25\);/);
    assert.doesNotMatch(titleStyles, /&:hover:not|box-shadow/);
});

test("notebook cards keep one selected document across view renders", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    assert.match(source, /private selectedDocumentKey\?: string;/);
    assert.match(source, /document\.addEventListener\("click"/);
    assert.match(source, /notebookRootElementKey\(document\)/);
    assert.match(source, /notebook-root__document--selected/);
    assert.match(styles, /&\.notebook-root__document--selected/);
});

test("desktop card context menus select the card without changing open gestures", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    assert.match(source, /private selectDocument\(document: HTMLElement\)/);
    assert.match(source, /document\.addEventListener\("contextmenu"/);
    assert.match(source, /event\.preventDefault\(\)/);
    assert.match(source, /event\.stopPropagation\(\)/);
    assert.match(source, /this\.selectDocument\(document\)/);
    assert.match(source, /openNotebookRootContextMenu\(\{/);
    assert.match(source, /position: \{x: event\.clientX, y: event\.clientY\}/);
    assert.match(source, /if \(!isMobile\(\)\) \{[\s\S]*?addEventListener\("contextmenu"/);
});

test("notebook cards open with desktop double click and mobile single click", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const mobileSource = readFileSync(resolve(process.cwd(), "src/mobile/markdown.ts"), "utf8");
    assert.match(source, /document\.addEventListener\(isMobile\(\) \? "click" : "dblclick", open\);/);
    assert.match(source, /if \(source && this\.openDocument\)/);
    assert.match(mobileSource, /openDocument: \(document\) =>/);
    assert.match(mobileSource, /openMobileMarkdownFile\(app, document\.notebook, document\.path, document\.title\)/);
    assert.match(mobileSource, /openMobileFileById\(app, document\.documentID\)/);
});

test("notebook root documents emit stable preview keys", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/render.ts"), "utf8");
    assert.match(source, /notebookRootDocumentKey\(\{/);
    assert.match(source, /data-preview-key="\$\{escapeAttr\(previewKey\)\}"/);
});

test("view switching replaces only the document region and keeps preview jobs alive", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const switchStart = source.indexOf("private switchView(view: NotebookRootView)");
    const updateStart = source.indexOf("private updateViewButtons", switchStart);
    const viewHandlerSource = source.slice(switchStart, updateStart);
    assert.match(viewHandlerSource, /captureNotebookRootLayoutSnapshot/);
    assert.match(viewHandlerSource, /hydrateNotebookRootLayout/);
    assert.match(viewHandlerSource, /current\.replaceWith\(next\)/);
    assert.match(viewHandlerSource, /restoreNotebookRootScrollAnchor/);
    assert.match(viewHandlerSource, /this\.createMasonryController\(next\);[\s\S]*?restoreNotebookRootScrollAnchor/);
    assert.match(viewHandlerSource, /this\.previewController\.rebind/);
    assert.doesNotMatch(viewHandlerSource, /this\.renderShell\(\)|previewController\.destroy\(\)/);
});

test("list rows render escaped preview text and omit empty previews", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/render.ts"), "utf8");
    assert.match(source, /document\.previewText \? `<span class="notebook-root__document-preview-text">\$\{escapeHtml\(document\.previewText\)\}<\/span>` : ""/);
    assert.doesNotMatch(source, /\$\{document\.previewText\}/);
});

test("time groups render only for large and list time-sorted notebook roots", () => {
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/render.ts"), "utf8");
    assert.match(source, /Pick<NotebookRootListing, "sortMode">/);
    assert.match(source, /view === "masonry" \? undefined : notebookRootTimeGroupField\(listing\.sortMode\)/);
    assert.match(source, /class="notebook-root__time-group" role="heading" aria-level="2"/);
    assert.match(source, /notebookRootTimeGroup\(document\[groupField\], nowMilliseconds, locale, groupLabels\)/);
    const headingTemplate = source.slice(source.indexOf("const timeGroupHeading"), source.indexOf("export const renderNotebookRootDocuments"));
    assert.doesNotMatch(headingTemplate, /notebook-root__document|data-path|draggable/);
});

test("time group headings span large grids and use theme tokens", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    assert.match(styles, /&__time-group \{[\s\S]*?grid-column: 1 \/ -1;/);
    assert.match(styles, /&__time-group \{[\s\S]*?color: var\(--b3-theme-on-surface-light\);/);
    const groupStyles = styles.slice(styles.indexOf("&__time-group"), styles.indexOf("&__list-header"));
    assert.doesNotMatch(groupStyles, /#[0-9a-fA-F]{3,8}|rgb\(/);
});

test("list rows keep a stable height while previews load", () => {
    const source = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    assert.match(source, /&--list \{[\s\S]*?grid-template-columns: minmax\(0, 1fr\) 140px 140px;[\s\S]*?height: 80px;/);
    assert.match(source, /\.notebook-root__preview-box \{\s+flex: 0 0 44px;\s+width: 44px;\s+height: 56px;/);
    assert.match(source, /\.notebook-root__placeholder \{\s+width: 100%;\s+height: 100%;\s+min-height: 0;/);
    assert.match(source, /&__document-preview-text \{[\s\S]*?text-overflow: ellipsis;[\s\S]*?white-space: nowrap;[\s\S]*?var\(--b3-theme-on-surface-light\)/);
});

test("notebook root keeps a fixed compact toolbar above the scrolling content", () => {
    const styles = readFileSync(resolve(process.cwd(), "src/assets/scss/business/_notebook-root.scss"), "utf8");
    const source = readFileSync(resolve(process.cwd(), "src/notebookRoot/NotebookRoot.ts"), "utf8");
    const assets = readFileSync(resolve(process.cwd(), "src/util/assets.ts"), "utf8");
    assert.match(styles, /\.notebook-root \{[\s\S]*?display: flex;[\s\S]*?height: 100%;[\s\S]*?overflow: hidden;/);
    assert.match(styles, /&__toolbar \{[\s\S]*?min-height: 56px;[\s\S]*?padding: 4px 8px;/);
    assert.match(styles, /&__content \{[\s\S]*?min-height: 0;[\s\S]*?overflow-y: auto;/);
    assert.doesNotMatch(styles, /&__documents \{[\s\S]*?overflow-y: auto;/);
    assert.match(source, /class="notebook-root__content"/);
    assert.match(source, /captureNotebookRootLayoutSnapshot\(scroller, current\)/);
    assert.match(source, /restoreNotebookRootScrollAnchor\(scroller, next, snapshot\)/);
    assert.match(source, /window\.addEventListener\("siyuan-theme-applied", this\.handleThemeApplied\)/);
    assert.match(source, /this\.previewController\?\.refreshAppearance\(\)/);
    assert.doesNotMatch(source, /attributeFilter: \["data-theme-mode"\]/);
    assert.match(assets, /window\.dispatchEvent\(new CustomEvent\("siyuan-theme-applied"/);
    assert.match(styles, /&__placeholder \{[\s\S]*?background: transparent;/);
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
