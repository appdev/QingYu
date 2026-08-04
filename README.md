<p align="center">
  <img src="logo.png" width="96" alt="轻语 logo" />
</p>

<p align="center">
  <strong>明窗净几，字字轻语。</strong>
  <br />
  <strong>完全开源，免费使用。笔记始终可迁移。</strong>
</p>

<p align="center">
  <a href="README.en.md">English</a> | 简体中文 | <a href="https://editor.markra.app/">Web 版</a> | <a href="#下载">下载</a> | <a href="#文档">文档</a> | <a href="#核心特性">核心特性</a> | <a href="#参与贡献">参与贡献</a> | <a href="#许可证">许可证</a>
</p>

<p align="center">
  <img alt="Desktop" src="https://img.shields.io/badge/Desktop-Tauri-24C8DB" />
  <img alt="Web" src="https://img.shields.io/badge/Web-Notes-2563EB" />
  <img alt="WYSIWYG Markdown" src="https://img.shields.io/badge/Markdown-WYSIWYG-000000" />
  <img alt="Free" src="https://img.shields.io/badge/Free-Open_Source-16A34A" />
  <img alt="下载量" src="https://img.shields.io/github/downloads/appdev/QingYu/total?label=%E4%B8%8B%E8%BD%BD%E9%87%8F&amp;color=0EA5E9&amp;cacheSeconds=3600" />
  <img alt="License" src="https://img.shields.io/badge/License-AGPL--3.0-important" />
</p>

轻语是一个面向简单、实用记录的开源 Markdown 笔记应用。你可以在整洁的文档视图中书写，桌面端和浏览器文件版也可以切换到源码模式；笔记始终是普通的 `.md` 文件，桌面端和浏览器文件句柄由你决定位置，移动端与 Server Web 则使用各自固定的托管工作区。

桌面端、移动端和 Server Web 都由 Kernel 负责文档、设置、历史、资源和同步。公网浏览器文件版无需账号，专注浏览器文件句柄；Server Web/Docker 是新的单用户笔记服务，需要初始化 token 和所有者密码，浏览器只是连接同一个 `/data` 工作区的客户端。

## 产品宣言

> 我们并不需要另一个“第二大脑”，<br />
> 我们只需要一个能安心写字的地方。<br />
> 剥离复杂的块与双链，回归最纯粹的行云流水。<br />
> 数据归于你的 S3，灵感归于你的内心。<br />
> 在这里，只有你与文字的轻语。

## 下载

使用浏览器文件版：[editor.markra.app](https://editor.markra.app/)。

自托管 Server Web/Docker 版请从 [Docker 单用户部署说明](deploy/docker/README.md) 开始。它运行同一个 Web 应用和 Kernel API，固定使用 `/data/workspace`、`/data/config` 和 `/data/state`，适合把轻语作为单用户网页笔记服务部署。

macOS 用户可以通过 Homebrew 安装：

```sh
brew install --cask markrahq/tap/markra
```

也可以从 [GitHub Releases](https://github.com/appdev/QingYu/releases/latest) 下载 macOS、Windows 和 Linux 桌面版；Linux 还提供 AppImage、DEB、RPM 和 Arch Linux x64 安装包。

在 Arch Linux 上，下载发布页中的 x64 安装包后运行：

```sh
sudo pacman -U ./QingYu_<version>_linux_x64.pkg.tar.zst
```

## 文档

- 用户与能力：
  - [隐私与数据流](docs/privacy.md)
  - [轻语 MCP 配置与安全说明](docs/qingyu-mcp.md)
  - [主题制作教程与能力边界](docs/theme-authoring.md)
  - [默认臻楷主题设计与完整案例](docs/default-theme-zhenkai.md)
- 开发与发布：
  - [贡献指南](CONTRIBUTING.md)
  - [Docker 单用户部署说明](deploy/docker/README.md)
  - [Kernel 运行时迁移状态](docs/kernel-runtime-migration-status.md)
  - [更新日志](CHANGELOG.md)

## 运行时能力矩阵

| 能力 | 桌面版 | 移动端 | 浏览器文件版 | Server Web/Docker |
| --- | --- | --- | --- | --- |
| 产品形态 | 本地优先桌面笔记应用 | 原生移动笔记应用 | 浏览器里的轻量文件笔记入口 | 单用户自托管网页笔记服务 |
| 编辑界面 | 完整桌面布局、所见即所得、源码/分屏、标签页和分栏 | 紧凑全屏编辑器、移动格式工具栏、系统返回和键盘安全区处理 | 浏览器内编辑器和响应式界面 | 浏览器访问的完整笔记工作区 |
| 工作区模型 | 选择或切换本机笔记目录，也可打开独立 Markdown 文件 | 固定的应用托管工作区；不提供本机目录选择、独立文件窗口或远端笔记目录 catalog | 浏览器文件选择、文件夹选择和文件句柄 | 一个部署对应一个用户，固定 `/data/workspace`，不暴露目录选择或切换 |
| 权限与登录 | 本机应用权限 | 移动应用沙盒权限 | 无轻语账号，依赖浏览器授权文件句柄 | 初始化 token 设置所有者密码；之后通过同源 Cookie 会话登录 |
| 文件树操作 | 新建、重命名、移动、删除、排序、定位和多选 | 新建、重命名、移动、删除、搜索和全屏文件浏览 | 在浏览器权限允许时新建、重命名、移动和删除 | Kernel 管理 `/data/workspace` 中的 Markdown 文件、历史和搜索 |
| 自动保存和恢复 | Kernel AppConfig 恢复文件、标签页、草稿和工作区窗口 | 内嵌 Kernel AppConfig 恢复托管工作区、当前文档和草稿；支持移动生命周期刷新 | 支持浏览器文件句柄和 IndexedDB 状态时可用 | Kernel AppConfig 写入 `/data/config/settings.json`，刷新或换浏览器仍恢复同一状态 |
| 图片与附件 | 图片进入笔记目录或文档旁的 `assets/`；可打开本地附件和所在文件夹 | 通过系统图片选择器导入图片到托管工作区 `assets/`；不打开非图片本地附件 | 在权限允许时使用浏览器句柄和本地引用 | 图片和资源由 Kernel 从 `/data/workspace` 提供；浏览器不是文件权威 |
| 笔记同步 | WebDAV、S3 兼容同步所选笔记目录，并可从云端恢复具名目录 | WebDAV、S3 兼容同步固定托管工作区；无本机目录选择和远端目录切换 | Web 运行时不可用 | WebDAV、S3 兼容同步固定 `/data/workspace` |
| 导出 | HTML、PDF、带附件的可移植 Markdown，以及配置 Pandoc 后的更多格式 | 当前移动运行时不提供导出或 Pandoc | HTML 下载和浏览器打印/PDF | 浏览器侧导出能力可用；不提供 Pandoc |
| 桌面专属集成 | MCP、本机设置窗口、菜单/快捷键、更新器、系统字体和 Pandoc | 不提供 | 不提供 | 不提供；服务端通过 Docker/Kernel 配置运维 |

## 核心特性

### Markdown 笔记

- 桌面版和浏览器文件版可以在所见即所得和源码模式之间切换；移动端和 Server Web 提供面向笔记工作区的文档视图。底层文件始终保持 Markdown 格式。
- 在文档中直接渲染链接、图片、HTML、KaTeX 公式、Mermaid 图和 GFM 表格。
- 支持斜杠命令、块拖拽、可视化表格操作、提示块和带语法高亮的代码块。
- 可调整书写宽度、字号、行高、主题和快捷键。

### 桌面端文件与工作区

- 选择或切换唯一的当前笔记目录；在文件树中完成新建、重命名、移动、删除、排序、定位和多选。
- 独立 Markdown 文件可以作为不同步的编辑文档打开。再次选择目录会切换当前笔记目录，不再创建临时外部目录会话。
- 使用多标签、左右分栏、快速打开、工作区搜索、大纲导航、双链补全、独立设置窗口和桌面菜单。
- 自动保存已有文件，恢复标签页和工作区状态，并查看全文或选中文本字数。
- 在文档具有本地保存位置时，将粘贴、拖入、导入或下载的图片放进普通的 `assets/` 文件夹。

### 移动端工作区

- Android 和 iOS 使用内嵌 Kernel 和固定的应用托管工作区；不暴露本机目录选择、独立文件窗口、桌面 MCP、系统字体设置、Pandoc 或桌面更新器。
- 使用紧凑全屏的编辑、文件、设置和同步页面；系统返回键、页面隐藏、前后台切换和键盘安全区由移动运行时处理。
- 可以在托管工作区中创建、重命名、移动、删除和搜索 Markdown 文件，并通过历史记录恢复旧版本。
- 从系统图片选择器导入图片到托管工作区的 `assets/`，图片随普通笔记同步；非图片本地附件不会被移动端直接打开。

### Web 与自托管服务

- 浏览器文件版保留轻量入口：不需要轻语账号，依赖浏览器文件/文件夹授权和 IndexedDB 状态，适合直接编辑本机可授权的 Markdown 文件。
- Server Web/Docker 是重构后的网页笔记服务：一个部署服务一个用户，首次使用初始化 token 设置所有者密码，之后浏览器通过同源 Cookie 会话连接 Kernel。
- Server Web 不使用浏览器存储作为权威状态。设置、打开文件、草稿、布局和同步配置由 Kernel 发布到 `/data/config`，刷新页面、更换浏览器或替换容器都会读取同一个持久卷。
- Server Web 没有本机目录选择、独立文件窗口、MCP、系统字体或 Pandoc；它固定管理 `/data/workspace`，并通过同源 HTTP/WebSocket 与 Kernel 通信。

### 同步与导出

- 桌面端可以启用一份应用级 WebDAV 或 S3 兼容同步配置。它只同步当前笔记目录到 `notes/<目录名>/`；切换笔记目录时保留同一份配置，只改变云端目录名。
- 移动端使用同一套 Kernel 同步引擎，但同步目标始终是固定托管工作区；不提供本机工作区选择、远端根目录选择或远端笔记目录 catalog。
- Server Web 使用同一套 Kernel 同步引擎，但同步目标始终是 `/data/workspace`；浏览器文件版不提供轻语同步。
- 打开桌面独立 Markdown 文件不会改变当前笔记目录或同步目标。桌面新设备从云端恢复时只列出目录名，并且只下载用户选择的一个笔记目录。
- 桌面端可导出为独立 HTML、PDF 或带附件的可移植 Markdown；配置 Pandoc 后还可使用更多格式。移动端当前不提供导出或 Pandoc，Server Web 不提供 Pandoc。

同步设置和凭据保存在应用数据目录，不会写进笔记目录。凭据会在本机以明文保存，但不会参与轻语同步；主题、布局等可迁移偏好可以独立随笔记同步，设备路径、同步状态和 MCP 运行数据始终留在本机。

## 产品原则

- **简单** — 打开笔记即可记录，不需要额外设置。
- **实用** — 文件操作、搜索、历史、同步和运行时可用的导出服务于日常记录。
- **本地优先** — 除非你主动为当前桌面目录、移动托管工作区或 Server Web 持久卷启用同步，否则笔记就是本地 Markdown 文件。
- **始终可迁移** — 不依赖专有文档格式或托管工作区。

## 精选 Slogan

### 文人 / 审美：极简纯粹

- “明窗净几，字字轻语。”
- “把复杂的格式，留给即时渲染的诗意。”

### 极客 / 反卷：无负担、数据自主

- “不建大脑，不打补丁。今天，只记三两文章。”
- “你的笔记，本该躺在你自己的存储桶（S3）里。”

### 多端跨平台：PC 雕琢，手机随笔

- “案头挥毫，掌中轻语。”

## 开始使用

1. 打开 [Web 版](https://editor.markra.app/)、[下载](https://github.com/appdev/QingYu/releases/latest)桌面版，或按 [Docker 文档](deploy/docker/README.md)部署 Server Web。
2. 桌面端选择笔记目录、从已配置的云端恢复一个具名目录，或暂缓设置并打开独立 Markdown 文件；移动端直接进入应用托管工作区；Server Web 完成初始化后进入 `/data/workspace`。
3. 在文档视图中记录；桌面端和浏览器文件版需要时可切换到源码模式。
4. 保存、导出，或按运行时支持的范围同步由你管理的笔记工作区。

## 参与贡献

欢迎围绕 Markdown 编辑、文件可靠性、跨平台体验、同步、导出、主题、MCP、测试和文档改进提交贡献。开发前请阅读 [贡献指南](CONTRIBUTING.md)，其中包含 pnpm workspace 命令、测试边界和发布说明。

## 许可证

轻语使用 AGPL-3.0 许可证。
