# Markdown 编辑器整体表现契约验证

## 验收结论

本轮已完成整体表现契约、思源主题解析、原生等价内容、Markdown 专属控件、源码模式、移动端状态和实际应用矩阵的实现与验证。自动测试、布局测试、实际应用矩阵、类型检查和项目 Lint 均以退出码 `0` 完成；51 个契约全部取得运行态证据，未覆盖项为空。

严格完成条件仍保留一项已知差异：标准第三方主题、320px、关闭代码换行时，原生 `.hljs` 拥有块内 `overflow: auto`，横向滚动条占用 10px；CodeMirror 使用整个编辑器的 `.cm-scroller`，连续可编辑代码行无法共享原生块内滚动容器，因此代码块总高度相差 10px。500px 精确代码块测试中背景、圆角、字体、首行位置、语言位置、操作组位置、总高度和语法颜色均为 0px 差异。该差异未使用固定高度或宽度专用 CSS 掩盖，后续若要求 320px 也达到 1px，需要重构代码块为可编辑的单一块级滚动容器。

因此，本轮自动化门禁通过，整体主题与组件适配已落地；若按设计文档“所有关键几何不超过 1px”的绝对标准判断，严格整体验收尚差上述一项架构性滚动容器差异。

## 运行态矩阵

- 契约数量：51
- 已取得证据：51
- 未覆盖项：无
- 矩阵行：6
- 截图数量：7
- 最大受控几何差值：10px
- 主题专用补丁：无
- 标准第三方主题语义回退：无；测试主题同时覆盖思源标准变量和标准 Protyle 选择器
- 运行态报告目录：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH`
- 汇总报告：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/summary.json`

矩阵覆盖：

| 主题 | 模式 | 平台 | 宽度 | 输入方式 |
| --- | --- | --- | --- | --- |
| daylight | visual | desktop | 500px | mouse |
| midnight | visual | desktop | 500px | keyboard |
| standard-third-party | visual | desktop | 320px | mouse |
| daylight | visual | mobile | 375px | touch |
| midnight | source | desktop | 500px | keyboard |
| standard-third-party | source | mobile | 375px | touch |

每行运行并测量基础、聚焦、选区、拖拽、剪贴板上传、错误、媒体查看和展开浮层状态；另有独立空文档状态。深色视觉模式额外断言段落和一级标题与原生计算颜色完全相等；移动视觉模式额外断言表格工具栏默认隐藏、表格聚焦后显示。

## 实际应用证据

测试通过 Chrome DevTools Protocol 驱动已启动的 QingYu Electron 应用，使用真实 Lute BlockDOM、真实 CodeMirror `EditorView` 和当前思源主题 CSS 创建不落盘的并排夹具。结束后检测结果为：页面 `readyState=complete`、测试夹具已卸载、页面可见、现有 Markdown 编辑器仍在；未调用创建、保存或删除文档 API。

代表性截图：

- 浅色桌面视觉：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/0-daylight-visual-desktop-visual.png`
- 深色桌面视觉：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/1-midnight-visual-desktop-visual.png`
- 第三方主题窄页签：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/2-standard-third-party-visual-desktop-visual.png`
- 移动端视觉：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/3-daylight-visual-mobile-visual.png`
- 深色源码模式：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/4-midnight-source-desktop-visual.png`
- 第三方主题移动源码：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/5-standard-third-party-source-mobile-visual.png`
- 空文档：`/var/folders/df/wysbnwxj4qvdypflggngkjcw0000gn/T/qingyu-markdown-appearance-frZsLH/empty-daylight-visual-desktop.png`

## 验证命令

| 命令 | 结果 |
| --- | --- |
| `cd app && pnpm exec node --import tsx --test src/markdown/*.test.ts src/markdown/appearance/*.test.ts src/protyle/codeLanguageMenu.test.ts` | 退出码 0；109/109 通过；0 skipped；0 cancelled |
| `cd app && pnpm run test:markdown-layout` | 退出码 0；宽表格、图片、标题、工具栏和代码块精确布局通过 |
| `cd app && pnpm run test:markdown-live-preview` | 退出码 0；真实应用源码/视觉切换通过 |
| `cd app && pnpm run test:markdown-appearance` | 退出码 0；6 行、51/51 契约、7 张截图、未覆盖项为空 |
| `cd app && pnpm run typecheck` | 退出码 0 |
| `cd app && pnpm run lint` | 退出码 0；0 errors；0 warnings |
| `cd app && pnpm exec node --import tsx --test src/markdown/markraIntegration.test.ts src/markdown/appearance/markdownControls.test.ts` | 退出码 0；lint 清理后的 14/14 聚焦回归通过 |
| `git diff --check` | 退出码 0；无输出 |

未运行 `pnpm build`、`pnpm dev` 或 `npx webpack`。为实际应用自测使用了独立的程序化开发监听器，输出仅用于本地应用加载；`app/stage/build/**` 没有进入 Git 变更。

## 已验证的根因修复

- 主题解析器从主题作用域内的 Protyle 探针读取标准变量，并仅在作用域缺失时回退到应用根变量，修复深色和第三方主题仍写回浅色快照的问题。
- 运行态主题夹具加载仓库当前 daylight/midnight CSS，并隔离到夹具与探针，不修改应用根主题。
- 表格工具栏改为块内浮动层，默认不占正文高度；桌面悬停/聚焦显示，移动端进入表格后显示。
- Blockquote 不再错误复用原生 `.bq` 的容器布局类；首尾行形成连续原生引用外观，Callout 使用独立契约。
- 代码块复用原生操作栏层级、语言菜单、图标、配置和思源主题参数；宽屏精确布局测试的所有关键值相等。
- 图片保留固有尺寸，并给原生拖动手柄预留编辑区边界；表格、代码块和图片均无编辑器横向溢出。
- 空内容、真实选区、拖拽指示、剪贴板上传、错误、媒体查看、搜索、脚注和代码语言浮层均使用真实运行态 DOM 进入矩阵。
- 已移除旧 `markdownThemeBridge`、`--b3-markdown-*` 视觉桥变量及迁移组件的重复视觉 `baseTheme`；策略测试阻止独立色板和未登记可见选择器回流。

## 冻结状态

- `git diff --check`：通过。
- `app/stage/build/**`：无 Git 变更。
- 工作区仍包含本任务链路开始前及本轮形成的未提交改动；冻结检查统计为 39 个已修改、2 个已删除、22 个未跟踪路径。
- 已跟踪差异摘要：41 个文件，1569 行新增、1360 行删除；未跟踪文件不计入该统计。
- 未执行 `git commit`、`git push`、合并或发布。
