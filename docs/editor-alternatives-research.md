# 所见即所得编辑器替代方案调研

日期：2026-11-11。仅检查官方文档及主仓库源码，没有替换生产编辑器、安装新依赖或做候选编辑器实测。

## 结论

没有证据表明换一个 SDK 就能直接得到“所有 Markdown 与妙言预览一致、编辑后原文无损”的成品。Milkdown 和 Tiptap 提供正式的节点、视图、解析和序列化扩展接口，适合解决 Vditor 编辑模型难以定制的问题；CodeMirror 6 保持文本为文档真源，最适合源码保真，但完整的富文本表格编辑需要我们建设。

若确定要替换，并坚持直接编辑标题、列表、表格的 Typora 式交互，建议先用 **Milkdown Core + 定制插件**做小范围验证。理由是 Markdown 解析/序列化是其显式扩展面，而不是因为它已经覆盖全部缺口。不要直接把 Crepe 当作完整格式兼容层。若更看重 HTML 富文本能力与通用产品生态，可选 Tiptap；若更看重 Markdown 原文及预览统一，选 CodeMirror 6 Live Preview，但需接受更多编辑交互开发。此推荐是架构判断，不是性能验收。

## 逐项能力

| 需求 | Milkdown | Tiptap | CodeMirror 6 |
| --- | --- | --- | --- |
| 行内 HTML：mark/sub/sup/u/kbd | 可用 `$mark`、`$remark` 及 `parseMarkdown/toMarkdown` 定制；**默认 HTML 节点仍显示源码文本**，不是开箱渲染全部 HTML。 | 可使用已有 Highlight、Subscript、Superscript、Underline 等 mark，kbd 可自定义 mark；HTML 导入只支持已注册 schema，Markdown 回写规则仍须验收。 | 用 `Decoration.replace`/widget 展示 HTML 结果，原文留在文本状态；标签内的直接富文本编辑要自己映射回源码。 |
| 表格中的 `<br>` | 有 GFM 表格，但官方表格换行需求仍开放，不能宣称默认已解决；需定制 HTML→hardbreak 解析及 Markdown 输出。 | 有 HardBreak 节点；表格内 `<br>` 导入/回写要专项测试。官方 Markdown 文档限制单元格只包含一个子节点，不能据此推断所有富表格结构均可无损。 | 可以替换成表格 widget 并复用现有预览；单元格编辑、选区、行列操作和源码增量回写由我们实现。 |
| 脚注 | GFM 预设内置 reference/definition schema，但定义默认是 `dl/dt/dd`，不等于妙言文末脚注和回链；源码有 label 同步 TODO。需要视图/事务插件。 | 本次未确认官方完整脚注成品；可以自定义 node、Markdown token/serializer 和 NodeView，文末布局与回链仍是产品逻辑。 | 定义留在原文，可隐藏定义并派生文末 widget；引用跳转与编辑位置映射自己实现。 |
| 引用链接与定义位置 | 默认 schema 不构成逐字源码保存保证；应定制 reference/definition 节点和序列化，或额外保留原始片段。 | JSON 模型会重新序列化；需自定义引用节点、定义存储与输出规则，不能把 getMarkdown 当作源码无损承诺。 | 原始 `[label][id]`、定义位置、空白保存在文档文本中；只改装饰时不需要重新序列化全文。 |
| YAML 元数据 | 官方 frontmatter 需求仍开放；可 `$remark` 接 remark-frontmatter，再定义节点/NodeView，或在应用层保留元数据、编辑正文。 | 可自定义 tokenizer/node，或应用层保留；不是普通预览 CSS 能解决的元数据存储问题。 | 原文保留，折叠/替换装饰可隐藏 YAML；单独面板需应用实现。 |
| Callout 图标和结构 | 定制 remark/node/NodeView，可完全控制 SVG 图标和样式；不能假定默认成品与妙言一致。 | 自定义 node + parseMarkdown/renderMarkdown + NodeView，可控制同一套 SVG 图标。 | 解析提示块后用 decorations/widget，复用预览图标；源码编辑逻辑留在 CodeMirror。 |
| 标题锚点 | schema 属性/ProseMirror 插件统一 slug 与跳转规则。 | UniqueID 提供 generateID 等扩展；妙言标题 slug、重复标题处理及链接定位仍需自己统一。 | 源码 heading→统一 slug 表，再把锚点绑定到装饰 DOM；需处理虚拟视口中目标尚未挂载的情况。 |
| Mermaid/Markmap/PlantUML | `$view` 是正式 NodeView 接口，节点存源码、视图复用现有 renderer。 | `addNodeView` 是正式接口，配合自定义节点和 Markdown serializer。 | block widget/replace decoration 复用现有 renderer；编辑时显示源码或做自定义编辑面板。 |

## 公开扩展接口与直接证据

- Milkdown：[`$remark`、`$node`、`$mark`、`$view`、schema 扩展](https://milkdown.dev/docs/api/utils)；[NodeSchema 的 `parseMarkdown`/`toMarkdown`](https://milkdown.dev/docs/api/transformer)；[GFM 表格与脚注节点](https://milkdown.dev/docs/api/preset-gfm)。
- Milkdown 默认 HTML 节点：[html.ts](https://github.com/Milkdown/milkdown/blob/main/packages/plugins/preset-commonmark/src/node/html.ts) 的 `atom: true`、`toDOM` 返回标签文本；[remark-html-transformer.ts](https://github.com/Milkdown/milkdown/blob/main/packages/plugins/preset-commonmark/src/plugin/remark-html-transformer.ts) 明确仍是 HTML 预设支持前的转换层。
- Milkdown 脚注定义：[definition.ts](https://github.com/Milkdown/milkdown/blob/main/packages/plugins/preset-gfm/src/node/footnote/definition.ts)，默认 `dl/dt/dd`，含 label 同步 TODO。
- Milkdown 未闭合的相关官方需求：[表格单元格换行 #2078](https://github.com/Milkdown/milkdown/issues/2078)、[Frontmatter #1712](https://github.com/Milkdown/milkdown/issues/1712)、[常用 HTML 标签 #1249](https://github.com/Milkdown/milkdown/issues/1249)。2026-11-11 GitHub API 返回三者均为 open；开放问题本身不代表无法用插件实现。
- Tiptap：[Markdown 总览与限制](https://tiptap.dev/docs/editor/markdown)仍标为 Beta，路线是 MarkedJS token→扩展处理→JSON，再序列化 Markdown；不保证任意 Markdown/HTML 原样往返。
- Tiptap：[Markdown 扩展接口](https://tiptap.dev/docs/editor/markdown/api/editor)公开 `parseMarkdown`、`renderMarkdown`、`markdownTokenizer`；[NodeView](https://tiptap.dev/docs/editor/extensions/custom-extensions/node-views)公开定制编辑视图；[HardBreak](https://tiptap.dev/docs/editor/extensions/nodes/hard-break)、[Highlight](https://tiptap.dev/docs/editor/extensions/marks/highlight)、[UniqueID](https://tiptap.dev/docs/editor/extensions/functionality/uniqueid)提供基础语义和 ID 扩展。
- CodeMirror：[官方 Decoration 示例](https://codemirror.net/examples/decoration/)提供 mark、widget、replace、line 装饰，明确禁止直接改编辑 DOM；[参考手册](https://codemirror.net/docs/ref/#view.Decoration.replace)说明隐藏/替换文本的正式接口。影响纵向布局的块 widget 必须直接提供装饰集，不能仅依据虚拟视口临时计算。

## 替换前的最小验证门槛

复用现有全格式文档，先验证行内 HTML、表格 `<br>`、脚注、引用链接、YAML、图表六类；每类至少覆盖导入、编辑、撤销/重做、导出、再次导入。将“语义正确”与“Markdown 逐字相同”分开检查，不把无编辑状态保留旧 source 缓存误当作编辑后无损。

再检查目标设备上的中文 IME、选区、表格内回车、软键盘、滚动以及大文档更新。Milkdown GFM 文档虽有 Safari 表格 IME 修复插件，不代表本应用 WebView/设备组合已验收。没有实测依据称三者之一必然更流畅，手机测试仍暂停。
