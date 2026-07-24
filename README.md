<p align="center">
  <img src="logo.png" width="96" alt="轻语 logo" />
</p>

<p align="center">
  <strong>明窗净几，字字轻语。</strong>
  <br />
  <strong>完全开源，免费使用。笔记始终可迁移。</strong>
</p>

<p align="center">
  <a href="README.en.md">English</a> | 简体中文 | <a href="https://editor.markra.app/">Web 编辑器</a> | <a href="#下载">下载</a> | <a href="#文档">文档</a> | <a href="#核心特性">核心特性</a> | <a href="#参与贡献">参与贡献</a> | <a href="#许可证">许可证</a>
</p>

<p align="center">
  <img alt="Desktop" src="https://img.shields.io/badge/Desktop-Tauri-24C8DB" />
  <img alt="Web 编辑器" src="https://img.shields.io/badge/Web-Editor-2563EB" />
  <img alt="WYSIWYG Markdown" src="https://img.shields.io/badge/Markdown-WYSIWYG-000000" />
  <img alt="Free" src="https://img.shields.io/badge/Free-Open_Source-16A34A" />
  <img alt="下载量" src="https://img.shields.io/github/downloads/appdev/QingYu/total?label=%E4%B8%8B%E8%BD%BD%E9%87%8F&amp;color=0EA5E9&amp;cacheSeconds=3600" />
  <img alt="License" src="https://img.shields.io/badge/License-AGPL--3.0-important" />
</p>

轻语是一个面向简单、实用记录的开源 Markdown 编辑器。你可以在整洁的文档视图中书写，也可以切换到源码模式；笔记始终是普通的 `.md` 文件，文件放在哪里由你决定。

无需注册账号。文件默认留在本地磁盘。桌面端和移动端还可以通过 WebDAV、S3 兼容存储同步当前笔记目录。

## 产品宣言

> 我们并不需要另一个“第二大脑”，<br />
> 我们只需要一个能安心写字的地方。<br />
> 剥离复杂的块与双链，回归最纯粹的行云流水。<br />
> 数据归于你的 S3，灵感归于你的内心。<br />
> 在这里，只有你与文字的轻语。

## 下载

使用 Web 版编辑器：[editor.markra.app](https://editor.markra.app/)。

macOS 用户可以通过 Homebrew 安装：

```sh
brew install --cask markrahq/tap/markra
```

也可以从 [GitHub Releases](https://github.com/appdev/QingYu/releases/latest) 下载 macOS、Windows 和 Linux 桌面版。

## 文档

- [轻语 MCP 配置与安全说明](docs/qingyu-mcp.md)
- [更新日志](CHANGELOG.md)
- [隐私与数据流](docs/privacy.md)
- [贡献指南](CONTRIBUTING.md)

## 桌面版与 Web 版

| 能力 | 桌面版 | Web 版 |
| --- | --- | --- |
| 所见即所得和源码编辑 | 完整编辑体验 | 完整编辑体验 |
| 笔记目录与文件访问 | 选择或切换当前笔记目录，并可打开独立 Markdown 文件 | 浏览器文件选择、文件夹选择和文件句柄 |
| 文件树操作 | 新建、重命名、移动、删除、排序、定位和多选 | 在浏览器权限允许时新建、重命名、移动和删除 |
| 自动保存和状态恢复 | 已有文件、标签页、草稿和工作区窗口 | 支持浏览器文件句柄和 IndexedDB 状态时可用 |
| 资源处理 | 当前笔记目录根部的 `assets/`、文档旁的 `assets/`，以及已有本地引用 | 在权限允许时使用浏览器句柄和本地引用 |
| 笔记同步 | 当前笔记目录可选的 WebDAV、S3 兼容双向同步 | Web 运行时不可用 |
| 导出 | HTML、PDF，以及配置 Pandoc 后的更多格式 | HTML 下载和浏览器打印/PDF |

## 核心特性

### Markdown 编辑

- 在所见即所得和源码模式之间切换，底层文件始终保持 Markdown 格式。
- 在文档中直接渲染链接、图片、HTML、KaTeX 公式、Mermaid 图和 GFM 表格。
- 支持斜杠命令、块拖拽、可视化表格操作、提示块和带语法高亮的代码块。
- 可调整书写宽度、字号、行高、主题和快捷键。

### 本地文件与文件夹

- 选择或切换唯一的当前笔记目录；在文件树中完成新建、重命名、移动、删除、排序、定位和多选。
- 独立 Markdown 文件可以作为不同步的编辑文档打开。再次选择目录会切换当前笔记目录，不再创建临时外部目录会话。
- 使用多标签、左右分栏、快速打开、工作区搜索、大纲导航和双链补全。
- 自动保存已有文件，恢复标签页和工作区状态，并查看全文或选中文本字数。
- 在文档具有本地保存位置时，将粘贴、拖入、导入或下载的图片放进普通的 `assets/` 文件夹。

### 同步与导出

- 可以启用一份应用级 WebDAV 或 S3 兼容同步配置。它只同步当前笔记目录到 `notes/<目录名>/`；切换笔记目录时保留同一份配置，只改变云端目录名。
- 打开独立 Markdown 文件不会改变当前笔记目录或同步目标。新设备从云端恢复时只列出目录名，并且只下载用户选择的一个笔记目录。
- 导出为独立 HTML 或 PDF；配置 Pandoc 后还可使用更多格式。

同步设置和凭据保存在应用数据目录，不会写进笔记目录。凭据会在本机以明文保存，但不会参与轻语同步；主题、布局等可迁移偏好可以独立随笔记同步，设备路径、同步状态和 MCP 运行数据始终留在本机。

## 产品原则

- **简单** — 打开笔记即可记录，不需要额外设置。
- **实用** — 文件操作、搜索、历史、同步和导出服务于日常记录。
- **本地优先** — 除非你主动为当前笔记目录启用同步，否则笔记就是本地 Markdown 文件。
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

1. 打开 [Web 编辑器](https://editor.markra.app/)，或[下载](https://github.com/appdev/QingYu/releases/latest)桌面版。
2. 选择笔记目录、从已配置的云端恢复一个具名笔记目录，或暂缓设置并打开独立 Markdown 文件。
3. 在文档视图中记录，需要时切换到源码模式。
4. 保存、导出，或按需同步由你管理的当前笔记目录。

## 许可证

轻语使用 AGPL-3.0 许可证。
