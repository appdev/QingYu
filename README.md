# 轻羽 QingYu · 第一版

中文品牌为“轻羽”，英文为 `QingYu`，包与命令使用 `qingyu` / `qingyu-api`。侧边栏与登录页共用 `public/brand/qingyu-mark.svg`，应用 ICNS 使用用户提供的原文件。为继续访问已有笔记、设置与同步库，系统应用标识、原生 JNI 库名、旧存储键、`.miaoyan` 元数据和云端格式保持兼容；原版妙言的第三方资源与版权说明保留。

Docker 中设置 `QINGYU_WEB_PASSWORD` 即可启用只有访问密码的 Web 登录，不需要账号；Web 与 API 由同一个服务提供，配置、HTTPS 代理要求和浏览器本地存储边界见 [部署说明](docs/api-service.md#web-登录与-docker-密码)。

Tauri 2 + CodeMirror 6 Markdown 编辑器，提供本地笔记库、文件夹、编辑和预览；桌面端另提供直接编辑排版内容的所见即所得模式。

## 运行

```sh
npm install
npm run dev            # 浏览器开发，http://localhost:1420
npm run tauri dev      # 原生桌面窗口
npm test
npm run build
npm run tauri build
```

Android 工程已生成，保留了键盘和系统栏 Insets 适配，直接使用：

```sh
npm run tauri -- android dev
npm run tauri -- android build --debug --target aarch64 --apk
```

无需重新执行 `android init`。构建需要 Android SDK、NDK、JDK，以及 Rust 的 `aarch64-linux-android` target；项目固定 Rust 1.96.0。
iOS 尚未生成工程或验证；后续在具备模拟器运行时或真机签名环境后使用 `npm run tauri -- ios init`。

## 版本发布

[GitHub Releases](https://github.com/appdev/QingYu/releases) 提供 macOS Apple Silicon/Intel DMG、Windows x64 安装程序和 Linux x64 DEB/AppImage。首版为 `v0.1.0`；安装及签名边界见[发布说明](docs/releases/v0.1.0.md)。

发布前保持 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的版本一致，并添加 `docs/releases/v<版本>.md`。提交并推送对应的 `v<版本>` 标签后，在 GitHub Actions 的 Release 工作流中选择该标签，点击 Run workflow 手动发布；推送代码或标签不会自动触发。流程先测试并创建草稿，全部桌面安装包构建、上传成功后才公开发布。失败时保留草稿，可修复构建环境后重跑；源码修复使用新版本标签，避免移动已发布标签。

## 第一版边界

独立部署的笔记 API 服务支持目录、文档列表、搜索、读取、新建和带版本检查的更新，采用 Bearer Token 鉴权。可脱离桌面运行时运行，启动、Docker 和接口说明见 [API 服务](docs/api-service.md)。

- 文件夹、笔记列表、编辑区三栏布局；支持新建文件夹、新建笔记、全文搜索、按修改年份分组、正文摘要、所属文件夹和数量。
- Markdown 原文编辑，保留语法标记；编辑、预览、可调宽度的双栏分屏。
- 格式工具栏、快捷键、列表续写、缩进、撤销/重做、搜索。
- 表格、任务列表（预览可回写原文）、脚注、数学公式、Mermaid、Markmap 和 PlantUML。
- 笔记正文保存为真实 `.md/.markdown/.txt` 文件，文件夹对应磁盘目录。新安装的桌面默认位置为 `~/Documents/QingYu`；已有默认目录 `~/Documents/MiaoYan Editor` 会继续使用，可从“文件 → 打开笔记文件夹”切换；支持导入文件和递归导入文件夹。创建、数字/Copy 重名处理、名称字符转换和拒绝重名覆盖均对照妙言源码；空白新笔记以 UUID 创建，标题栏显示“无标题”。
- 初次启动自动迁移原 localStorage 笔记，保留原键并写入 `.miaoyan/localStorage-backup.json` 备份。隐藏索引只存路径、ID、置顶、选择和同步元数据；正文以磁盘文件为准。切换及退出前等待保存，失败不丢弃编辑；外部修改、删除和只读/损坏文件会阻止旧缓冲覆盖。桌面自动监听外部创建、修改、重命名和删除；当前编辑版本发生冲突时保存到隐藏的 `.miaoyan-conflicts` 后载入磁盘内容，旧撤销记录不跨外部更新。也可从“文件 → 重新读取笔记文件夹”手动载入。
- 手机窄屏使用编辑/预览切换。键盘打开后显示快捷格式栏。
- 原生端工具栏提供“创建日程”：标题默认取当前笔记名称，可填写起止时间、备注，并设置不提醒、开始时提醒、预设或自定义提前分钟数。macOS、iOS 使用 EventKit，Android 使用系统日历 Provider；保存时申请日历权限，默认写入系统默认日历（Android 优先可写主日历）。Windows/Linux 打开包含提醒的 `.ics` 文件，由默认日历软件确认导入。Web 不显示此功能，详情及平台验证边界见[日程说明](docs/calendar.md)。
- 桌面工具栏的 **Aa** 按钮进入“所见即所得”，可以直接编辑标题、正文、列表、任务框和表格，支持公式、脚注、撤销及格式快捷键；再次点击回到源码编辑。
- 笔记列表右键先选中目标，桌面使用系统原生菜单；重命名和移动直接操作文件，重命名不改正文或扩展名。`Trash/` 废纸篓保留正文与原文件夹，支持恢复，不提供永久删除。Markdown 导出用于额外保存副本。
- S3 在打开应用、每 3 分钟和桌面退出前同步，支持手动立即同步；笔记名称、置顶、废纸篓状态及根目录内的图片和附件一并同步，二进制冲突保留双方。移动端进入后台时尝试同步，系统挂起或强制结束进程无法保证执行。当前附件上限为单文件 64 MiB、总快照 256 MiB，详见 [同步说明](docs/s3-sync.md)。PlantUML 通过原版的在线服务绘图，图表源码发送到 `https://www.plantuml.com`；网络失败时保留源码显示。其他图表使用本地资源。

## 与妙言对照

参考仓库：<https://github.com/tw93/MiaoYan>，版本 `9ce84b01bd5f6b23a7156e317b2a34a464e3df76`。

直接复用 `Resources/DownView.bundle` 的原始 CSS、预览处理脚本、KaTeX 字体和图表资源；四个工具栏 SVG 来自 `Resources/Images.xcassets`，未修改其路径数据。
`public/miaoyan/LICENSE` 保留妙言 MIT 许可证，第三方预览库清单及许可证标记见 `public/miaoyan/js/vendor/MANIFEST.json`。

2026-11-10 按官方 V4.8.0 原生窗口校准：正文使用 16px 系统字体、26.8px 行高与硬换行最小高度、0.02em 字距、31px 左右留白；标题 20px/600；工具栏图标 20px、间隔 8px、右侧留白 24px。修正任务标记、表头、语法标记和代码配色，移除额外代码底色、删除线和常驻状态栏，保存失败提示仍可见。预览补齐 CJK 强调语言属性。

宽窗口相同样本前十组文字的纵向边界与原版一致。窄窗口分屏另修正预览误用手机留白、顶部被同步滚动卷掉，以及引用块版本不一致；同宽样本的预览标题、引用、列表、表格和代码块纵向位置一致。WebKit 自动折行仍有小数行高取整差异，脚注结构也未全部对齐。详见[官方版对照报告](docs/macos-official-comparison.md)。

编辑引擎替换为 CodeMirror，HTML 解析器替换为 markdown-it；这不是妙言官方应用，也不宣称所有 Markdown 边界语法及原生 TextKit 排版逐像素相同。Windows/Android 的系统中文字体与 macOS 不同。

## 桌面所见即所得

采用 [Vditor 4.0.1](https://github.com/Vanessa219/vditor) 的 `wysiwyg` 模式，首次点击 Aa 才加载，界面沿用妙言的字体、纸色与排版参数。Lute、中文语言包、图标、KaTeX 和字体全部随包离线提供，许可证见 `public/vditor/`；原有 CSP 保持不变。

可视化编辑与源码、分屏、预览共用当前笔记及保存流程。仅打开、关闭可视化模式不会重新序列化原文；实际编辑后，表格空格、任务框 `[x]`/`[X]` 等 Markdown 书写形式可能被规范化。切换笔记会清空当前编辑器的撤销历史，防止撤销带入其他笔记的内容。普通代码和 Mermaid 使用预览相同的本地渲染库、主题与配色；点击显示块可展开源码编辑，离开后折回渲染内容。其他图表类型在此模式下继续显示代码。

本轮验证了无编辑切换保留原文、快速切换前提交、表格编辑和插入、任务回写、撤销、中文组合事件、格式快捷键、草稿保存、frontmatter/脚注/公式/提示块保留及 HTML 清理。macOS 原生窗口已显示标题、表格与公式；系统输入法候选栏尚未单独验收。手机测试保持暂停，现有 Android APK 没有重新构建或覆盖安装。

## 验证

`tests/editor.test.ts` 验证源码位置、frontmatter、公式保护、代码转义、表格、脚注、中文/emoji 选区和任务框回写。

2026-11-10 实际验证：

- `npm test`：20 项通过，包含两种模式共用的 CJK 强调规则、预览顶部留白、后续滚动插值、Mermaid 容器样式与深色彩色节点的文字对比度、过期图表结果丢弃、SVG 清理、代码显示缓存，以及可视化修改回写源码时的 Unicode/选区映射；TypeScript 检查、Vite 构建和 `git diff --check` 通过。
- 本轮核验的 47 个共享 CSS、字体和 JavaScript 文件与官方 V4.8.0 逐字节一致（引用块 CSS 已冻结到发布版），原始妙言仓库保持干净。
- 浏览器：提示块图标、公式、Mermaid、Markmap、HTML 清理、带 frontmatter 的任务回写、撤销、选区格式、列表续写及草稿保存通过。
- 桌面浏览器 5000 行中文文档：一次全文设置约 31ms，一次插入约 18ms；这些是本机采样，不代表移动端帧率保证。
- macOS arm64：release `.app` 构建并启动，原生 WKWebView 分屏预览、公式和流程图正常。
- vivo V2307A / Android 16 / WebView 138：debug APK 安装并启动；通过屏幕九宫格键盘输入、选词提交“你好”、草稿保存、键盘上方格式栏、预览切换自动收起键盘、公式和流程图检查。
- 真机远程自动化连接期间出现一次 WebView renderer SIGTRAP，重启后直接操作系统键盘及预览未再复现；原因未完全定位，保留为兼容性风险。

排版校准另验证了选区格式、草稿保存、配额不足时错误提示与恢复，以及 CJK、公式、流程图预览；重新构建并运行 macOS arm64 应用。

随后已重新构建 Android arm64 debug APK 并安装到 vivo V2307A（Android 16、WebView 138）。系统栏区域改用与正文相同的浅色/深色背景；原生容器统一处理系统栏、挖孔与键盘 Insets，再将已处理的尺寸清零传给 WebView，避免重复留白；外观配置变化时刷新系统栏图标和背景。处理方式参考 [Android 官方 WebView Insets 文档](https://developer.android.com/develop/ui/views/layout/webapps/understand-window-insets)。

本次真机验证范围为浅色、竖屏、手势导航：编辑与预览、键盘弹出/收起、文末光标、键盘上方格式栏均正常。WebView 原生边界由 `(0,133)-(1260,2737)` 缩至 `(0,133)-(1260,1731)`，键盘收起后恢复原边界，没有残留空白。截图与包摘要见 `artifacts/android-systembars/`。旋转请求未生效，横屏未验收；深色、三键导航及更新版本 WebView 未实测。

`artifacts/` 包含本机 macOS 应用、Android arm64 调试 APK 和效果截图。APK 包含调试符号，体积较大，未作为正式签名发行包；macOS 应用未公证。

未验证 Windows、Linux、iOS、其他 Android 机型和输入法。本地文件存储和导入先验证 macOS，手机测试继续暂停；尚无完整笔记库备份恢复界面。文件契约与对照来源见 [docs/file-library.md](docs/file-library.md)。
