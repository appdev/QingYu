import assert = require("node:assert/strict");
import test from "node:test";
import {DocumentCardPreviewFontCache, documentCardPreviewFontSignature} from "./previewFonts";
import {queueDocumentCardPreviewRender} from "./previewRenderQueue";

test("font cache shares preparation, invalidates by identity and retries failures", async () => {
    const cache = new DocumentCardPreviewFontCache();
    let count = 0;
    const load = async () => `font-${++count}`;
    const first = cache.get("theme-a/font-a", load);
    assert.equal(cache.get("theme-a/font-a", load), first);
    assert.equal(await first, "font-1");
    assert.equal(await cache.get("theme-a/font-b", load), "font-2");
    assert.equal(await cache.get("theme-b/font-b", load), "font-3");
    await assert.rejects(cache.get("failure", async () => { throw new Error("font unavailable"); }));
    assert.equal(await cache.get("failure", load), "font-4");
    await cache.get(undefined, load);
    await cache.get(undefined, load);
    assert.equal(count, 6);
});

test("font identity tracks font source and conditional rules without including body text", async () => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM("<style>@font-face {font-family: Test;src: url(a.woff)}</style>");
    const previous = globalThis.document;
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    try {
        const before = documentCardPreviewFontSignature();
        dom.window.document.body.textContent = "unrelated content";
        assert.equal(documentCardPreviewFontSignature(), before);
        const sheet = dom.window.document.styleSheets[0];
        sheet.insertRule(sheet.cssRules[0].cssText, sheet.cssRules.length);
        assert.equal(documentCardPreviewFontSignature(), before);
        dom.window.document.querySelector("style").textContent = "@font-face {font-family: Test;src: url(b.woff)}";
        assert.notEqual(documentCardPreviewFontSignature(), before);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previous});
        dom.window.close();
    }
});

test("render queue serializes pages and drops cancelled queued work", async () => {
    let release: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const calls: number[] = [];
    const first = queueDocumentCardPreviewRender(undefined, async () => { calls.push(1); await gate; });
    const second = queueDocumentCardPreviewRender(() => false, async () => { calls.push(2); });
    const rejected = assert.rejects(second, {name: "AbortError"});
    await Promise.resolve();
    assert.deepEqual(calls, [1]);
    release();
    await first;
    await rejected;
    await queueDocumentCardPreviewRender(undefined, async () => { calls.push(3); });
    assert.deepEqual(calls, [1, 3]);
});
