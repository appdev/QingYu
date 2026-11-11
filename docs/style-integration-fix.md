# CSS 与图表集成修复验收

日期：2026-11-11。范围：修复我们自己的显示样式、测量环境和图表资源接入；保留 SDK 编辑模型。已完成同文档复测、macOS 构建及原生运行。手机测试继续暂停。

## 本轮修改

| 修改 | 归属与结果 |
| --- | --- |
| Mermaid 测量与间距 | 临时标签 margin 重置只作用于本应用生成的测量容器；图块间距改为原版的 8px。7 个合法图的 viewBox 在两种主题下均与原版相同，错误图继续回退源码。 |
| 长代码行 | 只读显示层使用 `pre-wrap`、原版换行规则，保留高亮与可编辑源码。 |
| 公式、图片、HTML 显示层 | 对齐连续块公式间距、普通图片的 8px 圆角/顶部间距；去除 HTML 实体多余代码底色、details 显示副本的代码背景与 padding，不改变 HTML 编辑节点。 |
| Markmap | 复用已有原版 d3、Markmap 与主题资源。临时 HTML 测量容器采用实际显示字体，节点路径与颜色对齐。标签先安全过滤，再交给绘图库；保留独立源码副本。 |
| Markmap 资源加载 | 新增共用的本地 loader 配置，修复原版脚本在应用根路径下请求 `js/d3.min.js` 等不存在路径的问题。加载次序包含原版后台帧回退脚本。 |
| PlantUML | 按用户明确选择接入原版在线服务，图表源码发送到 `https://www.plantuml.com/plantuml/svg/`。使用图片显示，失败回退源码；没有启用 object，也没有放宽 CSP。预览和所见即所得均支持。 |

主要文件：[rich-code.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-code.ts)、[rich-editor.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-editor.ts)、[rich-editor.css](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-editor.css)、[markmap-config.js](/Users/evan/Downloads/public/MiaoYanEditor/public/markmap-config.js)、[preview.html](/Users/evan/Downloads/public/MiaoYanEditor/public/preview.html)、[preview.js](/Users/evan/Downloads/public/MiaoYanEditor/public/preview.js)。原版 vendor CSS/JS、普通预览的字体/配色规范及 CSP 配置未修改。

工作区已有的笔记库、侧栏与正文标题 helper 改动已保留。本轮没有重做这些功能，也没有替换真实应用草稿。YAML 在所见即所得中仍是可编辑块，按范围保留。

## 同一文档复测

使用原有 [全格式对照.md](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/全格式对照.md)，未修改内容。SHA-256：`d541026362ef0c42d6099740312ed736bc76357fe2ee53106a9db21bbb0c66f6`。

参照仍为原版妙言 4.8.0 实际生成的 HTML 与原版资源。开发版先复测，冻结后构建，再用打包产物复测；两种主题的正文视口均为 1120×900，侧栏全部收起。

- 37 组格式、浅色/深色，共 148 张截图：[最新双栏对照页](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/release-css-integration/gallery.html)。
- 12 组本轮相关用例通过局部区段高度断言，差值小于 0.1px：分隔线、长代码、块公式、各类 Mermaid、details、普通 PNG、Markmap、PlantUML。
- 7 个合法 Mermaid 的 viewBox 全部一致；1 个故意写错的图继续回退源码。
- Markmap 节点连线路径及颜色与原版一致；普通语言代码高亮 token 文本、类别、颜色保持一致。
- 引用链接、表格 HTML、脚注、行内 HTML 四组 SDK 编辑 DOM 与修复前逐字相同，确认没有绕过本轮范围。
- 没有编辑的模式切换后，Markdown 与输入逐字相同。浏览器页面错误为 0，原先图表资源的 404 已消除。

数据：[release-verification.json](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/style-integration-fix/release-verification.json)、[完整比较结果](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/release-css-integration/comparison.json)。普通 PNG 的相对文件路径仅在提供同字节测试资源的回放环境验收，不代表原生附件访问功能已完成。

## 构建和运行

- 单元测试：5 个测试文件、32 项通过；新增覆盖 Markmap 标签过滤、原文保留、PlantUML 图片及网络失败回退。
- 类型检查与 `git diff --check` 通过。
- 现有代码/Mermaid 的点击编辑、撤销/重做、主题切换、预览回写通过；新增 Markmap/PlantUML 的点击编辑、失焦重绘、主题切换、源码回写通过，未生成 object。
- 当前没有 IDE `build_project` 工具，使用 `npx tauri build --bundles app`，macOS release 构建成功，产物约 12.01 MiB。构建只有依赖包体积提示，没有编译错误。
- [冻结输入清单](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/style-integration-fix/frozen-tree.json)：158 个源文件/配置输入，标识 `7c4f8dbfd869063e1d4169d4967179bc1cb53ac1a5ed4aa20bcad66ae7837384`。构建后检查内容未漂移。
- 已更新并启动 [MiaoYan Editor.app](</Users/evan/Downloads/public/MiaoYanEditor/artifacts/MiaoYan Editor.app>)，签名校验通过；真实草稿的“书生1 / 痴情专一2”和 587 字符状态保留。
- 隔离的 Comparison Desktop 实例载入同一格式文档，原生窗口已确认 Mermaid、Markmap 和在线 PlantUML 正常显示。[原生图表截图](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/style-integration-fix/native-diagrams-wide.webp)、[Mermaid 截图](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/style-integration-fix/native-mermaid.webp)。没有把回放数据当成手机或其他平台验证。

## 保留的边界

HTML 行内标签及表格 `<br>` 的可编辑代码表示、脚注/引用定义的位置、提示块 emoji、YAML 编辑块、内部锚点规则、SVG data URI 图片等 SDK 模型或解析差异本轮不处理。没有宣称整篇文档逐像素一致。深色彩色节点的黑字及边框可读性修正继续保留。

PlantUML 依赖网络服务，失败时显示源码。没有做 Android、iOS、Windows 或 Linux 验收；手机测试仍暂停。没有新增附件管理或同步功能。

本轮验证方法纠正：备份测试文件改用 `.bak` 后缀，避免被 Vitest 扫描；WebKit 的 foreignObject 外框宽度可能为 0，而其中的 HTML 标签实际可见，因此交互验证改为检查子标签；颜色比较先归一化 HEX/RGB。侧栏对比脚本按真实布局循环收起两栏并断言内容宽度，避免拿不同正文宽度比较。Markmap 的“已能绘图”与“节点测量已对齐”分开验收，防止再次用较窄结论概括全格式一致。
