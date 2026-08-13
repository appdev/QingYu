# QingYu Privacy Statement Evidence Matrix

本文件记录 2026-08-13 隐私政策表述所依据的代码边界，不替代运行时抓包、已发布网站审计或法律意见。

| Policy claim | Repository evidence | Wording boundary |
| --- | --- | --- |
| 工作空间、配置和日志主要在本地处理 | `kernel/util/working.go`、`kernel/model/conf.go`、`kernel/util/logging.go` 及工作空间初始化路径 | 使用“主要”“默认”，不承诺所有可选功能永不联网 |
| 未发现面向 QingYu 开发者的独立遥测后台 | 对 `telemetry`、`analytics`、`tracking`、安装事件和设备标识上传路径的代码扫描；产品更新器在 `kernel/model/updater.go` 中为空实现 | 使用“当前版本未发现/未配置”，不使用不可验证的永久性“绝不收集” |
| 用户启用网络能力时可能传输数据 | `kernel/model/sync.go`、`kernel/model/repository.go`、`kernel/model/webdav.go`、`kernel/model/s3.go`、`kernel/mcp/`、`kernel/model/publish.go`、插件和网络资源代码 | 数据接收方由用户选择的服务或扩展决定，其政策独立适用 |
| 日志和反馈由用户决定披露 | `kernel/api/system.go` 的日志导出路径、界面反馈入口 `mailto:lengyue@apkdv.com` | 不声称日志永不包含路径、标题、错误上下文或系统信息；要求发送前检查 |
| 数据保留和删除主要由本地文件及所选服务控制 | notebook、文件系统、历史、同步和仓库实现 | 删除本地数据不保证同时删除备份、同步目标或第三方副本 |
| 应用版本更新不连接上游发行服务 | `kernel/model/updater.go`、`kernel/model/updater_test.go`、关于页和 Electron 安装 IPC 入口 | 仅适用于 QingYu 应用版本更新；插件、主题和用户启用的网络功能仍可能联网 |

验证时应运行：`rg -n "telemetry|analytics|tracking|GetRhyResult|release\\.b3log|release\\.liuyun" kernel app/src app/electron`、品牌审计、相关 Go 测试和前端测试。若未来增加开发者运营的后台、崩溃上报、账号、同步、更新或分析服务，必须先更新本矩阵和四语言隐私政策。
