# Milkdown 格式能力复核：编辑与预览一起切换

日期：2026-11-11。仅评估，没有迁移生产代码、安装依赖、执行构建或手机测试。补充先前的 [编辑器选型调查](./editor-alternatives-research.md)，本轮逐项重新检查固定版本源码，不把旧版结论当成现状。

基准：官方发布页当前 latest 为 **v7.23.0**，tag 指向 **`2e7799559e784b825514734a3dbcad30b2cd7575`**。只读源码 clone 位于 `/tmp/milkdown-format-v7.23.0`。[官方 release](https://github.com/Milkdown/milkdown/releases/tag/v7.23.0)

## 判断

**可以让编辑与预览都采用 Milkdown 的同一模型、插件和样式，从架构上减少两套解析器带来的分歧；但它不是开箱覆盖妙言全部格式的替换包。** 当前 GFM 已有脚注，引用链接 title 也有完整语义通路；剩余主要工作是完整 HTML、引用定义保留、YAML 及妙言式脚注交互。换 SDK 后仍要实现这些模型与序列化规则，不能只移植 CSS。

| 格式 | v7.23.0 开箱能力 | 要达到当前目标还需什么 |
| --- | --- | --- |
| HTML `mark/sub/sup/u/kbd` | Markdown 的 HTML token 存进 inline atom，显示标签文本；没有对应的默认语义 mark | 解析 HTML 成自定义 mark/node，定义属性白名单、DOM 输出和 Markdown serializer；验证嵌套、跨格式选区及撤销 |
| HTML attributes | HTML 原始 token 的 `value` 保存文本；普通节点只接收各自 schema 明确存储的属性 | 原始标签属性不自动变成可编辑 schema 属性；需显式保留/过滤 `class/id/style/data-*` 等，并定义安全策略与回写 |
| block HTML / `details/summary` | block HTML 被包装成段落中的 HTML atom，不是可展开、可编辑的 details 容器 | 自定义 block node、summary/body 内容结构及 NodeView；对不支持的 HTML 保留源码块或明确降级 |
| 脚注 | GFM preset **内置** reference 和 definition schema，可解析及序列化 | 默认引用显示 label，定义原位使用 `dl/dt/dd`；妙言式首次引用顺序编号、文末布局、分隔线、回链及定义编辑需插件 |
| 引用链接 / definition / title | 默认把引用展开为普通 link/image，定义被移除；`title` 有明确解析、属性与回写通路 | 语义 title 可保留，原始引用写法、标识符、定义位置和未使用定义默认不保留；需要替换默认转换及增加定义模型 |
| YAML front matter | commonmark/GFM 默认未注册 frontmatter parser 或 yaml schema | `$remark` 接入 remark-frontmatter，再加节点与 serializer；或沿用应用层原文保留、正文交给编辑器 |

## HTML：默认保存标签，不等于直接编辑渲染内容

[`html.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/node/html.ts) 的 `htmlSchema` 明确为 `inline: true`、`atom: true`，唯一内容属性为 `value`；`toDOM` 输出 `span[data-type=html]`，其文本内容就是原始标签字符串；`toMarkdown` 再输出 mdast `html` 节点。因此它不会把 `<mark>文字</mark>` 自动变成可编辑高亮 mark。`htmlAttr` 控制这个展示 span 的 DOM 属性，也不是任意用户 HTML 属性的无损存储接口。

[`remark-html-transformer.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/plugin/remark-html-transformer.ts) 把 block container 内的 HTML 包入段落，源码注释仍写着等待 preset 正式支持 HTML。不能据存在 `htmlSchema` 推断 `details/summary`、任意嵌套 HTML 已具备富文本编辑能力。普通 schema 的 `parseDOM` 可处理其声明的粘贴 DOM，和从 Markdown 原始 HTML token 导入是两条路径。

扩展入口是正式的：[`$markSchema` / extendSchema](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/utils/src/composable/composed/%24mark-schema.ts)、[`$nodeSchema` / extendSchema](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/utils/src/composable/composed/%24node-schema.ts)、[`$remark`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/utils/src/composable/composed/%24remark.ts)、[`$view`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/utils/src/composable/%24view.ts)。它们允许控制 AST 转换、`parseMarkdown/toMarkdown`、ProseMirror node/mark 与 NodeView；这证明可以实现适配，不证明我们尚未写出的适配已经支持任意 HTML。

原型应先限定五种行内标签和明确允许的属性，再验证 details block。任意 HTML 的结构、非法嵌套、样式与事件属性不能直接当成安全可编辑内容接入；这属于新适配工作，不是简单替换配置。

## 脚注：已经支持语法，默认视图仍需定制

GFM 的 [`composed/schema.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-gfm/src/composed/schema.ts) 注册 `footnoteReferenceSchema` 与 `footnoteDefinitionSchema`。因此不能说 Milkdown 当前不支持脚注。

但 [`reference.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-gfm/src/node/footnote/reference.ts) 输出的是带 label 文本的 `sup`，并非自动生成按引用顺序排列的数字链接；[`definition.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-gfm/src/node/footnote/definition.ts) 使用原位 block node，DOM 是 `dl > dt + dd`，定义内容可包含 `block+`。两处仍有 label 变化同步的 TODO。文末聚合、重复引用的多个回链和修改定义的产品行为，不能靠分隔线 CSS 一并解决。

可以利用同一 schema 和事务插件生成两模式共用的编号/回链数据；定义仍要有可控的编辑入口，不能为了文末外观直接删除、隐藏后丢失它。实现后需验证重命名、增删引用、重排、撤销/重做及再次导入，当前没有运行这些原型。

## 引用链接：title 语义保留，引用源码默认规范化

commonmark 的 [`composed/plugins.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/composed/plugins.ts) 默认包含 [`remark-inline-link-plugin.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/plugin/remark-inline-link-plugin.ts)。上游 [`remark-inline-links 7.0.0`](https://github.com/remarkjs/remark-inline-links/blob/7.0.0/lib/index.js) 明确删除 `definition`，并将 `linkReference/imageReference` 转换为普通 link/image，携带 `def.url` 和 **`def.title`**。

Milkdown 的 [`link.ts`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/mark/link.ts) 再把 title 存进 mark 属性，DOM 与 `toMarkdown` 均使用它。因此按源码，引用 title 不应因这条默认转换而丢失；仍应做编辑/导出回归，不能从静态阅读保证所有 title 引号与转义形式逐字不变。

例如 `[文字][id]` 加独立定义的关系，默认会在模型里变为 href/title 的 link mark，之后输出 inline link。定义原来放在哪里、标识符大小写、是否为 collapsed/shortcut reference、未被使用的定义，都没有进入默认 link schema。若保留这些信息是验收条件，要在 AST 转换前替换默认 inline-link 插件并设计 reference/definition 的存储与序列化；关闭该插件本身还不够，因为默认 schema 没有对应 reference/definition 类型。

## YAML：扩展 parser 之后还需要模型

固定版本的 [commonmark schema 列表](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/composed/schema.ts)、[插件列表](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/composed/plugins.ts) 与 [GFM 插件列表](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-gfm/src/composed/plugins.ts) 没有默认 YAML/frontmatter。官方生态 [`remark-frontmatter 5.0.0`](https://github.com/remarkjs/remark-frontmatter/blob/5.0.0/readme.md) 可以提供语法解析/序列化，但只识别 frontmatter，并不解析 YAML 数据内容或替我们创建 ProseMirror 节点。

Milkdown parser 遇到无 schema 匹配的 AST 类型会抛出匹配错误，因此不能只 `.use(remarkFrontmatter)` 就宣称完成。若需要节点可编辑，则增加 yaml node、`parseMarkdown/toMarkdown` 和视图；若当前只要求隐藏且原样保存，应用层独立保留原始 front matter 与换行边界更小。[parser 状态机](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/transformer/src/parser/state.ts)

## Markdown 原文保真边界

默认链路为 Markdown → remark AST → ProseMirror 文档；导出是 ProseMirror → 新 remark AST → `remark.stringify`，不是对原始 Markdown 做最小文本补丁。[初始化](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/core/src/internal-plugin/init.ts)、[parser](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/transformer/src/parser/state.ts)、[serializer](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/transformer/src/serializer/state.ts)

这不表示所有标记必定丢失：[strong](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/mark/strong.ts) 与 [emphasis](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/plugins/preset-commonmark/src/mark/emphasis.ts) 显式保留 marker，项目也有保留空行的插件。但没有进入模型的源码细节无法由 serializer 还原；上面的引用定义被消除就是明确例子。保留未编辑时的原文缓存，不等于正文编辑之后的全文逐字保真。

预览方式也影响一致性：[`getHTML()`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/utils/src/macro/get-html.ts) 使用 `DOMSerializer.fromSchema`，只调用 schema 的 DOM 输出，不包含自定义 NodeView 已经渲染的图表 DOM。因此若采用静态 HTML 预览，自定义 HTML/图表仍需共享渲染函数；如果采用同一 EditorView 的只读模式，更容易共用 NodeView，但仍需验收链接、图表和只读交互。这是架构判断，不是已完成的运行验证。

若选这条路线，建议**同一 ProseMirror Doc、Schema、NodeViews 和 CSS，尽量保留同一 EditorView，只切换可编辑状态**；避免每次切换都 serialize → parse、重建状态而影响选区和撤销历史。官方 [`CrepeBuilder.setReadonly`](https://github.com/Milkdown/milkdown/blob/2e7799559e784b825514734a3dbcad30b2cd7575/packages/crepe/src/core/builder.ts) 就通过 `view.setProps({ editable: () => !value })` 切换。`editable=false` 仅限制编辑器默认输入，不自动阻止应用调用事务 dispatch；自定义 NodeView 中的输入框、表格按钮和内嵌 CodeMirror 也必须遵守同一 readonly 状态。

## 最小原型验收范围

仅当决定迁移时再做，不在本轮执行：同一份全格式文档覆盖五种行内 HTML 与属性、嵌套 details、表格 `<br>`、重复/重排脚注、引用 title/未使用定义、YAML。每项比较导入、局部编辑、撤销/重做、导出及再次导入；分别记录视觉一致、语义一致、源码逐字一致。以妙言为视觉基准需要显式重建兼容规则，两个模式都切 Milkdown 只消除内部双管线分歧，不会自动等同于妙言的 cmark-gfm 与前后处理。

本轮证据为固定源码与官方文档核查；未运行 Milkdown 原型、性能或移动设备验证。手机测试继续暂停。
