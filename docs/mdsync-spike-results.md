# MDSync 接入前验证

日期：2026-11-11，Asia/Shanghai。采用 MDSync 1.5.1，固定 commit `6f0190efd517b709ff21c4d77bcc0b221bce52b6`。上游独立 checkout：`/Users/evan/Downloads/public/mdsync-validation`；Node v24.22.2、pnpm 10.17.0，依照其锁文件安装。

## 结果与采用建议

**MDSync 的同步、合并、冲突副本和历史核心值得复用；本轮发现的并发发布问题应在接入真实笔记前解决。** 本次直接调用上游核心，没有复制合并算法，没有将其注册进应用启动流程，也没有操作现有笔记或用户 bucket。手机测试仍暂停。

上游 6 个测试文件共 **99 项通过**；另写 10 项本项目验证，其中 **9 项功能验证通过，1 项确认了已知安全限制**。后者通过不代表风险消失。

| 项目验证 | 结果 |
| --- | --- |
| 通过现有 Notebook 保存接口往返中文、emoji、CRLF、表格、代码、数学及 Mermaid 源文 | 正文逐字保留；本机文件夹和选中项未改变 |
| 两端修改同一篇不同段落 | 自动合并，推送/拉取后两端一致 |
| 两端修改同一段 | 不自动覆盖；选择 keep both 后双方版本同步到两端 |
| 首次同步同路径已有不同内容、没有共同基线 | 保持冲突，两边原文保留 |
| 已有同名冲突副本 | 另取空闲名称，旧副本保留 |
| 合并下载期间继续输入 | 新输入保留，不写入过时合并结果 |
| 上传失败后重试 | 原文本及旧基线保留，重试后另一端得到新内容 |
| 开启历史后再次修改 | 两次历史记录可读，旧版本正文仍可取回 |
| 原 S3 adapter + loopback HTTP | 正文往返、前缀隔离、密文对象、仅创建写入及 403 拒绝通过 |
| 两端同时推送不同的新笔记 | **复现限制：双方报告成功，当前 HEAD 只保留其中一篇新笔记** |

## 并发限制的复现

两端先持有相同基线，各自新增一篇笔记，然后：

1. 两端比较并都通过发布前的父 snapshot 检查。
2. 第一端发布索引并读回自己的 snapshot，返回成功。
3. 第二端才执行已经通过检查的无条件索引 PUT，并读回自己的 snapshot，也返回成功。
4. 最终 HEAD 只有第二次发布的新增笔记。第一端的内容对象未必删除，但它的新笔记已经不在当前索引里。

测试用确定性的 Promise 屏障安排交错，不靠计时或随机碰撞。调用的是上游原始 `pushPathsOp` 和 `publishManifestWithGuard`；未修改生产实现。正式接入至少需要为索引发布增加原子 ETag 条件更新（或等价的并发提交协议），让第二端得到明确冲突后重新比较、合并、重试。

## 可重跑的入口

见 [实验说明](../experiments/mdsync/README.md)，运行 `node experiments/mdsync/run.mjs`。运行器检查固定 commit 及干净的已跟踪文件；临时注入测试和 HTTP fixture，完成后清除。fixture 仅将上游监听改为 loopback 临时端口；上游生产文件不变。机器可读结果位于 `artifacts/mdsync-spike/probe-results.json`；实际代码输入哈希记录于 `artifacts/mdsync-spike/verified-inputs.json`。

上游测试命令（在独立 checkout）：

```sh
pnpm --filter mdsync test tests/features/sync/engine-roundtrip.test.ts tests/features/sync/apply/auto-merge.test.ts tests/features/sync/hunks/merge-model.test.ts tests/features/sync/remote/manifest-guard.test.ts tests/features/storage/backends/s3-adapter.test.ts tests/features/storage/backends/s3-signer.test.ts
```

## 尚未覆盖

- Notebook 测试仅验证正文适配，不是完整 Library 同步实现；未知笔记的导入、文件夹/元数据合并、整库事务和同步基线落盘尚未接入。
- HTTP 服务是上游的内存 S3 fixture，不校验 SigV4；不能代替真实 AWS、R2、MinIO 的签名、HTTPS、条件请求验收。
- Obsidian 运行时使用上游 stub，网络调用替换为 Node fetch；未验证 Tauri WebView/原生网络、CORS、凭证存储或移动端。
- 未做应用打包、安装、UI 和真实多设备测试；未启用自动同步。现有生产源码及 package/lockfile 无改动。
