# S3 集成验证结果

日期：2026-11-11。使用用户明确授权的可丢弃测试 Bucket，仅操作本轮独立测试前缀。凭证由仓库外 0600 临时文件提供，未写入源码、构建资源或报告；结束后删除临时凭证文件。

## 已完成

- MDSync 核心固定在 `6f0190efd517b709ff21c4d77bcc0b221bce52b6`；提取包约 93.6 KB，构建出的同步界面分包约 87.3 KB（gzip 31.5 KB），点击入口时加载。
- 条件提交：索引初建使用 `If-None-Match: *`，更新使用 ETag `If-Match`；过期写入被拒绝后重新比较并合并，缺少 ETag 不降级覆盖。
- 完整库映射：正文、稳定 ID、文件夹/空文件夹、创建与修改时间、本机选中项，以及当前模型的名称、置顶、废纸篓和冲突标记。当前选中项不上传。
- 本机事务：内容和同步基线同时落盘。远端已经提交但本机保存失败时保留旧本机状态，下一次同步可恢复，不制造多余副本。

## 运行证据

| 验证 | 结果 |
| --- | --- |
| `npx vitest run tests/` | 60 项通过，真实云端测试默认跳过 |
| 新增的远端提交成功/本机落盘失败用例 | 单独运行通过；生产同步代码未因此改变 |
| 真实 S3 集成 `tests/sync-cloud.test.ts` | 通过；164 次请求，3 次预期的并发/重复创建拒绝 |
| 同时两端新增笔记 | 真实服务拒绝过期提交，重试后两篇笔记均保留并收敛 |
| 真实服务中的内容与冲突 | CRLF、中文、emoji、Markdown、空文件夹、三方合并、冲突保留、幂等重复同步及保存失败恢复通过 |
| Rust 原生 HTTP `sync_http::tests` | 3 项通过；包含真实 HTTPS GET、条件更新、过期 ETag 412、重复创建 412，以及网络目标限制 |
| `npm run tauri -- build --bundles app` | 前端类型检查、Vite 和 Rust release 构建通过；生成 macOS app（约 16.79 MiB） |
| 原生桌面 UI | 隔离实例成功上传；独立客户端新增笔记并修改正文，桌面再次同步后列表找到新增笔记，编辑器确认远端追加文字 |
| 凭证检查 | 源码、测试、工具、文档、构建资源与 JSON 报告未检出测试凭证 |

机器可读结果：`artifacts/mdsync-spike/live-results.json`、`desktop-witness.json`。同步代码及测试输入哈希记录：`integrated-tree.json`。原生界面截图：`native-sync-success.webp`。

## 数据处理与交付边界

自动化验证使用 `miaoyan-validation/20261111-3dc6e365-3bda-4e29-a498-d5b045596c57` 及其 `-cas`、`-native` 前缀，均已清空。本轮未清空 Bucket，未操作其他前缀。

`-app` 前缀保留两篇合成演示笔记，供复查原生桌面双端同步。桌面验证使用同一 release 包的隔离副本，单独的 Bundle ID `app.miaoyan.editor.s3qa.20261111`，没有替换用户原有本机笔记库。正式包位于 `src-tauri/target/release/bundle/macos/MiaoYan Editor.app`。

当前提供手动同步，凭证仅当前进程有效；密钥采用 Bucket 权限保护，未提供独立端到端加密。未恢复手机测试，未验证 Android/iOS 的网络与打包；未增加后台同步和历史浏览 UI。操作方式和协议见 [S3 同步说明](s3-sync.md)。
