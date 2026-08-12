# Markdown 编辑器 Markra Core 移植设计

## 1. 目标

在不引入 React、不改变思源整体 UI 风格的前提下，用 Markra 已验证的 CodeMirror Markdown 编辑器核心替换当前尚未完成的自研实时预览实现。保留 QingYu/SiYuan 的 Electron 外壳、原生 TypeScript UI、标签页、标题、面包屑、模式按钮、正文宽度、主题、移动端布局、Go Kernel 文件 API 和 revision 冲突控制。

完成后的产品仍然是思源界面中的原始 `.md`/`.markdown` 编辑器。替换范围集中在 Markdown 解析、实时预览、编辑命令、剪贴板和复杂块交互，不迁移 Markra 产品 UI。

## 2. 决策

采用“Markra 框架无关核心 + SiYuan Adapter + 现有 MarkdownEditor 外壳”方案。

Markra 的 React 只属于应用桥接层，不是 CodeMirror 核心的必要依赖。移植以 Markra `packages/editor` 中框架无关的 TypeScript/CodeMirror 模块为基础，排除 `editor-react`、Tauri、AI、拼写检查、文件工作区、同步、主题和其他产品能力。

当前 `app/src/markdown/livePreview` 不作为第二套长期实现保留。新核心通过验证后替换它，避免同时维护两套解析、Decoration 和 Widget 行为。

## 3. 非目标

- 不把当前项目改造成 React。
- 不嵌入完整 Markra 应用或使用 iframe、WebView 套壳。
- 不改变思源标签页、文件树、标题区、面包屑、模式按钮或保存状态 UI。
- 不把 `.md` 转换为 `.sy` 文档或 Protyle 块树。
- 不迁移 Markra AI、拼写检查、工作区、主题、同步、导出或 Tauri 文件能力。
- 不继续扩展当前自研预览引擎与 Markra Core 形成混合实现。
- 不直接复用 Protyle 的 `.sy` transaction；Markdown 内容修改始终通过 CodeMirror Transaction 完成。

## 4. 总体架构

```text
SiYuan MarkdownEditor 外壳
  标签页、标题、面包屑、模式按钮、保存状态、正文宽度
                         ↓
SiyuanMarkdownAdapter
  文件、资源、URL、图标、菜单、语言、渲染服务、错误反馈
                         ↓
Markra Core
  语法、预览、Widget、剪贴板、快捷键、选区、撤销
                         ↓
CodeMirror EditorState.doc
  唯一可写 Markdown 文档状态
```

`EditorState.doc` 是编辑期间唯一内容来源。所见即所得模式和源码模式共用一个长期存活的 `EditorView`、同一份 selection 和同一套 history。模式切换只通过 `Compartment.reconfigure` 改变扩展，不复制文档、不重建可编辑 HTML，也不触发保存。

## 5. 组件边界

### 5.1 MarkdownEditor 外壳

现有 `app/src/markdown/MarkdownEditor.ts` 继续负责：

- `/api/markdown/get`、`/api/markdown/save` 和 `/api/markdown/rename`；
- 800ms 自动保存；
- SHA-256 revision 冲突；
- 标签页、标题、面包屑和保存状态；
- 桌面和移动端生命周期；
- “Markdown / 所见即所得”模式按钮。

外壳只装配核心和 Adapter，不实现 Markdown 语法判断、Widget 或剪贴板转换。

### 5.2 Markra Core

核心保持接近 Markra 上游的目录结构和公开接口。允许的修改限于：

- 包路径和 TypeScript 4.9 兼容；
- 删除明确排除的产品能力；
- 将平台调用替换为 Adapter 接口；
- 必要的思源宿主生命周期修正。

核心文件不得直接访问 `window.siyuan`、Electron、Go Kernel API 或 Protyle transaction。产品定制不得散落在同步文件中。

### 5.3 SiyuanMarkdownAdapter

Adapter 为核心提供稳定能力：

- 保存剪贴板图片和附件；
- 解析本地资源和安全图片 URL；
- 打开外部链接和文档链接；
- 提供 Mermaid、KaTeX 和代码高亮运行时；
- 创建思源图标、菜单和提示；
- 提供本地化标签；
- 报告可恢复错误；
- 提供思源图片缩放 UI 桥接。

Adapter 不解析 Markdown，不维护第二份文档内容。

## 6. 同步范围

### 6.1 直接同步或轻量兼容

- CodeMirror 基础：`changes`、`syntax`、`policy`、`preview`、`renderers`、`plugin`；
- 编辑行为：空行、Markdown editing、快捷键、格式化、插入、任务列表、块拖拽；
- 结构预览：标题、强调、链接、引用、列表、Callout、脚注、Frontmatter、水平线；
- 复杂块：代码块、数学公式、Mermaid、原始 HTML、表格；
- 图片数据逻辑：语法识别、安全 URL、属性解析、宽度序列化和源码范围映射；
- 剪贴板：HTML 转 Markdown、代码粘贴、表格片段合并、资源分类和异步占位；
- 编辑辅助：折叠、搜索、selection hold、trailing space、延迟解析、Vim 和 typewriter；
- 与上述模块对应的框架无关测试。

### 6.2 通过 Adapter 改接

- `@markra/shared` 中的图标、菜单定位、拖拽 MIME 和通用 UI helper；
- `@markra/markdown` 中核心需要的纯 Markdown helper；
- Tauri 本地文件和资源 URL；
- Mermaid、KaTeX、Lowlight 的宿主加载；
- 图片和附件保存；
- 链接打开、本地化及错误提示。

优先移植必要的纯函数，而不是引入完整 Markra 应用包。

### 6.3 明确排除

- `packages/editor-react`；
- AI preview、AI selection 和 `@markra/ai`；
- 自定义拼写检查和词典；
- Markra 应用菜单、标签页、工作区、主题和 CSS；
- Tauri 命令、文件管理、同步和产品设置。

## 7. UI 保持策略

不导入 Markra 应用 CSS。现有 `.markdown-editor` DOM 外壳保持稳定，Markra Core 生成的语义类全部限制在 `.markdown-editor` 命名空间内，并映射到：

- `--b3-theme-*`；
- `--b3-border-color`；
- `--b3-font-family-protyle` 和代码字体变量；
- `--b3-font-size-editor`；
- `b3-typography` 的标题、段落、引用、代码、表格和媒体视觉；
- 当前正文宽度、全宽模式和移动端间距规则。

替换不得无意改变：

- 标签页结构与高度；
- 标题、面包屑、模式按钮和状态图标的位置；
- 正文最大宽度、居中方式、字体、字号和行高；
- 明暗主题颜色；
- 桌面分栏和窄标签页行为；
- 移动端标题栏、正文留白和触摸命中区。

Markra Core 的 UI contribution 必须通过 Adapter 转换为思源图标、菜单和弹层。不得直接显示 Markra 风格的 Lucide 按钮或应用级浮层。

## 8. 图片缩放

图片采用“Markra 数据逻辑 + 思源原生交互 UI”。

### 8.1 UI 和交互

- 图片 Widget 使用思源现有 `.img`、内部 `span`、`img` 和 `.protyle-action__drag` DOM 结构；
- 复用思源现有缩放手柄样式、悬停显示、拖动遮罩、选中效果、鼠标指针和移动端触摸区域；
- 从 Protyle 图片缩放事件中抽取框架无关的几何计算和 pointer 生命周期，使 Protyle 与 Markdown 编辑器共享；
- 不使用 Markra 图片缩放按钮、控制柄、弹出菜单或相关 UI CSS。

### 8.2 数据写回

Protyle 继续使用其现有 `.sy` transaction。Markdown 编辑器在拖动结束时通过一次 CodeMirror Transaction 更新对应源码范围，并进入 CodeMirror 撤销历史。

普通 Markdown 图片在用户首次明确调整宽度时转换为带整数 `width` 的安全 HTML `<img>`；已有 HTML 图片只更新 `width`。未调整过的普通图片保持原始 Markdown 不变。图片受 `max-width: 100%` 约束，窄屏只限制显示宽度，不静默改写持久化宽度。

同一张图片在思源文档和 Markdown 文档中的手柄外观、出现时机、拖动方向、最小宽度和移动端触摸行为必须一致。

## 9. 剪贴板

使用 Markra 成熟的剪贴板分流和 HTML 转 Markdown 逻辑，替换当前以少量正则判断 Markdown 的实现。

```text
ClipboardEvent
  ├─ Markdown/plain text → 无损插入
  ├─ structured HTML → Turndown 转 Markdown
  ├─ table fragment → GFM 表格规范化
  ├─ image/file → Adapter 保存资源后插入链接
  └─ remote image → 按安全策略保存或保留远程引用
```

所有结果通过 CodeMirror Transaction 插入。异步资源处理必须保存并映射原始插入目标；用户在等待期间继续编辑时不得把图片插入到错误位置。转换失败时保留可插入的纯文本，不允许静默丢失剪贴板内容。

## 10. 渲染、错误和性能

- 单个 Renderer 失败只回退对应节点的原始 Markdown；
- Mermaid、KaTeX、图片和原始 HTML 失败不得阻止普通文本编辑和保存；
- 异步结果应用前验证文档版本和源码范围；
- 原始 HTML 和第三方渲染结果继续经过安全清理；
- 普通输入不得执行整篇 Markdown→HTML→Markdown 转换；
- Decoration 优先限制在可见范围和必要邻域；
- 长文、宽表格、大图和 Mermaid 不得扩大标签页面板宽度；
- 核心初始化失败时显示现有思源错误反馈，并保留原始 Markdown 的可恢复路径。

## 11. 上游同步策略

首次移植记录准确的 Markra 上游提交 SHA、原始路径和许可证信息。核心同步文件与 SiYuan Adapter 分目录管理，并维护一份排除清单。

后续同步按模块审查，不直接覆盖：

1. 比较记录的上游 SHA 与目标 SHA；
2. 接受框架无关的编辑器修复和测试；
3. 将新增平台调用翻译到 Adapter；
4. 拒绝 React、Tauri、AI、拼写检查和产品 UI 回流；
5. 运行核心测试、思源集成测试和 UI 基线检查。

## 12. 迁移顺序

1. 固定 Markra 来源 SHA，建立 Core、Adapter 和排除清单，不切换产品入口；
2. 移植基础语言、预览、selection/reveal、编辑命令和测试；
3. 移植剪贴板、表格、代码、公式、Mermaid、HTML 和图片数据逻辑；
4. 接入思源资源、链接、图标、菜单、语言和渲染服务；
5. 抽取并接入思源原生图片缩放 UI；
6. 在现有 `MarkdownEditor` 中切换到 Markra Core；
7. 删除旧 `livePreview` 实现和重复测试；
8. 完成长文、桌面、移动端及样式验收。

切换之前，旧实现只作为对照存在，不继续增加新能力。切换之后不保留用户可见的双内核开关。

## 13. 验证

### 13.1 自动测试

- 移植 Markra 核心对应测试，并适配当前测试运行器；
- Markdown/plain text/HTML/表格/图片/附件粘贴；
- 标题、列表、引用、代码、公式、Mermaid、HTML、脚注、Callout 和 Frontmatter；
- 表格编辑、图片缩放、任务切换和链接操作的精确源码 Transaction；
- `Cmd/Ctrl+A`、复制、粘贴、撤销、重做和模式切换；
- 异步图片插入目标在并发编辑后的正确映射；
- 加载、800ms 保存、revision 冲突和重新打开；
- `cd app && pnpm run lint`。

遵守项目规则，不使用 `pnpm build` 验证，不在开发者运行内核时擅自编译或重启 Kernel。

### 13.2 实际文档

使用《推送通知端到端技术方案.md》验证：

- 打开、粘贴、编辑、切换模式、保存和重载后原文无意外变化；
- 所有表格、Mermaid、图片、链接、代码和标题均显示；
- 宽表格、长行和大图片不撑宽标签页；
- 大文档输入、选择和滚动无明显阻塞。

### 13.3 UI 基线

保存替换前后同尺寸截图并检查：

- macOS 桌面浅色和深色；
- 普通标签页和窄分栏；
- 手机窄视口；
- 空文档、长文档、宽表格和图片选中/缩放状态。

允许 Markdown 内容渲染更完整，但页面框架、间距、颜色、字号和控件位置不得发生无意变化。

## 14. 完成标准

- 当前自研实时预览核心已由 Markra Core 替换，不存在两套长期编辑内核；
- React、Tauri、AI、拼写检查和 Markra 产品 UI 未进入当前项目；
- SiYuan UI 外壳和整体视觉保持一致；
- 图片大小编辑使用思源原生 UI 和交互；
- Markdown 原文是唯一内容真相，所有编辑进入 CodeMirror history；
- 用户参考长文的粘贴、显示、编辑、保存和重载通过；
- 桌面与移动端功能、宽度和样式验收通过；
- Markra 来源 SHA、排除项和后续同步方法有明确记录。
