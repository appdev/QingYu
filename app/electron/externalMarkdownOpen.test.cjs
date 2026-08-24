const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");

const {
    createExternalMarkdownWindowTabs,
    createExternalMarkdownOpenCoordinator,
    extractExternalMarkdownPaths,
    redactExternalMarkdownArgs,
} = require("./externalMarkdownOpen");

test("extractExternalMarkdownPaths resolves relative files from the second-instance working directory", () => {
    const result = extractExternalMarkdownPaths([
        "/opt/qingyu/qingyu",
        "--workspace=/workspace",
        "notes/one.md",
        "qingyu://blocks/abc",
        "/tmp/two.MARKDOWN",
        '"notes/three.md"',
        "--inspect=9229",
    ], {appIsPackaged: true, defaultApp: false, workingDirectory: "/incoming"});

    assert.deepEqual(result, [
        path.resolve("/incoming/notes/one.md"),
        path.resolve("/tmp/two.MARKDOWN"),
        path.resolve("/incoming/notes/three.md"),
    ]);
    assert.deepEqual(redactExternalMarkdownArgs(["/opt/qingyu/qingyu", "/secret/note.md", "--lang=zh_CN"]), [
        "/opt/qingyu/qingyu",
        "[external-markdown]",
        "--lang=zh_CN",
    ]);
});

test("the coordinator waits for a renderer and focuses an existing global owner", async () => {
    const sent = [];
    const focused = [];
    const coordinator = createExternalMarkdownOpenCoordinator({
        async grant(filePath) {
            return {capabilityId: filePath, name: path.basename(filePath), displayPath: filePath};
        },
        findOwner(capabilityId) {
            return capabilityId.endsWith("existing.md") ? 9 : undefined;
        },
        selectWindow() {
            return 7;
        },
        focusOwner(webContentsId, capabilityId) {
            focused.push([webContentsId, capabilityId]);
        },
        send(webContentsId, payload) {
            sent.push([webContentsId, payload]);
        },
    });
    coordinator.enqueue(["/tmp/queued.md", "/tmp/existing.md"]);
    await coordinator.drain();
    assert.deepEqual(sent, []);

    coordinator.markReady(7);
    await coordinator.drain();

    assert.equal(sent[0][0], 7);
    assert.equal(sent[0][1].descriptor.capabilityId, "/tmp/queued.md");
    assert.deepEqual(focused, [[9, "/tmp/existing.md"]]);
});

test("the coordinator creates one dedicated window and queues later files until it is ready", async () => {
    const created = [];
    const sent = [];
    let dedicatedWindowExists = false;
    const coordinator = createExternalMarkdownOpenCoordinator({
        async grant(filePath) {
            return {capabilityId: filePath, name: path.basename(filePath), displayPath: filePath};
        },
        findOwner() {
            return undefined;
        },
        selectWindow(readyIds) {
            return readyIds.includes(12) ? 12 : undefined;
        },
        focusOwner() {},
        send(webContentsId, payload) {
            sent.push([webContentsId, payload]);
        },
        createWindow(payload) {
            if (dedicatedWindowExists) return false;
            dedicatedWindowExists = true;
            created.push(payload);
            return true;
        },
    });

    coordinator.enqueue(["/tmp/first.md", "/tmp/second.md"]);
    await coordinator.drain();

    assert.equal(created.length, 1);
    assert.equal(created[0].descriptor.capabilityId, "/tmp/first.md");
    assert.deepEqual(sent, []);

    coordinator.markReady(12);
    await coordinator.drain();

    assert.equal(sent.length, 1);
    assert.equal(sent[0][0], 12);
    assert.equal(sent[0][1].descriptor.capabilityId, "/tmp/second.md");

    coordinator.removeWindow(12);
    dedicatedWindowExists = false;
    coordinator.enqueue(["/tmp/third.md"]);
    await coordinator.drain();

    assert.equal(created.length, 2);
    assert.equal(created[1].descriptor.capabilityId, "/tmp/third.md");
});

test("createExternalMarkdownWindowTabs serializes one active external Markdown tab", () => {
    assert.deepEqual(createExternalMarkdownWindowTabs({
        capabilityId: "cap-1",
        name: "outside.md",
        displayPath: "/tmp/outside.md",
    }), [{
        title: "outside.md",
        icon: "iconMarkdown",
        pin: false,
        active: true,
        instance: "Tab",
        action: "Tab",
        children: {
            instance: "MarkdownEditor",
            externalCapabilityId: "cap-1",
        },
    }]);
});
