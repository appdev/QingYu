import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {scrollCodeMirrorBlockDragViewport} from "./markra-core/codemirror/block-drag";
import {installMarkdownTestDom} from "./markraTestDom";

let cleanup: () => void;
let container: HTMLElement;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    container = document.body.appendChild(document.createElement("div"));
    container.scrollTop = 100;
    container.getBoundingClientRect = () => ({
        bottom: 220,
        height: 200,
        left: 0,
        right: 400,
        top: 20,
        width: 400,
        x: 0,
        y: 20,
        toJSON: () => ({}),
    });
});

afterEach(() => cleanup());

test("scrolls the injected document container near the drag edge", () => {
    scrollCodeMirrorBlockDragViewport(container, 30);
    assert.equal(container.scrollTop, 82);

    scrollCodeMirrorBlockDragViewport(container, 215);
    assert.equal(container.scrollTop, 100);
});

test("does not scroll while the pointer is away from document edges", () => {
    scrollCodeMirrorBlockDragViewport(container, 120);
    assert.equal(container.scrollTop, 100);
});
