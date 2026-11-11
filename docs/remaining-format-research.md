# 剩余格式的可靠适配路径

后续实现与原型结果见 [引用与 HTML 保真报告](./html-fidelity.md)。下文保留调研时的实验与官方源码证据。

日期：2026-11-11。研究基准是项目固定的 Vditor 4.0.1，实际加载其 bundled Lute（运行时 `Lute.Version` 为 `1.7.6`）。本报告只覆盖隔离转换实验与源码调查，不代表生产功能已经接入。没有升级依赖、关闭 sanitize、修改 CSP 或替换 SDK。

后续已落地的窄范围适配及实际编辑验收，见 [本轮修复验收](./remaining-format-fix.md)。下文保留调研时的证据与边界。

## 结果与分级

| 项目 | 当前结论 | 下一步最小路径 |
| --- | --- | --- |
| 脚注初始位置、正文与定义互相跳转 | **有可落地的窄范围方案**。SDK 输入处理本身就会把定义合并到文末；初次导入没有做相同整理。 | 初始转换后沿用 SDK 规则整理定义，再记录 `renderedSource` / 初始撤销基线。导航按钮放到公开 `customWysiwygToolbar` 的 popover，不加入编辑正文；保留最后一次来源引用。主线程负责真实交互验收。 |
| mark/sub/sup 的语义显示 | **需完整适配原型，不是一条 renderer 即可**。正确的 SDK `data-marker` 协议可让三种原生 mark 经 Spin 稳定保留，但默认导出会改成 Markdown 扩展语法。 | 如果必须保留原 HTML，显示时转换成 SDK 可编辑结构，进入 Spin/序列化前还原原生 `html-inline` 表示；不要全局打开扩展后假定所有原文件语义不变。 |
| u/kbd 的语义显示 | **需原型**。Lute 不稳定保留普通真实标签的编辑表示，u 还会改变段落结构。 | 五标签严格白名单、正确嵌套的显示/还原层；或安全的只读预览、点击回源码编辑。后一种不等于直接富文本编辑。 |
| SVG data URI | **明确是安全策略拦截，不是缺一个 CSS 或图片开关**。固定引擎与上游 sanitize 均删除 SVG data URL 的 src。 | 保持目前限制。若要支持，应先设计安全 SVG 解码/清洗/资源隔离与原文保存，或转成本地安全位图；不能用自定义 renderer 偷渡被 sanitize 删除的 URL。 |
| 非段首尖括号邮箱 | **上游解析位置错误，有精确证据**。并非只发生于中文标点。 | 优先修复并验证上游 Lute；应用层可做有原文还原能力的严格显示适配，但不能只改 link renderer 或对全文做正则替换。显式 `[邮箱](mailto:邮箱)` 当前可用。 |

## 隔离实验及证据

运行：`node artifacts/remaining-format-research/check.mjs`。结果：[result.json](../artifacts/remaining-format-research/result.json)。所有实验保持 `SetSanitize(true)`；raw HTML renderer 只用于固定可信测试片段，脚本中已注明不能复用到生产。

### 1. 行内 HTML：首次显示、编辑规范化、导出是三个不同问题

测试 `<mark>高亮</mark> H<sub>2</sub>O x<sup>2</sup> <u>下划线</u> <kbd>Ctrl</kbd>`，并把“高亮”改为“高亮修改”，然后执行 Spin、再次 Spin、导出、重新导入。

- 只在 `Md2VditorDOM.renderInlineHTML` 原样输出标签，初始显示可以成立；随后 u/kbd 会重回 `html-inline` 代码表示。u 的序列化会插入换行/分段。
- `SetMark/Sub/Sup(true)` 只能启用对应扩展，不能保留 `<mark>`/`<sub>` 的原始写法；直接导出会变为 `==...==`/`~...~`。
- 裸 `<sup>` 缺少 SDK `data-marker="^"` 时，导出实验会丢失闭合 `^`。使用 `<sup data-marker="^">` 则能稳定通过 Spin/导出；mark/sub 加正确 marker 也能稳定。这是 DOM 协议问题，不能把裸标签结果当成 SDK 完全没有能力。
- 在编辑 DOM 里放一个 `contenteditable=false` 的显示副本，`VditorDOM2Md` 仍把其文字写进源码。只读属性不是序列化隔离边界。

可研究的具体方案是：原生 HTML 代码节点为存储协议，显示层只接管“无属性、正确配对、白名单五标签”的内容；规范化和导出前还原，再显示。必须覆盖 `Md2VditorDOM`、粘贴转换、Spin、序列化及撤销恢复，而不只是覆盖初次 render。五标签之间的嵌套、跨 mark 选区、删除开始/结束边界、中文 IME、复制为 HTML/Markdown 都要验证。若保留任意属性、未知标签和不配对 HTML，就不是这条最小方案的范围。

直接开启 mark/sub/sup 还会改变原本显示为字面的 `==...==`、`~...~`、`^...^` 语法解释；这属于兼容性选择，不能为了 HTML 显示静默扩大语法。

### 2. 脚注：可采用 SDK 已有的文末结构

固定样本包含同一个脚注的两次引用、两段定义及定义后正文。实验确认：

- 初始 `Md2VditorDOM()` 把定义放在原始位置。
- 把完整 `footnotes-block` 放到文末，经过 Spin、修改定义首段、导出，重复引用与两段定义仍保留；导出定义规范化到文末。
- 给引用/定义添加普通 id 属性不改变 Markdown；这类属性在后续 Spin 是否保留仍应由每次派生补齐，不能当持久状态。
- 把真实 `<a>↩</a>` 回链直接插进定义 li，导出会产生 `[↩](#fnref-a)`，污染文档。

固定版本源码 [input.ts:184](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/wysiwyg/input.ts#L184) 已合并 `footnotes-block` 并追加至编辑根结尾。初始整理与之保持一致，避免首次输入才突然换位。

公开 `customWysiwygToolbar` 在 [highlightToolbarWYSIWYG.ts](https://github.com/Vanessa219/vditor/blob/v4.0.1/src/ts/wysiwyg/highlightToolbarWYSIWYG.ts) 的 `footnotes-block` 与 `footnotes-ref` 两处被调用。按钮属于 popover，位于可编辑根外，可以执行滚动/设置选区而不进入序列化。`vditor-footnotes__goto-ref` 在本地 4.0.1 仅检索到 CSS 样式，没有对应 WYSIWYG 点击绑定；不能仅加这个 class 就认为导航完成。

无编辑的切换必须继续返回应用保存的原 source；先整理再记录 `renderedSource`，否则“初始规范化差异”可能被当成用户编辑而保存。真实编辑后的定义位置会规范化到文末，应明确承认这一边界。新增来源状态要在换文档时清除，重复引用优先返回本次来源，引用已删除时才回退同 label 的现存引用。

此处转换实验没有代替真实 SDK popover、选区与撤销验收；实现由主线程完成后才可称功能通过。

### 3. SVG data URI：parse、sanitize、CSP 必须分别检查

实验覆盖 percent 编码 SVG、base64 SVG、base64 PNG 和 javascript URL，并分别调用 `SetDataImage(false/true)`：

- `SetDataImage(true)` 下，SVG 两种编码都会生成无有效 src 的图片；PNG data URI 正常保留。
- `Lute.Sanitize('<img src="data:image/svg+xml,...">')` 直接移除 src；不经过 Markdown parser 也会复现。
- 当前 CSP 本来就允许 `img-src data:`，所以本案例无需归因为 WebView/CSP 拦截。
- `SetDataImage` 虽存在于实际引擎中，但不在本项目 Vditor 4.0.1 的公开 Lute 类型声明里；即使调用也不能改变 sanitizer 的 SVG 策略。

上游 [render/sanitizer.go](https://github.com/88250/lute/blob/master/render/sanitizer.go) 的 URL 属性检查明确排除 `data:image/svg+xml`、`data:text/html`、javascript。它是安全边界，不应通过 renderImage 直接重新加入被删掉的 src。

如需支持，可单独原型验证“解码并安全清洗 SVG → 受控图片资源 → 源码仍保存原 URL”；需要限制大小、外部资源、脚本/事件属性，并验证粘贴/保存/撤销时资源映射不泄漏或丢失。安全清洗完成前不宜列为普通样式修复。

### 4. 邮箱：问题是位置偏移，不是 gfmAutoLink 开关

同一固定引擎在 gfmAutoLink 两种状态均得到：

| 输入 | 结果 |
| --- | --- |
| `<test@example.com>` | 正常 mailto |
| `text <test@example.com>.` | 未识别，仍是尖括号文本 |
| `中文<test@example.com>。` | 未识别 |
| `、 <test@example.com> 、` | 未识别 |
| `中文[test@example.com](mailto:test@example.com)。` | 正常 mailto |

官方主仓库 [parse/auto_link.go](https://github.com/88250/lute/blob/master/parse/auto_link.go) 的 `parseAutoEmailLink` 使用 `tokens := ctx.tokens[1:]`，未按当前 `ctx.pos` 切片；同文件 URL 的 `parseAutolink` 从 `ctx.pos+1` 开始。这解释了“段首成功、前面有任意文字就失败”的实测边界。主仓库快照与固定引擎是不同证据，不能据此声称精确定位到了固定 minified JS 的原始行号，但行为相符。

失败的样本在 renderer 中已经是一个普通 Text token，因此 `renderLink` 无法处理。若要应用层适配，必须只处理编辑 DOM 的普通文本节点，跳过代码、链接、HTML 标签和转义标记；同时在 Spin/导出前还原，并验证点击 mailto 与编辑边界。实验中的反斜线转义有独立 `data-type=backslash` 节点，代码和 HTML 标签也各有编辑节点，绝不能把全文正则替换当作等价方案。

更小且可靠的用户入口是对选中邮箱显式插入 `[邮箱](mailto:邮箱)`，但这会修改 Markdown，并不是原语法的自动兼容修复。若要对所有 `<邮箱>` 自动正确解析，上游位置修复与引擎重新构建/升级更合适；仍需已有全部格式回归，不以 alpha 发布说明替代验证。

## 官方源码快照与未验证边界

2026-11-11 下载了官方 Lute master 相关源码作为调查快照，文件 SHA 见 [upstream-manifest.json](../artifacts/remaining-format-research/upstream-manifest.json)；实际执行始终是项目已安装的 4.0.1 引擎，SHA 在 result.json。没有执行 4.0.2-alpha.8，不能声称新版本已修复邮箱或其他格式。

本研究没有完成通用 HTML 的选区/粘贴/撤销、SVG 资源隔离或应用层邮箱恢复原型；没有做手机、性能或其他平台测试。脚注和应用显示适配的生产完成状态以主线程后续测试为准。
