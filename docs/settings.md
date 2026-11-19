# 设置功能与验收

设置参考本机妙言 App 和 `MiaoYan/Controllers/PrefsWindowController.swift`、`BasePrefsViewController.swift`、三个 Prefs 分类控制器及 `Views/PrefsControls.swift`：568pt 宽、52pt 顶栏、居中分类、左侧标题和说明、右侧控件、固定窗口高度和内部滚动。

## 已实现

- 通用包含任务清单设置和编辑器分组。桌面可设置工具按钮显示方式，原生桌面可设置窗口置顶，各平台可调整正文宽度。
- 外观提供正文的字体选项，桌面还可调整正文字号。代码使用系统等宽字体，应用界面使用系统字体。
- 原生应用在「存储与同步」中选择轻羽云存储或自定义 S3，手动切换前弹窗确认。轻羽云存储通过账号 API 获得短期凭据，在 `repo/qingyu/v1` 同步当前笔记库。自定义 S3 的字段、访问凭据、超时、寻址、TLS 校验、并发数和存储方式保存在应用私有配置目录，重启和切换后可恢复。S3 提供独立连接测试，错误详情可选择和复制。
- 两种方式都支持立即同步。未配置 S3 且未登录时自动同步关闭；首次识别到登录或重新登录后默认启用轻羽云并立即尝试同步，保留服务端套餐和邮箱校验。用户手动关闭后重启仍保持关闭。开启自动同步后，应用在笔记保存、启动、联网恢复、退出和指定间隔尝试同步，默认间隔为 3 分钟。轻羽云的自动同步开关绑定开启时的登录账号。账户资料、套餐、用量和兑换记录在应用私有目录按账号缓存 24 小时，可在设置里手动刷新。
- 账号刷新令牌保存在应用本地数据目录的 `.account/session.json`。访问令牌只保存在内存，密码仅用于登录请求。会话文件仅供原生账号层使用，在 Unix 上使用 `0600` 文件权限和 `0700` 目录权限，Windows 使用应用本地数据目录的用户权限。退出时清除本地会话，旧版系统凭据会话需重新登录一次。

macOS：应用菜单“设置…”及 ⌘, 打开独立窗口；⌘W/Escape 关闭；再次打开复用窗口并保留当前分类。Windows/Linux：Edit 菜单和 Ctrl+,，使用系统标题栏。移动端：工具栏设置入口、全屏模态页面和返回操作，隔离编辑区快捷键。

`src/preferences.ts` 管理偏好；`settings-view.ts`/`settings.css` 负责界面；`settings-bridge.ts` 在窗口间传递设置和同步请求；`src-tauri/src/settings.rs` 提供原生菜单、窗口和字体列表；`src-tauri/src/sync_profile.rs` 和 `src-tauri/src/client_account.rs` 分别管理同步配置和账号会话。同步由主窗口发起，传输期间仍可编辑、切换和新建笔记。提交前再次保存输入并校验笔记库版本。

## 2026-11-11 首版验证（历史）

- 最终快照的 5 个定向测试文件共 42 项通过：设置验证/持久化/存储失败/恢复、平台快捷键、分类键盘操作与模态返回、同步会话串行化及凭证脱敏、同步提交前刷新输入、预览和所见即所得主题更新。
- 最后一次表单可访问性修改后，重新运行设置的 7 项测试通过；其余未受影响的测试证据复用。Tauri 打包中的 TypeScript 检查通过。
- 使用项目原生 `npm run tauri -- build --bundles app` 完成 macOS release 打包。
- 本机隔离笔记库实测：原生设置菜单、⌘,、⌘W、窗口复用和分类保留；固定深色及重启恢复；本机字体菜单（包含 Menlo、PingFang 等）；Menlo/Georgia 和 20px 字号即时更新预览与所见即所得；默认所见即所得模式重启生效；恢复默认设置。
- 同步分类的可访问名称、字段和按钮已在 macOS 原生可访问性树中验证。使用合成凭证和保留域名 `unit.invalid` 完成“保存连接”的跨窗口联调，无真实网络同步请求。
- 原版参考截图：`artifacts/settings-reference.webp`。交付界面：`artifacts/settings-final.webp`。主题联动截图：`settings-preview-theme.webp`、`settings-rich-theme.webp`。

## 首版交付边界（历史）

工作目录同时存在其他笔记库、菜单功能修改。为避免打包过程中源文件变化，交付包从固定源码快照构建；没有覆盖或回退工作目录中的其他改动。

- 应用：`artifacts/MiaoYan Editor Settings.app`
- 源码快照：`artifacts/settings-source.zip`，文件清单：`artifacts/settings-final-inputs.json`
- 快照清单 SHA256（排序后的 JSON 内容）：`9c176363445003c6ee29f37f00f9fe7f54b327d6d4090f6007f8fe835349bbe7`
- 应用可执行文件 SHA256：`91d1040465c5b5a5811ba9d2aef54a261ae0a970d6728f66218c27957f7ed0ba`
- 构建日志：`artifacts/settings-final-build.log`

Windows/Linux 尚未实际构建运行；Android/iOS 实机测试按要求暂停。本轮没有重复执行此前已经验证的真实 S3 云端同步套件。未加入应用尚不具备的原版图片上传、PPT、全局唤起快捷键和多语言设置。

## 2026-11-11 同步设置修正（历史）

用户确认：AK/SK 需要持久保存；修改后自动保存，不再显示“保存连接”；点击立即同步后的错误显示在按钮下方。

- 完整连接配置（包括 AK/SK）通过 [keyring-core](https://github.com/open-source-cooperative/keyring-rs/wiki/Keyring) 保存为单个系统凭据条目：macOS 钥匙串、iOS Protected Data、Windows Credential Manager、Linux Secret Service、Android Keystore。浏览器 localStorage 不存 AK/SK。
- 自动保存支持不完整配置；写入串行执行，失败保留原有配置。关闭应用前等待已经提交的保存操作结束。旧版非敏感连接字段会在下次修改时迁移。
- 删除“保存连接”及“仅本次运行内保留”等说明，配置校验、保存与网络同步错误统一显示在“立即同步”下方，并遮盖凭据。
- 14 项设置/同步会话定向测试通过，Rust 字段校验测试通过；TypeScript 检查、macOS release 打包通过。
- macOS 实测：在独立编译的 QA 应用中输入合成 AK/SK，不点击同步按钮，直接退出重启，两项凭据与连接字段均恢复。只在 QA 命名空间创建了钥匙串条目，正式应用命名空间没有创建条目。
- 重启后点击立即同步访问未监听的 `127.0.0.1:9`，请求进入原生网络层并失败，错误在按钮下方以红字展示；没有真实 S3 云端请求。截图：`artifacts/sync-settings-error.webp`。

最新应用：`artifacts/MiaoYan Editor S3 Settings.app`。源码快照：`artifacts/sync-settings-source.zip`；清单：`artifacts/sync-settings-inputs.json`；构建日志：`artifacts/sync-settings-build.log`。

源码快照清单 SHA256：`9eef763f36a18a804dd48068ccf055c667929ccc3a07c01fdb96f7318c6b3c63`（含构建解析后的 Cargo.lock）。交付可执行文件 SHA256：`a745677fb56c0848165e8621575d9c8800e6be48230a42fff6a7902366bee094`。QA 单独覆盖编译了应用 identifier 和 productName，以隔离笔记目录及凭据条目；其余源码与交付包一致。

此次修改尚未在 Windows/Linux/Android/iOS 上实机验证；手机测试保持暂停。同一工作目录其他功能的并发后续修改未纳入这个固定快照交付包。

## 2026-11-11 笔记位置和唤起快捷键

通用设置新增两行，布局与妙言相同：笔记位置在顶部，唤起快捷键在窗口置顶下方。目录显示使用 `~` 缩写并保留尾部文件夹名，完整路径可通过提示查看。

- 笔记位置复用现有主窗口笔记库与系统文件夹选择器。更改前保存当前输入，选择后立即打开新目录；取消或失败保留当前目录。此操作不搬迁原目录文件。同步与目录切换互斥，选择器打开期间锁定编辑，避免丢失输入。
- 唤起快捷键使用 [Tauri 官方 global-shortcut 插件](https://github.com/tauri-apps/tauri-plugin-global-shortcut)。点击录入，Esc 取消，× 清除。录入时暂时停用旧组合，结束后恢复或替换。注册、保存失败时保留旧配置；清除后持久保存禁用状态。系统不支持全局快捷键时显示错误，不影响笔记功能。
- 默认组合为 macOS `⌥⌘M`，Windows/Linux `Ctrl+Alt+M`；原生配置保存在应用配置目录的 `activation-shortcut.json`，启动时恢复。触发时恢复、显示并聚焦主窗口。
- 实现位置：`src/shortcut-recorder.ts`、`src/library-settings.ts`、`src/file-library-ui.ts`、`src/settings-view.ts`、`src/settings.css`、`src-tauri/src/activation_shortcut.rs`，以及主窗口与原生窗口的接入代码。

验证：15 项设置/目录/录入器测试通过；Rust 的快捷键解析、冲突回退、保存失败回退及清除持久化 3 项测试通过。TypeScript 检查、macOS release 打包通过。

macOS 隔离应用实测：取消目录选择不改变路径；选择新目录后设置和主窗口立即更新；原目录与新目录的测试文件内容保持不变；重启后目录恢复。自定义组合 `⌃⌥⇧⌘J` 录入成功，从访达触发后恢复并聚焦最小化的主窗口；重启后组合恢复；清除后重启仍为未设置。测试应用已退出，测试组合已释放。

最新应用：`artifacts/MiaoYan Editor General Settings.app`；截图：`artifacts/general-settings-final.webp`；源码快照：`artifacts/general-settings-source.zip`；清单：`artifacts/general-settings-inputs.json`；构建日志：`artifacts/general-settings-build.log`。QA 单独编译了应用标识 `app.miaoyan.editor.generalsettingsqa.20261111`，隔离笔记目录、配置和快捷键持久化文件。

源码清单 SHA256：`653990738fb1f0f09ff741a40f0bdc57a03e15c995ce1dc01cfae5edcd1c4dab`。交付可执行文件 SHA256：`d880e9ac1b588a6c01267c6bf58e963790e13c31969b219186fc2e015af68b96`。

全局唤起仅用于桌面平台；Windows/Linux 尚未实机验证，手机测试保持暂停。交付包仍使用固定源码快照，工作目录中其他聊天的后续改动不包含在本次包内。
