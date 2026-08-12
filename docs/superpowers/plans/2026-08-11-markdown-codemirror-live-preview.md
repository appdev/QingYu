# Markdown CodeMirror 实时预览编辑器实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将原始 Markdown 编辑器替换为以 CodeMirror 6 `EditorState` 为唯一数据源的实时预览编辑器，同时保持 QingYu 现有 UI、模式按钮、文件 API 和移动端外观。

**Architecture:** `MarkdownEditor` 继续拥有加载、保存、标题和标签页生命周期，但只挂载一个长期存活的 `EditorView`。所见即所得与源码模式通过 `Compartment` 切换，Lezer Markdown 语法树驱动 Decoration 和 Widget，所有可视交互通过 CodeMirror Transaction 修改原始 Markdown，不再维护可编辑 HTML 或执行输入后的整篇 `HTML2Md`。

**Tech Stack:** TypeScript 4.9、CodeMirror 6、`@codemirror/lang-markdown`、`@codemirror/language`、`@codemirror/state`、`@codemirror/view`、`@lezer/markdown`、JSDOM、现有 Lute/DOMPurify/SiYuan Mermaid/KaTeX/代码高亮渲染器、Go Kernel Markdown API。

## Global Constraints

- Markdown 文档默认进入现有“所见即所得”模式，现有“Markdown / 所见即所得”按钮保持不变，不增加分屏模式。
- 标题、面包屑、保存状态、标签页、正文宽度、字体、颜色、暗色主题、桌面布局和移动端布局保持现有 QingYu 样式。
- 编辑期间只有 `EditorState.doc` 是可写文档状态；Widget DOM不得参与保存。
- 所见即所得与源码模式共享一个 `EditorView`、一个文档和一套撤销历史。
- 不引入 React、Tauri、Markra AI、Markra 文件工作区或 Markra 产品 UI。
- Lute仅保留给导入、导出、兼容转换和只读渲染，不再用于日常输入后的整篇 HTML → Markdown 回写。
- 无法无损映射的视觉操作必须回退源码，不得静默规范化或重写 Markdown。
- 保留现有 Go Kernel Markdown API、路径校验、SHA-256 revision 冲突检测和 800ms 自动保存。
- 所有前端验证必须包含 `cd app && pnpm run lint`；不得运行项目禁止的 `pnpm build`。
- 当前任务没有提交授权；实施步骤不得运行 `git commit` 或 `git push`，每个任务以工作树检查和验证结果作为评审检查点。

---

## 文件结构

### 新建文件

- `app/src/markdown/livePreview/types.ts`：编辑模式、renderer、render context 和异步 Widget 版本类型。
- `app/src/markdown/livePreview/language.ts`：Markdown/GFM 语言扩展和语法节点辅助函数。
- `app/src/markdown/livePreview/renderer.ts`：renderer Facet、注册函数和按节点查询函数。
- `app/src/markdown/livePreview/testDom.ts`：Node 测试中安装和恢复 JSDOM/CodeMirror 所需浏览器全局对象。
- `app/src/markdown/livePreview/reveal.ts`：光标、选区、组合输入和拖拽触发的源码显隐策略。
- `app/src/markdown/livePreview/preview.ts`：遍历可见语法树并汇总 Decoration 的主 ViewPlugin。
- `app/src/markdown/livePreview/inline.ts`：标题、强调、删除线、行内代码、链接、引用和列表 Decoration。
- `app/src/markdown/livePreview/structural.ts`：水平线、脚注、Frontmatter 和 GitHub 风格提示块的视觉结构与源码回退。
- `app/src/markdown/livePreview/code.ts`：代码块显示、语言标签和源码回退。
- `app/src/markdown/livePreview/diagram.ts`：Mermaid/公式异步 Widget 与版本失效保护。
- `app/src/markdown/livePreview/image.ts`：图片解析、安全 URL、预览 Widget、缩放和 HTML 图片序列化。
- `app/src/markdown/livePreview/table.ts`：GFM 表格解析、序列化、可视 Widget 和范围 Transaction。
- `app/src/markdown/livePreview/rawHtml.ts`：经过 DOMPurify 的只读 HTML Widget 和源码回退。
- `app/src/markdown/livePreview/extension.ts`：组装视觉模式和源码模式的 CodeMirror Extension。
- `app/src/markdown/livePreview/*.test.ts`：每个纯逻辑边界及 JSDOM EditorView 行为测试。
- `app/scripts/testMarkdownLivePreview.cjs`：Electron 中的真实布局、模式、宽表格和复杂 Widget 测试。

### 修改文件

- `app/package.json`、`app/pnpm-lock.yaml`：声明直接 CodeMirror/Lezer/JSDOM 测试依赖并注册实时预览布局测试脚本。
- `app/src/markdown/MarkdownEditor.ts`：改为单一 EditorView、Compartment 模式切换、Transaction 粘贴和保存。
- `app/src/markdown/clipboard.ts`、`app/src/markdown/clipboard.test.ts`：保留 Markdown 检测/退化文本修复，删除只为 HTML 预览恢复 Mermaid DOM属性的接口。
- `app/src/markdown/keyboard.ts`、`app/src/markdown/keyboard.test.ts`：将模式无关快捷键交给 CodeMirror，保留必要的外壳级保存行为。
- `app/src/assets/scss/business/_markdown.scss`：将预览样式迁移到单一 `.markdown-editor__surface` 和 `cm-markra-*` 语义类。
- `app/scripts/testMarkdownLayout.cjs`：使用新 surface 验证空文档高度和标签页宽度。
- `app/src/mobile/markdown.ts`：继续挂载同一个 `MarkdownEditor`，补充触摸环境配置但不建立移动端独立内核。

---

### Task 1: 建立 CodeMirror 实时预览契约和测试环境

**Files:**
- Modify: `app/package.json`
- Modify: `app/pnpm-lock.yaml`
- Create: `app/src/markdown/livePreview/types.ts`
- Create: `app/src/markdown/livePreview/renderer.ts`
- Create: `app/src/markdown/livePreview/testDom.ts`
- Test: `app/src/markdown/livePreview/renderer.test.ts`

**Interfaces:**
- Produces: `MarkdownEditorMode = "visual" | "source"`。
- Produces: `MarkdownRenderer { id, nodeNames, scope, render(context) }`。
- Produces: `markdownRenderer(renderer): Extension` 和 `getMarkdownRenderers(state, nodeName): readonly MarkdownRenderer[]`。
- Produces: `installMarkdownTestDom(): () => void`，返回恢复原全局对象的 cleanup。
- Consumes: CodeMirror `Facet`、`EditorState`、`EditorView`、`Decoration` 和 Lezer `SyntaxNodeRef`。

- [ ] **Step 1: 添加直接依赖并更新锁文件**

在 `app` 目录运行：

```bash
pnpm add -D @codemirror/language@^6.12.0 @codemirror/state@^6.5.0 @codemirror/view@^6.38.0 @lezer/markdown@^1.6.0 jsdom@^26.0.0 @types/jsdom@^21.1.7
```

依赖使用与现有 `@codemirror/lang-markdown` 兼容的同一 CodeMirror 6 主版本；如果 pnpm 解析到更新的兼容小版本，保留锁文件实际结果，不手工编辑 lockfile。

- [ ] **Step 2: 编写 renderer 注册失败测试**

```ts
import * as assert from "node:assert/strict";
import {describe, it} from "node:test";
import {EditorState} from "@codemirror/state";
import {getMarkdownRenderers, markdownRenderer} from "./renderer";

describe("markdownRenderer", () => {
    it("indexes renderers by syntax node name", () => {
        const renderer = {id: "image", nodeNames: ["Image"], render: () => undefined};
        const state = EditorState.create({extensions: [markdownRenderer(renderer)]});
        assert.deepEqual(getMarkdownRenderers(state, "Image"), [renderer]);
        assert.deepEqual(getMarkdownRenderers(state, "Table"), []);
    });

    it("rejects empty and duplicate ids", () => {
        assert.throws(() => markdownRenderer({id: "", nodeNames: ["Image"], render: () => undefined}));
        assert.throws(() => EditorState.create({extensions: [
            markdownRenderer({id: "image", nodeNames: ["Image"], render: () => undefined}),
            markdownRenderer({id: "image", nodeNames: ["Image"], render: () => undefined}),
        ]}));
    });
});
```

- [ ] **Step 3: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/renderer.test.ts`

Expected: FAIL，提示 `./renderer` 或导出函数不存在。

- [ ] **Step 4: 实现类型和 Facet 注册表**

`types.ts` 定义：

```ts
export type MarkdownEditorMode = "visual" | "source";
export type MarkdownRendererScope = "node" | "visible-range";

export interface MarkdownRenderContext {
    state: EditorState;
    view: EditorView;
    node: SyntaxNodeRef;
    visibleFrom: number;
    visibleTo: number;
    revealed: (from?: number, to?: number) => boolean;
    add: (range: Range<Decoration>) => void;
}

export interface MarkdownRenderer {
    id: string;
    nodeNames: readonly string[];
    scope?: MarkdownRendererScope;
    render: (context: MarkdownRenderContext) => void;
}
```

`renderer.ts` 使用 `Facet.define<MarkdownRenderer, MarkdownRendererRegistry>`，在 `combine` 中拒绝空 ID、重复 ID和空 `nodeNames`，并建立 `ReadonlyMap<string, readonly MarkdownRenderer[]>`。

- [ ] **Step 5: 建立可复用 JSDOM 测试环境**

`testDom.ts` 创建 `new JSDOM("<!doctype html><body></body>", {pretendToBeVisual: true})`，将 `window`、`document`、`navigator`、`MutationObserver`、`ResizeObserver`、`requestAnimationFrame`、`cancelAnimationFrame`、`HTMLElement`、`Node`、`Range` 和 `getComputedStyle` 安装到 `globalThis`；缺失的 `ResizeObserver` 使用具有空 `observe/unobserve/disconnect` 的测试实现。cleanup 按原值恢复或删除这些属性，所有创建 EditorView 的测试必须在 `afterEach` 中同时 `view.destroy()` 和调用 cleanup。

- [ ] **Step 6: 验证契约**

Run: `cd app && node --import tsx --test src/markdown/livePreview/renderer.test.ts`

Expected: PASS。

- [ ] **Step 7: 评审检查点**

Run: `git diff --check -- app/package.json app/pnpm-lock.yaml app/src/markdown/livePreview/types.ts app/src/markdown/livePreview/renderer.ts app/src/markdown/livePreview/testDom.ts app/src/markdown/livePreview/renderer.test.ts`

Expected: 无输出；不提交。

---

### Task 2: 将 MarkdownEditor 收敛为单一 EditorView

**Files:**
- Create: `app/src/markdown/livePreview/extension.ts`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/scripts/testMarkdownLayout.cjs`
- Test: `app/src/markdown/livePreview/extension.test.ts`

**Interfaces:**
- Consumes: Task 1 的 `MarkdownEditorMode`。
- Produces: `createMarkdownModeExtension(mode: MarkdownEditorMode): Extension`。
- Produces: `setMarkdownEditorMode(view, compartment, mode): void`，只发出 reconfigure effect。
- Produces: `.markdown-editor__surface` 单一挂载容器。

- [ ] **Step 1: 编写模式切换不修改文档和历史的失败测试**

创建 JSDOM 环境并断言同一个 `EditorView` 在 `visual → source → visual` 后 `state.doc`、选区和 undo 能力保持：

```ts
it("reconfigures one editor without replacing markdown", () => {
    const compartment = new Compartment();
    const view = createTestView("# 标题\n\n正文", compartment.of(createMarkdownModeExtension("visual")));
    view.dispatch({changes: {from: view.state.doc.length, insert: "!"}});
    const before = view.state.doc.toString();
    setMarkdownEditorMode(view, compartment, "source");
    setMarkdownEditorMode(view, compartment, "visual");
    assert.equal(view.state.doc.toString(), before);
    assert.equal(undo(view), true);
    assert.equal(view.state.doc.toString(), "# 标题\n\n正文");
});
```

- [ ] **Step 2: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/extension.test.ts`

Expected: FAIL，模式扩展尚不存在。

- [ ] **Step 3: 建立模式 Compartment 和单一 surface**

将 `MarkdownEditor` 的 shell 改为：

```html
<div class="markdown-editor__body">
    <div class="markdown-editor__surface"></div>
</div>
```

删除独立 source/preview 元素、`contenteditable` 预览事件和 `syncPreviewToSource()`。`load()` 创建一个 `EditorView`，基础扩展、history、lineWrapping、updateListener 永久存在，视觉/源码差异只放入 `markdownModeCompartment.of(createMarkdownModeExtension("visual"))`。

- [ ] **Step 4: 改写模式按钮**

`setPreview(preview)` 只更新按钮 active 状态、调用 `setMarkdownEditorMode(this.view, this.modeCompartment, preview ? "visual" : "source")` 并聚焦同一个 view；不得请求 `/api/lute/md2html`、替换文档或重建 EditorView。

- [ ] **Step 5: 迁移基础布局测试**

将 `testMarkdownLayout.cjs` 的 `.markdown-editor__preview` fixture 和查询改成 `.markdown-editor__surface`，保留空编辑器高度和标签页不横向溢出的断言；此任务暂不放入复杂表格 Widget。

- [ ] **Step 6: 运行聚焦验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/extension.test.ts`

Expected: PASS。

Run: `cd app && pnpm run test:markdown-layout`

Expected: PASS，surface 填满正文且 editor 宽度不超过标签页。

- [ ] **Step 7: 评审检查点**

确认 `MarkdownEditor.ts` 中只剩一个 `new EditorView`，且 `rg 'contenteditable="true"|HTML2Md|md2html' app/src/markdown/MarkdownEditor.ts` 不再命中正文预览链路；标题的 `contenteditable` 允许保留。

---

### Task 3: 实现 Lezer 语言层、Reveal Policy 和行内实时预览

**Files:**
- Create: `app/src/markdown/livePreview/language.ts`
- Create: `app/src/markdown/livePreview/reveal.ts`
- Create: `app/src/markdown/livePreview/inline.ts`
- Create: `app/src/markdown/livePreview/preview.ts`
- Modify: `app/src/markdown/livePreview/extension.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Test: `app/src/markdown/livePreview/reveal.test.ts`
- Test: `app/src/markdown/livePreview/preview.test.ts`

**Interfaces:**
- Produces: `markdownLanguageExtension: Extension`。
- Produces: `rangeIsRevealed(state, from, to, composing = false): boolean`。
- Produces: `markdownLivePreview(renderers?: readonly MarkdownRenderer[]): Extension`。
- Produces semantic classes `cm-markra-h1` 至 `cm-markra-h6`、`cm-markra-strong`、`cm-markra-emphasis`、`cm-markra-inline-code`、`cm-markra-link`、`cm-markra-blockquote`、`cm-markra-list`。

- [ ] **Step 1: 编写 Reveal Policy 失败测试**

覆盖折叠光标在范围外、光标在范围内、跨范围选择、全文选择和 composing 强制展开：

```ts
assert.equal(rangeIsRevealed(stateWithCursor(0), 5, 12), false);
assert.equal(rangeIsRevealed(stateWithCursor(7), 5, 12), true);
assert.equal(rangeIsRevealed(stateWithSelection(3, 9), 5, 12), true);
assert.equal(rangeIsRevealed(stateWithCursor(0), 5, 12, true), true);
```

- [ ] **Step 2: 编写行内预览失败测试**

创建包含 `# 标题`、`**粗体**`、`*斜体*`、`` `代码` ``、`[链接](https://example.com)`、引用和列表的 EditorView，断言光标在末尾时语法标记被 replace Decoration 隐藏且语义类存在；将光标移入粗体范围后断言 `**` 恢复可见。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/reveal.test.ts src/markdown/livePreview/preview.test.ts`

Expected: FAIL，Reveal 和实时预览扩展不存在。

- [ ] **Step 4: 实现语言和可见范围遍历**

使用 `markdown({extensions: [GFM]})` 建立语言层。`preview.ts` 使用 `ViewPlugin`，仅遍历 `view.visibleRanges` 与一行邻域；收集 Decoration 后用 `Decoration.set(ranges, true)`，在 `docChanged`、`selectionSet`、`viewportChanged` 或 composing 状态变化时重建。

- [ ] **Step 5: 实现行内 Decoration 和源码显隐**

对 heading/paragraph/blockquote/list 添加 line 或 mark Decoration；对 `HeaderMark`、`EmphasisMark`、`CodeMark`、`StrikethroughMark`、`LinkMark`、`URL` 和 `ListMark` 仅在 `rangeIsRevealed` 为 false 时添加 replace Decoration。隐藏标记不得跨换行；跨行范围拆成逐行 Decoration。

- [ ] **Step 6: 映射现有 QingYu 样式**

在 `_markdown.scss` 中只使用现有主题变量，为 `cm-markra-*` 设置与当前 `b3-typography` 对应的字号、字重、颜色、间距和边框；`.cm-gutters { display: none; }` 保证视觉与源码模式均不主动显示行号。

- [ ] **Step 7: 运行测试和类型检查**

Run: `cd app && node --import tsx --test src/markdown/livePreview/reveal.test.ts src/markdown/livePreview/preview.test.ts`

Expected: PASS。

Run: `cd app && pnpm run typecheck`

Expected: PASS。

---

### Task 4: 实现任务、链接、代码块和安全 HTML 回退

**Files:**
- Create: `app/src/markdown/livePreview/code.ts`
- Create: `app/src/markdown/livePreview/rawHtml.ts`
- Create: `app/src/markdown/livePreview/structural.ts`
- Modify: `app/src/markdown/livePreview/inline.ts`
- Modify: `app/src/markdown/livePreview/extension.ts`
- Test: `app/src/markdown/livePreview/code.test.ts`
- Test: `app/src/markdown/livePreview/rawHtml.test.ts`
- Test: `app/src/markdown/livePreview/inline.test.ts`
- Test: `app/src/markdown/livePreview/structural.test.ts`

**Interfaces:**
- Produces: `toggleMarkdownTask(view, markerFrom): boolean`。
- Produces: `resolveSafeMarkdownLink(source): string | undefined`。
- Produces: `codeBlockRenderer(): MarkdownRenderer`。
- Produces: `rawHtmlRenderer(): MarkdownRenderer`。
- Produces: `structuralMarkdownRenderers(): readonly MarkdownRenderer[]`，覆盖 horizontal rule、footnote、Frontmatter 和 GitHub alert/callout。

- [ ] **Step 1: 编写 Transaction 和安全失败测试**

断言点击任务只把 `[ ]` 改为 `[x]`、再次点击恢复；普通点击链接只定位选区，`Cmd/Ctrl+点击` 仅允许 `http:`, `https:`, `mailto:` 和现有安全本地链接；`javascript:` 返回 undefined。断言 raw HTML 中的 `<script>`、`onerror` 和 `javascript:` 在 Widget DOM中不存在，但源码字符逐字不变。断言水平线、脚注定义/引用、YAML Frontmatter 和 `[!NOTE]`/`[!TIP]`/`[!IMPORTANT]`/`[!WARNING]`/`[!CAUTION]` 提示块具有视觉结构，光标进入后完整源码恢复。

- [ ] **Step 2: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/inline.test.ts src/markdown/livePreview/code.test.ts src/markdown/livePreview/rawHtml.test.ts src/markdown/livePreview/structural.test.ts`

Expected: FAIL，相关 renderer 和 command 不存在。

- [ ] **Step 3: 实现任务和链接行为**

任务 Widget 的 `mousedown` 阻止浏览器默认选区，command 验证目标范围精确等于 `[ ]`、`[x]` 或 `[X]` 后 dispatch 单一 change。链接 renderer 复用当前 URL 安全边界；无修饰键时显示源码并设置 CodeMirror selection，不打开 URL。

- [ ] **Step 4: 实现代码块**

未 reveal 的 fenced code block 显示当前代码块表面、语言标签和高亮结果；光标进入 fenced range 时移除块级替换并显示原始 fence 和代码文本。未知语言必须显示纯文本，不能抛出异常或改写 fence info。

- [ ] **Step 5: 实现安全 HTML 回退**

raw HTML renderer 将节点源码传给 `window.DOMPurify.sanitize` 后放入只读 Widget，Widget `ignoreEvent()` 返回 false 仅用于“编辑源码”按钮；按钮把 selection 定位到原节点并触发 reveal。DOMPurify 不可用或 sanitizer 抛错时不创建 Widget，直接显示源码。

- [ ] **Step 6: 实现结构性语法**

水平线使用 replace Widget但不提供独立可编辑状态；脚注引用显示上标，脚注定义使用 line Decoration；Frontmatter 折叠为带 `YAML` 标签的只读摘要；GitHub alert/callout 使用现有主题语义色和引用块结构。四类结构在光标/选区进入对应 Markdown range 时全部移除替换层并显示源码，解析失败时不创建视觉结构。

- [ ] **Step 7: 验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/inline.test.ts src/markdown/livePreview/code.test.ts src/markdown/livePreview/rawHtml.test.ts src/markdown/livePreview/structural.test.ts`

Expected: PASS。

---

### Task 5: 实现 Mermaid 和数学公式异步 Widget

**Files:**
- Create: `app/src/markdown/livePreview/diagram.ts`
- Modify: `app/src/markdown/livePreview/extension.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Test: `app/src/markdown/livePreview/diagram.test.ts`

**Interfaces:**
- Produces: `diagramRenderKey(kind, source, documentVersion): string`。
- Produces: `diagramRenderer(options): MarkdownRenderer`，其中 `options.renderMermaid(element, source)` 和 `options.renderMath(element, source, displayMode)` 返回 `void | Promise<void>`。
- Produces: `AsyncMarkdownWidget`，只接受与当前 document version 和 source hash 匹配的异步结果。

- [ ] **Step 1: 编写异步失效和回退失败测试**

使用可控 Promise 创建 Mermaid Widget，先开始渲染，再修改 fenced source，随后完成旧 Promise；断言旧结果未写入新 Widget。再让 renderer reject，断言源码不变、Widget 显示错误提示且“编辑源码”可定位原范围。

- [ ] **Step 2: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/diagram.test.ts`

Expected: FAIL，异步 Widget 不存在。

- [ ] **Step 3: 实现 fenced Mermaid 识别**

只匹配 info string 第一 token 为 `mermaid` 的 fenced code block，Widget source 为 fence 内原文，不使用经过 DOMPurify 丢失属性的 HTML 占位符。光标进入 block range 时显示完整源码，离开后按可见范围渲染。

- [ ] **Step 4: 接入现有渲染能力**

为 Mermaid 建立适配器调用现有脚本加载和渲染入口；为行内/块公式建立 KaTeX 适配器。适配器只接收 DOM容器和源码，不读取或回写 MarkdownEditor DOM。渲染输出继续经过现有安全处理。

- [ ] **Step 5: 加入缓存和销毁**

缓存键由 kind、source hash、主题模式和渲染器版本组成。Widget `destroy()` 清理观察器、事件和临时 DOM；离开 viewport 后允许释放 SVG/KaTeX DOM，但保留轻量缓存元数据。

- [ ] **Step 6: 验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/diagram.test.ts`

Expected: PASS，旧异步结果不能污染新文档。

---

### Task 6: 实现图片 Widget、链接安全和持久化缩放

**Files:**
- Create: `app/src/markdown/livePreview/image.ts`
- Modify: `app/src/markdown/livePreview/extension.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Test: `app/src/markdown/livePreview/image.test.ts`

**Interfaces:**
- Produces: `parseMarkdownImage(source): MarkdownImageReference | undefined`。
- Produces: `serializeResizedMarkdownImage(image, width): string`。
- Produces: `resizeMarkdownImage(view, range, image, width): boolean`。
- Produces: `imageRenderer(options): MarkdownRenderer`，复用现有资源 URL解析函数。

- [ ] **Step 1: 编写图片保真和缩放失败测试**

测试普通 `![说明](assets/a.png "标题")` 未缩放时源码不变；首次缩放为 320 时序列化为转义后的 `<img src="assets/a.png" alt="说明" title="标题" width="320">`；已有 `<img width="200">` 缩放只更新 `width`；负数、NaN 和超过 surface 的宽度被限制到 `[17, surfaceWidth - 8]`。

- [ ] **Step 2: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/image.test.ts`

Expected: FAIL，图片 parser/serializer 不存在。

- [ ] **Step 3: 实现图片解析和安全序列化**

parser 保留 alt、src、title 和原范围。serializer 使用 DOM 属性转义规则生成固定属性顺序 `src`, `alt`, `title`, `width`，width 为四舍五入整数；没有 title 时省略该属性。普通图片只在用户完成一次实际缩放后转换为 HTML。

- [ ] **Step 4: 实现现有样式的图片 Widget**

Widget DOM沿用 `.img`、`.protyle-action__drag` 和当前 hover/touch 样式。pointermove 只更新临时宽度；pointerup/pointercancel 后验证原 Markdown range 仍匹配初始 source，再 dispatch 一次 Transaction，否则取消写入并展开源码。

- [ ] **Step 5: 验证桌面和触摸路径**

单元测试模拟 pointer sequence 并断言只有结束时产生一个文档 change；`@media (hover: none)` 下缩放手柄持续可见，拖拽期间禁止文本选择且结束后清理监听器。

- [ ] **Step 6: 运行验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/image.test.ts`

Expected: PASS。

---

### Task 7: 实现 GFM 表格解析、序列化和可视编辑

**Files:**
- Create: `app/src/markdown/livePreview/table.ts`
- Modify: `app/src/markdown/livePreview/extension.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Test: `app/src/markdown/livePreview/table.test.ts`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Produces: `parseMarkdownTable(source): MarkdownTableModel | undefined`。
- Produces: `serializeMarkdownTable(model): string`。
- Produces: `replaceMarkdownTable(view, range, model): boolean`。
- Produces: `tableRenderer(): MarkdownRenderer`。

- [ ] **Step 1: 编写表格 round-trip 失败测试**

测试中文长内容、escaped pipe、空单元格、左右/居中对齐、单元格中的行内代码和不规则空格。没有编辑时 `serialize(parse(source))` 不得用于回写；发生单元格编辑时只替换目标表格范围，表格前后文本逐字不变。

- [ ] **Step 2: 编写表格操作失败测试**

覆盖编辑单元格、添加/删除行、添加/删除列、修改对齐；每个 command 产生一个 Transaction，并把光标映射到相应的新单元格。无合法 delimiter row 或列数不可确定时 parser 返回 undefined，显示源码。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/table.test.ts`

Expected: FAIL，表格模块不存在。

- [ ] **Step 4: 实现 parser 和 serializer**

parser 从 Lezer `Table` 节点提供的精确 range 读取文本，逐字符处理 escaped pipe 和行内代码，不用简单 `split("|")`。model 保存 `cells`, `alignments`, `header`, `source`；serializer 只在用户执行表格操作时生成稳定 GFM，转义单元格中的未转义 pipe 并保留换行边界。

- [ ] **Step 5: 实现可视表格 Widget**

Widget 使用现有表格字体、边框、斑马纹和间距。单元格编辑器的值提交时 dispatch range replacement；解析或 range 校验失败时关闭 Widget 并把 selection 放入源码表格。宽表格使用自身横向滚动区域或压缩列宽，不能扩大 `.markdown-editor`。

- [ ] **Step 6: 更新真实布局测试**

`testMarkdownLayout.cjs` 构造 `.markdown-editor__surface` 和表格 Widget DOM，断言 `editorScrollWidth === editorClientWidth`、表格容器不超过 surface、真实 table 为 `display: table` 且高度大于 0。

- [ ] **Step 7: 验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/table.test.ts`

Expected: PASS。

Run: `cd app && pnpm run test:markdown-layout`

Expected: PASS。

---

### Task 8: 将剪贴板、图片附件和拖拽迁移到 Transaction

**Files:**
- Modify: `app/src/markdown/clipboard.ts`
- Modify: `app/src/markdown/clipboard.test.ts`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Create: `app/src/markdown/livePreview/clipboard.ts`
- Test: `app/src/markdown/livePreview/clipboard.test.ts`

**Interfaces:**
- Consumes: 现有 `getMarkdownClipboardData(dataTransfer): string | undefined`。
- Produces: `insertMarkdownClipboard(view, markdown): boolean`。
- Produces: `markdownClipboardExtension(options): Extension`，其中资源保存回调返回要插入的 Markdown 链接或 undefined。
- Removes: `getMarkdownDiagramBlocks`、`restoreMarkdownDiagramAttributes`、`restoreMarkdownDiagramSources`。

- [ ] **Step 1: 编写直接粘贴失败测试**

测试空文档、选区替换、文档中间插入和全文选择粘贴，断言原始 Markdown 直接进入 `EditorState.doc`，不会请求 `/api/lute/md2html`。保留现有用户长文退化文本修复用例，确保其输出仍通过同一 Transaction 插入。

- [ ] **Step 2: 编写资源粘贴失败测试**

模拟图片保存成功返回 `![image](assets/pasted.png)`、失败返回 undefined、多文件包含 Markdown 文本。断言成功只插入一次链接，失败不删除选区，多文件有明确 Markdown 时优先插入文本。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/clipboard.test.ts src/markdown/livePreview/clipboard.test.ts`

Expected: FAIL，新 clipboard extension 尚不存在。

- [ ] **Step 4: 实现 CodeMirror clipboard extension**

使用 `EditorView.domEventHandlers({paste, drop})`。同步 Markdown 通过 `view.dispatch({changes: view.state.changeByRange(...)})` 支持多选区；异步资源保存开始前记录 selection 和 source snapshot，完成时通过 mapped transaction 或当前有效 selection 插入，文档已切换则丢弃结果并显示现有错误提示。

- [ ] **Step 5: 删除 HTML 预览专属恢复代码**

从 `MarkdownEditor` 删除 `pasteMarkdown` 的 md2html/DOM fragment 路径；从 `clipboard.ts` 和测试删除 diagram DOM属性恢复函数，只保留 Markdown 检测、换行规范化和退化文本修复。

- [ ] **Step 6: 验证**

Run: `cd app && node --import tsx --test src/markdown/clipboard.test.ts src/markdown/livePreview/clipboard.test.ts`

Expected: PASS，且 `rg 'restoreMarkdownDiagram|createContextualFragment|/api/lute/md2html' app/src/markdown` 无命中。

---

### Task 9: 输入法、异步生命周期和长文性能保护

**Files:**
- Modify: `app/src/markdown/livePreview/preview.ts`
- Modify: `app/src/markdown/livePreview/diagram.ts`
- Modify: `app/src/markdown/livePreview/image.ts`
- Modify: `app/src/markdown/livePreview/table.ts`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Test: `app/src/markdown/livePreview/performance.test.ts`
- Test: `app/src/markdown/livePreview/composition.test.ts`

**Interfaces:**
- Produces: `MarkdownRenderBudget { maxTableCharacters, viewportMarginLines }`。
- Produces: `markdownCompositionState: StateField<boolean>` 或等价的 ViewPlugin 状态，供 Reveal/Widget 查询。
- Produces: `MarkdownDocumentToken { path, revision, generation }`，异步回调必须校验完整 token。

- [ ] **Step 1: 编写 IME 和可见范围失败测试**

模拟 `compositionstart → docChanged → compositionend`，断言组合范围保持源码展开、中间 Transaction 不触发结构 Widget 重建、最终 Markdown change 只发布稳定内容。构造 100 个 Mermaid 和 50 个大表格，断言初次 viewport 只创建可见范围及邻近范围的 Widget。

- [ ] **Step 2: 编写文档切换失效测试**

开始旧文档 Mermaid、图片和附件异步任务后切换 `path/revision/generation`，完成旧任务，断言当前 EditorView DOM、文档和保存状态均未改变。

- [ ] **Step 3: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/composition.test.ts src/markdown/livePreview/performance.test.ts`

Expected: FAIL，composition/token/budget 尚未实现。

- [ ] **Step 4: 实现组合输入保护**

通过 CodeMirror composing 状态和 DOM composition 事件更新 StateField；Reveal Policy 在 composing 时展开当前行，preview plugin 延迟当前结构节点的替换 Decoration，`MarkdownEditor` 的 updateListener 仍将最终 `state.doc` 标记 dirty，但保存防抖只读取最终快照。

- [ ] **Step 5: 实现 viewport 和预算降级**

默认 `viewportMarginLines = 20`、`maxTableCharacters = 20000`。超过表格预算或不在可见邻域的昂贵节点显示轻量占位或源码，不创建 Mermaid/KaTeX/table DOM；滚动进入范围后再渲染。

- [ ] **Step 6: 实现 document token**

每次 load/reload/rename 增加 generation；异步 renderer 和资源保存捕获 `{path, revision, generation}`，完成前与当前 token 严格相等，否则静默销毁结果。destroy 后所有 token 自动失效。

- [ ] **Step 7: 验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/composition.test.ts src/markdown/livePreview/performance.test.ts`

Expected: PASS。

---

### Task 10: 完成样式迁移、移动端共享和旧链路删除

**Files:**
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/src/mobile/markdown.ts`
- Modify: `app/src/markdown/keyboard.ts`
- Modify: `app/src/markdown/keyboard.test.ts`
- Delete obsolete code inside: `app/src/markdown/clipboard.ts`
- Test: `app/src/markdown/livePreview/mode.test.ts`

**Interfaces:**
- Consumes: Tasks 1–9 的 `createMarkdownModeExtension`、renderer 和 clipboard extension。
- Produces: 完整的单一 `MarkdownEditor.view` 生命周期。
- Removes: preview DOM、`syncPreviewToSource()`、`pasteMarkdown()`、`resizeImage()` 旧 DOM路径和 `private lute = Lute.New()`。

- [ ] **Step 1: 编写最终模式和快捷键失败测试**

断言视觉模式与源码模式使用同一 `EditorView` identity；`Cmd/Ctrl+A` 全选 `state.doc`；模式切换不创建 history；`Cmd/Ctrl+S` 调用外壳 save；移动端创建的实例启用同一 `createMarkdownModeExtension("visual")`。

- [ ] **Step 2: 运行测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/livePreview/mode.test.ts src/markdown/keyboard.test.ts`

Expected: FAIL，仍存在旧模式分支或快捷键行为不统一。

- [ ] **Step 3: 删除旧双编辑面实现**

删除所有预览 input/beforeinput/pointerdown 监听、HTML clone、diagram DOM源码恢复、Lute HTML2Md 和 md2html preview 请求。标题 `contenteditable`、Go API、保存冲突处理、重命名和 breadcrumb 保留。

- [ ] **Step 4: 完成 CSS 与移动端**

把 `.markdown-editor__preview` 规则迁移到 `.markdown-editor__surface .cm-content` 和语义 Widget 类；桌面正文最大宽度保持 760px/fullWidth 规则，移动端只调整 padding、触摸手柄和 viewport，不复制 renderer 或 clipboard 代码。

- [ ] **Step 5: 执行残留扫描**

Run: `rg 'markdown-editor__preview|syncPreviewToSource|HTML2Md|restoreMarkdownDiagram|createContextualFragment' app/src/markdown app/src/mobile app/src/assets/scss/business/_markdown.scss`

Expected: 无命中。

Run: `rg 'contenteditable="true"' app/src/markdown/MarkdownEditor.ts`

Expected: 只命中标题输入；正文无命中。

- [ ] **Step 6: 验证**

Run: `cd app && node --import tsx --test src/markdown/livePreview/mode.test.ts src/markdown/keyboard.test.ts`

Expected: PASS。

---

### Task 11: 使用真实长文完成桌面与移动端集成验收

**Files:**
- Create: `app/scripts/testMarkdownLivePreview.cjs`
- Modify: `app/package.json`
- Modify: `app/scripts/testMarkdownLayout.cjs`
- Test fixture read-only: `/Users/ying/Downloads/推送通知端到端技术方案.md`
- Test fixture read-only: `/Users/ying/.codex/attachments/3bf7631b-cbb6-4523-aee4-d3c4b603c294/pasted-text.txt`

**Interfaces:**
- Produces: `pnpm run test:markdown-live-preview`。
- Consumes: 最终 MarkdownEditor、现有桌面开发运行时和用户参考长文。

该脚本是针对实际开发客户端的 CDP 集成测试，运行前必须以 `--remote-debugging-port=9222` 启动 QingYu Electron，并在测试工作空间中打开由参考长文复制出的测试 Markdown 文档。脚本只读参考 fixture，通过应用 API创建唯一命名的测试副本；测试结束保留副本供人工复核，不删除用户文件。

- [ ] **Step 1: 编写 Electron 集成测试**

测试脚本连接 `http://127.0.0.1:9222/json/list`，定位标题包含 `QingYu` 的页面，通过 Chrome DevTools Protocol 执行实际应用 DOM和 CodeMirror 操作。它读取参考长文创建测试副本后执行：打开、视觉渲染、源码切换、模式返回、选区编辑、undo、redo、粘贴、保存和重新读取；不得写回用户提供的原始文件。

- [ ] **Step 2: 添加结构断言**

断言参考长文识别 33 个表格、8 个 Mermaid，并且所有复杂块要么有正高度 Widget，要么处于明确源码回退状态；“文档变更记录”表格和“13.3 关键路径（移动端）”Mermaid 在滚动进入 viewport 后可见；标签页容器没有横向溢出。

- [ ] **Step 3: 添加保真断言**

记录输入前 Markdown，将一次明确字符插入和一次表格单元格编辑表示为 expected ranges；模式切换、滚动、渲染和 reload 模拟后，除这两个 ranges 外其余源码逐字相等。对用户 pasted-text fixture 执行粘贴，断言标题、表格和 fenced Mermaid 数量与输入预期一致。

- [ ] **Step 4: 添加移动窄视口断言**

把 BrowserWindow viewport 设置为 390×844，断言 surface 宽度不超过容器、表格内部处理溢出、图片 `max-width: 100%`、触摸缩放手柄可见、软键盘等价 composition 事件后 Markdown 正确。

- [ ] **Step 5: 注册和运行测试**

在 `package.json` 增加：

```json
"test:markdown-live-preview": "electron ./scripts/testMarkdownLivePreview.cjs"
```

Run: `cd app && pnpm run test:markdown-live-preview`

Expected: PASS，并输出桌面/移动 viewport、表格数、Mermaid 数、源码保真结果和 overflow 指标。

- [ ] **Step 6: 实际桌面 UI检查**

在当前 QingYu macOS 开发客户端中打开用户参考长文，逐项操作两个模式按钮、`Cmd+A`、复制、粘贴、撤销、重做、表格单元格、图片缩放、Mermaid 源码展开和保存重载。通过 CDP或 Computer Use 记录最终 DOM指标和截图，但不修改用户原始 fixture。

---

### Task 12: 最终回归、冻结范围和交付

**Files:**
- Verify all files under: `app/src/markdown/`
- Verify: `app/src/assets/scss/business/_markdown.scss`
- Verify: `app/src/mobile/markdown.ts`
- Verify: `app/package.json`
- Verify: `app/pnpm-lock.yaml`
- Verify: `app/scripts/testMarkdownLayout.cjs`
- Verify: `app/scripts/testMarkdownLivePreview.cjs`

**Interfaces:**
- Consumes: Tasks 1–11 的最终实现。
- Produces: 可运行、可验证且没有旧可编辑 HTML链路的 Markdown 编辑器工作树。

- [ ] **Step 1: 运行全部自动化测试**

Run: `cd app && pnpm test`

Expected: 全部测试 PASS，无 skipped Markdown 核心用例。

- [ ] **Step 2: 运行布局和实时预览测试**

Run: `cd app && pnpm run test:markdown-layout && pnpm run test:markdown-live-preview`

Expected: 两个命令 PASS，33 个表格和 8 个 Mermaid 验收数据正确。

- [ ] **Step 3: 运行类型检查与 lint**

Run: `cd app && pnpm run lint`

Expected: TypeScript 和 ESLint PASS；不得用 `pnpm build` 代替。

- [ ] **Step 4: 执行安全与旧链路扫描**

Run: `rg 'markdown-editor__preview|syncPreviewToSource|HTML2Md|restoreMarkdownDiagram|createContextualFragment' app/src/markdown app/src/mobile app/src/assets/scss/business/_markdown.scss`

Expected: 无命中。

Run: `rg 'innerHTML\s*=|insertAdjacentHTML|outerHTML\s*=' app/src/markdown/livePreview`

Expected: 只允许 `rawHtml.ts` 和受控第三方 renderer 容器中的 DOMPurify 清理结果；逐条人工确认，没有用户源码未经清理进入 DOM。

- [ ] **Step 5: 检查最终差异**

Run: `git diff --check -- app/package.json app/pnpm-lock.yaml app/src/markdown app/src/assets/scss/business/_markdown.scss app/src/mobile/markdown.ts app/scripts/testMarkdownLayout.cjs app/scripts/testMarkdownLivePreview.cjs`

Expected: 无空白错误。

Run: `git status --short`

Expected: 保留用户既有无关修改；报告本计划涉及的文件，不清理、不暂存、不提交。

- [ ] **Step 6: 交付报告**

报告单一 EditorState、UI保持、语法支持、桌面/移动端 UI证据、全部验证命令和结果；若任何 renderer 仍回退源码，列出具体语法与残余风险，不得将降级描述为完整支持。
