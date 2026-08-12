# 轻语独立产品身份设计

## 1. 背景与目标

当前项目基于 SiYuan 构建轻语，但开发机已安装原版思源。现有开发版本仍使用思源的端口、配置目录、应用 ID、系统协议和内核文件名，导致开发启动时读取原版工作空间配置，并可能访问或同步原笔记数据。

本次将轻语改造成可以与原版思源同时安装、同时运行的独立产品。改造采用最小身份隔离路线：精确修改会造成运行冲突或直接展示产品品牌的标识，不重构仍可正常工作的内部兼容接口。

## 2. 产品命名规则

- 中文产品名统一为“轻语”。
- 非中文产品名统一为“QingYu”。
- 桌面应用使用 `QingYu.app`、`QingYu.exe`；Linux 命令和安装包文件名使用常规小写 `qingyu`。
- 内核程序统一命名为 `QingYu-Kernel`，Windows 使用 `QingYu-Kernel.exe`。
- 中文语言资源使用“轻语”，其他语言资源使用“QingYu”。

用户可见品牌覆盖窗口标题、启动页、错误页、菜单、托盘提示、设置页、关于页、Web 页面标题、CLI 帮助、内核启动横幅、安装包名称及各语言资源。

## 3. 运行身份与数据隔离

| 项目 | SiYuan 原值 | QingYu 目标值 |
| --- | --- | --- |
| 应用 ID | `org.b3log.siyuan` | `com.apkdv.qingyu` |
| 系统协议 | `siyuan://` | `qingyu://` |
| 全局配置目录 | `~/.config/siyuan` | `~/.config/qingyu` |
| Electron 用户数据目录 | `SiYuan-Electron` | `QingYu-Electron` |
| CLI 默认工作空间 | `~/SiYuan` | `~/QingYu` |
| 固定服务端口 | `6806` | `9806` |
| 发布服务默认端口 | `6808` | `9808` |
| 内核二进制 | `SiYuan-Kernel` | `QingYu-Kernel` |

轻语只读取 `~/.config/qingyu` 中的工作空间历史、窗口状态、崩溃信息和其他全局配置，不迁移、不回退读取思源配置。用户仍可主动选择任意工作空间目录，包括已有的思源工作空间；这种显式选择不属于自动读取或身份冲突。

Electron 用户数据、单实例锁、Windows AppUserModelId、macOS/Windows/Linux 应用标识和系统协议注册都使用轻语身份。系统只注册 `qingyu://`，不注册或抢占 `siyuan://`。

## 4. 端口行为

端口保持原版多工作空间架构，只修改固定默认值：

- 正式桌面端继续通过系统 `listen(0)` 为每个工作空间分配实际内核端口。
- 第一个工作空间额外提供 `9806` 固定端口反向代理，供外部 API 等固定入口使用。
- 后续工作空间继续使用各自的系统随机端口，不建立第二个固定代理。
- 开发模式、Docker 和移动端的固定默认端口改为 `9806`。
- 发布服务默认端口改为 `9808`。
- 用户通过命令行或设置显式指定端口时完全尊重用户选择，不限制为 `9XXX`。
- 自动动态端口继续由系统从全部可用端口中选择，不增加 `9000–9999` 分配器或校验器。
- 发布服务配置为 `0` 时保持系统随机端口行为。

如果 `9806` 已被占用，固定代理沿用现有逻辑记录警告，实际随机端口内核仍可运行。用户指定的内核端口不可用时沿用现有错误提示，不新增定制恢复机制。

## 5. 桌面、Web 与构建产物

Electron 在 macOS/Linux 只查找并启动 `kernel/QingYu-Kernel`，在 Windows 只查找并启动 `kernel/QingYu-Kernel.exe`。`kernel/`、`kernel-darwin/`、`kernel-darwin-arm64/`、`kernel-linux/`、`kernel-linux-arm64/` 和 `kernel-arm64/` 等资源目录保持现有结构，只重命名其中的二进制。

macOS、Windows、Linux 构建脚本、CI 配置、Electron Builder 配置、资源复制路径、安装器快捷方式、包名和产物名同步更新。应用 ID 统一使用 `com.apkdv.qingyu`，系统协议统一使用 `qingyu`，User-Agent 使用 `QingYu/<version>` 品牌，不继续把 SiYuan 品牌作为轻语的运行身份。

Docker/Web 的默认暴露端口、示例地址、API 文档和与固定端口有关的说明同步改为 `9806`；发布服务文档改为 `9808`。只修改与当前产品运行或构建直接相关的文档，不对上游历史记录做无关重写。

## 6. 内部兼容边界

为降低风险并保持插件与数据兼容，下列内部标识不在本次重命名范围：

- `.sy` 数据格式及现有工作空间结构。
- `window.siyuan` 插件 API 和既有插件接口契约。
- `/api/*` HTTP 接口路径。
- Go Module、上游依赖导入路径和无需对外展示的内部类型名。
- 无需对外展示且不会导致产品冲突的 CSS 类、数据字段和局部变量。
- 内容导入、解析时对旧 `siyuan://blocks/...` 链接的兼容识别。

新生成和对外打开的应用链接使用 `qingyu://`。保留旧内容链接的解析能力不代表向操作系统注册 `siyuan://`。

AGPL 许可证、原作者版权声明和用于追溯上游问题的 GitHub 链接必须保留。品牌替换不得删除或伪造上游归属信息。

## 7. Android 完整迁移

实施阶段将官方 `siyuan-note/siyuan-android` 仓库克隆到 `/Volumes/extendData/Data/IdeaProjects/siyuan-android`。克隆后先读取该仓库实际存在的项目规则，再进行修改。

Android 不采用内部 Namespace 兼容边界，而是完成全量包迁移：

- `applicationId` 和 `namespace` 改为 `com.apkdv.qingyu`。
- Debug 包使用 `com.apkdv.qingyu.debug`。
- Java/Kotlin 源码目录迁移到 `com/apkdv/qingyu`，所有包声明及引用同步更新。
- Manifest 中的 Activity、Service、Receiver、Provider、任务标识和组件引用同步更新。
- FileProvider authority 使用轻语应用 ID。
- `Theme.SiYuan` 等产品资源标识改为 `Theme.QingYu`，相关 Manifest 引用同步更新。
- Android 系统协议改为 `qingyu://`。
- 中文应用名改为“轻语”，非中文应用名改为“QingYu”。
- APK/AAB 基础文件名改为 `qingyu-*`。
- ProGuard、测试和构建脚本中依赖旧包路径的规则同步更新。
- Android 继续使用主仓库生成的 `kernel.aar`，其固定默认端口随共享内核改为 `9806`。

Android 不保留 `org.b3log.siyuan` Namespace、源码包声明或运行身份。许可证、原作者版权和必要溯源链接不属于产品 Namespace，继续保留。

## 8. 暂缓的官方应用更新改造

当前代码仍会查询思源官方版本接口、下载 `siyuan-*` 官方安装包并启动安装。该链路与独立产品身份不一致，但用户明确要求本次不处理其行为。

本次仅在更新核心入口附近加入一个可通过 `rg 'TODO\(QingYu\)'` 快速定位的中文代码注释，记录后续必须替换的范围：思源版本接口、官方安装包下载地址、安装包命名、校验与安装启动流程，以及设置页中的检查更新和自动下载入口。

该定位标记不包含插件、主题或在线集市更新。本次不得删除、禁用或重写官方应用更新逻辑。

## 9. 实施边界

本次实施涉及两个仓库：当前 `siyuan` 主仓库和待克隆的同级 `siyuan-android` 仓库。修改按以下边界推进：

1. 先用自动检查固定运行身份、端口和内核文件名契约。
2. 修改主仓库配置目录、应用 ID、协议、默认端口、内核名和构建路径。
3. 修改桌面、Web、CLI 和语言资源中的用户可见品牌。
4. 克隆并迁移 Android 的应用 ID、Namespace、源码包、资源和构建产物。
5. 添加官方应用更新暂缓项的唯一定位标记。
6. 完成静态、构建和最小运行验证。

不得借品牌改造重构插件 API、数据格式、HTTP API、编辑器、同步协议或其他无关业务。不得提交、推送、发布或部署，除非用户另行明确授权。

## 10. 验证标准

主仓库需要完成以下验证：

- 品牌边界测试覆盖应用 ID、配置目录、Electron 数据目录、协议、固定端口、发布端口和内核文件名。
- 静态检查确认用户可见产品位置不再出现“思源/SiYuan”，并通过白名单保留许可证、上游链接和明确的内部兼容标识。
- 前端通过项目规定的类型检查与 lint。
- 相关 Go 测试通过。
- 内核成功编译为 `app/kernel/QingYu-Kernel`，Electron 不再依赖 `SiYuan-Kernel`。
- 使用全新的临时轻语工作空间启动内核与 Electron，确认固定端口 `9806`、窗口标题、配置目录和协议均使用轻语身份。
- 验证轻语启动过程不读取 `~/.config/siyuan`，不使用原笔记工作空间，也不启动或修改已安装的原版思源。
- 验证用户显式指定非 `9XXX` 空闲端口时仍可正常启动。

Android 需要完成以下验证：

- 运行 Gradle 编译或当前环境下最窄的可用构建任务。
- 检查合并后的 Manifest 使用 `com.apkdv.qingyu` 组件、Provider authority 和 `qingyu://`。
- 检查源码和构建配置不再使用 `org.b3log.siyuan` Namespace 或包路径。
- 检查 APK/AAB 文件名和中文、非中文应用名。

Windows、Linux、Docker 和 Android 真机的完整运行联调属于后续任务。本次仍需同步更新它们的静态构建配置，并说明当前环境无法覆盖的剩余风险。

## 11. 非目标

- 不限制用户指定端口必须属于 `9XXX`。
- 不把系统动态端口限制到 `9000–9999`。
- 不建立新的端口分配、冲突恢复或品牌配置生成框架。
- 不迁移原版思源的配置、窗口状态或工作空间历史。
- 不删除 `.sy`、`window.siyuan`、`/api/*` 或 Go Module 等兼容标识。
- 不处理思源官方应用更新行为，只添加明确定位标记。
- 不在本次完成 Windows、Linux、Docker 或 Android 真机的完整业务回归。
- 不提交、推送、发布或部署任何修改。
