# QingYu v1.0.1 发布记录

- 类型：task_start、context、decision
- 风险等级：L3
- 日期：2026-08-14
- 目标：将 QingYu 从 `1.0.0` 升级到稳定版 `1.0.1`，提交并推送全部本地修改，执行正式 Release Actions，并核验发布结果。
- 当前上下文：远端为 `appdev/QingYu`，分支为 `main`；执行前本地领先 `origin/main` 1 个提交，并存在 Markdown 编辑、外观、图标、兼容版本和测试相关的未提交修改。
- 已确认决策：升级 `app/package.json` 与 `kernel/util/working.go` 的版本，将本地更新日志占位同步到 `v1.0.1`；验证后用一个提交纳入全部修改；推送 `main`；以 `prerelease=false` 触发 `.github/workflows/release.yml`。
- 发布影响：工作流将创建公开 GitHub Release、上传桌面端与 Android 安装包，并向 Docker Hub 推送 `latest` 与 `v1.0.1` 镜像。
- 验证边界：不运行仓库禁止的 `pnpm build`，不编译或重启内核二进制；使用前端测试、lint、Markdown 专项检查、品牌与图标检查、Bazaar Go 测试和 Git 差异检查作为发布门。
- 类型：verification、test
- 本地验证：`pnpm run lint` 通过；`pnpm test` 共 253 项全部通过；`pnpm test:markdown-layout` 通过；`pnpm test:brand` 与 `pnpm check:qingyu-icons` 通过；`go test ./bazaar` 通过；`git diff --check` 通过；版本源均为 `1.0.1`；差异中未发现凭据或调试残留。
- 未验证项：`pnpm test:markdown-appearance` 依赖运行中的开发内核 `127.0.0.1:9806` 和 Electron 调试宿主 `127.0.0.1:9222`；两者均未运行，且仓库规则禁止代理编译或重启内核，因此本次未完成该运行时矩阵检查。相关静态与单元测试已包含在 253 项前端测试中。
- 类型：handoff
- 提交边界：冻结候选树包含执行前全部本地修改、`1.0.1` 版本升级、`v1.0.1` 更新日志占位和本记录；提交后仅执行推送、Release Actions、Release Notes 更新与远端状态核验，不再改变发布目标树。
