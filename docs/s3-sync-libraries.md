# S3 同步第三方库调研

查询日期：2026-11-11。范围：官方文档、官方源码/仓库、维护者讨论，以及 Context7 返回的官方源码片段；没有修改生产代码、依赖或配置，没有连接用户 bucket。

后续已完成 MDSync 隔离运行验证：9 项功能验证通过，另 1 项确定性测试复现索引并发丢更新；详见 [验证结果](mdsync-spike-results.md)。本文中的“未运行”描述调研阶段的边界，以后续运行证据为准。

再后续已补齐条件提交并接入桌面应用，真实 S3、双端并发及原生桌面 UI 验证见 [正式集成结果](s3-sync-verification.md)。

## 结论

补充 MDSync/Awan 等候选后，修正前轮遗漏：**确有带三方合并和历史的现成 TypeScript 同步实现，MDSync 是本轮最值得优先验证提取复用的引擎候选**。它是 MIT 的 Obsidian 插件，还不是能直接装进本项目的通用 SDK；需验证宿主适配、当前笔记库映射和并发提交边界。[MDSync 引擎](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/engine.ts)、[许可证](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/LICENSE)。用户候选表的“自动合并”不能视为等价保证：Awan 有未处理的合并失败标志；Remotely Save 的开源主体是 Apache-2.0，PRO 部分才是 PolyForm Strict，不能将整项笼统写成 Source Available。

落地路线建议：先以 MDSync 做一次有边界的引擎提取验证，复用其基线、三方合并、冲突副本及历史设计；S3 原子提交仍可由 Rust **`object_store` 条件写**承担，两者并不对立。若适配成本超过当前基本同步需求，采用 `object_store` + 下文的最小应用同步层。不要同时引入两个完整同步引擎，也不为集成 MDSync 自动扩大到 relay、协作和共享文件夹。[object_store 条件写源码](https://github.com/apache/arrow-rs-object-store/blob/main/src/aws/mod.rs)。完整 S3 API 需求可考虑 AWS 官方 SDK；未来明确需要多存储后端再考虑 OpenDAL。[AWS PutObject API](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/operation/put_object/builders/struct.PutObjectFluentBuilder.html)、[OpenDAL S3 后端](https://github.com/apache/opendal/blob/main/core/services/s3/src/backend.rs)。

**下面四个 Rust 候选都是对象存储客户端，不是笔记双向同步引擎。** 条件写只能检测过期写入；基线保存、合并、冲突副本、离线恢复和编辑器交接需应用实现或复用后文的同步引擎。不能把“支持 S3”解释成“已经提供冲突处理”。[object_store 项目定位](https://github.com/apache/arrow-rs-object-store)、[AWS SDK 定位](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html)、[rust-s3 项目定位](https://github.com/durch/rust-s3)、[OpenDAL 项目定位](https://github.com/apache/opendal)。

## 当前项目约束

现场读取 `src/notebook.ts`、`src/main.ts`、`src-tauri/Cargo.toml`：当前 Tauri 2 Rust 后端仅依赖 `tauri`，前端通过一个 localStorage JSON 保存整个 `Library`。`Note` 包含 `id/folderId/source/createdAt/updatedAt`；`Library` 还有文件夹和本机 `activeId`。目前没有同步基线、远端 ETag 或删除记录。保存失败会保留旧内存状态，这个边界需要延续。

因此最小远端布局可以先用一个带协议版本的笔记库 JSON 对象；同步笔记和文件夹，`activeId` 留在本机。远端对象越大，全量传输和冲突概率越高，但目前无需先增加每篇笔记对象、manifest 和跨对象事务。`updatedAt` 继续用于显示/排序，不应用设备时钟决定覆盖胜负。以上是针对本仓库的方案判断。

## 候选对比

| 候选 | 维护与成熟度证据 | S3 兼容 endpoint | 条件写与 ETag | 版本读写 | 本项目判断 |
| --- | --- | --- | --- | --- | --- |
| `object_store` | Apache Arrow 体系；原 InfluxData 项目；现场 Cargo/CHANGELOG 为 0.14.2，CHANGELOG 日期 2026-09-11。[项目](https://github.com/apache/arrow-rs-object-store)、[CHANGELOG](https://github.com/apache/arrow-rs-object-store/blob/main/CHANGELOG.md) | `AmazonS3Builder::with_endpoint`；可选择 virtual-hosted/path 形式。[builder](https://docs.rs/object_store/latest/object_store/aws/struct.AmazonS3Builder.html) | `PutMode::Create` → `If-None-Match: *`；`Update(UpdateVersion)` → `If-Match`；S3 使用 ETag。支持显式配置 `S3ConditionalPut::ETagMatch`；禁用条件写会报不支持，不能降级覆盖。[源码](https://github.com/apache/arrow-rs-object-store/blob/main/src/aws/mod.rs)、[条件写配置](https://docs.rs/object_store/latest/object_store/aws/enum.S3ConditionalPut.html) | `GetOptions.version` 指定读取版本；元数据/PutResult 返回 ETag、版本信息。S3 CAS 不是用 version ID 代替 ETag。[GetOptions](https://docs.rs/object_store/0.14.2/object_store/struct.GetOptions.html)、[PutResult](https://docs.rs/object_store/latest/object_store/struct.PutResult.html) | 首选做最小原型；只启用需要的 S3 功能。 |
| `aws-sdk-s3` | AWS 官方 SDK；现场 GitHub API 最新 SDK 发布是 `release-2026-11-09`，不是所有服务共用一个 crate 版本。[发布](https://github.com/awslabs/aws-sdk-rust/releases/tag/release-2026-11-09)、[开发指南](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html) | 服务 config 有 `endpoint_url`、`force_path_style`。[官方源码](https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/s3/src/config.rs) | PutObject 原生 `if_match`、`if_none_match`，返回 ETag；不必自己拼签名请求。[PutObject](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/operation/put_object/builders/struct.PutObjectFluentBuilder.html) | GetObject 指定 `version_id`，PutObjectOutput 返回 `version_id`；另有 bucket versioning、列举版本等完整 S3 API。[Client](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/struct.Client.html)、[PutObjectOutput](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/operation/put_object/struct.PutObjectOutput.html) | 完整官方路线；当前小型笔记库不一定需要其全部 API 面。未实测二进制/构建成本，不用印象宣称具体体积。 |
| `rust-s3` | MIT 社区项目，仍有发布；GitHub API 最新 `v0.38.0`，2026-10-04。README 同时明确作者投入有波动。[发布](https://github.com/durch/rust-s3/releases/tag/v0.38.0)、[README](https://github.com/durch/rust-s3) | `Region::Custom { region, endpoint }` + `with_path_style`。[Bucket](https://docs.rs/rust-s3/latest/s3/bucket/struct.Bucket.html) | `put_object_builder(...).with_header(...)` 可发送条件头；需要自己映射 412/409、提取 ETag，不如 `PutMode` 直接表达语义。[builder 源码](https://github.com/durch/rust-s3/blob/master/s3/src/put_object_request.rs) | 有额外 query/header 和 attributes 的 version 参数；本次未验证完整的版本列举/恢复 API，不宣称与官方 SDK 等价。[Bucket API](https://docs.rs/rust-s3/latest/s3/bucket/struct.Bucket.html) | 可用，但当前冲突要求下优先级低于前两者。 |
| Apache OpenDAL | Apache 项目，多存储统一接口；GitHub API 最新 `v0.59.5`，2026-10-18。[项目](https://github.com/apache/opendal)、[发布](https://github.com/apache/opendal/releases/tag/v0.59.5) | S3 服务 config 支持 endpoint、region、bucket。[配置源码](https://github.com/apache/opendal/blob/main/core/services/s3/src/config.rs) | 现场 main S3 capability 声明 `write_with_if_match` / `write_with_if_not_exists`；后者编码为 `If-None-Match: *`，前者为 `If-Match`。不能把其他后端能力类推到 S3。[backend](https://github.com/apache/opendal/blob/main/core/services/s3/src/backend.rs)、[请求编码](https://github.com/apache/opendal/blob/main/core/services/s3/src/core.rs) | S3 read/stat 接受 version；源码有版本列举。存储能力依赖 backend 和服务。[core](https://github.com/apache/opendal/blob/main/core/services/s3/src/core.rs) | 成熟替代，适合明确要扩展 WebDAV 等后端；本任务只要 S3，暂不为多后端增加范围。 |

版本口径：以上是查询当天真实返回的版本/日期。搜索缓存曾返回 rust-s3 0.37.2、OpenDAL 0.59.4，随后直接 docs.rs/GitHub API 返回 0.38.0/0.59.5；AWS docs.rs PutObject 页面显示 1.158.0。没有把搜索缓存日期当成发布日期。源码链接 `main/master` 是查询时接口证据，集成时应锁定具体发布版本并核对 API，不能认为分支源码和旧版本完全一致。

## Android / iOS：证据与缺口

不能仅凭“Rust/Tauri 支持移动端”宣称这些库已在本项目 Android/iOS 可用。本次没有做任何目标编译、安装或真机网络测试，也没有找到四个候选对本项目全部移动端组合的正式支持承诺。

- **object_store**：当前 Cargo S3 feature 使用 reqwest/rustls/aws-lc-rs；Rust 最低版本声明为 1.85。已读取的 CI 主要为 Ubuntu/Windows 和 WASM，未见 Android/iOS S3 目标矩阵。[Cargo](https://github.com/apache/arrow-rs-object-store/blob/main/Cargo.toml)、[CI](https://github.com/apache/arrow-rs-object-store/blob/main/.github/workflows/ci.yml)。Android `LocalFileSystem` 曾受 hard-link 限制，相关修复 PR #826 查询时仍 open；它是本地文件后端问题，不等于 S3 后端不能工作，也提醒不要随意把同步基线迁到该后端。[PR #826](https://github.com/apache/arrow-rs-object-store/pull/826)。
- **AWS SDK**：维护者已确认自定义 trust store 功能随 hyper 1.x 升级实现。原问题涉及 iOS/Android 默认根证书加载，这不能被简化为“SDK 不支持移动端”；也不能因 issue 已关闭就当成完整移动端验收。[issue #1171 及维护者回复](https://github.com/awslabs/aws-sdk-rust/issues/1171)、[官方 HTTP/TLS 配置](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/http.html)。现场 README 最低 Rust 为 1.94.1，集成时要与本机工具链核对。[README](https://github.com/awslabs/aws-sdk-rust/blob/main/README.md)。
- **rust-s3**：默认 feature 是 `tokio-native-tls`，另有 rustls feature，不能沿用默认 TLS 就假定 Android 交叉编译成功；已读取 CI 为 Ubuntu 下不同 runtime/TLS 组合，未见移动端目标矩阵。[Cargo](https://github.com/durch/rust-s3/blob/master/s3/Cargo.toml)、[CI](https://github.com/durch/rust-s3/blob/master/.github/workflows/rust.yml)。
- **OpenDAL**：本轮核实了 Rust S3 请求能力，没有取得 Android/iOS 的正式目标保证或本项目编译证据；多语言绑定存在也不能替代 Tauri Rust 后端的移动端验证。[官方仓库](https://github.com/apache/opendal)。

建议移动端若属于本次交付范围，在选型冻结前做最小 spike：使用拟定版本/feature，以项目实际 Android ABI 和 iOS device/simulator target 编译；在至少一台 Android、一台 iOS 检查 HTTPS/证书、签名、条件 PUT 冲突。这是下一步建议，不是本报告已完成项。

## 最小同步层仍需实现什么

以下是面向本项目的设计建议，不是库内置能力：

1. **持久基线**：按 `endpoint/bucket/prefix` 隔离记录“上次成功共同版本”的笔记库和 ETag。初次连接没有基线时，本机/远端已有数据都要保留；不能把现有本机内容直接标成已同步。凭证不放进笔记 JSON/localStorage，也不上传到笔记对象。
2. **三方比较**：以基线 B、本机 L、远端 R 按稳定 note ID 比较。只有一侧改动就采用该侧；不同笔记分别改动可自动合并；两侧改成相同内容不制造冲突；同一篇两侧都改且内容不同则保留双方，生成带来源标记的冲突副本。文件夹按 ID 合并，处理同 ID 改名冲突和笔记的有效 folderId。不要只比较 `updatedAt`，也不先引入自动文本 diff3/CRDT。
3. **安全发布**：读远端内容和 ETag 后，合并结果通过 `PutMode::Update` 条件上传；远端不存在则 `PutMode::Create`。412/409 应重新读取并重新合并，限制重试并显示失败；永远不能回退无条件覆盖。网络失败保留本机笔记和待同步状态。AWS 对条件请求冲突行为有明确说明。[S3 条件写语义](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html)。
4. **提交/编辑竞态**：同步先提交当前编辑器草稿；网络期间继续产生的本机修改必须重新纳入合并，不能用旧请求结果替换新草稿。本机保存/基线更新失败不应宣称成功；远端成功但本机未提交的恢复路径也要可重试。可用一个本地记录保存笔记与同步基线，避免分别写两个 localStorage key 形成部分提交。
5. **状态与验证**：先提供设置、测试连接、手动双向同步和成功/失败/冲突反馈。最小回归覆盖首次两端有数据、单侧更新、不同篇并发、同篇并发保留双方、过期 ETag、网络失败、同步中继续编辑及本机保存失败。服务端必须真的拒绝过期 ETag 和重复创建；不能仅凭 SDK 发出了头就验收兼容。

当前没有笔记删除 API；第一版不必先实现删除。将来新增删除时，必须引入 tombstone 或可靠删除基线和冲突规则，不能把“本地未出现”一律当成应该删除远端。

S3 bucket versioning 可作为额外恢复手段，不是并发合并策略，也不应成为基础同步必须开启的前提。ETag 当作服务端返回的并发令牌原样保存，不能自己假定它总是内容 MD5。[S3 ETag/校验和说明](https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity-upload.html)。S3-compatible endpoint 是否实现同样的条件写、版本和签名行为，需要针对实际服务验收；本次没有进行服务兼容测试。

## 是否有现成双向同步引擎可直接复用

这类项目包含同步算法，需要与仅提供对象读写的客户端分别比较：

| 候选 | 已核实能力 | 当前项目适配结论 |
| --- | --- | --- |
| MDSync (`k1tbyte/mdsync`) | MIT；已实现共同基线三方行合并、人工合并编辑器、保留双方和版本历史；1.5.1 发布于 2026-11-03。[发布](https://github.com/k1tbyte/mdsync/releases/tag/1.5.1)、[合并源码](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/apply/conflict-merge.ts) | 当前最值得验证提取的完整引擎候选，但仍是 private Obsidian 插件，需适配文件 IO、网络和本地模型；manifest 保护不是原子 CAS。详见下一节。 |
| Awan (`risangbaskoro/awan`) | MIT；使用本地 snapshot 作为共同基线，diff-match-patch 产生并应用补丁；另有冲突副本策略。[冲突源码](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/src/utils/conflict.ts) | 0.1.0 Obsidian 插件；merge 分支忽略 `cleaned` 后覆盖本地和远端，不推荐直接复用为可靠冲突引擎。 |
| Third-party Sync (`nightfall-yl/obsidian-third-party-sync`) | Apache-2.0；现场 package/manifest 26.2.8，`prevSync` 检测双边改动，策略为 keep_newer/keep_larger。[同步源码](https://github.com/nightfall-yl/obsidian-third-party-sync/blob/3f0206ddb88eae97543704bd81ddcc4144ab1fba/src/sync.ts)、[许可证](https://github.com/nightfall-yl/obsidian-third-party-sync/blob/3f0206ddb88eae97543704bd81ddcc4144ab1fba/LICENSE) | 有历史比较不等于三方文本合并；S3 层依赖 Obsidian Vault/requestUrl/Platform，扫描该文件未见 IfMatch/IfNoneMatch。[S3 源码](https://github.com/nightfall-yl/obsidian-third-party-sync/blob/3f0206ddb88eae97543704bd81ddcc4144ab1fba/src/remoteForS3.ts)。 |
| rclone bisync | 真正的双向文件树同步，默认冲突保留双方并改名，也可选择较新版本优先。v1.66 起采用 snapshot 降低运行期文件变动风险，但读写瞬间变动仍可能出错。[官方文档](https://rclone.org/bisync/) | 可用于文件夹/CLI 产品；本项目没有本地 Markdown 文件树，直接用于整库 JSON 无法理解 note ID 或分别合并笔记。还要增加进程/文件桥接和移动端嵌入验证，当前并非最小路线。 |
| Remotely Save | 开源主体 `src/tests/docs/assets` 为 Apache-2.0，`pro` 为 PolyForm Strict；basic 为 newer/larger 优先，PRO 才包含小型 Markdown 合并和其他文件冲突副本。[许可证](https://github.com/remotely-save/remotely-save/blob/master/LICENSE)、[PRO 说明](https://github.com/remotely-save/remotely-save/blob/master/pro/README.md) | Obsidian 插件，非独立通用 SDK；不能按主体许可证直接搬入 PRO 智能冲突代码。算法 v3 intro 中的未勾选清单不能当成功能已交付证据。 |
| livesync-commonlib | MIT，pre-1.0；README 明确它不是 general-purpose SDK，没有稳定 client factory，`createLiveSyncFileClient` 仍是 planned。[README](https://github.com/vrtmrz/livesync-commonlib) | 冲突解析仍要求共同祖先，UI 和持久化由宿主负责；并不是装包就能替代当前模型的同步层。[冲突文档](https://github.com/vrtmrz/livesync-commonlib/blob/main/docs/conflict-resolution.md)。 |
| `diffy` | Rust 三方文本合并 `merge(ancestor, ours, theirs)`，以 Ok/Err 区分干净合并和冲突；查询文档版本 0.5.2。[API](https://docs.rs/diffy/latest/diffy/fn.merge.html) | 是文本辅助算法，不提供 S3/基线/重试/同步。首版可先保留冲突副本；未来明确要减少手动处理时再接入。 |

因此不应再表述成“没有现成同步引擎”。准确结论是：有可复用研究的实现，MDSync 尤其接近目标；本次仍未确认可直接嵌入当前 Tauri/localStorage 模型的成熟通用 SDK。先验证提取成本，再决定复用范围。

## MDSync 与 Awan 的源码复核

本节固定查询时 commit：MDSync `6f0190efd517b709ff21c4d77bcc0b221bce52b6`；Awan `a5b3c56e719b71851a4b264126bd6c439f23f40d`。两者仓库 LICENSE 均已现场读取为 MIT；同名网页/PyPI 项目不在此结论范围。Context7 对新候选的一次 resolve 只返回无关结果，没有命中 MDSync/Awan，因此依据官方仓库源码，而非以无关库文档补证。

**MDSync 的三方合并不是单纯拼接 local/remote。** `tryAutoMergeConflict` 要求 baseline/local/remote hash 并加载三份内容；`threeWayRegions` 将相对于 base 的行变化组成区域，双方改同一区域且内容不同才判冲突；`mergeNote` 发现未解决项返回 null，不自动落盘。无共同祖先也不自动合并。[三方加载](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/apply/conflict-merge.ts)、[区域算法](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/hunks/merge-model.ts#L64)。保留双方会寻找未占用副本路径，避免同名副本被覆盖。[副本命名](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/apply/conflict-copy.ts)。

**MDSync 的并发保护需补验证，不能等同原子 CAS。** `publishManifestWithGuard` 先重读父 snapshotId，随后无条件 `storage.put`，再读回确认；S3 `putIfAbsent` 确有 `If-None-Match: *`，但 manifest 发布走的是普通 put。[manifest 第124–165行](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/remote/manifest.ts#L124)、[S3 请求](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/storage/backends/s3-store.ts)。源码推论：A/B 都通过前置检查，A 写入并确认成功之后 B 才覆盖并确认，双方可能都认为成功；这不是已运行复现的缺陷结论。提取前应以对抗并发测试确认，并为 S3 manifest 补原子 ETag CAS 或等价提交协议。

MDSync 有发布、Vitest 合并/竞态测试，CI 执行 build/lint/typecheck/arch/test，本轮未运行外仓测试。[合并测试](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/tests/features/sync/apply/auto-merge.test.ts)、[CI](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/.github/workflows/lint.yml)。`EngineDependencies` 仍依赖 Obsidian DataAdapter 等接口；package 为 `private:true`，非发布的通用 SDK。[引擎接口](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/src/features/sync/engine.ts#L48)、[package](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/packages/plugin/package.json)。manifest 的 `isDesktopOnly:false` 表示支持意图；E2E 文档明确手机只是宽度模拟，不能当 Android/iOS 真机验证。[E2E 边界](https://github.com/k1tbyte/mdsync/blob/6f0190efd517b709ff21c4d77bcc0b221bce52b6/tests/e2e/README.md#L47)。

**Awan 也有共同 snapshot，但没有同等的冲突失败处理。** `performMerge` 将 base→remote 的 diff-match-patch 补丁应用到 local，返回 `cleaned`；调用方只取 `content`，无论补丁是否全部成功都会写入本地并上传远端。冲突副本是另一项策略，不是此自动合并失败后的可靠回退。非 Markdown 在 merge 策略下退回时间较新者优先。[算法](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/src/utils/conflict.ts)、[忽略 cleaned 的调用，第181–219行](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/src/commands/sync.ts#L181)。S3 `_write` 的 Upload 参数未提供 IfMatch/IfNoneMatch，不能认为读后写受 CAS 保护。[S3 write](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/src/filesystems/s3.ts#L430)。这些是源码核对，未运行外部仓库复现。

Awan 为 0.1.0，GitHub latest release API 返回 404；package 无 test 脚本，CI 仅 build/lint。它仍依赖 Obsidian App/Vault/requestUrl；manifest `isDesktopOnly:false` 不代表本项目移动端已验收。[package](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/package.json)、[manifest](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/manifest.json)、[CI](https://github.com/risangbaskoro/awan/blob/a5b3c56e719b71851a4b264126bd6c439f23f40d/.github/workflows/lint.yml)。

## 本轮验证边界

已完成：读取当前仓库模型；通过 Context7 查询三个主要 Rust 候选；直接核对官方 API、源码、发布记录和上述移动端问题讨论；报告内区分事实、方案判断和未验证项。

未完成：编译/体积测量、Android/iOS 目标构建与真机、实际 bucket/第三方服务条件写测试、凭证存储实现、完整同步引擎代码。没有安装库或启动开发。
