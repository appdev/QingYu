# Markdown 编辑器整体表现契约 Implementation Plan

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立以 Protyle 和思源组件体系为基准的有限编辑器表现契约，使 Markdown 可视化模式、源码模式、桌面端、移动端及标准第三方主题获得一致、可测试、可持续维护的整体 UI。

**Architecture:** 使用单一契约目录声明组件、状态、平台、原生参照、样式属性、几何指标和回退变量；主题解析器优先消费思源标准变量，只为第三方主题对标准 Protyle 选择器的覆盖建立有限真实探针。思源宿主适配层负责共享 DOM 控件和 SCSS，`markra-core` 仅消费宿主无关接口；真实 Lute BlockDOM、CodeMirror 运行态和实际应用矩阵共同提供验收证据。

**Tech Stack:** TypeScript、CodeMirror 6、SCSS、Lute `Md2BlockDOM`、Node.js `node:test`、JSDOM、Electron、Chrome DevTools Protocol、pnpm。

## Global Constraints

- 本计划的唯一需求基准是 `docs/superpowers/specs/2026-08-13-markdown-editor-appearance-contract-design.md`；它取代 `2026-08-12-markdown-native-theme-parity` 设计与计划。
- 可视化模式中有原生对应物的内容必须匹配 Protyle；Markdown 独有功能必须使用思源标准组件与语义变量。
- 源码模式保留代码编辑器布局，同时覆盖背景、字体、行高、光标、选区、行号、活动行、搜索和浮层。
- 桌面端与移动端消费同一契约；平台差异只能由契约中的响应式规则表达。
- 覆盖默认、悬停、聚焦、选中、禁用、编辑、只读、空内容、错误、展开、拖拽和键盘导航等适用状态。
- `markra-core` 不得读取 `window.siyuan`、持有 Protyle DOM 或依赖思源私有事件类名。
- 第三方主题直接支持标准 `--b3-*` 变量和标准 Protyle/组件选择器；依赖私有 DOM 或不可读取伪元素的主题使用标准语义回退，不增加主题专用补丁。
- 不改变 Markdown 解析、文件格式、保存、撤销、选区、输入法、粘贴、资源本地化和链接语义。
- 不修改导出 HTML/PDF 链路，不手工编辑 `app/stage/protyle/js/lute/lute.min.js`，不修改 `app/stage/build/**`。
- 主题或配置热更新不得重建 `EditorView`，不得改变文档、选区、滚动、撤销、脏状态和当前模式。
- 每个原生等价组件比较结构、计算样式和关键几何；关键几何差异上限为 `1px`。
- 完成前必须通过单元测试、Markdown 布局测试、实际应用矩阵、类型检查和 `cd app && pnpm run lint`；禁止用 `pnpm build`、`pnpm dev` 或 `npx webpack` 代替验证。
- 现有工作区包含用户未提交改动；执行时逐文件合并，不得重置、覆盖或清除无关改动。
- 项目规则禁止未经用户明确授权执行 `git commit` 或 `git push`；每个任务以测试结果和 `git diff --check` 作为评审关口，保持改动未提交。

---

## 文件结构与职责

| 路径 | 职责 |
| --- | --- |
| `app/src/markdown/appearance/contracts.json` | 唯一的表现组件目录，供 TypeScript 与 Electron 测试共同读取。 |
| `app/src/markdown/appearance/contracts.ts` | 契约类型、运行时校验、查询和 CSS 变量命名。 |
| `app/src/markdown/appearance/testSupport.ts` | 测试专用源码扫描、宿主适配器、EditorView 挂载和连续性快照工具。 |
| `app/src/markdown/appearance/fixture.ts` | 标准 Markdown 样本、Lute 原生夹具构造和组件查找。 |
| `app/src/markdown/appearance/themeResolver.ts` | 标准变量读取、有限原生探针、快照比较、热更新和引用计数。 |
| `app/src/markdown/appearance/runtimeHarness.ts` | 开发环境实际应用测试入口；创建不落盘的原生/Markdown 并排夹具。 |
| `app/src/markdown/appearance/*.test.ts` | 契约、解析顺序、生命周期、状态保持和样式源约束测试。 |
| `app/src/assets/scss/util/_editor-appearance.scss` | Protyle 与 Markdown 共同消费的有限布局/状态 mixin。 |
| `app/src/protyle/codeLanguageMenu.ts` | 原生 Protyle 与 Markdown 共用的代码语言菜单 DOM、筛选和键盘行为。 |
| `app/src/markdown/markra-core/adapter.ts` | 宿主无关的控件请求和关闭句柄接口。 |
| `app/src/markdown/siyuanAdapter.ts` | 将宿主请求映射到思源菜单、图标、定位、渲染器和插件事件。 |
| `app/src/assets/scss/business/_markdown.scss` | 只保留 Markdown 壳层、契约映射和 Markdown 独有布局；删除重复视觉常量。 |
| `app/scripts/testMarkdownAppearance.cjs` | 通过 CDP 驱动正在运行的应用，执行完整矩阵、输出 JSON 报告和截图。 |
| `app/scripts/testMarkdownLayout.cjs` | 保留无应用依赖的布局回归，并改用 Lute 生成原生基准。 |
| `docs/superpowers/verification/2026-08-13-markdown-editor-appearance-contract.md` | 最终矩阵结果、运行命令、截图目录和第三方主题回退项。 |

## 契约组件清单

以下 ID 是实现和验收的固定范围。`native selector` 为空的条目以 `reference selector` 指向的思源标准组件为基准。

| ID | 类别 | Markdown selector | Native/reference selector | 状态 |
| --- | --- | --- | --- | --- |
| `shell.document` | `editor-foundation` | `.markdown-editor` | `.protyle` | default, readonly |
| `shell.metadata` | `native-equivalent` | `.markdown-editor__metadata` | `.protyle-background` | default, hover, disabled |
| `shell.title` | `native-equivalent` | `.markdown-editor__title` | `.protyle-title` | default, focus, readonly, empty |
| `editor.visual` | `editor-foundation` | `.cm-editor[data-markdown-mode="visual"]` | `.protyle-wysiwyg` | default, focus, readonly, empty |
| `editor.source` | `editor-foundation` | `.cm-editor[data-markdown-mode="source"]` | `.b3-typography` | default, focus, readonly, empty |
| `editor.cursor` | `editor-foundation` | `.cm-cursor` | `--b3-theme-primary` | default, focus |
| `editor.selection` | `editor-foundation` | `.cm-selectionBackground` | `--b3-theme-primary-lightest` | selected, focus |
| `editor.active-line` | `editor-foundation` | `.cm-activeLine` | `--b3-list-hover` | default |
| `editor.gutter` | `editor-foundation` | `.cm-gutters` | `.b3-list` | default, selected |
| `editor.placeholder` | `editor-foundation` | `.cm-placeholder` | `.protyle-wysiwyg--empty:empty::before` | empty |
| `editor.scroller` | `editor-foundation` | `.cm-scroller` | `.protyle-content` | default |
| `editor.drag-indicator` | `editor-foundation` | `.markra-block-drop-indicator` | `.dragover__bottom::after` | drag |
| `editor.error` | `editor-foundation` | `.markdown-editor__status[data-status="error"]` | `.b3-snackbar--error` | error |
| `block.paragraph` | `native-equivalent` | `.cm-line:not([class*="cm-markra-"])` | `.protyle-wysiwyg .p` | default, focus, selected |
| `block.heading-1` | `native-equivalent` | `.cm-markra-h1` | `.protyle-wysiwyg .h1` | default, focus, selected |
| `block.heading-2` | `native-equivalent` | `.cm-markra-h2` | `.protyle-wysiwyg .h2` | default, focus, selected |
| `block.heading-3` | `native-equivalent` | `.cm-markra-h3` | `.protyle-wysiwyg .h3` | default, focus, selected |
| `block.heading-4` | `native-equivalent` | `.cm-markra-h4` | `.protyle-wysiwyg .h4` | default, focus, selected |
| `block.heading-5` | `native-equivalent` | `.cm-markra-h5` | `.protyle-wysiwyg .h5` | default, focus, selected |
| `block.heading-6` | `native-equivalent` | `.cm-markra-h6` | `.protyle-wysiwyg .h6` | default, focus, selected |
| `block.list` | `native-equivalent` | `.cm-markra-list-item` | `.protyle-wysiwyg .list` | default, hover, selected |
| `block.task` | `native-equivalent` | `.cm-markra-task-checkbox` | `.protyle-wysiwyg .protyle-task--done` | default, hover, focus, disabled |
| `block.blockquote` | `native-equivalent` | `.cm-markra-blockquote` | `.protyle-wysiwyg .bq` | default, selected |
| `block.callout` | `native-equivalent` | `.cm-markra-callout` | `.protyle-wysiwyg [data-type="NodeBlockquote"]` | default, expanded, selected |
| `block.horizontal-rule` | `native-equivalent` | `.cm-markra-horizontal-rule` | `.protyle-wysiwyg .hr` | default, hover, selected |
| `block.table` | `native-equivalent` | `.cm-markra-table-wrap` | `.protyle-wysiwyg .table` | default, focus, selected, readonly |
| `block.code` | `native-equivalent` | `.markra-code-block` | `.protyle-wysiwyg .code-block` | default, hover, focus, readonly |
| `block.math` | `native-equivalent` | `.markra-math-render-display` | `.protyle-wysiwyg [data-subtype="math"]` | default, focus, error |
| `block.mermaid` | `native-equivalent` | `.markra-mermaid-render` | `.protyle-wysiwyg [data-subtype="mermaid"]` | default, focus, error |
| `block.raw-html` | `native-equivalent` | `.markra-html-node` | `.protyle-wysiwyg [data-subtype="html"]` | default, focus, error |
| `inline.strong` | `native-equivalent` | `.cm-markra-strong` | `.protyle-wysiwyg [data-type~="strong"]` | default, selected |
| `inline.emphasis` | `native-equivalent` | `.cm-markra-emphasis` | `.protyle-wysiwyg [data-type~="em"]` | default, selected |
| `inline.strikethrough` | `native-equivalent` | `.cm-markra-strikethrough` | `.protyle-wysiwyg [data-type~="s"]` | default, selected |
| `inline.highlight` | `native-equivalent` | `.cm-markra-highlight` | `.protyle-wysiwyg [data-type~="mark"]` | default, selected |
| `inline.code` | `native-equivalent` | `.cm-markra-inline-code` | `.protyle-wysiwyg [data-type~="code"]` | default, selected |
| `inline.link` | `native-equivalent` | `.cm-markra-link` | `.protyle-wysiwyg [data-type~="a"]` | default, hover, focus, selected |
| `inline.math` | `native-equivalent` | `.markra-math-render-inline` | `.protyle-wysiwyg [data-type~="inline-math"]` | default, focus, selected |
| `media.image` | `native-equivalent` | `.markra-image-node` | `.protyle-wysiwyg .img` | default, hover, selected, drag, error |
| `control.code-language` | `markdown-exclusive` | `.markra-code-language-control` | `.protyle-action__language` | default, hover, focus, expanded, disabled |
| `control.code-actions` | `markdown-exclusive` | `.cm-markra-code-actions` | `.protyle-action .protyle-icon` | default, hover, focus, disabled |
| `control.table-toolbar` | `markdown-exclusive` | `.markra-table-align-controls` | `.block__icons .block__icon` | default, hover, focus, selected, disabled |
| `control.fold` | `markdown-exclusive` | `.markra-heading-toggle-button, .markra-list-toggle-button` | `.protyle-action__arrow` | default, hover, focus, expanded |
| `control.block-toolbar` | `markdown-exclusive` | `.cm-markra-block-toolbar` | `.protyle-gutters .b3-menu__item` | default, hover, focus, drag |
| `control.math-macro` | `markdown-exclusive` | `.markra-math-macro-fold` | `.protyle-action__arrow` | default, hover, focus, expanded |
| `overlay.code-language` | `markdown-exclusive` | `.markra-code-language-popover` | `.protyle-util [data-id="codeLanguage"]` | expanded, focus, selected, empty |
| `overlay.search` | `markdown-exclusive` | `.cm-search` | `.b3-menu` | expanded, focus, selected, empty |
| `overlay.footnote` | `markdown-exclusive` | `.markra-footnote-preview` | `.protyle-util` | expanded, focus, error |
| `overlay.media-viewer` | `markdown-exclusive` | `.markra-media-viewer-dialog` | `.viewer-container` | expanded, focus, disabled |
| `state.syntax-hint` | `markdown-exclusive` | `.cm-markra-syntax-character` | `--b3-theme-on-surface-light` | default, selected |
| `state.trailing-space` | `markdown-exclusive` | `.cm-markra-trailing-space` | `--b3-theme-warning` | default |
| `state.clipboard-progress` | `markdown-exclusive` | `.markra-image-upload-placeholder` | `.b3-progress` | default, error |

---

### Task 1: 建立唯一表现契约目录

**Files:**
- Create: `app/src/markdown/appearance/contracts.json`
- Create: `app/src/markdown/appearance/contracts.ts`
- Create: `app/src/markdown/appearance/contracts.test.ts`
- Create: `app/src/markdown/appearance/testSupport.ts`
- Modify: `app/tsconfig.json`

**Interfaces:**
- Produces: `MarkdownAppearanceContract`、`MarkdownAppearanceState`、`getAppearanceContract(id)`、`listAppearanceContracts()`、`appearanceVariableName(id, property)`。
- Produces: 测试工具 `createTestHostAdapter()`、`mountTestEditor(mode)`、`captureEditorContinuity(view)`、`findIndependentBaseThemeDeclarations(selector)`、`readMarkdownAppearanceSources()`、`collectVisibleMarkraSelectors()`、`isSelectorCoveredByContract(selector, contracts)`、`applyThemeCss(css)`。
- Produces: 上述“契约组件清单”中每个 ID 的唯一 JSON 条目，后续测试、主题解析器和运行态矩阵不得维护第二份组件列表。

- [ ] **Step 1: 写契约校验失败测试**

```ts
import assert from "node:assert/strict";
import test from "node:test";
import {listAppearanceContracts} from "./contracts";

test("appearance contracts are unique and complete", () => {
    const contracts = listAppearanceContracts();
    assert.equal(new Set(contracts.map((item) => item.id)).size, contracts.length);
    assert.deepEqual(new Set(contracts.map((item) => item.category)), new Set([
        "native-equivalent", "editor-foundation", "markdown-exclusive",
    ]));
    for (const contract of contracts) {
        assert.ok(contract.markdownSelector);
        assert.ok(Array.isArray(contract.ownedSelectors));
        assert.ok(contract.reference.selector || contract.reference.variable);
        assert.ok(contract.states.length > 0);
        assert.ok(contract.platforms.length > 0);
        assert.ok(contract.modes.length > 0);
        assert.ok(contract.styleProperties.length > 0 || contract.geometry.length > 0);
        assert.ok(contract.fallbackVariables.every((name) => name.startsWith("--b3-")));
    }
});
```

- [ ] **Step 2: 运行测试并确认因模块不存在而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/contracts.test.ts`

Expected: FAIL，错误指出无法解析 `./contracts`。

- [ ] **Step 3: 添加 JSON 模块支持和严格类型**

在 `app/tsconfig.json` 的 `compilerOptions` 中加入：

```json
"resolveJsonModule": true
```

在 `contracts.ts` 定义：

```ts
import data from "./contracts.json";

export type MarkdownAppearanceCategory = "native-equivalent" | "editor-foundation" | "markdown-exclusive";
export type MarkdownAppearanceMode = "source" | "visual";
export type MarkdownAppearancePlatform = "desktop" | "mobile";
export type MarkdownAppearanceState = "default" | "hover" | "focus" | "selected" | "disabled" |
    "editing" | "readonly" | "empty" | "error" | "expanded" | "drag" | "keyboard";
export type AppearanceGeometryMetric = "bottom" | "contentLeft" | "controlRight" | "height" | "left" |
    "top" | "width";

export interface MarkdownAppearanceContract {
    id: string;
    category: MarkdownAppearanceCategory;
    markdownSelector: string;
    ownedSelectors: string[];
    reference: {kind: "native" | "component" | "variable"; selector?: string; variable?: string};
    states: MarkdownAppearanceState[];
    modes: MarkdownAppearanceMode[];
    platforms: MarkdownAppearancePlatform[];
    styleProperties: string[];
    geometry: AppearanceGeometryMetric[];
    fallbackVariables: string[];
    probe: boolean;
}

const allowedStates = new Set<MarkdownAppearanceState>([
    "default", "hover", "focus", "selected", "disabled", "editing", "readonly", "empty", "error",
    "expanded", "drag", "keyboard",
]);

const validateContracts = (value: unknown): readonly MarkdownAppearanceContract[] => {
    if (!Array.isArray(value)) throw new TypeError("Markdown appearance contracts must be an array");
    const ids = new Set<string>();
    const selectors = new Set<string>();
    return Object.freeze(value.map((candidate) => {
        const item = candidate as MarkdownAppearanceContract;
        if (!item.id || ids.has(item.id)) throw new TypeError(`Invalid appearance contract id: ${item.id}`);
        if (!item.states?.every((state) => allowedStates.has(state))) {
            throw new TypeError(`Invalid appearance states for ${item.id}`);
        }
        if (!item.reference?.selector && !item.reference?.variable) {
            throw new TypeError(`Missing appearance reference for ${item.id}`);
        }
        for (const selector of [item.markdownSelector, ...item.ownedSelectors]) {
            if (selectors.has(selector)) throw new TypeError(`Duplicate appearance selector owner: ${selector}`);
            selectors.add(selector);
        }
        ids.add(item.id);
        return Object.freeze(item);
    }));
};

const contracts = validateContracts(data);

export const listAppearanceContracts = () => contracts;
export const getAppearanceContract = (id: string) => contracts.find((item) => item.id === id);
export const appearanceVariableName = (id: string, property: string) =>
    `--b3-editor-appearance-${id.replaceAll(".", "-")}-${property.replace(/[A-Z]/gu, (letter) => `-${letter.toLowerCase()}`)}`;
```

- [ ] **Step 4: 按固定清单填充全部 JSON 条目**

原生等价条目的 `styleProperties` 使用 `backgroundColor, borderColor, borderRadius, color, fontFamily, fontSize, fontStyle, fontWeight, lineHeight, marginBottom, marginTop, paddingBottom, paddingLeft, paddingRight, paddingTop` 的适用子集；`block.code`、`block.table`、`media.image` 和所有控件额外包含 `boxShadow, height, opacity, width`。每个插件产生的可见子 selector 必须放入唯一父契约的 `ownedSelectors`，不能由两个契约重复拥有。基础界面条目使用对应的标准语义变量；只有第三方主题可能直接覆盖标准 Protyle 选择器的原生等价条目设置 `probe: true`。

```json
{
  "id": "block.code",
  "category": "native-equivalent",
  "markdownSelector": ".markra-code-block",
  "ownedSelectors": [".cm-markra-code-line", ".cm-markra-code-content-line"],
  "reference": {"kind": "native", "selector": ".protyle-wysiwyg .code-block"},
  "states": ["default", "hover", "focus", "readonly"],
  "modes": ["visual"],
  "platforms": ["desktop", "mobile"],
  "styleProperties": ["backgroundColor", "borderColor", "borderRadius", "boxShadow", "color", "fontFamily", "fontSize", "lineHeight", "paddingBottom", "paddingLeft", "paddingRight", "paddingTop"],
  "geometry": ["contentLeft", "controlRight", "height", "top", "width"],
  "fallbackVariables": ["--b3-theme-surface", "--b3-theme-on-background", "--b3-font-family-code"],
  "probe": true
}
```

- [ ] **Step 5: 实现后续任务共用的测试工具**

`testSupport.ts` 使用 `node:fs` 只读取 `app/src/markdown` 与 `_markdown.scss`，不写仓库文件；EditorView 工具在 JSDOM 容器中挂载真实 `createSiyuanMarkraExtension()` 并提供完整的无副作用 `MarkdownHostAdapter`。连续性快照固定读取 view identity、文档、selection、scrollTop 和 `undoDepth(view.state)`。

```ts
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {history, undoDepth} from "@codemirror/commands";
import {EditorState} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import type {MarkdownHostAdapter} from "../markra-core/adapter";
import {createSiyuanMarkraExtension} from "../markraExtension";
import type {MarkdownAppearanceContract} from "./contracts";

export interface EditorContinuitySnapshot {
    view: EditorView;
    document: string;
    selection: {anchor: number; head: number};
    scrollTop: number;
    undoDepth: number;
}

export const captureEditorContinuity = (view: EditorView): EditorContinuitySnapshot => ({
    view,
    document: view.state.doc.toString(),
    selection: {
        anchor: view.state.selection.main.anchor,
        head: view.state.selection.main.head,
    },
    scrollTop: view.scrollDOM.scrollTop,
    undoDepth: undoDepth(view.state),
});

export const applyThemeCss = (css: string) => {
    const element = document.createElement("style");
    element.dataset.appearanceTestTheme = "true";
    element.textContent = css;
    document.head.append(element);
    return () => element.remove();
};

export const createTestHostAdapter = (): MarkdownHostAdapter => ({
    createIcon(_name, className, ownerDocument) {
        const icon = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
        icon.setAttribute("class", className);
        return icon;
    },
    notifyError() {},
    openLink() {},
    positionPopover() {},
    renderMath(source, _displayMode, context) {
        const element = context.ownerDocument.createElement("span");
        element.textContent = source;
        return element;
    },
    async renderMermaid(source, context) {
        const element = context.ownerDocument.createElement("div");
        element.textContent = source;
        return element;
    },
    resolveImageSource: (source) => source,
    async saveClipboardAssets() { return []; },
});

export const mountTestEditor = (mode: "source" | "visual") => {
    const parent = document.body.appendChild(document.createElement("div"));
    const view = new EditorView({
        parent,
        state: EditorState.create({
            doc: "line one\nline two",
            extensions: [history(), createSiyuanMarkraExtension({
                adapter: createTestHostAdapter(),
                documentPath: () => "/appearance-test.md",
                mode,
            })],
        }),
    });
    return {view, destroy: () => { view.destroy(); parent.remove(); }};
};

const appearanceSourcePaths = [
    "src/assets/scss/business/_markdown.scss",
    "src/markdown/markra-core/codemirror/theme.ts",
    "src/markdown/markra-core/codemirror/block-drag.ts",
    "src/markdown/markra-core/codemirror/callout-preview.ts",
    "src/markdown/markra-core/codemirror/clipboard-assets.ts",
    "src/markdown/markra-core/codemirror/code-block.ts",
    "src/markdown/markra-core/codemirror/fold-toggle.ts",
    "src/markdown/markra-core/codemirror/footnote-preview.ts",
    "src/markdown/markra-core/codemirror/horizontal-rule.ts",
    "src/markdown/markra-core/codemirror/image.ts",
    "src/markdown/markra-core/codemirror/math-preview.ts",
    "src/markdown/markra-core/codemirror/raw-html-preview.ts",
    "src/markdown/markra-core/codemirror/search.ts",
    "src/markdown/markra-core/codemirror/selection-hold.ts",
    "src/markdown/markra-core/codemirror/table-fragment-merge.ts",
    "src/markdown/markra-core/codemirror/table.ts",
    "src/markdown/markra-core/codemirror/trailing-space.ts",
    "src/markdown/markra-core/codemirror/typewriter.ts",
] as const;

export const readMarkdownAppearanceSources = () => appearanceSourcePaths.map((path) => ({
    path,
    text: readFileSync(resolve(process.cwd(), path), "utf8"),
}));

export const findIndependentBaseThemeDeclarations = (selector: string) => {
    const anchor = selector.match(/\.[a-z][\w-]*/iu)?.[0] ?? selector;
    const escaped = anchor.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    const visualProperty = /\b(?:background(?:Color)?|border(?:Color|Radius)?|boxShadow|color|font(?:Family|Size|Style|Weight)?|height|lineHeight|margin|opacity|padding|width)\s*:/u;
    const declaration = new RegExp(`["'][^"']*${escaped}[^"']*["']\\s*:\\s*\\{([^}]*)\\}`, "gu");
    return readMarkdownAppearanceSources().filter((file) => file.path.endsWith(".ts")).flatMap((file) =>
        [...file.text.matchAll(declaration)]
            .filter((match) => visualProperty.test(match[1]))
            .map((match) => ({path: file.path, declaration: match[0]}))
    );
};

export const collectVisibleMarkraSelectors = () => [...new Set(readMarkdownAppearanceSources().flatMap((file) =>
    [...file.text.matchAll(/["'](\.(?:cm-)?markra-[\w-]+)/gu)].map((match) => match[1])
))].sort();

export const isSelectorCoveredByContract = (
    selector: string,
    contracts: readonly MarkdownAppearanceContract[],
) => contracts.some((contract) =>
    contract.markdownSelector.includes(selector) || contract.ownedSelectors.includes(selector)
);
```

- [ ] **Step 6: 运行契约测试和类型检查**

Run: `cd app && node --import tsx --test src/markdown/appearance/contracts.test.ts && pnpm run typecheck`

Expected: PASS；JSON 中缺失、重复、非法状态或非 `--b3-*` 回退变量都会使测试失败。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/tsconfig.json app/src/markdown/appearance/contracts.json app/src/markdown/appearance/contracts.ts app/src/markdown/appearance/contracts.test.ts app/src/markdown/appearance/testSupport.ts`

Expected: 无输出；不提交改动。

---

### Task 2: 用真实 Lute BlockDOM 替换手写原生基准

**Files:**
- Create: `app/src/markdown/appearance/fixture.ts`
- Create: `app/src/markdown/appearance/fixture.test.ts`
- Create: `app/scripts/markdownAppearanceFixture.cjs`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Consumes: `listAppearanceContracts()`。
- Produces: `APPEARANCE_FIXTURE_MARKDOWN`、`renderNativeAppearanceFixture(options)`、CJS `installLute(webContents)`。

- [ ] **Step 1: 写夹具结构失败测试**

```ts
import assert from "node:assert/strict";
import test from "node:test";
import {JSDOM} from "jsdom";
import {renderNativeAppearanceFixture} from "./fixture";

test("native fixture preserves the complete Lute code action structure", () => {
    const document = new JSDOM("<!doctype html><body></body>").window.document;
    const root = renderNativeAppearanceFixture({
        document,
        blockDOM: `<div data-type="NodeCodeBlock" class="code-block"><div class="protyle-action"><span class="protyle-action--first protyle-action__language">java</span><span class="fn__flex-1"></span><span class="protyle-icon protyle-action__copy"></span><span class="protyle-icon protyle-action__menu"></span></div><div class="hljs"><div contenteditable="true">const value = 1;</div></div></div>`,
    });
    assert.equal(root.className, "protyle-wysiwyg");
    assert.ok(root.querySelector(".code-block > .protyle-action > .fn__flex-1"));
    assert.ok(root.querySelector(".code-block > .hljs [contenteditable=true]"));
});
```

- [ ] **Step 2: 运行测试并确认因夹具模块不存在而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/fixture.test.ts`

Expected: FAIL，错误指出无法解析 `./fixture`。

- [ ] **Step 3: 实现固定样本和夹具容器**

```ts
export const APPEARANCE_FIXTURE_MARKDOWN = `# Heading 1
## Heading 2
### Heading 3

Paragraph with **strong**, *emphasis*, ~~strike~~, ==mark==, \`inline code\`, [link](https://example.com), and $x^2$.

- list item
- [x] completed task

> quote

---

| Head | Value |
| --- | --- |
| Cell | Text |

\`\`\`java
const message = "theme parity";
if (message) {
    console.log(message);
}
\`\`\`

![image](assets/appearance-contract.png)

$$x^2 + y^2$$`;

export const renderNativeAppearanceFixture = ({document, blockDOM}: {
    document: Document;
    blockDOM: string;
}) => {
    const root = document.createElement("div");
    root.className = "protyle-wysiwyg";
    root.dataset.appearanceFixture = "native";
    root.innerHTML = blockDOM;
    return root;
};
```

- [ ] **Step 4: 在 Electron 中加载仓库自带 Lute 并生成 BlockDOM**

`app/scripts/markdownAppearanceFixture.cjs` 只读取生成物，不修改它：

```js
const fs = require("node:fs");
const path = require("node:path");

const installLute = async (webContents) => {
    const source = fs.readFileSync(path.join(__dirname, "../stage/protyle/js/lute/lute.min.js"), "utf8");
    await webContents.executeJavaScript(`${source}\nvoid 0;`);
};

const markdownToBlockDOM = (webContents, markdown) => webContents.executeJavaScript(`(() => {
    const lute = Lute.New();
    return lute.Md2BlockDOM(${JSON.stringify(markdown)});
})()`);

module.exports = {installLute, markdownToBlockDOM};
```

- [ ] **Step 5: 删除布局测试中的手写原生代码块并改用 Lute 返回值**

先在同一个 BrowserWindow 加载空白 `data:text/html` bootstrap 页，调用 `installLute` 与 `markdownToBlockDOM` 得到字符串，再加载包含该字符串的完整测试页。页面重载会清除 Lute 全局，但已经生成的 BlockDOM 保留在 HTML 中。断言真实结构包含语言、`fn__flex-1`、复制、更多和 `.hljs` 内容容器；原有 `code-parity-editor` 中手写的原生半边必须删除。

```js
assert.equal(await window.webContents.executeJavaScript(`document.querySelectorAll(
    '[data-appearance-fixture="native"] .code-block > .protyle-action > .fn__flex-1'
).length`), 1);
```

- [ ] **Step 6: 运行夹具单元测试和布局测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/fixture.test.ts && pnpm run test:markdown-layout`

Expected: PASS；测试页中的原生基准来自 `Lute.New().Md2BlockDOM()`。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/fixture.ts app/src/markdown/appearance/fixture.test.ts app/scripts/markdownAppearanceFixture.cjs app/scripts/testMarkdownLayout.cjs`

Expected: 无输出；不提交改动。

---

### Task 3: 以有限主题解析器替换开放式样式桥

**Files:**
- Create: `app/src/markdown/appearance/themeResolver.ts`
- Create: `app/src/markdown/appearance/themeResolver.test.ts`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Delete: `app/src/markdown/markdownThemeBridge.ts`
- Delete: `app/src/markdown/markdownThemeBridge.test.ts`

**Interfaces:**
- Consumes: `MarkdownAppearanceContract`、`appearanceVariableName()`。
- Produces: `MarkdownAppearanceSnapshot`、`MarkdownAppearanceHandle`、`acquireMarkdownAppearance(root)`、`refreshMarkdownAppearance(document)`、测试专用 `resolveMarkdownAppearanceForTest(document)`。

- [ ] **Step 1: 写解析顺序、引用计数和最后有效快照测试**

```ts
test("resolver prefers standard variables, probes native selectors, and retains the last valid snapshot", () => {
    const root = document.createElement("div");
    root.className = "markdown-editor";
    document.body.append(root);
    document.documentElement.style.setProperty("--b3-theme-on-background", "rgb(1, 2, 3)");
    const handle = acquireMarkdownAppearance(root);
    refreshMarkdownAppearance(document);
    assert.equal(root.style.getPropertyValue("--b3-editor-appearance-shell-document-color"), "rgb(1, 2, 3)");
    document.documentElement.style.removeProperty("--b3-theme-on-background");
    refreshMarkdownAppearance(document);
    assert.equal(root.style.getPropertyValue("--b3-editor-appearance-shell-document-color"), "rgb(1, 2, 3)");
    handle.release();
    assert.equal(document.querySelectorAll("[data-markdown-appearance-probe]").length, 0);
});
```

- [ ] **Step 2: 运行测试并确认因解析器不存在而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/themeResolver.test.ts`

Expected: FAIL，错误指出无法解析 `./themeResolver`。

- [ ] **Step 3: 实现按 Document 共享、按编辑器根节点应用的解析器**

```ts
export interface MarkdownAppearanceSnapshot {
    revision: number;
    values: Readonly<Record<string, string>>;
}

export interface MarkdownAppearanceHandle {
    refresh(): void;
    release(): void;
}

const resolvers = new WeakMap<Document, MarkdownAppearanceResolver>();

export const acquireMarkdownAppearance = (root: HTMLElement): MarkdownAppearanceHandle => {
    const document = root.ownerDocument;
    let resolver = resolvers.get(document);
    if (!resolver) {
        resolver = new MarkdownAppearanceResolver(document, () => resolvers.delete(document));
        resolvers.set(document, resolver);
    }
    return resolver.acquire(root);
};

export const refreshMarkdownAppearance = (document: Document) => resolvers.get(document)?.refresh();
export const resolveMarkdownAppearanceForTest = (document: Document) =>
    MarkdownAppearanceResolver.resolveOnce(document);
```

解析器对每个契约严格执行：标准变量 → 标准原生选择器计算样式 → `fallbackVariables`。空值、`initial`、`unset`、`revert` 和不可解析结果不覆盖旧快照。`MutationObserver` 只监听根主题属性与 `head` 中样式表变化，使用双 `requestAnimationFrame` 合并刷新；仅当值映射变化时写入每个 `.markdown-editor` 根节点。

- [ ] **Step 4: 使用由真实 BlockDOM 生成的单一探针容器**

探针必须是完整 `.protyle-wysiwyg` 结构，`aria-hidden="true"`、`inert`、固定离屏且不可交互。每个 `probe: true` 条目只读取其 `styleProperties`，不得再维护全局 `STYLE_PROPERTIES` 或 `PROBES` 并批量扩张 `--b3-markdown-*`。

```ts
private createProbe() {
    const root = this.document.createElement("div");
    root.className = "protyle-wysiwyg";
    root.dataset.markdownAppearanceProbe = "true";
    root.setAttribute("aria-hidden", "true");
    root.setAttribute("inert", "");
    const lute = this.window.Lute?.New();
    if (lute) root.innerHTML = lute.Md2BlockDOM(APPEARANCE_FIXTURE_MARKDOWN);
    return root;
}

private readContract(contract: MarkdownAppearanceContract) {
    const reference = this.probe.querySelector<HTMLElement>(contract.reference.selector ?? "");
    if (!reference) return {};
    const computed = this.window.getComputedStyle(reference);
    return Object.fromEntries(contract.styleProperties.flatMap((property) => {
        const value = computed[property as keyof CSSStyleDeclaration];
        return typeof value === "string" && this.isValid(value)
            ? [[appearanceVariableName(contract.id, property), value]]
            : [];
    }));
}
```

Lute 不可用时不创建手写替代 DOM，解析器直接使用标准变量和回退；单元测试通过注入实现 `Md2BlockDOM()` 的 Lute stub 验证探针分支，实际应用矩阵必须使用页面真实 `window.Lute`。

- [ ] **Step 5: 替换 MarkdownEditor 生命周期集成**

将 `releaseThemeBridge: () => void` 改为 `appearanceHandle: MarkdownAppearanceHandle`，构造时执行 `acquireMarkdownAppearance(this.element)`，销毁时执行 `this.appearanceHandle.release()`。`refreshEditorConfig()` 先刷新解析器，再重配置已有 compartment；不得创建新 `EditorView`。

```ts
public refreshEditorConfig() {
    this.appearanceHandle?.refresh();
    if (!this.view) return;
    reconfigureSiyuanMarkraExtension(this.view, this.modeCompartment, {
        adapter: createSiyuanMarkdownAdapter({app: this.app, documentPath: () => this.path}),
        documentPath: () => this.path,
        mode: this.preview ? "visual" : "source",
    });
}
```

- [ ] **Step 6: 删除旧桥和旧测试**

只有 `rg -n "markdownThemeBridge|--b3-markdown-" app/src/markdown app/src/assets/scss/business/_markdown.scss` 的剩余命中全部属于尚未迁移组件时才继续后续任务；`markdownThemeBridge.ts`、其测试和 MarkdownEditor 中旧导入必须在本任务结束前删除。

- [ ] **Step 7: 运行解析器、配置热更新和类型测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/themeResolver.test.ts src/markdown/markdownEditorConfig.test.ts && pnpm run typecheck`

Expected: PASS；测试断言同一 Document 只创建一个探针、最后一个 handle 释放后清理、无效刷新保留旧值、配置刷新保持同一 view。

- [ ] **Step 8: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/themeResolver.ts app/src/markdown/appearance/themeResolver.test.ts app/src/markdown/MarkdownEditor.ts app/src/markdown/markdownThemeBridge.ts app/src/markdown/markdownThemeBridge.test.ts`

Expected: 无输出；不提交改动。

---

### Task 4: 统一编辑器壳层、基础状态和源码模式

**Files:**
- Create: `app/src/assets/scss/util/_editor-appearance.scss`
- Create: `app/src/markdown/appearance/editorFoundation.test.ts`
- Modify: `app/src/assets/scss/component/_typography.scss`
- Modify: `app/src/assets/scss/protyle/_wysiwyg.scss`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/src/markdown/markraExtension.ts`
- Modify: `app/src/markdown/markra-core/codemirror/theme.ts`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Consumes: `acquireMarkdownAppearance()`、`shell.document`、`shell.metadata`、`shell.title`、全部 `editor.*` 契约和 `data-markdown-mode`。
- Produces: SCSS mixin `editor-content-root`、`editor-block-box`、`editor-focus-ring`、`editor-drag-indicator`；`createSiyuanModeAppearanceExtension(mode)`；根属性 `data-markdown-platform`。

- [ ] **Step 1: 写源码/可视化模式基础状态失败测试**

```ts
test("source mode exposes themed gutters while visual mode keeps Protyle geometry", () => {
    const source = mountTestEditor("source");
    const visual = mountTestEditor("visual");
    assert.equal(source.view.dom.dataset.markdownMode, "source");
    assert.ok(source.view.dom.querySelector(".cm-gutters"));
    assert.equal(visual.view.dom.dataset.markdownMode, "visual");
    assert.equal(visual.view.dom.querySelector(".cm-gutters"), null);
    source.destroy();
    visual.destroy();
});

test("shell and editor foundation contracts cover title, placeholder, scroller, and states", () => {
    for (const id of [
        "shell.document", "shell.metadata", "shell.title", "editor.visual", "editor.source", "editor.cursor",
        "editor.selection", "editor.active-line", "editor.gutter", "editor.placeholder", "editor.scroller",
        "editor.drag-indicator", "editor.error",
    ]) {
        assert.ok(getAppearanceContract(id), id);
    }
});
```

- [ ] **Step 2: 运行基础状态测试并确认失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/editorFoundation.test.ts`

Expected: FAIL，缺少 `markdownAppearanceModeFacet` 或模式配置。

- [ ] **Step 3: 提取共享 SCSS mixin**

```scss
@mixin editor-content-root {
  display: flex;
  flex-direction: column;
  font-family: var(--b3-font-family-protyle);
  font-size: var(--b3-font-size-editor);
  font-variant-ligatures: no-common-ligatures;
}

@mixin editor-block-box {
  border-radius: var(--b3-border-radius);
  line-height: 1.625;
  margin: 2px 0;
  padding: 4px;
}

@mixin editor-focus-ring {
  outline: 1px solid var(--b3-theme-primary);
  outline-offset: 1px;
}
```

`.b3-typography`、`.protyle-wysiwyg` 和 `.markdown-editor__surface` 消费这些 mixin；同一属性不得在三处再次写独立数值。

`testMarkdownLayout.cjs` 同时比较 `.markdown-editor__metadata` 对 `.protyle-background`、`.markdown-editor__title` 对 `.protyle-title`、`.cm-placeholder` 对原生 empty 提示、`.cm-scroller` 对 `.protyle-content` 的契约属性与关键几何。

- [ ] **Step 4: 为源码模式增加思源语义化 CodeMirror 基础扩展**

```ts
const sourceModeExtensions: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    EditorView.editorAttributes.of({"data-markdown-mode": "source"}),
];

const visualModeExtensions: Extension[] = [
    EditorView.editorAttributes.of({"data-markdown-mode": "visual"}),
];

export const createSiyuanModeAppearanceExtension = (
    mode: "source" | "visual",
): Extension => mode === "source" ? sourceModeExtensions : visualModeExtensions;
```

源码模式显示行号与活动行；可视化模式隐藏 gutter。两种模式共同使用 `--b3-theme-background`、`--b3-theme-on-background`、`--b3-theme-primary-lightest`、`--b3-list-hover`、`--b3-font-family-code` 和契约变量，删除浏览器默认颜色。

- [ ] **Step 5: 明确桌面/移动根状态且保持模式切换稳定**

`MarkdownEditor.updateLayout()` 设置 `data-markdown-platform="mobile|desktop"`，移动判定复用 `isMobile()`；`setPreview()` 只重配置既有 compartment。测试保存切换前后的 `view` 引用、文档文本、selection、scrollTop 和 history undo depth。

```ts
const before = captureEditorContinuity(editor.view);
editor.element.querySelector<HTMLButtonElement>('[data-type="markdown-source"]')!.click();
const after = captureEditorContinuity(editor.view);
assert.equal(after.view, before.view);
assert.equal(after.document, before.document);
assert.deepEqual(after.selection, before.selection);
assert.equal(after.scrollTop, before.scrollTop);
assert.equal(after.undoDepth, before.undoDepth);
assert.equal(editor.view.dom.dataset.markdownMode, "source");
```

- [ ] **Step 6: 运行基础测试、连续性测试和布局测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/editorFoundation.test.ts src/markdown/markdownEditorConfig.test.ts && pnpm run test:markdown-layout`

Expected: PASS；模式切换只改变模式属性和扩展，不改变编辑状态。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/assets/scss/util/_editor-appearance.scss app/src/assets/scss/component/_typography.scss app/src/assets/scss/protyle/_wysiwyg.scss app/src/assets/scss/business/_markdown.scss app/src/markdown/markraExtension.ts app/src/markdown/markra-core/codemirror/theme.ts app/src/markdown/MarkdownEditor.ts app/scripts/testMarkdownLayout.cjs`

Expected: 无输出；不提交改动。

---

### Task 5: 迁移静态块、列表和行内格式

**Files:**
- Create: `app/src/markdown/appearance/typographyParity.test.ts`
- Modify: `app/src/assets/scss/util/_editor-appearance.scss`
- Modify: `app/src/assets/scss/component/_typography.scss`
- Modify: `app/src/assets/scss/protyle/_wysiwyg.scss`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/src/markdown/markra-core/codemirror/theme.ts`
- Modify: `app/src/markdown/markra-core/codemirror/horizontal-rule.ts`

**Interfaces:**
- Consumes: 契约 ID `block.paragraph`、`block.heading-1..6`、`block.list`、`block.task`、`block.blockquote`、`block.horizontal-rule`、`inline.*`。
- Produces: 共享 mixin `editor-heading($level)`、`editor-inline-format($kind)`、`editor-horizontal-rule`。

- [ ] **Step 1: 写全部静态内容的参数化失败测试**

```ts
for (const id of [
    "block.paragraph", "block.heading-1", "block.heading-2", "block.heading-3", "block.heading-4",
    "block.heading-5", "block.heading-6", "block.list", "block.task", "block.blockquote",
    "block.horizontal-rule", "inline.strong", "inline.emphasis", "inline.strikethrough", "inline.highlight",
    "inline.code", "inline.link", "inline.math",
]) {
    test(`${id} has one contract-driven visual source`, () => {
        const contract = getAppearanceContract(id);
        assert.ok(contract);
        assert.equal(findIndependentBaseThemeDeclarations(contract.markdownSelector).length, 0);
    });
}
```

- [ ] **Step 2: 运行测试并记录现有重复声明导致的失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/typographyParity.test.ts`

Expected: FAIL，至少报告 `markraTheme` 中标题、引用、链接、行内代码、高亮和任务项的独立视觉声明。

- [ ] **Step 3: 将 Protyle 与 Markdown 的公共规则提取为 mixin**

标题字号、字重、行高、上下间距，段落盒模型，列表缩进/引导线，任务状态，引用标记和分割线由 `_editor-appearance.scss` 输出；原生选择器与 Markdown 选择器分别 `@include` 同一个 mixin。第三方主题覆盖由主题解析器变量覆盖 mixin 的标准值。

```scss
@mixin editor-heading($level) {
  color: var(--b3-editor-appearance-block-heading-#{$level}-color, inherit);
  font-family: var(--b3-editor-appearance-block-heading-#{$level}-font-family, inherit);
  font-size: var(--b3-editor-appearance-block-heading-#{$level}-font-size);
  font-weight: var(--b3-editor-appearance-block-heading-#{$level}-font-weight, 600);
  line-height: var(--b3-editor-appearance-block-heading-#{$level}-line-height, 1.625);
}
```

- [ ] **Step 4: 从 `markraTheme` 和插件 `baseTheme` 删除已迁移视觉属性**

`baseTheme` 只保留 CodeMirror 生命周期必需的定位、display 和指针行为；颜色、字体、边框、圆角、间距、阴影和尺寸全部由共享 SCSS/契约变量决定。IME composing 选区隐藏规则保留，因为它是编辑行为状态而不是独立视觉规范。

```ts
export const markraTheme = EditorView.baseTheme({
    '&[data-markra-composing="true"] .cm-selectionBackground': {
        backgroundColor: "transparent !important",
    },
    '&[data-markra-link-modifier="true"] .cm-markra-link': {
        cursor: "pointer",
    },
});
```

- [ ] **Step 5: 在 Electron 布局测试中比较静态内容计算样式与几何**

对每个 ID 比较契约列出的属性，并比较 `top`、`left`、`width`、`height` 与内容起点。使用统一断言：

```js
const assertGeometry = (nativeRect, markdownRect, id) => {
    for (const key of ["left", "top", "width", "height"]) {
        assert.ok(Math.abs(nativeRect[key] - markdownRect[key]) <= 1, `${id}.${key}`);
    }
};
```

- [ ] **Step 6: 运行静态内容测试和布局测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/typographyParity.test.ts && pnpm run test:markdown-layout`

Expected: PASS；上述静态组件不再由 `EditorView.baseTheme` 与 `_markdown.scss` 重复决定视觉属性。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/assets/scss/util/_editor-appearance.scss app/src/assets/scss/component/_typography.scss app/src/assets/scss/protyle/_wysiwyg.scss app/src/assets/scss/business/_markdown.scss app/src/markdown/markra-core/codemirror/theme.ts app/src/markdown/markra-core/codemirror/horizontal-rule.ts app/src/markdown/appearance/typographyParity.test.ts`

Expected: 无输出；不提交改动。

---

### Task 6: 重建代码块结构并共享代码语言菜单

**Files:**
- Create: `app/src/protyle/codeLanguageMenu.ts`
- Create: `app/src/protyle/codeLanguageMenu.test.ts`
- Create: `app/src/markdown/appearance/codeBlockParity.test.ts`
- Modify: `app/src/protyle/toolbar/index.ts`
- Modify: `app/src/markdown/markra-core/adapter.ts`
- Modify: `app/src/markdown/siyuanAdapter.ts`
- Modify: `app/src/markdown/markraExtension.ts`
- Modify: `app/src/markdown/markra-core/codemirror/code-block.ts`
- Modify: `app/src/assets/scss/util/_editor-appearance.scss`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Produces: `mountCodeLanguageMenu(options): CodeLanguageMenuHandle`。
- Produces: `MarkdownHostAdapter.openCodeLanguageMenu?(request): MarkdownControlHandle | null`。
- Consumes: `getCodeLanguages()`、插件 `code-language-update` 事件、思源 `setPosition()`。

- [ ] **Step 1: 写共享菜单 DOM、筛选、插件异常隔离和键盘测试**

```ts
test("code language menu is shared, searchable, keyboard accessible, and plugin-safe", () => {
    const selected: string[] = [];
    const handle = mountCodeLanguageMenu({
        anchor: document.body.appendChild(document.createElement("button")),
        container: document.body,
        currentLanguage: "java",
        languages: ["bash", "java", "javascript"],
        labels: {clear: "Clear", search: "Search"},
        onFilter: ({languages}) => languages,
        onSelect: (value) => selected.push(value),
        position: () => undefined,
    });
    handle.element.querySelector("input")!.value = "java";
    handle.element.querySelector("input")!.dispatchEvent(new Event("input"));
    handle.element.querySelector<HTMLInputElement>("input")!.dispatchEvent(new KeyboardEvent("keydown", {key: "Enter"}));
    assert.deepEqual(selected, ["java"]);
    handle.destroy();
    assert.equal(handle.element.isConnected, false);
});
```

- [ ] **Step 2: 运行菜单测试并确认模块不存在而失败**

Run: `cd app && node --import tsx --test src/protyle/codeLanguageMenu.test.ts`

Expected: FAIL，错误指出无法解析 `./codeLanguageMenu`。

- [ ] **Step 3: 定义共享菜单与宿主请求接口**

```ts
export interface CodeLanguageMenuHandle {
    element: HTMLElement;
    focus(): void;
    destroy(): void;
}

export interface CodeLanguageFilterDetail {
    languages: string[];
    listElement: HTMLElement;
    type: "init" | "match";
    value: string;
}

export interface CodeLanguageMenuOptions {
    anchor: HTMLElement;
    container: HTMLElement;
    currentLanguage: string;
    languages: readonly string[];
    labels: {clear: string; search: string};
    onFilter(detail: CodeLanguageFilterDetail): readonly string[];
    onSelect(language: string): void;
    position(anchor: HTMLElement, popover: HTMLElement): void;
}

export interface MarkdownControlHandle {destroy(): void; focus(): void;}
export interface MarkdownCodeLanguageMenuRequest {
    anchor: HTMLElement;
    currentLanguage: string;
    languages: readonly string[];
    onSelect(language: string): void;
    ownerDocument: Document;
}
```

`MarkdownHostAdapter` 增加 `openCodeLanguageMenu?`；`markra-core` 只发请求并管理返回 handle，不引用 `b3-*` 或 `protyle-*` 类。宿主未提供时保留无思源依赖的可访问列表框回退。

- [ ] **Step 4: 让 Protyle 工具栏和 Markdown 适配器挂载同一个菜单工厂**

`Toolbar.showCodeLanguage()` 负责获取 range 和更新原生块语言，然后调用 `mountCodeLanguageMenu()`；`createSiyuanMarkdownAdapter()` 负责传入思源文案、定位、插件过滤和 Markdown `onSelect`。插件返回值先过滤非字符串、空值和重复项；单个插件抛错时继续显示其余语言。

```ts
const emitCodeLanguageUpdate = (app: App, detail: CodeLanguageFilterDetail) => {
    for (const plugin of app.plugins) {
        try {
            plugin.eventBus.emit("code-language-update", detail);
        } catch (error) {
            console.warn("code-language-update failed", error);
        }
    }
    return [...new Set(detail.languages.filter((item): item is string =>
        typeof item === "string" && item.trim().length > 0
    ))];
};

openCodeLanguageMenu(request) {
    return mountCodeLanguageMenu({
        ...request,
        container: request.anchor.closest(".markdown-editor") ?? request.ownerDocument.body,
        labels: {clear: window.siyuan.languages.clear, search: window.siyuan.languages.search},
        onFilter: (detail) => emitCodeLanguageUpdate(options.app, detail),
        position: (anchor, popover) => {
            const rect = anchor.getBoundingClientRect();
            setPosition(popover, rect.left, rect.bottom, rect.height);
        },
    });
}
```

- [ ] **Step 5: 重建 Markdown 代码块操作栏为原生等价层级**

CodeMirror opening-fence widget 输出 `.protyle-action`，内部顺序固定为语言、`.fn__flex-1`、复制、更多；语言必须位于左侧，操作按钮位于右侧。代码组继续由连续的 `.cm-markra-code-line` 表达，不强行构造跨多行的无效 DOM；围栏标记仍由 CodeMirror decoration 控制，不改变文本、selection 或 fenced-code 解析。

```html
<span class="protyle-action cm-markra-code-actions">
  <button class="protyle-action--first protyle-action__language markra-code-language-control"></button>
  <span class="fn__flex-1"></span>
  <button class="protyle-icon protyle-action__copy"></button>
  <button class="protyle-icon protyle-action__menu"></button>
</span>
```

- [ ] **Step 6: 使用共享代码块 mixin 删除高度模拟**

删除 `.cm-markra-code-top-gap` 和独立 header 固定高度。opening/closing fence 行只作为零高度装饰锚点，首个内容行获得原生 `.hljs` 的 `2em` 顶部空间，最后内容行获得 `1.6em` 底部空间，所有内容行使用 `1em` 左右 padding；操作栏使用原生 `inset`、flex 占位和图标尺寸。Highlight.js token 类保持不变，以继续消费当前代码主题。

```scss
.cm-markra-code-opening-line,
.cm-markra-code-closing-line {
  height: 0;
  min-height: 0;
  padding: 0 1em;
  position: relative;
}

.cm-markra-code-content-first { padding-top: 2em; }
.cm-markra-code-content-last { padding-bottom: 1.6em; }
.cm-markra-code-content-line { padding-left: 1em; padding-right: 1em; }
```

- [ ] **Step 7: 扩充代码块结构、计算样式和几何测试**

比较背景、圆角、字体、字号、行高、四边 padding、语言 `left/top`、操作组 `right/top`、内容起点、总高度和 token 颜色；菜单比较输入框、列表项、focus 项、宽高和键盘行为。每个几何差值必须不超过 `1px`。

- [ ] **Step 8: 运行代码块、菜单、布局和集成测试**

Run: `cd app && node --import tsx --test src/protyle/codeLanguageMenu.test.ts src/markdown/appearance/codeBlockParity.test.ts src/markdown/codeBlockHeader.test.ts src/markdown/codeBlockConfig.test.ts src/markdown/markraIntegration.test.ts && pnpm run test:markdown-layout`

Expected: PASS；真实 Lute 代码块与 Markdown 代码块的语言位置、上下间距、内容起点和总高度满足契约。

- [ ] **Step 9: 检查本任务差异**

Run: `git diff --check -- app/src/protyle/codeLanguageMenu.ts app/src/protyle/codeLanguageMenu.test.ts app/src/protyle/toolbar/index.ts app/src/markdown/markra-core/adapter.ts app/src/markdown/siyuanAdapter.ts app/src/markdown/markraExtension.ts app/src/markdown/markra-core/codemirror/code-block.ts app/src/assets/scss/util/_editor-appearance.scss app/src/assets/scss/business/_markdown.scss app/scripts/testMarkdownLayout.cjs`

Expected: 无输出；不提交改动。

---

### Task 7: 迁移表格内容与编辑工具栏

**Files:**
- Create: `app/src/markdown/appearance/tableParity.test.ts`
- Modify: `app/src/markdown/markra-core/codemirror/table.ts`
- Modify: `app/src/markdown/markra-core/codemirror/table-fragment-merge.ts`
- Modify: `app/src/assets/scss/util/_editor-appearance.scss`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Consumes: 契约 `block.table`、`control.table-toolbar` 和标准 `.block__icon` 状态。
- Produces: `editor-table`、`editor-control-group` SCSS mixin。

- [ ] **Step 1: 写表格内容、溢出和工具栏状态失败测试**

```ts
test("table contract covers content and every visible control state", () => {
    const table = getAppearanceContract("block.table")!;
    const toolbar = getAppearanceContract("control.table-toolbar")!;
    assert.deepEqual(toolbar.states, ["default", "hover", "focus", "selected", "disabled"]);
    assert.ok(table.geometry.includes("width"));
    assert.ok(table.geometry.includes("height"));
    assert.equal(findIndependentBaseThemeDeclarations(table.markdownSelector).length, 0);
});
```

- [ ] **Step 2: 运行测试并确认 `tableTheme` 独立视觉声明导致失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/tableParity.test.ts`

Expected: FAIL，报告 `table.ts` 或 `_markdown.scss` 中重复的颜色、边框、间距和控件尺寸。

- [ ] **Step 3: 将表格盒模型迁移到共享 mixin**

原生 `.table` 与 Markdown `.cm-markra-table-wrap` 共享表格背景、边框、圆角、单元格 padding、表头字重、选中状态和滚动容器规则。宽表只允许 `.markra-table-scroll` 横向滚动，不允许 `.markdown-editor` 根节点产生横向滚动。

```scss
@mixin editor-table {
  border: 1px solid var(--b3-border-color);
  border-collapse: collapse;
  border-radius: var(--b3-border-radius);

  th,
  td {
    border: 1px solid var(--b3-border-color);
    padding: 4px 8px;
  }
}

.markra-table-scroll {
  max-width: 100%;
  overflow-x: auto;
  overflow-y: hidden;
}
```

- [ ] **Step 4: 将工具栏改为思源按钮状态体系**

工具栏按钮复用 `.block__icon` 的图标尺寸、hover、focus、pressed、disabled 和 error 语义；分组容器只保留表格专属排列。`table.ts` 的 `baseTheme` 仅保留装饰定位、拖动命中区和隐藏逻辑。

```scss
.markra-table-control {
  @include editor-control-button;

  &[aria-pressed="true"] {
    color: var(--b3-theme-primary);
  }

  &:disabled {
    opacity: .38;
    pointer-events: none;
  }
}
```

- [ ] **Step 5: 扩充布局测试的窄页签和移动宽度断言**

桌面 `480px`、窄页签 `320px`、移动 `375px` 三种宽度都断言 editor 根无横向溢出、表格 viewport 可滚动、工具栏不越过 surface 右边界、控件状态与标准按钮计算样式一致。

- [ ] **Step 6: 运行表格测试和布局测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/tableParity.test.ts src/markdown/markraIntegration.test.ts && pnpm run test:markdown-layout`

Expected: PASS；三种宽度的根容器均无横向溢出，表格和工具栏满足契约。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/tableParity.test.ts app/src/markdown/markra-core/codemirror/table.ts app/src/markdown/markra-core/codemirror/table-fragment-merge.ts app/src/assets/scss/util/_editor-appearance.scss app/src/assets/scss/business/_markdown.scss app/scripts/testMarkdownLayout.cjs`

Expected: 无输出；不提交改动。

---

### Task 8: 迁移图片、Callout、数学公式、Mermaid 与原始 HTML

**Files:**
- Create: `app/src/markdown/appearance/renderedBlocksParity.test.ts`
- Modify: `app/src/markdown/markra-core/codemirror/image.ts`
- Modify: `app/src/markdown/markra-core/codemirror/callout-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/code-block.ts`
- Modify: `app/src/markdown/markra-core/codemirror/math-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/raw-html-preview.ts`
- Modify: `app/src/markdown/markraExtension.ts`
- Modify: `app/src/markdown/siyuanAdapter.ts`
- Modify: `app/src/assets/scss/util/_editor-appearance.scss`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Consumes: `MarkdownHostAdapter.renderMath()`、`renderMermaid()`、`resolveImageSource()` 和契约 `block.callout`、`block.math`、`block.mermaid`、`block.raw-html`、`media.image`。
- Produces: 统一 `data-appearance-state="loading|ready|error"` 状态。

- [ ] **Step 1: 写渲染块默认、加载、错误、选中和拖拽状态失败测试**

```ts
const expectedStates = new Map([
    ["block.callout", ["default", "expanded", "selected"]],
    ["block.math", ["default", "focus", "error"]],
    ["block.mermaid", ["default", "focus", "error"]],
    ["block.raw-html", ["default", "focus", "error"]],
    ["media.image", ["default", "hover", "selected", "drag", "error"]],
]);

for (const [id, states] of expectedStates) {
    test(`${id} declares every rendered state`, () => {
        const contract = getAppearanceContract(id)!;
        assert.deepEqual(contract.states, states);
    });
}
```

- [ ] **Step 2: 运行测试并确认契约或插件状态缺失而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/renderedBlocksParity.test.ts`

Expected: FAIL，指出缺少显式 error/selected/drag 状态或重复 `baseTheme` 声明。

- [ ] **Step 3: 统一异步渲染状态而不改变渲染语义**

Math、Mermaid 和 raw HTML widget 在宿主渲染开始、完成、抛错时只更新 `data-appearance-state` 和可访问错误文本；思源扩展把 `adapter.renderMermaid()` 传给 `codeBlockPreviewPlugin()`，仍调用现有宿主/核心渲染器，不改变源 Markdown、渲染缓存或资源解析。

```ts
element.dataset.appearanceState = "loading";
try {
    const rendered = await adapter.renderMermaid(source, context);
    element.replaceChildren(rendered);
    element.dataset.appearanceState = "ready";
} catch (error) {
    element.dataset.appearanceState = "error";
    element.setAttribute("aria-label", error instanceof Error ? error.message : "Render error");
}
```

- [ ] **Step 4: 共享内容盒模型并删除插件独立色板**

Callout 复用 Protyle 引用/提示语义，图片复用 `.img` 的居中、选中轮廓和拖动手柄，渲染节点复用 `.render-node` 的背景、边框和 error 色。保留插件所需 absolute/relative、拖动命中区和异步占位 display，不保留独立背景、边框、圆角、阴影、文字色和固定间距。

```scss
@mixin editor-rendered-block {
  background: var(--b3-theme-surface);
  border: 1px solid var(--b3-border-color);
  border-radius: var(--b3-border-radius);
  color: var(--b3-theme-on-background);

  &[data-appearance-state="error"] {
    border-color: var(--b3-theme-error);
    color: var(--b3-theme-error);
  }
}

.markra-image-node-selected .markra-image-frame {
  outline: 1px solid var(--b3-theme-primary);
}
```

- [ ] **Step 5: 增加图片尺寸、拖动手柄和渲染错误几何测试**

断言默认图片不超过 surface，用户指定宽度在宽容器中保持比例，在窄容器中收缩；手柄中心与图片右边界误差不超过 `1px`。渲染错误块与 `.b3-snackbar--error` 使用相同 error 语义色，且不会扩大编辑器 scroll width。

- [ ] **Step 6: 运行渲染块、图片和布局测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/renderedBlocksParity.test.ts src/markdown/markraIntegration.test.ts && pnpm run test:markdown-layout`

Expected: PASS；所有渲染块拥有可测试状态，图片与渲染错误不产生根级横向溢出。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/renderedBlocksParity.test.ts app/src/markdown/markra-core/codemirror/image.ts app/src/markdown/markra-core/codemirror/callout-preview.ts app/src/markdown/markra-core/codemirror/code-block.ts app/src/markdown/markra-core/codemirror/math-preview.ts app/src/markdown/markra-core/codemirror/raw-html-preview.ts app/src/markdown/markraExtension.ts app/src/markdown/siyuanAdapter.ts app/src/assets/scss/util/_editor-appearance.scss app/src/assets/scss/business/_markdown.scss app/scripts/testMarkdownLayout.cjs`

Expected: 无输出；不提交改动。

---

### Task 9: 统一 Markdown 独有浮层、辅助控件与编辑状态

**Files:**
- Create: `app/src/markdown/appearance/markdownControls.test.ts`
- Modify: `app/src/markdown/markra-core/adapter.ts`
- Modify: `app/src/markdown/siyuanAdapter.ts`
- Modify: `app/src/markdown/markra-core/codemirror/search.ts`
- Modify: `app/src/markdown/markra-core/codemirror/footnote-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/fold-toggle.ts`
- Modify: `app/src/markdown/markra-core/codemirror/block-drag.ts`
- Modify: `app/src/markdown/markra-core/codemirror/clipboard-assets.ts`
- Modify: `app/src/markdown/markra-core/codemirror/selection-hold.ts`
- Modify: `app/src/markdown/markra-core/codemirror/trailing-space.ts`
- Modify: `app/src/markdown/markra-core/codemirror/typewriter.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`

**Interfaces:**
- Produces: `MarkdownPopoverRequest`、`MarkdownHostAdapter.mountPopover?(request): MarkdownControlHandle | null` 与 `mountSiyuanMarkdownPopover(request)`。
- Consumes: 标准 `.protyle-util`、`.b3-menu`、`.b3-text-field`、`.b3-list-item`、`.block__icon` 和语义变量。

- [ ] **Step 1: 写宿主浮层与键盘生命周期失败测试**

```ts
test("host popovers restore focus and close exactly once", () => {
    const anchor = document.body.appendChild(document.createElement("button"));
    const handle = mountSiyuanMarkdownPopover({
        anchor,
        content: document.createElement("div"),
        kind: "footnote",
        ownerDocument: document,
        position: () => undefined,
        restoreFocus: true,
    });
    handle.focus();
    handle.destroy();
    handle.destroy();
    assert.equal(document.activeElement, anchor);
});
```

- [ ] **Step 2: 运行测试并确认宿主浮层接口不存在而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/markdownControls.test.ts`

Expected: FAIL，指出 `mountPopover` 未定义或控件仍由插件独立创建思源类 DOM。

- [ ] **Step 3: 增加宿主无关浮层请求**

```ts
export interface MarkdownPopoverRequest {
    anchor: HTMLElement;
    content: HTMLElement;
    kind: "footnote" | "media" | "search";
    ownerDocument: Document;
    restoreFocus: boolean;
}

export interface SiyuanMarkdownPopoverRequest extends MarkdownPopoverRequest {
    position(anchor: HTMLElement, popover: HTMLElement): void;
}

class SiyuanMarkdownPopover implements MarkdownControlHandle {
    private controller = new AbortController();
    private destroyed = false;
    readonly element: HTMLElement;

    constructor(private request: SiyuanMarkdownPopoverRequest) {
        this.element = request.ownerDocument.createElement("div");
        this.element.className = request.kind === "search" ? "b3-menu" : "protyle-util";
        this.element.append(request.content);
        (request.anchor.closest(".markdown-editor") ?? request.ownerDocument.body).append(this.element);
        request.position(request.anchor, this.element);
        request.ownerDocument.addEventListener("keydown", (event) => {
            if (event.key === "Escape") this.destroy();
        }, {signal: this.controller.signal});
        request.ownerDocument.addEventListener("pointerdown", (event) => {
            const target = event.target as Node;
            if (!this.element.contains(target) && !request.anchor.contains(target)) this.destroy();
        }, {capture: true, signal: this.controller.signal});
    }

    focus() {
        (this.element.querySelector<HTMLElement>("input, button, [tabindex]") ?? this.element).focus();
    }

    destroy() {
        if (this.destroyed) return;
        this.destroyed = true;
        this.controller.abort();
        this.element.remove();
        if (this.request.restoreFocus) this.request.anchor.focus();
    }
}

export const mountSiyuanMarkdownPopover = (
    request: SiyuanMarkdownPopoverRequest,
): MarkdownControlHandle => new SiyuanMarkdownPopover(request);
```

`markra-core` 仅提供内容和语义 kind；思源适配器创建标准外壳、调用 `setPosition`、管理 z-index、outside click、Escape 和 focus restore。宿主方法缺失时使用 `markra-core/shared/popover.ts` 的几何算法和 ARIA dialog/listbox 回退。

- [ ] **Step 4: 迁移搜索、脚注、折叠、媒体查看器和编辑辅助状态**

搜索浮层使用 `.b3-menu`/`.b3-text-field`，脚注使用 `.protyle-util`，折叠使用 `.protyle-action__arrow` 状态，媒体查看器复用现有 viewer 类。拖拽、粘贴进度、selection hold、尾随空格和打字机活动行仅保留行为/定位规则，视觉属性使用契约变量。

```ts
const foldToggleTheme = EditorView.baseTheme({
    ".markra-heading-toggle-button, .markra-list-toggle-button": {
        cursor: "pointer",
        display: "inline-block",
    },
    '.markra-heading-toggle-button[data-collapsed="true"], .markra-list-toggle-button[data-collapsed="true"]': {
        transform: "rotate(-90deg)",
    },
});
```

```scss
.markra-heading-toggle-button,
.markra-list-toggle-button {
  @include editor-control-button;
}

.markra-block-drop-indicator {
  background: var(--b3-theme-primary-lighter);
  height: 4px;
}
```

- [ ] **Step 5: 覆盖 hover、focus、selected、disabled、expanded、drag、error 和键盘状态**

测试按契约逐状态触发真实 DOM event，断言 `aria-expanded`、`aria-selected`、`aria-disabled`、focus owner 和计算样式。Escape 必须关闭浮层并回到 anchor；上下键必须移动 `.b3-list-item--focus`；Enter 只执行当前项一次。

- [ ] **Step 6: 运行控件、适配器和 Markra 集成测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/markdownControls.test.ts src/markdown/markraIntegration.test.ts`

Expected: PASS；Markdown 独有控件不再维护独立思源仿制样式，宿主缺失时仍可键盘操作。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/markdownControls.test.ts app/src/markdown/markra-core/adapter.ts app/src/markdown/siyuanAdapter.ts app/src/markdown/markra-core/codemirror/search.ts app/src/markdown/markra-core/codemirror/footnote-preview.ts app/src/markdown/markra-core/codemirror/fold-toggle.ts app/src/markdown/markra-core/codemirror/block-drag.ts app/src/markdown/markra-core/codemirror/clipboard-assets.ts app/src/markdown/markra-core/codemirror/selection-hold.ts app/src/markdown/markra-core/codemirror/trailing-space.ts app/src/markdown/markra-core/codemirror/typewriter.ts app/src/assets/scss/business/_markdown.scss`

Expected: 无输出；不提交改动。

---

### Task 10: 完成移动端与标准第三方主题兼容

**Files:**
- Create: `app/src/markdown/appearance/platformThemeMatrix.test.ts`
- Modify: `app/src/markdown/appearance/themeResolver.ts`
- Modify: `app/src/assets/scss/util/_editor-appearance.scss`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Modify: `app/scripts/testMarkdownLayout.cjs`

**Interfaces:**
- Consumes: 契约 `platforms`、`fallbackVariables`、`probe` 与 `data-markdown-platform`。
- Produces: 测试主题 `STANDARD_THIRD_PARTY_THEME_CSS`，同时覆盖标准变量和标准 Protyle 选择器。

- [ ] **Step 1: 写主题优先级与平台矩阵失败测试**

```ts
test("standard third-party theme supports variables and Protyle selector probes", () => {
    applyThemeCss(`:root { --b3-theme-on-background: rgb(10, 20, 30); }
        .protyle-wysiwyg .code-block { border-radius: 13px; }`);
    const snapshot = resolveMarkdownAppearanceForTest(document);
    assert.equal(snapshot.values["--b3-editor-appearance-shell-document-color"], "rgb(10, 20, 30)");
    assert.equal(snapshot.values["--b3-editor-appearance-block-code-border-radius"], "13px");
});
```

- [ ] **Step 2: 运行测试并确认平台/第三方覆盖不足而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/platformThemeMatrix.test.ts`

Expected: FAIL，指出标准 Protyle selector 覆盖未进入快照或移动规则没有契约映射。

- [ ] **Step 3: 实现第三方主题优先级和失败隔离**

解析器先读标准变量；契约 `probe: true` 时允许原生计算样式覆盖该组件无法由变量表达的属性；无效值保留上一快照，首次无有效值则按 `fallbackVariables` 顺序读取。单条目错误记录 `{contractId, state, property, reason}` 到 development diagnostics，不抛出到编辑器。

- [ ] **Step 4: 用同一 SCSS 规则表达桌面与移动差异**

移动端只在 `[data-markdown-platform="mobile"]` 和 `@media (hover: none)` 中调整页面边距、触控入口常显、浮层边界和控件命中区；不复制颜色、字体、表格、代码块或排版规则。控件可见尺寸与当前思源移动 `.block__icon`/`.b3-list-item` 计算尺寸一致，不写另一套固定尺寸。

```scss
.markdown-editor[data-markdown-platform="mobile"] {
  .markdown-editor__body { padding-inline: 8px; }
  .cm-markra-code-actions,
  .markra-table-align-controls,
  .cm-markra-block-toolbar { opacity: 1; pointer-events: auto; }
  .protyle-util,
  .b3-menu { max-width: calc(100vw - 16px); }
}

@media (hover: none) {
  .markdown-editor .protyle-icon,
  .markdown-editor .block__icon { opacity: 1; }
}
```

- [ ] **Step 5: 扩充 `500/320/375px` 三宽度和 hover/touch 测试**

布局测试断言桌面常规、桌面窄页签、移动宽度的 body padding、可用宽度、根 scrollWidth、语言菜单边界、表格 viewport 和图片边界；触控模式下无 hover 依赖的入口必须可见。

- [ ] **Step 6: 运行平台、主题解析和布局测试**

Run: `cd app && node --import tsx --test src/markdown/appearance/platformThemeMatrix.test.ts src/markdown/appearance/themeResolver.test.ts && pnpm run test:markdown-layout`

Expected: PASS；内置语义变量、标准 Protyle selector 和移动触控状态均命中同一契约。

- [ ] **Step 7: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/platformThemeMatrix.test.ts app/src/markdown/appearance/themeResolver.ts app/src/assets/scss/util/_editor-appearance.scss app/src/assets/scss/business/_markdown.scss app/src/markdown/MarkdownEditor.ts app/scripts/testMarkdownLayout.cjs`

Expected: 无输出；不提交改动。

---

### Task 11: 建立实际应用运行态矩阵与截图证据

**Files:**
- Create: `app/src/markdown/appearance/runtimeHarness.ts`
- Create: `app/src/markdown/appearance/runtimeHarness.test.ts`
- Create: `app/scripts/testMarkdownAppearance.cjs`
- Modify: `app/src/index.ts`
- Modify: `app/src/types/index.d.ts`
- Modify: `app/package.json`
- Modify: `app/scripts/testMarkdownLivePreview.cjs`

**Interfaces:**
- Produces development-only `window.__siyuanMarkdownAppearanceTest.mount(options)`、`setTheme(mode)`、`measure()`、`interact()`、`destroy()`，以及测试函数 `captureApplicationAppearance(document, appearance)`。
- Consumes: 实际 `createSiyuanMarkraExtension()`、`createSiyuanMarkdownAdapter()`、`Lute.New().Md2BlockDOM()`、`loadAssets()` 和唯一契约目录。

- [ ] **Step 1: 写运行态夹具不落盘和销毁恢复测试**

```ts
test("runtime harness mounts isolated fixtures and restores application appearance", async () => {
    const previousSiyuan = window.siyuan;
    const appearance = {mode: 0, themeLight: "daylight", themeDark: "midnight"} as Config.IAppearance;
    window.siyuan = {
        config: {appearance, editor: {}},
        languages: {clear: "Clear", search: "Search"},
    } as typeof window.siyuan;
    try {
        const initial = captureApplicationAppearance(document, appearance);
        const app = {plugins: []} as unknown as App;
        const harness = installMarkdownAppearanceRuntimeHarness(app);
        const fixture = await harness.mount({markdown: APPEARANCE_FIXTURE_MARKDOWN, mode: "visual", width: 500});
        assert.ok(fixture.root.querySelector('[data-appearance-fixture="native"]'));
        assert.ok(fixture.root.querySelector('[data-markdown-mode="visual"]'));
        await fixture.destroy();
        assert.deepEqual(captureApplicationAppearance(document, appearance), initial);
    } finally {
        window.siyuan = previousSiyuan;
    }
});
```

- [ ] **Step 2: 运行测试并确认运行态入口不存在而失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/runtimeHarness.test.ts`

Expected: FAIL，错误指出 `installMarkdownAppearanceRuntimeHarness` 未定义。

- [ ] **Step 3: 实现仅开发环境安装的并排夹具**

`runtimeHarness.ts` 在一个临时 overlay 中创建真实 `.protyle-wysiwyg` BlockDOM 和真实 CodeMirror `EditorView`；不调用 `/api/markdown/create`、`/save` 或 `/remove`，不读取或修改当前用户文档。`index.ts` 只在 `process.env.NODE_ENV === "development"` 时安装入口。

```ts
if (process.env.NODE_ENV === "development") {
    installMarkdownAppearanceRuntimeHarness(app);
}
```

`destroy()` 必须销毁 view、移除 overlay/测试 style、恢复根主题属性、appearance 配置对象、theme link href、focus 和页面滚动位置。

- [ ] **Step 4: 为矩阵提供结构化测量结果**

```ts
export interface AppearanceMeasurement {
    contractId: string;
    state: MarkdownAppearanceState;
    native: {styles: Record<string, string>; rect: DOMRectReadOnly} | null;
    markdown: {styles: Record<string, string>; rect: DOMRectReadOnly};
    styleDiffs: Record<string, {expected: string; actual: string}>;
    geometryDiffs: Record<string, number>;
    fallback?: string;
}

export interface RuntimeAppearanceReport {
    contractCount: number;
    matrix: Array<{theme: string; mode: "source" | "visual"; platform: "desktop" | "mobile"; width: number; input: string}>;
    maximumGeometryDifference: number;
    outputDirectory: string;
    fallbacks: Array<{contractId: string; property: string; reason: string}>;
    uncovered: string[];
    measurements: AppearanceMeasurement[];
}
```

`measure()` 遍历 `contracts.json`，原生等价内容比较 native，Markdown 独有功能比较标准 component；任何适用契约缺少元素必须作为失败，而不是跳过。

- [ ] **Step 5: 实现 CDP 驱动器和固定矩阵**

`testMarkdownAppearance.cjs` 复用 `testMarkdownLivePreview.cjs` 的 `connect()`/`evaluate()`，要求正在运行且以 `--remote-debugging-port=9222` 启动的轻语页面。矩阵固定为：

```js
const matrix = [
    {theme: "daylight", mode: "visual", platform: "desktop", width: 500, input: "mouse"},
    {theme: "midnight", mode: "visual", platform: "desktop", width: 500, input: "keyboard"},
    {theme: "standard-third-party", mode: "visual", platform: "desktop", width: 320, input: "mouse"},
    {theme: "daylight", mode: "visual", platform: "mobile", width: 375, input: "touch"},
    {theme: "midnight", mode: "source", platform: "desktop", width: 500, input: "keyboard"},
    {theme: "standard-third-party", mode: "source", platform: "mobile", width: 375, input: "touch"},
];
```

每行执行 default、hover/touch-visible、focus、selected、disabled/readonly、expanded、empty、error、drag 中适用状态，并保存 `{matrix-row}.json` 与 `{matrix-row}.png` 到 `fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-markdown-appearance-"))`。

```js
const outputDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-markdown-appearance-"));
try {
    for (const [index, row] of matrix.entries()) {
        await evaluate(call, `window.__siyuanMarkdownAppearanceTest.mount(${JSON.stringify(row)})`);
        const measurement = await evaluate(call, "window.__siyuanMarkdownAppearanceTest.measure()");
        fs.writeFileSync(path.join(outputDirectory, `${index}.json`), JSON.stringify(measurement, null, 2));
        const screenshot = await call("Page.captureScreenshot", {format: "png", fromSurface: true});
        fs.writeFileSync(path.join(outputDirectory, `${index}.png`), screenshot.data, "base64");
        await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
    }
} finally {
    await evaluate(call, "window.__siyuanMarkdownAppearanceTest.destroy()");
}
```

- [ ] **Step 6: 加入热更新与编辑连续性测试**

在临时 view 中设置非空 selection、scrollTop 和 undo history，切换主题、配置和 source/visual；断言 view identity、文档、selection、scrollTop、undo depth 和 mode 仅按请求变化。实际 MarkdownEditor 的 dirty 保持由 `markdownEditorConfig.test.ts` 使用测试实例覆盖，运行态夹具不触碰用户文档。

- [ ] **Step 7: 添加 package script 并执行运行态矩阵**

在 `app/package.json` 增加：

```json
"test:markdown-appearance": "node ./scripts/testMarkdownAppearance.cjs"
```

Run: `cd app && pnpm run test:markdown-appearance`

Expected: PASS；控制台打印报告目录、全部矩阵行数量、契约数量、截图数量和第三方回退项；任一 style diff、超过 `1px` 的 geometry diff、缺失组件或未恢复应用状态都会非零退出。

- [ ] **Step 8: 运行既有实际应用模式切换测试**

Run: `cd app && pnpm run test:markdown-live-preview`

Expected: PASS；同一 view 在 source/visual 切换中保持文档和 selection。

- [ ] **Step 9: 检查本任务差异**

Run: `git diff --check -- app/src/markdown/appearance/runtimeHarness.ts app/src/markdown/appearance/runtimeHarness.test.ts app/scripts/testMarkdownAppearance.cjs app/src/index.ts app/src/types/index.d.ts app/package.json app/scripts/testMarkdownLivePreview.cjs`

Expected: 无输出；不提交改动。

---

### Task 12: 删除旧视觉源、冻结契约并完成全量验收

**Files:**
- Create: `app/src/markdown/appearance/styleSourcePolicy.test.ts`
- Create: `docs/superpowers/verification/2026-08-13-markdown-editor-appearance-contract.md`
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/src/markdown/markra-core/codemirror/theme.ts`
- Modify: `app/src/markdown/markra-core/codemirror/block-drag.ts`
- Modify: `app/src/markdown/markra-core/codemirror/callout-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/clipboard-assets.ts`
- Modify: `app/src/markdown/markra-core/codemirror/code-block.ts`
- Modify: `app/src/markdown/markra-core/codemirror/fold-toggle.ts`
- Modify: `app/src/markdown/markra-core/codemirror/footnote-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/horizontal-rule.ts`
- Modify: `app/src/markdown/markra-core/codemirror/image.ts`
- Modify: `app/src/markdown/markra-core/codemirror/math-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/raw-html-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/search.ts`
- Modify: `app/src/markdown/markra-core/codemirror/selection-hold.ts`
- Modify: `app/src/markdown/markra-core/codemirror/table-fragment-merge.ts`
- Modify: `app/src/markdown/markra-core/codemirror/table.ts`
- Modify: `app/src/markdown/markra-core/codemirror/trailing-space.ts`
- Modify: `app/src/markdown/markra-core/codemirror/typewriter.ts`

**Interfaces:**
- Consumes: 全部契约、全部任务测试、运行态 JSON/PNG 输出。
- Produces: 阻止表现源再次分叉的策略测试和最终验证报告。

- [ ] **Step 1: 写样式源约束失败测试**

```ts
test("Markdown appearance has no independent palette or legacy bridge variables", async () => {
    const files = await readMarkdownAppearanceSources();
    for (const file of files) {
        assert.doesNotMatch(file.text, /--b3-markdown-/u, file.path);
        assert.doesNotMatch(file.text, /#[\da-f]{3,8}\b/iu, file.path);
        assert.doesNotMatch(file.text, /\brgba?\s*\(/iu, file.path);
        assert.doesNotMatch(file.text, /\b(?:Canvas|CanvasText|Highlight|HighlightText)\b/u, file.path);
    }
});

test("every visible Markra selector belongs to an appearance contract", async () => {
    const visibleSelectors = await collectVisibleMarkraSelectors();
    const contracts = listAppearanceContracts();
    assert.deepEqual(visibleSelectors.filter((selector) => !isSelectorCoveredByContract(selector, contracts)), []);
});
```

- [ ] **Step 2: 运行策略测试并确认剩余旧变量/独立色板导致失败**

Run: `cd app && node --import tsx --test src/markdown/appearance/styleSourcePolicy.test.ts`

Expected: FAIL，并精确列出仍存在的旧变量、裸颜色或未登记可见 selector。

- [ ] **Step 3: 逐命中删除旧表现源**

删除 `_markdown.scss` 中已由共享 mixin/契约决定的重复属性，删除所有 `--b3-markdown-*`，删除各插件 `baseTheme` 中的颜色、字体、边框、圆角、间距、阴影和控件尺寸。保留的 `baseTheme` 只能包含 CodeMirror 必需的 display、position、pointer-events、overflow、z-index 和编辑状态切换；策略测试对白名单逐项写明文件、selector、property，不允许通配整个文件。

- [ ] **Step 4: 运行全部 Markdown 单元测试**

Run: `cd app && node --import tsx --test src/markdown/*.test.ts src/markdown/appearance/*.test.ts src/protyle/codeLanguageMenu.test.ts`

Expected: PASS；无 skipped、cancelled 或 only 测试。

- [ ] **Step 5: 运行布局与实际应用矩阵**

Run: `cd app && pnpm run test:markdown-layout && pnpm run test:markdown-live-preview && pnpm run test:markdown-appearance`

Expected: PASS；矩阵覆盖 visual/source、daylight/midnight/standard-third-party、500/320/375px、mouse/keyboard/touch 和全部适用交互状态。

- [ ] **Step 6: 运行项目规定的前端验证**

Run: `cd app && pnpm run lint`

Expected: PASS；该命令包含 typecheck 和 ESLint。不要运行 `pnpm build`、`pnpm dev` 或 `npx webpack`。

- [ ] **Step 7: 写最终验证报告**

报告必须逐项记录：冻结的 Git diff 摘要、执行命令及退出码、矩阵行与契约计数、JSON/PNG 绝对路径、几何最大差值、第三方主题回退条目、未覆盖项。先用下列结构从运行态 JSON 读取实际值，再使用 `apply_patch` 把实际值写入验证文档，不使用脚本直接写仓库文件。只有未覆盖项为空且所有命令通过时，结论写“整体表现契约验收通过”。

```ts
const runtime = JSON.parse(readFileSync(runtimeReportPath, "utf8")) as RuntimeAppearanceReport;
const verification = `# Markdown 编辑器整体表现契约验证

- 契约数量：${runtime.contractCount}
- 矩阵行：${runtime.matrix.length}
- 最大几何差值：${runtime.maximumGeometryDifference}px
- 运行态报告：${runtime.outputDirectory}
- 第三方主题回退：${runtime.fallbacks.length === 0 ? "无" : runtime.fallbacks
    .map((item) => `${item.contractId} / ${item.property} / ${item.reason}`).join("；")}
- 未覆盖项：${runtime.uncovered.length === 0 ? "无" : runtime.uncovered.join("；")}
`;
```

- [ ] **Step 8: 冻结最终树并做一致性检查**

Run: `git diff --check && rg -n "markdownThemeBridge|--b3-markdown-|markdown-theme-probe" app/src app/scripts`

Expected: `git diff --check` 无输出；`rg` 无命中。记录 `git status --short` 与 `git diff --stat` 到验证报告，不提交、不推送。

---

## 执行顺序与评审关口

- Task 1–3 建立契约、真实基准和主题数据流；未通过前不得迁移具体组件。
- Task 4–5 固定编辑器基础、源码模式和静态排版；这是后续复杂块的几何基线。
- Task 6–9 逐类迁移复杂块和控件；每个任务结束时对应契约组必须独立通过。
- Task 10 验证同一契约在移动端和标准第三方主题下生效，不接受复制主题规则。
- Task 11 提供实际应用证据；没有该证据时不得依据 JSDOM 或手写 DOM 宣称 UI 完成。
- Task 12 删除过渡层并运行冻结后的全量验证；任何后续样式改动都会使此前运行态证据失效，必须重跑受影响矩阵。

## 完成定义

- `contracts.json` 中每个组件均有结构、样式、几何和适用状态证据。
- 原生等价组件关键几何误差不超过 `1px`，计算样式无差异。
- Markdown 独有组件与其引用的思源标准组件状态一致。
- 可视化/源码、桌面/移动、内置明暗/标准第三方主题全部通过。
- 主题、配置和模式热更新保持 view、文档、选区、滚动、撤销和脏状态。
- `markdownThemeBridge`、`--b3-markdown-*`、重复视觉 `baseTheme` 和手写简化原生基准全部移除。
- 最终验证报告列出运行态 JSON/PNG 路径和所有第三方回退；未覆盖项为空。
- 所有改动保持未提交，等待用户另行授权 Git 操作。
