# QingYu Manual Release Workflows Implementation Plan

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 QingYu 的 GitHub Release、Docker 构建和锁帖工作流全部改为仅手动运行，并通过一个带 `prerelease` 参数的统一入口协调 Release 与 Docker。

**Architecture:** `cd.yml` 与恢复后的 `dockerimage.yml` 同时提供 `workflow_dispatch` 和 `workflow_call`，各自拥有完整实现并接受同名布尔输入 `prerelease`。新建 `release.yml` 作为仅手动调度层，使用本地 reusable workflow 调用并通过 `secrets: inherit` 传递仓库 Secret；`lock.yml` 删除定时触发。

**Tech Stack:** GitHub Actions reusable workflows、Docker Buildx、Docker Hub、Node.js test runner、TypeScript、`yaml` 2.9.0。

## Global Constraints

- 所有顶层 workflow 只能通过 `workflow_dispatch` 手动启动；`workflow_call` 仅用于被统一入口调用。
- 每个发布入口的 `prerelease` 均为必填 boolean，默认 `true`。
- `prerelease: true` 仅允许 `x.y.z-alpha...`、`x.y.z-beta...`、`x.y.z-rc...`，Docker 只构建不登录、不推送。
- `prerelease: false` 仅允许严格的 `x.y.z`，Docker 推送 `apkdv/qingyu:v<version>` 与 `apkdv/qingyu:latest`。
- Release tag 只能由 `app/package.json` 派生为 `v<version>`，不得自动创建或推送 Git tag。
- 不恢复 AUR 和 PR 分支改向 workflow，不修改 Dockerfile 或应用运行时代码。
- 不输出 Android keystore、签名密码、Docker Hub token 或 Base64 内容。
- 未获得新的明确授权前不执行 `git commit`、`git push` 或正式发布。

---

### Task 1: 建立 workflow 合约回归测试

**Files:**
- Create: `app/src/util/qingyuWorkflows.test.ts`

**Interfaces:**
- Consumes: `.github/workflows/cd.yml`、`.github/workflows/dockerimage.yml`、`.github/workflows/release.yml`、`.github/workflows/lock.yml` 的 YAML 文本。
- Produces: 对手动触发、reusable workflow 输入、版本校验、Release 类型、Docker push 条件和统一调用关系的自动化约束。

- [ ] **Step 1: 新建测试工具并写 CD 合约测试**

```ts
import * as assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import {join} from "node:path";
import test from "node:test";
import {parse} from "yaml";

const repositoryRoot = join(__dirname, "../../..");
const readWorkflow = async (name: string) => {
    const source = await readFile(join(repositoryRoot, `.github/workflows/${name}`), "utf8");
    return {source, workflow: parse(source) as Record<string, unknown>};
};

test("CD is manual, reusable, and publishes the requested release type", async () => {
    const {source, workflow} = await readWorkflow("cd.yml");
    const triggers = workflow.on as Record<string, {inputs?: Record<string, unknown>}>;
    assert.deepEqual(Object.keys(triggers).sort(), ["workflow_call", "workflow_dispatch"]);
    assert.equal((triggers.workflow_dispatch.inputs?.prerelease as {type: string}).type, "boolean");
    assert.equal((triggers.workflow_call.inputs?.prerelease as {type: string}).type, "boolean");
    assert.match(source, /prerelease: \$\{\{ inputs\.prerelease \}\}/);
    assert.match(source, /RELEASE_TAG="v\$VERSION"/);
    assert.match(source, /\^\[0-9\]\+\\\.\[0-9\]\+\\\.\[0-9\]\+\$/);
    assert.match(source, /-\(alpha\|beta\|rc\)/);
});
```

- [ ] **Step 2: 写 Docker 合约测试**

```ts
test("Docker builds prereleases without publishing and pushes stable images", async () => {
    const {source, workflow} = await readWorkflow("dockerimage.yml");
    const triggers = workflow.on as Record<string, {inputs?: Record<string, unknown>}>;
    assert.deepEqual(Object.keys(triggers).sort(), ["workflow_call", "workflow_dispatch"]);
    assert.match(source, /docker_hub_owner: "apkdv"/);
    assert.match(source, /docker_hub_repo: "qingyu"/);
    assert.match(source, /if: \$\{\{ !inputs\.prerelease \}\}/);
    assert.match(source, /push: \$\{\{ !inputs\.prerelease \}\}/);
    assert.match(source, /apkdv\/qingyu:latest/);
    assert.match(source, /apkdv\/qingyu:v\$\{\{ steps\.version\.outputs\.value \}\}/);
    assert.doesNotMatch(source, /b3log\/siyuan|github\.repository_owner == 'siyuan-note'/);
});
```

- [ ] **Step 3: 写统一入口与全仓库手动触发测试**

```ts
test("release dispatcher passes prerelease to both reusable workflows", async () => {
    const {source, workflow} = await readWorkflow("release.yml");
    const triggers = workflow.on as Record<string, unknown>;
    assert.deepEqual(Object.keys(triggers), ["workflow_dispatch"]);
    assert.match(source, /uses: \.\/\.github\/workflows\/cd\.yml/);
    assert.match(source, /uses: \.\/\.github\/workflows\/dockerimage\.yml/);
    assert.equal(source.match(/prerelease: \$\{\{ inputs\.prerelease \}\}/g)?.length, 2);
    assert.equal(source.match(/secrets: inherit/g)?.length, 2);
});

test("every workflow has no automatic event trigger", async () => {
    for (const name of ["cd.yml", "dockerimage.yml", "release.yml", "lock.yml"]) {
        const {workflow} = await readWorkflow(name);
        const triggerNames = Object.keys(workflow.on as Record<string, unknown>);
        assert.ok(triggerNames.includes("workflow_dispatch"), name);
        assert.deepEqual(triggerNames.filter((trigger) => trigger !== "workflow_dispatch" && trigger !== "workflow_call"), [], name);
    }
});
```

- [ ] **Step 4: 运行新测试并确认按预期失败**

Run: `cd app && node --import tsx --test src/util/qingyuWorkflows.test.ts`

Expected: FAIL，因为 `dockerimage.yml` 与 `release.yml` 尚不存在、`cd.yml` 尚无 `workflow_call/prerelease` 合约、`lock.yml` 仍有 `schedule`。

---

### Task 2: 将 CD For QingYu 改为手动 reusable workflow

**Files:**
- Modify: `.github/workflows/cd.yml`
- Test: `app/src/util/qingyuWorkflows.test.ts`
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Consumes: boolean `inputs.prerelease`，Android 四项 `ANDROID_*` Secret，`app/package.json` 版本。
- Produces: 可直接手动运行或被 `release.yml` 调用的 CD workflow；GitHub Release tag `v<version>`。

- [ ] **Step 1: 替换触发器并声明两个相同输入**

将 `on` 改为：

```yaml
on:
  workflow_dispatch:
    inputs:
      prerelease:
        description: "Create a prerelease"
        required: true
        default: true
        type: boolean
  workflow_call:
    inputs:
      prerelease:
        description: "Create a prerelease"
        required: true
        type: boolean
```

- [ ] **Step 2: 在 Prepare 中先校验版本并输出固定 Release tag**

在读取 `app/package.json` 后执行：

```bash
VERSION="${{ steps.version.outputs.value }}"
if [ "${{ inputs.prerelease }}" = "true" ]; then
  if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+-(alpha|beta|rc)(\.[0-9A-Za-z.-]+)?$ ]]; then
    echo "Prerelease version must use an alpha, beta, or rc suffix: $VERSION"
    exit 1
  fi
elif ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Stable version must use x.y.z: $VERSION"
  exit 1
fi
echo "release_tag=v$VERSION" >> "$GITHUB_OUTPUT"
```

删除基于 `github.ref`/`github.event_name` 的 tag 分支逻辑；changelog 脚本统一使用 Prepare 输出的 `release_tag`。

- [ ] **Step 3: 将 Release 类型绑定到输入**

```yaml
      - name: Create Release
        uses: ncipollo/release-action@v1
        with:
          name: ${{ needs.prepare.outputs.release_title }}
          tag: ${{ needs.prepare.outputs.release_tag }}
          commit: ${{ github.sha }}
          body: ${{ needs.prepare.outputs.release_body }}
          draft: false
          prerelease: ${{ inputs.prerelease }}
          token: ${{ secrets.GITHUB_TOKEN }}
```

- [ ] **Step 4: 更新旧品牌测试中的 CD 断言**

保留 `QingYu packaging launches the renamed kernel` 对 `cd.yml` 的品牌约束；删除或改写 `QingYu manual CD derives a release tag and supports the first release` 中已经过时的 `workflow_dispatch` 分支断言，使版本/触发合约由 `qingyuWorkflows.test.ts` 唯一负责。

- [ ] **Step 5: 运行 CD 相关测试**

Run: `cd app && node --import tsx --test --test-name-pattern='CD|packaging launches' src/util/qingyuWorkflows.test.ts src/util/qingyuBranding.test.ts`

Expected: CD 合约与品牌测试 PASS；Docker/统一入口测试仍因文件缺失而 FAIL，不把这些预期失败误报为 CD 回归。

---

### Task 3: 恢复 QingYu Docker reusable workflow

**Files:**
- Create: `.github/workflows/dockerimage.yml`
- Test: `app/src/util/qingyuWorkflows.test.ts`

**Interfaces:**
- Consumes: boolean `inputs.prerelease`、`DOCKER_HUB_USER`、`DOCKER_HUB_PWD`、`app/package.json` 版本、仓库根 `Dockerfile`。
- Produces: 预发布多架构只构建结果，或稳定版 `apkdv/qingyu:v<version>` 与 `apkdv/qingyu:latest`。

- [ ] **Step 1: 创建仅手动/可复用的 workflow 骨架和版本校验**

使用与 Task 2 完全相同的 `workflow_dispatch`/`workflow_call` `prerelease` 输入，并配置：

```yaml
env:
  package_json: "app/package.json"
  docker_hub_owner: "apkdv"
  docker_hub_repo: "qingyu"

jobs:
  build:
    if: ${{ github.repository == 'appdev/QingYu' }}
    runs-on: ubuntu-latest
    permissions:
      contents: read
```

在 checkout 和版本提取后复用 Task 2 的严格稳定/预发布正则校验。

- [ ] **Step 2: 配置 Buildx 与条件登录**

```yaml
      - name: Set up QEMU
        uses: docker/setup-qemu-action@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v4

      - name: Log in to Docker Hub
        if: ${{ !inputs.prerelease }}
        uses: docker/login-action@v4
        with:
          username: ${{ secrets.DOCKER_HUB_USER }}
          password: ${{ secrets.DOCKER_HUB_PWD }}
```

- [ ] **Step 3: 使用一个 Buildx action 同时覆盖只构建和发布**

```yaml
      - name: Build Docker image
        uses: docker/build-push-action@v7
        with:
          context: .
          platforms: linux/amd64,linux/arm64,linux/arm/v7,linux/arm/v8
          push: ${{ !inputs.prerelease }}
          tags: |
            apkdv/qingyu:latest
            apkdv/qingyu:v${{ steps.version.outputs.value }}
```

`push: false` 时 BuildKit 完成所有目标平台构建但不导出 registry manifest；登录步骤也被跳过。

- [ ] **Step 4: 运行 Docker 合约测试**

Run: `cd app && node --import tsx --test --test-name-pattern='Docker' src/util/qingyuWorkflows.test.ts`

Expected: PASS。

---

### Task 4: 新建统一入口并移除锁帖定时触发

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/lock.yml`
- Test: `app/src/util/qingyuWorkflows.test.ts`

**Interfaces:**
- Consumes: 用户手动选择的 boolean `inputs.prerelease` 和仓库 Actions Secrets。
- Produces: 两个并行 reusable workflow 调用及统一运行结论。

- [ ] **Step 1: 创建统一手动调度 workflow**

```yaml
name: Release QingYu

on:
  workflow_dispatch:
    inputs:
      prerelease:
        description: "Create a prerelease without publishing Docker images"
        required: true
        default: true
        type: boolean

permissions:
  contents: write

jobs:
  release:
    uses: ./.github/workflows/cd.yml
    with:
      prerelease: ${{ inputs.prerelease }}
    secrets: inherit

  docker:
    uses: ./.github/workflows/dockerimage.yml
    with:
      prerelease: ${{ inputs.prerelease }}
    secrets: inherit
```

- [ ] **Step 2: 删除 lock.yml 的 schedule**

将触发器收敛为：

```yaml
on:
  workflow_dispatch:
```

- [ ] **Step 3: 运行全部 workflow 合约测试**

Run: `cd app && node --import tsx --test src/util/qingyuWorkflows.test.ts`

Expected: 所有 workflow 合约测试 PASS。

---

### Task 5: 完整验证和评审

**Files:**
- Verify: `.github/workflows/cd.yml`
- Verify: `.github/workflows/dockerimage.yml`
- Verify: `.github/workflows/release.yml`
- Verify: `.github/workflows/lock.yml`
- Verify: `app/src/util/qingyuWorkflows.test.ts`
- Verify: `app/src/util/qingyuBranding.test.ts`
- Verify: `docs/superpowers/specs/2026-08-12-manual-release-workflows-design.md`
- Verify: `docs/superpowers/plans/2026-08-12-manual-release-workflows.md`

**Interfaces:**
- Consumes: Tasks 1-4 的冻结工作树。
- Produces: 本地验证证据和可供用户决定是否提交、推送、远程运行的最终差异。

- [ ] **Step 1: 解析所有 workflow YAML**

Run:

```bash
ruby -e 'require "yaml"; Dir[".github/workflows/*.{yml,yaml}"].each { |f| YAML.parse_file(f); puts "YAML OK: #{f}" }'
```

Expected: `cd.yml`、`dockerimage.yml`、`release.yml`、`lock.yml` 均输出 `YAML OK`。

- [ ] **Step 2: 检查自动触发器与上游发布目标**

Run:

```bash
rg -n '^  (push|release|schedule|pull_request|pull_request_target):|b3log/siyuan|github\.repository_owner == .siyuan-note.|Upload to AUR' .github/workflows
```

Expected: 无输出，退出码 1 表示未匹配到禁止项。

- [ ] **Step 3: 运行完整前端测试**

Run: `cd app && pnpm test`

Expected: 全部测试 PASS，0 failures。

- [ ] **Step 4: 运行项目规定的前端 lint/typecheck**

Run: `cd app && pnpm run lint`

Expected: exit 0，无 TypeScript 或 ESLint 错误；若 ESLint `--fix` 产生改动，重新检查 diff 并重跑受影响测试。

- [ ] **Step 5: 检查最终差异与敏感信息**

Run:

```bash
git diff --check
git status --short
git diff -- .github/workflows app/src/util docs/superpowers
```

Expected: 只有计划内文件；不存在 keystore、密码、Docker token、Base64 数据、调试输出或无关改动。

- [ ] **Step 6: 停在本地交付边界**

报告修改、验证和残余风险。除非用户再次明确授权，否则不提交、不推送、不运行远程 Release；`prerelease: false` 的远程运行属于正式发布，必须获得精确授权。
