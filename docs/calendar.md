# 创建系统日程

原生应用工具栏的日历按钮打开创建表单。标题默认取当前笔记名称；备注由用户自行填写，不自动复制笔记正文。支持起止时间、不提醒、开始时提醒、提前 5/15/30/60 分钟或 1 天、自定义提前 0–40320 分钟（28 天）。默认开始时间至少晚于当前时间 16 分钟，预留默认提前 15 分钟提醒的时间。输入按当前设备时区转换为绝对时间；不提供重复、全天、编辑或删除已有日程。

| 平台 | 写入方式 | 权限及限制 |
| --- | --- | --- |
| macOS | EventKit 保存到默认日历，提醒使用 EKAlarm | macOS 14+ 申请仅写权限；旧版申请日历访问。应用 Info.plist 包含两种用途说明。没有默认可写日历时报告错误。 |
| iOS | EventKit 保存到默认日历，提醒使用 EKAlarm | iOS 17+ 申请仅写权限；旧版申请日历访问。工程尚未初始化，原生插件随将来的 `tauri ios init` 接入。 |
| Android | CalendarContract；优先可写且可见的主日历，否则使用首个可写日历 | 申请 READ_CALENDAR / WRITE_CALENDAR，用于选择日历和保存；不查询已有事件。有提醒时仅选支持本地提醒的日历，事件和提醒通过 applyBatch 一起写入。无账户或无可写日历时提示用户先在系统日历添加。 |
| Windows / Linux | 生成带 VALARM 的 RFC 5545 `.ics`，交给默认关联应用 | 必须安装并关联日历软件，由用户确认导入。仅报告已交给日历软件，不能确认已保存。导入器可能忽略提醒，需在目标软件检查。文件保留在应用缓存的 calendar 子目录供异步导入，包含用户填写的标题和备注，可随应用缓存删除。 |
| Web | 不提供入口或写入 | 不申请日历权限。 |

只在点击“保存到日历”后请求系统权限或交给日历软件。写入失败保留表单，等待用户修正或重试；写入成功停用再次保存，避免重复创建。通知发送由系统日历负责，受日历通知权限、勿扰模式、后台策略等影响；应用不另设本地定时通知。

原生桥接位于 `src-tauri/plugins/calendar`，仅由应用的 `create_calendar_event` 命令调用；前端校验之外，Rust 在桥接边界再次校验标题、时间范围和提醒值。ICS 使用 UTC、转义文本、按 UTF-8 字节折行，防止标题/备注注入其他事件字段。

API 依据：[Apple 事件库权限](https://developer.apple.com/documentation/eventkit/accessing-the-event-store)、[Android CalendarContract](https://developer.android.com/reference/android/provider/CalendarContract)、[Tauri 移动插件](https://v2.tauri.app/develop/plugins/develop-mobile/)。

## 验证（2026-11-11）

- 前端构建通过；日程表单 3 项测试、Rust 参数边界与 ICS 编码 2 项测试通过。
- 浏览器模拟原生桥接，验证自定义提醒、失败后保留输入、360px 窄屏无横向溢出；正式 Web 页面隐藏日程入口。没有写入真实日历。
- macOS Debug `.app` 构建通过，检查到日历权限用途说明。Android arm64 Debug APK 构建通过，核对合并后的日历读写权限及插件类。未安装设备。
- Android 构建同时发现现有 `workspace::show_library` 在移动端调用桌面 `unminimize`，仅为该调用补充桌面平台条件。
- iOS Swift 语法及 Swift Package 配置检查通过。在临时源码快照中生成了 Xcode 工程，但无签名模拟器构建被 `iOS platform not installed` 阻断。验证时补齐了 Rust 的 `aarch64-apple-ios-sim` target；Tauri 初始化还通过 Homebrew 安装了缺失的 `libimobiledevice` 及其依赖。工作区未生成 iOS 工程。
- Windows/Linux 仅验证共享 ICS 编码，尚未平台编译或验证默认日历导入。所有平台均未验证真实日历写入、授权拒绝的系统流程和提醒通知触发。

因工作区有其他聊天的并发修改，构建使用 `artifacts/calendar-20261111/source` 固定快照；源码清单、构建日志、截图和验证摘要同存于该目录的上级。后续工作区修改不自动包含在构建产物中。复用 Android 构建缓存时，需包含被 Git 忽略的 Tauri 生成基类；本次按已锁定的 Tauri 模板补齐并记录生成输入。
