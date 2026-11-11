# 剩余格式：已完成适配与调研边界

后续进展：引用定义 title 丢失已修复，并完成独立行内 HTML 编辑原型。最新实现、验证及边界见 [引用与 HTML 保真报告](./html-fidelity.md)；下文保留上一轮验收记录。

日期：2026-11-11。继续使用 Vditor 4.0.1。未升级依赖、修改 SDK vendor、关闭 sanitize 或放宽 CSP。同步、设置及笔记库的并行会话改动均保留，手机测试继续暂停。

## 本轮落地

| 项目 | 结果 |
| --- | --- |
| 普通正文中的裸 `<br>` | 正文、强调、列表和引用中显示真实换行；通过临时标记，在 Spin/序列化前还原 SDK 的 HTML-inline 表示，防止被 SDK 回写成软换行。表格沿用上一轮实现。其他标签和带属性的 `<br>` 不扩大支持。 |
| 引用链接显示 | 保留 SDK 的 span/reference 语法，颜色与普通链接相同。默认隐藏定义，但定义节点仍在编辑文档中，不删除用户内容。 |
| 引用打开与编辑 | 使用 SDK 自带弹层，增加“打开链接”“编辑定义”；支持 ⌘/Ctrl+单击打开。编辑定义时暂时显示定义块，点击其他正文后收起。目标地址由公开的 `VditorDOM2HTML` 解析，沿用 SDK 安全过滤，内部锚点复用上一轮规则。 |
| 脚注初始布局 | 初次转换就把完整脚注块移到文末，与 SDK 原有的后续输入处理一致；在记录规范化基线前完成。无编辑切换仍保留原 source，实际编辑后定义位置会规范化到文末。 |
| 脚注导航 | SDK 脚注弹层增加“跳到脚注”“返回引用”，返回时优先回到本次点击的重复引用。按钮位于编辑正文外，不把回链文字写进 Markdown。补齐点击上标边缘的选区定位，并通过 SDK 原有 click 路径刷新弹层。 |
| 脚注排版 | 去掉 SDK 额外的分隔线与 32px 顶部 padding，按妙言设置正文到脚注的间距；正文及脚注内容继续使用现有排版。 |

主要源码：[rich-markdown.ts](../src/rich-markdown.ts)、[rich-editor.ts](../src/rich-editor.ts)、[rich-editor.css](../src/rich-editor.css)、[测试](../tests/rich-markdown.test.ts)。没有把引用 span 直接改为 a；隔离实验已证明后者会把 `[文字][id]` 转成内联链接，破坏原引用形式。

## 验收

- 4 个相关测试文件、29 项通过；整工程 `tsc --noEmit` 通过。普通 HTML 标签、代码中的 `<br>`、带属性标签保持原模型；反复 Spin/导出不泄漏临时标记。
- WebKit 真实页面验证正文换行编辑/撤销/重做；引用的颜色、隐藏与编辑定义、修改目标后打开、内部锚点、危险地址拦截；脚注末尾布局、跳转与返回第二处引用、脚注正文编辑/撤销/重做；预览回写与无编辑原文一致。见 [交互结果](../artifacts/remaining-format/interaction-result.json)、[单元测试日志](../artifacts/remaining-format/unit.log)。
- 同一份 37 组全格式文档，浅深两种主题，148 张截图。新增比较引用区段及脚注盒子/间距，代码高亮、7 个 Mermaid 和 Markmap 的既有对齐结果继续回归。见 [最新对照](../artifacts/format-comparison/release-remaining-format/gallery.html)、[几何断言](../artifacts/remaining-format/comparison-verification.json)。
- 脚注操作入口采用 SDK 弹层，未复制原版正文里的 ↩ 回链；这属于有意保留的编辑交互区别，不宣称所有 DOM 或操作逐项相同。

## 调研结论

完整实验与官方源码证据见 [剩余格式调研](./remaining-format-research.md)，可运行 `node artifacts/remaining-format-research/check.mjs` 复现。

1. **完整行内 HTML**：白名单五标签的显示/还原层有可行方向。mark/sub/sup 需要正确的 SDK marker，单纯真实标签输出不足；u/kbd 还存在结构变化。需要单独覆盖嵌套、边界删除、跨节点选区、粘贴和中文输入，当前保持原状。
2. **SVG data URI**：固定引擎的 sanitizer 会主动删除 SVG src，而 PNG 可保留；现有 CSP 本已允许 data 图片。不是补 CSS 或开启图片开关即可修好。后续需要独立的 SVG 清洗、隔离资源及原文映射方案，本轮不绕过此安全策略。
3. **非段首尖括号邮箱**：不仅中文，ASCII 前缀也能复现。官方解析源码存在忽略当前位置的切片逻辑，与固定引擎结果一致；建议上游修复后验证。不能靠 link renderer 修复已经被解析成普通文本的内容。
4. **引用定义 title 保真**：固定 4.0.1 初始编辑转换就可能丢弃定义的 title。应用仍用原 source 缓存保护无编辑切换，但实际编辑回写不保证 title 保留；本轮链接外观、定义编辑和跳转不等于完整引用模型保真。

## 构建及运行边界

使用项目原生命令 `npx tauri build --bundles app`，完整类型检查、前端及原生构建成功，应用 19.48 MiB，签名验证通过。见 [构建日志](../artifacts/remaining-format/build.log)。本轮包位于 [MiaoYan Editor.app](</Users/evan/Downloads/public/MiaoYanEditor/artifacts/remaining-format/MiaoYan Editor.app>)；保留主应用实例供另一个会话使用，没有重启或覆盖正在使用的主实例，也没有向真实笔记库写入测试文档。打包版前端通过同一 WebKit 的实际编辑验证；本轮未单独做原生窗口验收，不将浏览器验证称为原生验收。

本轮四个目标文件在构建及最终验证期间内容未变，见 [文件指纹](../artifacts/remaining-format/frozen-scope.json)。并行会话继续修改 main、settings-view 和原生 file_library，因此不宣称当前整个工作区与构建输入完全一致，也不为追赶无关改动重复打包。最终比较使用复制并逐文件校验的独立发布前端（1423），避免开发服务或后续 dist 变化影响结果；[发布前端指纹](../artifacts/remaining-format/release-web-sha256.json)及[最终验收记录](../artifacts/remaining-format/verification.json)记录这一边界。

其他平台、手机、长文档性能、完整 HTML 往返和 SVG 隔离未验收。实际编辑仍有 SDK 的规范化行为，不承诺 Markdown 逐字保真。并行会话的同步/设置功能不在本报告验收范围内。

## 验证路径修正

共享开发服务出现过 Outdated Optimize Dep 504；只隔离端口仍共享依赖缓存。日志还确认并行设置模块修改触发 page reload，测试输出 HTML 也可能触发开发服务刷新。开发验证采用任务独立缓存、明确入口及关闭文件监听的临时 Vite 配置，最终验证采用独立发布前端快照；浏览器检查顺序执行，生产构建配置未改。预览回写等待具体更新内容，而不以旧 h1 已存在作为渲染完成信号。脚注验收增加末尾盒子及间距断言，避免只验证移位和跳转后遗漏显示层样式。
