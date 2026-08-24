const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const {ExternalMarkdownService, fileIdentity} = require("./externalMarkdownService");

const fixture = async (t, bytes = Buffer.from("# Note\n")) => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "qingyu-external-md-"));
    t.after(() => fs.rm(root, {recursive: true, force: true}));
    const file = path.join(root, "笔记.md");
    await fs.writeFile(file, bytes);
    const service = await ExternalMarkdownService.create({
        registryPath: path.join(root, "registry.json"),
        pruneDelayMs: 100,
    });
    return {root, file, service};
};

test("file identity keeps a usable inode when the platform device number is zero", () => {
    assert.equal(fileIdentity({dev: 0, ino: 42}), "0:42");
    assert.equal(fileIdentity({dev: 0, ino: 0}), undefined);
});

test("grantFromSystem accepts Markdown files and deduplicates their real paths", async (t) => {
    const {file, service} = await fixture(t);
    const first = await service.grantFromSystem(file);
    const second = await service.grantFromSystem(path.join(path.dirname(file), ".", path.basename(file)));

    assert.equal(first.capabilityId, second.capabilityId);
    assert.equal(first.name, "笔记.md");
    assert.equal(first.displayPath, await fs.realpath(file));
});

test("a persisted capability does not silently authorize a replacement file", async (t) => {
    const {root, file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);
    const replacement = path.join(root, "replacement.md");
    await fs.writeFile(replacement, "replacement\n");
    await fs.rename(replacement, file);
    const restored = await ExternalMarkdownService.create({registryPath: path.join(root, "registry.json")});

    await assert.rejects(() => restored.read(descriptor.capabilityId), {code: "FILE_IDENTITY_CHANGED"});
});

test("save and rename reject a replacement file even when its current revision is supplied", async (t) => {
    const {root, file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);
    const replacement = path.join(root, "replacement.md");
    await fs.writeFile(replacement, "replacement\n");
    await fs.rename(replacement, file);
    const replacementBytes = await fs.readFile(file);
    const stat = await fs.stat(file, {bigint: true});
    const revision = require("node:crypto").createHash("sha256")
        .update(`${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:`)
        .update(replacementBytes)
        .digest("hex");

    assert.deepEqual(await service.save(descriptor.capabilityId, {
        content: "local\n",
        revision,
        overwriteRevision: revision,
    }), {status: "error", code: "FILE_IDENTITY_CHANGED"});
    assert.deepEqual(await service.rename(descriptor.capabilityId, {name: "renamed.md", revision}), {
        status: "error",
        code: "FILE_IDENTITY_CHANGED",
    });
    assert.equal(await fs.readFile(file, "utf8"), "replacement\n");
});

test("read reports BOM and the dominant original line ending", async (t) => {
    const bytes = Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("one\r\ntwo\r\nthree\nfour\r")]);
    const {file, service} = await fixture(t, bytes);
    const descriptor = await service.grantFromSystem(file);

    const document = await service.read(descriptor.capabilityId);

    assert.equal(document.content, "one\r\ntwo\r\nthree\nfour\r");
    assert.equal(document.utf8Bom, true);
    assert.equal(document.lineEnding, "\r\n");
    assert.match(document.revision, /^[a-f0-9]{64}$/);
});

test("line ending detection defaults to LF and uses the first occurrence to break ties", async (t) => {
    const withoutLines = await fixture(t, Buffer.from("single line"));
    const firstLf = await fixture(t, Buffer.from("one\ntwo\r\nthree"));

    const withoutLinesDescriptor = await withoutLines.service.grantFromSystem(withoutLines.file);
    const firstLfDescriptor = await firstLf.service.grantFromSystem(firstLf.file);

    assert.equal((await withoutLines.service.read(withoutLinesDescriptor.capabilityId)).lineEnding, "\n");
    assert.equal((await firstLf.service.read(firstLfDescriptor.capabilityId)).lineEnding, "\n");
});

test("save preserves BOM and line endings while rejecting a stale revision", async (t) => {
    const original = Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("one\r\ntwo\r\n")]);
    const {file, service} = await fixture(t, original);
    const descriptor = await service.grantFromSystem(file);
    const loaded = await service.read(descriptor.capabilityId);
    await fs.writeFile(file, Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("outside\r\n")]));

    const conflict = await service.save(descriptor.capabilityId, {content: "local\nchange\n", revision: loaded.revision});

    assert.equal(conflict.status, "conflict");
    const overwritten = await service.save(descriptor.capabilityId, {
        content: "local\nchange\n",
        revision: loaded.revision,
        overwriteRevision: conflict.revision,
    });
    assert.equal(overwritten.status, "ok");
    assert.deepEqual(await fs.readFile(file), Buffer.concat([
        Buffer.from([0xef, 0xbb, 0xbf]),
        Buffer.from("local\r\nchange\r\n"),
    ]));
});

test("save detects a second disk change immediately before replacement", async (t) => {
    let file;
    let changed = false;
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "qingyu-external-md-"));
    t.after(() => fs.rm(root, {recursive: true, force: true}));
    file = path.join(root, "race.md");
    await fs.writeFile(file, "loaded\n");
    const service = await ExternalMarkdownService.create({
        registryPath: path.join(root, "registry.json"),
        beforeReplace: async () => {
            if (!changed) {
                changed = true;
                await fs.writeFile(file, "outside\n");
            }
        },
    });
    const descriptor = await service.grantFromSystem(file);
    const loaded = await service.read(descriptor.capabilityId);

    const result = await service.save(descriptor.capabilityId, {content: "local\n", revision: loaded.revision});

    assert.equal(result.status, "conflict");
    assert.equal(await fs.readFile(file, "utf8"), "outside\n");
});

test("grantRelativeMarkdown stays inside the authorized directory", async (t) => {
    const {root, file, service} = await fixture(t);
    const linked = path.join(root, "linked.markdown");
    await fs.writeFile(linked, "linked\n");
    const outsideRoot = await fs.mkdtemp(path.join(os.tmpdir(), "qingyu-external-outside-"));
    t.after(() => fs.rm(outsideRoot, {recursive: true, force: true}));
    const outside = path.join(outsideRoot, "outside.md");
    await fs.writeFile(outside, "outside\n");
    await fs.symlink(outside, path.join(root, "escape.md"));
    const parent = await service.grantFromSystem(file);

    const granted = await service.grantRelativeMarkdown(parent.capabilityId, "linked.markdown?view=1#part");

    assert.equal(granted.name, "linked.markdown");
    await assert.rejects(() => service.grantRelativeMarkdown(parent.capabilityId, "../outside.md"), {code: "PATH_OUTSIDE_SCOPE"});
    await assert.rejects(() => service.grantRelativeMarkdown(parent.capabilityId, "escape.md"), {code: "PATH_OUTSIDE_SCOPE"});
    await assert.rejects(() => service.grantRelativeMarkdown(parent.capabilityId, "linked%00.markdown"), {code: "INVALID_PATH"});
});

test("resolveRelativeFile permits ordinary sibling files without authorizing paths outside the directory", async (t) => {
    const {root, file, service} = await fixture(t);
    const sibling = path.join(root, "manual.pdf");
    await fs.writeFile(sibling, "pdf");
    const parent = await service.grantFromSystem(file);

    assert.equal(await service.resolveRelativeFile(parent.capabilityId, "manual.pdf#page=2"), await fs.realpath(sibling));
    await assert.rejects(() => service.resolveRelativeFile(parent.capabilityId, "../manual.pdf"), {code: "PATH_OUTSIDE_SCOPE"});
});

test("runtime owners are global and layout references keep capabilities restorable", async (t) => {
    const {file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);

    await service.retainCapability(descriptor.capabilityId, 41);
    assert.equal(service.findCapabilityOwner(descriptor.capabilityId), 41);
    await assert.rejects(() => service.retainCapability(descriptor.capabilityId, 42), {code: "CAPABILITY_IN_USE"});
    await service.setWorkspaceLayoutReferences("workspace", [descriptor.capabilityId, "missing"]);
    await service.releaseWindowCapabilities(41);

    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.equal(service.getDescriptor(descriptor.capabilityId)?.capabilityId, descriptor.capabilityId);
});

test("appearance references keep external capabilities across app restarts", async (t) => {
    const {root, file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);
    await service.setAppearanceReference(descriptor.capabilityId, true);

    const restored = await ExternalMarkdownService.create({
        registryPath: path.join(root, "registry.json"),
        pruneDelayMs: 30,
    });
    await restored.setWorkspaceLayoutReferences("workspace", []);
    await new Promise((resolve) => setTimeout(resolve, 50));

    assert.equal(restored.getDescriptor(descriptor.capabilityId)?.capabilityId, descriptor.capabilityId);
    await restored.setAppearanceReference(descriptor.capabilityId, false);
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.equal(restored.getDescriptor(descriptor.capabilityId), undefined);
});

test("resource tokens are valid only while the capability belongs to that window", async (t) => {
    const {file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);
    await service.retainCapability(descriptor.capabilityId, 41);

    const token = service.getResourceToken(descriptor.capabilityId, 41);

    assert.equal(service.verifyResourceToken(descriptor.capabilityId, token), true);
    await service.releaseCapability(descriptor.capabilityId, 41);
    assert.equal(service.verifyResourceToken(descriptor.capabilityId, token), false);
});

test("rename keeps the capability and rejects paths or existing targets", async (t) => {
    const {root, file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);
    const loaded = await service.read(descriptor.capabilityId);

    const renamed = await service.rename(descriptor.capabilityId, {name: "新名称.markdown", revision: loaded.revision});

    assert.equal(renamed.status, "ok");
    assert.equal(renamed.document.name, "新名称.markdown");
    await assert.rejects(() => fs.stat(file), {code: "ENOENT"});
    await fs.writeFile(path.join(root, "exists.md"), "exists\n");
    assert.equal((await service.rename(descriptor.capabilityId, {
        name: "exists.md",
        revision: renamed.document.revision,
    })).code, "TARGET_EXISTS");
    assert.equal((await service.rename(descriptor.capabilityId, {
        name: "../escape.md",
        revision: renamed.document.revision,
    })).code, "INVALID_NAME");
});

test("saveAssets writes validated images to a non-linked sibling assets directory", async (t) => {
    const {root, file, service} = await fixture(t);
    const descriptor = await service.grantFromSystem(file);
    const png = Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00]);

    const [saved] = await service.saveAssets(descriptor.capabilityId, [{
        name: "屏幕 截图.png",
        mimeType: "image/png",
        bytes: png,
    }]);

    assert.equal(saved.markdownDestination, "assets/屏幕-截图.png");
    assert.deepEqual(await fs.readFile(path.join(root, saved.markdownDestination)), Buffer.from(png));
    assert.equal((await service.resolveResource(descriptor.capabilityId, saved.markdownDestination)).mimeType, "image/png");
    await fs.rm(path.join(root, "assets"), {recursive: true});
    const outside = await fs.mkdtemp(path.join(os.tmpdir(), "qingyu-assets-outside-"));
    t.after(() => fs.rm(outside, {recursive: true, force: true}));
    await fs.symlink(outside, path.join(root, "assets"));
    await assert.rejects(() => service.saveAssets(descriptor.capabilityId, [{
        name: "escape.png",
        mimeType: "image/png",
        bytes: png,
    }]), {code: "UNSAFE_ASSETS_DIRECTORY"});
});
