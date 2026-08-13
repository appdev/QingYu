# QingYu v1.0.0 发布记录

## 任务开始

- 日期：2026-08-13
- 目标：将 QingYu 版本统一为 `1.0.0`，提交并推送当前工作区，通过稳定版工作流重新打包 GitHub Release 与 Docker 镜像，随后删除旧 `v3.7.3` Release、Git 标签和 Docker 标签。
- 发布仓库：`appdev/QingYu`
- Docker 仓库：`apkdv/qingyu`

## 上下文

- 发布前 GitHub 最新版本为 `v3.7.3`，对应标签提交为 `794ec1b5fc9f693938fd56a591fc792a9bd2dfcb`。
- 发布前 Docker Hub 标签为 `latest` 与 `v3.7.3`。
- `app/package.json` 是发布工作流读取的版本源，`kernel/util/working.go` 是运行时版本源，两者必须保持一致。
- 项目规则禁止本地执行 `pnpm build`；正式产物由 GitHub Actions 的 QingYu Release 工作流构建。

## 决策

- 先成功发布并验证 `v1.0.0`，再删除旧版本，避免破坏发布工作流计算变更记录时使用的上一版本基线。
- 稳定版工作流参数使用 `prerelease=false`，预期生成 GitHub Release `v1.0.0` 以及 Docker 标签 `latest`、`v1.0.0`。
- GitHub 旧 Release 与标签使用 `gh` 删除；Docker Hub 旧标签按用户要求通过 Chrome 删除，并在永久删除前执行最终确认。

## 验证

- `cd app && pnpm run lint`：退出码 `0`，依赖锁检查、TypeScript 类型检查和 ESLint 通过。
- `cd app && pnpm test`：退出码 `0`，`200/200` 通过，失败、跳过和取消均为 `0`。
- `cd app && pnpm run test:markdown-layout`：退出码 `0`，代码块语言位置、操作组位置、首行偏移、高度、背景、圆角、字体和语法颜色均与原生代码块一致。
- `cd app && pnpm run test:markdown-appearance`：在连接运行中 QingYu 内核的独立 Electron 客户端上退出码 `0`，6 行矩阵、51/51 契约、7 张截图，未覆盖项为空，最大受控几何差异为 10px。
- `cd app && pnpm run test:markdown-live-preview`：退出码 `0`，真实应用源码/视觉模式切换、同一 `EditorView`、文档长度与选择行为通过。
- `cd app && pnpm run gen:code-languages`：退出码 `0`，重新生成 200 种代码语言目录。
- `cd kernel && go test ./util`：退出码 `0`。
- GitHub Actions 工作流 YAML 解析：4 个工作流均通过。
- `git diff --check`：退出码 `0`，无输出。
- 版本一致性：`app/package.json` 与 `kernel/util/working.go` 均为 `1.0.0`。
- 未运行本地生产构建；GitHub Actions 的正式发布结果将在完成后补充。

## 交接

最终发布地址、工作流、提交、产物和旧版本删除结果将在执行完成后补充。
