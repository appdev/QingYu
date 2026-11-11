# 引用定义保真修复与行内 HTML 原型

后续进展：五标签适配已接入主编辑器，脚注与两个模式的样式也已按妙言统一。最新三方对比和发布验收见 [行内 HTML 接入报告](./inline-html-integration.md)。下文保留此前原型阶段记录。

日期：2026-11-11。Vditor 仍为 4.0.1；没有修改 vendor、关闭 sanitize、放宽 CSP 或启动手机测试。

## 引用定义 title：已接入主编辑器

问题发生在 `Md2VditorDOM` 和 `SpinVditorDOM`：引擎输出定义块时省略 title，而 `VditorDOM2Md` 能原样导出块里已有的定义文字。应用原来的无编辑缓存只能保护查看，不能保护实际编辑后的保存。

修复位于 [rich-references.ts](../src/rich-references.ts) 和 [rich-markdown.ts](../src/rich-markdown.ts)：

- 复用现有 markdown-it 的 reference block rule，识别真正的定义，不从代码块、行内代码或普通正文里抓取相似字符串。
- 初次导入从当前 Markdown 保留定义原文；规范化时从当前编辑 DOM 提取。按标签匹配恢复定义，保留 title 的引号、转义、换行及目标地址尖括号，不靠跨编辑历史的旧元数据补回。
- 用户修改、清空或删除 title 后，以当前文本为准；删除整条定义也不会复活。SDK 正常的文档规范化行为仍保留。
- 额外处理光标紧跟 `<destination>` 时的解析失败：仅在 Spin 前移开有效定义中的临时 `wbr`，恢复定义原文时恢复光标位置，避免删完 title 就变成普通段落。
- 定义原文始终通过文本节点恢复。收集器允许记录危险地址的字面源码以避免丢内容，但不渲染 HTML、不打开链接；实际显示与打开仍通过 Lute sanitize。测试包含保留 `javascript:` 定义文字而拒绝打开的情况。

直接导入 markdown-it 已导出的 reference rule，不依赖压缩后可能变化的函数名。相关类型声明补在 [types.d.ts](../src/types.d.ts)。

## 行内 HTML：独立原型，尚未默认启用

实现现已移至 [rich-inline-html.ts](../src/rich-inline-html.ts)，演示：[inline-html.html](../prototypes/inline-html.html)。开发服务启动后访问 `/prototypes/inline-html.html`。本轮专用服务地址为 `http://127.0.0.1:1422/prototypes/inline-html.html`；演示不读取或保存笔记。

原型先覆盖前一轮调查的五种标签：`mark`、`sub`、`sup`、`u`、`kbd`，包括正确配对的嵌套及与 Markdown 加粗、列表、引用、表格的组合。

显示时把 SDK 的 HTML-inline 开闭标签节点配对成真实可编辑元素；原始开闭标签作为节点属性保存。进入 Spin、Markdown 导出和 HTML 导出前，再还原成 SDK 的 HTML-inline 表示。这样不需要打开 `==...==`、`~...~`、`^...^` 扩展，也不会把原 HTML 静默改写为这些语法。HTML 粘贴走 SDK `HTML2VditorDOM` 及原有安全处理后再适配。

这是验证完整行内 HTML 适配路径的第一版原型，不等于任意 HTML 编辑器。**带属性、未知标签、未闭合/错误交叉标签、HTML 块和代码内容继续保留 SDK 原表示**，不扩大执行 HTML 的范围。主编辑器只接入 title 修复；没有引入 HTML 原型开关或替换默认编辑模型。

## 验证与证据

- [49 项相关测试](../artifacts/html-fidelity/final-unit.log)：五个文件通过；整工程 [类型检查](../artifacts/html-fidelity/final-typecheck.log) 通过。
- [主编辑器实际交互](../artifacts/html-fidelity/production-title-result.json)：无关正文编辑后 title 不丢失；title 修改、删除、撤销/重做及 CodeMirror 回写；单双引号、尖括号地址、多行定义；所见即所得与预览切换后保留编辑结果。
- [发布前端既有功能回归](../artifacts/html-fidelity/interaction-result.json)：沿用上一轮交互脚本，正文 `<br>`、引用打开/定义编辑/危险地址拦截、脚注位置与来回跳转、撤销/重做和预览回写均通过。
- [HTML 原型 WebKit 验证](../artifacts/html-fidelity/webkit-result.json)：五标签内真实输入和中文文本插入、嵌套编辑、起始边界退格、跨节点替换、列表/引用/表格、内部 Enter 换行、HTML 粘贴、SDK 复制、模拟 composition 事件，以及上述编辑的撤销/重做。
- 每个撤销/重做断言比较完整的导出 Markdown 快照；标签内文本编辑保留 HTML 开闭标签，不泄漏 `data-rich-*` 或零宽标记。复制测试保留原始 `<MARK>` 大小写。
- 同一份 [37 组全格式文档](../artifacts/format-comparison/全格式对照.md)，启用原型前后各执行三次 Spin，导出 Markdown 完全相同。该检查验证原型没有增加回写差异，**不等于所有格式与妙言预览完全一致**，本轮没有重做 148 张格式截图。
- [构建日志](../artifacts/html-fidelity/build.log)、[目标文件指纹](../artifacts/html-fidelity/frozen-scope.json)、[最终记录](../artifacts/html-fidelity/verification.json)记录打包及发布产物复核结果。

桌面构建 `npx tauri build --bundles app` 成功，应用 19.99 MiB，签名验证通过。本轮包：[MiaoYan Editor.app](</Users/evan/Downloads/public/MiaoYanEditor/artifacts/html-fidelity/MiaoYan Editor.app>)。发布产物前端复制并校验后独立提供服务；上面的主编辑器交互结果来自该发布产物（1423），不是开发缓存。本轮没有做原生窗口人工验收。

构建后，并行会话继续修改设置、快捷键及原生入口；具体路径见最终记录的 `concurrentWorktreeDrift`。本轮目标文件、应用包和验收用前端快照均未变，不把当前整个工作区宣称为已经构建验收，也没有为追赶其他会话的后续修改重复打包。

## 尚未覆盖的边界

1. 任意属性和其他 HTML 标签尚未直接编辑；当前原型采用安全回退，仍需独立验证更广白名单及属性处理。
2. 中文文本输入和模拟 composition 序列已测，**真实 macOS 输入法候选、选词与取消组合未人工验收**。系统剪贴板、拖放、辅助功能及大文档性能也未验收。
3. HTML 内 Enter、跨节点替换会按浏览器与 SDK 的编辑语义拆分或保留元素。撤销可还原对应快照，但不承诺编辑后整个文件逐字不变。原本存在的列表/表格/定义位置等 SDK 规范化仍存在。
4. 本轮桌面构建包含其他会话在同一工作区的现有改动；同步、设置和笔记库功能不在本轮验收范围。没有重启或覆盖另一个会话使用的主应用，也没有向真实笔记库写入测试文件。手机测试继续暂停。

## 验证路径修正

SDK 默认历史合并延迟为 800ms。应用 blur/save 可能提前回写到 CodeMirror，因此“原文已变”不等于“撤销快照已提交”；交互检查等待历史提交后再断言。选区辅助函数按整个定义文本定位，允许 SDK 的临时 `wbr` 把文字拆成多个节点。

专用 Vite 服务关闭文件监听以避开并行会话刷新，因此源文件变更后必须重启服务，不能把旧的转换缓存当成新代码。开发期还遇到过资源加载失败；之后记录失败请求，最终主编辑器验证使用独立发布产物快照，不把开发服务超时当成格式根因。这些修正只作用于本轮验证路径，没有修改全局规则或持久记忆。
