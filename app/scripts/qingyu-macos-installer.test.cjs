const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {spawnSync} = require("node:child_process");
const test = require("node:test");
const YAML = require("yaml");

const appRoot = path.join(__dirname, "..");
const installerPath = path.join(appRoot, "resources", "macos", "自动安装.sh");

test("macOS installer uses only the terminal-oriented .sh filename", () => {
    assert.equal(fs.existsSync(installerPath), true);
    assert.equal(fs.existsSync(path.join(appRoot, "resources", "macos", "自动安装.command")), false);
});

test("macOS installer has valid Bash syntax and is executable", () => {
    const result = spawnSync("/bin/bash", ["-n", installerPath], {encoding: "utf8"});
    assert.equal(result.status, 0, result.stderr);
    assert.notEqual(fs.statSync(installerPath).mode & 0o111, 0);
});

test("macOS installer rejects a missing source application", {skip: process.platform !== "darwin"}, (t) => {
    const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-installer-"));
    const fixtureInstaller = path.join(fixtureRoot, "自动安装.sh");
    t.after(() => fs.rmSync(fixtureRoot, {recursive: true, force: true}));
    fs.copyFileSync(installerPath, fixtureInstaller);
    fs.chmodSync(fixtureInstaller, 0o755);

    const result = spawnSync(fixtureInstaller, [], {
        encoding: "utf8",
        env: {...process.env, QINGYU_INSTALLER_NO_DIALOG: "1"},
    });

    assert.equal(result.status, 1);
    assert.match(result.stderr, /未找到与脚本同目录的 QingYu\.app/);
});

test("macOS installer rejects an invalid source application bundle", {skip: process.platform !== "darwin"}, (t) => {
    const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-installer-"));
    const fixtureInstaller = path.join(fixtureRoot, "自动安装.sh");
    t.after(() => fs.rmSync(fixtureRoot, {recursive: true, force: true}));
    fs.copyFileSync(installerPath, fixtureInstaller);
    fs.chmodSync(fixtureInstaller, 0o755);
    fs.mkdirSync(path.join(fixtureRoot, "QingYu.app"));

    const result = spawnSync(fixtureInstaller, [], {
        encoding: "utf8",
        env: {...process.env, QINGYU_INSTALLER_NO_DIALOG: "1"},
    });

    assert.equal(result.status, 1);
    assert.match(result.stderr, /QingYu\.app .*无效/);
});

test("macOS installer validates the bundle identity and executable", {skip: process.platform !== "darwin"}, (t) => {
    const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "qingyu-installer-"));
    const fixtureApp = path.join(fixtureRoot, "QingYu.app");
    const contentsPath = path.join(fixtureApp, "Contents");
    const executablePath = path.join(contentsPath, "MacOS", "QingYu");
    t.after(() => fs.rmSync(fixtureRoot, {recursive: true, force: true}));
    fs.mkdirSync(path.dirname(executablePath), {recursive: true});
    fs.writeFileSync(executablePath, "#!/bin/sh\nexit 0\n", {mode: 0o755});
    fs.writeFileSync(path.join(contentsPath, "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.apkdv.qingyu</string>
<key>CFBundleExecutable</key><string>QingYu</string>
</dict></plist>
`);

    const validate = (appPath) => spawnSync("/bin/bash", [
        "-c",
        "source \"$1\"; validate_app \"$2\"",
        "qingyu-installer-test",
        installerPath,
        appPath,
    ], {
        encoding: "utf8",
        env: {...process.env, QINGYU_INSTALLER_NO_DIALOG: "1"},
    });

    assert.equal(validate(fixtureApp).status, 0);
    fs.writeFileSync(path.join(contentsPath, "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>example.invalid</string>
<key>CFBundleExecutable</key><string>QingYu</string>
</dict></plist>
`);
    assert.notEqual(validate(fixtureApp).status, 0);

    const writeValidIdentity = (executableName) => fs.writeFileSync(path.join(contentsPath, "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.apkdv.qingyu</string>
<key>CFBundleExecutable</key><string>${executableName}</string>
</dict></plist>
`);
    writeValidIdentity(".");
    assert.notEqual(validate(fixtureApp).status, 0);
    writeValidIdentity("MissingExecutable");
    assert.notEqual(validate(fixtureApp).status, 0);
    writeValidIdentity("QingYu");
    fs.chmodSync(executablePath, 0o644);
    assert.notEqual(validate(fixtureApp).status, 0);
});

test("both macOS DMGs contain the application, Applications link, and shared installer", () => {
    const expectedContents = [
        {x: 130, y: 180},
        {x: 410, y: 180, type: "link", path: "/Applications"},
        {
            x: 270,
            y: 310,
            type: "file",
            path: "resources/macos/自动安装.sh",
            name: "自动安装.sh",
        },
    ];

    for (const configName of ["electron-builder-darwin.yml", "electron-builder-darwin-arm64.yml"]) {
        const config = YAML.parse(fs.readFileSync(path.join(appRoot, configName), "utf8"));
        assert.deepEqual(config.dmg?.contents, expectedContents, configName);
    }
});
