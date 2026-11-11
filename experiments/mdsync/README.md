# MDSync 隔离验证

固定上游：`k1tbyte/mdsync@6f0190efd517b709ff21c4d77bcc0b221bce52b6`（1.5.1，MIT）。这不是正式同步入口，不读取当前应用 localStorage，也不连接真实 bucket。

在本项目旁准备独立 checkout（已有目录时不要覆盖）：

```sh
git clone https://github.com/k1tbyte/mdsync.git ../mdsync-validation
git -C ../mdsync-validation checkout --detach 6f0190efd517b709ff21c4d77bcc0b221bce52b6
cd ../mdsync-validation
pnpm install --frozen-lockfile --ignore-scripts
cd ../MiaoYanEditor
node experiments/mdsync/run.mjs
```

可用 `node experiments/mdsync/run.mjs /absolute/path/to/mdsync-checkout` 指定上游目录。Node 24，pnpm 10.17.0；运行器检查 commit，用上游自己的 Vitest 和依赖运行测试，完成后删除临时测试文件。结果写入 `artifacts/mdsync-spike/probe-results.json`。本项目 package/lockfile 不变。

测试直接调用 MDSync 的 compare/push/pull/autoMerge/keepBoth/history 和 S3 adapter，没有复制这些算法。远端采用上游内存存储，以及上游 S3 HTTP fixture（仅将监听改为临时 loopback 端口）。临时测试文件使用 `wx` 创建，不覆盖已有文件。

`KNOWN LIMITATION` 测试的通过表示复现了并发覆盖窗口，不能算同步安全通过：两端通过前置检查，第一端发布并确认后第二端再发布，双方都报告成功，但当前 HEAD 只保留其中一篇新增笔记。内容对象未必被删除；这是索引丢更新。

边界：Notebook 测试只验证单篇 Markdown 正文通过现有保存接口往返，尚未实现整个笔记库的导入映射、文件夹/元数据同步、状态落盘及凭证存储。HTTP fixture 不校验 SigV4 签名，不代表 AWS/R2/MinIO 验收；Node fetch 也不代表 Tauri 的网络、CORS 或证书验证。未测试 UI、手机、真实云服务，未启用自动同步。
