import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../markraTestDom";
import {mountSiyuanMarkdownPopover} from "../siyuanMarkdownPopover";

let cleanup: () => void;

beforeEach(() => {
    cleanup = installMarkdownTestDom();
});

afterEach(() => cleanup());

test("host popovers restore focus and close exactly once", () => {
    const anchor = document.body.appendChild(document.createElement("button"));
    const input = document.createElement("input");
    anchor.focus();
    const handle = mountSiyuanMarkdownPopover({
        anchor,
        content: input,
        kind: "footnote",
        ownerDocument: document,
        position: () => undefined,
        restoreFocus: true,
    });

    assert.equal(handle.element.classList.contains("protyle-util"), true);
    handle.focus();
    assert.equal(document.activeElement, input);
    handle.destroy();
    handle.destroy();
    assert.equal(document.activeElement, anchor);
    assert.equal(handle.element.isConnected, false);
});

test("host popovers close on Escape and outside pointer interaction", () => {
    const anchor = document.body.appendChild(document.createElement("button"));
    const first = mountSiyuanMarkdownPopover({
        anchor,
        content: document.createElement("div"),
        kind: "search",
        ownerDocument: document,
        position: () => undefined,
        restoreFocus: false,
    });
    document.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape"}));
    assert.equal(first.element.isConnected, false);

    const second = mountSiyuanMarkdownPopover({
        anchor,
        content: document.createElement("div"),
        kind: "media",
        ownerDocument: document,
        position: () => undefined,
        restoreFocus: false,
    });
    document.body.dispatchEvent(new MouseEvent("pointerdown", {bubbles: true}));
    assert.equal(second.element.isConnected, false);
});
