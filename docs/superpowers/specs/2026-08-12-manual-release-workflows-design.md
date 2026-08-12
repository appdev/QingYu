# QingYu 手动发布工作流设计

## 目标

建立一套仅允许手动触发的发布流程，使 GitHub Release 与 Docker 镜像能够独立运行或由统一入口并行运行，并通过同一个 `prerelease` 参数保持发布语义一致。

## 范围

- 将 `CD For QingYu` 改为仅手动触发、同时可被其他 workflow 调用的可复用 workflow。
- 恢复 `Release Docker Image`，目标限定为 `apkdv/qingyu`，并改为仅手动触发、同时可复用。
- 新建统一手动发布 workflow，并行调用 CD 与 Docker workflow。
- 删除 `lock.yml` 的定时触发，使仓库内所有 workflow 都只能由用户手动启动。
- 不恢复 AUR 发布和 PR 分支自动改向工作流。

## 触发模型

仓库中的每个顶层 workflow 仅声明 `workflow_dispatch`。需要被统一入口调用的 CD 与 Docker workflow 额外声明 `workflow_call`；`workflow_call` 不是外部事件自动触发器，只允许另一个已由用户手动启动的 workflow 调用。

三个发布入口如下：

1. `CD For QingYu`：单独手动构建桌面端、Android 并创建 GitHub Release。
2. `Release Docker Image`：单独手动构建 Docker；是否推送由 `prerelease` 决定。
3. `Release QingYu`：统一手动入口，并行调用前两个 workflow。

每个发布入口均提供必填布尔参数 `prerelease`，默认值为 `true`，以降低误发正式版本和覆盖 `latest` 的风险。

## 版本规则

版本来源唯一为 `app/package.json` 的 `version` 字段，Release tag 与 Docker 版本标签统一派生为 `v<version>`。

- `prerelease: true` 时，版本必须匹配 `x.y.z-alpha...`、`x.y.z-beta...` 或 `x.y.z-rc...`。
- `prerelease: false` 时，版本必须严格匹配 `x.y.z`。
- 不接受 `dev`、任意未识别后缀、缺失版本段或 tag 与包版本分离的状态。

校验在耗时构建开始前执行。校验失败时 workflow 直接终止，不创建 Release，也不尝试登录或推送 Docker Hub。

## CD For QingYu

`CD For QingYu` 保留现有桌面端、Android、产物聚合和 Release 创建逻辑，并将发布类型改为使用传入的 `prerelease`。

- `prerelease: true` 创建 GitHub Pre-release。
- `prerelease: false` 创建正式 GitHub Release。
- tag 始终使用 `v<package version>`，并明确指向当前 workflow 使用的提交。
- 手动直接运行与统一入口调用共享相同实现，不复制构建逻辑。
- Android 签名继续使用仓库中已配置的四项 `ANDROID_*` Secret。

## Release Docker Image

Docker workflow 的镜像目标固定为 `apkdv/qingyu`，不允许出现 `b3log/siyuan` 或其他上游发布目标。

所有模式均执行以下平台构建：

- `linux/amd64`
- `linux/arm64`
- `linux/arm/v7`
- `linux/arm/v8`

发布行为：

- `prerelease: true`：完成多架构 BuildKit 构建验证，不登录 Docker Hub，不导出 registry 镜像，不创建或更新远程 tag。
- `prerelease: false`：使用 `DOCKER_HUB_USER` 与 `DOCKER_HUB_PWD` 登录 Docker Hub，并推送 `apkdv/qingyu:v<version>` 与 `apkdv/qingyu:latest`。

Docker 构建不依赖 CD 成功；在统一入口中两者并行执行。统一入口的整体结果只有在两个被调用 workflow 都成功时才成功。

## Release QingYu 统一入口

统一入口只包含参数声明与两个 reusable workflow job：

- 将同一个 `prerelease` 值传给 CD。
- 将同一个 `prerelease` 值传给 Docker。
- 使用 `secrets: inherit` 将仓库 Actions Secrets 传给两个子 workflow。
- 不复制构建、版本或发布逻辑。

## 权限与安全

- CD 仅申请创建 GitHub Release 所需的 `contents: write`。
- Docker 仅申请读取仓库所需的 `contents: read`；Docker Hub 凭据仅在稳定发布步骤中注入。
- workflow 与日志不得打印 keystore、签名密码、Docker token 或其 Base64 内容。
- Docker 预发布构建不得执行登录步骤，避免无发布需求时暴露外部凭据。
- 所有发布 job 限定仓库为 `appdev/QingYu`，防止 fork 手动运行后误用发布逻辑。

## 失败处理

- 版本不符合所选发布类型：在 prepare/validate 阶段失败。
- CD 任一平台失败：不创建 GitHub Release。
- Docker 预发布构建失败：不产生远程镜像，workflow 失败。
- Docker 稳定发布失败：不宣称发布完成；已成功推送的单个平台 manifest 或 tag 由 Buildx 的 registry 输出语义处理，不进行自动删除。
- 统一入口中一个子 workflow 失败不会主动取消另一个，以保留完整构建证据；最终统一入口标记为失败。

## 验证

实施后执行以下检查：

1. 解析所有 `.github/workflows/*.yml`，确认 YAML 有效。
2. 使用自动化断言检查三个发布入口的 `prerelease` 参数、调用关系、Release 标志和 Docker push 条件。
3. 搜索全部 workflow，确认不存在 `push`、`release`、`schedule`、`pull_request` 或 `pull_request_target` 自动触发器。
4. 搜索全部 workflow，确认不存在 `b3log/siyuan`、`siyuan-note` 所有者限制或上游 AUR 发布目标。
5. 运行 `cd app && pnpm test`。
6. 运行 `cd app && pnpm run lint`。
7. 推送后分别以 `prerelease: true` 验证不发布 Docker，以及在正式发布获明确授权时以 `prerelease: false` 验证 Release 与 Docker tag。

## 非目标

- 不自动创建或推送 Git tag。
- 不根据分支、tag 或 GitHub Release 事件自动执行任何 workflow。
- 不恢复 AUR 发布。
- 不把 Docker 构建合并到 CD 实现内部。
- 不修改应用运行时代码、Android 原生仓库或 Dockerfile 的产品行为。
