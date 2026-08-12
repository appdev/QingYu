# Markdown CodeMirror 实时预览编辑器设计

## 1. 背景与目标

当前原始 Markdown 编辑器使用 CodeMirror 6 保存源码，同时使用可编辑的 HTML 预览作为默认编辑面。预览通过 Lute 将 Markdown 转换为 HTML，用户编辑后再通过 `Lute.HTML2Md` 将整篇 DOM 转回 Markdown。该方案能够快速复用现有 SiYuan 渲染能力，但复杂表格、嵌套列表、图片尺寸、Mermaid、公式、原始 HTML 和剪贴板内容都需要额外的双向适配，并使浏览器编辑历史、选区、输入法状态与 CodeMirror 状态难以保持一致。

本次将原始 Markdown 编辑器改造成以 CodeMirror 6 `EditorState` 为唯一文档数据源的实时预览编辑器。实现采用 Markra 的核心技术思路：使用 Lezer Markdown 语法树、CodeMirror Decoration、Widget 和 Transaction 完成所见即所得编辑，但不引入 Markra 的 React、Tauri、AI、文件管理或产品 UI。QingYu 现有标签页、标题、面包屑、模式按钮、保存状态、正文宽度、主题变量、桌面布局和移动端布局保持不变。

## 2. 用户体验边界

- Markdown 文档默认打开现有“所见即所得”模式。
- 现有“Markdown”按钮切换到完整源码模式，现有“预览”按钮返回实时所见即所得模式。
- 不增加分屏模式，不增加新的顶层工具栏、设置项或页面。
- 所见即所得和源码模式共享同一个 CodeMirror 文档、选区映射和撤销历史。
- 标题、面包屑、保存状态图标、标签页标题、正文宽度、字体、颜色、暗色主题和移动端外观继续使用当前 QingYu 样式。
- 所见即所得模式不显示行号；源码模式沿用当前不主动显示行号的行为。
- 用户编辑、复制、粘贴、撤销、重做和保存的最终结果始终是普通 `.md` 或 `.markdown` 文本。

## 3. 非目标

- 不把原始 Markdown 文件转换为 `.sy` 文档或 Protyle 块树。
- 不引入 React、Tauri 或 Markra 应用层依赖。
- 不复制 Markra 的 AI、拼写检查、分屏、主题系统、文件工作区、同步或导出产品能力。
- 不改变现有 Go Kernel Markdown 文件 API、路径模型、SHA-256 revision 冲突检查或 800ms 自动保存策略，除非实施中发现与单一 CodeMirror 状态直接冲突的缺陷。
- 不在第一阶段新增 Markdown 方言；现有 Lute 能识别并已在当前预览中支持的语法是兼容基线。

## 4. 架构

```text
.md 文件
   ↓ /api/markdown/get
CodeMirror EditorState（唯一数据源）
   ↓ Lezer Markdown 语法树
实时预览扩展
   ├─ 标记隐藏与样式 Decoration
   ├─ 块级 Line Decoration
   ├─ 图片、表格、公式、Mermaid Widget
   ├─ 光标与选区 Reveal Policy
   └─ 粘贴、拖拽、快捷键 ViewPlugin
   ↓ CodeMirror Transaction
原始 Markdown 文本
   ↓ /api/markdown/save
Go Kernel 文件写入与 revision 冲突检查
```

`EditorState.doc` 是编辑期间唯一可写的文档状态。任何视觉控件都只能通过 CodeMirror Transaction 修改 Markdown 范围，不得维护可独立编辑的 HTML 文档副本。Lute继续用于导入、导出、兼容性转换和必要的只读渲染，但不再承担日常输入后的整篇 HTML → Markdown 回写。

## 5. 组件边界

### 5.1 MarkdownEditor 外壳

现有 `MarkdownEditor` 继续负责加载、保存、重命名、销毁、标签页标题、面包屑、状态图标、焦点和模式按钮。它只创建一个长期存活的 `EditorView`，模式切换通过 CodeMirror `Compartment` 重新配置扩展，不销毁或复制文档。

外壳删除独立的 `.markdown-editor__source` 和可编辑 `.markdown-editor__preview`，改为一个 `.markdown-editor__surface` CodeMirror 挂载容器。现有预览样式迁移到该容器下的语义类，不保留第二个隐藏编辑面。模式状态只决定启用实时预览扩展还是源码语法扩展。

### 5.2 Markdown 语言与语法树

语言层使用 `@codemirror/lang-markdown` 和 `@lezer/markdown`，启用当前产品需要的 GFM 节点。扩展语法必须以独立 parser extension 或明确的语法范围识别实现，不能依赖扫描渲染后 HTML。

语法树是视觉装饰和交互范围的唯一结构依据。对于 Lezer 无法完整表达的 Lute 扩展语法，可增加受测试约束的行级解析器，但其输出仍必须是 Markdown 文本范围，不能生成第二份权威内容。

### 5.3 Reveal Policy

Reveal Policy 根据光标、选区、拖拽和输入法状态决定 Markdown 标记是否显示。光标不在节点内时，可隐藏标题 `#`、强调标记、链接目标和其他非必要语法；光标进入节点、跨节点选择、拖拽源码或发生组合输入时，显示足以无损编辑的原始 Markdown。

Reveal Policy 必须保证隐藏 Decoration 不改变文档位置，选区与 Transaction 坐标始终使用原始 Markdown offset。无法安全隐藏的结构默认显示源码，不以视觉完整性换取数据正确性。

### 5.4 Renderer 与 Widget 注册

建立轻量的 QingYu Markdown renderer 注册接口，每个 renderer 声明稳定 ID、匹配的语法节点、是否只处理可见范围以及渲染函数。该接口只负责 CodeMirror 渲染扩展，不复制 Markra 的应用级命令和 UI 协议。

Renderer 之间不得直接修改 DOM 或对方状态。用户操作通过显式 command 生成 Transaction；异步渲染结果必须验证文档版本和节点范围仍然有效后才能更新 Widget 状态。

### 5.5 Inline Decoration

标题、粗体、斜体、删除线、行内代码、链接、引用和列表优先使用 mark、replace 或 line Decoration。样式类映射到现有 `b3-typography` 和 Protyle 主题变量，使视觉结果保持当前 QingYu 风格。

任务列表复选框使用可交互 Widget，但点击只替换原 Markdown 中 `[ ]` 或 `[x]` 的精确范围。链接点击与编辑需区分修饰键打开和普通光标定位，避免用户无法进入链接源码。

### 5.6 块级 Widget

- 图片：复用现有图片外观、资源解析和缩放手柄；未调整尺寸的普通 Markdown 图片语法保持原文不变，用户首次明确调整尺寸时以一次 Transaction 将该图片转换为经过转义的内联 HTML `<img>`，使用整数 `width` 属性持久化宽度并由 CSS `max-width: 100%` 约束窄屏，已有 HTML 图片则只更新其 `width` 属性。
- 表格：解析 GFM 表格对应的 Markdown 行范围并构建可视表格；单元格输入、增加或删除行列、对齐和宽度调整都序列化为对该范围的一次 Transaction；解析失败时自动回退源码显示。
- Mermaid：复用现有 Mermaid 加载与渲染能力，Widget 以 fenced code block 内容为输入；渲染失败时显示错误状态和“编辑源码”入口，但不得修改源码。
- 数学公式：复用现有 KaTeX 渲染能力；光标进入公式范围时展开源码，离开后重新渲染。
- 代码块：保留语言信息、代码高亮和当前样式；编辑时使用原始 CodeMirror 文本，不在嵌套 `contenteditable` 中维护另一份代码。
- 原始 HTML：默认经过 DOMPurify 后只读渲染；任何不能安全映射回源码的交互都回退为源码编辑。

## 6. 模式切换与历史

所见即所得和源码模式使用同一个 `EditorView` 和 `EditorState`。切换模式通过 `Compartment.reconfigure` 启用或关闭预览 Decoration、Widget、主题和源码标记显示，不替换 `state.doc`，不触发保存，也不创建撤销记录。

所有文本修改、表格操作、任务切换、图片属性修改和格式化命令都通过 Transaction 完成并进入同一 CodeMirror history。`Cmd/Ctrl+Z`、重做、`Cmd/Ctrl+A`、复制和选区行为由 CodeMirror 统一处理；全选必须覆盖原始 Markdown 全文，而不是只选择当前可见 Widget DOM。

## 7. 输入法、粘贴与拖拽

组合输入期间不重建当前行的结构性 Widget，不隐藏组合范围中的 Markdown 标记，也不将中间态发布为独立业务变更。组合结束后由 CodeMirror 的最终 Transaction 更新语法树和预览。

粘贴优先读取明确的 Markdown 或 `text/plain`，直接通过 Transaction 插入源文本。普通网页富文本可以通过受控转换器转换为 Markdown 后插入，但不得经过可编辑 HTML 预览再回写。粘贴图片和附件继续调用当前资源保存边界，成功后插入 Markdown 链接；保存失败时保留剪贴板内容并显示现有错误反馈。

块拖拽和图片拖拽必须携带 Markdown 范围或资源引用，落点换算为 CodeMirror 文档位置后生成 Transaction。拖拽期间的视觉占位符不进入 Markdown。

## 8. 保存、外部修改与错误处理

保存继续从 `view.state.doc.toString()` 获取内容，沿用 800ms 防抖和现有 `/api/markdown/save`。成功后更新 revision；保存期间继续编辑时，当前内容与已提交快照不同则再次排队保存。

Kernel 返回 revision 冲突时保持当前冲突提示和刷新选择。刷新必须明确放弃当前未保存内容后重新加载文件；不得把过期 Widget DOM或异步渲染结果写入新文档。

单个 renderer 失败不得阻止文档编辑。对应节点回退为可编辑 Markdown 源码并记录受控错误；整个 EditorView 只有在 CodeMirror 初始化失败时才进入编辑器错误状态。Mermaid、KaTeX 和图片加载失败只影响相应 Widget。

## 9. 性能策略

- Decoration 优先限制在 CodeMirror 可见范围及必要的邻近语法范围。
- Mermaid、公式、图片和大型表格采用按需 Widget，离开可见区域后允许释放昂贵渲染资源。
- 普通输入不得执行整篇 `md2html`、整篇 `HTML2Md` 或整篇文档替换。
- 文档切换和外部重新加载可以执行一次整篇 CodeMirror replacement，但必须关闭历史记录或建立明确的新文档基线。
- 大型表格或无法在预算内解析的复杂块优先回退源码，保证输入延迟优先于视觉渲染。

## 10. 样式兼容

编辑器继续使用 `.markdown-editor`、现有正文宽度规则和 SiYuan CSS 变量。新增 CodeMirror 类只表达节点角色，不硬编码主题颜色、字体或产品尺寸。标题、段落、引用、列表、代码、表格、图片、Mermaid 和公式应尽量复用当前 `b3-typography` 的视觉值，并通过局部适配避免污染普通 Protyle 编辑器。

桌面与移动端使用相同语义类和 renderer，响应式 CSS 决定间距、触摸命中区和图片手柄。移动端不得维护独立 Markdown 转换实现。

## 11. 安全与数据完整性

- 原始 HTML、Mermaid 生成内容和其他第三方渲染结果继续经过现有安全边界，不允许脚本或事件属性进入编辑器 DOM。
- Widget DOM一律视为派生视图，不参与保存。
- 图片和链接继续使用现有路径与 URL 安全规则。
- 任意 renderer 无法确定无损修改范围时必须拒绝可视修改并展开源码。
- Markdown 原文是最终正确性基准；视觉近似可以降级，原文不得被静默重写或规范化。

## 12. 迁移阶段

### 阶段一：内核与基础语法

建立单一 EditorView、模式 Compartment、Reveal Policy、renderer 注册接口和基础标题、强调、链接、列表、引用 Decoration。完成后默认视觉模式不再依赖可编辑 HTML，源码切换和共享历史可用。

### 阶段二：复杂块

依次实现代码块、图片、公式、Mermaid 和 GFM 表格 Widget。每一种复杂块都先具备只读渲染、源码展开和失败回退，再增加对应的可视交互。

### 阶段三：剪贴板与交互收口

将 Markdown、网页富文本、图片和附件粘贴统一迁入 CodeMirror Transaction，完成任务切换、链接打开、图片缩放、表格编辑、拖拽以及移动端触摸行为。

### 阶段四：移除旧链路

在支持矩阵和回归测试全部通过后，删除可编辑 `.markdown-editor__preview`、输入后的 `Lute.HTML2Md`、图表源码 DOM恢复逻辑以及只为双编辑面服务的选区同步代码。Lute导入、导出和只读兼容用途保留。

迁移可以按阶段提交，但产品默认编辑器只能在阶段四验收通过后切换。实施期间允许保留只用于开发验证的旧内核回退开关，不新增用户设置，最终删除该开关。

## 13. 测试与验收

### 单元测试

- Reveal Policy：光标进入、离开、跨节点选择、全选和组合输入时标记显隐正确。
- Transaction：格式化、任务切换、表格行列操作、图片尺寸和 Widget 编辑只修改目标 Markdown 范围。
- Parser/renderer：标题、强调、链接、列表、引用、代码、图片、公式、Mermaid、表格、脚注和原始 HTML 的节点识别、失败回退与不支持语法保留。
- Clipboard：原始 Markdown、纯文本、网页富文本、多文件、图片和附件粘贴结果正确。
- History：视觉模式、源码模式和模式切换后的撤销/重做保持连续。

### 集成测试

- 打开用户提供的长篇《推送通知端到端技术方案》，源码内容在打开、编辑、模式切换、保存和重新加载后逐字保持，除用户实际编辑的范围外不发生变化。
- 文档中的 33 个表格和 8 个 Mermaid 均可显示；任一渲染器失败时相应源码仍可编辑。
- `Cmd/Ctrl+A`、复制、粘贴、撤销、重做、保存和标题重命名在桌面端与移动端工作。
- 外部修改同一文件触发 revision 冲突，不覆盖外部内容或当前未保存内容。
- 宽表格、超长行、大图片和 Mermaid 不扩大标签页面板宽度。

### 性能验收

- 普通字符输入路径不调用整篇 Lute Markdown/HTML 双向转换。
- 长文滚动只渲染可见或邻近的昂贵 Widget。
- 使用当前长文连续输入、撤销、模式切换和滚动时无明显阻塞；实施计划应在现有运行环境记录可重复的基线与迁移后数据，而不预设脱离实测的毫秒阈值。

### 项目验证

- 前端修改运行 `cd app && pnpm run lint`。
- 运行 Markdown 编辑器单元测试、布局测试和针对桌面客户端的 UI 测试。
- 移动端至少验证窄视口、触摸选区、软键盘输入、图片缩放和复杂块源码回退。
- 不运行项目禁止的 `pnpm build`；开发运行继续使用项目既有方式。

## 14. 完成标准

- 编辑期间只有 CodeMirror `EditorState.doc` 是可写文档状态。
- 默认所见即所得模式不再使用可编辑 HTML 或输入后的 `HTML2Md`。
- 现有 UI结构与视觉样式保持不变，模式按钮仍为“Markdown / 所见即所得”。
- 基础和复杂 Markdown 语法均可无损编辑，无法渲染时自动回退源码。
- 桌面与移动端共享同一编辑内核和 Markdown Transaction 逻辑。
- 用户参考长文完成打开、渲染、编辑、粘贴、保存和重载验证，除明确编辑外原始 Markdown 不发生变化。
