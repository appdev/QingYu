import assert = require("node:assert/strict");
import test from "node:test";

test("document card previews resolve workspace asset paths from the server root", async () => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM(`<div id="content">
        <img id="relative" src="assets/image.png" data-src="assets/image.png">
        <video id="dot-relative" poster="./assets/poster.png"></video>
        <img id="absolute" src="/assets/absolute.png">
        <img id="data" src="data:image/png;base64,AAAA">
        <img id="remote" src="https://example.com/image.png">
    </div>`);
    const {normalizeDocumentCardPreviewAssets} = await import("./previewAssets");
    const content = dom.window.document.querySelector<HTMLElement>("#content");
    normalizeDocumentCardPreviewAssets(content);
    assert.equal(content.querySelector("#relative").getAttribute("src"), "/assets/image.png");
    assert.equal(content.querySelector("#relative").getAttribute("data-src"), "/assets/image.png");
    assert.equal(content.querySelector("#dot-relative").getAttribute("poster"), "/assets/poster.png");
    assert.equal(content.querySelector("#absolute").getAttribute("src"), "/assets/absolute.png");
    assert.equal(content.querySelector("#data").getAttribute("src"), "data:image/png;base64,AAAA");
    assert.equal(content.querySelector("#remote").getAttribute("src"), "https://example.com/image.png");
    dom.window.close();
});
