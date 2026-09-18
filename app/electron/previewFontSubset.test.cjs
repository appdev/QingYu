const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const path = require("node:path");
const {Worker} = require("node:worker_threads");
const {EventEmitter} = require("node:events");
const {createPreviewFontSubsetService} = require("./previewFontSubsetService.cjs");
const {canSubset, subsetCSS} = require("./previewFontSubset.cjs");
const font = fs.readFileSync(path.join(__dirname, "../appearance/fonts/LxgwWenKai-Lite-1.501/LXGWWenKaiLite-Regular.ttf"));
const color = fs.readFileSync(path.join(__dirname, "../appearance/fonts/Noto-COLRv1-2.047/Noto-COLRv1.woff2"));
const rule = (buffer) => `@font-face{font-family:"User font";src:url("data:font/ttf;base64,${buffer.toString("base64")}");font-weight:400;}`;

test("only large plain outline fonts are eligible", () => {
    assert.equal(canSubset(font), true);
    assert.equal(canSubset(color), false);
    assert.equal(canSubset(Buffer.alloc(100)), false);
    const variable = Buffer.from(font);
    variable.write("fvar", 12, "ascii");
    assert.equal(canSubset(variable), false);
});

test("subsets retain descriptors and distinguish font bytes and character sets", async () => {
    const css = rule(font);
    const first = await subsetCSS(css, "中文AB");
    assert.ok(first.length < css.length / 10);
    assert.ok(first.endsWith(";font-weight:400;}"));
    assert.equal(await subsetCSS(css, "中文AB"), first);
    assert.notEqual(await subsetCSS(css, "中文AB新增文字"), first);
    assert.equal(await subsetCSS(rule(color), "中文AB"), rule(color));
});

test("complex text and unsupported or broken fonts retain complete input", async () => {
    assert.equal(await subsetCSS(rule(font), "العربية"), rule(font));
    assert.equal(await subsetCSS(rule(font), "e\u0301"), rule(font));
    const broken = Buffer.alloc(300000);
    broken.writeUInt32BE(0x00010000, 0);
    broken.writeUInt16BE(1, 4);
    broken.write("glyf", 12);
    assert.equal(await subsetCSS(rule(broken), "AB"), rule(broken));
    await assert.rejects(subsetCSS("", "a".repeat(16385)));
});

test("worker loads packaged dependencies and recovers after an unknown source", async () => {
    const worker = new Worker(path.join(__dirname, "previewFontSubsetWorker.cjs"));
    const call = (message) => new Promise((resolve, reject) => {
        const onError = (error) => reject(error);
        worker.once("error", onError);
        worker.once("message", (data) => { worker.removeListener("error", onError); resolve(data); });
        worker.postMessage(message);
    });
    try {
        assert.deepEqual(await call({id: 1, sourceID: 7, text: "AB"}), {id: 1});
        const first = await call({id: 2, sourceID: 8, css: rule(font), text: "AB"});
        assert.ok(first.css.length < rule(font).length / 10);
        assert.equal((await call({id: 3, sourceID: 8, text: "AB"})).css, first.css);
        assert.equal((await call({id: 4, sourceID: 9, css: rule(color), text: "AB"})).css, rule(color));
    } finally {
        await worker.terminate();
    }
});

test("main-process service releases owners and falls back on timeout", async () => {
    const owner = Object.assign(new EventEmitter(), {isDestroyed: () => false});
    const timed = createPreviewFontSubsetService({timeout: 1});
    assert.equal(await timed.request(owner, {css: rule(font), text: "AB"}), undefined);
    assert.equal(owner.listenerCount("destroyed"), 0);
    const service = createPreviewFontSubsetService();
    const pending = service.request(owner, {css: rule(font), text: "AB"});
    owner.emit("destroyed");
    assert.equal(await pending, undefined);
    assert.equal(owner.listenerCount("destroyed"), 0);
    const result = await service.request(owner, {css: rule(font), text: "CD"});
    assert.ok(result.length < rule(font).length / 10);
    service.stop(owner);
});

test("main-process service bounds queued input and validates messages", async () => {
    const owner = Object.assign(new EventEmitter(), {isDestroyed: () => false});
    const service = createPreviewFontSubsetService();
    assert.equal(await service.request(owner, {css: 1, text: "AB"}), undefined);
    const pending = Array.from({length: 4}, () => service.request(owner, {css: "", text: "AB"}));
    assert.equal(await service.request(owner, {css: "", text: "AB"}), undefined);
    service.stop(owner);
    assert.deepEqual(await Promise.all(pending), [undefined, undefined, undefined, undefined]);
});
