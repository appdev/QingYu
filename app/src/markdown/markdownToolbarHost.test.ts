import assert = require("node:assert/strict");
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {test} from "node:test";

const source = readFileSync(resolve(process.cwd(), "src/markdown/MarkdownEditor.ts"), "utf8");
const layoutSource = readFileSync(resolve(process.cwd(), "src/layout/index.ts"), "utf8");
const wndSource = readFileSync(resolve(process.cwd(), "src/layout/Wnd.ts"), "utf8");
const dockSource = readFileSync(resolve(process.cwd(), "src/layout/dock/index.ts"), "utf8");
const outlineSource = readFileSync(resolve(process.cwd(), "src/layout/dock/Outline.ts"), "utf8");

const methodSource = (start: string, end: string) => {
    const from = source.indexOf(start);
    const to = source.indexOf(end, from);
    assert.notEqual(from, -1, start);
    assert.notEqual(to, -1, end);
    return source.slice(from, to);
};

test("Markdown shell exposes one direct mode toggle and one native more entry", () => {
    const renderShell = methodSource("    private renderShell()", "    private scheduleSourceTitleSync()");
    assert.equal(renderShell.match(/data-type="markdown-mode"/gu)?.length, 1);
    assert.match(renderShell, /data-type="markdown-more"/u);
    assert.match(renderShell,
        /dataset\.type === "markdown-more"\) \{\s*this\.openMoreMenu\(action\);\s*event\.stopPropagation\(\);\s*event\.preventDefault\(\);/u);
    for (const removed of ["markdown-source", "markdown-preview", "markdown-typewriter", "markdown-rtl", "markdown-justify"]) {
        assert.doesNotMatch(renderShell, new RegExp(`data-type="${removed}"`, "u"));
    }
});

test("Markdown outline toggles a per-document companion in the native right split", () => {
    const openOutline = methodSource("    public openOutline()", "    private async flushDirty()");
    assert.match(openOutline, /existing\.close\(\)/u);
    assert.doesNotMatch(openOutline, /existing\.element\.getBoundingClientRect\(\)/u);
    assert.match(openOutline, /split\("lr"\)/u);
    assert.match(openOutline, /fixWndFlex1\(wnd\.parent\)/u);
    assert.match(openOutline, /addTab\([\s\S]*\), false, true\);/u);
    assert.match(openOutline, /public hideOutline\(preserveState = true, isSaveLayout = false\)/u);
    assert.match(source, /private outlineOpen = false;/u);
});

test("active Markdown tabs restore their own companion and feed the pinned outline", () => {
    assert.match(wndSource, /item\.hideOutline\(true, false\)/u);
    assert.match(wndSource, /updatePanelByMarkdownEditor\(currentTab\.model\);\s*currentTab\.model\.restoreOutline\(\);/u);
    assert.match(dockSource, /markdownEditor,/u);
    assert.match(outlineSource, /public bindMarkdownEditor\(editor: RegisteredMarkdownEditor\)/u);
    assert.match(outlineSource, /buildMarkdownOutlineTreeData\(items\)/u);
    assert.match(outlineSource, /if \(wasMarkdown\) \{\s*this\.tree\.updateData\(\[\]\);/u);
});

test("native split tolerates Markdown content without a Protyle WYSIWYG node", () => {
    assert.match(layoutSource, /const wysiwygElement = element\.querySelector<HTMLElement>\("\.protyle-wysiwyg"\);/u);
    assert.match(layoutSource, /if \(wysiwygElement\) wysiwygElement\.style\.padding = "";/u);
});
