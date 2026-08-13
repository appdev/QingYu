# QingYu 品牌与法律边界任务记录

- 类型：task_start、context、decision
- 风险等级：L3
- 日期：2026-08-13
- 目标：清除 QingYu 发行树中把 SiYuan 名称、Logo、下载、更新、协议和服务作为当前产品身份使用的风险，同时保留 AGPL、上游版权归属及必要兼容标识。
- 已确认决策：采用分层整改；`logo.png` 是唯一品牌图形主源；重建四套精简 QingYu 指南；编写四语言 QingYu 隐私政策与用户协议；协议署名使用 GitHub 开发者 `appdev` 和公开邮箱 `lengyue@apkdv.com`；以美国加利福尼亚州法为设计基准，不采用强制仲裁；无自有服务承接时隐藏或禁用入口。
- 当前证据：SiYuan 官方应用更新器仍可下载上游安装包；官网、下载、协议和支持链接仍指向上游；135 个内置 `.sy` 指南文件包含 366 行 `SiYuan/思源`；旧 SiYuan Logo 仍由产品菜单消费。
- 初始边界：不提交、不推送、不发布、不部署；不破坏 `.sy`、`window.siyuan`、旧 URI 和上游版权溯源；隐私与法律文本必须建立在代码审计和可验证公开身份上。
- 恢复入口：`docs/superpowers/specs/2026-08-13-qingyu-brand-and-legal-isolation-design.md`。
- 最终授权：用户已明确授权提交并推送当前完整整改范围，不创建 PR、不发布、不部署。
- 最终验证：前端 209 项测试通过，类型检查与 ESLint 通过；图标、指南、法律文本生成检查和品牌审计通过；21 个语言文件键一致；Go `model`、`api`、`conf`、`util` 测试通过；`git diff --check` 通过。
