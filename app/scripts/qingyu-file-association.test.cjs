const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const YAML = require("yaml");

const appRoot = path.resolve(__dirname, "..");
const readYaml = (name) => YAML.parse(fs.readFileSync(path.join(appRoot, name), "utf8"));

test("Darwin packages declare Markdown editor associations", () => {
    for (const name of ["electron-builder-darwin.yml", "electron-builder-darwin-arm64.yml"]) {
        const association = readYaml(name).fileAssociations?.[0];
        assert.deepEqual(association?.ext, ["md", "markdown"]);
        assert.equal(association?.mimeType, "text/markdown");
        assert.equal(association?.role, "Editor");
    }
});

test("Linux packages advertise Markdown and pass all selected files", () => {
    for (const name of ["electron-builder-linux.yml", "electron-builder-linux-arm64.yml"]) {
        const linux = readYaml(name).linux;
        assert.deepEqual(linux.mimeTypes, ["text/markdown"]);
        assert.equal(linux.desktop.entry.MimeType, "text/markdown;");
        assert.equal(linux.desktop.entry.Exec, "qingyu %F");
    }
});

test("Windows NSIS installs and removes QingYu Markdown ProgIDs in both install modes", () => {
    const source = fs.readFileSync(path.join(appRoot, "nsis/installer.nsh"), "utf8");
    assert.match(source, /WriteRegStr \$\{Root\} "Software\\Classes\\QingYu.Markdown"/);
    assert.match(source, /WriteRegStr \$\{Root\} "Software\\Classes\\.md\\OpenWithProgids"/);
    assert.match(source, /WriteRegStr \$\{Root\} "Software\\Classes\\.markdown\\OpenWithProgids"/);
    assert.match(source, /DeleteRegKey \$\{Root\} "Software\\Classes\\QingYu.Markdown"/);
    assert.match(source, /DeleteRegValue \$\{Root\} "Software\\Classes\\.md\\OpenWithProgids"/);
    assert.match(source, /DeleteRegValue \$\{Root\} "Software\\Classes\\.markdown\\OpenWithProgids"/);
    for (const root of ["HKCU", "HKLM"]) {
        assert.match(source, new RegExp(`!insertmacro RegisterMarkdownAssociation ${root}`));
        assert.match(source, new RegExp(`!insertmacro UnregisterMarkdownAssociation ${root}`));
    }
    assert.match(source, /"\$INSTDIR\\QingYu\.exe".*"%1"/);
});
