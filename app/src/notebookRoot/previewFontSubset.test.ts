import assert = require("node:assert/strict");
import test from "node:test";
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {documentCardPreviewCharacters, hasSubsetCandidate, PreviewFontSubsetClient} from "./previewFontSubset";

const font = readFileSync(resolve("appearance/fonts/LxgwWenKai-Lite-1.501/LXGWWenKaiLite-Regular.ttf"));
const css = `@font-face{font-family:Test;src:url(data:font/ttf;base64,${font.toString("base64")})}`;

class FakeWorker {
    public messages: any[] = [];
    public silent = false;
    public failed = false;
    public invoke(_channel: string, message: any): Promise<string> {
        this.messages.push(message);
        if (this.failed) return Promise.reject(new Error("worker unavailable"));
        if (this.silent) return new Promise(() => {});
        return Promise.resolve(`subset:${message.text}`);
    }
}

const withWindow = async (run: (worker: FakeWorker, dom: any) => Promise<void>) => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM("<!doctype html><body></body>");
    const previous = globalThis.window;
    const worker = new FakeWorker();
    Object.defineProperty(globalThis, "window", {configurable: true, value: dom.window});
    Object.assign(dom.window, {require: (name: string) => {
        if (name === "electron") return {ipcRenderer: worker};
        throw new Error(name);
    }});
    try { await run(worker, dom); } finally {
        Object.defineProperty(globalThis, "window", {configurable: true, value: previous});
        dom.window.close();
    }
};

test("default color and small fonts do not need a worker", () => {
    assert.equal(hasSubsetCandidate(css), true);
    const color = readFileSync(resolve("appearance/fonts/Noto-COLRv1-2.047/Noto-COLRv1.woff2"));
    assert.equal(hasSubsetCandidate(`url(data:font/woff2;base64,${color.toString("base64")})`), false);
    assert.equal(hasSubsetCandidate("@font-face{src:url(test.woff2)}"), false);
});

test("worker source reuse still separates text and source changes", async () => withWindow(async (worker) => {
    const client = new PreviewFontSubsetClient();
    try {
        assert.equal(await client.subset(css, "中文"), "subset:中文");
        assert.equal(await client.subset(css, "中文"), "subset:中文");
        assert.equal(worker.messages.length, 1);
        await client.subset(css, "新增");
        assert.equal(worker.messages[1].css, css);
        await client.subset(css + "/*font source changed*/", "新增");
        assert.equal(worker.messages[2].css, css + "/*font source changed*/");
    } finally { client.stop(); }
}));

test("timeout, worker error and shutdown preserve complete fonts", async () => withWindow(async (worker, dom) => {
    const client = new PreviewFontSubsetClient();
    worker.failed = true;
    const first = client.subset(css, "AB");
    assert.equal(await first, css);
    worker.failed = false;
    worker.silent = true;
    const second = client.subset(css, "CD");
    client.stop();
    assert.equal(await second, css);
    const original = dom.window.setTimeout.bind(dom.window);
    dom.window.setTimeout = (callback: () => void, delay: number) => original(callback, delay === 5500 ? 0 : delay);
    assert.equal(await client.subset(css, "EF"), css);
    client.stop();
}));

test("browser environments preserve complete fonts", async () => withWindow(async (_worker, dom) => {
    dom.window.require = undefined;
    const client = new PreviewFontSubsetClient();
    assert.equal(await client.subset(css, "AB"), css);
    client.stop();
}));

test("character collection includes rendered and pseudo text but falls back for transformed pseudo text", async () => {
    await withWindow(async (_worker, dom) => {
        const element = dom.window.document.createElement("div");
        element.textContent = "中文é";
        element.innerText = "中文É";
        dom.window.getComputedStyle = (_node: Element, pseudo?: string) => ({
            display: "block", content: pseudo === "::before" ? '"前缀"' : "none", textTransform: "none",
        });
        const text = documentCardPreviewCharacters(element);
        assert.ok(text.includes("É") && text.includes("é") && text.includes("缀"));
        dom.window.getComputedStyle = (_node: Element, pseudo?: string) => ({
            display: "block", content: pseudo === "::before" ? '"é"' : "none", textTransform: "uppercase",
        });
        assert.equal(documentCardPreviewCharacters(element), undefined);
    });
});
