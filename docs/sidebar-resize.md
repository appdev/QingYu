# 桌面分栏拖动

本文保留初版实现与验证记录；列表拖动下限和窗口缩放的后续修正见 [分栏对齐记录](sidebar-alignment.md)。

按原版妙言默认 AppKit 窗口实现，不采用试验性 SwiftUI 窗口的另一套尺寸。

- 文件夹栏默认及恢复下限为 138px，上限 280px；拖动时允许继续缩小，到 86px 及以下吸附收起。
- 笔记列表默认 280px，可见宽度至少 220px，上限 600px；小于 180px 收起，并同时收起文件夹栏。
- 宽屏布局给编辑区保留至少 480px，窗口空间不足时限制实际显示宽度，不覆盖保存的偏好宽度。900px 及以下继续沿用原来的抽屉布局，不显示这两个拖动手柄。
- 记住宽度及三栏／两栏／纯编辑区状态，收起不覆盖原宽度。存储在本地界面偏好 `miaoyan-editor.sidebar-size.v1`，不写入笔记正文或同步数据。
- 9px 透明命中区沿用原有细分隔线；鼠标捕获归属始终可见的外层容器，支持拖动途中收起再拉回。Escape、指针取消或失去窗口焦点会取消未完成的拖动。分隔线可用方向键调整，Home 收起、End 调到可用上限。

实现归属：`src/sidebar-layout.ts` 管尺寸规则，`src/sidebar-resize.ts` 管指针和本地偏好；显隐仍走 `src/sidebar.ts` 原有布局状态。未修改 Notebook、文件保存、同步或编辑器模型。

## 2026-11-11 验证

- TypeScript 检查通过；定向 Vitest 命令通过，见 [测试日志](../artifacts/sidebar-resize/unit.log)。该命令实际匹配了工作区内历史快照中的同名测试，不将其计数作为新增覆盖范围。
- WebKit 对同一份发布前端的 22 组检查通过：两栏上限、收起／恢复、拖动中拉回、取消、键盘、刷新后的记忆、缩小再放大窗口、编辑区下限、跨预览 iframe 拖动、正文模型不变，以及手机抽屉保留。[验证结果](../artifacts/sidebar-resize/verification.json)
- 原生 Release 构建通过，本地 ad-hoc 签名后严格校验通过；构建输入完整包含内置编辑器资源，构建期间快照未变。[构建日志](../artifacts/sidebar-resize/rebuild/build.log)、[输入清单](../artifacts/sidebar-resize/rebuild/inputs.json)
- 已运行新桌面包，原生窗口实际鼠标拖动后，文件夹栏为 201px、列表为 360px；分别拖动收起，再以 ⌘1／⌘2 展开，恢复这两个宽度。正常退出并确认进程结束后重开，两个分隔线的辅助功能数值仍为 201、360。
- [当前桌面包](<../artifacts/sidebar-resize/rebuild/MiaoYan Editor.app>)保留运行，停在三栏布局。没有可用的 IDE `build_project`，使用项目原生 Tauri 构建命令。

未实测 Windows/Linux 的原生鼠标交互，未打包或安装手机版本，未重跑全格式渲染回归。窄窗口继续采用既有抽屉行为，因此不宣称整个窗口缩放策略与原版完全一致。

验证工具修正：窗口变化后等待实际几何达到预期；正文保真读取 CodeMirror 模型，不读取隐藏或虚拟化 DOM 的 `innerText`。未修改全局规则或持久记忆。
