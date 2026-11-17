# Pinto 设计对齐

2026-11-13：用户确认原生设计审阅稿，授权修改产品 UI，暂不处理导入功能。

## 执行顺序

1. 工作台：栏宽、侧栏分组、列表标题、编辑器工具栏与正文排版。
2. 设置：独立窗口、手机 sheet、iPad 居中 sheet，以及外观选择与已有功能入口。
3. 菜单与弹层：复用现有行为，统一锚点、分组和视觉。
4. 定向测试、类型检查和浏览器视觉核对；记录尚未实现的原版专属状态。

串行执行：设置、工具栏与多端布局共享 DOM 和 CSS，不能安全并行改动。

## 约束

- 原生底稿是本地设计对照归档中的 `qingyu/index.html`。
- 保留原有未提交修改、笔记保存和同步逻辑。改动前文件备份及哈希在 `artifacts/pinto-ui-alignment/`。
- 不把截图作为产品界面，不伪造账户、AI、协作或历史数据。尚无后端的操作须明确不可用。
- 颜色归 `palette.json`，偏好归 `preferences.ts`，栏宽规则归 `sidebar-layout.ts`，设置导航归 `settings-view.ts`，原生窗口尺寸归 `src-tauri/src/settings.rs`。不另建并行状态体系。
- 此前偏差源于近似绘制与跨状态比较；这次的验收要求同平台、主题、尺寸和内容逐屏比较。未核对状态不能声明完全一致。

## 当前实现

- 工作台：桌面分栏默认 248/330、文件夹和日期分组、编辑器工具栏、格式菜单；手机文件夹页、列表与底部搜索工具区。
- 设置：账户、通用、外观、快捷键、AI、今日笔记、隐私、存储、帮助，以及个人信息、登录安全、加密子页。原有编辑器、字体和 S3 同步设置仍可访问。
- 外观：三种明暗模式、六套配色、正文字体；桌面设置 760×660、手机底部 sheet、宽屏平板居中 580×650。
- 分享页包括协作、发送副本、网页和图片。图片支持预览、复制、保存及 macOS 系统分享。更多菜单提供加密和版本历史页面。
- 模板：原项目 23 个模板封面、分类、搜索、示例与关联模板。资源保留 MIT 许可。
- 当前禁用 AI 连接、账户服务、加密、协作邀请、网页发布、历史恢复和模板创建。复制文本、编辑与同步继续使用现有实现。

## 验证记录

- 11 个定向测试文件通过 76 项；补充刷新后设置回退回归后，受影响的 4 个文件通过 37 项。覆盖集合共 77 项，无需重复未受影响用例。
- `npm run build`：通过，含 TypeScript 检查；最后一轮日志在 `artifacts/pinto-ui-alignment/build-final.log`。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。此后只修改前端 CSS、设置 history 清理及对应测试，Rust 检查证据仍适用。当前无 IDE `build_project` 能力，使用原生命令。
- `git diff --check`：通过。
- Chrome：桌面九个主要设置页面可见，模板库与示例、协作/网页禁用状态、更多菜单、历史不可用状态、设置关闭均已检查；最终捕获的 console error 列表为空。
- 手机模拟：393×852，外观页没有横向溢出；宽屏平板模拟：1032×1376，设置卡片位于 (226,363)，尺寸 580×650。
- 视觉检查修复了手机外观页背景优先级、模板搜索框样式误命中清单复选框，以及刷新后残留设置 history 导致弹窗无法关闭的问题。
- 完整文件哈希清单：`artifacts/pinto-ui-alignment/final-tree.json`。原有未提交修改保留；Pinto 参考仓库已恢复干净状态。没有提交 Git。

## 检查截图

截图为实际产品界面，不作为产品背景。Chrome 保留在 `http://localhost:1420/` 的外观设置页。

| 页面 | 截图 |
| --- | --- |
| 桌面工作台 | [工作台](../artifacts/pinto-ui-alignment/desktop-workspace-final.webp) |
| 桌面外观 | [外观](../artifacts/pinto-ui-alignment/desktop-appearance-final.webp) |
| 账户 / 通用 | [账户](../artifacts/pinto-ui-alignment/desktop-account-final.webp) · [通用](../artifacts/pinto-ui-alignment/desktop-general-final.webp) |
| AI / 今日笔记 | [AI](../artifacts/pinto-ui-alignment/desktop-ai-final.webp) · [今日笔记](../artifacts/pinto-ui-alignment/desktop-today-final.webp) |
| 隐私 / 存储 | [隐私](../artifacts/pinto-ui-alignment/desktop-privacy-final.webp) · [存储](../artifacts/pinto-ui-alignment/desktop-storage-final.webp) |
| 快捷键 / 帮助 | [快捷键](../artifacts/pinto-ui-alignment/desktop-shortcuts-final.webp) · [帮助](../artifacts/pinto-ui-alignment/desktop-help-final.webp) |
| 模板 | [模板库](../artifacts/pinto-ui-alignment/desktop-templates-final.webp) · [示例](../artifacts/pinto-ui-alignment/desktop-template-preview-final.webp) |
| 协作 | [协作](../artifacts/pinto-ui-alignment/desktop-collaboration-final.webp) |
| 手机 | [文件夹](../artifacts/pinto-ui-alignment/phone-folders-final.webp) · [外观](../artifacts/pinto-ui-alignment/phone-appearance-final.webp) |
| 平板 | [外观](../artifacts/pinto-ui-alignment/tablet-appearance-final.webp) |

## 验收边界

当前为可审阅实现，不能称作原版全部状态已逐像素一致。已完成下述 macOS 隔离验证包打包与运行检查，未替换正式安装，也未执行 iPhone/iPad 真机验收。手机和平板截图来自 Chrome 设备模拟，最后的主题卡片底色微调未重新生成手机/平板截图。

初轮已知差异包括浏览器控件与原生 SF Symbols/材质、正文大小选择器、模式缩略图细节，以及保留的轻羽专属设置。字号与缩略图已在下文的外观设置复测中更新。未接入服务只展示不可用状态，不伪造已登录、已加密、成员或历史数据；相应成功/错误/权限状态尚未覆盖。

重复偏差反思：问题位于项目的设计参照和视觉验证路径，属于本项目。最小修正是把“同平台、同尺寸、同主题、同内容逐屏比较”作为验收条件，记录截图与未覆盖项；不把构建通过当作视觉一致。该规则已写在本文件，不更改全局规则或持久记忆。Hallmark 的自由设计方向和营销页规则不覆盖用户要求的原版复刻；浏览器验证通过 CUA 完成，未另启自动渲染浏览器。

## macOS 客户端验证（2026-11-14）

运行独立标识 `app.miaoyan.editor.pintouiqa20261114` 的 `src-tauri/target/debug/bundle/macos/轻羽 UI 验证.app`，使用隔离测试库；未修改用户正式笔记库、账户或同步配置。验证包为本地 ad-hoc 签名的 debug bundle，不作为发布安装包。

本轮在客户端发现并修正：

- `src/sidebar.css`：macOS 侧栏工具区避让窗口控制按钮。
- `src/main.ts`：更多菜单向左对齐展开，避免从窗口右缘溢出。
- `src-tauri/src/note_ui.rs`：原生菜单识别英文条目并设置 SF Symbols，同时保留中文菜单映射。
- `src/note-menu.ts`：无目标文件夹时禁用 Move to；`tests/note-menu.test.ts` 补充无目标/有目标的回归用例。

验证证据：

- 菜单测试 3 项通过；`git diff --check` 通过。
- 完成检查后冻结文件清单 `artifacts/pinto-ui-alignment/macos/final-tree.json`，SHA-256 为 `1251a8fc435de36e40f4d28f76b48499e946d6bc2b3f1ea129734085e97b35b2`。此后仅更新本文档与验证产物，生产代码未变。
- `npm run build` 与 `npm run tauri -- build --debug --bundles app --config artifacts/pinto-ui-alignment/macos/tauri.verify.json` 通过。日志分别为同目录的 `frontend-final.log`、`build-final.log`。无可用 IDE build_project 工具，使用项目原生构建命令。
- 签名严格检查通过；运行前后可执行文件 SHA-256 均为 `c9a0f3d4c2b30ebd325560839bfa603d07d725e33af6359af9d30de166f8ffc9`；最终进程 PID 9693，路径确认为上述验证包。
- 原生主窗口 1120×780：侧栏折叠/展开恢复 248/330 宽度，更多菜单和右键菜单的图标、位置、禁用项正常；格式弹层打开和退出正常。
- 独立设置窗口 760×660：九个主设置页面，以及个人信息、登录安全、加密子页均已查看截图；返回父页和关闭设置正常。Light/Paper 在主窗口与设置窗口同步并在重启后保留。
- 分享的协作与发送副本页、版本历史不可用弹层、模板示例已在本轮初始原生包打开检查；之后的四处修正未改变这些页面。未接入操作保持禁用。
- 隔离笔记完成编辑、保存、重命名、正常退出和重新启动；磁盘 Markdown 与预期中文、英文和符号逐字一致，重命名不改变正文。

启动异常与边界：更新后的包读取 Downloads 内的测试库曾超时，线程采样停在 `std::fs::read_dir → __opendir2`；没有据此认定数据丢失或确定权限根因。原测试文件保留完整，将测试副本放到验证包自己的 Application Support 目录后正常加载，随后正常退出/重启也通过。没有修改系统权限；Downloads 路径的访问原因仍待单独排查。最终测试库为 `~/Library/Application Support/app.miaoyan.editor.pintouiqa20261114/ui-verification-notes`。部分设置页的 CUA AX 树未完整暴露正文，本轮以实际截图补核，未进行 VoiceOver 验收。

| 原生界面 | 截图 |
| --- | --- |
| 工作台 / 更多菜单 | [工作台](../artifacts/pinto-ui-alignment/macos/workspace-final.webp) · [更多菜单](../artifacts/pinto-ui-alignment/macos/more-menu-final.webp) |
| 右键 / 格式 | [右键菜单](../artifacts/pinto-ui-alignment/macos/context-menu-final.webp) · [格式弹层](../artifacts/pinto-ui-alignment/macos/format-final.webp) |
| 外观 / 账户 / 通用 | [外观](../artifacts/pinto-ui-alignment/macos/settings-appearance-final.webp) · [账户](../artifacts/pinto-ui-alignment/macos/settings-account-final.webp) · [通用](../artifacts/pinto-ui-alignment/macos/settings-general-final.webp) |
| AI / 今日笔记 / 快捷键 | [AI](../artifacts/pinto-ui-alignment/macos/settings-ai-final.webp) · [今日笔记](../artifacts/pinto-ui-alignment/macos/settings-today-final.webp) · [快捷键](../artifacts/pinto-ui-alignment/macos/settings-shortcuts-final.webp) |
| 隐私 / 存储 / 帮助 | [隐私](../artifacts/pinto-ui-alignment/macos/settings-privacy-final.webp) · [存储](../artifacts/pinto-ui-alignment/macos/settings-storage-final.webp) · [帮助](../artifacts/pinto-ui-alignment/macos/settings-help-final.webp) |
| 账户子页 / 加密 | [个人信息](../artifacts/pinto-ui-alignment/macos/settings-personal-final.webp) · [登录安全](../artifacts/pinto-ui-alignment/macos/settings-security-final.webp) · [加密](../artifacts/pinto-ui-alignment/macos/settings-encryption-final.webp) |

## Markra 编辑器验证（2026-11-15）

- macOS 隔离验证包 `轻羽 Pinto 验证.app` 使用标识 `app.miaoyan.editor.pintomarkraqa`。实测表格行列菜单、单元格中文输入、缺表头的 Markdown 片段粘贴、保存和重启。原有笔记库未参与测试。
- 附件菜单可搜索并插入笔记链接，保留选中的文字作为别名。macOS 实测点击打开、保存到 Markdown 和 `⌘[` / `⌘]` 前后导航。浏览器额外验证了表格单元格选中文字后的别名插入及链接跳转。
- macOS 实测右键复制外部链接，再粘贴可得到完整 URL。Pinto 的外部链接菜单还包含 AppKit 的「打开方式」、系统编辑与共享子菜单，当前验证包仅提供打开与复制链接。
- ABC 输入源下 `[[` 可用回车选中候选并保存。豆包输入法下能看到候选，但本轮方向键和回车未能选中，原输入源已恢复。手机和平板外观使用 Chrome 设备模拟核验。
- 最终代码树哈希 `f802e33d5285744cb461f87cd5b9273074ded263f45b713e0d4db3d4477b7096`，清单位于 `artifacts/pinto-markra/tree-checksums.txt`。`npm test` 通过 196 项、跳过 2 项。TypeScript、Vite、macOS debug 打包与严格签名检查通过，日志位于同一目录。

### 右键菜单与长文复测

- 任务圆点和任务文字都可触发「取消勾选此清单的所有任务」，格式菜单也提供该操作。macOS 实测当前组和嵌套任务一起取消勾选，空行后的独立清单保留勾选，一次 `⌘Z` 恢复。浏览器验证了格式菜单入口，自动测试覆盖了代码块内的字面量。
- 外部链接菜单包含剪切、复制、粘贴及 macOS「打开方式」。验证包读取系统注册的处理程序及默认项，实测从子菜单选 Safari 后打开指定的本机回环地址。机器上两个同名 Chrome for Testing 安装会出现同名条目。Pinto 的链接卡片切换、选区图片分享及 AppKit 系统服务菜单仍有差异。
- 本机输入源为豆包时，连续按 `[[`、下键、回车，确认选中第二项并插入笔记链接。逐步读取辅助功能树时曾丢失焦点，WebKit 事件记录未收到对应的下键。连续测试期间输入源处于英数状态，尚无用中文组字过滤候选的验收证据。
- 隔离库 375 节、154,030 字节的长文通过中文粘贴、保存、正常退出和重启校验。写入内容在界面和 Markdown 文件中逐字一致。
- `npm test` 通过 197 项、跳过 2 项，`cargo test --manifest-path src-tauri/Cargo.toml --lib note_ui::` 通过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查和 `git diff --check` 通过。冻结代码树哈希 `91016d12704863608690bf0c69c54bd670458a0e10c8b924acf3f0c9477ab11a`，清单和日志位于 `artifacts/pinto-markra`。

### 链接显示方式

- 对照 Pinto `LinkForms.swift` 和 `EmbedViews.swift`，独占一行的网页支持普通链接、紧凑显示和卡片，笔记链接支持普通链接和卡片，附件支持紧凑显示和卡片。紧凑行高 34、卡片高 72，显示方式保存在原 Markdown 中。行内链接和代码示例照常显示，图片沿用原图片组件。
- macOS 隔离样本完成网页普通链接 → 紧凑显示 → 卡片、笔记和附件转卡片。点击三类卡片分别打开默认浏览器里的回环地址、目标笔记和 TextEdit 中的原附件。重启后三类卡片均能恢复，磁盘上的目标 URL、笔记别名、附件路径和行内链接与预期一致。测试结束前点击正文末尾为隔离样本添加了一个空行。
- 浏览器确认分屏预览的卡片与所见即所得编辑器一致，笔记卡片展示目标笔记摘要和文件夹。自动测试覆盖网页显示方式往返、移除后撤销、笔记别名和附件路径、代码块和普通图像。截图为[浏览器中的卡片](../artifacts/pinto-markra/link-cards-final.webp)。
- 网页卡片目前显示域名和图标，附件卡片使用文件图标。网页标题、摘要、远程缩略图及文件首页预览仍待补齐。图片分享、系统服务菜单与原生移动端仍待进一步验收。
- `npm test` 通过 200 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查与 `git diff --check` 通过。冻结代码树哈希 `f5e92e902eaf6594723b615e5e4f4a562f9dd922143588f2fd7d9845c086469b`，清单和日志位于 `artifacts/pinto-markra`。

### 外观设置复测

- 桌面正文字号采用 Pinto 的 13、15、17、20、24 pt 五档，旧档位按参考规则向上归入最近一档，超上限归到 24 pt。原文和界面字号仍由各自设置控制。新选择的衬线及等宽字体使用系统字体系列，已有自定义字体仍可使用。
- 「跟随系统」缩略图在同一条中线裁切明暗画面，保留各配色的选中行、文字和侧栏色。主题卡片和字号滑杆依据参考调整。浏览器显示示例和正文同步生效，iPhone 模拟宽 390 无横向溢出，iPad 模拟外观窗 580×650 居中。
- macOS 独立设置窗口实测拖到 24 pt 后主窗口立即生效。验证「显示」菜单的放大、缩小和恢复默认，以及字号边界的禁用状态。`⌘=` 和 `⌘+` 各放大一档，`⌘−` 从 24 pt 回到 20 pt，`⌘0` 回到 13 pt。重启回读确认浅色、森林、衬线和 24 pt 已保存。
- 24 pt 验收发现笔记卡片的第三行裁切。调整卡片内单行文字排版，保持 72 pt 卡片和 34 pt 紧凑项。浏览器尺寸检测与最终 macOS 截图均确认三行内容完整。
- 前端全量测试通过 202 项、跳过 2 项，最后的卡片排版调整后重跑相关测试 17 项通过。TypeScript、Vite、macOS debug 打包、严格签名检查和 `git diff --check` 通过。冻结代码树哈希 `596a9d947d3ebc1bcd8e99a223a52e324de046b7c9117c4fcba037ebc6f11af0`，清单、测试日志和浏览器截图位于 `artifacts/pinto-markra`。

### 图片分享与选区导出

- 对照 Pinto 的 `NotePicture.swift` 和 `NotePictureSheet.swift`，工具栏图片页和选区右键共用 PNG 渲染。选区扩展到完整行，碰到表格和代码块时保留完整块。六款背景、浅色和深色、轻羽标记可在预览中调整。
- macOS 实测右键从中文选区生成完整一行，深色和标记开关即时反映在预览。文字和表格都通过原生 `NSSavePanel` 保存为 2400×1800 PNG。先选中正文再选中表格单元格时，实际导出包含完整表格，周围正文不进入图片。
- 整篇笔记的图片复制后可粘贴回轻羽并作为 2400×2465 的 PNG 附件落盘，图片保留中文、清单、表格和本地附件。系统分享菜单已实测打开。验收样本为[整篇笔记](../artifacts/pinto-markra/picture-native-clipboard.png)、[中文选区](../artifacts/pinto-markra/picture-native-selection.png)和[表格选区](../artifacts/pinto-markra/picture-native-table.png)。
- 图片导出使用本地笔记库中的附件，远程图片显示替代文字。第三方服务发送、移动端原生图片分享及复杂图表导出仍待验收。
- `npm test` 通过 208 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查和 `git diff --check` 通过。冻结代码树哈希 `bda7e98f7bb0fbc4056f07b60792994fe0e3348ac31a1cfb00b90dd06241b46f`，清单为 `artifacts/pinto-markra/tree-checksums-picture-macos.txt`。

### 表格键盘交互与桌面侧栏底部

- 对照 Pinto `TableGrid.swift` 及 `TableMenuTests.swift` 补齐光标落点。新增行进入新行首格，新增列进入新列表头。表尾 Tab 从新行首格继续，回车下移同列并在表尾新增行。行和列手柄均提供删除表格。删除表头时首个数据行成为新表头，保留对齐和相邻正文。
- macOS 在隔离笔记分别输入 `新行首格`、`新列表头`、`表尾Tab`，核对文字实际写入对应位置。两次连续 `⌘Z` 依次撤销刚输入的文字和新增行。原生菜单实测删表头、恢复表头、删除整表及恢复，最终测试笔记的文件哈希与删除及撤销前一致。原 375 节长文重新打开检查，文件哈希一致。
- 桌面补上模板与连接 AI 下方的分隔线，校准这些入口的行高及底部设置、同步按钮位置。macOS 实测模板库和 AI 设置页可从侧栏入口打开。输入法组字期间的结构保护由定向测试覆盖，移动端实机和中文组字与表格导航的组合仍待复测。
- `npm test` 通过 216 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `cf9cdafbe107cc60e41e1b28153185f0618d7fa68aa9b36573de8b7d851d6cb7`，清单为 `artifacts/pinto-markra/tree-checksums-grid-footer-final.txt`。

### 代码块排版与围栏编辑

- 对照 Pinto `MarkdownStyler.swift`、`DecoratedLayoutFragment.swift` 和 `NoteBlockLookTests.swift`，闲置时隐藏围栏，仅在第一行右侧显示语言。进入代码块显示两行围栏，语言可直接编辑。首尾各预留 20 pt，正文横向留 14 pt。新配置默认使用系统等宽，已保存的代码字体偏好继续生效。
- 保留右键「复制代码」，复制范围只含代码正文。macOS 从 375 节长文复制两行代码并粘贴到隔离笔记，核对完整内容。长文切换到编辑围栏时后续章节位置保持一致，长文 Markdown 哈希一致。另用隔离样本把 `js` 改成 `ts`，在末尾空行按回车退出，落盘文件保留两行代码且空行已移除。系统等宽设置和 Mermaid 预览、源码、预览往返均已原生验证。
- 中文组字尝试中，当前输入源在正文和表格都直接录入拼音字母，原输入源已恢复。真实候选提交与表格导航仍待验证。
- `npm test` 通过 219 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `9ef3872e5dd185393416316a31d2a45069d12a212b18108d5a914ce8d2115343`，清单为 `artifacts/pinto-markra/tree-checksums-code-fences.txt`。

### 表格方向键与输入法对照

- 对照 Pinto `TableInteractionTests.swift`，从正文按下键进入表头首格，按上键从下方进入末行首格。表格内上下键保持同列，穿过首尾回到正文。Escape 从下方退出并保留编辑。向下退出时保留表格终止空行，文档末尾按需补齐，避免接下来输入的文字成为 GFM 表格行。
- macOS 验证包从上下两侧进出表格，并通过实际中文粘贴核对表头、同列操作和相邻正文的落点。Escape 保留对应单元格内容。保存、重启及逐字反查 Markdown 通过，375 节长文源文件哈希一致。自动测试覆盖组字事件期间不接管方向键，以及在文档末尾按 Escape 后继续输入。
- 系统键盘页显示 ABC 和豆包输入法。豆包的中英文切换配置为单独按 Shift。当前验证环境中，独立的系统设置搜索框与轻羽同样接收到拼音字母。已清空搜索，保留原输入源。真实中文候选提交仍待验证。
- `npm test` 通过 222 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `2bc43d61086bdc4d5369761768d8acb6448fd25de360c7d2c73e8df27cabe609`，清单为 `artifacts/pinto-markra/tree-checksums-grid-arrows.txt`。

#### macOS 表格真实中文候选复测

- 在表格边缘修复的 macOS 隔离包中，用豆包拼音将候选「你好」「世界」「再见」提交到短表的相邻单元格。方向键、Shift+Tab 和 Tab 进入预期单元格。组字期间 Markdown 无新增拼音，提交后立即回车在表尾新增一行并进入同列。四次撤销恢复两格内容和行数。
- 在同一包的 375 节长文中，通过 Tab 连续向第一节表格的两格提交「你好」「测试」。磁盘对照只改变该数据行，两次撤销后文件 SHA-256 与基线一致。
- 当前集成树 `ff1d79aed1e723482bcb280c7a6a1f562139d84b993c1ec453265593e2ba5733` 已重新构建并签名。新包重开长文、表格中文粘贴、保存及 `⌘Z` 复测通过。输入源在该包的本轮尝试中直接提交英文字母，候选提交的证据来自上一段所述验证包。

### 表格行列手柄

- 对照 Pinto `TableMenuTests.swift`，点行列手柄会高亮整行或整列。原生菜单关闭后，焦点回到原单元格，选区保留。退格或前向 Delete 清空选区，行列结构和对齐保留。一次撤销恢复内容。行列菜单仍可删除相应结构，点击正文后高亮消失。
- macOS 实测退格清空第一数据行，前向 Delete 清空第二列及其表头，均可撤销。另实测通过菜单删除第二列，单元格数量从 6 变为 3，撤销后恢复为 6。最终落盘 Markdown 与测试原文逐字一致。
- `npm test` 通过 224 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `65bccde9739d9bde4fde9a3807f863d2b53cdba2e496b6c61f8329c2e403ee39`，清单为 `artifacts/pinto-markra/tree-checksums-grid-picks.txt`。移动端手柄命中区域与触屏菜单仍待核验。

### 粘贴缺表头的片段

- macOS 隔离客户端复现了带独立斜杠段落的快捷键表格粘贴。剪贴板内容含正文时，原逻辑跳过补表头，编辑器显示 `|` 和 `---` 原文。修正后识别 Markdown 中独立成段的表格片段并补空表头，保留周围文字、代码块和已有表头的表格。
- 在修复后的 macOS 验证包粘贴同样的斜杠和两行快捷键，在界面确认两列及两条数据，磁盘 Markdown 保留斜杠、空表头、两条数据和后续文字。自动测试覆盖 CRLF、代码块、撤销和混合粘贴。
- `npm test` 通过 229 项、跳过 2 项。TypeScript、Vite、macOS debug 打包和本地签名校验通过。冻结代码树哈希 `04b96b490d0099151e128f3335d664392d5490cd89eda91abf3ff5737494e24d`，清单为 `artifacts/pinto-markra/tree-checksums-table-paste.txt`。

### iPhone 与 iPad 表格触控

- 对照 Pinto `GridMetrics` 和 `GridHandles`，行列手柄保留 16×30 pt 外观，触控区域为 44×44 pt，重叠时选离触点最近的手柄。底部新增条高 16 pt、可点区域高 44 pt，列的插入从列菜单进入。移动端保持表格网格显示，并在 WebKit 把点击光标放进外层编辑区时恢复到所点单元格。
- iOS 26.5 的 iPhone 17 和 iPad Pro 13 英寸模拟器使用 `app.miaoyan.editor.gridtouchqa` 隔离包。iPhone 核对了手柄和底部新增条的可见区域外点击、行菜单新增、列高亮及关闭菜单后的光标。iPad 核对了行列菜单、整列高亮、底部新增条及新增行列后直接输入。两个沙盒中的 Markdown 均保留前后正文和测试值，iPhone 另验证重启回读。
- 普通模拟器包在启动时崩溃于 Tauri 的 WebKit 版本查询。隔离构建使用仅对 iOS Simulator 生效的 `tauri-runtime-wry` 临时补丁。产品依赖配置保持原样。iPad 有一次点按表格边缘空白后把后续输入写进了表头源码，测试输入已撤销。边缘光标、Pinto 原生 `UIMenu` 样式和真机仍待核验。
- `npm test` 通过 231 项、跳过 2 项。TypeScript、Vite、iOS Simulator debug archive 及 `git diff --check` 通过。冻结代码树哈希 `1672c654d1a2875f08d211f069909be0ba9a9428d161d69890828be3427e3a5d`，清单为 `artifacts/pinto-markra/tree-checksums-grid-touch-final.txt`。

### 表格周围的可编辑正文

- 参照 Pinto `GridHolder` 把未命中单元格和控制按钮的点击交给文本编辑器的行为。移动端点按表格周围的同一渲染行时，将光标放到相邻正文，方向取触点更接近的一侧。表格在文末或紧邻另一张表时，向下离开会按需留出可输入的一行。
- iPhone 17 模拟器实测点上方留白和下方留白后分别输入。iPad Pro 13 英寸实测下方、右侧留白输入及底部新增行。对照两个隔离沙盒的 Markdown，原表头、分隔线和数据行逐字一致，测试文本写入对应的正文或新增行。
- `npm test` 通过 236 项、跳过 2 项。TypeScript、Vite、iOS Simulator debug archive 与 `git diff --check` 通过。模拟器包沿用临时启动补丁。冻结代码树哈希 `f568dd33583edc1119398978f31f551bd37d5f92dc10a6b4549dcafdda99567e`，清单为 `artifacts/pinto-markra/tree-checksums-grid-touch-edges.txt`。

### iOS 表格手柄原生菜单

- 对照 Pinto `TableMenuPresenter` 和 `EditorController.tableMenu`，iOS 17.4 起手柄以 44 pt 触控矩形作为 UIKit 下拉菜单锚点，插入、行列删除、整表删除分组显示，删除操作为红色，不可用项目隐藏。菜单关闭后恢复单元格焦点，通过已有表格命令应用所选操作。低版本 iOS 沿用网页菜单。
- iOS 26.5 的 iPad Pro 模拟器实测行列菜单、取消后继续在原单元格输入、新增列进入表头及新增行进入首格。iPhone 17 模拟器实测列菜单、删除列和一次撤销。两个隔离笔记的落盘内容与预期逐字一致。iPhone 键盘切换时的行手柄触点仍待复核，真机未验收。
- `npm test` 通过 239 项、跳过 2 项。覆盖结构分组、不可用动作过滤、取消、选中、旧系统回退。TypeScript、Vite、iOS Simulator debug archive 和 `git diff --check` 通过，模拟器构建沿用临时启动补丁。macOS debug bundle 与本地签名检查通过，随后仅修正 iOS 条件编译命令。冻结代码树哈希 `9dd8fd182864b6e286707bd0f40ad33e48e4a691fc18969d7155f576ec7ccaba`，清单为 `artifacts/pinto-markra/tree-checksums-grid-native-menu.txt`。

### 移动端表格工具面板

- 对照 Pinto `TablePanel`，移动端在单元格内通过表格工具打开键盘位置的面板。行列分别提供三个网格图标按钮，整表删除独占一行。面板同时标出当前行和列，交叉格更深。关闭恢复原选区，新增行列沿用已有光标和撤销规则。
- iPhone 17 模拟器实测键盘快捷栏开面板、关闭恢复软件键盘、立即重开仍定位同一表格、从面板新增行并在首格输入。iPad Pro 13 英寸实测顶部工具栏开面板、交叉高亮、新增列后在表头输入。两个沙盒的最终 Markdown 与预期逐字一致。
- 面板经 WebView 绘制，已验证软件键盘让位与返回。Pinto 系统 `UIInputView` 的转场细节及真机仍待验收。`npm test` 通过 242 项、跳过 2 项。TypeScript、Vite、iOS Simulator debug archive 和 `git diff --check` 通过，模拟器构建沿用临时启动补丁。冻结代码树哈希 `33535530179bf181bb2381ab177ccac4e7e5b3308f30f40aa9df5136ddaccd6f`，清单为 `artifacts/pinto-markra/tree-checksums-grid-table-panel-final.txt`。

### 长文表格列宽与横向滚动

- 对照 Pinto `GridTable.columnWidths` 和 `GridOverflow.Fade`，短表保持自然宽度，达到可用宽度 70% 的表格延展。超宽表先收紧列内留白，再提供横向滚动和 28 pt 渐隐提示。宽度按绘制文字测量并计入等宽数字、列线及内边距。聚焦或编辑后重算列宽时维持当前格可见。
- macOS 实测短表、近整宽表、右侧渐隐、拖到末列后的左侧渐隐、末列输入保留位置。iPhone 17 模拟器实测 `Tab` 从 Minutes 进入最后一列时自动横移。重新打开 375 节长文并滚到第 11 节，核对表格和左下角模板、连接 AI、设置及同步入口。长文和两份宽表验收文件的最终哈希均与各自基线一致。
- 宽表末格重新聚焦后用 `Control+Z` 撤销并核对落盘。直接用 macOS `⌘Z` 的这条路径仍待修复或进一步定位。iPhone 触摸滑动和真机仍待验收。`npm test` 通过 244 项、跳过 2 项。TypeScript、Vite、macOS debug 包及本地签名、iOS Simulator debug archive 和 `git diff --check` 通过。冻结代码树哈希 `b49d0b7d27fb0597e35380b15af90d7d2d34a3e69cd2eb5871750fa05fb823e7`，清单为 `artifacts/pinto-markra/tree-checksums-grid-widths-final.txt`。

### macOS 表格边缘点击

- 对照 macOS 系统备忘录，点表格右侧留白保留表格并把光标显示在右边框，输入接到表格后的正文。点表格下方留白则显示普通的正文光标。
- 在隔离客户端用日期表复现原先的 Markdown 回退，再验证右侧、下方、单元格和左侧行手柄。粘贴中文后核对 Markdown 文件，`⌘Z` 撤销后文件 SHA-256 与基线一致。`npm test` 通过 254 项、跳过 2 项。TypeScript、Vite、macOS debug 打包和本地签名验证通过。冻结代码树哈希 `5817af97393fd021a1758ad92304dcdc9aaaa161fd28d7416f6de90e74937615`。
- 按系统备忘录的交互修正表格右侧和下方点击。先放置光标，首次输入才按需加入 Markdown 分隔。后续为代码块时，点击留白保持代码预览。macOS 隔离包用 375 节长文和无尾部换行的表格验证了点击前后文件哈希一致、中文粘贴为独立段落、一次撤销恢复两份原文。`npm test` 通过 258 项、跳过 2 项，TypeScript、Vite、macOS debug 包和严格签名验证通过。冻结代码树哈希 `99a67d7e4f9dd9da5cb165352a5b220493e67dc75d221dd06afb5d2101ec6fbf`。移动端本轮覆盖自动测试。
- 此后用 macOS 实际按数字键复现了 `2` 写进紧邻代码开头围栏的错误。最后一行表格源码在预览 DOM 中隐藏，WebKit 会借用下一行的原生插入点。无独立可编辑行时改为显示表格右边框，使用独立输入控件接收按键和组字，提交时在表格后建立正文段落。点击本身保持 Markdown 字节不变。
- 回归覆盖右侧、下方、代码块顶部、已有一行空白及文末。macOS 用实际按键验证 `2`、连续输入 `23`、退格、Delete、回车后输入、中文粘贴和撤销。375 节长文副本落盘时只新增独立段落 `2`，另一次完整操作实测一次 `⌘Z` 恢复，短样本和长文副本与原文逐字节一致。组字提交和取消已通过自动测试，本机输入法实测仍直接输入拉丁字母。移动端待实测。
- 这次漏测发生在项目的验证路径。表格边界回归需要在 macOS WebKit 里对着实际点击位置发送真实字符键并核对落盘差异。直接构造 CodeMirror 事务只能覆盖 Markdown 编辑逻辑，无法覆盖 WebKit 的插入点。

### macOS 保存后的撤销与重做

- 文件监听核对落盘内容后恢复原输入焦点和选区。若笔记已切换、原节点已移除或焦点已交给其他控件，则跳过恢复。宽表末格在内容已写入 Markdown 后可直接按 `⌘Z`，无需重新点击。
- 对照 Pinto 的笔记撤销栈，轻羽 macOS「编辑」菜单与快捷键共用当前 CodeMirror 编辑器的历史记录。菜单根据当前输入框或编辑区的历史启用，表格继续走已有的单元格焦点恢复。笔记搜索和独立设置窗口中的输入走 WebKit 自身的撤销记录。
- 隔离验证包实测宽表末列及 375 节长文落盘后，可通过点击菜单撤销、重做，再用快捷键撤销。另验证笔记搜索和设置搜索独立撤回。独立文档及长文均用真实输入法把 `nihao` 提交为「你好」，核对磁盘文件，再将修改撤回。三份验收文件恢复到各自原文。
- `npm test` 通过 247 项、跳过 2 项。TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `3a603699d870cb8c5a80a70a11b002a43dfe1d18f42f9967e651a6e53190f5de`，清单为 `artifacts/pinto-markra/tree-checksums-history-menu.txt`。

### 表格右键剪贴板

- 对照 Pinto `PaneTextView.tableCellMenu` 和 `EditorController.setCell`。直接右键剪切、复制整格，粘贴替换整格。WebKit 自动选中的词不算主动选区。用户拖选内容后仍可对选区进行剪切、复制、粘贴和分享为图片。macOS 通过系统剪贴板读写，空格禁用剪切与复制，表格操作菜单关闭 AppKit 附加项。
- macOS 隔离包实测从含中文和竖线的单元格复制并覆盖已有内容，再剪切原格并粘贴到空格。主动拖选「保留」后复测只复制这两个字。每次落盘核对相邻单元格及表格前后正文，按次撤销后验收文件 SHA-256 与基线一致。另用英文输入采样确认表格和正文的输入与撤销，未在本轮提交表格中的中文候选。
- 自动测试复现了 WebKit 右键自动选词并覆盖了整格剪贴板、换行合并、反斜杠和竖线、相邻单元格保留及逐次撤销。`npm test` 通过 248 项、跳过 2 项，TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `c201cc091f40e2915c17cc9d3bdbfad1960490e0c15e1b1cd58589ff62827211`。

### macOS 正文选区右键

- 用 WebKit 已选中的可见正文决定剪切与复制的可用状态，覆盖 CodeMirror 尚未同步选区时的菜单。正文菜单使用「编辑操作」标签，原生「分享为图片」与剪贴板项使用系统图标。
- macOS 隔离包验证了正文右键和图片预览。系统服务尝试从应用菜单复制到右键时，AppKit 将复制的条目禁用，因此移除了该尝试。最终包通过应用菜单的「服务」读取到「在词典中查询」。58 项定向测试、macOS debug 包和严格签名验证通过。冻结代码树哈希 `28fe238c691236ad1e92d4279f2979db1bfceede49bc92a0f50233c3a8f0b80a`。

### 长文标题折叠

- 对照 Pinto `HeadingFolds`，前三层标题可折叠，节内表格、子标题和代码块随之隐藏，靠近下一节的空行保留。macOS 悬停显示左侧箭头，收起后箭头和标题末尾的省略号均可展开。回车在折叠区之后插入一行，跳转或查找可展开命中的折叠章节。原始 Markdown 不受视图折叠影响。
- 折叠状态按笔记保存在本机。对标题、层级和同名段落序号生成散列，用于重新打开时定位。自动测试验证了同名标题隔离、前面新增文字后的恢复、跳转时展开及表格重显。
- macOS 隔离客户端实测 375 节长文。通过左侧箭头收起第一节，重启后恢复同一折叠状态。实测标题行内省略号、「显示」菜单及 `⌘⌥⇧←`／`⌘⌥⇧→`。查找并选择第一节表格文字后，该节及其上级展开，第二节保留折叠。测试结束后恢复全部展开，Markdown 哈希与基线一致。移动端查找栏外观仍待核对。
- 对照 Pinto 的 `onlyShown` 触控规则，移动端只在光标进入标题或章节已收起时显示左侧折叠箭头。iPhone 17 和 iPad Pro 13 英寸的 iOS 26.5 模拟器均实测隐藏箭头区域、收起带表格和代码的章节、重启恢复、通过箭头和省略号展开。两个隔离文件的 Markdown SHA-256 均与基线一致。定向自动测试 3 项通过。模拟器包沿用隔离构建中的 WebKit 启动补丁，真机未验收。冻结代码树哈希 `5d4c33b4fb149e03fcfc7b25f1acf73ea09988d822714a6c36fb139b7813706c`。
- `npm test` 通过 250 项、跳过 2 项，TypeScript、Vite、macOS debug 打包、严格签名检查及 `git diff --check` 通过。冻结代码树哈希 `9041c73d1b8487f010a884e3ececca201649776bb906704f1a85e032af03c1c9`，清单为 `artifacts/pinto-markra/tree-checksums-heading-folds-final.txt`。

### 移动端撤销与重做

- 对照 Pinto `NoteDetailView.toolbar` 和 `NoteAIUndo.button`，轻羽在当前笔记有撤销记录时，在分享按钮左侧显示独立的撤销按钮。可以重做时长按打开菜单。当前笔记的正文和表格单元格共用编辑历史。
- iPad Pro 模拟器实测正文编辑后显示按钮、点击撤销、在单元格编辑后撤销并保留焦点、长按重做，撤回最后一笔后按钮隐藏。iPhone 17 模拟器确认编辑后按钮出现，坐标工具随后无法命中窗口，按钮点按和重做未完成复测。两端的隔离测试笔记均恢复为原始 Markdown。
- `npm test` 通过 255 项、跳过 2 项。TypeScript、Vite、iOS Simulator debug archive 及 `git diff --check` 通过。模拟器继续使用隔离构建的 WebKit 启动补丁。冻结代码树哈希 `ff1d79aed1e723482bcb280c7a6a1f562139d84b993c1ec453265593e2ba5733`。

### 笔记内查找与替换

- 对照同宽 macOS Pinto 客户端的原生查找栏。轻羽查找栏移到正文上方，提供匹配数、前后导航、清除、完成和按需展开的替换行。查找条件包含忽略大小写、循环、包含、词首和全词。模式查找沿用正则表达式输入。原生菜单和 `⌘F` 可在笔记列表有焦点时打开当前笔记查找。
- macOS 隔离客户端核对同名欢迎笔记的单行、双行查找栏和高亮。轻羽样本的链接地址含额外的 `notes` 子串，因此包含模式为 4 项，全词为 3 项。实测回车下一个匹配，关闭循环后首项的上一个置灰。轻羽与 Pinto 参考窗口的配色、字体设置不同。
- 375 节长文中查找 `Table cell 1`，界面 23 项与磁盘扫描一致。实测命中后展开第一节及其上级，第二节保留折叠，单元格中的命中词可见。替换唯一验收标记并核对落盘后，点击「完成」可直接用 `⌘Z` 恢复。长文及欢迎笔记哈希均与基线一致。
- `npm test` 通过 251 项、跳过 2 项。最后的查找遮罩样式修正后，定向测试通过 54 项，并复测原生截图。TypeScript、Vite、macOS debug 打包、严格签名及 `git diff --check` 通过。冻结代码树哈希 `7f1f081549aac2645c18632830e5e2aecec216189ee150ddc750eb83884d82a5`。移动端查找尚未运行验证。

### 标注类型菜单

- 对照 Pinto `Callouts` 和 `showCalloutMenu`，点击标注左侧图标打开 10 项类型菜单。macOS 使用带颜色和勾选的 SF Symbols。选择类型只改 Markdown 标记中的类型名，保留自定义标题、正文和 `+`／`-`。兼容 Pinto 识别的别名，围栏代码内仍按代码显示。
- macOS 隔离客户端实测点击、勾选、选中 Quote 后落盘及一次撤销，样本 SHA-256 恢复到基线。实测发现切换时会露出文首标题的 Markdown，随后修正焦点。最后版本的焦点由自动回归验证，仍待原生复测。移动端标注菜单待实测。
- 全量前端测试在焦点修正前通过 262 项、跳过 2 项。最终 79 项定向测试、TypeScript、Vite、macOS debug 打包、严格签名及 `git diff --check` 通过。冻结代码树哈希 `c0c5359e3e34158a5502c48f0c3af9e27fe6f4c56373281430561a040bbdcdc9`。

## 斜杠菜单、勾选动效与外部修改着色（2026-11-17）

本轮无法截取 Pinto 窗口（当前进程没有屏幕录制权限，`screencapture -l` 报 `could not create image from window`），对照基准是 Pinto 源码里的数值和 `docs/images` 的官方截图。轻羽一侧在 Chrome 1024×674 和 375×812 下实测。

### 斜杠菜单

- 对照 `SlashCommands.swift` 和 `SlashMenu.swift` 重写 `src/editor-slash-menu.ts`，不再使用 Markra 自带的菜单状态。命令共 18 条，顺序为标题、二级标题、三级标题、正文、任务清单、项目符号列表、短划线列表、编号列表、块引用、标注、高亮、代码块、表格、分隔线、日期、链接、链接到笔记、添加文件。手机不提供添加文件。
- 斜杠在行首或空格之后才打开菜单，查询最长 24 个字符。代码块、行内代码和表格行里不打开，`/` 后紧跟空格即关闭。按 Esc 关掉后，同一个斜杠继续输入不再弹出。保留中文输入法的 `、`。
- 匹配顺序：名称开头，名称里某个词的开头，别名开头。三者都没有时按字母顺序模糊匹配，例如 `chk` 找到任务清单。没有输入时，最近用过的 3 条排在最前，下面有一条分隔线。`/warn` 这类输入会把标注行改成对应类型并直接插入。
- 桌面菜单尺寸：宽 236 px，行高 24 px，内边距 4 px，圆角 8 px，1 px 描边，无阴影。最多显示 9.5 行，底部 12 px 渐隐。图标 14 px 占 18 px 宽，文字 13 px，快捷键 12 px 靠右。
- 选中效果：选中行底色为强调色的浅色（默认主题浅色 `#FFF1DC`、深色 `#3A3A3A`），圆角 5 px，快捷键文字由次要色变为正文色。鼠标悬停使用现有的 `--control-hover`。方向键在首尾循环，回车或 Tab 执行，点击同样执行。
- 手机上命令排在键盘上方的工具条里，行高 44 px，文字 16 px，图标 22 px，不显示快捷键、分隔线和选中底色。
- 实测：桌面输入 `/` 显示 18 条，输入 `div` 只剩分隔线，回车后插入分隔线并记入最近使用。手机显示 17 条。测试输入已撤销。

### 格式快捷键

- 改为 Pinto「格式」菜单的组合，按物理键位匹配：标题 `⇧⌘T`、二级标题 `⇧⌘H`、三级标题 `⇧⌘J`、正文 `⇧⌘B`、等宽 `⇧⌘M`、任务清单 `⇧⌘L`、项目符号 `⇧⌘7`、短划线 `⇧⌘8`、编号 `⇧⌘9`、块引用 `⌘'`、表格 `⌥⌘T`、日期 `⇧⌘D`、添加文件 `⇧⌘A`。原来的等宽 `⌘E` 和块引用 `⌘⇧Q` 已移除。
- 浏览器会占用 `⇧⌘T`、`⇧⌘B`、`⇧⌘J`、`⇧⌘D`、`⇧⌘A` 等组合，Web 版按不出来，桌面 App 不受影响。这些快捷键本轮没有在桌面 App 里实测。

### 勾选动效

- 对照 `CheckPop.swift`。勾选时圆圈在 0.32 秒内按 0.55、1.12、0.97、1 缩放，对勾延迟 0.06 秒后用 0.2 秒画出。整张清单勾完时，其余圆圈由近到远依次跳起（0.34 秒，每个相隔 0.055 秒），并从刚勾的圆圈撒出 10 个小点（0.8 秒）。系统要求减少动态效果时不播放。
- Chrome 实测勾选最后一项，上述四组动画全部触发，小点位置在圆圈中心。

### 外部修改着色

- 对照 `ChangeHighlight.swift` 和 `DecoratedLayoutFragment`。笔记被编辑器之外的来源改动并重新载入后，新增或改过的行在文字后面铺 24% 强调色，内边距 4×1 px，圆角 5 px。文字左侧 8 px 处有一条 2 px 竖线。底色先涨到 36% 再回到 24%，共 0.8 秒。开始打字、撤销或离开这篇笔记时标记消失。
- 编辑器底部显示回执条「外部修改了 N 行」，点「撤销」把正文恢复到改动之前，并走正常的保存。
- Chrome 里用模拟的差异实测了任务清单、编号列表和引用，序号和圆圈不受底色影响。文件监听触发的真实外部修改没有在桌面 App 里实测。

### 与 Pinto 仍不一致的地方

原生专属、WebView 里做不到一致的：

- 勾选动效在 Pinto 里是 Core Animation 图层，对勾沿路径描出（`strokeEnd`）。轻羽直接缩放真实的复选框，对勾用裁剪从左到右露出，笔画顺序是近似的。小点的位移、缩放和透明度在 Pinto 里是三条独立的时间曲线，轻羽合并成一组关键帧。
- Pinto 的着色强度以 30 帧每秒重绘，可以超过 1 再回落，竖线同步由页面色过渡到强调色。轻羽用 CSS 动画实现同样的时长和峰值，中间帧的颜色插值不同。
- 斜杠菜单在 Pinto 里移动选择时会向 VoiceOver 播报「名称，第几项，共几项，快捷键」。轻羽只给每一行写了对应的无障碍名称，没有主动播报。Pinto 使用系统的覆盖式滚动条，轻羽隐藏了滚动条，只保留底部渐隐。
- 此前已记录的 SF Symbols、Liquid Glass 材质、`UIMenu` 和 `UIInputView` 转场仍然适用。

功能上还没做的：

- Pinto 能显示是哪个 AI 改的，并提供「Review」进入修改记录。轻羽的外部修改没有来源信息，回执条统一写「外部」，只有「撤销」。
- Pinto 对被删掉的行会在空隙处画一条虚线，轻羽只标新增和改动的行。Pinto 在 App 内自己发起的修改会保持 2.6 秒后用 1.6 秒淡出，轻羽只实现了一直保留到下一次编辑的那一种。
- 「笔记作为应用」的嵌入组件（`NoteAppWidget`）没有实现。

### 验证

- `npx vitest run`：42 个文件中 40 个通过、2 个跳过，共 407 项通过、2 项跳过。新增 `tests/editor-slash-menu.test.ts` 和 `tests/editor-change-tint.test.ts`。
- `npm run build`（含 TypeScript 检查）和 `git diff --check` 通过。
- 没有运行 macOS、iOS 和 Android 的 App 构建，以上改动在 WKWebView 里的表现待实测。

## 原生窗口并排对比（2026-11-17）

屏幕录制授权后，从应用内终端截取窗口。Pinto 用 `-uitest -demo -open Lisbon -captureWindow` 的 1280×800 离屏窗口和 `-uitest -demo -openSettings` 的 760×660 设置窗口。轻羽用隔离标识 `app.miaoyan.editor.pintouiqa20261117` 的 debug 验证包，测试库按 Pinto 演示数据建了 8 篇笔记。两边都按 2x 截图做像素扫描，比较每行文字的中线。

### 编辑区

| 调整 | 原值 | 新值 | 原因 |
| --- | ---: | ---: | --- |
| `--rich-editor-leading` | 1.46 | 1.4616 | WKWebView 把 13 px × 1.46 截成 18 px，Pinto 是 19 px |
| `--task-leading` | 1.76 | 1.82 | 任务清单行距 Pinto 实测 23.67 px |
| `--table-after-gap` | 10 px | 26.5 px | 表格后的空行在轻羽里高度为 0 |
| 表格上方留白 | 16 px | 14.5 px | 表格整体低 1.5 px |
| 一级标题后 / 二级标题前 / 二级标题后 | 4 / 8 / 2 px | 6 / 5 / 3 px | 按段间距逐项对齐 |
| 正文顶部内边距 | 16 px | 15 px | 标题低 1 px |

复测结果：Lisbon 笔记从标题到「Food」每行与 Pinto 相差不超过 0.4 px。表格和引用在调整留白前低 1.5 px，最后一轮按计算修正，复测见下文。

### 主窗口侧栏和笔记列表

- 「文件夹」标题高 28 → 20 px，上移 3 px；文件夹选中底左侧内缩 8 → 12 px，上下 1 → 2 px。复测后每行中线与 Pinto 相差不超过 0.3 px，选中底高 28 px 一致。
- 笔记行内边距改为 11 / 10 / 9 / 15.5 px，去掉文件夹行上方的 3 px。标题、时间、文件夹三行的间距与 Pinto 相差 1 px 以内，文字左边距一致。
- 侧栏底色截到 Pinto `#EBEBEB`、轻羽 `#F0F0F0`。Pinto 是离屏窗口上的系统材质，没有据此改色。

### 设置窗口外框

账户页和「AI 连接」页的内容由用户另行接入自有账号服务和本地 MCP，本轮只对齐外框。

| 部位 | Pinto | 轻羽原值 | 轻羽新值 |
| --- | --- | --- | --- |
| 搜索框 | 高 28 px，4% 底色，0.5 px 描边，放大镜图标 | 1 px 描边，较深底色，`⌕` 字符 | 与 Pinto 一致，图标用 CSS 遮罩画出 |
| 搜索框到账户行 | 10 px | 14 px | 10 px |
| 账户行 | 高 48 px，左内边距 6 px，头像与文字间距 9 px | 52 / 10 / 10 px | 48 / 6 / 9 px |
| 头像底色 | 强调色 `#D96A06`（深色 `#F4AD33`），人形 78% 黑 | `#EDB646` | 同 Pinto，主窗口侧栏的账户头像随之变化 |
| 标签行 | 行高 29、间距 3、圆角 10 px | 29 / 4 / 8 px | 29 / 3 / 10 px |
| 组间距 | 22 px | 18 px（另有 4 px 行距） | 19 px（另有 3 px 行距） |
| 悬停 | 灰色 7% | 强调色 12%（橙色） | 灰色，与选中同色 |
| 前进后退胶囊 | 12–40 px，4% 底色，无描边 | 14.5–41.5 px，描边 | 12–40 px，与 Pinto 一致 |

复测结果：搜索框、账户行（87–135 px）、头像位置与颜色、导航胶囊的上下沿都与 Pinto 相同。

### 仍不一致

- 账户行在 Pinto 中是「账户」加邮箱或「登录以同步」两行，头像里是实心人形。轻羽目前是一行名称和描边人形，这部分在用户接入账号服务时一并处理。
- 设置页标题 Pinto 用系统字体的拉丁字形，轻羽是中文，字形高度不同，标题中线相差约 2 px，按胶囊按钮的中线对齐。
- Pinto 未登录时侧栏不显示「AI 连接」，有「隐私」页。轻羽保留「MCP 配置」，没有「隐私」。

### 主窗口工具栏胶囊

对照用户提供的 Pinto 与轻羽同屏截图（浅色，2x）。

| 部位 | Pinto | 轻羽原值 | 轻羽新值 |
| --- | --- | --- | --- |
| 左侧「··· 新建」胶囊底色 | 白色，无描边 | `#F4F4F4`，0.5 pt 灰描边 | 页面色 86%，无描边 |
| 胶囊阴影 | 中性灰，下沿 3.5%，向下约 30 pt 淡出 | 暖棕色，下沿约 10%；右侧在标题栏底边被编辑区截断 | `0 8px 40px` 5.5% 黑（`--toolbar-shadow`），标题栏提到编辑区上层 |
| 左侧图标颜色 | 正文色 | 次要灰 `#6A6A6A` | 正文色 |
| 右侧图标线宽 | 约 1.5 pt | 约 2 pt | 统一为 1.55（24 单位视框，约 1.3 px） |
| 新建图标 | 方框加斜出的铅笔 | 圆角框内一支笔（实心轮廓） | 描线的方框加斜出铅笔 |

复测：胶囊下方 0、6、14、22、28 pt 处加深 3.1 / 2.7 / 2.0 / 1.2 / 0.8%，Pinto 为 3.5 / 3.1 / 2.4 / 1.2 / 0.8%。胶囊上方相差 0.4% 以内。深色模式的阴影取 40% 黑，没有 Pinto 深色截图可对照，未实测。下拉菜单的阴影保持原样。

### 工具栏玻璃组件、阴影与按下效果（第二轮）

- 左侧「··· 新建」胶囊原来是 CSS 画的普通胶囊，现在和右侧编辑工具栏一样通过 `mountGlassToolbar` 挂 glassy-liquid 玻璃组件（只在桌面挂载）。窄屏布局里这一行是整条导航栏，隐藏玻璃层和阴影。
- Pinto 的工具栏是系统原生 `ToolbarItemGroup`（macOS 26 Liquid Glass），按下和展开的高亮由系统绘制，源码里没有数值，按用户截图实测：灰圆 `#E6E6E6`，直径 28 pt（胶囊 36 pt，四周各收 4 pt），图标保持正文色。
- 轻羽：展开菜单时玻璃高亮圆缩成按钮内 4 px 的圆，颜色为当前主题正文色的 11.5%（深色模式沿用浅色 16%），去掉原来的白色描边。按下时（glassy-liquid 的 `data-pressed`，或 `:active`）显示同样的灰圆。工具栏里展开状态的图标不再变成强调色；`aria-pressed` 的开关按钮仍用强调色。
- 阴影按 Pinto 截图拟合，改为 `0 8px 28px 8px`，浅色 4.7% 黑（`--toolbar-shadow`），深色 25% 黑未实测。原来只靠模糊，侧边只有 Pinto 的一半深。

复测（macOS 验证包）：按下与展开时灰圆为 `#E6E6E6` / `#E7E7E7`，与 Pinto 一致；胶囊下方 1、4、8、12、16、20、24、28、36 pt 处加深 3.9 / 3.9 / 3.5 / 2.7 / 2.4 / 2.0 / 1.2 / 0.8 / 0.4%，Pinto 为 3.9 / 3.3 / 2.7 / 2.4 / 2.0 / 1.6 / 1.2 / 0.8 / 0.4%，中段略深 0.4–0.8%。

### 笔记列表滚动边缘、吸顶分组标题与缩略图

对照 Pinto `NoteListView`：顶部 `.scrollEdgeEffectStyle(.soft, for: .top)`；分组标题是系统列表的吸顶标题；带图片的笔记在行尾显示第一张本地图片（`RowMetrics.thumbnail` 32 pt，圆角为边长的 22%，0.5 pt 描边，墨色 12%）。

| 问题 | 原因 | 修改 |
| --- | --- | --- |
| 吸顶的「今天」是白色色块，选中行的灰底在它上下各漏出一截 | 分组标题用不透明的列表底色，高度盖不全行 | 标题平时透明；吸顶后用 72% 列表底色加 12 px 背景模糊，底部一条横贯整列的细线（`data-stuck`，由列表滚动时计算） |
| 滚上去的行从标题栏和吸顶标题之间露出来 | 列表顶部 10 px 内边距在吸顶标题上方 | 标题栏改为浮在列表上方，列表上内边距加上标题栏高度；吸顶位置用负的 `top` 贴住标题栏底边 |
| 没有滚动边缘效果 | — | 用 glassy-liquid 的 `<glass-progressive-blur>`（最大 20 px、6 层、向列表底色淡出），外层再垫 6 px 均匀模糊和 55% 列表底色，底边 6 px 淡出 |
| 行尾没有缩略图 | — | `firstLocalImage` 取正文里第一张本地图片（跳过代码块和网络图片），桌面走 `qingyu-asset` 协议，服务器笔记库走 `/api/web/assets`。32 px、圆角 7 px、0.5 px 描边，距行右缘 31 px，行内文字右侧留出 83 px；手机 42 px。图片读不到时去掉缩略图 |

只在桌面宽度（≥ 901 px）启用滚动边缘和吸顶样式；手机布局不变。macOS 验证包实拍：静止时与改动前一致；滚动后吸顶标题贴住标题栏、细线横贯整列，标题栏下的内容模糊淡出；Trip documents 行尾显示缩略图。Pinto 滚动边缘的模糊强度没有逐像素对比，按截图目测调整。

#### 工具栏按下效果修正

- 按下时的灰圆原来靠按钮内边距画，「Aa」有 `#format-button { padding: 0 }` 覆盖，灰圆铺满整个按钮。改为按钮自己的 `::before` 画圆（内缩 4 px），按下 0.05 秒出现、松开 0.15 秒淡出。
- 展开菜单的高亮圆藏起时停在左上角，出现时位移动画从那里开始，会从胶囊左缘滑进来。改为从隐藏到出现时直接定位，只在两个按钮之间切换时滑动。
- macOS 验证包逐帧复测（深色模式）：任务清单与「Aa」按下时灰圆大小一致，松开后淡出；「Aa」打开菜单后高亮圆原地出现并保持。

### 编辑器里的图片

对照 Pinto `ImageEmbed`、`EmbedPress` 和 `MarkdownEditor.keyDown`。

| 操作 | Pinto（macOS） | 轻羽原来 | 轻羽现在 |
| --- | --- | --- | --- |
| 单击 | 选中整张图片：35% 文字选中色覆盖，2 pt 强调色描边，圆角 14 pt，无插入光标 | 光标放进图片的 Markdown，并在图片上方插入一行源码输入框，图片整体下移 | 选区覆盖整张图片，同样的覆盖色和描边，隐藏插入光标，不显示源码 |
| 双击 | Quick Look 打开 | 第一下已让图片下移，第二下常常点空 | 打开查看器 |
| 空格 | 选中的图片用 Quick Look 打开，再按空格关闭 | 无 | 选区正好是这张图片时打开查看器，查看器里再按空格关闭（焦点在工具条按钮上时空格按下按钮） |
| Return / Delete | 在图片后换行 / 删除图片 | 同 | 同 |
| 悬停 | 压暗 6%，平时 1 px 8% 描边 | 右上角放大按钮 | 压暗 6%、1 px 描边，放大按钮隐藏（右键「打开图片」仍用它） |
| 手机 | 轻点用 Quick Look 打开 | 显示源码输入框 | 轻点打开查看器 |

查看器（图片与 Mermaid 图表共用）原来缺少样式，弹出时按钮和图片挤在窗口左下角。补上居中浮窗：40% 遮罩加 6 px 背景模糊，16 px 圆角面板，44 px 工具条（缩小、比例、放大、还原、全屏、关闭），图片等比缩放且不超过原尺寸。打开时焦点放在图片区域，不在关闭按钮上显示焦点环。

一并修正：列表标题栏改为浮动后，打开笔记时选中行会被标题栏和吸顶标题盖住。给桌面列表加了 `scroll-padding-top`（标题栏高度加吸顶标题高度），选中行的自动定位也把这段高度算进去。

无法一致：Pinto 在 macOS 用系统 Quick Look 浮窗（可分享、用其他应用打开），轻羽在所有平台用自带查看器。

验证：新增 `tests/editor-image-click.test.ts` 4 项；macOS 验证包实拍单击选中、双击打开、空格开关、Esc 关闭，以及启动时选中行完整可见。
