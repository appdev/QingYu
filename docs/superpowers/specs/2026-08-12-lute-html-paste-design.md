# Lute HTML 粘贴转换设计

## 目标

让思源宿主中的 Markdown 可视化编辑器优先使用 Lute 将结构化剪贴板 HTML 转换为 Markdown，使标题、普通表格、列表、链接和代码块的粘贴结果与思源块编辑器保持一致，同时保留 Markra 的宿主无关能力。

## 边界

- `markra-core` 不直接访问全局 `Lute`。
- `MarkdownHostAdapter` 提供可选的 HTML 转 Markdown 能力。
- 思源宿主惰性获取或创建 Lute 实例，避免动态脚本尚未加载时在模块初始化阶段报错。
- 转换前使用 `Lute.Sanitize` 清理外部 HTML。
- 无宿主转换器、转换失败或转换结果为空时继续使用现有 Turndown 转换。
- 保留现有 Markdown 源文识别、IDE 代码识别、HTML 结构判断、远程图片提取和图片本地化流程。

## 数据流

1. 粘贴处理读取 `text/plain`、`text/html` 和文件。
2. 现有代码粘贴识别先处理明确的 IDE 代码。
3. HTML 转换模块继续解析 DOM、判断结构并收集远程图片。
4. 对普通结构化 HTML，优先调用宿主注入的转换器；思源实现使用 `lute.HTML2Md(Lute.Sanitize(html))`。
5. 宿主转换不可用或没有产生有效 Markdown 时，回退到现有 Turndown 规则。
6. 转换结果继续经过换行规范化、远程图片定位和编辑器插入流程。

## 兼容性与安全

- Lute 的列表标记配置为 `-`，减少无意义的 Markdown 源文变化。
- 普通 GFM 表格必须能够被 Markra 表格解析器识别。
- 合并单元格产生的 Kramdown IAL 不在本次扩展支持范围内，但不得破坏普通表格。
- 外部 HTML 在进入 Lute 前必须清理；Markra 原始 HTML 渲染层继续执行二次清理。
- 远程图片 URL 必须仍可在生成的 Markdown 中定位；无法定位时不得替换错误文本。

## 验证

- 添加粘贴集成测试，证明包含标题、普通表格、列表和代码块的结构化 HTML 使用宿主转换结果。
- 添加回退测试，证明没有宿主转换器或转换结果为空时仍使用 Turndown。
- 添加安全测试，证明传给宿主转换器的是清理后的 HTML。
- 覆盖远程图片转换后的定位行为。
- 运行 Markdown 相关测试和 `cd app && pnpm run lint`。
