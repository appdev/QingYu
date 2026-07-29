# 轻语默认臻楷主题设计与制作说明

本文既是轻语两套默认主题的设计说明，也是第三方主题作者可以逐项检查的完整案例。默认主题是前端基础资源，源码位于：

- `packages/app/src/themes/light.css`
- `packages/app/src/themes/dark.css`
- `packages/app/src/themes/zhenkai-font.css`
- `packages/app/src/themes/fonts/`
- `packages/app/src/themes/licenses/`

对应的主题 ID 是 `light` 和 `dark`。它们是应用内置、受保护且不可删除的默认主题；用户没有保存主题选择时，轻语会根据系统明暗模式直接使用其中一套。`classic-light` 和 `classic-dark` 保留修改前的中性系统主题，同样属于不可删除的前端内置主题，但不是默认值。

这四个主题由前端注册表 `packages/app/src/themes/registry.ts` 直接提供，不写入用户主题目录，不参与 Rust 端目录扫描、迁移、导入、替换或删除。第三方主题仍由原生主题目录和安全校验链负责。构建时，桌面端和 Web 端会把 9 份臻楷 WOFF2 子集以及 3 份许可证/来源记录写入前端产物，并执行 `verify:theme-assets` 防止漏包或重复分发。

完整的包结构、校验规则和导入流程请同时阅读 [轻语主题制作规范](theme-authoring.md)。

## 设计目标

默认臻楷主题遵循四条原则：

1. 编辑器是视觉中心，应用外壳保持安静。
2. 明暗主题共享语义和排版节奏，但各自设计对比度，不做机械反色。
3. 参考妙言的中文阅读气质和颜色角色，不复制其组件、选择器或运行时代码。
4. 所有应用外壳样式通过轻语的稳定语义令牌完成；编辑器内部选择器只用于稳定令牌无法表达的 Markdown 细节。

参考来源包括妙言的[排版规则](https://github.com/tw93/MiaoYan/blob/2571ab5c48b5b4e1271a64bfe40990b9957e2d53/Resources/DownView.bundle/css/typography.css)、[浅色变量](https://github.com/tw93/MiaoYan/blob/2571ab5c48b5b4e1271a64bfe40990b9957e2d53/Resources/DownView.bundle/css/theme-light.css)和[深色变量](https://github.com/tw93/MiaoYan/blob/2571ab5c48b5b4e1271a64bfe40990b9957e2d53/Resources/DownView.bundle/css/theme-dark.css)。这些链接用于说明设计出处，不构成兼容层或运行时依赖。

## 字体：霞鹜臻楷 Regular 与合成粗体

主题字体来源于官方维护的 [霞鹜臻楷 GB](https://github.com/lxgw/LxgwZhenKai)。它是从霞鹜文楷加粗衍生、针对屏幕阅读重新调整笔画的独立字体，并不属于霞鹜文楷的多字重系列。轻语固定使用官方 v0.825 的 `LXGWZhenKaiGB-Regular.ttf`；它的 OS/2 字重是 `400`，但字面灰度明显高于此前使用的文楷 Screen Regular，更适合作为默认正文。

| 字体角色 | CSS 字重 | 用途 |
| --- | --- | --- |
| ZhenKai GB Regular（真实字形） | `400` | 正文、文件树和普通大纲项 |
| WebView 合成半粗体 | `600` | H1–H6、当前大纲项、H1/H2 大纲项和表头 |
| WebView 合成粗体 | `700` | Markdown 强调等明确粗体语义 |

真实的 `400` 按 Unicode 范围拆成 9 个 WOFF2 资源分片。这样可以按需加载；第三方若采用相同方法，也更容易满足轻语“单个 WOFF2 不超过 4 MiB、压缩包不超过 16 MiB、解压后总量不超过 32 MiB”的导入限制。9 个分片合计覆盖原始字体全部 23,878 个 Unicode 映射；原始 TTF、官方 v0.825 提交和 SHA-256 均记录在 `packages/app/src/themes/licenses/FONT-SOURCE.txt` 中。

格式转换和 Unicode 子集化在 OFL 1.1 下属于修改版本。为避免派生 Webfont 使用上游保留名称，字体 name table 中面向用户的 family、full、unique 和 PostScript 名称统一改为 `Light Whisper ZhenKai`；版权记录和上游归属仍保留。CSS 也只引用这个内部家族名：

```css
@font-face {
  font-family: "Light Whisper ZhenKai";
  font-style: normal;
  font-weight: 400;
  src: url("./fonts/zhenkai-gb-regular-subset-1.woff2") format("woff2");
  unicode-range: U+0000-U+33FF;
}
```

主题只开启字重合成：

```css
:root[data-theme="light"] {
  font-synthesis: weight;
}

.markdown-paper[data-editor-theme="light"] {
  --editor-heading-font-weight: 600;
}
```

`weight` 允许 WebView 在缺少真实粗体面时生成粗体，但不会同时合成斜体、小型大写或上下标字形。标题和侧边栏强调项请求 `600`，确保它们进入粗体匹配范围；Markdown 的 `strong` 仍可请求 `700`。合成是浏览器对单字重字体的视觉近似，`600` 和 `700` 不代表主题包中存在两套额外字形。

同一声明也直接写在可视化编辑器和源码编辑器表面上。这样即使用户分别选择应用外观主题与编辑器主题，臻楷编辑器也不会错误继承另一套应用主题的字体合成策略。

这项选择的边界很明确：

- 不把臻楷与文楷混入同一个 CSS 字体家族，避免字符集、度量和笔画灰度跳变。
- 不使用 `-webkit-text-stroke` 手工描边；粗体由标准字体匹配与 `font-synthesis` 控制。
- 臻楷 GB 或 Unicode 分片未覆盖的字符会进入主题声明的楷体和系统字体回退链。
- 用户在设置中明确选择其他编辑器字体后，用户字体通过行内样式覆盖主题字体，这是产品能力，不是主题失效。

字体文件、官方 OFL 1.1 和来源记录必须一起分发。当前 WOFF2 只做 Webfont 格式转换、Unicode 子集和字体名称合规调整，没有主动修改上游字形轮廓；派生字体继续完整遵守 OFL 1.1。

## 核心颜色角色

颜色首先按用途命名，再映射到具体界面。第三方主题应替换角色值，而不是寻找某个偶然出现的十六进制颜色并全局替换。

| 角色 | 轻语 · 纸白 | 轻语 · 夜读 | 用途 |
| --- | --- | --- | --- |
| 编辑器纸面 | `#ffffff` | `#23282d` | 正文编辑背景 |
| 次级纸面 | `#f7f7f7` | `#282e33` | 代码块、次级面板 |
| 正文 | `#262626` | `#e7e9ea` | 正文与主要界面文字 |
| 次要文字 | `#6b6b6b` | `#abb2bf` | Markdown 标记、说明文字 |
| 普通边框 | `#ededed` | `#33373c` | 编辑器弱分隔 |
| 强边框 | `#d1d1d1` | `#4b4f52` | 表格、强调分隔 |
| 动作强调 | `#1c5d33` | `#54c59f` | 选中指示、任务、类型色 |
| 标题强调 | `#7a3dad` | `#a178ff` | H1/H2 与源码标题 |
| 链接 | `#0c6ada` | `#1d9bf0` | 可点击文本 |

绿色只承担动作和状态语义，紫色只承担文章标题语义。把所有强调元素统一成一个品牌色会让链接、当前项、标题和危险操作失去区分。

深色主题没有简单反转浅色值。它降低纸面亮度、提高次要文字亮度，并重新选择边框和选中填充，保证相邻深色面板仍然可辨。

## 编辑器排版

### 正文节奏

正文采用：

```css
line-height: 1.74;
letter-spacing: 0.035em;
```

妙言的阅读排版使用 `1.74` 行高和更舒展的中文字符间距。轻语保留行高，将正文字距从参考值稍微收窄，以减轻英文、数字和路径文本被过度拉开的现象。

段落间距不由主题控制。用户的编辑器设置会以内联值写入 `--editor-paragraph-spacing`，优先级高于主题。第三方主题不应声明该变量，也不应使用 `!important` 绕过用户选择。

### H1–H6 层级

| 层级 | 颜色 | 字号 | 紧凑字号 | 字重 | 字距 | 行高 |
| --- | --- | --- | --- | --- | --- | --- |
| H1 | 标题紫 | `2rem` | `1.75rem` | 合成 SemiBold `600` | `0.05em` | `1.55` |
| H2 | 标题紫 | `1.5rem` | `1.375rem` | 合成 SemiBold `600` | `0.05em` | `1.6` |
| H3 | 正文标题色 | `1.25rem` | — | 合成 SemiBold `600` | `0.04em` | `1.65` |
| H4 | 正文标题色 | `1rem` | — | 合成 SemiBold `600` | `0.02em` | `1.74` |
| H5 | 正文标题色 | `1rem` | — | 合成 SemiBold `600` | `0.02em` | `1.74` |
| H6 | 正文标题色 | `1rem` | — | 合成 SemiBold `600` | `0.02em` | `1.74` |

这套比例来自妙言 `2em / 1.5em / 1.25em / 1em` 的中文标题骨架，但没有复制其 `1.8` 标题行高。轻语的标题存在于可编辑 CodeMirror 行中；过大的标题行框会放大光标移动和空行跳动，因此 H1–H3 使用逐级放松的行高，H4–H6 才回到正文节奏。

H1/H2 使用紫色是对妙言编辑态标题色的吸收；H3–H6 回归正文标题色，避免长文大纲呈现为连续的大面积彩色文本。

轻语为每一级标题提供相同的一组稳定令牌：

```css
--editor-h1-color
--editor-h1-font-size
--editor-h1-font-size-compact
--editor-h1-font-weight
--editor-h1-letter-spacing
--editor-h1-line-height
```

H2 同样支持紧凑字号，H3–H6 支持除紧凑字号外的其他字段。CodeMirror 实时标题和渲染后的 Markdown 标题共同消费这些令牌，因此第三方主题不需要分别覆盖两条编辑器路径。

`--editor-h1-letter-spacing` 至 `--editor-h6-letter-spacing` 都会回退到 `--editor-heading-letter-spacing`。只需要统一字距的主题可以只设置通用值；需要中文标题节奏的主题再逐级覆盖。

## 侧边栏和应用外壳

默认主题不把编辑器背景色直接套在所有应用区域上。每个区域都有独立语义：

| 区域/状态 | 轻语 · 纸白 | 轻语 · 夜读 |
| --- | --- | --- |
| 标题栏 | `#ececec` | `#323232` |
| 侧边栏、工具条、大纲、底栏 | `#ffffff` | `#23282d` |
| 外壳边框 | `#e6e6e6` | `#393e42` |
| 树节点悬停 | `rgba(38, 38, 38, 0.055)` | `rgba(255, 255, 255, 0.055)` |
| 当前文档 | `#eeeeee` | `#363d45` |
| 多选节点 | `#d9d9d9` | `#424a54` |
| 当前大纲 | `#eeeeee` | `#363d45` |

文件树采用 14px、Regular 400、20px 行高；当前文档主要依靠背景和指示线识别，不额外增加字重，避免密集文件列表出现大面积粗体。

大纲同样以 14px、ZhenKai Regular 400、20px 为基础，当前标题和 H1/H2 请求合成 SemiBold 600，并通过字号、颜色和上方留白建立层级。

主题必须通过以下稳定令牌设置这些状态：

```css
--bg-titlebar
--bg-sidebar
--bg-sidebar-header
--bg-toolbar
--bg-outline
--bg-sidebar-footer
--border-chrome

--bg-tree-hover
--bg-tree-current
--bg-tree-selected
--text-tree
--text-tree-hover
--text-tree-current
--text-tree-selected
--tree-current-indicator
--tree-selected-indicator

--bg-outline-hover
--bg-outline-current
--text-outline
--text-outline-hover
--text-outline-current
```

不要直接选择文件树或大纲的 React 内部类名。内部结构可能重构，而稳定语义令牌由应用组件负责消费。

## 代码、引用和其他 Markdown 元素

默认主题保留妙言配色中“正文安静、代码清晰”的关系：

- 行内代码使用轻微的绿色透明底，不把正文切割成大量不透明色块。
- 代码块使用次级纸面，活动代码行只提高一级亮度。
- 浅色语法色接近 GitHub Light 的可读分布；深色语法色使用紫、薄荷绿、暖黄和浅蓝区分角色。
- 引用块使用绿色左边框和低透明背景，而不是整块高饱和填充。
- H2 保留一条弱边框，帮助长文形成章节边界。
- 链接使用独立蓝色，不与绿色动作色和紫色标题混淆。

主题可以直接设置标准 Markdown 元素以及轻语公开允许的编辑器后代样式；但 `.cm-*`、`.markra-*` 和其他编辑器实现类不是长期稳定契约。优先使用 `--editor-*` 令牌，只有令牌无法表达的 Markdown 细节才使用内部选择器。

## 第三方主题如何从这个案例开始

不要复制整个默认臻楷主题再做全局颜色替换。建议按以下顺序制作：

1. 先确定 `background / panel / text / accent` 四个预览角色，并完成 `manifest.json`。
2. 定义正文纸面、次级纸面、正文、标题、次要文字、普通边框、强边框、动作强调和链接九个基础角色。
3. 分别映射标题栏、侧边栏头部、工具条、文件树、大纲和底栏，不假设它们必须同色。
4. 完成文件树的普通、悬停、当前、多选以及指示线状态。
5. 完成 H1–H6 的颜色、字号、字重、字距和行高，然后同时检查实时编辑标题与渲染标题。
6. 完成代码、表格、引用、任务、标记、链接、图片和 Mermaid。
7. 最后再添加字体和图片资产；字体包必须包含对应许可证。不要复制 `light`、`dark`、`classic-light` 或 `classic-dark` 作为第三方 ID，它们由应用保留。

如果设计不需要独立字体或图片，优先使用单文件 CSS。只有需要一起分发资产时才制作资源主题和 `.theme` 包。

## 验收清单

默认主题或第三方主题发布前，至少完成以下人工检查：

- 在浅色与深色窗口中分别打开包含 H1–H6、中英文、数字和长链接的文档。
- 将窗口缩窄，确认 H1/H2 使用紧凑字号且标题控制、光标和换行没有错位。
- 在文件树中检查普通、悬停、当前、多选、键盘焦点和拖拽状态。
- 在大纲中检查 H1–H6、长标题截断、当前标题和层级间距。
- 检查正文选择、光标、搜索命中、链接、行内代码、代码块、引用、表格、任务列表和 Mermaid。
- 明确选择系统字体，确认用户字体能够覆盖主题字体；切回“使用主题字体”后再次确认 ZhenKai Regular 的真实 400 与合成粗体生效。
- 调整段落间距、字体大小、行高和内容宽度，确认主题没有用 `!important` 覆盖用户设置。
- 对第三方资源主题，将资源目录打成 `.theme`，通过“设置 → 外观 → 导入主题”验证导入；对同 ID 包再次导入，验证替换确认和原子更新流程。前端内置主题不经过这条流程。

仓库中的静态契约检查可以运行：

```sh
pnpm --filter @markra/app exec vitest run src/styles.test.ts src/lib/settings/app-settings.test.ts
pnpm --filter @markra/desktop verify:theme-assets
pnpm --filter @markra/web verify:theme-assets
```

运行时样式发生变化后还应执行仓库构建，并在真实桌面 WebView 中检查明暗主题。静态测试只能证明令牌、作用域、资产和许可证约束，不能代替视觉检查。

## 已知边界

- 霞鹜臻楷当前发布只提供真实 `400`；主题允许 WebView 合成 `600/700`，合成结果会随平台字体栅格化实现略有差异。
- v0.800.2 之后上游只维护 GB 字形版；需要 GB 字汇之外字形时会进入主题的系统回退链。
- 主题按 Unicode 范围分片加载字体；字体未收录或分片未覆盖的极少见字符会使用系统回退字体。
- 主题不能改变窗口交通灯、系统原生菜单等操作系统拥有的界面。
- 用户字体、字体大小、行高偏好、内容宽度和段落间距具有更高优先级。
- 主题不能增加按钮、命令、编辑器插件或脚本行为。
- 主题不能依赖网络字体、远程图片、远程 CSS 或运行时下载。
- 单个主题只描述一种明暗外观；成对主题仍然是两个独立 ID 和两个独立包。

这些边界保证第三方主题可以充分改变视觉语言，同时不会接管轻语的文件、编辑、同步和应用控制行为。
