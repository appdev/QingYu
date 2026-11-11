# Vditor 公开接口与剩余格式差异

日期：2026-11-11。仅调研、隔离实验及文档；没有修改生产编辑器、安装或升级依赖，没有构建或恢复手机测试。基准为本项目固定的 Vditor 4.0.1，并检查上游 v4.0.2-alpha.8 类型及发布说明。

后续已按用户选择执行四项适配，结果见 [表格、Callout、YAML 与锚点验收](./markdown-compat-fix.md)。下文保留当时的接口调查结论。

## 结论

之前“SDK 编辑模型相关”不代表完全没有公开扩展接口。Vditor 的公开类型同时包含高层编辑配置和底层 Lute `SetJSRenderers()`。后者明确支持 `Md2VditorDOM`、`HTML2VditorDOM` 等转换阶段及 HTML、表格、脚注、Callout、标题等 renderer 回调。因此不能断言所有剩余差异必须 fork SDK 或换编辑器。

但这些 renderer 负责输出，不是完整的可编辑节点插件接口。修改显示 DOM 后，仍需满足编辑规范化、输入法、选区、粘贴、撤销及序列化约定；Vditor 没有暴露类似 ProseMirror NodeView + schema + transaction 的完整节点扩展体系。本次实验已经证明：表格 `<br>` 的窄范围 renderer 可以生成换行并经规范化保留；直接放出行内 HTML 则不能稳定保留其表示。不能把“首次渲染成功”当成编辑模型兼容完成。

## 接口分层

| 接口 | 实际作用和限制 |
| --- | --- |
| `customWysiwygToolbar(type, element)` / 移动端对应回调 | 官方支持修改上下文工具栏。本项目已使用；不改变节点解析与序列化。 |
| `toolbar` 自定义按钮、快捷键、click；`insertMD/insertValue/updateValue/setValue/getValue` | 官方编辑命令与 Markdown 输入输出；不是任意节点的模型插件。 |
| `customRenders: [{language, render}]` | 围栏代码的展示扩展。4.0.1 的所见即所得先判断 Mermaid、Markmap、PlantUML 等内置语言，只有未匹配内置分支才查 customRenders；不能直接据此覆盖内置 Mermaid，也不是行内 HTML/脚注通用 hook。 |
| `preview.transform` / `preview.parse` | Vditor 自带的只读预览 HTML 前后处理。所见即所得直接调用 `Md2VditorDOM`，不经过这两个回调。我们现有普通预览是独立 markdown-it 管线，也不会自动经过 Vditor preview 回调。 |
| `Vditor.md2html/preview(..., {renderers})` | 静态转换的公开 renderer 配置；源码只将其传给 Lute 的 `Md2HTML`。不能误写为 `new Vditor(..., {preview: {renderers}})` 来改变所见即所得。 |
| `editor.vditor.lute.SetJSRenderers({renderers: {Md2VditorDOM: {...}}})` | Vditor 公开类型暴露的低层 Lute 接口，确实可改变 WYSIWYG 输出；还可覆盖 `HTML2VditorDOM` 等阶段。无统一的“编辑后节点生命周期”承诺，需要适配原有 DOM 协议，不能只改首次导入。 |
| `preview.markdown` 中 `mark/sub/sup/callout/footnotes/toc/gfmAutoLink` | 语法开关。`mark/sub/sup` 开启的是对应 Markdown 扩展，不能等同于任意 HTML 标签无损编辑。`footnotes: true` 不意味着定义位置、回链和外观与妙言相同。 |
| `link.click`；静态预览 `anchor`；`outlineRender` | 可接入链接跳转及大纲。`anchor` 不是统一标题 slug 的回调，编辑器初始化还将 `headingAnchor` 固定为 false；应用仍需自己的标题 ID/重复标题/定位策略。 |

固定版本官方证据：[类型声明](https://github.com/Vanessa219/vditor/blob/v4.0.1/types/index.d.ts)、[README/API](https://github.com/Vanessa219/vditor/blob/v4.0.1/README.md)、[WYSIWYG 入口](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/wysiwyg/renderDomByMd.ts)、[代码渲染分支](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/util/processCode.ts)、[静态预览 renderer 绑定](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/markdown/previewRender.ts)、[内置预览回调](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/preview/index.ts)。

## 逐项判断

| 剩余需求 | 可用公开能力 | 是否足够／建议 |
| --- | --- | --- |
| 表格 `<br>` 真换行 | Lute `renderInlineHTML`，配合检测上下文 | **有窄范围可行证据**：隔离转换、Spin、导出均保留表格换行。仍需真实单元格输入、删除、回车、粘贴、撤销验证；不必先换编辑器。 |
| 行内 HTML mark/sub/sup/u/kbd | Lute `renderInlineHTML`，扩展 `mark/sub/sup` 开关 | **不能靠一次输出覆盖完成**。实验中 mark/sub 被回写为 `==...==` / `~...~`，默认配置下 Spin 甚至将下标解释为删除线；完整样本的 u/kbd 又返回标签编辑表示并改变段落。需贯通规范化及序列化，或者采用源码/显示双层视图。 |
| 脚注文末布局和返回箭头 | `renderFootnotesDef/renderFootnotesRef`、输入输出方法 | 可改初始输出，但移动真实编辑定义会影响源码位置，隐藏定义也影响可编辑性；输入逻辑主动收集和重绘 footnotes-block。没有一键“妙言脚注模式”；需要定义编辑位置、派生文末视图及回链行为。 |
| 引用链接及定义隐藏 | `renderLink` 等 Lute hook；SDK link-ref 工具栏 | 可改引用外观，但定义位置与保存仍受编辑 DOM 约束；隐藏且保留定义可以做应用交互，不能删除定义来假装一致。没有现成的隐藏定义且完整编辑的统一 API。 |
| 邮箱上下文解析 | `gfmAutoLink`、Lute `SetGFMAutoLink` | 开关不能修复已复现的上下文边界；未识别成链接就不会触发 link renderer。需上游解析修复或可靠的输入适配，避免正则改写用户全文。 |
| Callout SVG 图标 | `callout` 开关、`renderCallout` 或显示层 CSS | 可定制，不构成换 SDK 的理由。若 renderer 插入真实 SVG，必须验证 Spin/回写不会把装饰写进 Markdown；CSS 派生图标可减少模型侵入。 |
| YAML 隐藏 | 输入输出方法，应用现有 front matter helper | 没有公开的“隐藏 YAML 且原样保留”开关。可在应用层保存原始元数据，只把正文交给编辑器，保存时拼回；需要明确元数据编辑入口及换行边界，不必改 Lute。 |
| 标题锚点统一 | `link.click`、标题 renderer、应用解析结果 | 应用层可统一 slug 和跳转，不必替换 SDK；不能直接重写 SDK 自用 ID 而不验证大纲、目录、选区。 |
| SVG data URI 图片 | `renderImage`、上传/图片相关 API | 首先区分解析、sanitize 和 WebView CSP。不能从有 image renderer 推断 data URI 已可用，也不应为此关闭 sanitize。未做新的图片实验。 |

## 可复现实验

运行 `node artifacts/vditor-api-research/check.mjs`，结果保存在 [result.json](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/vditor-api-research/result.json)。加载本项目 4.0.1 的真实 Lute，保持 sanitize=true，无浏览器、无生产编辑器实例，仅用固定的可信测试片段验证转换方法。

- `SetJSRenderers({renderers:{Md2VditorDOM:{renderInlineHTML}}})` 回调实际触发，生成真实 `<br>`、`<mark>`、`<sub>`。
- 表格换行经 `SpinVditorDOM()` 和 `VditorDOM2Md()` 保留为 `<br />`，表格 Markdown 被格式化，未宣称逐字保留。
- `<mark>高亮</mark> H<sub>2</sub>O` 直接导出变成 `==高亮== H~2~O`；默认开关下 Spin 的 `~2~` 成为删除线节点，显示与语义都不能只凭首次导入验收。
- `gfmAutoLink` true/false 下，独立邮箱均可生成 mailto，中文标点上下文均未生成 mailto。

Vditor 输入会再次执行 `SpinVditorDOM()`，并处理引用/脚注特殊块，证据：[input.ts](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/wysiwyg/input.ts)。本实验不是完整编辑往返验收，也没有把直出 HTML 的演示 hook 接入生产；自定义 renderer 的输出仍须安全过滤。

## 新版本与替代建议

[v4.0.2-alpha.8 发布说明](https://github.com/Vanessa219/vditor/releases/tag/v4.0.2-alpha.8)包含链接、目录、键盘标签往返及移动端交互修复；[类型声明](https://github.com/Vanessa219/vditor/blob/v4.0.2-alpha.8/types/index.d.ts)新增 `plantuml.server/renderer` 等接口。当前仍为预发布版，不能从发布说明推断已解决本项目全部差异。本轮没有升级或运行 alpha。

建议先保留 Vditor，优先验证 `<br>` 的窄范围 hook，以及应用层 Callout/YAML/锚点。只有要求“行内 HTML、脚注、引用定义的完整编辑模型均可控”时，再把替换编辑器列为正式选型。

替代方案详细证据见 [editor-alternatives-research.md](/Users/evan/Downloads/public/MiaoYanEditor/docs/editor-alternatives-research.md)：

- **Milkdown Core + 自定义插件**：适合 Markdown 为主、需要正式 schema、parse/serialize、NodeView 的直接编辑器；默认 HTML 也显示标签，表格换行和元数据等仍需扩展，不是开箱即与妙言一致。
- **Tiptap**：适合 HTML/结构化富文本为主的产品；Markdown 接口仍标 Beta，不能保证原始 Markdown 保真。
- **CodeMirror 6 Live Preview**：适合 Markdown 文本保真、编辑/预览共用显示规则；真实富文本表格和编辑位置映射工作较多。

以上是接口与架构判断；没有实测候选编辑器的中文输入、桌面/移动 WebView 表现或大文档性能，不能据此承诺流畅度。
