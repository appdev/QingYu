# 轻羽 FluidKit 组件迁移清单

2026-11-24 · MiaoYanEditor / QingYu 0.1.3 · 基于 `1963f05` 和扫描时的工作区。目标包为 `@fluidkit/core@2.0.0`，官方源码参考 `fb77c28`。

建议将 **现有玻璃组件与自定义交互** 作为迁移范围，优先替换工具栏、按钮、菜单、设置开关、图片查看器工具栏及两个拖动面板。**菜单折射、两处渐进模糊、移动端长按预览** 需要单独确认效果和交互是否达标，再决定是否替换对应实现和卸载 `glassy-liquid`。截至文末「编辑器工具栏菜单改用 Liquid Blur 与 Motion」一节，编辑器工具栏菜单、分享浮层和 Tabs 已改用 FluidKit，剩余项见该节。轻羽使用 Vite、TypeScript 和原生 DOM，接入 `@fluidkit/core` 即可。FluidKit 的核心能力是材质、动效和手势，菜单内容、表单语义、业务状态及原生窗口仍由轻羽承载。[官方 Core 文档](https://fluidkit.mogilnikov.dev/core) · [官方源码](https://github.com/mikhailmogilnikov/fluid-kit/tree/fb77c28fceb06e9ecfbcfbd7aee856c3705e37e2) · [npm 2.0.0](https://www.npmjs.com/package/@fluidkit/core/v/2.0.0)

## 按端查看替换清单

**移动端**分手机 App（iPhone、Android）和 iPad App。**PC 端**分桌面 App（macOS、Windows、Linux）和 Web。Web 单列验收，工具栏菜单按 CSS 视口宽度切换手机档（<780 px）、平板档（780–900 px）和 PC 档（≥901 px）。手机或 iPad 浏览器打开 Web 时，部分交互还会走移动设备识别分支，表中单独标出。

| 组件 | 手机 App | iPad App | 桌面 App（macOS / Windows / Linux） | Web |
| --- | --- | --- | --- | --- |
| 编辑工具栏玻璃材质、按压和高亮 | P1 · 紧凑布局，核对独立返回按钮和工具显隐 | P1 · 宽屏工具栏，窄窗口切回紧凑布局 | P1 · 桌面编辑工具栏 | P1 · 按浏览器实际布局验收 |
| 列表「···／新建」胶囊与移动新建按钮 | P1 · 独立玻璃新建按钮 | P1 · 独立玻璃新建按钮 | P1 · 桌面列表胶囊，窄窗复核平面布局 | P1 · 桌面浏览器列表胶囊，移动浏览器独立新建按钮 |
| 图片查看器底部工具栏 | P1 · 深色材质与触控 | P1 · 深色材质与触控／指针 | P1 · 深色材质与鼠标／键盘 | P1 · 深色材质与浏览器全屏 |
| 设置玻璃开关 | P1 · 现有通用开关 | P1 · 现有通用开关 | P1 · 通用开关 + 菜单栏／系统托盘、置顶及禁用的「登录时打开」 | P1 · 现有通用开关 |
| 玻璃锚点菜单及二级菜单 | P2 · 手机档。笔记和列表「···」、窄屏分享、移动撤销后的重做及可见的查找／附件入口 | P2 · 宽屏 iPad 档、窄屏手机档。核对附件、查找、列表二级菜单和宽窄屏分享分流 | P2 · PC 档。笔记和列表「···」、附件、查找。系统右键菜单走原生入口 | P2 · 按三档验证同一渲染器。移动浏览器另核对重做及可用入口；Web 分享使用自己的浮层 |
| `?menu=glassy` 实验入口、SVG 折射与色散 | 实验入口参与 P2 对照；材质按 WebView 能力验收 | 同手机 App，分别检查宽窄屏 | P2 · 实验入口对照，按各宿主能力判定折射与色散 | P2 · 三档对照，重点核对 Chromium 折射、色散与其他浏览器降级 |
| 设置面板拖动 | P3 · 窄视口底部面板，宽视口居中 | P3 · <780 px 底部面板、≥780 px 居中；当前两者都按移动设备绑定标题拖动，分别验证 | 设置使用独立窗口，P3 回归 | 居中弹窗；桌面浏览器无标题拖动，移动浏览器当前会绑定标题拖动 |
| 笔记「移动到…」拖动面板 | P3 · 移动菜单入口 | P3 · 移动菜单入口，宽窄屏均验证 | P3 · 快速打开中的「移动笔记…」复用面板；另回归桌面右键子菜单 | P3 · 移动浏览器走移动菜单，桌面浏览器走快速打开命令；另回归桌面右键子菜单 |
| 笔记行、文件夹行长按预览 | P4 候选 · 触屏长按和滚动取消 | P4 候选 · 触屏长按，另验触控板右键和键盘入口 | 系统右键和对应浮层按现有路径回归 | 移动浏览器为 P4 候选；桌面浏览器右键菜单属于可选视觉扩展 |
| 列表、编辑器顶部渐进模糊 | P4 候选 · 列表头部和编辑器顶部 | P4 候选 · 宽窄屏的列表头部和编辑器顶部 | P4 候选 · 普通主窗 ≥901 px 的两处边缘，另验独立笔记窗口及吸顶分组 | P4 候选 · 桌面浏览器重点验 ≥901 px 和独立笔记视图，移动浏览器验移动布局 |

通用设置开关指各入口已经挂载的完成项自动下移、启动时打开今日笔记和自动同步。具体可见性随设置页和同步配置变化。设置面板拖动按设备识别绑定，面板的位置由原生宿主和视口共同决定。各端的可选扩展见第三节。

## 一、建议替换的现有组件

同一行列出共用实现的各个入口。P1–P4 对应下文迁移阶段。

| 阶段 | 现有组件和入口 | FluidKit 接入点 | 保留的交互及约束 |
| --- | --- | --- | --- |
| P1 | 笔记详情右上编辑工具栏 `.actions`，桌面笔记列表「··· / 新建」胶囊 `.notes-navigation` | Liquid Blur CSS、`installPress` 和 `createIndicator`，替换 `mountGlassToolbar` 的材质、按压及展开高亮 | 展开项才出现高亮圆，首次直接定位，换项才滑动。排除侧栏开关，保留方向键及 Home / End。窄屏列表导航切回现有平面布局 |
| P1 | 移动端 `#new-note-button` 玻璃新建按钮 | Liquid Blur CSS、`installPress`，替换 `mountGlassButton` | 保持触控尺寸、创建和编辑聚焦行为 |
| P1 | 设置页玻璃开关。已接入的菜单栏／系统托盘、完成项自动下移、窗口置顶、启动时打开今日笔记、自动同步，以及禁用的「登录时打开」 | `createDrag`、Liquid Blur CSS，替换 `glassToggle` 的指针与滑块实现 | 保持原生 checkbox、`role=switch`、标签、禁用、键盘点击、偏好持久化和回写。拖动释放只提交一次 |
| P1 | Markra 图片查看器底部深色工具栏。正文图片、代码块图片预览入口共用 | Liquid Blur CSS，替换 `media-viewer.ts` 中的 `glass(toolbar)` | 保持缩放、平移、上一张／下一张、全屏、背景遮罩、焦点和系统栏状态。查看器平移与缩放是独立的手势实现 |
| P2 | 工具栏锚点菜单。笔记详情「···」、列表「···」、添加附件、窄屏移动 App 分享、查找选项、移动端长按撤销后的重做 | 编辑器工具栏 `.actions` 的菜单已改用 Liquid Blur 和 `fk-transition` 淡入淡出；其余锚点仍走 `createMorph` 和旧材质，待统一 | 共用 `showMenu({ anchor })`。保持桌面下方和键盘栏向上定位、按钮二次点击关闭、编辑器焦点、异步菜单作废及剪贴板所需用户激活 |
| P2 | 工具栏菜单二级子菜单。已发现的「排序方式」「按日期分组」，以及同一递归渲染入口 | 统一 Liquid Blur 材质。若需迁移二级展开动画，复用 Motion | 保持子菜单回退、菜单宽度、禁用项、勾选项、键盘导航和边界翻转 |
| P2 | `?menu=glassy` 工具栏菜单实验分支 | 用新旧材质对照验证，完成对照后随旧库分支退役 | 目前依赖旧 Web Component 的 Shadow DOM 和私有 spring，先用它作为对照基线。记录实验 URL 退役决定 |
| P3 | 移动设备设置面板的拖动与回弹，覆盖手机／iPad App 和移动浏览器 | `createDrag`，按现有 `bindPanelDrag` 边界替换，按需使用 Motion | 保持原生 `dialog`、底部／居中样式、设置分级返回、历史栈、点外关闭、释放阈值和拖动取消。图片分享同用容器，当前入口未启用此拖动 |
| P3 | 笔记「移动到…」文件夹选择面板。含移动端普通笔记和废纸篓恢复后的移入流程，以及桌面 App／Web 快速打开的「移动笔记…」命令 | `createDrag`、Motion、按需 Liquid Blur | 保持目标树、当前／禁用项、短拖复位、选中和关闭后的焦点，核对轨迹与关闭阈值 |

## 二、满足条件后再迁移的组件

| 阶段 | 现有组件 | 候选能力 | 决策条件 |
| --- | --- | --- | --- |
| P2 | 工具栏、移动新建按钮、一级和二级玻璃菜单在 Chromium 上的 SVG 折射、色散 | Liquid Blur CSS + 可继续独立保留的 CSS/SVG 层 | 编辑器工具栏菜单及其二级菜单已接受改用 CSS 玻璃，`AGENTS.md` 已同步。列表「···」、查找条件、重做和键盘栏附件菜单仍使用 SVG 折射。FluidKit 2.0 的公开玻璃材质没有等价的 SVG 折射、色散参数。验收当前 25 px 菜单折射边缘、4.5 px 模糊、色散 1 等视觉规范。只有实测通过或确认改用 CSS 玻璃后，才移除旧材质和仍有调用的滤镜资产 |
| P4 | 笔记列表标题及吸顶年份分组渐进模糊，编辑器内容顶部渐进模糊 | 两者分别对照 `.lb-edge` | 现有覆盖层在内容上做多层背景模糊并配合底色；`.lb-edge` 在滚动元素上做透明蒙版，依赖滚动驱动动画。逐个验收顶部、滚动中、底部、短内容和 WebView 降级。严格保留当前视觉时继续使用渐进模糊层 |
| P4 | 移动端笔记行和文件夹行长按预览菜单。包括普通笔记、多选、废纸篓及文件夹菜单的条件分支 | `createLift`，替换长按计时、抬升预览与遮罩的可替换部分 | 先验证菜单可在保存草稿、结束重命名和计算异步菜单项后安全打开。现实现用 450 ms 阈值、滚动取消、额外快捷操作、回收站和根目录分支、独立的右键及键盘入口。Lift 也监听右键与菜单键并使用顶层 dialog，需避免双菜单、重复动作和误打开，不为无预览分支强制加预览 |

## 三、可选的视觉扩展

这些入口有对应能力，当前清单的估时不含这些扩展。它们都可以独立于旧库移除来评估。

- 手机 App、iPad App · 图片外观分段按钮、设置选择菜单、键盘快捷工具组、格式与表格面板、移动侧栏导航。iPad 宽屏另有分享浮层。
- 桌面 App · 分享浮层与选项卡、设置分类、桌面格式菜单、快速打开、笔记排序和文件夹展开。
- Web · 分享浮层与选项卡、浏览器右键菜单和桌面浏览器上的快速打开及设置分类。移动浏览器另覆盖已启用的移动设置选择、快捷工具组、格式与表格面板和侧栏导航。
- 各端现有入口 · 状态胶囊、笔记链接选择器、查找面板、日期选择器及斜杠建议。

| 组件 | 对应能力 | 代码位置与边界 |
| --- | --- | --- |
| 桌面分享浮层、文字／图片／Web 分享选项卡 | 已完成：Liquid Blur、Motion 淡入淡出、Indicator | `reference-panels.ts`、`style.css`。保留原生 Popover、焦点、分享预览及图片导出 |
| 图片分享「浅色／深色」分段按钮 | Indicator | `note-picture.ts`、`note-picture.css`。颜色样本的选中边框单独处理 |
| 设置左侧分类、移动端设置选择菜单 | Indicator、Morph | `settings-view.ts`、`settings-select.ts`、`settings.css`。桌面原生 select、移动端历史栈和键盘语义继续由轻羽维护 |
| 编辑器键盘上方快捷工具组、建议条、收键盘按钮 | Liquid Blur CSS，横向工具组可试 `.lb-edge-x` | `main.ts`、`style.css`、`editor-viewport.ts`。验证 WKWebView 保持选区和键盘状态。短工具组无溢出时不需要边缘动效 |
| 底部报错与同步进度胶囊 | Liquid Blur CSS | `main.ts`、`style.css`。保留读屏播报、进度和错误文案、侧栏可见时的同步状态切换 |
| 桌面格式菜单、移动端格式与表格面板 | Morph 或 Motion | `main.ts`、`editor-format-panel.ts`、`editor-table-panel.ts`、`editor-viewport.ts`。只试视觉层，保留非模态输入面板、选区和 caret 可见性 |
| 笔记链接选择器、快速打开、查找面板、日期选择器、编辑器斜杠建议 | Indicator 或 Motion。现有横向表格溢出遮罩、斜杠菜单底部可单独评估 Scroll Edge | `reference-panels.ts`、`quick-open.ts`、`editor-find.ts`、`editor-note-dates.ts`、`editor-slash-menu.ts`、`rich-editor.css`。保留输入法、虚拟焦点、原生滚动及已有遮罩语义 |
| 侧栏笔记排序和文件夹展开、移动端侧栏导航切换 | Layout 或 Motion | `sidebar.ts`、`sidebar.css`。重建列表节点时要先验证稳定身份与节点复用。导航保留浏览历史、`inert` 和焦点恢复 |
| 非锚点的 Web 右键菜单，如编辑器选区、代码块、链接、图片、任务清单和表格菜单 | Liquid Blur / Indicator，若有统一菜单外观需求可共用 `popup-menu.ts` 的浏览器样式 | `rich-editor.ts`、`editor-table-panel.ts`、`note-menu.ts`、`folder-menu.ts`。原生右键菜单及编辑器原生选区菜单走原路 |

公共操作按钮、提示框、账号和同步表单、日历、版本历史、桌面窗口与系统菜单、侧栏宽度拖动、Markdown 编辑器正文和表格编辑按原实现评估。任务清单完成动画、编辑器块拖动和图片缩放涉及 CodeMirror 事务、撤销和原生选区，当前迁移没有替换要求。官方 Text Morph 示例引用独立的 `torph` 包，本计划无需引入。 [FluidKit 全部能力](https://github.com/mikhailmogilnikov/fluid-kit#whats-inside) · [Scroll Edge](https://fluidkit.mogilnikov.dev/edge) · [Lift](https://fluidkit.mogilnikov.dev/lift) · [Text](https://fluidkit.mogilnikov.dev/text)

## 四、文件和依赖

递归扫描了 Git 跟踪及未忽略的工作区源码，排除了二进制、生成依赖目录和锁文件；依赖表另核对了 npm 清单。所有 400 个匹配文件读取并提取静态线索，前端通过 TypeScript AST 和 HTML/CSS 引用做依赖索引，候选组件及相邻调用点逐项检查。长文件通过入口、声明和相关代码段定位。Rust、Kotlin、Swift 按原生 UI 边界核对，图中没有对这些语言做语义依赖解析。

| 层 | 文件数 | 行数 | 处理归属 |
| --- | ---: | ---: | --- |
| App / Web 自有源码及页面入口 | 103 | 20,397 | 下表按阶段分配候选及验证入口，其余归入现有功能保留 |
| 内嵌 Markra 和 mdsync | 112 | 38,013 | 只有媒体查看器玻璃工具条进入 P1，其余纳入回归边界 |
| Tauri 与原生宿主 | 74 | 27,949 | 桌面窗口、系统菜单、原生手势与状态栏的回归边界 |
| 根目录测试 | 55 | 10,031 | 相关测试见下表，其余按 CI 回归 |
| 工具、原型、实验及根配置 | 56 | 10,428 | Vite 入口检查、依赖管理，其余没有生产迁移事项 |

文件尺寸分布为小于 50 行 118 个、50–149 行 114 个、150–299 行 68 个、300–499 行 41 个、500 行以上（含 500）59 个。前端超过 1,000 行的自有源码为 `main.ts`、`settings-view.ts`、`workflow-copy.ts`，其中前两者有直接组件入口。

依赖图提取到 754 条可解析的内部引用，含类型导入、动态导入、重导出和样式引用。应用现有主路径为 `index.html → main.ts → glass-controls / sidebar / popup-menu / open-settings → feature-panel`。`settings.html → settings-page → settings-view → glass-controls`。`feature.html → feature-page → feature-panel / note-picture`。Markra 的 `image.ts`、`code-block.ts` 调用媒体查看器。登录、菜单栏和服务端同步 worker 核对样式及打包隔离，菜单栏若共享样式，避免因此初始化全局手势。

图中有五组强连通分量。相关两组是 `popup-menu ↔ glassy-menu-preview`（反向为类型引用，可随旧预览退出消失）和 `note-picture ↔ picture-window`（已有动态窗口路径，两端作为一组验证）。其余是 `ui-language ↔ workflow-copy`、`daily-note / note-links / notebook / server-library / sync-files` 和 Markra `blocks ↔ heading-level`，迁移不依赖拆环。

新增依赖仅 `@fluidkit/core` 与其样式。CodeMirror／Lezer、Tauri、Markdown 解析及渲染、DOMPurify、html2canvas、KaTeX、Mermaid、服务器与测试工具链均沿用。旧库有 **7 个生产源码文件直接导入**，包括容易遗漏的 Markra 媒体查看器以及仅使用 `clamp` 的 `context-sheet.ts`。旧库最终下线还需检查两个 mock、滚动组件标签、预览 URL 和 CSS 属性。

### 分文件处理

风险为 C（实现复杂度）/ D（影响面）/ V（近期变化）/ T（验证难度）/ E（外部耦合），各项 1–5，按 25% / 30% / 15% / 15% / 15% 加权。范围以单个阶段的实际变更为准。

| 文件 | 阶段 | 所属层与迁移事项 | 风险 C/D/V/T/E → 分数 |
| --- | --- | --- | --- |
| `package.json`, `package-lock.json` | P0/P4 | 构建。固定新包版本，旧依赖在所有入口迁完后清理 | 1/4/2/2/4 → 2.65 |
| `src/style.css` | P0/P1 | 样式入口。引入 FluidKit CSS，检查层叠、独立入口和移动按钮样式 | 2/5/2/3/3 → 3.20 |
| `src/glass-controls.ts` | P1 | 共享组件。保留既有挂载入口，替换材质、lens 和开关手势 | 4/4/3/3/4 → 3.70 |
| `src/sidebar.css` | P1/P2/P3/P4 | 样式。拆批对照工具栏、菜单一级／二级、移动笔记面板、两个滚动边缘 | 4/5/3/4/4 → 4.15 |
| `src/settings.css` | P1 | 样式。开关轨道、键盘焦点、禁用和减少透明度 | 2/3/4/3/3 → 2.90 |
| `src/main.ts` | P1/P4 | 主入口。保持工具栏挂载时机，核对移动编辑器菜单及最后的 blur 注册 | 3/5/4/4/4 → 4.05 |
| `src/settings-view.ts`, `src/settings-page.ts` | P1 | 设置组装和独立入口；验证所有开关接入、异步回填与关闭清理。预计可保留原调用签名 | 3/4/4/3/3 → 3.45 |
| `src/vendor/markra/editor/codemirror/media-viewer.ts` | P1 | 组件。仅调整深色工具条，核对图片入口及代码块入口；记录 vendor 补丁 | 2/3/2/3/3 → 2.60 |
| `src/rich-editor.css` | P1 | 样式。仅核对看图工具条的层级、深色填充和减少透明度 | 2/3/3/3/3 → 2.75 |
| `src/popup-menu.ts` | P2/P4 | 共享组件。替换根菜单形变、玻璃材质、子菜单表面；按阶段保留原生分支与浏览器分支 | 5/5/3/4/5 → 4.55 |
| `src/toolbar-menu-glass.ts`, `src/glassy-menu-preview.ts` | P2/P4 | 菜单材质参数与旧实验。对照完成后移除旧导入和查询参数入口 | 3/3/2/4/5 → 3.30 |
| `src/feature-panel.ts`, `src/feature-panel.css` | P3 | 共享组件。接管设置面板拖动与复位，保留弹窗生命周期和跨平台窗口分派 | 4/4/3/3/4 → 3.70 |
| `src/open-settings.ts`, `src/note-picture.ts`, `src/feature-page.ts` | P3 | 页面调用方。验证设置的历史返回和退出，验证图片分享有内联、独立窗口及 macOS 特例 | 3/3/3/3/3 → 3.00 |
| `src/note-menu.ts` | P2/P3/P4 | 组件。分别验证详情锚点、移动面板、长按内容；保留操作和异步保存 | 4/4/3/3/4 → 3.70 |
| `src/sidebar.ts`, `src/folder-menu.ts` | P2/P4 | 页面及组件。排序二级菜单、列表滚动层、笔记和文件夹长按及焦点路由 | 5/4/4/4/5 → 4.40 |
| `src/context-sheet.ts`, `src/context-sheet.css` | P4 | 组件。长按试验通过才整体替换；若保留原预览，可单独替换基础限幅函数 | 3/3/2/4/4 → 3.15 |
| `index.html`, `public/assets/toolbar-glass-map.svg` | P2/P4 | 页面及视觉资产。独立核对旧滤镜和编辑器滚动覆盖层的引用后决定去留 | 2/4/2/4/4 → 3.20 |
| `src/reference-panels.ts`, `src/editor-find.ts`, `src/editor-history-menu.ts`, `src/rich-editor.ts` | P2 | 菜单调用方。保留接口，回归分享、查找、重做及编辑器右键；确认锚点和焦点语义 | 3/4/4/4/3 → 3.60 |
| `AGENTS.md` | P2/P4 | 规范。以验收结论更新实现依赖和视觉约束，菜单内容与尺寸继续以项目规则为准 | 1/2/2/2/2 → 1.75 |

相关测试按入口定位：

- P1 · `settings-general.test.ts`、`settings.test.ts`、`buttons.test.ts`、`asset-download-view.test.ts`、`editor-image-click.test.ts`。复核旧玻璃材质 mock 和媒体查看器交互。
- P2 · `note-menu.test.ts`、`folder-tree.test.ts`、`share.test.ts`、`markra-editor.test.ts`。检查打开方式、键盘导航、子菜单、分享和用户激活、焦点恢复。查找条件菜单暂无专门用例，加入浏览器冒烟验证。
- P3 · `settings-mobile.test.ts`、`note-menu.test.ts`、`note-picture.test.ts`、`share.test.ts`。检查拖动取消、关闭、历史返回、焦点和共用容器。
- P4 · `mobile-navigation.test.ts`、`folder-menu.test.ts`、`note-menu.test.ts`、`note-selection.test.ts`。检查真实滚动边缘、滚动取消长按、多选和预览变体。将旧组件标签断言更新为可验证的行为。

## 五、迁移顺序、估时与回退

| 阶段 | 可独立验收的交付 | 基础工时 | 含 20% 余量 |
| --- | --- | ---: | ---: |
| P0 | 新包和样式入口。用现有按钮确认 CSS、主题、减少动态效果和减少透明度 | 3–4 h | 3.6–4.8 h |
| P1 | 两个工具栏、新建按钮、设置开关、图片查看器工具条 | 8–12 h | 9.6–14.4 h |
| P2 | 统一工具栏菜单形变、二级菜单与材质对照，按验收决定旧试验分支去留 | 12–18 h | 14.4–21.6 h |
| P3 | 设置和移动笔记两个拖动面板 | 8–12 h | 9.6–14.4 h |
| P4 | 两处边缘试验、长按 Lift 试验、按验收清理旧依赖 | 12–20 h | 14.4–24.0 h |
| 合计 | 基础工时含各阶段约 45 分钟的提交、评审和集成开销 | **43–66 h** | **51.6–79.2 h** |

一人约 7–10 个工作日，含 P4 试验和正常验证时间，实际取决于设备可用性和视觉取舍。每阶段独立提交并按逆依赖顺序回退。P4 清理提交恢复后仍可回退前面的组件批次。P1 与 P2 均触及 `sidebar.css`，P2 与 P4 共用菜单入口，适合按表串行集成。

## 六、验收重点

- 迁移前留存 Chrome／Safari 的亮暗主题基线，明确是否接受纯 CSS 玻璃和滚动蒙版。已有折射效果及减少透明度样式按项目规范测量。
- 手机 App 分别覆盖 iPhone WKWebView 和 Android System WebView，验证长按与滚动竞争、拖动阈值、点外关闭、焦点恢复及软键盘与输入法。
- iPad App 覆盖竖屏、横屏和跨越 780 px 的窗口变化，核对宽窄屏菜单、触控板右键、设置拖动和分享入口。
- 桌面 App 分别覆盖 macOS、Windows 和 Linux，核对原生右键、键盘焦点、独立窗口、渐进模糊及透明度降级。图片分享覆盖 macOS 模态弹窗及其他桌面端独立窗口。
- Web 核对 779/780 px 和 900/901 px 两个断点及移动浏览器入口。检查菜单宽高、分组间距、首项高亮、子菜单、屏幕下边缘和点击穿透。
- 按阶段运行对应 Vitest，冻结代码后执行 App/Web 构建并检查包体差异。最终按 `glassy-liquid`、`glass-progressive-blur` 和 `menu=glassy` 逐项复查生产入口和 mock。仍有视觉或运行时依赖时保留旧包。

## P0 / P1 实施与验证

已固定 `@fluidkit/core@2.0.0` 并接入共享样式。编辑和列表工具栏、移动新建按钮、设置开关及图片查看器工具栏已切换到 FluidKit。菜单折射、渐进模糊和长按预览继续使用现有实现与依赖。

| 端 | 验证结果 |
| --- | --- |
| 手机 App | iPhone 17 Pro 模拟器通过 XCTest，覆盖设置开关、新建笔记和详情菜单。Android vivo V2307A 已安装调试 APK，验证 WebView 中的触摸开关点击与拖动、工具栏和新建按钮材质，以及现有玻璃菜单和滚动边缘入口 |
| iPad App | iPad Pro 13 英寸模拟器通过 XCTest，覆盖居中设置弹窗中的开关、新建笔记和详情菜单 |
| 桌面 App | macOS 调试包构建并启动，已核对独立设置窗、开关及禁用项的辅助功能树。会话锁屏后，桌面 App 的交互复验未完成。Windows、Linux 暂无实包验证 |
| Web | 生产构建通过。Chrome 和 WebKit 分别覆盖桌面入口，WebKit 另覆盖手机及 iPad 布局。验证浅／深色、工具栏高亮、开关点击／键盘／拖动／取消，以及图片预览、缩放和关闭。Chrome 另核对减少透明度的实底降级，以及 779/780 px、900/901 px 的现有菜单宽度和行高 |

11 个相关 Vitest 文件共 111 项通过，包含使用 FluidKit Drag 的开关回归用例。图片查看器的本轮交互证据来自 Web 和测试，原生端实包尚未复验该入口。

## P2 / P3 和 Tabs Switch 实施与验证

- **Tabs Switch** · 分享的「发送／网页／图片」、图片外观的「浅色／深色」及桌面设置分类已接入 FluidKit Indicator。横向分段支持拖到另一项后松开选择，键盘和读屏状态由现有控件更新。后续已按 FluidKit Playground 的 Tabs 示例改为胶囊轨道加实色滑块，`mountTabIndicator` 不再提供玻璃滑块参数。
- **Corner Menu** · 工具栏一级菜单的转角形变使用 FluidKit `SpringAnimator`，按触发按钮和面板的相对位置展开，开到一半可收回。二级菜单使用 FluidKit 的交互动效曲线。现有菜单布局、焦点处理、SVG 折射和 `?menu=glassy` 对照入口继续保留。
- **拖动面板** · 设置面板与「移动到…」文件夹选择面板使用 FluidKit Drag。短拖、取消、展开／收回和长拖关闭仍由各面板自己的状态和历史栈控制。
- **P4** · 两处渐进模糊与笔记／文件夹长按预览继续使用现有实现。Scroll Edge 使用透明蒙版。Lift 需要另外验收长按后异步保存、结束重命名及预览菜单的选择流程。

| 端 | 验证结果 |
| --- | --- |
| 手机 App | iPhone 17 Pro 模拟器通过设置开关、新建和 Corner Menu。Android vivo V2307A 调试包通过抓取设置标题区的短拖、长拖和菜单中途收回 |
| iPad App | iPad Pro 13 英寸模拟器通过 Corner Menu 与分享 Tabs，切换到「网页」会进入对应内容 |
| 桌面 App | macOS 调试包通过原生运行检查，覆盖笔记菜单和独立设置窗内的分类切换。Windows 和 Linux 未做实包验证 |
| Web | 生产包在 Chrome、WebKit 桌面和 WebKit 的手机／iPad 布局通过冒烟，覆盖菜单开合、Tabs 切换及两处面板拖动 |

App 和 Web 前端构建通过，相关 Vitest 共 254 项通过。

### 设置 Switch 的透镜反馈

设置开关拖动时，滑块会放大并切为透明透镜，松手后吸附至对应位置。启用「减少动态效果」时保留吸附和开关语义，省略放大。相关 Vitest 共 28 项通过。Chrome、WebKit 和 Android 真机调试包验证了透镜、缩放和吸附。macOS 独立验收包验证了指针拖动切换及原值恢复。正式 App 与 TOS 联调包沿用其原版本。

### Corner Menu 接入 Morph

> 本节记录 Morph 对照阶段。当前锚点菜单使用下一节所述的 Liquid Blur 和 Motion。

默认工具栏锚点菜单已用 FluidKit `createMorph` 替换 P2 原先的手写形变。面板和触发按钮的图标由 Morph 处理展开和收回，开到一半可以反向。过渡期间使用 FluidKit 玻璃，面板落定后恢复现有 SVG 折射。系统减少动态效果时立即开合。

218 项相关 Vitest 通过。Web 生产包通过 Chrome 桌面及 WebKit 的桌面、手机、iPad 布局验证。Android 调试包真机确认了 Morph 动画层、中途关闭、动画层清理和折射恢复。macOS 独立验收包已启动并验证了菜单开关。这次 Morph 改动未复验原生 iOS、Windows 或 Linux。

## 编辑器工具栏菜单改用 Liquid Blur 与 Motion

下面先记录编辑器工具栏的迁移，再更新其余锚点菜单的状态。`AGENTS.md` 的工具栏菜单规范已同步。

- **编辑器工具栏菜单** · 由 `.actions` 内按钮打开的一级和二级菜单使用 `lb qingyu-glass` 加 `fk-transition fk-opacity`，在最终位置淡入淡出，时长 `120ms`，曲线为 `--fk-interactive-easing`。不再使用 Morph 形变和 SVG 折射，减少透明度时的实底由 FluidKit 处理。判断条件见 `src/popup-menu.ts` 的 `fluidToolbar`，`?menu=glassy` 打开时仍走旧预览。
- **工具栏高亮** · 编辑工具栏的 Indicator 改为 `trigger: 'press'`，按下时跟手，空闲时停在已展开的按钮上。列表胶囊仍为 `manual`，只跟随展开项。
- **分享浮层** · `.share-popover` 使用 `lb qingyu-glass` 和 `fk-transition fk-opacity`，与编辑器工具栏菜单同一淡入淡出参数。
- **Tabs** · 分享选项卡、图片外观分段和桌面设置分类改为胶囊轨道加实色滑块，滑块颜色由 `--tab-indicator-fill`、`--tab-indicator-shadow` 控制。纵向设置分类只响应点击，横向分段可拖选。

### 剩余替换项

| 剩余项 | 位置 | FluidKit 能力 | 说明 |
| --- | --- | --- | --- |
| 编辑器顶部渐进模糊 | `index.html` 的 `glass-progressive-blur`，`src/main.ts` 注册 | Scroll Edge `.lb-edge` | 多层模糊与透明蒙版效果不同，需先确认视觉 |
| 笔记列表顶部和吸顶分组渐进模糊 | `src/sidebar.ts` | Scroll Edge | 同上，另需处理 `--notes-edge` |
| 笔记行、文件夹行长按预览 | `src/sidebar.ts` 的 450 ms 计时、`src/context-sheet.ts` | Lift | 条件见第二节。预览定位已使用本地边界计算 |

列表「···」及其「排序方式」「按日期分组」、查找条件、移动端重做和键盘栏附件已统一使用 `src/popup-menu.ts` 的 Liquid Blur 和 FluidKit Motion。旧 Morph 路径、`?menu=glassy` 入口和菜单相关 mock 已退役。

`glassy-liquid` 目前用于两处渐进模糊。迁移后需要更新 `tests/mobile-navigation.test.ts` 对 `glass-progressive-blur` 的断言。
