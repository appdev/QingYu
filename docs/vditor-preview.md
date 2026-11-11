# Vditor 统一预览与编辑视图

2026-11-11。本轮目标是应用内部的显示一致性，保留 Vditor，不改动 SDK 解析/编辑模型。手机构建、安装及测试继续暂停。

## 改动

- [preview.ts](../src/preview.ts) 替换原 markdown-it Worker → 妙言 HTML 模板的运行链路。预览复用 [createRichEditor](../src/rich-editor.ts)，使用同一份 Lute 配置、HTML/引用定义适配、代码高亮及图表渲染器，只将视图设为只读。未使用另一套 `Vditor.preview()` HTML。
- 字体、标题/段落/列表/表格/分隔线间距和图表容器共用 [rich-editor.css](../src/rich-editor.css)。主题变量抽到 [theme.css](../src/theme.css)。修正代码字体设置：行内代码跟随正文，代码块使用代码字体，引用中的代码仍跟随引用正文。
- SDK `disabled()` 不会阻止所有点击/快捷键操作，增加只读事件隔离，阻止展开代码、表格工具栏和文字修改；保留选择/复制、链接、脚注跳转和任务勾选。拦截 WebKit 在非编辑区 Backspace 导致的页面返回。Markdown 在进入只读 DOM 前仍经 DOMPurify，不放宽原 HTML 安全边界。
- [preview-source.ts](../src/preview-source.ts) 复用已安装的 Lezer 生成源码范围，再与 Lute 实际渲染结果匹配，用于滚动同步和精确任务回写；它不参与显示 HTML 的生成。无法匹配的任务禁用回写，避免误改其他行。源码中的伪造位置属性会清除。
- 缓存未改变区块的位置匹配结果，保留未改变的代码/图表显示面板，避免普通文字修改触发整篇图表重绘。仅保留上一份文档的匹配缓存。
- 原 `src/render.worker.ts`、`public/preview.html/js` 和初始化脚本移除，预览改为 Vite 的独立入口 [preview.html](../preview.html)。`src/markdown.ts` 标注为历史比较基线，不再接入运行时预览；markdown-it 依赖仍用于现有引用定义保真适配。

## 发布产物验证

同一份 [全格式对照.md](../artifacts/format-comparison/全格式对照.md)，37 类格式及 YAML，SHA256 `d541026362ef0c42d6099740312ed736bc76357fe2ee53106a9db21bbb0c66f6`。以妙言 4.8.0 原生生成的 HTML、原样资源回放作为第三方基准；本轮未重新操作原版妙言原生窗口。

验证使用独立复制的本轮 `dist`，不是共享开发服务。WebKit，内容视口 1120×900，浅色和深色各 37 组，三方共 222 张截图。

- [三方截图](../artifacts/vditor-preview/comparison/gallery.html)、[完整测量](../artifacts/vditor-preview/comparison/comparison.json)、[尺寸及样式断言](../artifacts/vditor-preview/comparison-verification.json)。两种模式的全部区段高度、主要字体/行高/字距/间距、表格单元格、分隔线、代码 token/颜色、公式和图表尺寸匹配；差值容差 0.1px。跨行行内代码的一个空白盒边界例外见下文，未将其当作完全一致。
- 35/37 组区段的局部高度也与妙言匹配。这不是全部语法或全页像素一致声明，语法差异见下文。
- [发布版交互验证](../artifacts/vditor-preview/acceptance.json)：预览任务只修改原 Markdown 的一个字符，含 YAML 偏移、嵌套和引用任务；两种模式切换不回写原文；中文编辑、撤销/重做、引用 title 和 Markdown 回写通过；未改变的 Mermaid SVG 节点保持同一实例。
- 同一交互验证覆盖只读键盘/双击/表格操作隔离、脚本/事件属性/iframe/javascript 链接清理、分屏双向滚动；Georgia 正文、Menlo 代码、20px 字号、800px 宽度在浅深主题下的测量匹配。发布版没有页面脚本错误。
- TypeScript 检查通过；本次 Vitest 命令实际运行 32 个测试文件、281 个测试，全部通过。其中旧 markdown-it 比较测试已明确标记为历史基线，不能替代新预览的浏览器验证。
- `npx tauri build --bundles app` 通过，桌面包约 20.43 MiB。[独立桌面包](<../artifacts/vditor-preview/MiaoYan Editor.app>)的初始二进制只有 linker ad-hoc 签名、没有资源封印，首次严格校验失败；对副本执行本地 `codesign --force --deep --sign -` 后，严格校验通过，见[签名日志](../artifacts/vditor-preview/codesign.log)。签名不改变源码或已验证的 Web 内容，最终包哈希在签名后重新记录。没有可用 IDE `build_project` 能力，使用项目原生命令；没有重启其他会话正在使用的应用。
- [源码指纹](../artifacts/vditor-preview/frozen-tree.json)、[构建后源码检查](../artifacts/vditor-preview/tree-check.json)、[Web 产物哈希](../artifacts/vditor-preview/web-sha256.json)、[原生包哈希](../artifacts/vditor-preview/app-sha256.json)、[构建日志](../artifacts/vditor-preview/build.log)。构建与验收期间相关源码未变；保留其他会话的设置、同步及文件库代码。

## 保留差异与证据边界

| 项目 | 当前情况与原因 |
| --- | --- |
| C03 跨行行内代码 | WebKit 在 `contenteditable=true` 时保留行尾可编辑空白的盒宽度，本样例相差约 4.507px。相同 DOM/CSS，仅切换 `contenteditable=false → true → false` 即可重现；[独立探针](../artifacts/vditor-preview/geometry-probe.json)记录各行片段和文字范围。区段高度、实际文字起点、字号及行高不变，本轮未修改编辑模型或强行禁止行内代码换行。 |
| C04 转义与公式 | 两种模式现在都按 Lute 显示；妙言会把此样例转义方括号当作数学公式，因而局部高度不同。普通软换行在两个内部模式已统一。 |
| C05 非段首尖括号邮箱 | 两个内部模式均受现有 Lute 解析限制，显示文字而非 mailto 链接。旧 markdown-it 预览可解析此项；本轮统一引擎没有宣称兼容性无损。 |
| C28 SVG data 图片 | 两个内部模式均受现有 Lute 安全过滤影响，图片地址被移除；原版妙言可以显示。本轮没有放宽安全策略。 |
| 脚注操作 | 排版/编号一致；编辑模式保留 SDK 导航工具栏，预览隐藏编辑工具栏，只保留引用跳到定义。不是原版妙言全部导航交互的复刻。 |
| Markdown 全文件逐字节保真 | 无编辑及切换模式时保留原文；实际编辑仍会触发 SDK 对整篇 Markdown 的规范化，首次撤销不保证恢复原始字节布局。 |
| 性能 | [开发环境抽查](../artifacts/vditor-preview/performance.json)中，363 行文档后续文字刷新约 66–73ms，1625 行《调用关系图.md》约 130–155ms；首次转换更慢。只统计解析/DOM/位置匹配到确认，不含所有异步图表绘制完成，不代表手机性能或帧率保证。 |
| 平台 | 验证了桌面构建产物和同份发布前端的 WebKit 行为，没有启动新的原生窗口做独立验收；Android/iOS 安装、系统栏、触控和输入法未验证，手机测试继续暂停。 |

验证路径修正：开发阶段直接脚本滚动隐藏后重排的 iframe，曾出现滚动事件时序不稳定；发布验收使用 iframe 布局完成的双帧屏障和真实滚轮，再等待源码滚动结果，避免把固定等待时间当作交互证据。备份测试源码使用 `.snapshot` 后缀，避免 Vitest 将历史备份识别为当前测试。未修改全局规则或持久记忆。
