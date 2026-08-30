import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorState} from "@codemirror/state";
import {EditorView, minimalSetup} from "codemirror";
import {runScopeHandlers} from "@codemirror/view";
import type {MarkdownHostAdapter} from "./markra-core/adapter";
import {convertCodeMirrorClipboardHtml} from "./markra-core/codemirror";
import {createSiyuanMarkraExtension} from "./markraExtension";
import {focusVisualTableCell} from "./markra-core/codemirror/table";
import {handlePendingPlainTextPasteEvent, markNextPlainTextPaste} from "./markra-core/plain-text-paste";
import {installMarkdownTestDom} from "./markraTestDom";
import {renderMarkraMathToString} from "./markra-core/math-render";

const adapter: MarkdownHostAdapter = {
    createIcon(_name, className, ownerDocument) {
        const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
        icon.classList.add(className);
        return icon;
    },
    notifyError() {},
    openLink() {},
    positionPopover() {},
    renderMath(_source, _displayMode, context) {
        return context.ownerDocument.createElement("span");
    },
    async renderMermaid(_source, context) {
        return context.ownerDocument.createElement("div");
    },
    resolveImageSource(source) {
        return source;
    },
    async saveClipboardAssets() {
        return [];
    },
};

let cleanup: () => void;
let view: EditorView | undefined;

test("renders math once using the same HTML-only topology as the native editor", () => {
    const html = renderMarkraMathToString(String.raw`E=mc^2`, "inline");

    assert.match(html, /class="katex-html"/u);
    assert.doesNotMatch(html, /class="katex-mathml"/u);
});

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => {
    view?.destroy();
    view = undefined;
    cleanup();
});

const createView = (doc: string, mode: "source" | "visual" = "visual") => {
    view = new EditorView({
        parent: document.body,
        state: EditorState.create({
            doc,
            extensions: [
                minimalSetup,
                createSiyuanMarkraExtension({
                    adapter,
                    documentPath: () => "/test.md",
                    mode,
                }),
            ],
        }),
    });
    return view;
};

test("renders Markdown source without a line-number gutter", () => {
    const editor = createView("# Heading\n\nBody", "source");
    assert.equal(editor.dom.getAttribute("data-markdown-mode"), "source");
    assert.equal(editor.dom.querySelector(".cm-gutters"), null);
    assert.ok(editor.dom.querySelector(".cm-activeLine"));
    assert.equal(editor.state.doc.toString(), "# Heading\n\nBody");
});

test("converts copied rich tables to complete GFM Markdown", () => {
    const result = convertCodeMirrorClipboardHtml(`
<h2>文档变更记录</h2>
<table><thead><tr><th>版本</th><th>日期</th><th>作者</th><th>变更</th></tr></thead>
<tbody><tr><td>v1.0</td><td>2026-04-20</td><td>架构组</td><td><strong>初版</strong>：建立端到端框架</td></tr></tbody></table>`);
    assert.ok(result);
    assert.equal(result.structured, true);
    assert.match(result.markdown, /^## 文档变更记录/mu);
    assert.match(result.markdown, /\| 版本 \| 日期 \| 作者 \| 变更 \|/u);
    assert.match(result.markdown, /\| --- \| --- \| --- \| --- \|/u);
    assert.match(result.markdown, /\*\*初版\*\*/u);
});

test("renders tables through the Markra visual core without changing source", async () => {
    const source = "引言\n\n| 版本 | 日期 | 作者 | 变更 |\n| --- | --- | --- | --- |\n| v1.0 | 2026-04-20 | 架构组 | 初版 |";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.state.doc.toString(), source);
    assert.equal(editor.dom.getAttribute("data-markdown-mode"), "visual");
    assert.equal(editor.dom.querySelectorAll(".cm-gutter").length, 0);
    assert.ok(editor.dom.querySelector("table"));
    assert.equal(editor.dom.querySelectorAll("tbody tr").length, 1);
});

test("renders escaped table pipes and themed links with native semantics", async () => {
    const source = "| 语法 | 链接 |\n| --- | --- |\n| `a \\| b` | [示例](https://example.com) |";
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const code = editor.dom.querySelector<HTMLElement>(".cm-markra-table code");
    const link = editor.dom.querySelector<HTMLAnchorElement>(".cm-markra-table a");
    assert.equal(code?.textContent, "a | b");
    assert.equal(code?.dataset.markraCodeMarkdown, "`a \\| b`");
    assert.ok(link?.classList.contains("cm-markra-link"));
    assert.equal(editor.state.doc.toString(), source);
});

test("moves down from a heading into the visual table without breaking its source", async () => {
    Object.assign(globalThis, {
        HTMLTableCellElement: window.HTMLTableCellElement,
        InputEvent: window.InputEvent,
        NodeFilter: window.NodeFilter,
    });
    const source = "## 推送通知端到端技术方案\n\n| 项目 | 内容 |\n| --- | --- |\n| 文档版本 | v1.7 |";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("端到端")}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const handled = runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor");
    await Promise.resolve();

    assert.equal(handled, true);
    const selectionNode = document.getSelection()?.anchorNode;
    const selectionElement = selectionNode instanceof Element ? selectionNode : selectionNode?.parentElement;
    const cell = selectionElement?.closest<HTMLTableCellElement>("th, td");
    assert.equal(cell?.dataset.tableHeader, "true");

    cell.textContent = `X${cell.textContent}`;
    cell.dispatchEvent(new InputEvent("input", {bubbles: true, data: "X", inputType: "insertText"}));
    await Promise.resolve();

    assert.match(editor.state.doc.toString(), /\| X项目 \| 内容 \|/u);
    assert.ok(editor.dom.querySelector(".cm-markra-table"));
});

test("moves up from following text into the last visual table row", async () => {
    Object.assign(globalThis, {
        HTMLTableCellElement: window.HTMLTableCellElement,
        InputEvent: window.InputEvent,
        NodeFilter: window.NodeFilter,
    });
    const source = "| 项目 | 内容 |\n| --- | --- |\n| 文档版本 | v1.7 |\n\n下方正文";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("下方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const handled = runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowUp",
    }), "editor");
    await Promise.resolve();

    assert.equal(handled, true);
    const selectionNode = document.getSelection()?.anchorNode;
    const selectionElement = selectionNode instanceof Element ? selectionNode : selectionNode?.parentElement;
    const cell = selectionElement?.closest<HTMLTableCellElement>("th, td");
    assert.equal(cell?.dataset.tableRow, "0");
    assert.equal(cell?.dataset.tableHeader, "false");
    assert.equal(editor.state.doc.toString(), source);
});

test("moves through display math without editing the following heading", async () => {
    const source = "### M03-美元块公式\n\n$$\nx = 1\n$$\n\n### M04-Hugo 块公式与宏";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("\n")}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor"), true);
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.equal(editor.state.doc.toString(), "### M03-美元块公式\n\n$$\n新增x = 1\n$$\n\n### M04-Hugo 块公式与宏");
});

test("moves upward into display math without editing the preceding heading", async () => {
    const source = "### 上方标题\n\n$$\nx = 1\n$$\n\n下方正文";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("下方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowUp",
    }), "editor"), true);
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.equal(editor.state.doc.toString(), "### 上方标题\n\n$$\nx = 1新增\n$$\n\n下方正文");
});

test("moves into Mermaid source without crossing its fenced boundary", async () => {
    const source = "上方正文\n\n```mermaid\ngraph TD\nA --> B\n```\n\n### 下方标题";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("上方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor"), true);
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.equal(editor.state.doc.toString(), "上方正文\n\n```mermaid\n新增graph TD\nA --> B\n```\n\n### 下方标题");
});

test("moves into block HTML content without changing its tags or following heading", async () => {
    const source = "上方正文\n\n<details>\n<summary>标题</summary>\n正文\n</details>\n\n### 下方标题";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("上方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor"), true);
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.equal(editor.state.doc.toString(), "上方正文\n\n<details>\n新增<summary>标题</summary>\n正文\n</details>\n\n### 下方标题");
});

test("keeps ordinary fenced code navigation inside the code content", async () => {
    const source = "上方正文\n\n```ts\nconst value = true;\n```\n\n### 下方标题";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("上方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor");
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.match(editor.state.doc.toString(), /^上方正文\n\n```ts\n.*新增.*\n```\n\n### 下方标题$/su);
});

test("keeps an image block and following heading intact after downward input", async () => {
    const source = "上方正文\n\n![示例](image.png)\n\n### 下方标题";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("上方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor");
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.match(editor.state.doc.toString(), /\n!\[示例\]\(image\.png\)\n/u);
    assert.match(editor.state.doc.toString(), /\n### 下方标题$/u);
});

test("keeps a horizontal rule and following heading intact after downward input", async () => {
    const source = "上方正文\n\n---\n\n### 下方标题";
    const editor = createView(source);
    editor.focus();
    editor.dispatch({selection: {anchor: source.indexOf("上方正文") + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "ArrowDown",
    }), "editor");
    editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

    assert.match(editor.state.doc.toString(), /\n---\n/u);
    assert.match(editor.state.doc.toString(), /\n### 下方标题$/u);
});

test("keeps quote, list, and callout markers intact during vertical input", async () => {
    const cases = [
        {marker: "> ", source: "上方正文\n\n> 引用内容\n\n### 下方标题"},
        {marker: "- ", source: "上方正文\n\n- 列表内容\n\n### 下方标题"},
        {marker: "> [!NOTE]", source: "上方正文\n\n> [!NOTE]\n> Callout 内容\n\n### 下方标题"},
    ];

    for (const entry of cases) {
        const editor = createView(entry.source);
        editor.focus();
        editor.dispatch({selection: {anchor: entry.source.indexOf("上方正文") + 2}});
        await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
        runScopeHandlers(editor, new KeyboardEvent("keydown", {
            key: "ArrowDown",
        }), "editor");
        editor.dispatch({changes: {from: editor.state.selection.main.head, insert: "新增"}});

        assert.ok(editor.state.doc.toString().includes(entry.marker));
        assert.match(editor.state.doc.toString(), /\n### 下方标题$/u);
        editor.destroy();
        view = undefined;
    }
});

test("moves visual table focus on Tab without inserting indentation", async () => {
    const source = "| Alpha | Beta |\n| --- | --- |\n| one | two |";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    Object.assign(globalThis, {
        HTMLTableCellElement: window.HTMLTableCellElement,
        InputEvent: window.InputEvent,
        NodeFilter: window.NodeFilter,
    });
    focusVisualTableCell(editor, 0, -1, 0, true, 0);
    await Promise.resolve();
    const table = editor.dom.querySelector("table");
    const tab = new KeyboardEvent("keydown", {bubbles: true, cancelable: true, key: "Tab"});
    table.dispatchEvent(tab);
    assert.equal(tab.defaultPrevented, true);
    const activeCell = document.getSelection()?.anchorNode?.parentElement?.closest("th, td");
    assert.equal(activeCell?.getAttribute("data-table-column"), "1");
    assert.equal(editor.state.doc.toString(), source);
});

test("keeps a cross-cell plain-text paste inside the starting visual table cell", async () => {
    Object.assign(globalThis, {
        HTMLTableCellElement: window.HTMLTableCellElement,
        InputEvent: window.InputEvent,
        NodeFilter: window.NodeFilter,
    });
    const editor = createView("| Alpha | Beta |\n| --- | --- |\n| one | two |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    const cells = editor.dom.querySelectorAll<HTMLTableCellElement>("tbody td");
    const selection = document.getSelection();
    const range = document.createRange();
    range.setStart(cells[0].firstChild, 1);
    range.setEnd(cells[1].firstChild, 2);
    selection.removeAllRanges();
    selection.addRange(range);
    markNextPlainTextPaste(editor.contentDOM, "use-native-text");
    const paste = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(paste, "clipboardData", {value: {getData: () => "# literal"}});
    Object.defineProperty(paste, "target", {value: cells[0]});
    assert.equal(handlePendingPlainTextPasteEvent(paste as ClipboardEvent, editor.contentDOM), true);
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(paste.defaultPrevented, true);
    assert.match(editor.state.doc.toString(), /\| o\\# literalne \| two \|/u);
});

test("gives only the selected authored blank line an active empty-line row", async () => {
    const source = "# 目标\n\n身高：175 cm";
    const editor = createView(source);
    const blankLinePosition = source.indexOf("\n") + 1;

    editor.focus();
    editor.dispatch({selection: {anchor: blankLinePosition}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const emptyLine = editor.dom.querySelector(".cm-markra-empty-line");
    assert.ok(emptyLine);
    assert.equal(emptyLine.classList.contains("cm-markra-active-empty-line"), true);

    editor.dispatch({selection: {anchor: source.length}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.dom.querySelector(".cm-markra-active-empty-line"), null);
});

test("replaces an empty heading marker when pasting structured Markdown", () => {
    const editor = createView("###### ");
    editor.dispatch({selection: {anchor: editor.state.doc.length}});
    const event = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(event, "clipboardData", {
        value: {
            files: [],
            getData(type: string) {
                if (type === "text/html") return "<h2>目标</h2><ul><li>身高：175 cm</li></ul>";
                return type === "text/plain" ? "## 目标\n\n- 身高：175 cm" : "";
            },
        },
    });

    editor.contentDOM.dispatchEvent(event);

    assert.equal(editor.state.doc.toString(), "## 目标\n\n-   身高：175 cm");
});

test("replaces an empty heading marker when pasting plain Markdown", () => {
    const editor = createView("###### ");
    editor.dispatch({selection: {anchor: editor.state.doc.length}});
    const event = new Event("paste", {bubbles: true, cancelable: true});
    Object.defineProperty(event, "clipboardData", {
        value: {
            files: [],
            getData(type: string) {
                return type === "text/plain" ? "# 减脂要点\n\n## 目标" : "";
            },
        },
    });

    editor.contentDOM.dispatchEvent(event);

    assert.equal(editor.state.doc.toString(), "# 减脂要点\n\n## 目标");
});

test("labels the heading-level control with the active heading level", async () => {
    const editor = createView("###### 目标");
    editor.focus();
    editor.dispatch({selection: {anchor: editor.state.doc.length}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const button = editor.dom.querySelector<HTMLButtonElement>(".markra-heading-level-button");
    assert.ok(button);
    assert.equal(button.textContent, "H6");
});

test("keeps following text outside an unfinished code fence until Enter closes it", async () => {
    const editor = createView("```\n下方正文");
    editor.dispatch({selection: {anchor: 3}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.dom.querySelector(".cm-markra-code-header"), null);
    assert.equal(editor.dom.querySelector(".cm-markra-code-content-line"), null);
    assert.equal(editor.dom.querySelector(".cm-markra-code-top-gap"), null);

    const handled = runScopeHandlers(editor, new KeyboardEvent("keydown", {
        key: "Enter",
    }), "editor");
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(handled, true);
    assert.equal(editor.state.doc.toString(), "```\n\n```\n下方正文");
    assert.ok(editor.dom.querySelector(".cm-markra-code-header"));
    assert.equal(editor.dom.querySelector(".cm-markra-code-content-line")?.textContent, "");
});

test("does not preview an unfinished Mermaid fence over following text", async () => {
    const source = "```mermaid\ngraph TD\n下方正文";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: "```mermaid".length}});
    editor.contentDOM.dispatchEvent(new Event("blur"));
    await Promise.resolve();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.state.doc.toString(), source);
    assert.equal(editor.dom.querySelector(".markra-mermaid-render"), null);
});

test("adds safe SiYuan semantic aliases to rendered Markdown", async () => {
    const editor = createView("# 标题\n\n> 引用\n\n**粗体** *斜体* ~~删除~~ ==高亮== `代码`");
    editor.dispatch({selection: {anchor: editor.state.doc.length}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.ok(editor.dom.querySelector(".cm-markra-h1.h1"));
    assert.ok(editor.dom.querySelector(".cm-markra-blockquote.cm-markra-blockquote-first.cm-markra-blockquote-last"));
    assert.equal(editor.dom.querySelector(".cm-markra-blockquote.bq"), null);
    assert.ok(editor.dom.querySelector('.cm-markra-strong[data-type~="strong"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-emphasis[data-type~="em"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-strikethrough[data-type~="s"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-highlight[data-type~="mark"]'));
    assert.ok(editor.dom.querySelector('.cm-markra-inline-code[data-type~="code"]'));
    assert.equal(editor.dom.querySelector("[data-node-id]"), null);
});

test("maps native list roles through nested blockquotes without changing Markdown", async () => {
    const source = `- 顶层
  1. 嵌套
- [x] 已完成

> **引用**
>
> - 无序
>
> 1. 有序
>
> - [ ] 任务`;
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const listLines = Array.from(editor.dom.querySelectorAll<HTMLElement>(".cm-markra-list-item"));
    assert.equal(listLines.length, 6);
    assert.equal(editor.dom.querySelectorAll(".cm-markra-blockquote.cm-markra-list-item").length, 3);
    assert.equal(editor.dom.querySelectorAll(".cm-markra-list-marker--bullet").length, 2);
    assert.equal(editor.dom.querySelectorAll(".cm-markra-list-marker--ordered").length, 2);
    assert.equal(editor.dom.querySelector(".cm-markra-list-marker--ordered")?.textContent, "1.");
    assert.equal(editor.dom.querySelectorAll(".cm-markra-task-checkbox").length, 2);
    assert.equal(editor.dom.querySelectorAll(".cm-markra-task-done").length, 1);
    assert.equal(listLines[1].style.getPropertyValue("--markra-list-indent"), "34px");
    assert.equal(editor.state.doc.toString(), source);
});

test("draws one structural decoration for a compound blockquote", async () => {
    const source = `> **复合引用标题**：
>
> - 第一项包含 \`inlineCode\`
>
> - 第二项包含 **粗体**
>
> - 第三项用于验证连续轨道末端`;
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const rails = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-blockquote-decoration");
    assert.equal(rails.length, 1);
    assert.equal(rails[0].dataset.from, "0");
    assert.equal(rails[0].dataset.to, String(source.length));
    const firstDOM = editor.domAtPos(0).node;
    const firstLine = (firstDOM instanceof HTMLElement ? firstDOM : firstDOM.parentElement)
        ?.closest<HTMLElement>(".cm-line");
    assert.ok(firstLine);
    assert.equal(
        Number.parseFloat(rails[0].style.getPropertyValue("--markra-blockquote-span-top")),
        (firstLine.getBoundingClientRect().top - editor.documentTop) / editor.scaleY,
    );
    assert.equal(editor.state.doc.toString(), source);
});

test("draws nested quote rails while keeping callouts isolated", async () => {
    const nestedSource = `> 外层引用开始
>
> > 内层引用
> >
> > - 内层列表
>
> 外层引用结束`;
    let editor = createView(nestedSource);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const nestedRails = Array.from(editor.dom.querySelectorAll<HTMLElement>(".cm-markra-blockquote-decoration"));
    assert.equal(nestedRails.length, 2);
    assert.deepEqual(nestedRails.map((rail) => rail.style.getPropertyValue("--markra-blockquote-depth")), ["0", "1"]);
    const nestedLine = Array.from(editor.dom.querySelectorAll<HTMLElement>(".cm-markra-blockquote"))
        .find((line) => line.textContent === "内层引用");
    assert.equal(nestedLine?.dataset.blockquoteDepth, "1");
    assert.equal(nestedLine?.dataset.blockquoteStartCount, "1");
    assert.equal(nestedLine?.style.getPropertyValue("--markra-blockquote-depth"), "1");
    assert.equal(editor.state.doc.toString(), nestedSource);

    editor.destroy();
    const calloutSource = "> [!NOTE]\n> Callout 内容";
    editor = createView(calloutSource);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.equal(editor.dom.querySelectorAll(".cm-markra-blockquote-decoration").length, 0);
    assert.ok(editor.dom.querySelector(".cm-markra-callout"));
    assert.equal(editor.state.doc.toString(), calloutSource);
});

test("keeps unsupported Setext syntax literal while collapsing structural quote lines", async () => {
    const source = `标题
===

> 引用
>
> - 列表`;
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const sourceLines = source.split("\n");
    const renderedLines = Array.from(editor.dom.querySelectorAll<HTMLElement>(".cm-line"));
    const titleLine = renderedLines[sourceLines.indexOf("标题")];
    const setextLine = renderedLines[sourceLines.indexOf("===")];
    const quoteLine = renderedLines[sourceLines.indexOf(">")];
    assert.equal(titleLine.classList.contains("cm-markra-h1"), false);
    assert.equal(setextLine.classList.contains("cm-markra-setext-marker-line"), false);
    assert.equal(setextLine.classList.contains("cm-markra-structural-line"), false);
    assert.ok(quoteLine.classList.contains("cm-markra-structural-line"));
    assert.equal(quoteLine.classList.contains("cm-markra-active-structural-line"), false);

    editor.focus();
    editor.dispatch({selection: {anchor: editor.state.doc.line(sourceLines.indexOf(">") + 1).from}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    assert.ok(renderedLines[sourceLines.indexOf(">")].classList.contains("cm-markra-active-structural-line"));
    assert.equal(editor.state.doc.toString(), source);
});

test("renders valid indented code blocks with the native code surface", async () => {
    const source = "    function indented() {\n        return true;\n    }";
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const lines = editor.dom.querySelectorAll(".cm-markra-indented-code-line");
    assert.equal(lines.length, 3);
    assert.equal(lines[0].classList.contains("cm-markra-code-content-first"), true);
    assert.equal(lines[2].classList.contains("cm-markra-code-content-last"), true);
    assert.equal(editor.dom.querySelector(".cm-markra-code-actions"), null);
    assert.equal(editor.state.doc.toString(), source);
});

test("shows Markdown list source instead of duplicating the native marker on the active line", async () => {
    const source = "- 可编辑列表";
    const editor = createView(source);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    assert.ok(editor.dom.querySelector(".cm-markra-list-marker--bullet"));

    editor.focus();
    editor.dispatch({selection: {anchor: 1}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const line = editor.dom.querySelector<HTMLElement>(".cm-markra-list-item");
    assert.equal(line?.dataset.markraListSource, "visible");
    assert.equal(line?.querySelector(".cm-markra-list-marker"), null);
    assert.ok(line?.textContent?.includes("- 可编辑列表"));
});

test("reveals heading source at the clicked line while preserving selectable text", async () => {
    const editor = createView("## 可复制标题\n\n正文");
    editor.focus();
    editor.dispatch({selection: {anchor: 5}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const heading = editor.dom.querySelector(".cm-markra-h2");
    assert.ok(heading?.textContent?.endsWith("## 可复制标题"));

    editor.dispatch({selection: {anchor: 3, head: 8}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.ok(heading?.textContent?.endsWith("可复制标题"));
    assert.equal(editor.state.sliceDoc(
        editor.state.selection.main.from,
        editor.state.selection.main.to,
    ), "可复制标题");
});

test("renders horizontal rules as semantic containers with a stable paint surface", async () => {
    const editor = createView("上文\n\n---\n\n下文");
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const rule = editor.dom.querySelector<HTMLElement>(".cm-markra-horizontal-rule");
    assert.equal(rule?.tagName, "HR");
    assert.equal(rule?.childElementCount, 0);
    assert.ok(rule?.parentElement?.classList.contains("cm-markra-horizontal-rule-line"));
});

test("keeps safe details interactive without losing HTML source editing", async () => {
    const editor = createView("前文\n\n<details>\n<summary>可展开的安全 HTML</summary>\n<p>块级 HTML 内容。</p>\n</details>");
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const summary = editor.dom.querySelector<HTMLElement>(".markra-html-node summary");
    const details = summary?.closest("details") as HTMLDetailsElement | null;
    assert.ok(summary);
    assert.equal(details?.open, false);
    summary.dispatchEvent(new MouseEvent("mousedown", {bubbles: true, cancelable: true}));
    summary.click();
    assert.equal(details?.open, true);
    assert.ok(editor.dom.querySelector(".markra-html-node"));

    editor.dom.querySelector<HTMLElement>(".markra-html-node p")?.dispatchEvent(
        new MouseEvent("mousedown", {bubbles: true, cancelable: true}),
    );
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    assert.equal(editor.dom.querySelector(".markra-html-node"), null);
    assert.ok(editor.state.selection.main.from > editor.state.doc.toString().indexOf("<details>"));
});

test("uses SiYuan sprite icons for every visual table toolbar action", async () => {
    const editor = createView("引言\n\n| 版本 | 作者 |\n| --- | --- |\n| v1.0 | 架构组 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const iconHref = (selector: string) => editor.dom.querySelector(`${selector} use`)?.getAttribute("href");
    assert.equal(iconHref(".markra-table-size-button"), "#iconTable");
    assert.equal(iconHref(".markra-table-align-left"), "#iconAlignLeft");
    assert.equal(iconHref(".markra-table-align-center"), "#iconAlignCenter");
    assert.equal(iconHref(".markra-table-align-right"), "#iconAlignRight");
    assert.equal(iconHref(".markra-table-width-button"), "#iconWidth");
    assert.equal(iconHref(".markra-table-delete-table"), "#iconTrashcan");
});

test("groups visual table actions by function without changing button order", async () => {
    const editor = createView("| 项目 | 负责人 |\n| --- | --- |\n| Markdown | 架构组 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const toolbar = editor.dom.querySelector(".markra-table-align-controls");
    assert.ok(toolbar);
    assert.deepEqual(
        Array.from(toolbar.children, (element) => element.getAttribute("data-table-control-group")),
        ["size", "alignment", "width", "delete"],
    );
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="size"] .markra-table-control').length, 1);
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="alignment"] .markra-table-control').length, 3);
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="width"] .markra-table-control').length, 1);
    assert.equal(toolbar.querySelectorAll('[data-table-control-group="delete"] .markra-table-control').length, 1);
    assert.equal(
        toolbar.querySelector('[data-table-control-group="alignment"] [aria-pressed="true"]')?.classList.contains("markra-table-align-left"),
        true,
    );
    const widthButton = toolbar.querySelector('[data-table-control-group="width"] .markra-table-width-button');
    assert.equal(widthButton?.getAttribute("data-mode"), "auto");
    assert.equal(widthButton?.getAttribute("aria-pressed"), "true");
});

test("keeps visual table actions visible after the table loses focus", async () => {
    const editor = createView("| 项目 | 内容 |\n| --- | --- |\n| 文档版本 | v1.7 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const sizeButton = editor.dom.querySelector(".markra-table-size-button") as HTMLButtonElement;
    sizeButton.blur();

    assert.equal(sizeButton.classList.contains("block__icon"), true);
    assert.equal(sizeButton.hidden, false);
    assert.equal(sizeButton.disabled, false);
});

test("keeps only the active table toolbar visible until explicit dismissal", async () => {
    const first = "| A | B |\n| --- | --- |\n| 1 | 2 |";
    const second = "| X | Y |\n| --- | --- |\n| 3 | 4 |";
    const editor = createView(`${first}\n\n正文\n\n${second}`);
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    let wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    wrappers[0].querySelector("table")?.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].classList.contains("markra-table-controls-visible"), true);

    wrappers[0].querySelector<HTMLButtonElement>(".markra-table-align-center")?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].classList.contains("markra-table-controls-visible"), true);

    wrappers[0].querySelector<HTMLButtonElement>(".markra-table-width-button")?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].classList.contains("markra-table-controls-visible"), true);

    wrappers[0].querySelector<HTMLButtonElement>(".markra-table-add-row")?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].classList.contains("markra-table-controls-visible"), true);

    wrappers[1].querySelector("table")?.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].classList.contains("markra-table-controls-visible"), false);
    assert.equal(wrappers[1].classList.contains("markra-table-controls-visible"), true);

    const sizeButton = wrappers[1].querySelector<HTMLButtonElement>(".markra-table-size-button");
    sizeButton?.click();
    const popover = editor.dom.querySelector<HTMLElement>(".markra-table-size-popover");
    assert.equal(popover?.dataset.tableId, wrappers[1].dataset.tableId);
    popover?.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    assert.equal(wrappers[1].classList.contains("markra-table-controls-visible"), true);

    document.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[1].classList.contains("markra-table-controls-visible"), false);
    assert.equal(editor.dom.querySelector(".markra-table-size-popover"), null);

    wrappers[1].querySelector("table")?.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    editor.contentDOM.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[1].classList.contains("markra-table-controls-visible"), false);
    await new Promise((resolve) => setTimeout(resolve, 50));
});

test("keeps hover visibility separate from persistent table activity", async () => {
    const editor = createView("| A | B |\n| --- | --- |\n| 1 | 2 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    let wrapper = editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap");
    assert.ok(wrapper);
    wrapper.dispatchEvent(new MouseEvent("mouseenter"));
    assert.equal(wrapper.dataset.tableHovered, "true");
    assert.equal(wrapper.classList.contains("markra-table-controls-visible"), true);

    wrapper.dispatchEvent(new MouseEvent("mouseleave"));
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrapper = editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap");
    assert.ok(wrapper);
    assert.equal(wrapper.dataset.tableActive, "false");
    assert.equal(wrapper.classList.contains("markra-table-controls-visible"), false);

    const centerButton = wrapper.querySelector<HTMLButtonElement>(".markra-table-align-center");
    centerButton?.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    centerButton?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrapper = editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap");
    assert.ok(wrapper);
    assert.equal(wrapper.dataset.tableActive, "true");
    assert.equal(wrapper.classList.contains("markra-table-controls-visible"), true);

    wrapper.dispatchEvent(new MouseEvent("mouseleave"));
    await new Promise((resolve) => setTimeout(resolve, 200));

    assert.equal(wrapper.dataset.tableActive, "true");
    assert.equal(wrapper.classList.contains("markra-table-controls-visible"), true);
});

test("keeps the table size picker inside the CodeMirror theme scope", async () => {
    const editor = createView("| 项目 | 内容 |\n| --- | --- |\n| 文档版本 | v1.7 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const sizeButton = editor.dom.querySelector(".markra-table-size-button") as HTMLButtonElement;
    sizeButton.click();

    const popover = document.querySelector(".markra-table-size-popover");
    assert.ok(popover);
    assert.equal(popover.parentElement, editor.dom);
    assert.equal(popover.querySelectorAll(".markra-table-size-cell").length, 80);
});

test("keeps every table toolbar bound to its own table after document positions shift", async () => {
    const first = "| A | B |\n| --- | --- |\n| 1 | 2 |";
    const second = "| X | Y |\n| --- | --- |\n| 3 | 4 |";
    const source = `前文\n\n${first}\n\n中间\n\n${second}\n\n后文`;
    const editor = createView(source);
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    editor.dispatch({changes: {from: 0, insert: "新增前缀\n\n"}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers.length, 2);
    wrappers[0].querySelector<HTMLButtonElement>(".markra-table-align-center")?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const updated = editor.state.doc.toString();
    assert.match(updated, /\| A \| B \|\n\| :---: \| :---: \|/u);
    assert.match(updated, /\| X \| Y \|\n\| --- \| --- \|/u);
});

test("keeps column width mode scoped to its table while positions and content change", async () => {
    const first = "| A | B |\n| --- | --- |\n| 1 | 2 |";
    const second = "| X | Y |\n| --- | --- |\n| 3 | 4 |";
    const editor = createView(`${first}\n\n${second}`);
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    let wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    wrappers[0].querySelector<HTMLButtonElement>(".markra-table-width-button")?.click();
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].dataset.widthMode, "even");
    assert.equal(wrappers[1].dataset.widthMode, "auto");

    editor.dispatch({selection: {anchor: editor.state.doc.length}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].dataset.widthMode, "even");
    assert.equal(wrappers[1].dataset.widthMode, "auto");

    editor.dispatch({changes: {from: 0, insert: "前文\n\n"}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].dataset.widthMode, "even");
    assert.equal(wrappers[1].dataset.widthMode, "auto");

    const firstCell = editor.state.doc.toString().indexOf("1");
    editor.dispatch({changes: {from: firstCell, to: firstCell + 1, insert: "11"}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].dataset.widthMode, "even");
    assert.equal(wrappers[1].dataset.widthMode, "auto");

    wrappers[0].querySelector<HTMLButtonElement>(".markra-table-align-center")?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers[0].dataset.widthMode, "even");
    assert.equal(wrappers[1].dataset.widthMode, "auto");

    const firstTableEnd = editor.state.doc.toString().indexOf("\n\n", 4);
    editor.dispatch({changes: {from: 0, to: firstTableEnd + 2}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    wrappers = editor.dom.querySelectorAll<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(wrappers.length, 1);
    assert.equal(wrappers[0].dataset.widthMode, "auto");
});

test("keeps the latest width mode during rapid width and alignment actions", async () => {
    const editor = createView("| A | B |\n| --- | --- |\n| 1 | 2 |");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const initialWrapper = editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap");
    const initialTableId = initialWrapper?.dataset.tableId;
    const staleCenterButton = initialWrapper?.querySelector<HTMLButtonElement>(".markra-table-align-center");
    const widthButton = initialWrapper?.querySelector<HTMLButtonElement>(".markra-table-width-button");
    assert.ok(initialTableId);
    assert.ok(staleCenterButton);
    assert.ok(widthButton);

    widthButton.click();
    staleCenterButton.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const currentWrapper = editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(currentWrapper?.dataset.tableId, initialTableId);
    assert.equal(currentWrapper?.dataset.widthMode, "even");
    assert.equal(currentWrapper?.dataset.tableActive, "true");
    assert.equal(currentWrapper?.classList.contains("markra-table-controls-visible"), true);
    assert.match(editor.state.doc.toString(), /\| :---: \| :---: \|/u);

    const staleRightButton = currentWrapper?.querySelector<HTMLButtonElement>(".markra-table-align-right");
    currentWrapper?.querySelector<HTMLButtonElement>(".markra-table-width-button")?.click();
    assert.equal(
        editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap")?.dataset.widthMode,
        "auto",
    );
    staleRightButton?.click();
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const finalWrapper = editor.dom.querySelector<HTMLElement>(".cm-markra-table-wrap");
    assert.equal(finalWrapper?.dataset.tableId, initialTableId);
    assert.equal(finalWrapper?.dataset.widthMode, "auto");
    assert.equal(finalWrapper?.dataset.tableAlignment, "right");
});

test("uses SiYuan native image wrapper and resize handle", async () => {
    const editor = createView("引言\n\n![架构图](assets/architecture.png)");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    assert.ok(editor.dom.querySelector(".img.markra-image-node"));
    assert.ok(editor.dom.querySelector(".img .protyle-action__drag"));
    assert.equal(editor.dom.querySelector(".markra-image-resize-handle"), null);
    assert.equal(editor.dom.querySelector(".markra-image-viewer-button"), null);
});

test("renders a titled local image as one complete widget", async () => {
    const source = "![本地测试图片](assets/format-showcase.svg \"本地测试图片\")";
    const editor = createView(source);
    editor.dispatch({selection: {anchor: source.length}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const image = editor.dom.querySelector<HTMLImageElement>(".markra-image-frame img");
    assert.ok(image);
    assert.equal(image.getAttribute("src"), "assets/format-showcase.svg");
    assert.equal(image.title, "本地测试图片");
    assert.equal(editor.dom.querySelector(".cm-line")?.textContent, "");
    assert.equal(editor.state.doc.toString(), source);
});

test("uses the intrinsic image width until the user resizes it", async () => {
    const editor = createView("![截图](assets/screenshot.png)");
    editor.dispatch({selection: {anchor: 0}});
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

    const image = editor.dom.querySelector(".markra-image-frame img") as HTMLImageElement;
    const frame = editor.dom.querySelector(".markra-image-frame") as HTMLElement;
    assert.equal(image.loading, "eager");
    assert.equal(image.classList.contains("cm-markra-image"), true);
    Object.defineProperty(image, "naturalWidth", {configurable: true, value: 506});
    image.dispatchEvent(new Event("load"));

    assert.equal(frame.style.width, "506px");
    assert.equal(frame.classList.contains("markra-image-frame-sized"), false);
});

test("selects the current Markdown line before the complete document", () => {
    const editor = createView("第一段\n\n第二段");
    const event = () => new KeyboardEvent("keydown", {
        key: "a",
        metaKey: true,
    });
    assert.equal(runScopeHandlers(editor, event(), "editor"), true);
    assert.equal(editor.state.selection.main.from, 0);
    assert.equal(editor.state.selection.main.to, 3);
    assert.equal(runScopeHandlers(editor, event(), "editor"), true);
    assert.equal(editor.state.selection.main.from, 0);
    assert.equal(editor.state.selection.main.to, editor.state.doc.length);
});
