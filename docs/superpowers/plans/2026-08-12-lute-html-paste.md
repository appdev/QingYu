# Lute HTML Paste Implementation Plan

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让思源宿主中的 Markdown 可视化编辑器优先使用 Lute 转换结构化剪贴板 HTML，并在 Lute 不可用或转换失败时保留现有 Turndown 行为。

**Architecture:** `markra-core` 通过可选的 `MarkdownHostAdapter.convertHtmlToMarkdown` 接收宿主转换能力，不直接依赖全局 Lute。HTML 粘贴模块继续负责 DOM 规范化、结构识别与远程图片收集，思源适配器负责惰性获取 Lute、清理 HTML 和执行 `HTML2Md`，失败则由核心回退到 Turndown。

**Tech Stack:** TypeScript、CodeMirror 6、Markra、Lute、Turndown、Node test runner、linkedom 测试 DOM。

## Global Constraints

- 不新增依赖，不修改生成文件 `app/stage/protyle/js/lute/lute.min.js`。
- 不直接从 `markra-core` 访问全局 `Lute`。
- 保留 Markdown 源文、IDE 代码、远程图片与 Turndown 回退流程。
- Lute 转换前必须调用 `Lute.Sanitize`。
- Lute 专用实例必须惰性创建，并将无序列表标记配置为 `-`。
- 不修改用户已有改动的 `app/src/markdown/markra-core/codemirror/code-block.ts` 与 `app/src/markdown/markraIntegration.test.ts`。
- 不执行 `pnpm build`、`git commit` 或 `git push`。

---

## File Structure

- Modify `app/src/markdown/markra-core/adapter.ts`: 声明可选的宿主 HTML→Markdown 接口。
- Modify `app/src/markdown/markra-core/codemirror/html-paste.ts`: 在现有预处理后优先调用宿主转换器，失败时回退 Turndown。
- Modify `app/src/markdown/markra-core/codemirror/clipboard-assets.ts`: 将插件选项中的转换器传入 HTML 转换函数。
- Modify `app/src/markdown/markraExtension.ts`: 从宿主适配器向剪贴板插件传递转换器。
- Create `app/src/markdown/luteHtmlConverter.ts`: 无 UI 依赖地惰性获取 Lute、清理 HTML 并执行转换。
- Modify `app/src/markdown/siyuanAdapter.ts`: 将 Lute 转换器接入思源宿主适配器。
- Create `app/src/markdown/luteHtmlPaste.test.ts`: 覆盖宿主转换优先级、回退、Lute 清理和列表配置，不接触已有脏测试文件。

### Task 1: 为 HTML 粘贴转换增加可选宿主能力

**Files:**
- Modify: `app/src/markdown/markra-core/adapter.ts`
- Modify: `app/src/markdown/markra-core/codemirror/html-paste.ts`
- Test: `app/src/markdown/luteHtmlPaste.test.ts`

**Interfaces:**
- Produces: `MarkdownHostAdapter.convertHtmlToMarkdown?: (html: string) => string | null | undefined`
- Produces: `convertCodeMirrorClipboardHtml(html: string, plainText?: string, convertHtmlToMarkdown?: (html: string) => string | null | undefined): CodeMirrorHtmlPaste | null`

- [x] **Step 1: 编写宿主转换优先和失败回退测试**

在 `app/src/markdown/luteHtmlPaste.test.ts` 安装 `installMarkdownTestDom()`，添加以下断言：

```ts
test("prefers the host HTML to Markdown converter for structured clipboard content", () => {
    let received = "";
    const result = convertCodeMirrorClipboardHtml(
        "<h1>标题</h1><table><tr><th>项目</th></tr><tr><td>内容</td></tr></table>",
        "标题\n项目\n内容",
        (html) => {
            received = html;
            return "# 标题\n\n| 项目 |\n| --- |\n| 内容 |";
        },
    );
    assert.ok(result);
    assert.match(received, /<h1>标题<\/h1>/u);
    assert.equal(result.markdown, "# 标题\n\n| 项目 |\n| --- |\n| 内容 |");
});

test("falls back to Turndown when the host converter returns no Markdown", () => {
    const result = convertCodeMirrorClipboardHtml("<h2>回退标题</h2>", "", () => "");
    assert.equal(result?.markdown, "## 回退标题");
});

test("falls back to Turndown when the host converter throws", () => {
    const result = convertCodeMirrorClipboardHtml("<strong>粗体</strong>", "", () => {
        throw new Error("converter unavailable");
    });
    assert.equal(result?.markdown, "**粗体**");
});
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd app && pnpm test src/markdown/luteHtmlPaste.test.ts`

Expected: FAIL，因为 `convertCodeMirrorClipboardHtml` 尚不接受第三个参数，宿主转换结果不会被使用。

- [x] **Step 3: 添加最小接口和回退实现**

在 `adapter.ts` 中添加可选方法：

```ts
convertHtmlToMarkdown?(html: string): string | null | undefined;
```

在 `html-paste.ts` 中声明转换器类型并用独立小函数安全调用：

```ts
export type ConvertClipboardHtmlToMarkdown = (
  html: string,
) => string | null | undefined;

function hostMarkdown(
  html: string,
  convertHtmlToMarkdown: ConvertClipboardHtmlToMarkdown | undefined,
) {
  if (!convertHtmlToMarkdown) return "";
  try {
    return convertHtmlToMarkdown(html)?.trim() ?? "";
  } catch {
    return "";
  }
}
```

将 `convertCodeMirrorClipboardHtml` 增加第三个可选参数，并在 `normalizeStyledCodeBlocks(document)` 之后执行：

```ts
const convertedByHost = code || !structured
  ? ""
  : hostMarkdown(document.body.innerHTML, convertHtmlToMarkdown);
const markdown = code || convertedByHost || service.turndown(document.body.innerHTML)
  .replace(/\r\n?/gu, "\n")
  .replace(/\n{3,}/gu, "\n\n")
  .trim();
```

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd app && pnpm test src/markdown/luteHtmlPaste.test.ts`

Expected: 新增的三个测试全部 PASS。

### Task 2: 将宿主转换器接入 CodeMirror 粘贴管线

**Files:**
- Modify: `app/src/markdown/markra-core/codemirror/clipboard-assets.ts`
- Modify: `app/src/markdown/markraExtension.ts`
- Test: `app/src/markdown/luteHtmlPaste.test.ts`

**Interfaces:**
- Consumes: `MarkdownHostAdapter.convertHtmlToMarkdown`
- Consumes: `ConvertClipboardHtmlToMarkdown`
- Produces: `CodeMirrorClipboardAssetsPluginOptions.convertHtmlToMarkdown?: ConvertClipboardHtmlToMarkdown`

- [x] **Step 1: 添加真实粘贴管线测试**

在测试文件中创建带自定义适配器的 CodeMirror 视图，派发同时包含 `text/html` 和 `text/plain` 的 `ClipboardEvent`，通过 `runScopeHandlers` 或 DOM `dispatchEvent` 触发粘贴，断言最终文档严格等于宿主返回的 Markdown：

```ts
assert.equal(
    editor.state.doc.toString(),
    "# Flutter 与三端局域网扫描协议\n\n| 项目 | 内容 |\n| --- | --- |\n| 协议名称 | Bridge 协议 |",
);
```

测试适配器必须只新增可选方法，不更改已有测试适配器的必需字段。

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd app && pnpm test src/markdown/luteHtmlPaste.test.ts`

Expected: FAIL，实际文档仍为 Turndown 输出，证明转换器尚未贯穿插件管线。

- [x] **Step 3: 贯穿插件选项**

在 `clipboard-assets.ts` 中给 `CodeMirrorClipboardAssetsPluginOptions` 添加：

```ts
convertHtmlToMarkdown?: ConvertClipboardHtmlToMarkdown;
```

导入该类型，并把调用修改为：

```ts
const converted = convertCodeMirrorClipboardHtml(
  html,
  plainText,
  options.convertHtmlToMarkdown,
);
```

相应地让 `insertHtmlPaste` 接收插件选项中的转换器。在 `markraExtension.ts` 中创建剪贴板插件时传入：

```ts
codeMirrorClipboardAssetsPlugin({
    convertHtmlToMarkdown: adapter.convertHtmlToMarkdown,
    saveAttachment,
    saveImage,
}),
```

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd app && pnpm test src/markdown/luteHtmlPaste.test.ts`

Expected: 真实粘贴管线测试和 Task 1 测试全部 PASS。

### Task 3: 实现思源 Lute 转换适配器

**Files:**
- Create: `app/src/markdown/luteHtmlConverter.ts`
- Modify: `app/src/markdown/siyuanAdapter.ts`
- Test: `app/src/markdown/luteHtmlPaste.test.ts`

**Interfaces:**
- Consumes: 全局 `Lute.Sanitize(html: string): string`、`Lute.New(): Lute`
- Consumes: `getLuteInstance(): Lute | undefined`
- Produces: 思源适配器的 `convertHtmlToMarkdown(html: string): string`

- [x] **Step 1: 编写惰性初始化、清理和配置测试**

通过可恢复的全局 Lute 测试替身记录调用，断言：转换前调用 `Sanitize`、首次转换只调用一次 `New`、专用实例调用 `SetUnorderedListMarker("-")`，并将清理后的 HTML 传给 `HTML2Md`。测试结束必须在 `afterEach` 恢复原全局值。

核心断言：

```ts
assert.deepEqual(calls, [
    ["new"],
    ["list-marker", "-"],
    ["sanitize", "<h1 onclick=\"x\">标题</h1>"],
    ["html2md", "<h1>标题</h1>"],
]);
assert.equal(markdown, "# 标题\n");
```

- [x] **Step 2: 运行测试并确认 RED**

Run: `cd app && pnpm test src/markdown/luteHtmlPaste.test.ts`

Expected: FAIL，因为思源适配器尚未提供 `convertHtmlToMarkdown`。

- [x] **Step 3: 添加惰性 Lute 获取与宿主实现**

在 `luteHtmlConverter.ts` 中导入 `getLuteInstance` 并增加模块级缓存：

```ts
let markdownHtmlLute: Lute | undefined;

const getMarkdownHtmlLute = () => {
    if (!markdownHtmlLute) {
        markdownHtmlLute = getLuteInstance() || Lute.New();
        markdownHtmlLute.SetUnorderedListMarker("-");
    }
    return markdownHtmlLute;
};
```

导出转换函数：

```ts
export const convertSiyuanClipboardHtmlToMarkdown = (html: string) => {
    return getMarkdownHtmlLute().HTML2Md(Lute.Sanitize(html));
};
```

在 `siyuanAdapter.ts` 中把该函数赋给 `convertHtmlToMarkdown`。测试不暴露生产重置 API，而是在一次初始化后连续验证两次调用。

- [x] **Step 4: 运行聚焦测试并确认 GREEN**

Run: `cd app && pnpm test src/markdown/luteHtmlPaste.test.ts`

Expected: 所有 Lute、回退和粘贴管线测试 PASS，且没有未处理异常或控制台警告。

### Task 4: 回归验证与冻结检查

**Files:**
- Verify only: `app/src/markdown/**`

**Interfaces:**
- Consumes: Tasks 1–3 的完整转换管线。
- Produces: 可交付的验证证据，不修改生成文件。

- [x] **Step 1: 运行全部 Markdown Node 测试**

Run: `cd app && pnpm test src/markdown/*.test.ts`

Expected: 全部 PASS。

- [x] **Step 2: 运行前端项目规定的检查**

Run: `cd app && pnpm run lint`

Expected: TypeScript 类型检查和 ESLint 均成功；若 ESLint 的 `--fix` 修改无关文件，停止并报告，不覆盖用户改动。

- [x] **Step 3: 检查差异范围和空白错误**

Run: `git diff --check`

Expected: 无输出，退出码为 0。

Run: `git status --short`

Expected: 仅包含实施文件、设计/计划文档，以及实施前已经存在的 `code-block.ts`、`markraIntegration.test.ts` 用户修改。

- [x] **Step 4: 检查未修改生成文件和用户改动**

Run: `git diff --name-only -- app/stage/build app/stage/protyle/js/lute app/src/markdown/markra-core/codemirror/code-block.ts app/src/markdown/markraIntegration.test.ts`

Expected: 前两个生成目录无新增差异；后两个文件保持实施前已有状态且未被本任务追加修改。通过实施前后 `git diff -- <file>` 快照比对确认。
