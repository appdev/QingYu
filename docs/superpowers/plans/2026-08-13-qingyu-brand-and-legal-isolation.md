# QingYu Brand and Legal Isolation Implementation Plan

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将当前仓库整改为清晰、可验证且不冒用 SiYuan 名称、Logo、更新服务或法律文本的轻语独立发行版，同时保留 AGPL、上游归属和既有数据/API 兼容性。

**Architecture:** 用可执行的仓库品牌审计作为统一发布闸门，再分别收口品牌常量、应用更新链路、Electron/前端入口、Logo 消费者、内置指南与法律文档。指南和法律 HTML 都从单一、可审查的源文件确定性生成；兼容名称通过精确路径与用途白名单保留，禁止全局机械替换。

**Tech Stack:** Go 1.x（以 `kernel/go.mod` 为准）、TypeScript/Node.js、Electron、Node test runner、`.sy` JSON Spec 2、Markdown、项目现有 pnpm 工具链。

## Global Constraints

- 简体中文产品名固定为“轻语”，繁体中文固定为“輕語”，其他语言固定为“QingYu”。
- `logo.png` 是唯一品牌图形主源；产品表面不得消费 SiYuan Logo。
- 官网固定为 `https://apkdv.com/`，源码项目固定为 `https://github.com/appdev/QingYu`，联系邮箱固定为 `lengyue@apkdv.com`。
- 在轻语自有更新服务上线前，应用版本检查、安装包下载和安装协调必须关闭；插件、主题、图标、模板等集市更新必须保持不变。
- 法律主体不得虚构公司或地址，统一署名“QingYu 开发者 appdev”；用户协议以美国加利福尼亚州法为设计基准，不设强制仲裁，不虚构县级法院。
- 保留 Go Module、导入路径、`window.siyuan`、插件 API、序列化字段、`.sy`、旧 `siyuan://` 读取、旧原生 User-Agent、版权、许可证和工程溯源链接。
- 不手工编辑 `app/stage/build/**`、`app/changelogs/**`、`lute.min.js` 或其他项目规则禁止编辑的生成物。
- 不修改其他移动端仓库，不提交、不推送、不发布、不部署。
- 所有实现任务遵循测试先行：先加入能观察到预期失败的测试，再写最小实现并复验。

---

## File Map

- `app/scripts/qingyu-brand-audit.cjs`：扫描发行相关源文件，执行禁止标识、禁止服务、指南内容、更新器和 Logo 消费者规则。
- `app/scripts/qingyu-brand-policy.cjs`：集中定义审计范围、允许 URL、精确兼容白名单和禁止模式。
- `app/scripts/qingyu-brand-audit.test.cjs`：用临时夹具验证审计器会拒绝违规并接受精确兼容用途。
- `app/src/util/qingyuBrand.ts`：渲染进程的轻语官网、源码、联系和本地法律页面 URL。
- `app/electron/qingyuBrand.js`：Electron 预启动页面和主进程使用的同一组轻语公共地址。
- `kernel/model/updater.go`、`kernel/model/conf.go`、`kernel/conf/system.go`：把应用版本更新实现改为兼容 API 下的安全禁用状态。
- `kernel/model/updater_test.go`：证明旧配置无法重新启用上游应用更新。
- `app/scripts/generate-qingyu-guide.cjs`：仅对四个固定指南 notebook 进行受限、确定性重建。
- `app/guide-src/*.json`：四语言精简指南的唯一内容源。
- `app/guide/<fixed-box-id>/**`：生成的四套 `.sy` 指南、notebook 配置和 Logo 派生资源。
- `docs/legal/{privacy,terms}.{zh-CN,zh-TW,en,ja}.md`：四语言隐私政策和用户协议源文档。
- `docs/legal/privacy-evidence.md`：法律表述与当前代码行为之间的审计证据矩阵。
- `app/scripts/generate-qingyu-legal.cjs`：把法律 Markdown 渲染为受支持节点集合的静态 HTML。
- `app/stage/legal/*.html`：随发行包交付、可从应用本地打开的法律页面。
- `NOTICE.md`：修改版、品牌边界、首次可验证修改日期、许可证与上游归属声明。

### Task 1: 建立可执行的品牌与兼容审计闸门

**Files:**
- Create: `app/scripts/qingyu-brand-policy.cjs`
- Create: `app/scripts/qingyu-brand-audit.cjs`
- Create: `app/scripts/qingyu-brand-audit.test.cjs`
- Modify: `app/package.json`
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Produces: `auditRepository(root: string): { violations: Array<{ rule: string; file: string; line: number; excerpt: string }> }`。
- Produces: CLI `node scripts/qingyu-brand-audit.cjs [--root <path>]`，违规退出码为 1，无违规退出码为 0。
- Produces: pnpm 脚本 `test:brand`，顺序运行审计器单测、现有品牌测试和真实仓库审计。

- [x] **Step 1: 写审计器夹具测试并扩展现有品牌测试**

在 `app/scripts/qingyu-brand-audit.test.cjs` 中创建临时仓库夹具，至少包含以下用例：用户可见 HTML 中的 `SiYuan`、`release.b3log.org`、`github.com/siyuan-note/siyuan/releases`、`siyuan-*.exe` 和 `iconSiYuan` 消费者必须失败；Go import、`window.siyuan`、许可证归属和上游 commit URL 必须通过；白名单只允许文件和规则同时精确匹配。把 `app/src/util/qingyuBranding.test.ts` 中“保留官方更新器”的断言改成“上游应用更新入口不存在”的失败断言。

```js
test("rejects an upstream release URL on a product surface", () => {
    const root = makeFixture({"app/electron/error.html": '<a href="https://github.com/siyuan-note/siyuan/releases">Download</a>'});
    const result = auditRepository(root);
    assert.equal(result.violations.some((item) => item.rule === "upstream-update-service"), true);
});

test("allows a Go module compatibility import", () => {
    const root = makeFixture({"kernel/model/example.go": 'import "github.com/siyuan-note/siyuan/kernel/util"'});
    assert.deepEqual(auditRepository(root).violations, []);
});
```

- [x] **Step 2: 运行测试并记录预期失败**

Run: `cd app && node --test scripts/qingyu-brand-audit.test.cjs && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: FAIL，原因分别为审计模块尚不存在，以及现有更新器/用户表面仍包含被禁止的上游更新路径。

- [x] **Step 3: 实现审计策略和 CLI**

策略只扫描 Git 跟踪且会进入发行或用户文档的文本文件；二进制资源按哈希和消费者检查。规则对象必须显式包含 `id`、`patterns`、`includeGlobs` 和精确 `allow` 条目，不允许用整个 `kernel/**` 或 `docs/**` 作为豁免。CLI 输出 `rule file:line excerpt`，且不输出文件中的潜在凭据值。

```js
const POLICY = {
    approvedUrls: new Set([
        "https://apkdv.com/",
        "https://github.com/appdev/QingYu",
        "https://github.com/appdev",
        "mailto:lengyue@apkdv.com",
    ]),
    compatibilityAllows: [
        {glob: "kernel/**/*.go", rule: "go-module-path", pattern: /^github\.com\/siyuan-note\/siyuan\//},
        {glob: "app/src/**/*.ts", rule: "runtime-api-name", pattern: /window\.siyuan/},
    ],
};
```

- [x] **Step 4: 运行审计器单测和真实仓库审计**

Run: `cd app && node --test scripts/qingyu-brand-audit.test.cjs`

Expected: PASS。

Run: `cd app && node scripts/qingyu-brand-audit.cjs`

Expected: FAIL，并列出后续任务已知的旧指南、更新器、旧 URL 和 `iconSiYuan` 消费者；不应把 Go import、API 字段或许可证文本误报为产品品牌。

### Task 2: 集中轻语公共地址并清理 Electron 启动表面

**Files:**
- Create: `app/electron/qingyuBrand.js`
- Create: `app/src/util/qingyuBrand.ts`
- Modify: `app/electron/main.js`
- Modify: `app/electron/window.js`
- Modify: `app/electron/init.html`
- Modify: `app/electron/workspace.html`
- Modify: `app/electron/error.html`
- Modify: `app/src/layout/status.ts`
- Modify: `app/src/boot/nativeMenu.ts`
- Modify: `app/src/menus/workspace.ts`
- Modify: `app/src/mobile/menu/index.ts`
- Delete if unreferenced: `app/src/config/util/about.ts`
- Test: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Produces: renderer constants `QINGYU_WEBSITE_URL`, `QINGYU_SOURCE_URL`, `QINGYU_CONTACT_URL`。
- Produces: `getQingYuLegalURL(kind: "privacy" | "terms", language: string): string`，仅映射 `zh-CN`、`zh-TW`、`en`、`ja`，其他语言回退 `en`。
- Produces: CommonJS Electron export with the same three public URLs.

- [x] **Step 1: 加入公共地址和启动页面断言**

测试读取 Electron HTML/JS 与 renderer 菜单文件，断言不包含 B3log、链滴、liuyun、SiYuan release/download/feedback URL；断言官网、源码和邮件地址只来自新增常量模块；断言 `getQingYuLegalURL("privacy", "fr_FR")` 返回 `/stage/legal/privacy.en.html`。

- [x] **Step 2: 运行目标测试并确认失败**

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: FAIL，报告现有 `b3log.org`、`ld246.com`、`liuyun.io`、上游仓库和下载入口。

- [x] **Step 3: 实现常量并替换或移除入口**

将官网和源码菜单改为批准地址；反馈只使用 `mailto:lengyue@apkdv.com`。从错误页、工作空间页和初始化页移除没有轻语自有承接服务的社区、下载、定价、账号和上游反馈按钮。保留错误信息、重试、选择工作空间和退出等本地功能。主进程对外发送的产品 Referer 改为 `https://apkdv.com/`。

```ts
export const QINGYU_WEBSITE_URL = "https://apkdv.com/";
export const QINGYU_SOURCE_URL = "https://github.com/appdev/QingYu";
export const QINGYU_CONTACT_URL = "mailto:lengyue@apkdv.com";

export const getQingYuLegalURL = (kind: "privacy" | "terms", language: string) => {
    const locale = ["zh-CN", "zh-TW", "en", "ja"].includes(language) ? language : "en";
    return `/stage/legal/${kind}.${locale}.html`;
};
```

- [x] **Step 4: 运行目标测试和审计器**

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts && node scripts/qingyu-brand-audit.cjs`

Expected: 品牌测试通过；真实仓库审计仍因指南、更新器、Logo 或法律文档缺失而失败，但不再报告本任务文件。

### Task 3: 禁用应用版本更新和安装包协调

**Files:**
- Modify: `kernel/model/updater.go`
- Modify: `kernel/model/conf.go`
- Modify: `kernel/model/mount.go`
- Modify: `kernel/conf/system.go`
- Modify: `kernel/api/system.go`（以 `setDownloadInstallPkg` 实际所在文件为准）
- Create or Modify: `kernel/model/updater_test.go`
- Modify: `app/src/config/tabs/aboutTab.ts`
- Modify: `app/src/config/tabs/appRuntime.ts`
- Modify: `app/src/dialog/processSystem.ts`
- Modify: `app/electron/main.js`
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Preserves: 现有 `/api/system/checkUpdate` 和旧配置字段的 JSON/API 兼容形状。
- Produces: `skipNewVerInstallPkg() bool` 永远返回 `true`；`getNewVerInstallPkgPath() string` 永远返回空串。
- Removes: SiYuan 版本查询、`siyuan-*` 下载、校验、安装包启动和 Electron 安装协调的可达路径。

- [x] **Step 1: 写 Go 与前端失败测试**

Go 测试把 `Conf.System.DownloadInstallPkg` 设置为 `true`，断言仍跳过下载且安装路径为空；恢复测试前的全局配置。前端测试断言关于页没有检查更新和自动下载控件，`processSystem.ts` 与 Electron 主进程没有安装包启动分支，更新器文件没有上游版本/Release 地址。

```go
func TestApplicationUpdateRemainsDisabledForLegacyConfiguration(t *testing.T) {
    old := Conf.System.DownloadInstallPkg
    t.Cleanup(func() { Conf.System.DownloadInstallPkg = old })
    Conf.System.DownloadInstallPkg = true
    if !skipNewVerInstallPkg() {
        t.Fatal("legacy configuration must not enable application updates")
    }
    if got := getNewVerInstallPkgPath(); got != "" {
        t.Fatalf("unexpected application update package %q", got)
    }
}
```

- [x] **Step 2: 运行测试并确认现状失败**

Run: `cd kernel && go test ./model -run TestApplicationUpdateRemainsDisabledForLegacyConfiguration -count=1`

Expected: FAIL，因为旧配置当前可以进入下载/安装路径。

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: FAIL，因为设置和 Electron 仍包含应用更新 UI/协调代码。

- [x] **Step 3: 以兼容空实现关闭更新器**

删除 `util.GetRhyResult`、上游 release URL、安装包名称解析和下载实现所需的无用 import。API 可继续返回兼容结果结构，但不得发起网络请求；旧“自动下载”写入请求必须强制为 `false`，默认配置改为 `false`。移除 notebook 挂载完成后的自动 `CheckUpdate(true)` 调用，`Close` 不再返回安装包路径。

- [x] **Step 4: 移除前端和 Electron 的应用更新表面**

关于页仅显示当前 `Constants.SIYUAN_VERSION`（变量名作为内部兼容暂留）、源码、许可证、隐私政策和用户协议；删除检查更新按钮和自动下载开关。`appRuntime.ts` 不再把旧设置值转发给更新 API，只在读取旧配置时归一化为 `false`。`processSystem.ts` 保留退出、重启、工作空间切换行为，但不解释或转发 `installPkgPath`。Electron 删除安装包校验、等待、启动和退出协调函数。

- [x] **Step 5: 格式化并运行边界测试**

Run: `gofmt -w kernel/model/updater.go kernel/model/conf.go kernel/conf/system.go kernel/model/updater_test.go`

Run: `cd kernel && go test ./model -run 'TestApplicationUpdateRemainsDisabledForLegacyConfiguration|Test.*Bazaar' -count=1`

Expected: 更新禁用测试通过；若没有匹配的 Bazaar 测试，命令应明确报告无该测试，再在 Task 9 运行现有相关包测试。

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: PASS 本任务相关断言。

### Task 4: 清理用户可见文案、链接和旧 Logo 消费者

**Files:**
- Modify: `app/appearance/langs/*.json`
- Modify: `app/appearance/icons/litheness/icon.js`
- Modify: `app/appearance/icons/index.html`
- Modify: `app/appearance/emojis/index.html`
- Modify: `app/src/menus/dataMigration.ts`
- Modify: `app/src/menus/navigation.ts`
- Modify: `app/src/menus/commonMenuItem.ts`
- Modify: `app/src/util/mount.ts`
- Modify: `app/src/layout/status.ts`
- Test: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Preserves: 全部语言 JSON 键集合相同。
- Removes: 无消费者后的 `iconSiYuan` symbol。
- Uses: 已有 `iconUpload`、`iconDownload`、`iconGithub` 等最贴合操作语义的通用图标；归档/迁移图标在实现前从 `icon.js` 现有 symbol 中选取，不新增手写 SVG。

- [x] **Step 1: 扩展文案和 SVG 消费者失败断言**

断言所有语言的 `downloadLatestVer` 不再链接上游下载页，`_kernel["184"]` 不再把 SiYuan 作为当前产品来源链接；断言 TypeScript/HTML 不引用 `iconSiYuan`，且 sprite 中不再包含该 symbol。审计规则允许许可证/归属文案中的 SiYuan，但要求同一文档包含“非官方/无隶属或背书”的等价声明。

- [x] **Step 2: 运行测试并确认失败**

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: FAIL，报告 21 个语言文件和旧 Logo 消费者。

- [x] **Step 3: 修改全部语言文案并替换功能图标**

按每种语言真实翻译，使用三个 ASCII 句点表示省略号；`zh-CN` 使用“轻语”，`zh-TW` 使用“輕語”，其他语言使用“QingYu”。删除已隐藏 UI 不再使用的更新文案时，必须同步删除每个语言文件中的同一键；若保留键用于兼容，则改为不含上游下载承诺的中性版本信息。数据迁移、导入、导出和挂载入口改用现有功能图标。

- [x] **Step 4: 验证语言键和目标测试**

Run: `python scripts/check-lang-keys.py`

Expected: 所有语言键完全一致。

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: 文案与 Logo 消费者断言通过。

### Task 5: 受限、确定性重建四套内置轻语指南

**Files:**
- Create: `app/guide-src/en.json`
- Create: `app/guide-src/zh-CN.json`
- Create: `app/guide-src/zh-TW.json`
- Create: `app/guide-src/ja.json`
- Create: `app/scripts/generate-qingyu-guide.cjs`
- Create: `app/scripts/generate-qingyu-guide.test.cjs`
- Replace generated contents under: `app/guide/20210808180117-6v0mkxr/**`
- Replace generated contents under: `app/guide/20210808180117-czj9bvb/**`
- Replace generated contents under: `app/guide/20211226090932-5lcq56f/**`
- Replace generated contents under: `app/guide/20240530133126-axarxgx/**`
- Modify: `app/package.json`

**Interfaces:**
- Produces: CLI `node scripts/generate-qingyu-guide.cjs --write|--check`。
- Preserves box/root IDs: `20210808180117-6v0mkxr/20200923234011-ieuun1p`、`20210808180117-czj9bvb/20200812220555-lj3enxa`、`20211226090932-5lcq56f/20211226115423-d5z1joq`、`20240530133126-axarxgx/20240530101000-4qitucx`。
- Produces: 每种语言一个合法 `Spec: "2"` 根 `NodeDocument`，含八个已批准章节和由根 `logo.png` 确定性复制的 `assets/qingyu-logo.png`。

- [x] **Step 1: 写生成器安全和格式测试**

测试必须证明：缺少 `--write` 时不修改文件；清理目标只能是四个完整匹配的 box ID；目标路径逃逸时立即失败；生成两次字节一致；根 ID 与文件名一致；`Properties.type` 为 `doc`；`Children` 非空；所有 `NodeImage` 结构可由 JSON 重新解析；四种语言均含产品名、独立发行声明、本地法律链接和邮箱，且不含账号、会员、SiYuan 下载/定价/协议 URL。

- [x] **Step 2: 运行生成器测试并确认失败**

Run: `cd app && node --test scripts/generate-qingyu-guide.test.cjs`

Expected: FAIL，因为生成器和内容源尚不存在。

- [x] **Step 3: 编写四语言内容源和 `.sy` 生成器**

内容源固定字段为 `locale`、`productName`、`title`、`sections: Array<{heading: string; paragraphs: string[]; bullets: string[]}>`。生成器只支持标题、段落、列表、链接和图片所需的已验证 `.sy` 节点；遇到未知字段或空章节应报错。`updated` 使用固定值 `20260813000000`，避免每次运行产生差异。

```js
const GUIDES = {
    "zh-CN": {boxID: "20210808180117-czj9bvb", rootID: "20200812220555-lj3enxa", name: "轻语用户指南"},
    "zh-TW": {boxID: "20211226090932-5lcq56f", rootID: "20211226115423-d5z1joq", name: "輕語使用指南"},
    en: {boxID: "20210808180117-6v0mkxr", rootID: "20200923234011-ieuun1p", name: "QingYu User Guide"},
    ja: {boxID: "20240530133126-axarxgx", rootID: "20240530101000-4qitucx", name: "QingYu ユーザーガイド"},
};
```

- [x] **Step 4: 运行受限重建**

先验证四个目标目录和 ID 均存在，再运行：`cd app && node scripts/generate-qingyu-guide.cjs --write`。

Expected: 只删除并重建上述四个 Git 跟踪指南目录；旧内容可通过 Git 恢复；其他 `app/guide` 路径不变。

- [x] **Step 5: 验证确定性、格式与发行扫描**

Run: `cd app && node scripts/generate-qingyu-guide.cjs --check && node --test scripts/generate-qingyu-guide.test.cjs`

Expected: PASS，且 `--check` 不修改工作树。

Run: `cd app && node scripts/qingyu-brand-audit.cjs`

Expected: 不再报告指南冒用、上游服务或旧协议命中；法律文档缺失规则仍可失败。

### Task 6: 编写四语言隐私政策、用户协议和证据矩阵

**Files:**
- Create: `docs/legal/privacy.zh-CN.md`
- Create: `docs/legal/privacy.zh-TW.md`
- Create: `docs/legal/privacy.en.md`
- Create: `docs/legal/privacy.ja.md`
- Create: `docs/legal/terms.zh-CN.md`
- Create: `docs/legal/terms.zh-TW.md`
- Create: `docs/legal/terms.en.md`
- Create: `docs/legal/terms.ja.md`
- Create: `docs/legal/privacy-evidence.md`
- Create: `NOTICE.md`
- Test: `app/scripts/qingyu-brand-audit.test.cjs`

**Interfaces:**
- Produces: 八个具有相同章节 ID/实质规则的法律源文档，生效/更新日期为 `2026-08-13`。
- Produces: `NOTICE.md` 记录 `appdev`、首次可验证 QingYu 修改日期 `2026-08-12`、AGPL-3.0、上游版权和非官方关系。

- [x] **Step 1: 扩展法律文档结构和禁用表述测试**

测试要求每份隐私政策包含本地数据、网络能力、日志/反馈、第三方扩展、保留删除、安全、儿童、跨境、变更和联系章节；每份协议包含接受、资格、AGPL/第三方许可、用户责任、禁止行为、第三方服务、按现状、责任限制、赔偿、变更终止、可分割性、加州法、善意协商、法院和强制性消费者权利。禁止出现虚构公司、地址、强制仲裁、SiYuan 协议 URL 或绝对“从不联网/从不收集”承诺。

- [x] **Step 2: 运行审计测试并确认失败**

Run: `cd app && node --test scripts/qingyu-brand-audit.test.cjs`

Expected: FAIL，报告法律源文件和必要章节缺失。

- [x] **Step 3: 基于代码审计写隐私证据矩阵**

逐项记录声明、代码路径、验证命令和允许写入政策的限定词。至少核对工作空间/配置/日志路径、遥测或统计搜索结果、主动反馈、同步与 S3/WebDAV、插件/主题、MCP、发布、网络图片/资源、日志导出和删除行为。证据不足时政策使用“可能”“取决于用户配置”等限定，不写绝对承诺。

- [x] **Step 4: 写四语言隐私政策和用户协议**

简体中文为解释参考版本，但每个版本都明确不排除适用法律的强制性权利。协议争议条款使用“先联系 `lengyue@apkdv.com` 善意协商；未解决时由加利福尼亚州有管辖权的州或联邦法院处理”，不指定县、不写虚构送达地址、不设置仲裁。每份文档都链接 GitHub 身份、项目、网站和邮箱。

- [x] **Step 5: 写修改版与品牌归属声明并复验**

`NOTICE.md` 保留原权利人的版权，不宣称拥有 SiYuan 名称或 Logo；说明 QingYu 是独立修改版、不是官方发行、没有隶属/授权/认可/背书。注明法律文本上线前仍建议由合格律师复核，不把本工程整改表述为正式法律意见。

Run: `cd app && node --test scripts/qingyu-brand-audit.test.cjs`

Expected: PASS 法律结构和禁止表述测试。

### Task 7: 从法律 Markdown 生成并接入本地静态页面

**Files:**
- Create: `app/scripts/generate-qingyu-legal.cjs`
- Create: `app/scripts/generate-qingyu-legal.test.cjs`
- Create: `app/stage/legal/privacy.zh-CN.html`
- Create: `app/stage/legal/privacy.zh-TW.html`
- Create: `app/stage/legal/privacy.en.html`
- Create: `app/stage/legal/privacy.ja.html`
- Create: `app/stage/legal/terms.zh-CN.html`
- Create: `app/stage/legal/terms.zh-TW.html`
- Create: `app/stage/legal/terms.en.html`
- Create: `app/stage/legal/terms.ja.html`
- Modify: `app/package.json`
- Modify: `app/src/config/tabs/aboutTab.ts`
- Modify: `README.md`

**Interfaces:**
- Produces: CLI `node scripts/generate-qingyu-legal.cjs --write|--check`。
- Consumes: `docs/legal/{privacy,terms}.{locale}.md`。
- Produces: UTF-8 standalone HTML with CSP `default-src 'none'; style-src 'unsafe-inline'; img-src 'self' data:` and no remote script/style/font。

- [x] **Step 1: 写 Markdown 节点、转义、CSP 和确定性失败测试**

测试覆盖标题、段落、有序/无序列表、强调、行内代码和链接；HTML 特殊字符必须转义，`javascript:`、非批准 HTTP(S) 和未知 AST 节点必须报错；生成两次结果一致；`--check` 在源和输出不一致时退出 1。

- [x] **Step 2: 运行测试并确认失败**

Run: `cd app && node --test scripts/generate-qingyu-legal.test.cjs`

Expected: FAIL，因为生成器不存在。

- [x] **Step 3: 用项目现有 Markdown AST 依赖实现窄渲染器**

使用已安装的 `unified`、`remark-parse` 和 `remark-gfm` 解析，不新增依赖；实现 `renderNode(node)` 的显式 switch，只接受测试列出的节点，任何未知节点抛错。页面语言属性来自文件 locale，标题来自第一个一级标题。

- [x] **Step 4: 生成静态页面并接入关于页/README**

Run: `cd app && node scripts/generate-qingyu-legal.cjs --write`

关于页使用 `getQingYuLegalURL` 打开隐私政策和用户协议；README 增加独立发行、非官方关系、`NOTICE.md`、`LICENSE`、法律文档和源码入口。不得把 SiYuan 名称放进产品标题或宣传句，仅在必要归属段落中事实性使用。

- [x] **Step 5: 验证生成漂移和页面内容**

Run: `cd app && node scripts/generate-qingyu-legal.cjs --check && node --test scripts/generate-qingyu-legal.test.cjs`

Expected: PASS，生成文件与 Markdown 源一致，且没有外部可执行资源。

### Task 8: 覆盖修改版声明、Logo 派生关系和兼容回归

**Files:**
- Modify: `app/src/util/qingyuBranding.test.ts`
- Modify as needed: `app/package.json`
- Modify only when a current product fixture is affected: `kernel/model/assets_test.go`
- Read-only verify: `LICENSE`
- Read-only verify: `logo.png`
- Read-only verify: `app/build/icon.png`
- Read-only verify: `app/build/icon.icns`
- Read-only verify: `app/build/icon.ico`

**Interfaces:**
- Produces: 可重复的 PNG 像素/缩放关联测试，而不是仅比较文件名。
- Preserves: `window.siyuan`、`.sy`、旧 URI 读取和旧 User-Agent 兼容测试。

- [x] **Step 1: 加入 Logo 来源、声明和兼容性断言**

测试读取根 Logo 和产品 PNG 图标，验证根图为 1254×1254、派生 PNG 为预期尺寸且归一化缩放后的像素关系满足确定性阈值；断言 `NOTICE.md`、README 和指南均含 AGPL、修改版日期与非官方关系；断言兼容标识仍存在于指定 API/类型/解析代码中。

- [x] **Step 2: 运行测试并观察仍存在的失败**

Run: `cd app && pnpm test -- src/util/qingyuBranding.test.ts`

Expected: 若当前图标并非由根 Logo 可重复派生或声明入口未接齐则 FAIL，并给出具体文件。

- [x] **Step 3: 只修复被证据证明不一致的派生资源或声明入口**

若 PNG 关联失败，使用确定性图像缩放命令从根 `logo.png` 重新生成允许编辑的产品 PNG；不得手绘或重新创作 Logo。ICNS/ICO 只在现有项目脚本能由同一 PNG 源确定性生成并可验证时更新，否则保留并在风险报告中明确未验证。若测试文件中的当前产品临时安装包仍叫 `siyuan-*`，改为 `qingyu-*`；不修改兼容解析夹具。

- [x] **Step 4: 运行品牌与兼容目标测试**

Run: `cd app && pnpm run test:brand`

Expected: PASS。

Run: `cd kernel && go test ./model ./api ./util -run 'Test.*(URI|Scheme|UserAgent|ApplicationUpdate|Guide|Mount|Bazaar)' -count=1`

Expected: 所有存在的目标测试通过；无匹配测试的包应在最终报告中列为未覆盖，而不是虚构通过。

### Task 9: 全量定向验证、冻结结果和风险报告

**Files:**
- Create: `docs/legal/qingyu-brand-audit-report.md`
- Modify only for discovered in-scope defects: files from Tasks 1-8

**Interfaces:**
- Produces: 按“用户可见已清除、兼容保留、版权/溯源保留、未覆盖风险”分类的最终命中报告。
- Produces: 最终 Git tree/status、验证命令、退出码和日期的可追溯记录；不创建 commit。

- [x] **Step 1: 运行生成物漂移、语言和品牌闸门**

Run: `cd app && node scripts/generate-qingyu-guide.cjs --check && node scripts/generate-qingyu-legal.cjs --check && pnpm run test:brand`

Expected: 全部 PASS。

Run: `python scripts/check-lang-keys.py`

Expected: PASS。

- [x] **Step 2: 运行前端测试和项目规定 lint**

Run: `cd app && pnpm test`

Expected: PASS。

Run: `cd app && pnpm run lint`

Expected: PASS；该命令可能按项目脚本自动修复格式，之后必须重新运行品牌闸门并检查 diff。

- [x] **Step 3: 运行受影响 Go 包测试，不构建或重启内核**

Run: `cd kernel && go test ./model ./api ./conf ./util -count=1`

Expected: PASS。不得运行内核二进制、`go build` 或重启现有内核。

- [x] **Step 4: 重新扫描并编写分类报告**

Run: `rg -n --hidden --glob '!.git/**' 'SiYuan|思源|siyuan-note/siyuan/releases|release\.b3log\.org|release\.liuyun\.io'`

将每个剩余类别写入报告：精确路径、用途、为何允许、对应审计白名单规则。任何白名单外的用户可见命中必须修复后重新执行本任务验证。报告还需记录 Windows/Linux 安装器、移动端独立仓库、公开网站部署和律师审查未在本地完成。

- [x] **Step 5: 冻结工作树并执行最终集成复验**

先记录 `git status --short` 与 `git diff --stat`，然后只运行一次冻结后的集成集合：

Run: `cd app && pnpm run test:brand && node scripts/generate-qingyu-guide.cjs --check && node scripts/generate-qingyu-legal.cjs --check`

Run: `cd kernel && go test ./model ./api ./conf ./util -count=1`

Run: `git diff --check`

Expected: 全部退出码为 0；冻结后若任何文件变化，仅失效并重跑受影响的验证，不重复无关套件。

- [x] **Step 6: 交付但不提交**

最终说明必须列出变更范围、验证证据、剩余兼容名称类别、法律/商标复核风险以及被替换的四套旧指南内容可由 Git 恢复。明确当前工作树未提交、未推送、未发布、未部署。
