# 所见即所得与原版妙言逐格式对比

**以下为修复前基线。** 2026-11-11 的 CSS/集成修复及最新 148 张对照图见 [修复验收](/Users/evan/Downloads/public/MiaoYanEditor/docs/style-integration-fix.md)。SDK 编辑模型相关差异继续保留。

日期：2026-11-10。结论：常规 Markdown 排版和普通代码高亮已接近原版；扩展格式尚未完全对齐。Mermaid 的一部分尺寸差异、长代码行、HTML、脚注和其他图表仍需处理。本次只增加测试文档、对比工具和报告，没有修改产品代码，也没有重新构建。

补充调查：[剩余差异的归属确认](/Users/evan/Downloads/public/MiaoYanEditor/docs/rendering-attribution.md) 已通过纯 Lute 与单变量 CSS 实验区分 CSS、编辑模型、配置和资源接入问题；不能将所有未对齐项解释成 SDK 不支持。

## 文档、环境与证据

- 共用文档：[全格式对照.md](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/全格式对照.md)，37 组用例，另含 YAML front matter。
- [逐格式双栏截图页](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/gallery.html)：可切换 37 组格式、浅色/深色；共 148 张截图。
- [完整测量数据](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/comparison.json)、[验证摘要](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/verification.json)。
- 参照：原版妙言 **4.8.0**，通过原生应用打开同一文件，重新切换预览生成 HTML；使用其安装包内的 CSS/JS。没有用我们的 Markdown 解析器冒充原版输出。
- 待测：当前已打包的编辑器前端，Vditor 4.0.1 所见即所得；同一份文件也已载入隔离的 Comparison Desktop 原生实例。真实产品草稿未替换。
- 定量对比：同一个 WebKit 27.2 环境，内容视口均为 **1120×900**，正文有效内容宽度均为 **944px**。外层根元素宽度不同，但左右正文坐标一致；不是把整窗不同尺寸直接当成排版差异。
- 原版缓存 HTML：`/Users/evan/Library/Caches/com.tw93.miaoyan/wkPreview/index-0157377C-0E03-4D7E-B32D-214D4F1E318B.html`，原始副本保存于 `artifacts/format-comparison/official/index.html`。源文档与 HTML 的 SHA-256 已写入测量数据。
- 浏览器回放仅将原版的本地 `asset.png` 文件 URL 映射到同字节的本机 HTTP 图片，解决浏览器不能跨协议读取 file URL 的限制；未改原版样式或生成内容。因此普通图片结果只验收回放环境，不代表原生附件路径处理已完成。

本次 18 组用例通过局部块几何断言：块数量、相对该组标题的位置、宽、高的误差均小于 0.1px。该断言不代表逐字像素相等，也不覆盖隐藏的编辑结构。Swift、Kotlin、Java、JavaScript、JSON、自动识别与未知语言回退样本的高亮 token 文本、类别、颜色一致。两种主题下，两边都成功渲染 **7 个合法 Mermaid**，并将 **1 个故意写错的 Mermaid** 回退为源码；成功渲染不代表尺寸已经相同。

## 逐项结果

下表同时适用于浅色、深色，主题特有的差异单独说明。每一行都能在截图页选择对应编号。

| 编号 | 格式 | 结果与差异 |
| --- | --- | --- |
| C01 | ATX 一至六级标题 | 局部几何及基础字体样式一致。 |
| C02 | 正文、混排、自动换行 | 字体、段落高度、自动换行的块几何一致。 |
| C03 | 加粗、强调、删除线、行内代码、嵌套样式 | 接近一致；中文强调与英文斜体均按现有规则显示。 |
| C04 | 软/硬换行、转义、实体 | 换行接近。原版误把转义方括号识别为块公式；我们保留字面方括号。我们还把 HTML 实体套上了行内代码底色。 |
| C05 | 普通/引用/自动链接、邮箱 | 普通链接正常。我们显示引用定义，引用链接是 SDK 蓝色标记；邮箱仍显示尖括号文本，原版为 mailto 链接。 |
| C06 | 两种分隔线 | 1px、颜色及上下留白一致。 |
| C07 | 三层无序列表 | 局部几何、缩进、标记接近一致。 |
| C08 | 有序列表与非 1 起始序号 | 从 3 开始，嵌套编号及块几何一致。 |
| C09 | 宽松列表、多段、列表内引用/代码 | 整体局部块几何一致。 |
| C10 | 完成、未完成、嵌套任务 | 局部几何一致；编辑交互与只读预览不是同一测试目标。 |
| C11 | 多段/嵌套引用、引用内列表 | 局部块几何一致。 |
| C12 | NOTE/TIP/IMPORTANT/WARNING/CAUTION | 颜色与块高度接近；原版线形 SVG 图标与我们的 emoji 不同。 |
| C13 | 表格、三列对齐、转义竖线 | 12 个单元格尺寸及采集样式一致，转义竖线正常。 |
| C14 | 长单元格、HTML 换行 | 长中文换行正常；我们的 `<br>` 被显示成标签，原版换行。本组高度相差约 24.14px。 |
| C15 | Swift 高亮 | token 文本、类别、颜色及代码块高度一致，代码字号 13.76px。 |
| C16 | Kotlin/Java/JavaScript/JSON | 四个样本的 token 文本、类别、颜色及块高度一致。 |
| C17 | 无语言/未知语言 | 自动识别和回退表现一致。 |
| C18 | 缩进代码、长代码行 | 缩进代码可显示。原版长行换行，我们保持 `white-space: pre`；本例代码高度 44px 对 22px。 |
| C19 | 行内公式 | 三处均渲染，KaTeX 字号和公式外框高度一致。 |
| C20 | aligned、矩阵 | 公式正确且本体高度一致；公式区段整体留白差约 2.39px。 |
| C21 | 流程图、判断、回路 | 两边成功；原版 SVG 约 682.59×171.41，我们约 714.59×203.41。 |
| C22 | 分组与自定义节点配色 | 浅色尺寸相同；深色高度 200 对 232。我们的深色彩色节点使用黑字，是已做的可读性修正，原版仍用浅色字。 |
| C23 | Mermaid 时序图 | viewBox、标签、局部区段高度一致。 |
| C24 | Mermaid 类图、状态图 | 均成功。类图本体相同；深色状态图高度 294 对 390。连续图块留白也有差异。 |
| C25 | Mermaid 甘特图、饼图 | 本体 viewBox 与实际尺寸相同，连续图块使区段多约 10.39px。日期拥挤、饼图低亮度配色在两边均存在。 |
| C26 | Mermaid 语法错误 | 均回退源码，没有注入冗长错误页面；块高度一致。 |
| C27 | 多段脚注、重复引用 | 编号正常。原版定义集中在文末并有返回箭头；我们在定义所在位置显示可编辑脚注，缺少同样的返回箭头。定义区高度 89.25 对 122.25px。 |
| C28 | SVG data URI 图片 | 两边均不能按预期显示：原版 Image Not Found，我们显示 alt、src 为空。普通图片另见 C34。 |
| C29 | mark/sub/sup/u/kbd | 原版渲染 HTML 语义外观，我们显示行内标签代码，明显不同。 |
| C30 | HTML details 折叠块 | 两边均能呈现；我们多了代码块背景、padding、圆角，区段高约多 53.38px。 |
| C31 | 非标准高亮/上下标/定义列表/emoji | emoji 正常；多种扩展标记没有转成相应扩展结构。两边边界相近，不能把字面显示计为扩展支持。 |
| C32 | `[TOC]`、内部锚点 | `[TOC]` 两边均为文字。已核对：样本的 GitHub 风格锚点在两边均无匹配目标；原版使用标题原文 ID，SDK 使用带前缀和编号的 ID。显示为链接不代表能定位。 |
| C33 | 普通结束段 | 段落本体及相对标题的几何一致；脚注位置差异另见 C27。 |
| C34 | 相对路径/HTTP PNG | 回放环境中两张均为 160×80。原版有圆角、段落留白更大；我们直角，区段少 16px。原生本地附件路径未验收。 |
| C35 | Markmap | 原版绘图；我们的所见即所得显示 Markdown 源码。 |
| C36 | PlantUML | 原版通过 plantuml.com 返回图像并显示；我们的所见即所得显示源码。本次图片已在回放环境成功加载。 |
| C37 | Setext 两级标题 | 标题及末尾正文的局部几何一致。 |
| 额外 | YAML front matter | 原版预览隐藏；我们的所见即所得显示 YAML，窗口标题还取到了 `---`。 |

## 已定位的原因与处理顺序

1. **Mermaid 临时测量样式**：我们的正文内已有 `.language-mermaid p { margin: 0; padding: 0 }`，但原库先在 body 下的临时 `drich-mermaid-*` 容器中测量标签，这个阶段不在正文作用域里。测试页面仅对临时 `.nodeLabel p/.edgeLabel p` 重置 margin/padding 后，流程图、自定义配色图、状态图的 viewBox 全部恢复到原版值。证据：[layout-probe.json](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/layout-probe.json)。这是诊断注入，没有写入生产样式。修复边界应在 [rich-code.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-code.ts:19) 的渲染测量环境，避免改原版预览基准。
2. **代码长行**：当前 SDK 样式结果为 `pre`，原版为 `pre-wrap`。字体、高亮色已经相同，但换行规则不同，短样本不能揭示这个问题。修复归属 [rich-editor.css](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-editor.css:57) 的只读代码显示层，需保留源码编辑区行为。
3. **HTML 与引用链接**：SDK 使用专门的可编辑节点表示行内 HTML、HTML 实体、引用链接和引用定义。目前部分节点没有按原版只读外观呈现；例如表格 `<br>` 直接成为代码样式文本。此类问题不能靠统一字号解决，也不应直接替换可编辑 DOM 后丢失 Markdown。
4. **脚注与 front matter**：属于 SDK 的编辑文档结构与原版预览文档结构差异。需定义可编辑状态、非活动显示状态和序列化规则；本次未验证编辑后往返无损。
5. **Markmap/PlantUML**：当前 [rich-code.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-code.ts:49) 对其他图表显式保留代码回退，尚未接入原版两种图表绘制。SDK 还会尝试加载未打包的 `vditor/dist/js/markmap/markmap.min.js`、`vditor/dist/js/plantuml/plantuml-encoder.min.js`，本次各产生一个 404 和页面资源错误；与 `public/vditor/NOTICE.md` 中仅打包当前使用资源的说明一致。后续应一并处理绘图依赖和 SDK 自动加载关系。属于支持缺口，不是渲染失败后应隐藏的文本。PlantUML 原版还依赖外部服务。
6. **细节样式**：多图块间距、块公式留白、图片圆角/留白、提示块图标，后续可按本测试文档收敛。深色彩色节点黑字和边框修正应保留，不应为了模仿原版重新降低可读性。

## 验证范围与复用方式

执行了 `node artifacts/format-comparison/compare.mjs`、`node artifacts/format-comparison/gallery.mjs`；后者断言截图数量/尺寸、18 组局部块几何及普通代码 token 一致。截图页的主题/格式切换、上一项/下一项和图片加载也通过浏览器检查。HTML details 的折叠点击在两边均成功，内部锚点目标检查记录于 `artifacts/format-comparison/link-details-check.json`。两种主题下无编辑切换回源码后，Markdown 与输入逐字相同。已有生产源文件校验清单 `artifacts/rich-code/verified-tree.sha256` 全部通过，确认本次未改此前已构建的代码。

截图页可以直接从本地打开。重跑对比需先在 `artifacts/format-comparison` 启动 `python3 -m http.server 1424 --bind 127.0.0.1`；脚本使用已有的 WebKit 安装路径。重新修改测试文档时，必须让原版应用重新生成缓存 HTML，再更新 `official/index.html`，不能拿旧原版输出与新文档比较。

没有恢复手机测试；没有测试 Windows/Linux，没有穷举每一种第三方 Markdown 扩展，没有验收原生附件访问或编辑后 Markdown 往返。本次普通格式比较不等于整篇文档已逐像素一致。

验证方法修正：此前短样本和单次最终样式读数，未覆盖长代码行、混合扩展格式和 Mermaid 临时测量阶段。本次加入了这些用例，并从可见文本中排除隐藏源码/公式辅助节点；逐格式截图固定为同尺寸视口，避免整页截图缩放掩盖差异。后续修复应继续复用这套文档和截图对照，不新增全局工作流规则。
