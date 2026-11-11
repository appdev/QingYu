# Android 真机验证（2026-11-11）

设备：vivo V2307A，Android 16 / API 36，arm64-v8a，1260 × 2800，手势导航。
测试已恢复；采用保留数据的覆盖安装。原有笔记在迁移后的磁盘文件中逐字节一致，应用内保留一篇“手机格式测试”供查看。

## 修正

- `src-tauri/src/file_library.rs`、`file_library/folders.rs`：Rust 1.96 的标准库文件锁在 Android 上返回 Unsupported，改用 `flock`，没有取消并发写入保护。
- 同一文件库：设备禁止硬链接，Android 新建和移动改用 `renameat2(RENAME_NOREPLACE)`。创建事务记录暂存文件的设备号和 inode，重启恢复可以识别本次创建的文件；旧事务字段保持可读取。
- `MainActivity.kt`、`src/preferences.ts`、`src/types.d.ts`：通过仅允许应用来源、仅主框架的 WebMessage 通知原生外观；系统栏图标和背景跟随应用浅色/深色选择，保留原有系统栏及键盘 inset 处理。
- `src/settings.css`：输入框增加滚动留白，避免软键盘出现时底部字段被设置页脚遮住。

Rust 标准库兼容性核对：[Rust 1.96 文件锁实现](https://github.com/rust-lang/rust/blob/1.96.0/library/std/src/sys/fs/unix.rs)。手机探针实际返回 `try_lock() not supported`，原生 `flock` 的竞争排斥与关闭释放检查通过；同目录 `ln` 实际返回 Permission denied。

## 验证结果

| 项目 | 结果 |
| --- | --- |
| 启动、旧数据迁移、新建、编辑、重命名、自动保存 | 通过 |
| 强制结束后重启 | 原有笔记保留；测试文档与源 Markdown 逐字节一致 |
| 源码编辑与预览切换 | 通过；移动端继续不开放所见即所得 |
| 标题、粗斜体、删除线、列表、任务、引用、分隔线、表格 | 通过 |
| Kotlin 代码高亮、行内和块级公式、脚注、Mermaid | 通过；深色 Mermaid 三个节点及连线可见 |
| 侧栏开关、新建后返回编辑区 | 通过 |
| 四个设置分类、系统返回键关闭设置 | 通过；桌面专用设置项在手机隐藏 |
| AK/SK 自动保存、强制结束后恢复、密码遮蔽 | 通过；原始 SharedPreferences 中未检出测试密钥明文 |
| 立即同步网络失败 | 错误显示在按钮下方，结束后按钮恢复可点 |
| 浅色、深色系统栏与手势导航区域 | 通过 |
| 编辑键盘、格式工具栏、设置底部输入框 | 通过；最终包字段底边 390.50 CSS px，滚动区底边 404.57 px |
| 横竖屏设置布局 | 通过；视口分别为 765 × 322、360 × 744，无页面横向溢出 |

回归检查：文件库 Rust 测试 22 项通过（macOS Host）；设置与同步相关前端测试 30 项通过。Android 原生及前端构建通过；最终进程日志未发现所检查的崩溃、Rust panic 或 Chromium console error。

构建使用 `npx tauri android build --debug --target aarch64 --apk`。没有可用的 IDE `build_project` 接口；Jugg 未初始化本项目，采用 Tauri 原生构建和 Android CLI / ADB / WebView CDP 检查。没有清除应用数据。

## 交付与边界

最终包：`artifacts/mobile-test-20261111/MiaoYan-Editor-mobile-debug.apk`。
APK SHA-256：`52053af7d6cb5b26dccf4b295eee588233e3963a3de8d00bd0b4cf5b005bbae4`。
源码清单 SHA-256：`5e8b88adb3930d9fb43e85b4b0fb027538109daf0a5bf1cf3df2fff8ecbcce27`。
源码与证据：同目录 `source.zip`、`frozen-inputs.json`、`verification.json`、构建和测试日志及 WebP 截图。

当前工作目录有其他聊天的并发改动，因此最终包从固定源码快照构建，构建后清单核对无变化；后续工作目录变更不自动包含在此包中。

测试后已恢复原有外观、同步空配置、旋转设置，并保留测试笔记。原始应用数据备份只允许当前用户读取，不包含在源码包中。

未验证 iOS、其他 Android 版本或厂商、三键导航、大文档性能，以及真实 S3 成功传输和双端冲突。本轮 S3 网络测试使用合成密钥和不可达回环地址，没有操作云端笔记库。
