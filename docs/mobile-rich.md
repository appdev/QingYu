# 手机端所见即所得验证

2026-11-11。移动端开放 Vditor 所见即所得入口（顶部 Aa），设置中的默认模式也可选择“所见即所得”。保留原有源码、预览及分屏入口，不更换 SDK 编辑模型。

## 改动范围

- `index.html`、`src/main.ts`、`src/settings-view.ts`：取消桌面端限制，沿用现有模式切换、组合输入保护及保存流程。
- `src/editor-viewport.ts`：软键盘改变可视高度后，将活动光标滚入编辑区；预留 32 CSS px，避免 Android 原生光标拖动手柄遮挡底部快捷栏。
- `src/rich-editor.ts`：工具栏触控保留编辑焦点与选区，命令事件沿用 Vditor 4.0.1 的平台规则（iPhone 为 touchstart，其余为 click）。
- 复用 Android 原有系统栏、刘海和键盘 inset 处理，没有新增原生 padding、修改 Manifest 或 SDK 源码。

## 已验证

- vivo V2307A，Android 16 / API 36，手势导航。通过 `adb install -r` 覆盖安装，保留原数据；冷启动成功。
- 使用新增的“手机所见即所得测试”笔记，保留原来的两篇笔记。确认中文候选提交、触控加粗、撤销与重做，以及切到预览后 Markdown 回写正确。
- 最终键盘弹出时，编辑区底部为 409.714 CSS px，快捷栏从同一位置开始，光标底部为 377.661 CSS px，留白约 32.054 CSS px；无横向溢出。原生拖动手柄已离开快捷栏。
- 检查浅色、深色下状态栏和底部手势区域，键盘收起后内容恢复。外观恢复为“跟随系统”，原默认模式仍为源码编辑；当前手动停在所见即所得测试笔记，键盘关闭。
- 编辑后 507 字符 Markdown 在最终 APK 覆盖安装、冷启动前后逐字相同，SHA256：`1ba7f073adeb67e3c6febbc480fc20eebf6c6f187750d0691078e733d26a1f77`。
- 发布前端 WebKit 验证通过：Android / iPhone UA 下的移动入口、默认模式、键盘高度变化、触控加粗、焦点保留，以及收到 composition 事件时的模式切换保护。此项不是 iOS 真机验收。
- 定向 Vitest 命令结果为 4 文件、11 测试通过；原生 Tauri Android debug arm64 APK 构建通过。构建时没有可用的 IDE `build_project`，使用项目原生命令。
- 当前应用进程最后 200 行日志中未匹配 FATAL EXCEPTION、panic、Uncaught 或预览渲染失败；不代表完整稳定性回归。

## 产物与证据

- [最终 APK](../artifacts/mobile-rich/MiaoYan-Editor-mobile-rich-debug.apk)，SHA256：`50516fe8c06bdeb57ef03ea2e57bdf2304538dacd2043f10bd8b011f2bbe5cd7`。
- [验证汇总](../artifacts/mobile-rich/verification.json)、[构建日志](../artifacts/mobile-rich/build-final.log)、[单元测试](../artifacts/mobile-rich/unit-final.log)、[发布前端验证](../artifacts/mobile-rich/browser-release.json)。
- [键盘与光标截图](../artifacts/mobile-rich/final-caret.webp)、[深色键盘截图](../artifacts/mobile-rich/dark-keyboard.webp)、[最终手机画面](../artifacts/mobile-rich/final-screen.webp)。
- [构建输入清单](../artifacts/mobile-rich/final-inputs.json)记录独立源码快照 `artifacts/mobile-rich/source-final`。并行会话随后加入的外部文件工作区等改动不在本次 APK 内；当前所见即所得相关辅助代码与已构建快照一致。没有为其他会话的新增功能重复构建或做验收声明。

## 边界

- 此手机的九键输入法会把尚未确认的拼音留在输入法候选窗口，在提交前没有向 WebView 发送组合输入事件。应用无法保护尚未收到的候选缓存；主动切换模式可能丢弃该缓存。已确认提交的中文与 SDK 编辑回写已验证。
- 未实测 iOS、其他 Android 机型或三键导航，未执行手机端全格式及性能回归。
- 本轮验证工具修正：输入夹具前显式检查焦点；等待布局及保存稳定；接受 SDK 的 b / strong 等价输出；源码快照仅排除根构建目录，保留嵌套的 Vditor dist 资源。未修改全局规则或持久记忆。
