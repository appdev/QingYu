# cmark-gfm 桌面与移动接入路线调查

日期：2026-11-11。本轮只核查源码、官方 API 和构建机制，没有修改产品依赖、编译整包、生成或安装手机包。Vditor 继续保留。语义复刻实验由主线程另行记录，本报告不把平台推断写成设备验收。

## 结论

**真正的 cmark-gfm 可以用于我们的桌面和移动预览。两条合理路线是相同 C 源码原生静态链接，或将相同 C 源码编译为单线程 WASM 放入现有 Worker。** 二者都能保留 Vditor。选择主要取决于纯浏览器是否需要继续使用相同预览链路，以及是否愿意维护每个平台的 C 工具链；本轮没有数据证明其中一种在应用端到端体验中必然更快。

| 路线 | 桌面 Tauri | Android / iOS Tauri | 当前纯浏览器页面 | 主要代价 |
| --- | --- | --- | --- | --- |
| 锁定 C 源码 → Rust 小型 FFI → Tauri command | 可行，原生执行 | 架构可共用，逐目标编译待验证 | 没有 Rust IPC，需要另一个入口或不再作为同语义浏览器产品 | 多平台 C 构建、FFI 审计、IPC 数据拷贝和调度 |
| 锁定同一 C 源码 → 自编 WASM → 现有 Worker | 可行，WebView 内执行 | 可行方向，最低 WebView 和内存待验证 | 可共用同一个资产与 Worker | WASM 构建/打包、冷启动、堆内存和 WebView 兼容性 |
| Node 原生 addon | 默认 WebView 不能直接加载 | 不适合直接复用 | 不能直接用于浏览器 | 额外 Node 运行时或独立服务，当前没有必要引入 |

原生应用优先、浏览器仅为辅助时，先验证 Native 路线合理；若浏览器也是产品目标或需要保持同一套快速 UI 验证链路，同版本 WASM 进现有 Worker 是正式候选，不应因为现成包不合适而排除它。Tauri 的 UI 由系统 WebView 执行 HTML/CSS/JS，Rust Core 与其通过 IPC 通信；它没有默认给前端附带 Node 原生 addon 环境。[Tauri 进程模型](https://v2.tauri.app/concept/process-model/)

## 先锁基准源码，而不是只锁名字

妙言当前锁的是 `swift-cmark-gfm 1.0.2`、revision `7ec44e7384793e507cf046336aef68586719a24e`。这个包的实际 target 是 C 源码，并不要求我们在 Rust 工程引入 Swift runtime；其版本头为 **`0.29.0.gfm.3`**。可移植的对象是这套 C 源码及其许可证和生成头文件，不是整个 Swift 应用。[固定包清单](https://github.com/stackotter/swift-cmark-gfm/blob/7ec44e7384793e507cf046336aef68586719a24e/Package.swift)、[固定版本头](https://github.com/stackotter/swift-cmark-gfm/blob/7ec44e7384793e507cf046336aef68586719a24e/Sources/CMarkGFM/cmark-gfm_version.h)

GitHub cmark-gfm 当前 latest release API 仍指向 `0.29.0.gfm.13`（2023-07-21），但 master 在 2026-09-28 仍有提交，因此不能仅凭 release 日期判断项目停止维护。**复刻基准与长期维护版本是两个选择**：先用妙言精确 revision 建立对照，再评估更新版差异；如果直接换成最新包，即使都叫 cmark-gfm，也不能声称输出完全相同。[latest release API](https://api.github.com/repos/github/cmark-gfm/releases/latest)、[master 提交 API](https://api.github.com/repos/github/cmark-gfm/commits?per_page=1)

`pulldown-cmark` 是另一套 Rust CommonMark 实现；Comrak 虽源于 cmark-gfm 的 Rust 移植，其当前维护者明确说明两套代码已分化。它们可以是其他选型，但不能当成复用同一 C 引擎来证明妙言语义一致。[pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark)、[Comrak related projects](https://github.com/kivikakk/comrak#related-projects)

## Native：同一 Rust command，无需另写 Java 或 Swift 业务桥

建议链路：前端主上下文携带 `{id, source}` 调用 Rust command → `tauri::async_runtime::spawn_blocking` 执行 C 解析及 HTML 输出 → 返回 `{id, html, offset}` → 沿用当前 sanitizer、预览 iframe 和图表处理。`async fn` command 本身只表示调度到 async task，同步 CPU 密集的 C 解析仍应进入 blocking executor；不能在 async 函数中直接重活后就宣称不会占执行器。[Tauri command](https://v2.tauri.app/develop/calling-rust/)、[spawn_blocking API](https://docs.rs/tauri/latest/tauri/async_runtime/fn.spawn_blocking.html)

当前项目 [render.worker.ts](../src/render.worker.ts) 是纯 Worker；`@tauri-apps/api` 当前安装实现的 `invoke` 读取 `window.__TAURI_INTERNALS__`。Native 路线不宜假定可以把这次 invoke 原样塞进 Worker：应由主前端异步发起 IPC，Rust 负责重活，保留当前版本号过滤；主前端不用做同步解析。[项目 API 实现](../node_modules/@tauri-apps/api/core.js)、[Tauri core API 源码](https://github.com/tauri-apps/tauri/blob/dev/packages/api/src/core.ts)

移动应用已经包含 Rust 层。解析器没有摄像头、相册等平台专有 UI 需求，因此无需为这项业务单独增加 Kotlin/Java JNI 或 Swift 桥；只需让同一 C 库链接进每个 Rust 目标。这里不代表 Tauri 自身的移动桥不存在，也不代表现有第三方 crate 已通过 Android/iOS。[Tauri 移动前置条件](https://v2.tauri.app/start/prerequisites/#configure-for-mobile-targets)

### Native 构建和 FFI 边界

GitHub 源码提供静态 core/extension 库开关，官方 CI 覆盖 Linux、macOS 和 Windows；没看到同一 CI 对 Android/iOS 的覆盖。Windows 的静态库输出名带 `_static`，与 Unix 不同；跨平台构建应显式选择正确目标，不用开发机已安装的动态库充数。[CMake 根配置](https://github.com/github/cmark-gfm/blob/master/CMakeLists.txt)、[extension 静态目标](https://github.com/github/cmark-gfm/blob/master/extensions/CMakeLists.txt)、[官方 CI](https://github.com/github/cmark-gfm/blob/master/.github/workflows/ci.yml)

| 平台 | 所需接入 | 本轮证据边界 |
| --- | --- | --- |
| macOS | 对应 arm64/x86_64 的 Clang 静态库，或随 Cargo 编译 | 上游有桌面 CI；我们的应用集成尚未做 |
| Windows | MSVC 或选定 Clang 工具链、正确导出宏/库名和 CRT | 妙言 vendored `config.h` 无条件启用 GCC/Clang builtin/attribute，不能直接宣称原样通过 MSVC |
| Linux | 对目标架构编译静态库，保持 PIC 与系统 C ABI 一致 | 上游有 Linux CI；未在 Linux 应用验收 |
| Android | NDK Clang、ABI/API level/sysroot、目标 archive 和 PIC；C 与 Rust 目标一致 | 官方 NDK 支持现有 C 项目和 CMake；本项目编译、安装和实机测试全部未做 |
| iOS | Apple SDK、device/simulator 分开的架构和部署目标；链接进 Rust staticlib | 官方 CMake 有 iOS toolchain 支持；本项目目标编译和设备验收未做 |

Android 工具链及 ABI/API level 设置见 [NDK 官方说明](https://developer.android.com/ndk/guides/other_build_systems)；iOS 与模拟器选择见 [CMake toolchains](https://cmake.org/cmake/help/latest/manual/cmake-toolchains.7.html#cross-compiling-for-ios-tvos-visionos-or-watchos)。Cargo 构建可以使用 `cc` 调用目标 C 编译器，也可以由 CMake 产出固定两库；选择一套即可，不同时引入重复构建路径。[cc 构建说明](https://docs.rs/cc/latest/cc/)

FFI 最小边界需要：每次解析创建/释放 parser、AST 和 HTML 缓冲，输出复制为 Rust String 后由同一 allocator 释放；传入字节长度以允许 NUL 被引擎正确处理，避免 `CString::new(...).unwrap()` 让用户文档触发 panic。不要把裸 C 指针跨任务共享。精确源码的 extension 注册使用非原子的静态标志，Rust 层应在并行解析前通过 `Once` 类机制完成一次初始化；解析完成后不并发销毁全局 registry。[基准注册实现](https://github.com/stackotter/swift-cmark-gfm/blob/7ec44e7384793e507cf046336aef68586719a24e/Sources/CMarkGFM/core-extensions.c)

### 现成 Rust 包不是即插即用的全平台答案

`cmark-gfm 0.1.1` 和 `cmark-gfm-sys 0.29.0` 的最新发布、对应仓库最后提交均停在 2019。高层包暴露脚注/sourcepos 等 flags、扩展注册；但 sys 的 build.rs 使用 `cmake` 与 `bindgen 0.49`，运行构建时生成 bindings，增加 libclang 与交叉 target 配置要求，没有现成的移动 CI 保证。[crate API](https://crates.io/api/v1/crates/cmark-gfm)、[sys API](https://crates.io/api/v1/crates/cmark-gfm-sys)、[sys build.rs](https://github.com/poupryc/rust-cmark-gfm/blob/master/cmark-gfm-sys/build.rs)

源码还存在输入 `CString::new(...).unwrap()`，以及两个 HTML 输出路径没有看到对返回 C 缓冲的释放；因此不建议直接把高层包放进高频预览循环。可以借鉴其调用顺序，但应审计并替换为小型 RAII 封装，绑定我们选择的精确 C revision。[parser.rs](https://github.com/poupryc/rust-cmark-gfm/blob/master/src/parser.rs)、[render.rs](https://github.com/poupryc/rust-cmark-gfm/blob/master/src/render.rs)

## WASM：正式候选，但需要同版本自行构建

同一 C 源码可以通过 Emscripten 编译；其工具链支持 CMake/Make 等现有 C 构建系统。现有 Worker 可保持 `{id, source}` → `{id, html, offset}`，每个 Worker 初始化一次模块，每次调用回收输入/输出缓冲；不必先改成原生 IPC。[Emscripten 构建文档](https://emscripten.org/docs/compiling/Building-Projects.html)

这可以是**单线程 WASM 在普通 Web Worker 中执行**。它与 Emscripten pthreads 是两回事：不需要为了移开 UI 主线程就启用 pthread/SharedArrayBuffer，也不需要因此增加 COOP/COEP。只有选择 pthread 路线时，官方文档要求相应隔离头。[Emscripten pthreads](https://emscripten.org/docs/porting/pthreads.html)

当前 [tauri.conf.json](../src-tauri/tauri.conf.json) 已允许 `script-src 'self' 'wasm-unsafe-eval'`，也已有本地 Worker。不能写成“WASM 必然需要放宽当前 CSP”；应验证实际打包资产、MIME 或 arrayBuffer 加载、Worker 源及最低 WebView 支持，保持无需一般 JS `unsafe-eval`。[CSP 的 WASM 指令](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/script-src#unsafe_webassembly_execution)

**最低版本与实际执行上下文需要一起核对**：当前应用声明 iOS 15.0；浏览器兼容数据记录 `wasm-unsafe-eval` 从 Safari/iOS WebView 16、Chrome/Android WebView 97 才支持。WebKit 官方 STP 141 公告也记录这一指令在 2022 年实现，早期受 CSP 限制的 WebKit 上下文缺少此指令时要求更宽的 `unsafe-eval`。因此不能仅凭配置中存在 keyword 就宣称最低目标已覆盖。[兼容数据](https://github.com/mdn/browser-compat-data/blob/main/http/headers/Content-Security-Policy.json)、[WebKit STP 141](https://webkit.org/blog/12434/release-notes-for-safari-technology-preview-141/)、[WebKit 旧实现讨论](https://bugs.webkit.org/show_bug.cgi?id=197759)

但这也**不能推出 iOS 15 外部 Worker 必定失败**：普通外部 Worker 一般使用其脚本响应的 CSP，`blob:`/`data:` Worker 才继承创建者 CSP。当前安装的 Tauri 2.12.5 `get_asset` 仅为 `.html` 资产设置 `csp_header`；外部 `.js` Worker 不必然受到主文档 `script-src` 的相同限制。最终要看打包后的 Worker URL、脚本响应头、平台协议处理和 WebView 行为，不能把主文档 CSP 兼容表当成实测结论。后续原型需保持主文档现有安全策略，检查这些实际条件，不预先增加一般 JS `unsafe-eval` 或提高最低系统版本。本轮没有更改策略或最低版本。[Worker CSP 规则](https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Using_web_workers#content_security_policy)、[Tauri 2.12.5 资产实现](https://docs.rs/tauri/2.12.5/src/tauri/manager/mod.rs.html)、[Tauri 协议响应实现](https://docs.rs/tauri/2.12.5/src/tauri/protocol/tauri.rs.html)

现成候选仅作为工具链参考：

- **gfm-wasm 1.0.1** 的确是 cmark-gfm WASM，CMake 显式链接 core+extensions，提供预编译资产。但 C wrapper 把 options 硬编码为 `CMARK_OPT_DEFAULT`，并挂上 tagfilter；没有公开启用 FOOTNOTES/SOURCEPOS/UNSAFE 的入口。直接换包会丢掉我们需要的脚注、sourcepos 和行内 HTML 能力，不能满足妙言基准。需要改 wrapper 并用目标 revision 自编，而不是说明 WASM 路线本身不可行。[包 API](https://github.com/thelovekesh/gfm-wasm#api)、[C wrapper](https://github.com/thelovekesh/gfm-wasm/blob/main/gfm-wasm.c)、[WASM 构建](https://github.com/thelovekesh/gfm-wasm/blob/main/CMakeLists.txt)
- **cmark-gfm-js** 是旧 Emscripten JavaScript port，可用于 Node/browser，但其 README 明确 tasklist 不支持，构建脚本没有锁定为我们所需的现代表达式/资产形态，不应当成完整复刻的现成库。[官方仓库](https://github.com/mgenware/cmark-gfm-js)、[构建脚本](https://github.com/mgenware/cmark-gfm-js/blob/master/build.sh)

## 下一步最小验证矩阵

这不是本轮要执行的手机任务，而是接入前必须补的证据。

1. 固定基准 C revision、扩展列表、parse/render flags 和妙言前后处理；对 37 组文档及边界样本保存 AST/HTML 差异，再选 Native 或 WASM 的第一次应用原型。
2. 若选 Native，先 macOS 隔离 host/command 原型，再分别验证 Windows/Linux 的静态构建；Android/iOS 暂留到用户恢复手机相关验证后，不在本轮编译或安装。
3. 若选 WASM，先在同一现有 Worker 中验证真正基准源码和公开 ABI，使用本地资产、固定 Emscripten、显式内存释放；验证纯浏览器和桌面 WebView，再补未来 Android/iOS 最低目标。
4. 两条路线测同一文档的冷启动、热解析 P50/P95、IPC 或 Worker 拷贝、HTML patch/layout/highlight/图表总耗时、峰值内存。不能用单独 C 解析时间或上游 benchmark 代替应用流畅度。
5. 保留版本号过滤、IME 暂停、错误时保留原文；合并待处理输入，避免输入速度高于解析速度时持续排队。C 调用开始后不能假装通过丢弃结果就取消了其 CPU 工作。
6. 保持 sanitizer 和 URI 策略，未匹配的生成 HTML 不能直接进入 IPC 有权限的主界面。预览引擎变化不修复 Vditor 的编辑模型或 Markdown 规范化；手机测试继续暂停。

本轮没有运行任何平台新解析器应用接入、WASM 性能或手机实机验证。上述“可行”属于官方机制与源码支持的工程判断，不是五平台已经通过的结论。
