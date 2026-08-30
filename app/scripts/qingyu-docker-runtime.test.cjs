const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {spawnSync} = require("node:child_process");
const test = require("node:test");

const repositoryRoot = path.join(__dirname, "../..");
const dockerfilePath = path.join(repositoryRoot, "Dockerfile");
const entrypointPath = path.join(repositoryRoot, "kernel/entrypoint.sh");

const writeExecutable = (target, source) => {
    fs.writeFileSync(target, source);
    fs.chmodSync(target, 0o755);
};

const createMockPath = (overrides = {}) => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-entrypoint-"));
    writeExecutable(path.join(directory, "getent"), `#!/bin/sh
case "$1" in
    group) printf 'qingyu:x:%s:\n' "$2" ;;
    passwd) printf 'qingyu:x:%s:1000::/home/qingyu:/sbin/nologin\n' "$2" ;;
    *) exit 1 ;;
esac
`);
    for (const command of ["mkdir", "chown"]) {
        writeExecutable(path.join(directory, command), "#!/bin/sh\nexit 0\n");
    }
    for (const command of ["addgroup", "adduser"]) {
        writeExecutable(path.join(directory, command), "#!/bin/sh\nexit 97\n");
    }
    writeExecutable(path.join(directory, "su-exec"), `#!/bin/sh
printf 'workspace=[%s]\n' "$QINGYU_WORKSPACE_PATH"
for arg in "$@"; do
    printf 'arg=[%s]\n' "$arg"
done
`);
    for (const [command, source] of Object.entries(overrides)) {
        writeExecutable(path.join(directory, command), source);
    }
    return directory;
};

const runEntrypoint = (args, extraEnv = {}, mockOverrides = {}) => {
    const mockPath = createMockPath(mockOverrides);
    const env = {
        ...process.env,
        PATH: `${mockPath}${path.delimiter}${process.env.PATH}`,
        PUID: "1000",
        PGID: "1000",
        USER_NAME: "qingyu",
        GROUP_NAME: "qingyu",
        ...extraEnv,
    };
    if (!("QINGYU_WORKSPACE_PATH" in extraEnv)) {
        delete env.QINGYU_WORKSPACE_PATH;
    }
    const result = spawnSync("sh", [entrypointPath, ...args], {encoding: "utf8", env});
    fs.rmSync(mockPath, {recursive: true, force: true});
    return result;
};

const outputRecords = (stdout) => stdout.split(/\r?\n/).filter((line) => /^(?:workspace|arg)=/.test(line));

test("Docker image exposes the QingYu kernel command and paths", () => {
    const dockerfile = fs.readFileSync(dockerfilePath, "utf8");
    assert.match(dockerfile, /go build[^\n]*-o \/kernel\/QingYu-Kernel/);
    assert.match(dockerfile, /ENV HOME=\/home\/qingyu/);
    assert.match(dockerfile, /WORKDIR \/opt\/qingyu\//);
    assert.match(dockerfile, /ENTRYPOINT \["\/opt\/qingyu\/entrypoint\.sh"\]/);
    assert.match(dockerfile, /CMD \["\/opt\/qingyu\/QingYu-Kernel", "serve"\]/);
    assert.doesNotMatch(dockerfile, /\/opt\/siyuan|\/home\/siyuan|\/kernel\/kernel\b/);
});

test("entrypoint preserves the default QingYu command arguments", () => {
    const result = runEntrypoint([
        "/opt/qingyu/QingYu-Kernel",
        "serve",
        "--port=9806",
        "--accessAuthCode=two words",
    ]);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(outputRecords(result.stdout), [
        "workspace=[/qingyu/workspace]",
        "arg=[1000:1000]",
        "arg=[/opt/qingyu/QingYu-Kernel]",
        "arg=[serve]",
        "arg=[--port=9806]",
        "arg=[--accessAuthCode=two words]",
    ]);
});

test("entrypoint preserves a workspace argument containing spaces", () => {
    const result = runEntrypoint([
        "/opt/qingyu/QingYu-Kernel",
        "serve",
        "--workspace=/tmp/QingYu Workspace",
    ]);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(outputRecords(result.stdout), [
        "workspace=[/tmp/QingYu Workspace]",
        "arg=[1000:1000]",
        "arg=[/opt/qingyu/QingYu-Kernel]",
        "arg=[serve]",
        "arg=[--workspace=/tmp/QingYu Workspace]",
    ]);
});

test("entrypoint exports the QingYu workspace environment override", () => {
    const result = runEntrypoint([
        "/opt/qingyu/QingYu-Kernel",
        "serve",
    ], {QINGYU_WORKSPACE_PATH: "/env/QingYu Workspace"});
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(outputRecords(result.stdout), [
        "workspace=[/env/QingYu Workspace]",
        "arg=[1000:1000]",
        "arg=[/opt/qingyu/QingYu-Kernel]",
        "arg=[serve]",
    ]);
});

test("entrypoint exits when runtime directory ownership preparation fails", () => {
    const result = runEntrypoint([
        "/opt/qingyu/QingYu-Kernel",
        "serve",
    ], {}, {chown: "#!/bin/sh\nexit 73\n"});
    assert.equal(result.status, 73);
    assert.deepEqual(outputRecords(result.stdout), []);
});

test("entrypoint rejects commands without the full QingYu kernel path", () => {
    const result = runEntrypoint(["serve", "--port=9806"]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /command must start with \/opt\/qingyu\/QingYu-Kernel/i);
});

test("entrypoint is valid POSIX shell syntax", () => {
    const result = spawnSync("sh", ["-n", entrypointPath], {encoding: "utf8"});
    assert.equal(result.status, 0, result.stderr);
});
