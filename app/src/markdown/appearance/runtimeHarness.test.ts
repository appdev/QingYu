import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {APPEARANCE_FIXTURE_MARKDOWN} from "./fixture";
import {
    captureApplicationAppearance,
    installMarkdownAppearanceRuntimeHarness,
} from "./runtimeHarness";
import {createTestHostAdapter} from "./testSupport";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    Object.assign(window, {
        hljs: {listLanguages: () => []},
        Lute: {New: () => ({Md2BlockDOM: () => "<div class='p'>native</div>"})},
        siyuan: {
            config: {appearance: {mode: 0, themeLight: "daylight", themeDark: "midnight"}, editor: {}},
            languages: {
                clear: "Clear",
                emptyPlaceholder: "Write something",
                search: "Search",
                uploadError: "Upload error",
                uploading: "Uploading...",
            },
        },
    });
});

afterEach(() => cleanup());

test("runtime harness mounts isolated fixtures and restores application appearance", async () => {
    const appearance = window.siyuan.config.appearance;
    const initial = captureApplicationAppearance(document, appearance);
    const harness = installMarkdownAppearanceRuntimeHarness({plugins: []} as never, {
        adapterFactory: () => createTestHostAdapter(),
        renderNative: () => undefined,
    });
    const fixture = await harness.mount({
        markdown: APPEARANCE_FIXTURE_MARKDOWN,
        mode: "visual",
        width: 500,
    });

    assert.ok(fixture.root.querySelector('[data-appearance-fixture="native"]'));
    assert.ok(fixture.root.querySelector('[data-markdown-mode="visual"]'));
    const markdownContent = fixture.markdownRoot.querySelector(".markdown-editor__content");
    const nativeContent = fixture.root.querySelector(".markdown-appearance-runtime__native .protyle-content");
    assert.ok(markdownContent?.querySelector(":scope > .markdown-editor__top"));
    assert.ok(markdownContent?.querySelector(":scope > .markdown-editor__body"));
    assert.ok(nativeContent?.querySelector(":scope > .protyle-title"));
    assert.ok(nativeContent?.querySelector(':scope > [data-appearance-fixture="native"]'));
    assert.equal(fixture.root.closest(".markdown-editor"), null);
    await fixture.destroy();
    assert.deepEqual(captureApplicationAppearance(document, appearance), initial);
});

test("runtime harness switches mode without replacing the EditorView", async () => {
    const harness = installMarkdownAppearanceRuntimeHarness({plugins: []} as never, {
        adapterFactory: () => createTestHostAdapter(),
        renderNative: () => undefined,
    });
    const fixture = await harness.mount({mode: "visual"});
    const view = fixture.view;
    const source = view.state.doc.toString();
    await harness.setMode("source");

    assert.equal(fixture.view, view);
    assert.equal(view.state.doc.toString(), source);
    assert.equal(view.dom.dataset.markdownMode, "source");
    await harness.destroy();
});

test("runtime harness measures one semantic rail for a compound blockquote", async () => {
    const markdown = `### Q04-标题空行列表复合引用

> **复合引用标题**：
>
> - 第一项包含 \`inlineCode\`
>
> - 第二项包含 **粗体**
>
> - 第三项用于验证连续轨道末端`;
    const harness = installMarkdownAppearanceRuntimeHarness({plugins: []} as never, {
        adapterFactory: () => createTestHostAdapter(),
        renderNative: () => undefined,
    });
    await harness.mount({markdown, mode: "visual"});
    const report = await harness.measureCompoundTopology(markdown.indexOf("Q04-"));

    assert.equal(report.quoteRailCount, 1);
    assert.equal(report.calloutRailCount, 0);
    assert.ok(report.quoteHostWidth >= 0);
    assert.ok(report.quoteLineWidth >= 0);
    assert.ok(report.quoteRailHeight > 0);
    assert.ok(report.quoteSourceFrom > markdown.indexOf("Q04-"));
    assert.equal(report.quoteSourceTo, markdown.length);
    assert.equal(report.roundedInternalSegmentCount, 0);
    await harness.destroy();
});

test("runtime harness scopes the real SiYuan theme without changing the application root", async () => {
    const loadedThemes: string[] = [];
    const harness = installMarkdownAppearanceRuntimeHarness({plugins: []} as never, {
        adapterFactory: () => createTestHostAdapter(),
        loadThemeCss: async (theme) => {
            loadedThemes.push(theme);
            return `:root {
            --b3-theme-background: #1e1e1e;
            --b3-theme-on-background: #dadada;
        }
        .protyle-wysiwyg .code-block { background-color: #2c2c2c; }`;
        },
        renderNative: () => undefined,
    });
    const fixture = await harness.mount({mode: "visual"});
    await harness.setTheme("savor");

    const themeStyle = document.querySelector<HTMLStyleElement>("[data-markdown-appearance-runtime-theme]");
    assert.equal(fixture.root.style.color, "var(--b3-theme-on-background)");
    assert.match(themeStyle?.textContent ?? "", /\[data-appearance-runtime="true"\][^{]*\{[^}]*--b3-theme-background:\s*#1e1e1e/);
    assert.match(themeStyle?.textContent ?? "", /\[data-appearance-runtime="true"\] \.protyle-wysiwyg \.code-block/);
    assert.doesNotMatch(themeStyle?.textContent ?? "", /(^|\})\s*:root/u);
    assert.equal(document.documentElement.style.getPropertyValue("--b3-theme-background"), "");
    assert.deepEqual(loadedThemes, ["savor"]);
    await harness.destroy();
});

test("runtime harness renders the real empty and transient editor states", async () => {
    const harness = installMarkdownAppearanceRuntimeHarness({plugins: []} as never, {
        adapterFactory: () => createTestHostAdapter(),
        renderNative: () => undefined,
    });
    const emptyFixture = await harness.mount({markdown: "", mode: "visual"});

    assert.equal(emptyFixture.markdownRoot.querySelector(".cm-placeholder")?.textContent, "Write something");
    assert.equal(emptyFixture.nativeRoot.classList.contains("protyle-wysiwyg--empty"), true);
    assert.equal(emptyFixture.nativeRoot.getAttribute("placeholder"), "Write something");
    assert.ok(harness.measure().measurements.find((item) => item.contractId === "editor.placeholder")?.markdown);

    const fixture = await harness.mount({mode: "visual"});
    await harness.interact("selected");
    assert.equal(fixture.view.state.selection.main.empty, false);
    await harness.interact("drag");
    assert.equal(fixture.markdownRoot.querySelector(".markra-block-drop-indicator")?.getAttribute("data-show"), "true");
    await harness.interact("clipboard");
    assert.equal(fixture.markdownRoot.querySelector(".markra-image-upload-placeholder")?.getAttribute("role"), "status");
    await harness.interact("error");
    assert.equal(fixture.markdownRoot.querySelector(".markdown-editor__status")?.getAttribute("data-status"), "error");
    await harness.destroy();
});
