const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const {auditRepository} = require("./qingyu-brand-audit.cjs");

const makeFixture = (files) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-brand-audit-"));
    for (const [relativePath, content] of Object.entries(files)) {
        const target = path.join(root, relativePath);
        fs.mkdirSync(path.dirname(target), {recursive: true});
        fs.writeFileSync(target, content);
    }
    return root;
};

test("rejects upstream update services on product surfaces", (t) => {
    const root = makeFixture({
        "app/electron/error.html": "https://github.com/siyuan-note/siyuan/releases/download/v1/siyuan-1.exe",
        "app/appearance/langs/en.json": "https://liuyun.io/article/1686530886208",
        "kernel/model/updater.go": "https://release.b3log.org/siyuan/ https://release.liuyun.io/siyuan/",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    const ruleIDs = auditRepository(root).violations.map((item) => item.rule);
    assert.ok(ruleIDs.includes("upstream-update-service"));
    assert.ok(ruleIDs.includes("upstream-package-name"));
    assert.ok(ruleIDs.includes("upstream-product-service"));
});

test("rejects SiYuan product identity and logo consumers", (t) => {
    const root = makeFixture({
        "app/electron/workspace.html": "<h1>SiYuan</h1>",
        "app/src/layout/status.ts": "useIcon(\"iconSiYuan\")",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    const ruleIDs = auditRepository(root).violations.map((item) => item.rule);
    assert.ok(ruleIDs.includes("user-visible-upstream-name"));
    assert.ok(ruleIDs.includes("upstream-logo-consumer"));
});

test("allows exact compatibility, attribution, and source-trace uses", (t) => {
    const root = makeFixture({
        "kernel/model/example.go": "import \"github.com/siyuan-note/siyuan/kernel/util\"",
        "app/src/util/pathName.ts": "window.siyuan; const legacy = \"siyuan:\";",
        "NOTICE.md": "QingYu is modified from SiYuan under AGPL-3.0 and is not an official release. https://github.com/siyuan-note/siyuan/commit/abc",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    assert.deepEqual(auditRepository(root).violations, []);
});

test("does not treat an entire directory as a compatibility allowlist", (t) => {
    const root = makeFixture({
        "kernel/model/user_message.go": "const title = \"SiYuan official download\"",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    assert.equal(auditRepository(root).violations.some((item) => item.rule === "user-visible-upstream-name"), true);
});

test("rejects legacy SiYuan identifiers from the Docker runtime", (t) => {
    const root = makeFixture({
        Dockerfile: "WORKDIR /opt/siyuan/\nCOPY /kernel/kernel .\nENV HOME=/home/siyuan\n",
        "kernel/entrypoint.sh": "USER_NAME=${USER_NAME:-siyuan}\nSIYUAN_WORKSPACE_PATH=value\nWORKSPACE_DIR=/siyuan/workspace\n",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    const violations = auditRepository(root).violations.filter((item) => item.rule === "upstream-docker-runtime-identity");
    assert.equal(violations.length, 6);
    assert.deepEqual(new Set(violations.map((item) => item.excerpt)), new Set([
        "WORKDIR /opt/siyuan/",
        "COPY /kernel/kernel .",
        "ENV HOME=/home/siyuan",
        "USER_NAME=${USER_NAME:-siyuan}",
        "SIYUAN_WORKSPACE_PATH=value",
        "WORKSPACE_DIR=/siyuan/workspace",
    ]));
});

test("rejects legacy SiYuan access authorization environment variables", (t) => {
    const root = makeFixture({
        "README.md": "SIYUAN_ACCESS_AUTH_CODE=value\n",
        "kernel/util/working.go": "SIYUAN_ACCESS_AUTH_CODE_BYPASS=true\n",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    const violations = auditRepository(root).violations.filter((item) => item.rule === "upstream-access-auth-environment");
    assert.equal(violations.length, 2);
});

test("allows the complete QingYu Docker runtime identity", (t) => {
    const root = makeFixture({
        Dockerfile: "WORKDIR /opt/qingyu/\nCMD [\"/opt/qingyu/QingYu-Kernel\", \"serve\"]\n",
        "kernel/entrypoint.sh": "HOME=/home/qingyu\nQINGYU_WORKSPACE_PATH=/qingyu/workspace\n",
    });
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));

    assert.deepEqual(auditRepository(root).violations, []);
});
