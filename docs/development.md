# 开发与发布说明

本文说明如何运行、构建和发布轻羽，以及如何维护界面配色。产品介绍见 [README](../README.md)。

中文品牌为“轻羽”，英文为 `QingYu`，包与命令使用 `qingyu` / `qingyu-api`。侧边栏与登录页共用 `public/brand/qingyu-mark.svg`，应用 ICNS 使用用户提供的原文件。保留原生 JNI 库名、旧存储键、`.miaoyan` 元数据及原版妙言的第三方资源与版权说明。

Docker 中设置 `QINGYU_WEB_PASSWORD` 即可启用只有访问密码的 Web 登录，不需要账号。Web 编辑器通过同一服务读写服务器挂载目录，与桌面端共用磁盘存储核心；部署、HTTPS 代理和草稿边界见 [部署说明](api-service.md#web-登录与-docker-密码)。

基于 Tauri 2 的 Markdown 编辑器，提供本地笔记库和文件夹，使用所见即所得编辑。

## 运行

```sh
npm install
npm run dev            # 浏览器开发，http://localhost:1420
npm run tauri dev      # 原生桌面窗口
npm test
npm run build
npm run tauri build
```

原生应用的正式标识为 `com.apkdv.qingyu.editor`，调试标识为 `com.apkdv.qingyu.editor.debug`。用 `npm run tauri -- …` 启动开发或打包，按构建模式选用对应标识。

从 `app.miaoyan.editor.prototype` 升级前，请先备份旧版笔记。移动系统按新标识安装独立应用，旧版应用沙箱不会自动迁移。桌面端此前选定的笔记目录可在新版中重新打开，应用私有设置及登录状态可能需要重新配置。

Linux 使用 `npm run tauri -- build --bundles deb` 构建 DEB，加 `--debug` 构建调试版。打包需要 `dpkg-deb`，产物的 `Package` 字段取对应构建模式的标识。

Android 工程已生成，保留了键盘和系统栏 Insets 适配，直接使用：

```sh
npm run tauri -- android dev
npm run tauri -- android build --debug --target aarch64 --apk
```

无需重新执行 `android init`。构建需要 Android SDK、NDK、JDK，以及 Rust 的 `aarch64-linux-android` target；项目固定 Rust 1.96.0。
iOS 工程已纳入仓库，可使用 `npm run tauri -- ios dev` 调试。

## 颜色配置

运行时配色统一维护在 [`src/palette.json`](../src/palette.json)，浅深色使用相同的语义项，以妙言 4.8.0 和参考源码 `9ce84b01` 为基准。界面文字、源码标记、正文渲染、语法高亮、图表、提示块和设置控件按用途分开；调整颜色时修改对应语义项。正式登录页复用应用的外观偏好，支持保存的浅色／深色、跟随系统和跨窗口更新；深色的高亮兜底、错误文字和按钮交互色也在同一色表中维护。

Vite 的 PostCSS 插件将色表编译进 `theme.css`，主窗口、预览、设置和正式 Web 登录页共用；图表脚本通过 `rich-editor.ts` 获取同一份 JSON，macOS 简介弹层通过 Rust `include_str!` 读取它。修改后分别重建前端和原生程序即可生效，不维护生成的 CSS 色表。`public/miaoyan/css/theme-*.css` 和登录页 tokens 只保留名称映射；系统选区颜色、第三方 SDK 内部样式及平台启动背景由各自平台负责。

## 版本发布

[GitHub Releases](https://github.com/appdev/QingYu/releases) 提供 macOS Apple Silicon/Intel DMG、Windows x64 安装程序、Linux x64 DEB/AppImage、Android arm64 APK，以及用于自托管 Web 编辑器的 Linux amd64/arm64 Docker 镜像归档。首版为 `v0.1.0`，安装及签名边界见[发布说明](releases/v0.1.0.md)。

在 GitHub Actions 的 Release 工作流中点击 Run workflow，选择要发布的分支并输入版本号（如 `0.1.4` 或 `v0.1.4`），无需提前打标签。流程会同步 npm、Cargo、Tauri 及锁文件中的应用版本，测试通过后自动推送 `v<版本>` 标签。源分支已更新到目标版本时，标签直接指向该提交。随后创建草稿，全部桌面安装包、签名的 Android APK 和两个架构的 Docker 镜像通过构建并上传后才公开发布。存在 `docs/releases/v<版本>.md` 时使用该发布说明，否则自动生成说明。推送代码或标签不会自动触发。失败时可重跑失败作业；重新手动运行仅允许复用源码完全一致的标签和未公开的草稿，已发布版本及不同源码的同名标签不会被覆盖。

首次运行前，在仓库的 **Settings → Secrets and variables → Actions** 配置 `ANDROID_KEYSTORE_BASE64`（Base64 编码的发布密钥库）、`ANDROID_KEYSTORE_PASSWORD`（密钥库密码）、`ANDROID_KEY_ALIAS`（密钥别名）和 `ANDROID_KEY_PASSWORD`（密钥密码）。后续版本使用同一签名密钥才能覆盖安装。以前安装过的本地调试包如果签名不同，请先备份笔记，再迁移到发布版。

## 功能文档

- [本地笔记库的文件行为](file-library.md)
- [从文件管理器打开笔记](external-files.md)
- [创建系统日程](calendar.md)
- [笔记库同步](s3-sync.md)
- [API 服务与 Web 部署](api-service.md)
