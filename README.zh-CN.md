<p align="center">
<img alt="轻语" src="logo.png" width="128">
<br>
<strong>轻语</strong>
<br>
<em>轻语 · 明窗净几，字字轻语</em>
<br><br>
一方安静、清晰、由你掌控的书写空间。
</p>

<p align="center">
<a href="README.md">English</a>
| <b>中文</b>
| <a href="README.ja.md">日本語</a>
| <a href="README.tr.md">Türkçe</a>
</p>

> 轻语基于开源项目[思源笔记](https://github.com/siyuan-note/siyuan)二次开发，遵循 [AGPL-3.0](LICENSE) 开源协议。轻语不是思源笔记官方发行版；产品设计、功能取舍、发布与支持由轻语项目独立负责。

## 为什么是轻语

笔记不该成为另一项需要维护的负担。

轻语希望把界面、结构和工具退到恰当的位置，让注意力重新回到文字本身。你可以从一句话开始，慢慢连接想法、整理资料、沉淀长期内容；不必先设计一套复杂的方法，也不必把自己的知识交给某个账号体系。

这里没有喧闹的功能竞赛。轻语更在意书写是否顺手、内容是否清楚、资料是否仍然属于你。

## 核心体验

### 安静地写

以块为基础的编辑体验兼顾自由书写与清晰结构，支持 Markdown 所见即所得、列表大纲、数学公式、图表和大文档编辑。工具在需要时出现，其余时间让文字留在中央。

### 让想法重新相遇

块引用、双向链接、虚拟引用和全文搜索帮助内容自然发生联系。你无需预先建立完美分类，也能在日后的阅读和写作中重新发现过去的想法。

### 把资料放回语境

数据库表格、PDF 阅读与标注、网页剪藏、OCR、资源文件和多种导入导出方式，让资料不只是被收藏，而是能够进入文档、参与思考。

### 按自己的方式组织

文档树、标签、书签、模板、代码片段、主题、图标和插件共同提供可调整的工作空间。轻语给出稳定的底座，但不替你规定唯一的笔记方法。

### 在需要时走得更远

本地 API、内置 MCP Server、命令行工具和自托管访问为自动化与扩展保留入口。这些能力存在于产品之后，不会占据日常书写的中心。

## 数据与隐私

轻语默认把内容保存在你选择的本地工作空间中，并尽量让数据边界保持可理解、可迁移、可备份。

- 支持加密笔记本，为敏感内容提供独立保护。
- 支持本地数据仓库快照、历史记录与恢复。
- 支持 S3、WebDAV 和本地文件系统同步，由你选择并管理存储端。
- 不要求登录轻语云账号，也不依赖官方云同步才能使用核心功能。
- 当前代码审计未发现由轻语开发者运营的遥测后台；可选网络功能和第三方扩展可能向你选择的服务传输数据。
- 支持 Markdown、PDF、Word、HTML 等导出方式，避免内容被困在单一界面中。

隐私不是一句口号，而是产品在账号、网络、存储和功能取舍上的长期约束。

## 适合这样的你

- 希望长期写作，而不是不断折腾笔记方法。
- 需要整理研究资料、文献、项目记录或个人知识。
- 看重本地数据、开放格式、备份能力和迁移自由。
- 喜欢清晰结构，但不希望工具打断思考。
- 愿意根据自己的需要使用插件、自动化或自托管能力。

## 当前状态

轻语仍在持续开发中，产品能力、兼容边界和发布流程正在逐步稳定。正式分发渠道尚在准备，请不要把思源笔记的官方安装包、应用商店版本或云服务视为轻语的发行与服务。

当前仓库适合参与开发、审阅产品方向或自行构建。版本变化可查看 [CHANGELOG](CHANGELOG.md)。

## Docker

官方 Docker 镜像为 `apkdv/qingyu`。启动容器前，请先替换访问授权码：

```bash
docker run -d \
  --name qingyu \
  --restart unless-stopped \
  -p 9806:9806 \
  -v /absolute/path/to/qingyu-workspace:/qingyu/workspace \
  -e PUID=1000 \
  -e PGID=1000 \
  -e QINGYU_ACCESS_AUTH_CODE=change-this-password \
  apkdv/qingyu:latest \
  /opt/qingyu/QingYu-Kernel serve
```

使用 Docker Compose 或容器管理面板时，请填写下面这条完整命令。入口脚本会拒绝简写的 `serve` 命令和旧的内核路径：

```text
/opt/qingyu/QingYu-Kernel serve
```

```yaml
services:
  qingyu:
    image: apkdv/qingyu:latest
    container_name: qingyu
    restart: unless-stopped
    command: ["/opt/qingyu/QingYu-Kernel", "serve"]
    ports:
      - "9806:9806"
    environment:
      PUID: "1000"
      PGID: "1000"
      QINGYU_ACCESS_AUTH_CODE: "change-this-password"
    volumes:
      - /absolute/path/to/qingyu-workspace:/qingyu/workspace
```

请将 `PUID` 和 `PGID` 设置为宿主机工作空间目录所有者的用户与组 ID。轻语默认将持久化数据保存在 `/qingyu/workspace`，也可以通过 `QINGYU_WORKSPACE_PATH` 或 `--workspace` 参数修改。

## 开发者入口

轻语由 Go 内核与 TypeScript 前端组成，但 README 不再展开完整技术手册。需要深入时可从以下入口开始：

- [中文 API 文档](docs/API.zh-CN.md)
- [贡献指南](.github/CONTRIBUTING.zh-CN.md)
- [版本记录](CHANGELOG.md)
- [产品身份设计](docs/superpowers/specs/2026-08-10-qingyu-product-identity-design.md)
- [功能精简设计](docs/superpowers/specs/2026-08-10-feature-removal-design.md)
- macOS、Linux 与 Windows 构建入口位于 `scripts/`

请以 `kernel/go.mod`、`app/package.json` 和项目工作流配置中记录的工具版本为准。

## 基于思源笔记

轻语建立在思源笔记成熟的块编辑、数据格式与开源生态之上，并在此基础上重新塑造产品身份、功能边界和使用体验。

轻语保留必要的数据与插件兼容能力，但拥有独立的应用标识、配置目录、协议、端口、内核名称和产品取舍。轻语不是思源笔记官方发行版，也不代表思源笔记团队；轻语相关的问题、构建与支持应由轻语项目自行承担。

感谢思源笔记团队、Lute 等上游项目以及所有开源贡献者提供的长期积累。上游项目地址：[github.com/siyuan-note/siyuan](https://github.com/siyuan-note/siyuan)。

## 开源与致谢

轻语遵循 [GNU Affero General Public License v3.0](LICENSE) 开源。任何分发与修改都应继续遵守许可证要求，并保留原项目和贡献者的版权与归属信息。

另请阅读[修改版与品牌归属声明](NOTICE.md)、[隐私政策](docs/legal/privacy.zh-CN.md)和[用户协议](docs/legal/terms.zh-CN.md)。轻语官网为 [apkdv.com](https://apkdv.com/)，支持邮箱为 [lengyue@apkdv.com](mailto:lengyue@apkdv.com)。

愿每一次记录都更轻一点，每一段思考都更清楚一点。
