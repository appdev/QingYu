# Markdown 表格工具栏分组实施计划

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Markdown 所见即所得表格工具栏调整为已确认的 C1 四组布局，在不改变业务行为的前提下统一按钮尺寸、间距、主题状态与窄宽度表现。

**Architecture:** 保留 `table.ts` 中现有 CodeMirror Widget、按钮事件和思源图标创建逻辑，只为尺寸、对齐、列宽和删除操作增加明确的布局容器。Markra 基础主题负责结构尺寸和可见性，思源业务样式负责主题颜色与交互状态；Node DOM 测试验证语义结构，Electron 样式测试验证真实 CSS 布局。

**Tech Stack:** TypeScript、CodeMirror 6、SCSS、Node.js Test Runner、LinkeDOM、Electron、Sass、pnpm。

## Global Constraints

- 保持现有思源 SVG 精灵图标，不手写 SVG，不新增图标依赖。
- 不修改表格尺寸、对齐、列宽、增删行列、删除表格及 Markdown 持久化逻辑。
- 保留按钮现有 `aria-label`、`title`、`aria-pressed`、`aria-expanded` 和键盘焦点顺序。
- 工具栏在 320px 内容宽度内保持单行、完整可见，不换行、不产生自身横向滚动、不撑宽页签。
- 只修改本计划列出的文件，不触碰工作区中的其他用户修改。
- 按项目规则使用 `cd app && pnpm run lint` 验证，不运行 `pnpm build`，不编译或重启 Kernel。
- 未获得单独授权，不执行 `git commit` 或 `git push`。

## File Structure

- `app/src/markdown/markraIntegration.test.ts`：为真实 Markra 表格 Widget 增加四组结构、顺序、按钮归属和状态回归测试。
- `app/src/markdown/markra-core/codemirror/table.ts`：建立四个纯布局分组容器，并定义与字体缩放无关的紧凑基础尺寸。
- `app/src/assets/scss/business/_markdown.scss`：应用思源主题背景、边框、选中态、焦点态和删除警示态。
- `app/scripts/testMarkdownLayout.cjs`：在 Electron 中加载真实 SCSS，验证 320px 窄内容区域的尺寸、间距、单行和无溢出表现。

---

### Task 1: 用回归测试固定四组语义结构

**Files:**
- Modify: `app/src/markdown/markraIntegration.test.ts:91-105`

- [x] **Step 1: 添加当前实现会失败的四组结构测试**

在现有思源图标测试后添加测试，直接检查工具栏子元素顺序和每组按钮数量：

```ts
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
    assert.equal(toolbar.querySelector('[data-table-control-group="alignment"] [aria-pressed="true"]')?.classList.contains("markra-table-align-left"), true);
    assert.equal(toolbar.querySelector('[data-table-control-group="width"] [aria-pressed="true"]'), null);
});
```

- [x] **Step 2: 运行定向测试并确认按预期失败**

Run: `cd app && pnpm test -- src/markdown/markraIntegration.test.ts`

Expected: 新测试失败；当前 `.markra-table-align-controls` 的直接子元素不是四个带 `data-table-control-group` 的容器。

### Task 2: 建立四组 DOM，保持现有行为和焦点顺序

**Files:**
- Modify: `app/src/markdown/markra-core/codemirror/table.ts:1375-1585`

- [x] **Step 1: 创建并标记四个布局容器**

在 `toDOM` 的元素声明和类名初始化中保留 `alignControls` 作为工具栏根节点，增加三个分组容器，并为四组写入稳定语义：

```ts
const alignControls = document.createElement("span");
const sizeControls = document.createElement("span");
const alignmentControls = document.createElement("span");
const widthControls = document.createElement("span");
const deleteControls = document.createElement("span");

alignControls.className = "markra-table-align-controls";
sizeControls.className = "markra-table-control-group markra-table-size-controls";
alignmentControls.className = "markra-table-control-group markra-table-alignment-controls";
widthControls.className = "markra-table-control-group markra-table-width-controls";
deleteControls.className = "markra-table-control-group markra-table-delete-controls";
sizeControls.dataset.tableControlGroup = "size";
alignmentControls.dataset.tableControlGroup = "alignment";
widthControls.dataset.tableControlGroup = "width";
deleteControls.dataset.tableControlGroup = "delete";
```

- [x] **Step 2: 按原有按钮顺序装入四组**

替换当前扁平追加逻辑，不重新创建按钮、不移动事件绑定：

```ts
sizeControls.append(sizeButton);
alignmentControls.append(...alignButtons);
widthControls.append(widthModeButton);
deleteControls.append(deleteTableButton);
alignControls.append(sizeControls, alignmentControls, widthControls, deleteControls);
```

这样键盘焦点顺序仍为尺寸、左、中、右、列宽、删除。

- [x] **Step 3: 运行结构和图标测试**

Run: `cd app && pnpm test -- src/markdown/markraIntegration.test.ts`

Expected: 新增四组结构测试和原有思源图标、`aria-pressed` 测试全部通过。

### Task 3: 实现 C1 紧凑布局和思源主题状态

**Files:**
- Modify: `app/src/markdown/markra-core/codemirror/table.ts:1695-1740`
- Modify: `app/src/assets/scss/business/_markdown.scss:305-330`

- [x] **Step 1: 在 Markra 基础主题中定义结构尺寸**

调整 `.markra-table-align-controls` 与按钮基础规则，加入分组布局：

```ts
".markra-table-align-controls": {
  alignItems: "center",
  display: "flex",
  gap: "8px",
  left: "0",
  maxWidth: "100%",
  position: "absolute",
  top: "0",
  zIndex: "10",
},
".markra-table-control-group": {
  alignItems: "center",
  display: "inline-flex",
  flex: "0 0 auto",
  gap: "1px",
  padding: "3px",
},
".markra-table-control": {
  alignItems: "center",
  borderRadius: "6px",
  cursor: "pointer",
  display: "inline-flex",
  height: "28px",
  justifyContent: "center",
  opacity: "0",
  padding: "0",
  pointerEvents: "none",
  position: "absolute",
  transition: "opacity 150ms ease",
  width: "28px",
},
".markra-table-control-icon": {
  height: "16px",
  width: "16px",
},
```

同时让工具栏内六个按钮继续使用 `position: "static"`，并删除按钮自身的胶囊圆角和 `scale` 过渡。表格周围的行列增删按钮继续使用原有圆形定位规则。

- [x] **Step 2: 在思源 SCSS 中实现分组和状态颜色**

将每个按钮独立边框改为组边框；保持默认中性，选中态使用主色，删除仅在交互时使用错误色：

```scss
.markra-table-control-group {
  background-color: var(--b3-theme-background);
  border: 1px solid var(--b3-border-color);
  border-radius: 8px;
  box-sizing: border-box;
}

.markra-table-align-controls .markra-table-control {
  background-color: transparent;
  border: 0;
  color: var(--b3-theme-on-surface);

  &:hover,
  &:focus-visible,
  &[aria-pressed="true"],
  &[aria-expanded="true"] {
    background-color: var(--b3-list-hover);
    color: var(--b3-theme-primary);
  }
}

.markra-table-delete-table:hover,
.markra-table-delete-table:focus-visible {
  color: var(--b3-theme-error);
}
```

表格边缘的增删行列圆形按钮保留原有单按钮边框与主题反馈，避免这次纯工具栏调整影响其可点击边界。

- [x] **Step 3: 运行定向 DOM 测试和类型检查**

Run: `cd app && pnpm test -- src/markdown/markraIntegration.test.ts && pnpm run typecheck`

Expected: 两个命令均以退出码 0 完成。

### Task 4: 用 Electron 固定 320px 窄宽度和真实主题样式

**Files:**
- Modify: `app/scripts/testMarkdownLayout.cjs:12-75`
- Modify: `app/scripts/testMarkdownLayout.cjs:70-170`

- [x] **Step 1: 为布局页面增加真实四组工具栏夹具**

在 320px 宽的独立编辑器中加入与生产 DOM 相同的结构，并直接复用思源图标类名：

```html
<div class="protyle markdown-editor" id="toolbar-editor" style="width:320px">
  <div class="markdown-editor__surface b3-typography">
    <div class="cm-markra-table-wrap markra-table-controls-wrapper">
      <span class="markra-table-align-controls">
        <span class="markra-table-control-group markra-table-size-controls" data-table-control-group="size"><button class="markra-table-control markra-table-size-button"><svg class="markra-table-control-icon"></svg></button></span>
        <span class="markra-table-control-group markra-table-alignment-controls" data-table-control-group="alignment"><button class="markra-table-control markra-table-align-button" aria-pressed="true"><svg class="markra-table-control-icon"></svg></button><button class="markra-table-control markra-table-align-button"><svg class="markra-table-control-icon"></svg></button><button class="markra-table-control markra-table-align-button"><svg class="markra-table-control-icon"></svg></button></span>
        <span class="markra-table-control-group markra-table-width-controls" data-table-control-group="width"><button class="markra-table-control markra-table-width-button"><svg class="markra-table-control-icon"></svg></button></span>
        <span class="markra-table-control-group markra-table-delete-controls" data-table-control-group="delete"><button class="markra-table-control markra-table-delete-table"><svg class="markra-table-control-icon"></svg></button></span>
      </span>
      <div class="markra-table-scroll"><table class="cm-markra-table"><tbody><tr><td>内容</td></tr></tbody></table></div>
    </div>
  </div>
</div>
```

- [x] **Step 2: 读取并断言布局指标**

从页面读取工具栏、四组、按钮和图标矩形以及计算样式，并增加如下等价判断：

```js
const toolbarFitsNarrowEditor = metrics.toolbarScrollWidth <= metrics.toolbarEditorClientWidth &&
    metrics.toolbarHeight === metrics.groupHeight &&
    metrics.toolbarGroupCount === 4 &&
    metrics.toolbarGap === "8px" &&
    metrics.toolbarButtonWidths.every((width) => Math.abs(width - 28) <= 0.5) &&
    metrics.toolbarButtonHeights.every((height) => Math.abs(height - 28) <= 0.5) &&
    metrics.toolbarIconWidths.every((width) => Math.abs(width - 16) <= 0.5) &&
    metrics.toolbarIconHeights.every((height) => Math.abs(height - 16) <= 0.5);
```

为测试夹具强制显示工具栏按钮，避免透明度策略影响几何验证；仍由 DOM 测试保证生产结构和状态属性。

- [x] **Step 3: 运行 Electron 布局测试**

Run: `cd app && pnpm run test:markdown-layout`

Expected: 输出成功信息；320px 工具栏保持一行，四组完整显示，按钮约 28px，图标约 16px，编辑器无横向溢出。

### Task 5: 完整验证和样式复核

**Files:**
- Verify only: `app/src/markdown/markra-core/codemirror/table.ts`
- Verify only: `app/src/assets/scss/business/_markdown.scss`
- Verify only: `app/src/markdown/markraIntegration.test.ts`
- Verify only: `app/scripts/testMarkdownLayout.cjs`

- [x] **Step 1: 运行完整前端自动验证**

Run: `cd app && pnpm test && pnpm run test:markdown-layout && pnpm run lint`

Expected: 所有测试、Electron 布局检查、TypeScript 和 ESLint 均以退出码 0 完成。`pnpm run lint` 可能按项目配置自动格式化本次修改；完成后必须检查 diff，确认没有无关变化。

- [ ] **Step 2: 在真实桌面客户端检查深色与浅色状态**

使用当前开发客户端打开含表格的 Markdown 文档，分别验证：工具栏默认隐藏；悬停或聚焦表格后显示；四组顺序和间距正确；当前对齐与列宽状态使用主题主色；删除按钮默认中性且仅在悬停或键盘聚焦时显示错误色；图标居中；工具栏没有遮挡表格。

- [x] **Step 3: 检查窄桌面和移动布局**

将 Markdown 内容区域缩窄到约 320px，并在移动端宽度检查：工具栏保持一行且六个操作完整可见，页签和编辑器不产生额外横向滚动，表格内容仍只在 `.markra-table-scroll` 内横向滚动。

- [x] **Step 4: 审查最终差异和工作区边界**

Run: `git diff --check && git status --short && git diff -- app/src/markdown/markra-core/codemirror/table.ts app/src/assets/scss/business/_markdown.scss app/src/markdown/markraIntegration.test.ts app/scripts/testMarkdownLayout.cjs`

Expected: `git diff --check` 无输出；只报告本任务涉及的四个文件变更以及用户原有的其他工作区修改，不创建提交。
