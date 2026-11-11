# 预览迁移 cmark-gfm：接入评估与可减少的差异

日期：2026-11-11。结论是可以接入，但本轮没有替换产品预览、修改依赖或进行手机构建/测试。所见即所得继续保留 Vditor。

## 建议

按当前 PC 与移动原生应用目标，建议下一步先验证 **Rust + 静态 cmark-gfm + Tauri command**，同一解析核心用于 macOS、Windows、Linux、Android 和 iOS，移动端不为这项业务另写 Java/JNI 或 Swift 桥。这里是基于 Tauri 现有架构的接入判断，不是五平台已经编译通过。

如果纯浏览器也必须是同语义产品入口，则 **同一 C 源码自编单线程 WASM、放进现有 Worker** 是更直接的候选：桌面、移动 WebView 和浏览器可使用同一资产。本轮不建议同时维护两套新后端。现成 Rust/WASM 包存在版本、选项和内存管理等问题，均不能无审计直接替换；详细平台来源及构建矩阵见 [平台调查](./cmark-platform-research.md)。

两条路线都可以把重活移开 UI 线程，但实际流畅度仍受 IPC/Worker 拷贝、HTML patch/layout、代码高亮和图表绘制影响，不能仅凭“原生 C”宣称端到端必然更快。

## 必须匹配的妙言配置

本地妙言锁定 `swift-cmark-gfm 1.0.2`，revision `7ec44e7384793e507cf046336aef68586719a24e`，其 C 版本头报告 `0.29.0.gfm.3`。应锁定实际源码 revision 建立基准，不能把包名或版本字符串相同当成代码相同。[固定包清单](https://github.com/stackotter/swift-cmark-gfm/blob/7ec44e7384793e507cf046336aef68586719a24e/Package.swift)、[版本头](https://github.com/stackotter/swift-cmark-gfm/blob/7ec44e7384793e507cf046336aef68586719a24e/Sources/CMarkGFM/cmark-gfm_version.h)

[Markdown.swift](../../MiaoYan/Business/Markdown.swift:365) 实际链路包含：

1. 数学公式保护，结合 cmark AST 识别代码范围，保护 LaTeX 的转义、强调符号和换行。
2. parser 开启 `CMARK_OPT_FOOTNOTES`，按原版尝试注册 `table/footnotes/strikethrough/tasklist` 扩展。
3. HTML render 开启 `CMARK_OPT_SOURCEPOS | CMARK_OPT_UNSAFE`，按妙言换行设置选择 `HARDBREAKS` 或 `NOBREAKS`。
4. 数学换行清理、Callout 转换、空任务项转换、中文强调标注。
5. 原版应用另通过 `HtmlManager.processImages` 处理本地资源路径；我们应接到自己的资源解析流程，不照搬其 macOS 文件 URL。

`UNSAFE` 是让引擎输出 raw HTML 的参数，不是允许我们取消安全过滤。产品仍保留当前 DOMPurify、URI 处理和 CSP；HTML 清洗后再发送给预览 iframe。C 库 BSD-2-Clause 与妙言 MIT 代码的许可证/版权说明也要随接入保留。

## 本轮隔离 Host 实验

精确 C 源码已复制到 `artifacts/cmark-preview-research/swift-cmark-gfm`，独立编译了 34 个 C 文件及小型 C 命令行探针；另外编译实际的妙言 `renderMarkdownHTML` 和 `Emphasis` Swift 代码作为对照。Swift 仅用于 macOS Host 复现基准，不是 Android/Windows 接入方案。

原版 Swift 副本只去掉了 `import MiaoYanCore`，改为同次编译原样的 `Emphasis.swift`；输入是 stdin 文本，输出为 HTML，没有运行整个妙言应用。对照端通过 esbuild 单独打包当前 `src/markdown.ts`。编译命令、原始文件 SHA256 见 [source-manifest.json](../artifacts/cmark-preview-research/source-manifest.json)。

运行 `node artifacts/cmark-preview-research/run.mjs`，9 个针对性样例及同一份 37 组全格式文档全部完成 HTML 转换和断言。结果见 [result.json](../artifacts/cmark-preview-research/result.json)、[日志](../artifacts/cmark-preview-research/probe.log)。全部使用既有 DOMPurify 规则检查；这不是 WebView 视觉、应用 IPC、性能或移动平台验收。

| 样例 | 当前 markdown-it | 仅相同 cmark 核心 | 妙言完整转换函数 |
| --- | --- | --- | --- |
| 普通软换行 | 无 `br` | HARDBREAKS 下有 `br` | 与核心相同；Github/NOBREAKS 模式无 `br` |
| SVG data URI | 没有图片节点 | 输出图片 | 输出图片，经过当前 DOMPurify 后仍保留 SVG data URL |
| 重复脚注 | 已手工统一编号，但用插件自己的 ID | 原版 label 型 ID 与重复来源 ID | 如 `fnref-a`、`fnref-a-2` |
| Callout | 现有自定义转换正常 | 没有提示块结构 | 产生原版 `.markdown-alert` |
| 空任务项 `[ ]` / `[x]` | 保留为文字 | 仍为文字 | 产生两个 checkbox |
| 多行公式 | 当前自定义保护 | 会插入多余换行并受 Markdown 解析影响 | 通过原版保护与清理保持公式输入 |
| 五种行内 HTML | 已正常输出 | 正常输出 | 正常输出，不是本轮迁移的新增收益 |
| 中文强调 | 已有语言标注 | 没有对应标注 | 恢复 `lang="zh"` 等标注 |
| 非段首尖括号邮箱 | 预览本来正确 | 正确 | 正确，不会修复 Vditor 的独立问题 |

37 组文档 SHA256 仍为 `d541026362ef0c42d6099740312ed736bc76357fe2ee53106a9db21bbb0c66f6`。三种转换均保留了 37 个测试区段；当前转换输出 158 个 sourcepos 节点，cmark/妙言函数输出 172 个，说明源码位置范围和节点形态也有区别，不能只替换 HTML 文本而忽略滚动及交互定位。

## 对现有差异的影响

| 现有差异 | 能否通过迁移减少 | 仍需做什么 |
| --- | --- | --- |
| C04 软换行 | **可减少**。采用与妙言当前设置相同的换行 flags；默认 HARDBREAKS 能使预览与妙言及现有编辑区更接近。 | 不混淆“Markdown 换行语义”和设置里的视觉自动折行。原版转义方括号被公式识别的行为也要单独决定兼容范围。 |
| C31 定义列表样例的普通换行 | **可减少显示差异**，原因也是换行参数。 | cmark 并不会因此支持定义列表扩展，仍是普通文本。 |
| C28 SVG data 图片 | **预览解析可以修正**，Host 已验证输出和既有清洗后的 URL。 | WebView 显示、安全边界和大图片要继续验收；Vditor/Lute 仍会过滤其 src，两个模式之间的该差异依旧存在。 |
| C20 块公式外层结构/约 2.39px 留白 | **有望减少**，前提是一起移植妙言的数学保护与清理，使用相同 HTML 形态。 | KaTeX、容器 CSS 和最终尺寸仍需对照，不能说换核心已自动修好。 |
| 脚注、表格、任务项等节点结构 | **更容易与妙言同源**，可减少手工 renderer 补丁；空任务项需要原版后处理。 | 当前脚注显示已对齐，收益主要是结构与维护；验证 IDs、重复返回、checkbox 原文行定位和 sourcepos。 |
| C05 所见即所得邮箱解析 | **不会修复**。当前预览已正确，问题在 Lute。 | Vditor 另行适配或等待引擎修复。 |
| C36 PlantUML loading 文案/居中 | **不能作为解析器迁移的自动收益**。 | 图像生成在 JS 后处理；仍需单独检查容器样式和 loading 清理，即使代码块 HTML 更接近原版。 |
| 所见即所得 HTML 属性、复杂 HTML、Markdown 规范化 | **不会修复**。 | Vditor 编辑模型和序列化继续保持现状。 |
| 字体、颜色、图标及高亮/Mermaid/Markmap | **不会由 cmark 本身改变**。 | 继续使用现有样式和 JS 渲染链，按截图、尺寸与交互验收。 |

目标是预览更接近妙言，不是让 Vditor 和 cmark 成为同一个编辑模型。部分原来两边都失败的内容可能变成“预览正确、所见即所得仍有限制”，需要明确承认。

## 本项目接入边界

当前链路是 [render.worker.ts](../src/render.worker.ts) → `{id, html, offset}` → [main.ts](../src/main.ts) 版本过滤和 DOMPurify → 预览 iframe 的 CSS/JS 渲染。

- Native 方案：在主前端异步 invoke，Rust blocking 任务中运行 C 和兼容处理，返回同一结构；不假定 Tauri invoke 可以直接在现有 Worker 中运行。WASM 方案可保留现有 Worker 的消息接口。
- 保留 `{id}` 过期结果过滤、150ms 防抖、IME 期间暂停和失败时保留原文。增加至多一个执行中任务和一个最新待处理请求，避免长文档连续排队；忽略旧结果并不等于取消 C 解析。
- 保留 YAML 隐藏及 `offset`，核对 UTF-8 列位置与 JS UTF-16 的边界；当前大量操作按行定位，但不要把 cmark sourcepos 当成未经过数学保护变换的绝对字符偏移。
- 保留当前 sanitizer 和图表链路，先改解析后端，避免同时升级 KaTeX/Mermaid 等扩大对照范围。
- **不能直接卸载 markdown-it**：当前 [rich-references.ts](../src/rich-references.ts) 仍用它的 reference rule 保留 Vditor 引用定义原文。预览替换与该依赖清理是两件事，本轮不改它。
- Native 的纯浏览器预览没有 Rust IPC；若继续临时使用旧后端，只能视为开发辅助，不能拿它证明新产品解析器的视觉验收。浏览器也是产品目标时，更倾向统一 WASM。

下一步先选一条路线做桌面端最小原型，再以 37 组文档、空任务项、多行公式、长文档、恶意 HTML/URI、滚动同步和任务回写建立对照。性能要分开记录冷启动、热解析 P50/P95、拷贝和完整渲染时间。Android/iOS 的编译与真机验收留待手机相关工作恢复后执行；当前没有进行这些测试。
