# 行内 HTML 接入与脚注统一验收

日期：2026-11-11。Vditor 仍为 4.0.1。手机测试继续暂停，未修改依赖、vendor、安全过滤或 CSP。其他会话的设置、同步和原生入口修改均保留。

## 已完成

行内 HTML 的显示/还原逻辑已移至 [rich-inline-html.ts](../src/rich-inline-html.ts)，通过 [rich-markdown.ts](../src/rich-markdown.ts) 接入主编辑器原有转换入口。仅维护一套 Markdown、HTML 粘贴、Spin、Markdown 导出和 HTML 导出处理，不再额外安装原型转换钩子。旧验证页仍保留，但已直接使用主编辑器的同一实现。

支持正确配对、无属性的 `mark/sub/sup/u/kbd`，包含嵌套、Markdown 组合、列表、引用和表格。保留原开闭标签写法；其他标签、属性、错误配对、代码和 HTML 块继续走 SDK 原模型，不扩大 HTML 执行范围。正文不包含这些格式时跳过额外的模板转换，未修改的未知 HTML 返回原转换结果。

[rich-editor.css](../src/rich-editor.css) 按妙言补齐高亮底色、下划线、上下标及键盘文字。键盘文字明确使用 Courier：实测妙言没有声明文档语言时的系统 `monospace` 回退是 Courier；将其文档语言改为 zh-CN 会让键盘文字高度从 18px 变为 18.625px，连带段落高度从 27.21875px 变为 26.734375px。我们的两个模式明确同一字体后，恢复为妙言的原始尺寸，未更改整页语言或其他字体设置。

脚注在 [markdown.ts](../src/markdown.ts) 使用 markdown-it-footnote 的公开 renderer 接口统一：引用编号采用纯数字，重复引用同号，去掉插件自动添加的分隔线；预览返回链接也按妙言给重复来源附加上标序号。原有正文分隔线保留。脚注区域继续使用原版预览 CSS，其高度和正文间距已与可编辑区域一致。

## 相同文档三方对比

文档：[全格式对照.md](../artifacts/format-comparison/全格式对照.md)，37 组格式及 YAML；SHA256 为 `d541026362ef0c42d6099740312ed736bc76357fe2ee53106a9db21bbb0c66f6`，未改写测试文档。

基准是妙言 4.8.0 实际生成 HTML 及原样资源回放，另两方是本轮发布前端的所见即所得和预览。使用相同 WebKit、默认字体与宽度、内容视口 1120×900，浅深两种主题，共 222 张截图。不是重新在原生妙言窗口里截图。

[打开三方对照](../artifacts/inline-html-integration/comparison/gallery.html)，默认打开 C29；C27 看引用编号，C37 看文末脚注。

| 检查 | 结果 |
| --- | --- |
| 行内 HTML 六个实际元素 | 两种模式的标签、文字、字号、行高、颜色、底色、留白、边框、上下标偏移、阴影和元素宽高均与妙言匹配。 |
| 脚注编号 | 三方均为 `1、1、2`。 |
| 脚注分隔线 | 三方均没有自动脚注分隔线，正文原有分隔线仍在。 |
| 文末脚注区域 | 三方均高 89.25px，正文到脚注间距均为 18.390625px，浅深色相同。 |
| 其他局部排版 | 31 组区段的局部高度与妙言相差小于 0.1px，两个模式及两个主题均通过。不是全页逐像素一致声明。 |
| 代码与图表回归 | 普通代码 token 及颜色相同，7 个有效 Mermaid 的 SVG viewBox 与原版相同。 |

完整数据：[comparison.json](../artifacts/inline-html-integration/comparison/comparison.json)、[尺寸与样式断言](../artifacts/inline-html-integration/comparison-verification.json)。

## 编辑、撤销与 Markdown 回写

[发布版交互结果](../artifacts/inline-html-integration/edit-result.json)使用同一份完整文档，覆盖：

- 仅查看及切换模式时，原 Markdown 完全不变。
- 逐项在五种标签内真实输入中文，保留 HTML 开闭标签，不改成 Markdown 扩展语法；经 CodeMirror 回写，引用定义 title 仍保留。
- 逐项撤销、重做；首次操作恢复原标签内容，后续操作对完整导出快照做相等断言。
- 多段脚注正文编辑、撤销和重做；`[^one]` 的三处标记及 `[^two]` 的两处标记保留，正文改动之外的当前规范化 Markdown 不变。
- 从第二次引用跳到脚注并返回该次引用，导航不改写文档；切到预览后能看到更新后的高亮文字和脚注正文，编号仍为 `1、1、2`，没有脚注分隔线。

**保真边界**：首次实际编辑依然会触发 SDK 的整篇格式规范化，初次撤销恢复的定义位置也可能与编辑后不同。标签、标题和脚注内容的保留不等于整个文件逐字保真。初次撤销及编辑后的实际导出分别保存在 [initial-undo.md](../artifacts/inline-html-integration/initial-undo.md) 和 [initial-edit.md](../artifacts/inline-html-integration/initial-edit.md)。未为此引入新的 SDK 编辑模型。

## 测试与桌面构建

相关命令：

```sh
npx vitest run tests/inline-html.test.ts tests/rich-markdown.test.ts tests/editor.test.ts tests/preview.test.js tests/rich-code.test.ts
npx tsc --noEmit
npx tauri build --bundles app
```

Vitest 实际筛选到 9 个文件、90 项测试，全部通过；类型检查通过。桌面构建成功，应用 20.23 MiB，签名验证通过。本轮包：[MiaoYan Editor.app](</Users/evan/Downloads/public/MiaoYanEditor/artifacts/inline-html-integration/MiaoYan Editor.app>)。

本轮目标文件在代码审核、冻结、构建及最终发布验证期间保持不变；比较和交互使用从本轮发布产物复制并校验的独立前端快照，避免开发缓存或并行会话后续构建覆盖验收输入。见 [最终记录](../artifacts/inline-html-integration/verification.json)、[测试日志](../artifacts/inline-html-integration/unit.log)、[构建日志](../artifacts/inline-html-integration/build.log)。

构建后，并行会话又修改了文件库、设置、原生入口和 Cargo 依赖；路径已列入最终记录的 `otherWorktreeDrift`。这些后续变更未纳入本轮构建验收，不把当前整个工作树声明为已编译通过，也不为追赶无关变更重复构建。本轮目标文件和实际验收产物保持不变。

未重启其他会话正在使用的主应用，没有向真实笔记库写入测试文档。本轮没有原生窗口、系统输入法候选、系统剪贴板、Windows/Linux 或移动设备人工验收。

## 对比中确认的剩余差异

以下不是本轮新接入功能的通过项，也没有混入已对齐的 31 组区段：

| 项目 | 当前差异与原因 |
| --- | --- |
| C04 软换行 | 所见即所得保留物理换行，预览按普通 HTML 空白折叠；区段相差约 26.23px，属于现有编辑模型差异。原版妙言另有转义方括号被当作公式的行为。 |
| C05 邮箱 | 非段首尖括号邮箱仍有 Lute 解析问题；引用定义 title 的修复继续通过，不把 title 列作剩余缺陷。 |
| C20 块公式 | 公式本体可渲染，两个模式的区段高度仍相差约 2.39px，外层留白有差异。 |
| C28 SVG data URI | 预览的 markdown-it 拒绝该地址，所见即所得的 Lute sanitizer 删除图片 src。两边都未正确显示，失败表现不同。 |
| C31 非标准定义列表 | 两边没有启用对应扩展，其中普通换行在可编辑区和预览中仍相差一行。 |
| C36 PlantUML | 图像和原始尺寸相同，但我们的预览仍残留 loading 文案，外层留白与居中不同；所见即所得与妙言一致。 |

可编辑脚注继续使用 SDK 弹层导航，预览使用正文返回链接；这项交互区别仍保留。带属性 HTML、其他标签、完整 HTML 块直接编辑及 SVG 安全隔离仍未扩大支持。

## 验证路径修正

开发服务启动时再次遇到过模式被重置。服务日志这次明确显示 `new dependencies optimized: markdown-it-footnote, markdown-it-task-lists` 随后的自动 reload，而不只是文件监听造成刷新。仅关闭 watch 不足以避免这一类冷启动干扰；本轮验证配置显式预构建这两个插件及实际使用的依赖，正式验收改用独立发布快照。生产 Vite 配置未改。

旧测量脚本把所有有 `pre` 祖先的行内元素排除，但 Vditor 编辑根本身就是 `pre`，导致无法采集原生行内标签。新的三方脚本仅排除真正的代码预览 pre，允许编辑根 pre，并对六个元素逐项断言。

完整文档编辑时，WebKit 曾报告单次 `ResizeObserver loop completed with undelivered notifications.` 布局通知警告；它在交互记录中独立保存，其他脚本异常仍使验证失败。没有把这个警告作为无异常运行的证据，也没有在本轮扩大修改观察器或长文档性能逻辑。
