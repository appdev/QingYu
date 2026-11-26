# 轻羽 QingYu

**简体中文** · [繁體中文](docs/readme/README.zh-TW.md) · [English](docs/readme/README.en.md) · [日本語](docs/readme/README.ja.md) · [한국어](docs/readme/README.ko.md) · [Français](docs/readme/README.fr.md) · [Deutsch](docs/readme/README.de.md) · [Español](docs/readme/README.es.md) · [Português do Brasil](docs/readme/README.pt-BR.md) · [Italiano](docs/readme/README.it.md) · [Русский](docs/readme/README.ru.md)

轻羽是一款独立的本地优先 Markdown 笔记应用。打开一个文件夹就能开始写，每篇笔记都保存为普通的 `.md` 文件，也能用其他编辑器继续修改。

[下载轻羽](https://github.com/appdev/QingYu/releases/latest)

![Mac 和 iPhone 上打开的同一篇旅行笔记，包含行程表格和待办清单](docs/images/hero.webp)

## 写作与整理

- 直接编辑标题、表格和待办，输入 `/` 就能插入内容。公式、Mermaid 流程图和 Markmap 思维导图可以直接在正文中查看。
- 在侧栏按文件夹整理笔记，用全文搜索找回正文里的一句话。今日笔记适合记日程，版本历史可以找回删掉的段落。
- 手机键盘上方的工具栏可以插入附件或调整格式。分享时可把整篇笔记或选中的一段排成图片，复制或保存。
- 在「设置 → 通用 → 语言」选择 11 种界面语言之一，也可以跟随系统。外观、正文字体和字号可单独调整。
- 在 Mac 菜单栏可以随手记一条笔记。需要让 AI 工具读写笔记库时，可在设置中启用 MCP 服务。

## 笔记与同步

本地写作无需账号。需要跨设备时，可选择轻羽云或自己的 S3 兼容存储。原生应用会在上传前加密笔记和附件；两台设备改到同一处时，会保留冲突副本。

请保管好同步密码。如果忘记密码，又失去了所有已解锁设备，就无法解密云端数据。 [同步说明](docs/s3-sync.md)

## 安装与自托管

| 平台 | 安装包 |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [从源码构建](docs/development.md) |
| 自托管 Web | Docker · Linux amd64 / arm64 |

macOS 安装包未经 Apple 公证，首次打开时可能需要在「系统设置 → 隐私与安全性」中允许打开。Windows 安装包未签名，可能出现 SmartScreen 提示。

网页版读写服务器挂载的笔记目录。部署时设置访问密码，配置步骤见[Web 部署指南](docs/api-service.md)。

## 本地开发

安装 Node.js、Rust 1.96.0 和 Tauri 2 的系统依赖后，在仓库根目录运行。

```sh
npm ci
npm run tauri dev
```

[Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/) · [开发与发布说明](docs/development.md)

## 许可证

[GPL-3.0](LICENSE) · 第三方组件保留各自的许可证。 [MiaoYan](public/miaoyan/LICENSE) · [Markra](src/vendor/markra/LICENSE) · [MDSync](src/vendor/mdsync/LICENSE)
