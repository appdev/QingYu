# 表格、Callout、YAML 与锚点适配验收

日期：2026-11-11。保留 Vditor 4.0.1。只处理本应用的四项集成，不替换 SDK、不修改 vendor、不扩展 HTML/脚注/引用定义的编辑模型；手机测试继续暂停。

## 修改

| 项目 | 实现和边界 |
| --- | --- |
| 表格 `<br>` | 在 Lute 的 `Md2VditorDOM`、`HTML2VditorDOM`、`SpinVditorDOM` 转换输出后，仅把表格单元格内表示裸 `<br>` 的 HTML-inline 节点提升为原生换行。包含属性的标签、普通正文 HTML、代码和 HTML 块不处理。选择窄范围转换适配，是为了保留其他 HTML 的 SDK 默认 renderer，避免覆盖通用 `renderInlineHTML` 后自行复制默认渲染逻辑。 |
| Callout | 使用已有 `alert-icons.json` 的五种 SVG 路径，通过 CSS mask 显示。标题和标记仍保留在 SDK DOM 中，装饰不写入 Markdown。深色图标采用原版实际的正文浅色，标题仍按提示类型着色。 |
| YAML | 复用 `markdownBody()`；原始元数据片段保存在当前文档适配状态中，Vditor 只编辑正文，回写时拼回。没有新增元数据界面，仍可在源码模式修改。文件末尾分隔符后原来无换行时，首次添加正文只补必要的分隔换行。未关闭的 front matter 不剥离。 |
| 锚点 | 预览和所见即所得共用 `heading-links.js`，支持原版标题文字、URL 编码、常用小写/短横线别名与重复标题。通过公开 `link.click` 接入编辑器，预览在本地拦截匹配的内部链接。不改 SDK 自用标题 ID，也不跳出窗口。不是完整 GitHub slug 规范实现。 |

源码：[rich-markdown.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-markdown.ts)、[rich-editor.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-editor.ts)、[rich-editor.css](/Users/evan/Downloads/public/MiaoYanEditor/src/rich-editor.css)、[heading-links.js](/Users/evan/Downloads/public/MiaoYanEditor/public/heading-links.js)、[preview.js](/Users/evan/Downloads/public/MiaoYanEditor/public/preview.js)、[preview.html](/Users/evan/Downloads/public/MiaoYanEditor/public/preview.html)、[types.d.ts](/Users/evan/Downloads/public/MiaoYanEditor/src/types.d.ts)。

## 验证

- 4 个编辑器相关测试文件、26 项通过；新增真实 Lute 的表格导入/规范化/HTML 粘贴/导出、其他 HTML 不变、Callout 回写不变，以及共享锚点测试。见 [unit.log](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/markdown-compat/unit.log)。
- WebKit 真实页面通过表格输入、撤销/重做、YAML 原片段保留、源码模式修改元数据后再次编辑、浅深色图标、两种模式的中文/编码/重复标题跳转、无额外窗口和无编辑模式切换原文一致。见 [interaction-result.json](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/markdown-compat/interaction-result.json)。
- 独立组件页面补测仅有元数据的文档、CRLF、注释/引号、`...` 结束符、无结尾换行、未关闭元数据以及换笔记后的片段隔离。见 [yaml-result.json](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/markdown-compat/yaml-result.json)。应用的 CodeMirror 原本会统一换行；此组件测试的 CRLF 原片段保留，不代表整个应用新增了文件字节保真能力。
- 同一份全格式文档，浅深两种主题，37 组格式、148 张截图。打包产物复测通过；Callout 和含 `<br>` 表格局部区段高度均与原版一致，14 组相关区段误差小于 0.1px，7 个 Mermaid 的 viewBox 及 Markmap 路径/配色、代码高亮维持上一轮结果。见 [最新对照页](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/format-comparison/release-markdown-compat/gallery.html)、[断言结果](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/markdown-compat/comparison-verification.json)。
- 引用链接、脚注、行内 HTML 三组的 SDK 编辑 DOM 与上轮验收逐字相同；没有暗中更改未授权的模型。
- 本轮范围类型检查及整工程 `tsc --noEmit` 通过。同步模块中间出现的错误由另一个会话修复，本轮未修改这些文件；本轮路径检查不替代对同步功能的验收。

## 构建及原生窗口

当前没有 IDE `build_project` 接口，使用项目原生命令 `npx tauri build --bundles app`，macOS release 构建成功，产物约 16.31 MiB，见 [build.log](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/markdown-compat/build.log)。构建输入 240 项，冻结标识 `64b46665b5efeee853b4a4584174ffa6450dc94851495ed11b0a06c5388123c2`，构建后校验未漂移。

已更新并启动 [MiaoYan Editor.app](</Users/evan/Downloads/public/MiaoYanEditor/artifacts/MiaoYan Editor.app>)，签名校验通过，真实草稿的“书生1 / 痴情专一2”和 587 字符状态保留。隔离 Comparison Desktop 实例使用原有全格式文档，YAML 已隐藏，状态仍为 4,854 字符，原生窗口能显示 SVG mask 提示图标，见 [原生窗口截图](/Users/evan/Downloads/public/MiaoYanEditor/artifacts/markdown-compat/native-formats.webp)。没有将测试文档覆盖真实草稿。

工作区同时包含另一个会话的同步功能，已按用户要求保留。打包包含当时工作区内容，但本报告不构成同步功能验收。旧应用包已保存在 `artifacts/markdown-compat/*-before.app.saved`，没有改动笔记存储。

## 剩余边界

完整行内 HTML、脚注/引用定义的可控直接编辑、邮箱特定上下文解析、SVG data URI 及未启用的扩展语法保持原状。Vditor 在实际编辑后仍会格式化 Markdown，例如 `<br>` 可序列化为 `<br />`，不承诺编辑后的源文逐字相同。未验证 Android/iOS、Windows/Linux 或原生附件访问；没有恢复手机测试。

验证方法记录：回放服务器需要映射新增的 `heading-links.js`；首次打包回放缺少这条映射出现 404，补齐测试服务器映射后通过，没有因此修改产品或重复构建。原生截图定位时采用实际滚动，不能把选中文本等同于视口已滚动到该文本。
