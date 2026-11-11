# 剩余渲染差异的归属确认

本文记录修复前的归因；已执行的 CSS/集成修复及验证结果见 [2026-11-11 修复验收](/Users/evan/Downloads/public/MiaoYanEditor/docs/style-integration-fix.md)。SDK 编辑模型相关内容未改动。

2026-11-10，按 systematic-debugging 调查，只修改诊断脚本及报告，不修改产品代码。

结论：不是 CSS 或 SDK 二选一。现有差异分别来自 **CSS 适配遗漏、SDK 的可编辑 DOM 设计、我们的资源/配置接入、少量 Lute 解析边界**。不能把所有差异称为 SDK 不支持，也不能只补 CSS 解决全部问题。

## 为什么共用配色后仍不相同

原版妙言预览生成静态 HTML；我们的普通预览通过 markdown-it 生成静态 HTML；所见即所得则调用 Vditor/Lute 的 `Md2VditorDOM()` 生成可编辑节点。后者为了保存原始语法和支持编辑，会保留静态 HTML 中没有的包装层、定义块和代码标记。虽然普通代码和 Mermaid 已共用原版的绘图库、主题配置及部分样式，整个文档仍没有共用同一套 HTML 生成流程。

工作区证据：[rich-editor.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-editor.ts:49)、[renderDomByMd.ts](/Users/evan/Downloads/public/MiaoYanEditor/node_modules/vditor/src/ts/wysiwyg/renderDomByMd.ts:10)、[markdown.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/markdown.ts:94)。这不是 CodeMirror 的渲染问题；当前所见即所得使用 Vditor。

## 已确认的归属

| 差异 | 归属 | 根因与证据 |
| --- | --- | --- |
| Mermaid 图形偏大 | 我们的 CSS/渲染测量适配 | 完整格式文档下，Mermaid 在 body 下的临时容器测量标签，正文内的样式没有覆盖那里。实际记录到 `p` 的 margin 是 `16px 0px`。只在测试页面把临时标签 margin 改成 0，7 个合法图的 viewBox 全部恢复原版数值。不是 Mermaid 缺少对应图形能力。 |
| 长代码行不换行 | SDK 默认 CSS 与我们覆盖不完整 | SDK `_reset.less` 的 `pre > code` 规定 `white-space: pre`。测试页仅改为 `pre-wrap`，同一长行从 22px 变成 66px，内容不变。字号和高亮相同不代表换行策略相同。 |
| HTML、表格里的 `<br>` | SDK 所见即所得的编辑节点设计 | 不加载我们的任何 CSS，纯 Lute 的 `Md2VditorDOM()` 仍生成 `<code data-type="html-inline">&lt;br&gt;</code>`；同一引擎的 `Md2HTML()` 则生成真实 `<br>`。Lute 能解析 HTML，当前 WYSIWYG 路径选择了可编辑代码表示。CSS 无法把这个节点变成保留编辑语义的真实换行。 |
| 脚注、引用定义的位置与外观 | SDK 编辑文档结构差异 | 纯 Lute 已复现：WYSIWYG 在定义位置保留 `footnotes-block`，引用链接是 `span[data-type=link-ref]`，定义也是可编辑块；普通 HTML 输出则把脚注放到文末，并生成链接和返回箭头。不是我们的 CSS 把节点移错。 |
| Markmap、PlantUML 显示源码 | 我们的资源与绘图接入尚未完成 | Vditor 4.0.1 源码明确内置两种 renderer，npm 包里也有资源；应用的 `public/vditor` 没有复制这些资源，实际请求 404。我们的自定义显示层又显式保留其他图表的代码回退。不能归因为 SDK 不支持。PlantUML 默认 renderer 使用 `object`，与当前 `object-src 'none'` 也不兼容，后续需要绘图接入适配而非只复制文件。 |
| YAML 被显示、窗口标题为 `---` | 我们的预览/编辑输入处理与标题提取不同 | 普通预览先剥离 front matter；所见即所得直接传入完整 Markdown，由 SDK 显示 YAML 编辑块。标题提取则直接读原文第一行。这两处需要在我们自己的文档适配逻辑中处理。 |
| `==高亮==`、上下标未启用 | 配置边界，不能称为 SDK 无能力 | SDK 有 `mark/sub/sup` 配置且默认关闭。纯 Lute 将三者开启后能生成对应 `mark/sub/sup` 节点；开启是否符合原版预览的语法边界是另一个问题。 |
| 邮箱自动链接在部分上下文失败 | Lute 解析边界 | 独立 `<test@example.com>` 可生成 mailto 链接；加中文顿号后，纯 Lute 的 WYSIWYG 和普通 HTML 两条路径均输出转义文本。切换 `gfmAutoLink` 仍可复现。没有我们的 DOM/CSS 参与，具体上游解析条件尚未继续追踪。 |
| 深色彩色节点的黑字 | 已做的可读性调整 | 保留自定义填充色时，原版的浅字对比不足。我们主动调整文字和边框，这一差异不属于待消除的排版错误。 |

SDK 原始实现：[默认代码 CSS](/Users/evan/Downloads/public/MiaoYanEditor/node_modules/vditor/src/assets/less/_reset.less:267)、[Markmap renderer](/Users/evan/Downloads/public/MiaoYanEditor/node_modules/vditor/src/ts/markdown/markmapRender.ts:35)、[PlantUML renderer](/Users/evan/Downloads/public/MiaoYanEditor/node_modules/vditor/src/ts/markdown/plantumlRender.ts:9)、[Lute 配置映射](/Users/evan/Downloads/public/MiaoYanEditor/node_modules/vditor/src/ts/markdown/setLute.ts:20)。官方配置说明见 [Vditor README](https://github.com/vanessa219/vditor/blob/master/README.md)。具体结论以上述本地 4.0.1 源码和实验为准，不把 master 文档当作固定版本行为证明。

## 实验与边界

- [attribution.mjs](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/attribution.mjs) / [实验数据](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/attribution.json)：纯 Lute 的 DOM/HTML 对照、只修改 `white-space` 的长行实验、只修改临时标签 margin 的 Mermaid 实验。
- [lute-context.json](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/lute-context.json)：邮箱上下文与扩展配置实验。
- Mermaid 的确认实验使用上轮实际打包的前端和完整 37 组格式文档。开发页上的缩减三图样本没有复现偏大，不能用它取代完整上下文判定；完整文档、打包页面下稳定复现且能由单个 CSS 属性消除。
- 诊断期间未关闭 sanitize、未放宽 CSP、未替换编辑 DOM、未修改用户原文。此前已验证的 rich-editor/rich-code/preview 等源文件校验清单仍匹配；普通预览的 front matter helper 在工作区已有其他变动，本轮保留它，定量 Mermaid 结论对应已打包版本。
- 这轮没有进入修复阶段，没有构建/安装，也没有恢复手机测试。图片本地附件访问、全部扩展语法以及编辑后的 Markdown 往返仍不能由本实验验收。

此前“已一致”的表述覆盖过宽：当时主要验证短代码、部分图形和最终显示样式，遗漏了长行、HTML 编辑节点以及图形渲染前的临时测量阶段。后续应先确认每项差异属于静态样式、编辑结构、配置还是资源接入，再逐项验收，避免继续把所有问题都归为 CSS。
