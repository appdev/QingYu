const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const {createExternalMarkdownWindowLifecycle} = require("./externalMarkdownWindowLifecycle");

test("cold external launch hides startup windows and exits after its last external window", () => {
    const lifecycle = createExternalMarkdownWindowLifecycle({initialExternalRequest: true});
    assert.equal(lifecycle.isExternalOnly(), true);
    assert.equal(lifecycle.shouldShowStartupWindows(), false);
    assert.equal(lifecycle.shouldExitAfterLastExternalWindow(), true);
});

test("an external request received before ready converts a cold normal launch", () => {
    const lifecycle = createExternalMarkdownWindowLifecycle({initialExternalRequest: false});
    lifecycle.noteExternalRequestBeforeReady();
    assert.equal(lifecycle.isExternalOnly(), true);
    assert.equal(lifecycle.shouldShowStartupWindows(), false);
});

test("promotion restores the normal visible-main-window lifecycle", () => {
    const lifecycle = createExternalMarkdownWindowLifecycle({initialExternalRequest: true});
    lifecycle.promote();
    assert.equal(lifecycle.isExternalOnly(), false);
    assert.equal(lifecycle.shouldShowStartupWindows(), true);
    assert.equal(lifecycle.shouldExitAfterLastExternalWindow(), false);
});

test("a main close deferred by an external window is consumed once", () => {
    const lifecycle = createExternalMarkdownWindowLifecycle({initialExternalRequest: false});
    lifecycle.deferMainClose();
    assert.equal(lifecycle.consumeDeferredMainClose(), true);
    assert.equal(lifecycle.consumeDeferredMainClose(), false);
});

test("main process gates startup visibility and drains queued external files", () => {
    const source = fs.readFileSync(path.join(__dirname, "main.js"), "utf8");
    assert.match(source, /externalMarkdownLifecycle\.shouldShowStartupWindows\(\)/);
    assert.match(source, /externalMarkdownOpen\.drain\(\)/);
});

test("main process defers main close and promotes only through explicit activation", () => {
    const source = fs.readFileSync(path.join(__dirname, "main.js"), "utf8");
    assert.match(source, /externalMarkdownLifecycle\.deferMainClose\(\)/);
    assert.match(source, /externalMarkdownLifecycle\.consumeDeferredMainClose\(\)/);
    assert.match(source, /externalMarkdownLifecycle\.promote\(\)/);
    assert.match(source, /externalMarkdownLifecycle\.shouldExitAfterLastExternalWindow\(\)/);
});
