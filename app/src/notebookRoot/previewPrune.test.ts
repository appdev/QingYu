import assert = require("node:assert/strict");
import test from "node:test";

const bounds = (top: number, bottom: number) => ({
    x: 0,
    y: top,
    top,
    right: 640,
    bottom,
    left: 0,
    width: 640,
    height: bottom - top,
    toJSON: () => ({}),
});

test("preview pruning removes hidden sibling subtrees in batches", async () => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM(`<div id="capture"><div id="content">
        <section id="visible" data-top="0" data-bottom="1200">
            <p id="visible-child" data-top="20" data-bottom="200"></p>
            <p id="nested-hidden" data-top="1100" data-bottom="1180"></p>
            <p id="nested-hidden-tail" data-top="1180" data-bottom="1250"></p>
        </section>
        <section id="hidden" data-top="1300" data-bottom="1400"></section>
        <section id="hidden-tail" data-top="1400" data-bottom="1500"></section>
    </div></div>`);
    const previousDocument = globalThis.document;
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    try {
        const capture = dom.window.document.querySelector<HTMLElement>("#capture");
        const content = dom.window.document.querySelector<HTMLElement>("#content");
        capture.getBoundingClientRect = () => bounds(0, 960) as DOMRect;
        content.querySelectorAll<HTMLElement>("[data-top]").forEach((element) => {
            element.getBoundingClientRect = () => bounds(
                Number(element.dataset.top),
                Number(element.dataset.bottom),
            ) as DOMRect;
        });
        const {pruneDocumentCardPreviewContent} = await import("./previewPrune");
        pruneDocumentCardPreviewContent(content, capture, 0);
        assert.ok(content.querySelector("#visible"));
        assert.ok(content.querySelector("#visible-child"));
        assert.equal(content.querySelector("#nested-hidden"), null);
        assert.equal(content.querySelector("#nested-hidden-tail"), null);
        assert.equal(content.querySelector("#hidden"), null);
        assert.equal(content.querySelector("#hidden-tail"), null);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previousDocument});
        dom.window.close();
    }
});

test("preview pruning keeps the vertical overscan region", async () => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM(`<div id="capture"><div id="content">
        <p id="visible" data-top="1000" data-bottom="1040"></p>
        <p id="hidden" data-top="1130" data-bottom="1170"></p>
    </div></div>`);
    const previousDocument = globalThis.document;
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    try {
        const capture = dom.window.document.querySelector<HTMLElement>("#capture");
        const content = dom.window.document.querySelector<HTMLElement>("#content");
        capture.getBoundingClientRect = () => bounds(0, 960) as DOMRect;
        content.querySelectorAll<HTMLElement>("[data-top]").forEach((element) => {
            element.getBoundingClientRect = () => bounds(
                Number(element.dataset.top),
                Number(element.dataset.bottom),
            ) as DOMRect;
        });
        const {pruneDocumentCardPreviewContent} = await import("./previewPrune");
        pruneDocumentCardPreviewContent(content, capture);
        assert.ok(content.querySelector("#visible"));
        assert.equal(content.querySelector("#hidden"), null);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previousDocument});
        dom.window.close();
    }
});
